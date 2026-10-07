#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Run a command with no network beyond loopback (#61, F12; oac-testing section 2: the
# default tier runs "with no live provider, no API key, and no network beyond loopback").
#
#   scripts/loopback-only.sh <command> [args...]   # run it in a fresh network namespace
#   scripts/loopback-only.sh --probe               # prove the namespace: loopback connects,
#                                                  # an outside address does not
#   scripts/loopback-only.sh --select              # CI: print LOOPBACK_ONLY=<wrapper> for
#                                                  # GITHUB_ENV (empty off Linux)
#
# Linux only. The command runs in a new network namespace whose one interface is `lo`,
# brought up first, so 127.0.0.1 and ::1 work and every other address is unreachable. It
# runs as the invoking user, with PATH and HOME kept, so cargo and node find the toolchain
# and the caches the earlier (networked) steps filled. Three ways in, tried in order:
#   - already root: unshare --net directly, then drop to SUDO_UID/SUDO_GID if set;
#   - passwordless sudo (the hosted CI images): sudo unshare --net, then setpriv back to
#     the invoking uid/gid, so files the command writes stay the user's;
#   - an unprivileged user namespace (unshare --user --map-root-user --net), where the
#     kernel allows one (a WSL distro, most desktops).
# None available is an error (exit 2), never a silent run with the network up.
#
# Inside, before the command runs, every inherited file descriptor above 2 is closed, so a
# socket opened outside the namespace cannot carry traffic out. On the root and sudo paths
# setpriv also clears the supplementary groups (--clear-groups), so group membership such
# as `docker` on the hosted images gives no way to the docker socket.
#
# What a network namespace does not scope, stated plainly:
#   - Unix-domain sockets. A filesystem socket stays reachable inside: a resolver daemon's
#     socket (systemd-resolved, when nsswitch uses `resolve`) could answer DNS, and a
#     docker or other daemon socket the user can open could reach the network on the
#     command's behalf. The sudo and root paths drop the groups that usually grant that;
#     the user's own sockets remain.
#   - The user-namespace path keeps the caller's supplementary groups: an unprivileged
#     user namespace cannot drop them (setgroups is denied). It is the local-run path; CI
#     takes the sudo path.
#   - Anything outside the network stack: files, other processes, shared memory.

set -euo pipefail

# --select (CI): print the wrapper a later step prefixes its command with, as a
# GITHUB_ENV line. On Linux the sandbox is mandatory: it is tried first, and a failure
# fails the step, so a runner that cannot make a namespace never runs the tiers
# unsandboxed. Elsewhere the wrapper is empty. Keying on the kernel, not a runner image
# name, means a renamed image cannot drop the sandbox.
if [ "${1:-}" = "--select" ]; then
  if [ "$(uname -s)" = "Linux" ]; then
    bash "$0" true || { echo "loopback-only.sh: --select: cannot make a network namespace" >&2; exit 2; }
    echo "LOOPBACK_ONLY=bash scripts/loopback-only.sh"
  else
    echo "LOOPBACK_ONLY="
  fi
  exit 0
fi

if [ "$(uname -s)" != "Linux" ]; then
  echo "loopback-only.sh: Linux only (network namespaces)" >&2
  exit 2
fi
if [ "$#" -eq 0 ]; then
  echo "usage: scripts/loopback-only.sh <command> [args...] | --probe" >&2
  exit 2
fi

if [ "$1" = "--probe" ]; then
  # A loopback listener must accept; 192.0.2.1 (TEST-NET-1, never routed) and 1.1.1.1 must
  # fail at once with no route. A connection that opens, or a timeout (a route exists and
  # packets left the namespace), fails the probe. fd 99, opened here before entering, must
  # arrive closed.
  exec 99</dev/null
  probe='
    const net = require("node:net");
    const fs = require("node:fs");
    try { fs.fstatSync(99); console.error("probe failed: inherited fd 99 is open"); process.exit(1); }
    catch (e) { if (e.code !== "EBADF") throw e; }
    const outside = (host) => new Promise((ok, bad) => {
      const c = net.connect({ host, port: 443 });
      c.setTimeout(5000);
      c.on("connect", () => { c.destroy(); bad(new Error(`reached ${host}:443`)); });
      c.on("timeout", () => { c.destroy(); bad(new Error(`${host}:443 timed out: a route exists`)); });
      c.on("error", (e) => ok(`${host}:443 ${e.code}`));
    });
    const s = net.createServer((sock) => sock.end()).listen(0, "127.0.0.1", async () => {
      try {
        await new Promise((ok, bad) => net.connect(s.address().port, "127.0.0.1", ok).on("error", bad));
        const r = [await outside("192.0.2.1"), await outside("1.1.1.1")];
        console.log(`loopback connects; ${r.join("; ")}; inherited fds closed; groups [${process.getgroups().join(",")}]`);
        s.close();
      } catch (e) { console.error(`probe failed: ${e.message}`); process.exit(1); }
    });'
  set -- node -e "$probe"
fi

# Inside the namespace, as (namespace) root: bring lo up, close every inherited fd above 2,
# then exec the command as uid/gid. `groups` is `clear` (drop supplementary groups) or
# `keep` (the user-namespace path, which cannot drop them).
inner='
  set -eu
  u=$1 g=$2 groups=$3 p=$4 h=$5; shift 5
  ip link set lo up 2>/dev/null || { echo "loopback-only.sh: cannot bring lo up" >&2; exit 2; }
  for fd in $(ls /proc/$$/fd); do
    if [ "$fd" -gt 2 ]; then eval "exec $fd>&-" 2>/dev/null || true; fi
  done
  if [ "$groups" = keep ]; then exec env PATH="$p" HOME="$h" "$@"; fi
  exec setpriv --reuid="$u" --regid="$g" --clear-groups env PATH="$p" HOME="$h" "$@"'

if [ "$(id -u)" = 0 ]; then
  exec unshare --net -- bash -c "$inner" _ "${SUDO_UID:-0}" "${SUDO_GID:-0}" clear "$PATH" "$HOME" "$@"
elif sudo -n true 2>/dev/null; then
  exec sudo --preserve-env unshare --net -- bash -c "$inner" _ "$(id -u)" "$(id -g)" clear "$PATH" "$HOME" "$@"
elif unshare --user --map-root-user --net true 2>/dev/null; then
  exec unshare --user --map-root-user --net -- bash -c "$inner" _ 0 0 keep "$PATH" "$HOME" "$@"
fi
echo "loopback-only.sh: no way to make a network namespace (not root, no passwordless sudo, no unprivileged user namespaces)" >&2
exit 2

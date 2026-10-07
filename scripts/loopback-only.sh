#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Run a command with no network beyond loopback (#61, F12; oac-testing section 2: the
# default tier runs "with no live provider, no API key, and no network beyond loopback").
#
#   scripts/loopback-only.sh <command> [args...]   # run it in a fresh network namespace
#   scripts/loopback-only.sh --probe               # prove the namespace: loopback connects,
#                                                  # an outside address does not
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

set -euo pipefail

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
  # packets left the namespace), fails the probe.
  probe='
    const net = require("node:net");
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
        console.log(`loopback connects; ${r.join("; ")}`);
        s.close();
      } catch (e) { console.error(`probe failed: ${e.message}`); process.exit(1); }
    });'
  set -- node -e "$probe"
fi

# Inside the namespace, as root: bring lo up, then exec the command as uid/gid.
inner='
  set -eu
  u=$1 g=$2 p=$3 h=$4; shift 4
  ip link set lo up 2>/dev/null || { echo "loopback-only.sh: cannot bring lo up" >&2; exit 2; }
  if [ "$u" = 0 ]; then exec env PATH="$p" HOME="$h" "$@"; fi
  exec setpriv --reuid="$u" --regid="$g" --init-groups env PATH="$p" HOME="$h" "$@"'

if [ "$(id -u)" = 0 ]; then
  exec unshare --net -- bash -c "$inner" _ "${SUDO_UID:-0}" "${SUDO_GID:-0}" "$PATH" "$HOME" "$@"
elif sudo -n true 2>/dev/null; then
  exec sudo --preserve-env unshare --net -- bash -c "$inner" _ "$(id -u)" "$(id -g)" "$PATH" "$HOME" "$@"
elif unshare --user --map-root-user --net true 2>/dev/null; then
  exec unshare --user --map-root-user --net -- bash -c "$inner" _ 0 0 "$PATH" "$HOME" "$@"
fi
echo "loopback-only.sh: no way to make a network namespace (not root, no passwordless sudo, no unprivileged user namespaces)" >&2
exit 2

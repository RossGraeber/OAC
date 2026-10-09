#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# The bash bodies of scripts/local-ci.mjs (no GitHub-hosted CI since 2026-10-08; the lead's
# decision: "Anything that has an associated cost on the github side needs to go"). Run one
# section per call, always through local-ci.mjs, which copies this file to a temporary file
# with any CR dropped (a Windows checkout may hold it with CRLF line endings) and runs it,
# never from stdin, so a child that reads stdin cannot consume the rest of the script:
#
#   bash <copy of this file> <section>   (cwd: the repository root)
#
# Why under .github/: checks 3 and 8 search the tree with rg, which skips hidden paths, as
# it skipped .github/workflows/boundary-lint.yml when these commands lived there. Kept in
# a hidden directory, the patterns below do not match themselves; anywhere else check 8's
# own pattern would be a hit. It is not a workflow (GitHub runs nothing from it).
#
# Sections:
#   boundary-check-3, boundary-check-8, boundary-checks-1-2, boundary-check-11
#       The `rg` steps of the deleted .github/workflows/boundary-lint.yml, job
#       boundary-lint, copied verbatim (each `run:` block, de-indented). The source of
#       truth for every command is .claude/skills/oac-boundaries/references/mechanical-checks.md
#       and, for the zero-hits group, .claude/skills/oac-spec-authoring/references/
#       neutral-vocabulary-check.md: if a command changes there, change it here too.
#       rg exit 1 = zero hits (pass); exit 0 = hits (fail); exit 2 = error (fail).
#       --no-ignore: a committed .ignore/.rgignore/.gitignore must not switch a check off.
#   g3-macos
#       The steps of the deleted .github/workflows/g3-macos-hosted.yml (issue #219), with
#       $RUNNER_TEMP replaced by $G3_WORK (set by local-ci.mjs) and the artifact upload
#       replaced by leaving $G3_WORK/g3-out in place. macOS only.
#
# local-ci.mjs runs each section as `bash --noprofile --norc -eo pipefail <file> <section>`,
# the shell options GitHub Actions used for a `run:` step. For g3-macos that is stricter than
# the deleted workflow, whose steps ran as `bash -e` without pipefail (`set -eu`): there a
# verify.py failure piped into `tee` passed; here it fails the tier.

section="${1:-}"
shift || true

case "$section" in
boundary-check-3)
  # Check 3 - no provider SDK imports or deps in the code tree
  set +e
  rg -n --no-ignore --glob '!target' --glob '!docs/**' -i \
    '\bopenai\b|\banthropic\b|@anthropic-ai|from openai|import openai' \
    --glob '*.rs' --glob '*.py' --glob '*.ts' --glob '*.toml' \
    --glob '*.js' --glob '*.mjs' --glob '*.go' --glob '*.json' .
  status=$?
  [ "$status" -eq 1 ] && { echo "check 3 clean"; exit 0; }
  echo "check 3 failed (rg exit $status): stop and cite the boundary (oac-boundaries)"
  exit 1
  ;;

boundary-check-8)
  # Check 8 - no separately administered server for local use
  set +e
  rg -n --no-ignore -i --glob '!docs/**' 'dockerfile|docker-compose|kubernetes|helm|zenohd' .
  status=$?
  [ "$status" -eq 1 ] && { echo "check 8 clean"; exit 0; }
  echo "check 8 failed (rg exit $status): stop and cite the boundary (oac-boundaries)"
  exit 1
  ;;

boundary-checks-1-2)
  # Checks 1-2 over spec/ (mandatory: missing fails) and core/ (once it exists), then the
  # spec neutral-vocabulary zero-hits group over spec/. The files are listed explicitly
  # with find (every regular file, dot-files and ignored files included), so no ignore
  # file, glob or directory name can drop one. Check 2 and the zero-hits group exempt
  # exactly one regular file, spec/bindings/mcp.md (the task E6 binding document); check 1
  # exempts nothing. A symlink under spec/, or a non-regular spec/bindings/mcp.md, fails.
  set +e
  binding=spec/bindings/mcp.md
  if [ ! -d spec ] || [ -L spec ]; then echo "checks 1-2 failed: spec/ is missing or not a directory (mandatory since #41)"; exit 1; fi
  if [ ! -f spec/session-channels.md ] || [ -L spec/session-channels.md ]; then echo "checks 1-2 failed: spec/session-channels.md is missing or not a regular file"; exit 1; fi
  if { [ -e "$binding" ] || [ -L "$binding" ]; } && { [ -L "$binding" ] || [ ! -f "$binding" ]; }; then
    echo "checks 1-2 failed: $binding exists but is not a regular file"; exit 1
  fi
  links=$(find spec -type l)
  if [ -n "$links" ]; then echo "checks 1-2 failed: symlink under spec/:"; echo "$links"; exit 1; fi
  mapfile -d '' spec_all < <(find spec -type f -print0)
  spec_neutral=()
  for f in "${spec_all[@]}"; do [ "$f" = "$binding" ] || spec_neutral+=("$f"); done
  core_all=()
  [ -d core ] && mapfile -d '' core_all < <(find core -type f -not -path '*/target/*' -print0)
  fail=0
  rg -n --hidden --no-ignore -i '\bzenoh\b|\bzid\b|key[_-]?expr|liveliness' -- "${spec_all[@]}" "${core_all[@]}"
  s=$?; [ "$s" -eq 1 ] || { echo "check 1 failed (rg exit $s)"; fail=1; }
  rg -n --hidden --no-ignore \
    'claude/channel|thread/queue/add|turn/steer|turn/start|thread/start|thread/resume|notifications/claude/channel' \
    -- "${spec_neutral[@]}" "${core_all[@]}"
  s=$?; [ "$s" -eq 1 ] || { echo "check 2 failed (rg exit $s)"; fail=1; }
  rg -n --hidden --no-ignore -i \
    '\bzenoh\b|\bzid\b|key[_-]?expr|liveliness|scouting|\bmqtt\b|\bnats\b|\bclaude\b|\bcodex\b|app[ -]server|claude/channel|--channels|--dangerously-load-development-channels|thread/(queue/add|start|resume|loaded/list)|turn/(steer|start)|tools/call|prompts/get|resources/read|notifications/[a-z]+/' \
    -- "${spec_neutral[@]}"
  s=$?; [ "$s" -eq 1 ] || { echo "spec neutral-vocabulary check failed (rg exit $s)"; fail=1; }
  [ "$fail" -eq 0 ] && { echo "checks 1-2 and spec neutral vocabulary clean (${#spec_all[@]} spec/ files, ${#core_all[@]} core/ files)"; exit 0; }
  echo "stop and cite the boundary (oac-boundaries, oac-spec-authoring section 3)"
  exit 1
  ;;

boundary-check-11)
  # Scans git-tracked files only (what CI sees). No tracked files under the product
  # paths prints PENDING and exits 0 -- pending is not a pass. Hits in a tracked path
  # name or in file content fail.
  set +e
  pattern='\bbeacon\b|beacon_|beacon-managed|agent-beacon|asymptote-labs|memory\.db|get_memory_context|search_memory|get_memory\b'
  mapfile -d '' files < <(git ls-files -z -- adapters core cli transports spec Cargo.toml Cargo.lock)
  if [ "${#files[@]}" -eq 0 ]; then
    echo "check 11 PENDING: no tracked files under adapters/ core/ cli/ transports/ spec/ or root Cargo manifests"
    exit 0
  fi
  printf '%s\n' "${files[@]}" | rg -n -i -e "$pattern"
  pstatus=$?
  rg -H -n -i -e "$pattern" -- "${files[@]}"
  status=$?
  [ "$pstatus" -eq 1 ] && [ "$status" -eq 1 ] && { echo "check 11 clean (${#files[@]} tracked files)"; exit 0; }
  echo "check 11 failed (rg exit $pstatus/$status): stop and cite the boundary (oac-boundaries)"
  exit 1
  ;;

g3-macos)
  # G3 (Zenoh local peer) Mac leg (issue #219), formerly on a GitHub-hosted macos-latest
  # runner. It runs the quarantined G3 spike scripts in docs/planning/gates/fixtures/
  # g3-zenoh-peer/ unchanged (copied out under their working names, since run_matrix.py
  # spawns peer.py from its own directory) with the PyPI binding eclipse-zenoh==1.10.1
  # (docs/planning/PINS.md Zenoh row; G3-result.md "Same-core evidence"). Results land in
  # $G3_WORK/g3-out; record them in docs/planning/gates/G3-result.md by hand. The
  # throwaway test CA and peer key are generated in $G3_WORK/g3-certs, outside g3-out.
  # Networked: pip fetches eclipse-zenoh from PyPI.
  if [ "$(uname -s)" != "Darwin" ]; then echo "g3-macos: macOS only (sw_vers, lo0, sysctl)"; exit 3; fi
  : "${G3_WORK:?G3_WORK must be set (local-ci.mjs sets it)}"
  RUNNER_TEMP="$G3_WORK"

  echo "--- Record runner, OS and Python versions"
  OUT="$RUNNER_TEMP/g3-out"; mkdir -p "$OUT"
  {
    echo "date_utc=$(date -u +%FT%TZ)"
    echo "host=local (not a GitHub-hosted runner)"
    echo "--- sw_vers"; sw_vers
    echo "--- uname -a"; uname -a
    echo "--- sysctl hw.model machdep.cpu.brand_string"
    sysctl -n hw.model || true
    sysctl -n machdep.cpu.brand_string || true
    echo "--- python3"; command -v python3; python3 --version
    echo "--- openssl"; openssl version
    echo "--- ifconfig lo0"; ifconfig lo0
  } 2>&1 | tee "$OUT/environment.txt"

  echo "--- Install eclipse-zenoh==1.10.1 into a venv"
  python3 -m venv "$RUNNER_TEMP/venv"
  "$RUNNER_TEMP/venv/bin/python" -m pip install --disable-pip-version-check -q eclipse-zenoh==1.10.1
  {
    echo "--- venv python"; "$RUNNER_TEMP/venv/bin/python" --version
    echo "--- pip show eclipse-zenoh"; "$RUNNER_TEMP/venv/bin/python" -m pip show eclipse-zenoh
    echo "--- installed wheel tag"
    "$RUNNER_TEMP/venv/bin/python" -c "import importlib.metadata as m; d=m.distribution('eclipse-zenoh'); print(d.version); print(d.read_text('WHEEL'))"
  } 2>&1 | tee -a "$RUNNER_TEMP/g3-out/environment.txt"

  echo "--- Stage the quarantined spike scripts unchanged"
  SRC=docs/planning/gates/fixtures/g3-zenoh-peer
  W="$RUNNER_TEMP/g3"; mkdir -p "$W"
  for f in peer run_matrix extras verify; do
    cp "$SRC/$f.py.throwaway-quarantined" "$W/$f.py"
  done
  shasum -a 256 "$SRC"/*.py.throwaway-quarantined "$W"/*.py | tee "$RUNNER_TEMP/g3-out/scripts-sha256.txt"

  echo "--- Generate a throwaway test CA and peer certificate (kept out of g3-out)"
  C="$RUNNER_TEMP/g3-certs"; mkdir -p "$C"
  (
    cd "$C"
    openssl req -x509 -newkey rsa:2048 -nodes -days 7 -subj "/CN=oac-g3-test-ca" \
      -keyout ca.key -out ca.pem
    openssl req -newkey rsa:2048 -nodes -subj "/CN=localhost" -keyout peer.key -out peer.csr
    printf 'subjectAltName=IP:127.0.0.1,DNS:localhost\n' > san.ext
    openssl x509 -req -in peer.csr -CA ca.pem -CAkey ca.key -CAcreateserial -days 7 \
      -extfile san.ext -out peer.pem
    openssl x509 -in peer.pem -noout -subject -ext subjectAltName -enddate \
      | tee "$RUNNER_TEMP/g3-out/peer-cert-public-summary.txt"
  )

  echo "--- Run the G3 matrix (6 scenarios x 3 repetitions)"
  status=0
  (
    cd "$RUNNER_TEMP/g3"
    "$RUNNER_TEMP/venv/bin/python" run_matrix.py "$RUNNER_TEMP/g3-certs" \
      "$RUNNER_TEMP/g3-out/results-macos" 3 2>&1 | tee "$RUNNER_TEMP/g3-out/matrix.log"
    "$RUNNER_TEMP/venv/bin/python" verify.py "$RUNNER_TEMP/g3-out/results-macos" \
      2>&1 | tee "$RUNNER_TEMP/g3-out/verify.log"
  ) || status=1

  echo "--- Extras (negative control, iface probes, scouting-socket binds)"
  (
    set +e
    cd "$RUNNER_TEMP/g3"
    PY="$RUNNER_TEMP/venv/bin/python"
    E="$RUNNER_TEMP/g3-out/results-macos-extras"
    {
      echo "=== Mac extras: iface=lo0 (valid loopback name) ==="
      "$PY" extras.py "$E" lo0
      echo "=== Mac extras: iface=bogus0 (nonexistent) ==="
      "$PY" extras.py "$E-bogus" bogus0
      echo "=== Mac scouting socket bind while 3 peers live ==="
      for n in x y z; do
        "$PY" peer.py --name $n --listen tcp/127.0.0.1:0 --mcast on --expect 2 \
          --timeout 8 --linger 4 --out "$E/bind-$n.json" &
      done
      sleep 3
      echo "--- lsof -nP -iUDP:7446"; lsof -nP -iUDP:7446 || true
      echo "--- netstat -anv -p udp | grep 7446"; netstat -anv -p udp | grep -E '[.:]7446\b' || true
      wait
      for n in x y z; do
        "$PY" -c "import json;r=json.load(open('$E/bind-$n.json'));print('$n',r['ok'],r['t_discovery_ms'])"
      done
    } 2>&1 | tee "$RUNNER_TEMP/g3-out/extras.log"
  )
  echo "g3-macos: results in $RUNNER_TEMP/g3-out (no key material there)"
  exit "$status"
  ;;

*)
  echo "local-ci.sh: unknown section '$section' (run it through node scripts/local-ci.mjs)" >&2
  exit 2
  ;;
esac

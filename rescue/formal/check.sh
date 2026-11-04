#!/bin/sh
# Model-checks RestartChannel.tla. `./check.sh` checks the design in the code;
# `./check.sh broken` also checks that two broken designs fail.
set -eu
cd "$(dirname "$0")"
jar=${TLA2TOOLS_JAR:-$HOME/.local/share/tla/tla2tools.jar}
[ -f "$jar" ] || { echo "tla2tools.jar not found; set TLA2TOOLS_JAR" >&2; exit 2; }

tlc() {
  java -XX:+UseParallelGC -cp "$jar" tlc2.TLC -workers auto -deadlock \
    -metadir "${TMPDIR:-/tmp}/smith-tlc-$$-$1" -config "$1.cfg" RestartChannel.tla
}

echo "== RestartChannel (the design in the code; must pass)"
tlc RestartChannel

[ "${1:-}" = broken ] || exit 0
# Each broken design must fail on DoneMeansRebooted, not on a parse or
# config error.
expected="Invariant DoneMeansRebooted is violated"
for variant in DoneOnDelivery CompleteOnNextPoll; do
  echo "== $variant (broken design; must fail)"
  out="${TMPDIR:-/tmp}/smith-tlc-$variant.out"
  if tlc "$variant" > "$out" 2>&1; then
    echo "UNEXPECTED PASS: $variant; the check cannot see this break" >&2
    exit 1
  fi
  if ! grep -F "$expected" "$out"; then
    echo "UNEXPECTED FAILURE: $variant did not report '$expected'; see $out" >&2
    exit 1
  fi
done

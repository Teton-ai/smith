#!/bin/sh
# Model-checks CommandDelivery.tla. `./check.sh` checks the current code;
# `./check.sh findings` also checks the known findings and expects each to fail.
set -eu
cd "$(dirname "$0")"
jar=${TLA2TOOLS_JAR:-$HOME/.local/share/tla/tla2tools.jar}
[ -f "$jar" ] || { echo "tla2tools.jar not found; set TLA2TOOLS_JAR" >&2; exit 2; }

tlc() {
  java -XX:+UseParallelGC -cp "$jar" tlc2.TLC -workers auto -deadlock \
    -metadir "${TMPDIR:-/tmp}/smith-tlc-$$-$1" -config "$1.cfg" CommandDelivery.tla
}

echo "== CommandDelivery (current code; must pass)"
tlc CommandDelivery

[ "${1:-}" = findings ] || exit 0
# Each finding must fail on its own property, not on a parse or config error.
for check in \
  "AnswerLost:Invariant NoCommandLost is violated" \
  "Restart:Invariant NoCommandLost is violated" \
  "HangingHandler:Temporal property EveryCommandAnswered was violated"; do
  finding=${check%%:*}
  expected=${check#*:}
  echo "== $finding (known finding; must fail)"
  out="${TMPDIR:-/tmp}/smith-tlc-$finding.out"
  if tlc "$finding" > "$out" 2>&1; then
    echo "UNEXPECTED PASS: $finding no longer fails; update the skill" >&2
    exit 1
  fi
  if ! grep -F "$expected" "$out"; then
    echo "UNEXPECTED FAILURE: $finding did not report '$expected'; see $out" >&2
    exit 1
  fi
done

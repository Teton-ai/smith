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
for finding in AnswerLost Restart HangingHandler; do
  echo "== $finding (known finding; must fail)"
  if tlc "$finding" > "${TMPDIR:-/tmp}/smith-tlc-$finding.out" 2>&1; then
    echo "UNEXPECTED PASS: $finding no longer fails; update the skill" >&2
    exit 1
  fi
  grep -E "violated|Deadlock reached" "${TMPDIR:-/tmp}/smith-tlc-$finding.out" | head -2
done

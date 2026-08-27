#!/usr/bin/env bash
# Browser-level checks: drives the real pdf.js extraction in headless Chromium,
# fails on any console error or off-origin request, and then hands what the
# browser produced to the core's splitter.
#
# The splitter is covered by `cargo test` against a poppler extraction. This
# covers what that cannot: the browser half, and whether the two agree. A
# difference between "what the tool reads" and "what the tests read" is exactly
# the kind of defect that hides until a collaborator's manifest disagrees.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PY="$ROOT/.venv-test/bin/python"

[ -x "$PY" ] || { echo "run: make test-ui-setup"; exit 1; }

fail=0
ran=0
for pdf in "$ROOT"/*.pdf "$ROOT"/fixtures/*.pdf; do
  [ -e "$pdf" ] || continue
  ran=1
  echo
  echo "── $(basename "$pdf") ─────────────────────────"
  "$PY" "$ROOT/testing/drive.py" "$pdf" 2>&1 | grep -vE '^127\.0\.0\.1' || fail=1
done

if [ "$ran" = 0 ]; then
  echo "no PDFs to read. Put an assignment PDF in the repository root or in fixtures/."
  exit 1
fi

echo
echo "── a whole run through the real UI ──"
"$ROOT/packaging/package-web.sh" >/dev/null
"$PY" "$ROOT/testing/run_ui.py" 2>&1 | grep -vE '^127\.0\.0\.1' || fail=1

echo
echo "── the core splits what the browser read ──"
cargo test -p workbench-core --test browser -- --nocapture 2>&1 | grep -E "^test |skipped" || fail=1

echo
[ "$fail" = 0 ] && echo "all browser checks passed" || { echo "FAILURES"; exit 1; }

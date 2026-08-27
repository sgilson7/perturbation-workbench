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

# CI has no real assignment — the lab PDFs are not ours to publish — so one is
# written from scratch. It carries a Courier block, a running footer and three
# questions, which is what the extraction actually has to cope with.
python3 "$ROOT/testing/make_fixtures.py" synthesise "$ROOT/fixtures/sample-assignment.pdf"

fail=0
ran=0
for pdf in "$ROOT"/fixtures/*.pdf "$ROOT"/*.pdf; do
  [ -e "$pdf" ] || continue
  ran=1
  echo
  echo "── $(basename "$pdf") ─────────────────────────"
  "$PY" "$ROOT/testing/drive.py" "$pdf" 2>&1 | grep -vE '^127\.0\.0\.1' || fail=1
done

[ "$ran" = 1 ] || { echo "no PDFs to read, not even the generated one"; exit 1; }

echo
echo "── a whole run through the real UI ──"
"$ROOT/packaging/package-web.sh" >/dev/null
"$PY" "$ROOT/testing/run_ui.py" 2>&1 | grep -vE '^127\.0\.0\.1' || fail=1

echo
echo "── the recipe in the README, run ──"
# The README tells a collaborator to `shasum` the files and compare against the
# manifest. That is the whole basis of the claim, so it is executed here rather
# than described: a recipe nobody runs is a recipe that stops working quietly.
if [ -f "$ROOT/testing/out/run-manifest.json" ]; then
  for f in "$ROOT"/testing/out/q*.txt; do
    [ -e "$f" ] || continue
    got=$(shasum -a 256 "$f" | cut -d" " -f1)
    if grep -q "$got" "$ROOT/testing/out/run-manifest.json"; then
      echo "  ✓ $(basename "$f") → ${got:0:16}… is in the manifest"
    else
      echo "  x $(basename "$f") → ${got:0:16}… is NOT in the manifest"; fail=1
    fi
  done
else
  echo "  x no manifest was exported"; fail=1
fi

echo
echo "── the core splits what the browser read ──"
cargo test -p workbench-core --test browser -- --nocapture 2>&1 | grep -E "^test |skipped" || fail=1

echo
[ "$fail" = 0 ] && echo "all browser checks passed" || { echo "FAILURES"; exit 1; }

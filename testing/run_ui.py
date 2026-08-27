#!/usr/bin/env python3
"""Drive a whole perturbation run through the real UI.

`cargo test` covers every decision the core makes. This covers the part it
cannot: whether the page actually reaches those decisions, and whether it
reaches them with the right arguments. A UI that renders a correct `View`
wrongly is a defect no Rust test can see.

The run performed is the protocol: baseline passes, question is perturbed, the
perturbed version fails three times, and the manifest is exported. Anything
less would not exercise the transitions.
"""
import contextlib
import hashlib
import functools
import http.server
import pathlib
import socketserver
import sys
import threading

from playwright.sync_api import expect, sync_playwright

ROOT = pathlib.Path(__file__).resolve().parent.parent
DIST = ROOT / "dist" / "web"

QUESTION = """Complete the method below so that it returns the sum of the array.

```java
public static int sum(int[] xs) {
    // your code here
}
```

State its running time in big-O and justify it."""

PERTURBED = """Complete the method below so that it returns the sum of the array,
using only the loop skeleton drawn in the figure above.

```java
public static int sum(int[] xs) {
    // your code here
}
```

State its running time in big-O and justify it against that skeleton."""


@contextlib.contextmanager
def served(directory):
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(directory))
    handler.log_message = lambda *a, **k: None
    with socketserver.TCPServer(("127.0.0.1", 0), handler) as httpd:
        threading.Thread(target=httpd.serve_forever, daemon=True).start()
        yield f"http://127.0.0.1:{httpd.server_address[1]}"
        httpd.shutdown()


def mark(page, credit):
    """Mark every chip in the grading panel at one level of the scale."""
    for chip in page.eval_on_selector_all(
        ".chiprow .level[data-credit='%s']" % credit, "els => els.map(e => e.dataset.chip)"
    ):
        page.click(f".level[data-chip='{chip}'][data-credit='{credit}']")


def stamp(page, credit, response):
    mark(page, credit)
    page.fill("#response", response)
    page.click("#stamp")
    page.wait_for_timeout(120)


def main():
    if not (DIST / "index.html").exists():
        print("run: make web")
        return 1

    problems = []
    with served(DIST) as origin, sync_playwright() as p:
        browser = p.chromium.launch()
        page = browser.new_page()
        page.on("console", lambda m: problems.append(f"console {m.type}: {m.text}")
                if m.type == "error" else None)
        page.on("pageerror", lambda e: problems.append(f"page error: {e}"))
        # A blob: URL is an object the page made and handed to itself — the
        # exported manifest, the query files. It never touches the network,
        # which is the thing being checked here.
        page.on("request", lambda r: problems.append(f"off-origin request: {r.url}")
                if not (r.url.startswith(origin) or r.url.startswith("blob:")) else None)
        page.on("dialog", lambda d: d.dismiss())

        page.goto(f"{origin}/index.html")

        # 1. The tool opens ready to work, with both ways in offered equally.
        expect(page.locator("#start")).to_be_visible()
        expect(page.locator("#wayblank")).to_be_visible()
        expect(page.locator("#waypdf")).to_be_visible()
        expect(page.locator("#app")).to_be_hidden()
        print("  ✓ opens with somewhere to start")

        # 2. Type a question and the run begins.
        page.fill("#firstq", QUESTION)
        page.click("#addfirst")
        expect(page.locator("#app")).to_be_visible()
        expect(page.locator(".qcard")).to_have_count(1)
        expect(page.locator(".qcard .tag")).to_have_text("UNTESTED")
        print("  ✓ a typed question starts the run")

        # A question with no rubric cannot be graded, and the panel says so by
        # not being there.
        if page.locator("#stamp").count():
            problems.append("the grading panel opened with an empty rubric")

        # 3. Build a rubric out of chips.
        for label, points in [("Returns the sum", 6), ("States the running time", 2),
                              ("Justifies it", 2)]:
            page.fill("#chiplabel", label)
            page.fill("#chippoints", str(points))
            page.click("#addchip button[type=submit]")
            page.wait_for_timeout(60)
        expect(page.locator(".side .editrow input[data-f=label]")).to_have_count(3)
        print("  ✓ a rubric built chip by chip, 10 points")

        # 4. Nothing can be stamped before the target is named.
        expect(page.locator("#stamp")).to_be_disabled()
        page.fill("#model", "gemini-2.5-flash")
        page.dispatch_event("#model", "change")
        page.wait_for_timeout(120)
        expect(page.locator("#stamp")).to_be_enabled()
        print("  ✓ the stamp button waits for a named target")

        # 5. Step 3-4: the baseline passes, so the question is not resistant.
        stamp(page, "100", "Here is the full solution with a correct big-O argument.")
        expect(page.locator(".qcard .tag")).to_have_text("NOT RESISTANT")
        expect(page.locator(".banner")).to_contain_text("Step 4")
        print("  ✓ a passing baseline is NOT RESISTANT")

        # The version is locked now, and the rubric with it.
        expect(page.locator("#query")).to_have_attribute("readonly", "")
        if page.locator("#addchip").count():
            problems.append("the rubric stayed editable after the first attempt")
        print("  ✓ the first attempt locks the version and freezes the rubric")

        # 6. Step 5: perturb.
        page.click(".tab[data-s=spatial]")
        page.fill("#draft", PERTURBED)
        page.wait_for_timeout(120)
        page.click("#saveversion")
        page.wait_for_timeout(150)
        expect(page.locator(".tab.sel[data-v]")).to_contain_text("v1")
        print("  ✓ a perturbation saved as v1")

        # 7. Step 8: three failures.
        for i, r in enumerate(["Wrong answer one.", "Wrong answer two.", "Wrong answer three."]):
            stamp(page, "0", r)
            page.wait_for_timeout(120)
        expect(page.locator(".qcard .tag")).to_have_text("RESISTANT")
        expect(page.locator(".banner")).to_contain_text("Step 8b")
        expect(page.locator(".stamp .pct")).to_have_count(3)
        print("  ✓ three failures reach RESISTANT")

        # A fourth is not offered.
        if page.locator("#stamp").count():
            problems.append("a fourth attempt was offered")

        # 8. The manifest exports, and carries no question text. Read the file
        #    that would actually be downloaded rather than the preview, which
        #    is truncated and would hide a leak past the fold.
        page.click("#exportmanifest")
        expect(page.locator("#mtitle")).to_have_text("Run manifest")
        manifest = page.evaluate(
            "async () => (await fetch(document.getElementById('mdownload').href)).text()")
        for leak in ["sum(int", "big-O", "skeleton", "Returns the sum", "Wrong answer",
                     "Complete the method", "States the running time"]:
            if leak in manifest:
                problems.append(f"the manifest leaked {leak!r}")
        for wanted in ['"status": "resistant"', '"model": "gemini-2.5-flash"',
                       '"containsQuestionText": false', '"codeBlocks": 1']:
            if wanted not in manifest:
                problems.append(f"the manifest is missing {wanted}")
        page.click("#mclose")
        print("  ✓ the manifest exports, records the run, and carries no question text")

        # 9. The exact-query file is named by the digest the manifest records,
        #    which is the whole basis of a collaborator's `shasum` check.
        page.click("#exportqueries")
        expect(page.locator("#mtitle")).to_have_text("Exact query files")
        name = page.locator("#mbody .finding").first.inner_text().split(" ")[0]
        digest = name.removesuffix(".txt").split("-")[-1]
        query = page.evaluate(
            "async () => (await fetch(document.querySelector('#mbody a').href)).text()")
        got = hashlib.sha256(query.encode()).hexdigest()
        if not got.startswith(digest):
            problems.append(f"query file {name} hashes to {got[:8]}, not {digest}")
        if digest not in manifest:
            problems.append(f"the manifest does not carry the digest {digest}")
        page.click("#mclose")
        print(f"  ✓ the query file is named by its own digest ({digest}) and the manifest agrees")

        # Write both out so the shell can run the README's own recipe on them.
        # A recipe nobody executes is a recipe that stops working quietly.
        out = ROOT / "testing" / "out"
        out.mkdir(exist_ok=True)
        (out / name).write_text(query)
        (out / "run-manifest.json").write_text(manifest)

        # 10. The assignment: typeset, re-opened, hashed, and the manifest
        #     records the digest the file actually has.
        page.click("#exportassignment")
        expect(page.locator("#mtitle")).to_have_text("Export the assignment")
        page.select_option("#optmode", "fullHistory")
        page.fill("#opttitle", "CSC116 Lab 3")
        page.click("#optgo")
        page.wait_for_selector("#mtitle:text('Assignment')", timeout=20000)
        shown = page.locator("#mbody pre").inner_text()
        pdf = page.evaluate(
            "async () => { const r = await fetch(document.getElementById('mdownload').href);"
            " return [...new Uint8Array(await r.arrayBuffer())]; }")
        pdf = bytes(pdf)
        got = hashlib.sha256(pdf).hexdigest()
        if got not in shown:
            problems.append(f"the assignment digest shown is not the file's ({got[:8]})")
        if not pdf.startswith(b"%PDF-1.7"):
            problems.append("the assignment is not a PDF")
        for key in [b"/Info", b"/Metadata", b"/Producer", b"/Author"]:
            if key in pdf:
                problems.append(f"the assignment carried {key.decode()}")
        if pdf.count(b"%%EOF") != 1:
            problems.append("the assignment has more than one revision")
        # Uncompressed streams are what make this checkable at all.
        if b"CSC116 Lab 3" not in pdf:
            problems.append("the assignment title is not readable in the raw bytes")
        if b"Spatial Injection" not in pdf:
            problems.append("full history did not include the perturbation strategy")
        page.click("#mclose")
        print(f"  ✓ the assignment is typeset, re-read, and hashed ({got[:8]}, {len(pdf)} bytes)")

        # 11. The run survives a reload.
        page.reload()
        page.wait_for_selector(".qcard")
        expect(page.locator(".qcard .tag")).to_have_text("RESISTANT")
        expect(page.locator("#model")).to_have_value("gemini-2.5-flash")
        print("  ✓ the run survives a reload")

        # 12. And the other way in: a PDF, split without being told anything
        #     about how this particular assignment numbers its questions.
        # Prefer something that is actually an assignment. Pointed at a paper
        # the splitter finds its reference list and says so in the summary,
        # which is the right behaviour and a poor test.
        found = sorted(ROOT.glob("*.pdf")) + sorted((ROOT / "fixtures").glob("*.pdf"))
        pdfs = [p_ for p_ in found if "lab" in p_.name.lower()] or found
        if pdfs:
            page.evaluate("() => localStorage.clear()")
            page.reload()
            page.wait_for_selector("#wayblank")
            page.set_input_files("#file", str(pdfs[0]))
            page.wait_for_selector("#mtitle", timeout=30000)
            expect(page.locator("#mtitle")).to_have_text("Opened")
            opened = page.locator("#mbody").inner_text()
            page.click("#mclose")
            count = page.locator(".qcard").count()
            if count < 1:
                problems.append(f"{pdfs[0].name} produced no questions")
            print(f"  ✓ {pdfs[0].name} split into {count} question(s)")
            print("    " + " ".join(opened.split())[:150])
        else:
            print("  - no PDF to open (put one in the repository root)")

        browser.close()

    if problems:
        print()
        for p_ in problems:
            print(f"  x {p_}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

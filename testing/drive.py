#!/usr/bin/env python3
"""Drive the browser half of ingest and check it against the core's fixture.

The splitter is covered by `cargo test`. This covers the part that is not: the
pdf.js extraction that decides what a line is, which cannot be tested without a
browser and is exactly where a silent difference between "what the tool read"
and "what the tests read" would hide.

Fails on any console error and on any request that leaves the origin.
"""
import contextlib
import hashlib
import functools
import http.server
import json
import pathlib
import socketserver
import sys
import threading

from playwright.sync_api import sync_playwright

ROOT = pathlib.Path(__file__).resolve().parent.parent


@contextlib.contextmanager
def served():
    """Serve the repo over HTTP.

    Chromium refuses to load ES modules from a `file://` origin, and the app is
    hand-written modules with no bundler, so there is nothing to test without a
    server. Port 0 lets the OS pick, so two runs cannot collide.
    """
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(ROOT))
    handler.log_message = lambda *a, **k: None
    with socketserver.TCPServer(("127.0.0.1", 0), handler) as httpd:
        threading.Thread(target=httpd.serve_forever, daemon=True).start()
        yield f"http://127.0.0.1:{httpd.server_address[1]}"
        httpd.shutdown()


def main(pdf_path):
    pdf = pathlib.Path(pdf_path)
    if not pdf.exists():
        print(f"skipped: no PDF at {pdf}")
        return 0

    problems = []
    with served() as origin, sync_playwright() as p:
        browser = p.chromium.launch()
        page = browser.new_page()
        page.on("console", lambda m: problems.append(f"console {m.type}: {m.text}")
                if m.type == "error" else None)
        page.on("pageerror", lambda e: problems.append(f"page error: {e}"))
        # The claim is that nothing leaves the machine. A test that does not
        # check it is a test that would not notice when it stopped being true.
        page.on("request", lambda r: problems.append(f"off-origin request: {r.url}")
                if not r.url.startswith(origin) else None)

        page.goto(f"{origin}/testing/harness.html")
        page.wait_for_function("window.ready === true")

        got = page.evaluate("bytes => window.readPdf(bytes)", list(pdf.read_bytes()))
        browser.close()

    if problems:
        for p_ in problems:
            print(f"  x {p_}")
        return 1

    out = ROOT / "testing" / "out"
    out.mkdir(exist_ok=True)
    # Named after the source so the core can assert against a particular
    # document rather than against whichever one happened to be read last.
    (out / f"{pdf.stem}.lines.json").write_text(json.dumps(got["lines"], indent=1))

    # The digest is the run's `input` field, and the README's recipe is
    # `shasum -a 256`. Checking it against hashlib here is checking the recipe.
    want = hashlib.sha256(pdf.read_bytes()).hexdigest()
    if got["sha256"] != want:
        print(f"  x sha256 {got['sha256'][:16]}... != {want[:16]}...")
        return 1

    print(f"  pages: {got['pages']}")
    print(f"  lines: {len(got['lines'])}")
    print(f"  sha256: {got['sha256'][:16]}...")
    print(f"  monospaced lines: {sum(1 for l in got['lines'] if l['mono'])}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else ""))

#!/usr/bin/env python3
"""Extract an assignment PDF into the line list `ingest` consumes.

The browser does this with pdf.js. This is the same job done with poppler so
that `cargo test` can run the splitter against a real document without a
browser, and it is a *proxy*: pdf.js groups lines slightly differently, which is
the point of also having synthetic fixtures shaped after this one.

Output is gitignored. The lab PDFs are not ours to publish, so CI runs the
synthetic fixtures and this test skips when the file is absent.

    python3 testing/make_fixtures.py "Lab3-Part2 (3).pdf" fixtures/lab3_part2.lines.json
"""
import json
import subprocess
import sys


def lines(pdf):
    raw = subprocess.run(
        ["pdftotext", pdf, "-"], capture_output=True, check=True, text=True
    ).stdout
    out = []
    for page_no, page in enumerate(raw.split("\f"), start=1):
        for text in page.split("\n"):
            if text.strip():
                # Soft hyphens are a line-breaking artefact, not content.
                out.append({"text": text.replace("­", ""), "page": page_no, "mono": False})
    return out


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    src, dst = sys.argv[1], sys.argv[2]
    got = lines(src)
    with open(dst, "w") as f:
        json.dump(got, f, indent=1)
    print(f"{dst}: {len(got)} lines, {got[-1]['page']} pages")

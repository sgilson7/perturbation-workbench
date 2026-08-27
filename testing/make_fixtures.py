#!/usr/bin/env python3
"""Fixtures for the browser tests.

Two jobs.

`extract` turns an assignment PDF into the line list `ingest` consumes, so
`cargo test` can run the splitter against a real document without a browser. It
is a proxy for what pdf.js produces — poppler groups lines slightly differently
— which is why there are synthetic fixtures too.

`synthesise` writes a small assignment PDF from scratch, because CI has no real
one: the lab PDFs are not ours to publish, and a browser test with nothing to
read proves nothing. The document is hand-written PDF with no dependencies, and
it is deliberately shaped like an assignment rather than like a minimal file —
two pages, `Question N.` headings, `Part N` markers, a running footer whose page
number changes, and a block set in Courier. Each of those is something the
splitter has to handle, and the Courier block is the only exercise the
monospaced-run detection gets outside somebody's laptop.

    python3 testing/make_fixtures.py extract Lab3-Part2.pdf fixtures/lab3.lines.json
    python3 testing/make_fixtures.py synthesise fixtures/sample-assignment.pdf
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


# (font, size, x, y, text). F1 Helvetica, F2 Helvetica-Bold, F3 Courier.
PAGES = [
    [
        ("F2", 15, 60, 720, "CSC 999: Programming Fundamentals"),
        ("F1", 11, 60, 690, "Question 1. Compute the sum of the first ten perfect squares."),
        ("F1", 10, 60, 672, "Part 1 Show your working in full."),
        ("F1", 10, 60, 656, "Part 2 State the closed form you used."),
        ("F1", 11, 60, 620, "Question 2. Complete the method below so that it returns the sum."),
        ("F3", 9, 70, 596, "public static int sum(int[] xs) {"),
        ("F3", 9, 70, 584, "    int total = 0;"),
        ("F3", 9, 70, 572, "    for (int x : xs) {"),
        ("F3", 9, 70, 560, "        total += x;"),
        ("F3", 9, 70, 548, "    }"),
        ("F3", 9, 70, 536, "    return total;"),
        ("F3", 9, 70, 524, "}"),
        ("F1", 10, 60, 500, "Part 1 State its running time in big-O."),
        ("F1", 8, 60, 60, "© CSC 999 Faculty"),
        ("F1", 8, 480, 60, "Question 2:-1"),
    ],
    [
        ("F2", 15, 60, 720, "CSC 999: Programming Fundamentals"),
        ("F1", 11, 60, 690, "Question 3. Prove that the sum of two even integers is even."),
        ("F1", 10, 60, 672, "Part 1 State the definition of even you are using."),
        ("F1", 10, 60, 656, "Part 2 Give the proof."),
        ("F1", 8, 60, 60, "© CSC 999 Faculty"),
        ("F1", 8, 480, 60, "Question 3:-1"),
    ],
]

FACES = ["Helvetica", "Helvetica-Bold", "Courier"]


def synthesise(path):
    def esc(s):
        return s.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")

    streams = []
    for page in PAGES:
        body = ["BT"]
        for font, size, x, y, text in page:
            body += [f"/{font} {size} Tf", f"1 0 0 1 {x} {y} Tm", f"({esc(text)}) Tj"]
        body.append("ET")
        streams.append("\n".join(body).encode("latin-1", "replace"))

    n = len(PAGES)
    page_ids = [3 + 2 * i for i in range(n)]
    content_ids = [4 + 2 * i for i in range(n)]
    font_ids = [3 + 2 * n + i for i in range(len(FACES))]
    res = "/Font<<" + "".join(f"/F{i + 1} {fid} 0 R" for i, fid in enumerate(font_ids)) + ">>"

    objs = [
        b"<</Type/Catalog/Pages 2 0 R>>",
        ("<</Type/Pages/Count %d/Kids[%s]>>"
         % (n, " ".join(f"{i} 0 R" for i in page_ids))).encode(),
    ]
    for i in range(n):
        objs.append(
            (f"<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]"
             f"/Resources<<{res}>>/Contents {content_ids[i]} 0 R>>").encode()
        )
        objs.append(b"<</Length %d>>\nstream\n" % len(streams[i]) + streams[i] + b"\nendstream")
    for face in FACES:
        objs.append(
            f"<</Type/Font/Subtype/Type1/BaseFont/{face}/Encoding/WinAnsiEncoding>>".encode()
        )

    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for i, body in enumerate(objs, start=1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % i + body + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
    for off in offsets:
        out += b"%010d 00000 n \n" % off
    out += b"trailer\n<</Size %d/Root 1 0 R>>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, xref)

    with open(path, "wb") as f:
        f.write(bytes(out))
    print(f"{path}: {n} pages, {len(out)} bytes")


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "extract":
        got = lines(sys.argv[2])
        with open(sys.argv[3], "w") as f:
            json.dump(got, f, indent=1)
        print(f"{sys.argv[3]}: {len(got)} lines, {got[-1]['page']} pages")
    elif len(sys.argv) == 3 and sys.argv[1] == "synthesise":
        synthesise(sys.argv[2])
    else:
        sys.exit(__doc__)

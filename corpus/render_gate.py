#!/usr/bin/env python3
"""CI gate 2: what a corpus deck *looks like*, before and after the round trip
(`kb/61-phase-15-plan.md`, increment 15-J).

    python3 corpus/render_gate.py                  # the whole pptx corpus
    python3 corpus/render_gate.py basic-slides     # one deck, by name
    python3 corpus/render_gate.py --self-test      # the comparator alone, no renderer
    python3 corpus/render_gate.py --require-renderer   # CI: a missing renderer fails

Headless LibreOffice renders each deck to PDF, `pdftoppm` rasterises every page
to greyscale, and a low-frequency perceptual diff scores each page pair. A score
over the threshold fails the build and names the deck, the page and the number.

## What is compared, and why not simply "the round trip"

Both writers preserve the package: for every deck in `corpus/pptx` (15-H) and
every one in `corpus/odp` (18-D) the round trip is **byte for byte** the deck
that was read. Rendering both sides of
a byte-identical pair measures nothing at all, so this gate does not pretend to:
when the two packages have the same bytes it says so and counts the deck under
"by bytes", which is a strictly stronger statement than any render diff, and
spends no renderer time on it. The moment the writer produces different bytes —
an edit, a new increment, Phase 16 — the pair is rendered and scored.

The path that *does* produce new bytes today is generation: the same DocMark
with its `skeleton:` line removed, so the whole package is written on the
embedded template (15-G). Its render cannot be scored against the original — a
different master and theme legitimately look different — so what is asserted
there is what a render can honestly say about it: the deck still has the same
number of pages, and no page came out **blank** that had ink before. A slide
that silently renders empty is precisely the failure schema validation cannot
see. Its score is printed for the record, never used as a verdict.

## What this gate is not

LibreOffice is **not the oracle** for this project. Spike P3 §2.4 measured it
opening a deck with five dangling relationships without a murmur; risk P2 closes
with the PowerPoint checklist of 15-K, run by a human, and with nothing else.
This gate catches gross visual regressions — a slide gone blank, text landing in
the wrong shape, a picture lost, a page dropped. It does not catch a subtly
wrong font size, and it never says "PowerPoint opens this".

## Requirements

`soffice` (LibreOffice) and `pdftoppm` (poppler-utils), plus a built CLI
(`cargo build -p docsai-cli`). A renderer that is missing — or installed and
broken, which is the same thing to a gate — is a **skip**: exit 3, with a banner
saying what was therefore not measured, and never exit 0. Under
`--require-renderer`, which is what CI passes having installed LibreOffice
deliberately, it is a failure instead: there, «no renderer» means the install
step broke.
"""
import argparse
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from schema_gate import (  # noqa: E402
    CORPUS,
    ODP_CORPUS,
    find_binary,
    run,
    without_skeleton,
)

# The rasterisation resolution. 100 dpi puts a 10 x 7.5 inch slide on a
# 1000 x 750 raster: fine enough that a lost picture or a shifted paragraph is
# unmissable, coarse enough that a page costs milliseconds to compare.
DPI = 100

# The comparison is made on the mean of 8 x 8 pixel blocks — about 2 mm of
# slide. Averaging is the whole trick: anti-aliasing and hinting move ink
# *within* a block and cancel, while anything that moves ink *between* blocks
# (a shape in the wrong place, a picture gone, a different string) survives.
BLOCK = 8

# The threshold, in the units the score is printed in: 100 x RMSE over block
# means, on a 0..255 scale, so "1.0" means the average block differs by 1 % of
# full scale. Measured, not guessed — `--self-test` prints every number below,
# and the reasoning is in `kb/71-phase-15-render.md`:
#
#   same content:
#     the same PDF rasterised twice           0.0000  <- the real noise floor
#     one pixel of 750 000 changed            0.0144
#     anti-aliasing on vs off, same PDF       1.7216  <- pessimistic proxy
#     text shifted by half a point            1.8411  <- pessimistic proxy
#   different content:
#     one text line of twelve gone            3.5490  <- the smallest real one
#     text shifted by four points             8.8076
#     a different page                       32.3932
#
# Two renders of the same file by the same LibreOffice are bit-identical, so the
# operative floor within one CI run is 0. The two proxies stand in for what
# would differ if the renderer itself changed under the gate, and 2.5 sits 1.36x
# above the worst of them and 1.42x below the smallest genuine difference.
# What it blinds the gate to is stated in the KB entry: a change confined to a
# few blocks — one wrong word, one recoloured line — scores below it and passes.
THRESHOLD = 2.5

# A page darker than this on average, or with less ink than `MIN_INK`, is
# treated as blank. `MIN_INK` is a fraction of pixels below `INK_LEVEL`.
INK_LEVEL = 250
MIN_INK = 0.0002  # 150 dark pixels on a 1000 x 750 page


# --------------------------------------------------------------------------
# The raster format and the metric. Nothing here needs an image library: a PGM
# is a header and a row of bytes per row of pixels, which is exactly why the
# renderer is asked for one (see the module docstring and AGENTS.md §7 rule 4).
# --------------------------------------------------------------------------


def read_pgm(path):
    """A binary (P5) greyscale PGM as `(width, height, pixels)`."""
    blob = Path(path).read_bytes()
    fields, offset = [], 2  # past the "P5"
    while len(fields) < 3:
        while offset < len(blob) and blob[offset : offset + 1].isspace():
            offset += 1
        if blob[offset : offset + 1] == b"#":
            while offset < len(blob) and blob[offset] != 0x0A:
                offset += 1
            continue
        start = offset
        while offset < len(blob) and not blob[offset : offset + 1].isspace():
            offset += 1
        fields.append(int(blob[start:offset]))
    offset += 1  # the single whitespace byte after the maximum value
    width, height, _ = fields
    return width, height, blob[offset : offset + width * height]


def block_means(width, height, pixels):
    """The mean value of every `BLOCK` x `BLOCK` tile, row by row. Partial tiles
    at the right and bottom edge are averaged over the pixels they have."""
    means = []
    for top in range(0, height, BLOCK):
        rows = range(top, min(top + BLOCK, height))
        for left in range(0, width, BLOCK):
            right = min(left + BLOCK, width)
            total = 0
            for row in rows:
                base = row * width
                total += sum(pixels[base + left : base + right])
            means.append(total / (len(rows) * (right - left)))
    return means


def score(before, after):
    """The perceptual distance between two rasters, or `None` when they cannot
    be compared at all. Returns `(score, reason)`: exactly one is set.

    Two rasters of different size are not a score of any value — the page
    geometry itself changed — and the caller must report that as its own kind of
    failure rather than as a large number."""
    (w1, h1, p1), (w2, h2, p2) = before, after
    if (w1, h1) != (w2, h2):
        return None, f"raster geometry changed: {w1}x{h1} -> {w2}x{h2}"
    a, b = block_means(w1, h1, p1), block_means(w2, h2, p2)
    total = sum((x - y) ** 2 for x, y in zip(a, b))
    return 100.0 * (total / len(a)) ** 0.5 / 255.0, None


def ink(width, height, pixels):
    """The fraction of pixels with any ink on them. Used only to tell a page
    that renders from a page that renders *blank*."""
    table = bytes(1 if value < INK_LEVEL else 0 for value in range(256))
    return pixels.translate(table).count(1) / max(1, width * height)


# --------------------------------------------------------------------------
# The renderer
# --------------------------------------------------------------------------


class Renderer:
    """Headless LibreOffice to PDF, then poppler to greyscale rasters.

    Deliberately two steps. `soffice --convert-to png` exports a single page,
    which would render the first slide of every deck and call the rest measured;
    PDF is the only export that carries the whole deck, and rasterising it with
    `pdftoppm` fixes the resolution explicitly instead of inheriting whatever
    the exporter felt like."""

    def __init__(self, profile):
        self.profile = profile

    @staticmethod
    def unusable():
        """Why this machine cannot render, or `None`.

        A tool that is *installed but broken* has to land here too, and not
        halfway through the corpus as seventeen "not measured" notes: an
        incomplete LibreOffice — `soffice.bin: error while loading shared
        libraries` — is on `PATH`, exits 0, and prints no version. So the probe
        is what it says rather than whether it ran."""
        for tool in ("soffice", "pdftoppm"):
            if not shutil.which(tool):
                return f"{tool} is not installed"
        try:
            proc = subprocess.run(
                ["soffice", "--headless", "--version"],
                capture_output=True, text=True, check=False, timeout=120,
            )
        except (OSError, subprocess.SubprocessError) as exc:
            return f"soffice --version did not run: {exc}"
        if "LibreOffice" not in (proc.stdout or ""):
            detail = (proc.stderr or proc.stdout or "").strip().splitlines()
            return "soffice is installed but does not run" + (
                f": {detail[-1]}" if detail else ""
            )
        return None

    def pdf(self, document, outdir):
        outdir.mkdir(parents=True, exist_ok=True)
        proc = subprocess.run(
            [
                "soffice",
                "--headless",
                "--norestore",
                "--nolockcheck",
                f"-env:UserInstallation=file://{self.profile}",
                "--convert-to",
                "pdf",
                "--outdir",
                str(outdir),
                str(document),
            ],
            capture_output=True,
            text=True,
            check=False,
            timeout=300,
        )
        pdf = outdir / (Path(document).stem + ".pdf")
        if not pdf.exists():
            detail = (proc.stderr or proc.stdout or "").strip().splitlines()
            return None, "LibreOffice produced no PDF" + (
                f": {detail[-1]}" if detail else ""
            )
        return pdf, None

    @staticmethod
    def pages(pdf, outdir):
        """Every page of `pdf` as a P5 PGM, in page order."""
        outdir.mkdir(parents=True, exist_ok=True)
        subprocess.run(
            ["pdftoppm", "-gray", "-r", str(DPI), str(pdf), str(outdir / "page")],
            capture_output=True,
            check=False,
            timeout=300,
        )
        return sorted(outdir.glob("page-*.pgm"))

    def render(self, document, outdir):
        """`(pages, None)` or `(None, why)`."""
        pdf, why = self.pdf(document, outdir)
        if pdf is None:
            return None, why
        pages = self.pages(pdf, outdir)
        if not pages:
            return None, "pdftoppm rasterised no page"
        return pages, None


# --------------------------------------------------------------------------
# The comparator's own tests. They run with no renderer at all, which is the
# point: on a machine where LibreOffice is missing this is still measured.
# --------------------------------------------------------------------------


def synthetic(width, height, shift=0, ink_at=None):
    """A page of horizontal "text" bars, optionally shifted down by `shift`
    pixels; `ink_at` overrides the bar rows."""
    rows = ink_at if ink_at is not None else range(20, height - 20, 24)
    pixels = bytearray(b"\xff" * (width * height))
    for row in rows:
        for offset in range(6):
            line = row + offset + shift
            if 0 <= line < height:
                start = line * width + 40
                pixels[start : start + width - 120] = b"\x20" * (width - 120)
    return width, height, bytes(pixels)


MINIMAL_PDF = """%%PDF-1.4
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 720 540]/Resources<</Font<</F1 4 0 R>>>>/Contents 5 0 R>>endobj
4 0 obj<</Type/Font/Subtype/Type1/BaseFont/%(font)s>>endobj
5 0 obj<</Length %(length)d>>stream
%(content)sendstream
endobj
trailer<</Root 1 0 R/Size 6>>
"""


def minimal_pdf(path, font=b"Helvetica", dy=0.0, drop=()):
    """A one-page PDF of real text, so that the noise floor can be measured on
    an actual rasterisation rather than on drawn rectangles. Hand-written
    because a PDF library would be a dependency for a self-test."""
    lines = []
    for index in range(12):
        if index in drop:
            continue
        y = 480 - index * 36 + dy
        lines.append(
            b"BT /F1 24 Tf 48 %.2f Td (Phase 15 J render gate line %d) Tj ET"
            % (y, index)
        )
    content = b"\n".join(lines) + b"\n"
    body = MINIMAL_PDF.encode() % {
        b"font": font,
        b"length": len(content),
        b"content": content,
    }
    Path(path).write_bytes(body)


def self_test(workdir):
    """Measure the comparator on pairs whose answer is known, print every
    number, and check the ordering the threshold depends on."""
    width, height = 1000, 750
    base = synthetic(width, height)
    one_pixel = bytearray(base[2])
    one_pixel[height // 2 * width + width // 2] = 0
    rows = list(range(20, height - 20, 24))

    cases = [
        ("identical rasters", base, base, "under"),
        ("one pixel of 750000 changed", base, (width, height, bytes(one_pixel)), "under"),
        ("content shifted by 1 pixel", base, synthetic(width, height, shift=1), None),
        ("content shifted by 8 pixels", base, synthetic(width, height, shift=8), "over"),
        (
            "one line of twelve removed",
            base,
            synthetic(width, height, ink_at=rows[:-1]),
            "over",
        ),
        (
            "a wholly different page",
            base,
            synthetic(width, height, ink_at=range(10, height - 10, 11)),
            "over",
        ),
    ]

    # Two rasterisations of the same text, as LibreOffice would produce twice.
    # This is the pair that sets the noise floor, and it needs a real renderer
    # for the text — `pdftoppm` alone, not LibreOffice.
    if shutil.which("pdftoppm"):
        workdir.mkdir(parents=True, exist_ok=True)
        variants = {}
        for name, font, dy, aa, drop in (
            ("plain", b"Helvetica", 0.0, "yes", ()),
            ("no-aa", b"Helvetica", 0.0, "no", ()),
            ("half-point", b"Helvetica", 0.5, "yes", ()),
            ("one-point", b"Helvetica", 1.0, "yes", ()),
            ("two-point", b"Helvetica", 2.0, "yes", ()),
            ("four-point", b"Helvetica", 4.0, "yes", ()),
            ("one-line-gone", b"Helvetica", 0.0, "yes", (5,)),
            ("substituted-font", b"Times-Roman", 0.0, "yes", ()),
        ):
            pdf = workdir / f"{name}.pdf"
            minimal_pdf(pdf, font=font, dy=dy, drop=drop)
            subprocess.run(
                ["pdftoppm", "-gray", "-r", str(DPI), "-aa", aa, "-aaVector", aa,
                 str(pdf), str(workdir / name)],
                capture_output=True, check=False, timeout=120,
            )
            variants[name] = read_pgm(sorted(workdir.glob(f"{name}-*.pgm"))[0])
        cases += [
            ("same PDF rendered twice", variants["plain"], variants["plain"], "under"),
            ("anti-aliasing on vs off", variants["plain"], variants["no-aa"], "under"),
            ("text shifted by half a point", variants["plain"], variants["half-point"], "under"),
            ("text shifted by one point", variants["plain"], variants["one-point"], None),
            ("text shifted by two points", variants["plain"], variants["two-point"], None),
            ("text shifted by four points", variants["plain"], variants["four-point"], "over"),
            ("one line of twelve gone", variants["plain"], variants["one-line-gone"], "over"),
            ("font substituted", variants["plain"], variants["substituted-font"], None),
        ]
    else:
        print("  (pdftoppm absent: the rasterised cases were not measured)")

    print(f"comparator self-test (block {BLOCK}px at {DPI} dpi, threshold {THRESHOLD})")
    bad = []
    for name, before, after, expected in cases:
        value, reason = score(before, after)
        verdict = "n/a" if expected is None else ("over" if value > THRESHOLD else "under")
        print(f"  {value:9.4f}  {verdict:5}  {name}")
        if expected is not None and verdict != expected:
            bad.append(f"{name}: scored {value:.4f}, expected {expected} {THRESHOLD}")

    # The one comparison that is not a score at all.
    value, reason = score(base, synthetic(width, height // 2))
    print(f"  {'-':>9}  n/a    a page of another size: {reason}")
    if value is not None or reason is None:
        bad.append("a raster of another size was scored instead of reported")

    for problem in bad:
        print(f"  FAIL  {problem}")
    return 1 if bad else 0


# --------------------------------------------------------------------------
# The gate
# --------------------------------------------------------------------------


def packages_for(binary, deck, workdir):
    """The two packages this gate looks at: the round trip of `deck`, and the
    same document written with no skeleton at all."""
    workdir.mkdir(parents=True, exist_ok=True)
    markdown = workdir / "deck.dmk.md"
    run(binary, "convert", deck, "-o", markdown)
    if not markdown.exists():
        return None, None, "the deck did not convert to DocMark"

    roundtrip = workdir / f"roundtrip{deck.suffix}"
    run(binary, "convert", markdown, "-o", roundtrip)
    if not roundtrip.exists():
        return None, None, "the DocMark did not convert back to a package"

    stripped = workdir / "no-skeleton.dmk.md"
    stripped.write_text(
        without_skeleton(markdown.read_text(encoding="utf-8")), encoding="utf-8"
    )
    generated = workdir / f"generated{deck.suffix}"
    run(binary, "convert", stripped, "-o", generated)
    if not generated.exists():
        return roundtrip, None, "the skeleton-less DocMark did not convert to a package"
    return roundtrip, generated, None


def identical(one, other):
    with zipfile.ZipFile(one) as a, zipfile.ZipFile(other) as b:
        names = a.namelist()
        return names == b.namelist() and all(a.read(n) == b.read(n) for n in names)


def draw_pages(deck):
    """How many `draw:page` elements an `.odp` declares.

    Counted from the bytes rather than parsed, because the question is not what
    the page *is* — that is 18-B's job — but how many of them a renderer owes
    us. `<draw:page ` cannot appear anywhere else in `content.xml`: the
    only other element whose name starts that way is `draw:page-thumbnail`, and
    the trailing space excludes it."""
    with zipfile.ZipFile(deck) as zf:
        return zf.read("content.xml").count(b"<draw:page ")


class Report:
    def __init__(self):
        self.by_bytes = self.by_render = self.pages = self.failures = 0
        self.unmeasured = []
        self.worst = ("", 0.0)

    def fail(self, message):
        self.failures += 1
        print(f"  FAIL  {message}")

    def note(self, deck, why):
        self.unmeasured.append(f"{deck}: {why}")
        print(f"  SKIP  {why}")


def compare_pages(report, label, before, after):
    """Score a deck's pages, page by page. Returns True when every page is
    within the threshold."""
    if len(before) != len(after):
        report.fail(
            f"{label}: the page count changed, {len(before)} -> {len(after)}. "
            f"There is no score for this: the pages cannot be paired."
        )
        return False
    ok = True
    for index, (one, other) in enumerate(zip(before, after), start=1):
        value, reason = score(read_pgm(one), read_pgm(other))
        report.pages += 1
        if value is None:
            report.fail(f"{label} page {index}: {reason}")
            ok = False
            continue
        if value > report.worst[1]:
            report.worst = (f"{label} page {index}", value)
        if value > THRESHOLD:
            report.fail(
                f"{label} page {index}: perceptual diff {value:.4f} "
                f"over the threshold {THRESHOLD}"
            )
            ok = False
        else:
            print(f"  ok    {label} page {index}: {value:.4f}")
    return ok


def check_generated(report, deck, original, generated):
    """What a render can honestly say about a package written on the embedded
    template: the same number of pages, and no page that lost all its ink. The
    score against the original is printed and is *not* a verdict — a different
    master and theme are entitled to look different."""
    if len(original) != len(generated):
        report.fail(
            f"{deck} [generated]: {len(original)} page(s) in the original, "
            f"{len(generated)} written on the template"
        )
        return
    for index, (one, other) in enumerate(zip(original, generated), start=1):
        source, written = read_pgm(one), read_pgm(other)
        report.pages += 1
        before, after = ink(*source), ink(*written)
        value, _ = score(source, written)
        shown = f"{value:.4f}" if value is not None else "n/a (geometry)"
        if before >= MIN_INK and after < MIN_INK:
            report.fail(
                f"{deck} [generated] page {index}: rendered blank "
                f"(ink {before:.4%} -> {after:.4%}); diff {shown}"
            )
        else:
            print(
                f"  ok    {deck} [generated] page {index}: ink {after:.4%}, "
                f"diff {shown} (reported, not a verdict)"
            )


def check_odp(binary, renderer, report, scratch, wanted):
    """The `.odp` corpus: the fixture rendered, and the round trip compared
    (Phase 18-A, completed by 18-D).

    **The fixture.** LibreOffice reads `.odp` natively, so what is asserted is
    the thing schema validation cannot see — that it opens every fixture, draws
    the number of pages the fixture declares, and leaves ink on each of them. A
    `draw:page` that renders blank is a fixture that isolates nothing.

    **The round trip**, which 18-A could not do because there was no writer.
    Exactly the pptx arm's assertion, on the same comparator and the same
    threshold: convert the deck to DocMark, convert it back, and compare. The
    `.odp` writer preserves the package, so the pair is byte for byte for every
    fixture in this corpus and the deck is counted under "by bytes" — a
    strictly stronger statement than any render diff, and one that costs no
    renderer time. The moment the writer produces different bytes the pair is
    rendered and scored, which is what this arm exists to catch.

    The *generated* variant the pptx arm has arrives with 18-E and is measured
    by `check_cross()` below, because the package a generated `.odp` is written
    on is the same embedded template a crossed `.pptx` lands in."""
    decks = sorted(ODP_CORPUS.glob("*.odp")) if ODP_CORPUS.exists() else []
    if wanted:
        decks = [d for d in decks if d.stem in wanted]
    for deck in decks:
        print(f"{deck.name}")
        declared = draw_pages(deck)
        pages, why = renderer.render(deck, scratch / "odp" / deck.stem)
        if pages is None:
            report.note(deck.name, f"not rendered: {why}")
        elif len(pages) != declared:
            report.fail(
                f"{deck.name}: {declared} draw:page in content.xml, "
                f"{len(pages)} page(s) rendered"
            )
        else:
            report.by_render += 1
            blank = []
            for index, page in enumerate(pages, start=1):
                report.pages += 1
                width, height, pixels = read_pgm(page)
                covered = ink(width, height, pixels)
                if covered < MIN_INK:
                    blank.append(f"page {index} (ink {covered:.4%})")
            if blank:
                report.fail(f"{deck.name}: rendered blank: {', '.join(blank)}")
            else:
                print(f"  ok    fixture: {len(pages)} page(s), all with ink")

        workdir = scratch / "odp-roundtrip" / deck.stem
        roundtrip, why = odp_roundtrip(binary, deck, workdir)
        if roundtrip is None:
            report.note(deck.name, why)
            continue
        if identical(deck, roundtrip):
            report.by_bytes += 1
            print("  ok    round trip: identical package, byte for byte "
                  "(a render can only agree)")
            continue
        before, why = renderer.render(deck, workdir / "render-before")
        after, why2 = (renderer.render(roundtrip, workdir / "render-after")
                       if before is not None else (None, why))
        if before is None or after is None:
            report.note(deck.name, f"round trip not rendered: {why or why2}")
            continue
        report.by_render += 1
        compare_pages(report, f"{deck.name} [round trip]", before, after)


def odp_roundtrip(binary, deck, workdir):
    """The deck's DocMark converted back to a `.odp`."""
    workdir.mkdir(parents=True, exist_ok=True)
    markdown = workdir / "deck.dmk.md"
    run(binary, "convert", deck, "-o", markdown)
    if not markdown.exists():
        return None, "the deck did not convert to DocMark"
    roundtrip = workdir / "roundtrip.odp"
    run(binary, "convert", markdown, "-o", roundtrip)
    if not roundtrip.exists():
        return None, "the DocMark did not convert back to a package"
    return roundtrip, None


def slide_parts(deck):
    """How many `ppt/slides/slideN.xml` a `.pptx` declares."""
    with zipfile.ZipFile(deck) as zf:
        return sum(
            1
            for name in zf.namelist()
            if name.startswith("ppt/slides/slide") and name.endswith(".xml")
        )


def check_cross(binary, renderer, report, scratch, wanted):
    """Each corpus written in the other dialect, and opened (Phase 18-E).

    A cross conversion cannot be compared against the deck it came from the way
    a round trip can: the two dialects are different packages, they resolve
    through different masters and different default fonts, and a render diff
    between them would be measuring the template rather than the conversion. So
    what this arm asserts is the same thing the fixture arm asserts, and it is
    the assertion schema validation cannot make: **LibreOffice opens the crossed
    package, draws exactly the number of pages the package declares, and leaves
    ink on each of them.** A deck that crossed into a file no application can
    open is a conversion that reported success and produced nothing."""
    decks = sorted(p for p in CORPUS.iterdir() if p.suffix in (".pptx", ".pptm"))
    odp_decks = sorted(ODP_CORPUS.glob("*.odp")) if ODP_CORPUS.exists() else []
    if wanted:
        decks = [d for d in decks if d.stem in wanted]
        odp_decks = [d for d in odp_decks if d.stem in wanted]

    def one(deck, suffix, declared_by):
        print(f"{deck.name} [crossed to {suffix}]")
        workdir = scratch / f"cross{suffix}" / deck.stem
        workdir.mkdir(parents=True, exist_ok=True)
        markdown = workdir / "deck.dmk.md"
        run(binary, "convert", deck, "-o", markdown)
        if not markdown.exists():
            report.note(deck.name, "the deck did not convert to DocMark")
            return
        crossed = workdir / f"crossed{suffix}"
        run(binary, "convert", markdown, "-o", crossed)
        if not crossed.exists() or crossed.stat().st_size == 0:
            report.note(deck.name, f"the DocMark did not convert to {suffix}")
            return
        declared = declared_by(crossed)
        pages, why = renderer.render(crossed, workdir / "render")
        if pages is None:
            report.note(deck.name, f"crossed package not rendered: {why}")
            return
        if len(pages) != declared:
            report.fail(
                f"{deck.name} [crossed to {suffix}]: the package declares {declared} "
                f"page(s), {len(pages)} rendered"
            )
            return
        report.by_render += 1
        blank = []
        for index, page in enumerate(pages, start=1):
            report.pages += 1
            width, height, pixels = read_pgm(page)
            covered = ink(width, height, pixels)
            if covered < MIN_INK:
                blank.append(f"page {index} (ink {covered:.4%})")
        if blank:
            report.fail(f"{deck.name} [crossed to {suffix}]: rendered blank: "
                        f"{', '.join(blank)}")
        else:
            print(f"  ok    crossed: {len(pages)} page(s), all with ink")

    for deck in decks:
        one(deck, ".odp", draw_pages)
    for deck in odp_decks:
        one(deck, ".pptx", slide_parts)


def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("decks", nargs="*", help="deck names to check (default: all)")
    ap.add_argument("--binary", help="path to the docsai CLI")
    ap.add_argument("--keep", help="directory to leave the renders in")
    ap.add_argument("--self-test", action="store_true",
                    help="measure the comparator on known pairs and exit")
    ap.add_argument("--require-renderer", action="store_true",
                    help="a missing renderer is a failure, not a skip (CI)")
    args = ap.parse_args()

    scratch = Path(args.keep) if args.keep else Path(tempfile.mkdtemp(prefix="render-gate-"))
    try:
        if args.self_test:
            return self_test(scratch / "self-test")

        unusable = Renderer.unusable()
        if unusable:
            print("=" * 72)
            print("RENDER GATE SKIPPED — NOTHING WAS MEASURED")
            print(f"  reason:  {unusable}")
            print("  install: sudo apt-get install -y libreoffice poppler-utils")
            print("  not measured: how any corpus deck looks, before or after the")
            print("                round trip, on this machine.")
            print("  This is NOT a pass. The comparator itself is still checked by")
            print("      python3 corpus/render_gate.py --self-test")
            print("=" * 72)
            if args.require_renderer:
                print(
                    f"render_gate: a renderer is required here and there is none "
                    f"({unusable})",
                    file=sys.stderr,
                )
                return 2
            return 3

        binary = find_binary(args.binary)
        decks = sorted(p for p in CORPUS.iterdir() if p.suffix in (".pptx", ".pptm"))
        odp_decks = sorted(ODP_CORPUS.glob("*.odp")) if ODP_CORPUS.exists() else []
        wanted = None
        if args.decks:
            wanted = {
                d.removesuffix(".pptx").removesuffix(".pptm").removesuffix(".odp")
                for d in args.decks
            }
            decks = [d for d in decks if d.stem in wanted]
            odp_decks = [d for d in odp_decks if d.stem in wanted]
            if not decks and not odp_decks:
                sys.exit(f"render_gate: no corpus deck named {', '.join(sorted(wanted))}")

        renderer = Renderer(scratch / "profile")
        report = Report()
        for deck in decks:
            print(f"{deck.name}")
            workdir = scratch / deck.stem
            roundtrip, generated, why = packages_for(binary, deck, workdir)
            if roundtrip is None:
                report.note(deck.name, why)
                continue

            if identical(deck, roundtrip):
                report.by_bytes += 1
                print("  ok    round trip: identical package, byte for byte "
                      "(a render can only agree)")
            else:
                before, why = renderer.render(deck, workdir / "render-before")
                after, why2 = (renderer.render(roundtrip, workdir / "render-after")
                               if before is not None else (None, why))
                if before is None or after is None:
                    report.note(deck.name, f"round trip not rendered: {why or why2}")
                else:
                    report.by_render += 1
                    compare_pages(report, f"{deck.name} [round trip]", before, after)

            if generated is None:
                report.note(deck.name, why or "no generated package")
                continue
            source, why = renderer.render(deck, workdir / "render-source")
            written, why2 = (renderer.render(generated, workdir / "render-generated")
                             if source is not None else (None, why))
            if source is None or written is None:
                report.note(deck.name, f"generated package not rendered: {why or why2}")
                continue
            check_generated(report, deck.name, source, written)

        print("\n--- ODF: corpus/odp, rendered and round-tripped ---")
        check_odp(binary, renderer, report, scratch, wanted)

        print("\n--- cross conversion: each corpus opened in the other dialect ---")
        check_cross(binary, renderer, report, scratch, wanted)
    finally:
        if not args.keep:
            shutil.rmtree(scratch, ignore_errors=True)

    print(
        f"\n{len(decks)} deck(s) + {len(odp_decks)} .odp: "
        f"{report.by_bytes} round trip(s) identical by bytes, "
        f"{report.by_render} rendered, {report.pages} page(s) compared, "
        f"{report.failures} failure(s), {len(report.unmeasured)} not measured"
    )
    if report.worst[0]:
        print(f"worst page: {report.worst[0]} at {report.worst[1]:.4f} "
              f"(threshold {THRESHOLD})")
    if report.failures:
        return 1
    if report.unmeasured:
        print("nothing was measured for:", file=sys.stderr)
        for note in report.unmeasured:
            print(f"  {note}", file=sys.stderr)
        return 2
    if not report.pages and not report.by_bytes:
        print("render_gate: no page was compared at all", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

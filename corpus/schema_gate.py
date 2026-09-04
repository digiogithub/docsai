#!/usr/bin/env python3
"""CI gate 1: the markup this project produces, validated against the grammar
that describes it — the ECMA-376 XSDs for a `.pptx`
(`docs/technical-analysis-presentations.md` §5.5), and the OpenDocument 1.3
RELAX NG for an `.odp` (Phase 18-A, `kb/96-phase-18-corpus.md`).

    python3 corpus/schema_gate.py                 # both arms, the whole corpus
    python3 corpus/schema_gate.py basic-slides    # one fixture, by name
    python3 corpus/schema_gate.py --keep /tmp/g   # leave the packages behind

Both arms validate the same two things: the packages this project's fixtures
are, and every part its writers produce. `odf_arm()` carries the argument for
the one variant it does not have — an `.odp` written on no package at all,
which the `.odp` writer refuses by name rather than inventing a design.

Requires `xmllint` (Debian/Ubuntu `libxml2-utils`, macOS built in) and a built
CLI (`cargo build -p docsai-cli`). Neither is guessed at: a missing tool exits 2
with a message, never a green tick that measured nothing.

## What is validated, and what deliberately is not

A preserved deck written back is byte for byte the deck that was read — 15-H
asserts that over the whole corpus — so validating its parts would measure the
output of whatever wrote the deck in the first place, not ours. What this gate
validates is every part **this writer produced**, and it finds them the only way
that cannot drift: by diffing the written package against the package it was
written on, and taking every XML part whose bytes are new or changed.

Two packages are produced per corpus deck, because the writer has two paths and
an unedited round trip exercises neither:

- `edited` — the deck's DocMark with every slide title retyped, written back on
  the **preserved package**. What changes is exactly the slide parts the writer
  rebuilt (15-B, 15-C, 15-F).
- `generated` — the same DocMark with the `skeleton:` line removed, so the deck
  has no package at all and is written on the **embedded template** (15-G).
  What changes is every part the generator wrote: slides, their relationships,
  `[Content_Types].xml`, `ppt/presentation.xml`.

Not validated, and each for a reason: a part copied byte for byte out of the
skeleton (it is not ours, and it is unchanged); a non-XML part (media); and
`docProps/core.xml`, which no schema in `schemas/` declares — see the header of
`schemas/ooxml-transitional.xsd`. The third is not a silent exemption: if the
writer ever starts rebuilding that part, the part turns up in the diff, xmllint
reports "no matching global declaration", and this gate goes red.
"""
import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
CORPUS = ROOT / "corpus" / "pptx"
SCHEMA = ROOT / "schemas" / "ooxml-transitional.xsd"
TEMPLATE = ROOT / "crates" / "docsai-office" / "assets" / "default-template.pptx"
ODF_TEMPLATE = ROOT / "crates" / "docsai-odf" / "assets" / "default-template.odp"

# ODF is described by a RELAX NG grammar, not by an XSD: a different validation
# technology, which is why the ODF arm below is a second xmllint invocation with
# `--relaxng` rather than another entry in the OOXML schema set. Provenance and
# licence are in `schemas/README.md`.
ODP_CORPUS = ROOT / "corpus" / "odp"
ODF_SCHEMA = ROOT / "schemas" / "odf-1.3" / "OpenDocument-v1.3-schema.rng"
ODF_MANIFEST_SCHEMA = (
    ROOT / "schemas" / "odf-1.3" / "OpenDocument-v1.3-manifest-schema.rng"
)

# `## Title {#n1 .slide …}`: the one line a person would edit first, and the one
# that forces the writer to rebuild the slide part it belongs to.
SLIDE_HEADING = re.compile(r"^##\s.*\{[^}]*\.slide")

MCE = "http://schemas.openxmlformats.org/markup-compatibility/2006"


def markup_compatibility(blob):
    """Apply the ECMA-376 **Part 3** processing a consumer applies before it
    reads a part, and return the bytes to validate.

    `mc:AlternateContent` is how a producer offers an extension element with a
    fallback for consumers that do not know it. It is legal everywhere, and the
    Part 4 schemas cannot say so: they describe the markup *after* compatibility
    processing, so validating the raw bytes reports every `AlternateContent` as
    an unexpected element. `corpus/pptx/smartart-fallback.pptx` carries one, as
    every PowerPoint deck with a diagram does.

    A validator that understands none of the `Requires` namespaces — which is
    what a Transitional-only schema set is — selects the `mc:Fallback` branch.
    That is what this does, and nothing else: the rest of the part keeps its
    bytes, and a part with no `AlternateContent` in it is returned untouched so
    that xmllint's line numbers still point into the file the writer produced.

    The limit, stated rather than discovered: `mc:Ignorable` with
    `mc:ProcessContent` is not implemented. No part this gate validates uses it;
    one that did would be reported as invalid rather than passed."""
    if MCE.encode() not in blob:
        return blob, False

    root = ET.fromstring(blob)
    parents = {child: parent for parent in root.iter() for child in parent}
    for node in list(root.iter(f"{{{MCE}}}AlternateContent")):
        parent = parents.get(node)
        if parent is None:
            break
        fallback = node.find(f"{{{MCE}}}Fallback")
        index = list(parent).index(node)
        parent.remove(node)
        for offset, child in enumerate(list(fallback) if fallback is not None else []):
            parent.insert(index + offset, child)
    return ET.tostring(root, encoding="utf-8"), True


def find_binary(explicit):
    if explicit:
        path = Path(explicit)
        if not path.exists():
            sys.exit(f"schema_gate: no CLI at {path}")
        return path
    exe = "docsai.exe" if os.name == "nt" else "docsai"
    for profile in ("debug", "release"):
        path = ROOT / "target" / profile / exe
        if path.exists():
            return path
    sys.exit(
        "schema_gate: no docsai binary under target/; build one with\n"
        "    cargo build -p docsai-cli"
    )


def run(binary, *args):
    """`docsai` exits non-zero when a conversion emitted warnings, which is not
    a failure of this gate; only a missing output is."""
    subprocess.run(
        [str(binary), *[str(a) for a in args]],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )


def retitle(markdown):
    """Retype every slide title. Returns the text and how many it touched."""
    out, touched = [], 0
    for line in markdown.splitlines(keepends=True):
        if SLIDE_HEADING.match(line):
            head, sep, rest = line.partition(" {")
            out.append(f"{head} (edited){sep}{rest}")
            touched += 1
        else:
            out.append(line)
    return "".join(out), touched


def bump_chart_values(markdown):
    """Add a digit to every number in a chart's data table (Phase 16-C).

    The `edited` variant exists to make the writer *write*, and until this
    increment nothing it did touched `ppt/charts/*`: the retitle above moves a
    slide and no chart part is ever validated on the preserved-package path.
    A chart's caches are spliced inside a part this gate would otherwise never
    see, so the mutation reaches them."""
    out, touched = [], 0
    for line in markdown.splitlines(keepends=True):
        cells = line.split("|")
        # A GFM body row of a chart table: a leading pipe, and at least one cell
        # that is nothing but a number.
        if len(cells) > 2 and any(c.strip().replace(".", "", 1).isdigit()
                                  for c in cells[2:]):
            cells = [
                (f" {c.strip()}1 " if c.strip().replace(".", "", 1).isdigit() else c)
                for c in cells
            ]
            line = "|".join(cells)
            touched += 1
        out.append(line)
    return "".join(out), touched


def edit_comments(markdown):
    """Retype the first line of every `::: {.comment}` container (Phase 16-F).

    Same argument as `bump_chart_values` one increment earlier: the comment
    parts are preserved, so nothing else this gate does makes the writer touch
    `ppt/comments/*` or `ppt/commentAuthors.xml`, and a part the gate never
    sees is a part whose schema nobody checks. Retyping the text is the
    smallest edit that forces the splice."""
    out, touched, inside = [], 0, False
    for line in markdown.splitlines(keepends=True):
        if line.startswith("::: {") and ".comment" in line:
            inside = True
            out.append(line)
            continue
        if inside:
            inside = False
            if line.strip() and not line.startswith(":::"):
                line = line.rstrip("\n") + " (revisado)\n"
                touched += 1
        out.append(line)
    return "".join(out), touched


def without_skeleton(markdown):
    return "".join(
        line
        for line in markdown.splitlines(keepends=True)
        if not line.startswith("skeleton:")
    )


def written_parts(package, baseline):
    """The XML parts of `package` that `baseline` does not have, or has with
    other bytes. This is the whole definition of "what the writer wrote"."""
    with zipfile.ZipFile(package) as new, zipfile.ZipFile(baseline) as old:
        known = set(old.namelist())
        parts = []
        for name in sorted(new.namelist()):
            if not name.endswith((".xml", ".rels")):
                continue
            if name in known and old.read(name) == new.read(name):
                continue
            parts.append((name, new.read(name)))
        return parts


def xmllint(files, schema_args):
    """Run xmllint over `files` — a `{path: reported name}` map — with
    `schema_args` naming the grammar, and turn its stderr into
    `{name: [problem, …]}`.

    Split out of `validate()` so the ODF arm below can reuse it: the grammar is
    a different technology (`--relaxng` against a RELAX NG file rather than
    `--schema` against an XSD set) but xmllint reports its verdicts in exactly
    the same three shapes, and a second copy of this parser would be a second
    place for a silent pass to hide."""
    proc = subprocess.run(
        ["xmllint", "--nonet", "--noout", *schema_args, *files],
        capture_output=True,
        text=True,
        check=False,
    )
    problems = {name: [] for name in files.values()}
    judged = set()
    for line in proc.stderr.splitlines():
        # `<file>:<line>: element <x>: Schemas validity error : <message>`
        match = re.match(r"^(.*?):(\d+): element [^:]*: (.*)$", line)
        if match and match.group(1) in files:
            path, lineno, message = match.groups()
            problems[files[path]].append(f"line {lineno}: {message.strip()}")
        elif line.endswith(" fails to validate"):
            path = line[: -len(" fails to validate")]
            judged.add(path)
            if path in files and not problems[files[path]]:
                problems[files[path]].append("fails to validate")
        elif line.endswith(" validates"):
            judged.add(line[: -len(" validates")])
        else:
            # Anything xmllint says that is neither a verdict nor a parsed
            # error is still evidence; losing it would be the silent pass this
            # gate exists to prevent.
            problems.setdefault("xmllint", []).append(line)

    # A part xmllint reached no verdict on is the one way this gate could go
    # green having measured nothing — a schema set that failed to compile, say.
    # It is a failure, and it names itself.
    for path, name in files.items():
        if path not in judged:
            problems[name].append(
                "xmllint reached no verdict on this part "
                "(did the schema set compile?)"
            )
    return problems


def validate(parts, workdir):
    """xmllint over one package's parts. Returns {part: [problem, …]}, empty
    lists for the parts that validate."""
    if not parts:
        return {}, set()
    files, processed = {}, set()
    for name, blob in parts:
        path = workdir / name.replace("[", "_").replace("]", "_")
        path.parent.mkdir(parents=True, exist_ok=True)
        blob, touched = markup_compatibility(blob)
        if touched:
            processed.add(name)
        path.write_bytes(blob)
        files[str(path)] = name
    return xmllint(files, ["--schema", str(SCHEMA)]), processed


def package_for(binary, deck, workdir, variant):
    """Convert `deck` to DocMark, mutate it for `variant`, write it back."""
    workdir.mkdir(parents=True, exist_ok=True)
    markdown_path = workdir / "deck.dmk.md"
    run(binary, "convert", deck, "-o", markdown_path)
    if not markdown_path.exists():
        return None, "the deck did not convert to DocMark"

    markdown, touched = retitle(markdown_path.read_text(encoding="utf-8"))
    if not touched:
        return None, "no slide heading to retype"
    markdown, _ = bump_chart_values(markdown)
    markdown, _ = edit_comments(markdown)
    if variant == "generated":
        markdown = without_skeleton(markdown)
    edited = workdir / "edited.dmk.md"
    edited.write_text(markdown, encoding="utf-8")

    out = workdir / f"{variant}{deck.suffix}"
    run(binary, "convert", edited, "-o", out)
    if not out.exists():
        return None, "the DocMark did not convert back to a package"
    return out, None


VARIANTS = {
    "edited": ("slides rebuilt on the preserved package", None),
    "generated": ("the whole package written on the embedded template", TEMPLATE),
}


# --------------------------------------------------------------------------
# The ODF arm (Phase 18-A)
# --------------------------------------------------------------------------


def odf_namespaces():
    """The namespaces the vendored grammar itself declares.

    Derived from the schema rather than listed here, so that refreshing the
    schema cannot leave a stale list behind."""
    # The `xml:` namespace is bound by the XML specification itself and is
    # never declared, so no schema's namespace map mentions it. Without it here
    # `xml:id` is read as foreign and stripped, and every `smil:targetElement`
    # that points at one becomes a dangling IDREF — which is how this line came
    # to be written.
    allowed = {"http://www.w3.org/XML/1998/namespace"}
    for _, (prefix, uri) in ET.iterparse(ODF_SCHEMA, events=["start-ns"]):
        del prefix
        allowed.add(uri)
    for _, (prefix, uri) in ET.iterparse(ODF_MANIFEST_SCHEMA, events=["start-ns"]):
        del prefix
        allowed.add(uri)
    return allowed


def strip_foreign(blob, allowed):
    """Remove foreign elements and attributes, and return the bytes to validate.

    This is the ODF counterpart of `markup_compatibility()` above, and it rests
    on the same kind of clause: ODF 1.3 conformance defines an *extended*
    document as one that may carry elements and attributes from namespaces the
    grammar does not define, which a consumer ignores. The grammar describes the
    conformant subset, so validating the raw bytes reports every extension as an
    unexpected element.

    There is a concrete one in this corpus. LibreOffice Impress stores a slide
    comment as `officeooo:annotation` (namespace
    `http://openoffice.org/2009/office`) directly under `draw:page`, and that is
    the only spelling that works: ODF 1.3 admits `office:annotation` in
    paragraph content, LibreOffice refuses to open a deck that has one there
    («source file could not be loaded», measured), and a page-level
    `office:annotation` it opens and silently discards. Every real `.odp` with
    comments in it therefore carries markup this grammar cannot describe.

    The limit, stated rather than discovered: an extension that *wraps*
    conformant content is stripped with its children, so nothing inside it is
    validated. No part here does that; one that did would be under-measured
    rather than reported, and the printed "(after foreign markup removal)" is
    what says a part was touched at all."""
    root = ET.fromstring(blob)
    touched = False
    parents = {child: parent for parent in root.iter() for child in parent}
    for node in list(root.iter()):
        namespace = node.tag[1:].partition("}")[0] if node.tag.startswith("{") else ""
        if namespace and namespace not in allowed:
            parent = parents.get(node)
            if parent is not None:
                parent.remove(node)
                touched = True
            continue
        for key in list(node.attrib):
            if not key.startswith("{"):
                continue
            if key[1:].partition("}")[0] not in allowed:
                del node.attrib[key]
                touched = True
    if not touched:
        return blob, False
    return ET.tostring(root, encoding="utf-8"), True


def odf_parts(package, only=None):
    """Every XML part of an ODF package, paired with the grammar that describes
    it. `META-INF/manifest.xml` has a schema of its own — it is not written in
    the document grammar and validating it against the big one reports "Did not
    expect element manifest there".

    `only` narrows the set to a collection of part names, which is how the
    writer arm asks for the parts the writer actually produced."""
    parts = []
    with zipfile.ZipFile(package) as zf:
        for name in sorted(zf.namelist()):
            if not name.endswith(".xml"):
                continue
            if only is not None and name not in only:
                continue
            schema = ODF_MANIFEST_SCHEMA if name.startswith("META-INF/") else ODF_SCHEMA
            parts.append((name, zf.read(name), schema))
    return parts


def validate_odf(package, workdir, only=None):
    """Every XML part of one ODF package against the vendored RELAX NG.

    Two grammars means two xmllint runs; both are reported into one dictionary
    so the caller cannot tell — or forget — that there were two."""
    problems = {}
    by_schema = {}
    processed = set()
    allowed = odf_namespaces()
    for name, blob, schema in odf_parts(package, only):
        # A space in a part name — `Object 1/content.xml` is where an ODF chart
        # lives — comes back from xmllint percent-escaped, and the escaped path
        # matches nothing this function wrote. Renaming it on disk keeps every
        # error attributable to the part it came from.
        path = workdir / name.replace(" ", "_")
        path.parent.mkdir(parents=True, exist_ok=True)
        blob, touched = strip_foreign(blob, allowed)
        if touched:
            processed.add(name)
        path.write_bytes(blob)
        by_schema.setdefault(schema, {})[str(path)] = name
    for schema, files in by_schema.items():
        problems.update(xmllint(files, ["--relaxng", str(schema)]))
    return problems, processed


def odf_arm(scratch, wanted, binary):
    """The `.odp` corpus against ODF 1.3, in two passes.

    **The fixtures**, which is what 18-A could measure: the packages
    `corpus/generate.py` produces, every XML part of each.

    **The writer's output**, which 18-D closes: the same thing the OOXML arm
    does — the deck's DocMark with every page title retyped, written back on the
    preserved package, and every XML part whose bytes are new or changed
    validated. That set is found by diffing, so it cannot drift: a part the
    writer starts rewriting turns up in it by itself.

    The `generated` variant the OOXML arm has arrived with 18-E, and it is
    measured by `cross_arm()` below rather than here, because the package a
    generated `.odp` is written on is the same one a `.pptx` crosses into: an
    embedded ODF template. 18-D had no such template and refused a deck with no
    `skeleton:` by name; 18-E ships one, because a deck read from a `.pptx`
    carries an OPC package and therefore has no ODF package to splice into.

    `odt/` and `ods/` are deliberately **not** here, and not because the
    machinery cannot reach them: it is one glob away, and switching it on today
    reports exactly one failure, `corpus/odt/images-transformed.odt`. Its
    `style:graphic-properties` carries `draw:rotation-angle`, which ODF 1.3 has
    nowhere — a shape is rotated by `draw:transform` on the shape, not by a
    style attribute — and `crates/docsai-odf/src/styles.rs:187` is what reads
    it, so correcting the fixture drops the rotation from its golden. That is
    reader work, in a crate this increment does not touch. `kb/96-phase-18-corpus.md`
    carries it forward."""
    if not ODF_SCHEMA.exists() or not ODF_MANIFEST_SCHEMA.exists():
        print(f"schema_gate: no ODF schema set under {ODF_SCHEMA.parent}",
              file=sys.stderr)
        return 0, 0, [f"the whole odp corpus: no schema at {ODF_SCHEMA}"]

    decks = sorted(ODP_CORPUS.iterdir()) if ODP_CORPUS.exists() else []
    decks = [d for d in decks if d.suffix == ".odp"]
    if wanted:
        decks = [d for d in decks if d.stem in wanted]
    checked = invalid = 0
    failures = []
    def report(problems, processed):
        nonlocal checked, invalid
        for name in sorted(problems):
            checked += 1
            if problems[name]:
                invalid += 1
                print(f"  FAIL  {name}")
                for problem in problems[name]:
                    print(f"        {problem}")
            else:
                foreign = " (after foreign markup removal)" if name in processed else ""
                print(f"  ok    {name}{foreign}")

    for deck in decks:
        print(f"{deck.name} [fixture: the package corpus/generate.py wrote]")
        problems, processed = validate_odf(deck, scratch / "odf" / deck.stem)
        if not problems:
            failures.append(f"{deck.name}: the package has no XML part at all")
            print("  SKIP  no XML part to validate")
            continue
        report(problems, processed)

        print(f"{deck.name} [edited: the parts the .odp writer rewrote]")
        workdir = scratch / "odf-written" / deck.stem
        written, why = odp_package_for(binary, deck, workdir)
        if written is None:
            skipped = f"{deck.name} [edited]: {why}"
            failures.append(skipped)
            print(f"  SKIP  {why}")
            continue
        names = [name for name, _ in written_parts(written, deck)]
        if not names:
            failures.append(
                f"{deck.name} [edited]: nothing to validate — the writer produced "
                f"no part of its own"
            )
            print("  SKIP  the writer produced no part of its own")
            continue
        problems, processed = validate_odf(written, workdir / "parts", only=set(names))
        report(problems, processed)
    return checked, invalid, failures


def odp_package_for(binary, deck, workdir):
    """Convert a `.odp` to DocMark, retype every page title, write it back.

    The retitle is what makes the writer *write*: an untouched `.odp` comes back
    byte for byte the package that was read (18-D), so a round trip on its own
    produces no part of the writer's for a schema to look at."""
    workdir.mkdir(parents=True, exist_ok=True)
    markdown_path = workdir / "deck.dmk.md"
    run(binary, "convert", deck, "-o", markdown_path)
    if not markdown_path.exists():
        return None, "the deck did not convert to DocMark"

    markdown, touched = retitle(markdown_path.read_text(encoding="utf-8"))
    if not touched:
        return None, "no page heading to retype"
    edited = workdir / "edited.dmk.md"
    edited.write_text(markdown, encoding="utf-8")

    out = workdir / "edited.odp"
    run(binary, "convert", edited, "-o", out)
    if not out.exists():
        return None, "the DocMark did not convert back to a package"
    return out, None


# --------------------------------------------------------------------------
# The cross arm (Phase 18-E)
# --------------------------------------------------------------------------


def crossed(binary, deck, workdir, suffix):
    """`deck` -> DocMark -> a package of the *other* dialect.

    No edit is made to the DocMark on the way: a cross conversion writes every
    page from the model whatever anybody typed, so there is nothing a retitle
    would provoke that the plain conversion does not."""
    workdir.mkdir(parents=True, exist_ok=True)
    markdown = workdir / "deck.dmk.md"
    run(binary, "convert", deck, "-o", markdown)
    if not markdown.exists():
        return None, "the deck did not convert to DocMark"
    out = workdir / f"crossed{suffix}"
    run(binary, "convert", markdown, "-o", out)
    if not out.exists() or out.stat().st_size == 0:
        return None, f"the DocMark did not convert to {suffix}"
    return out, None


def cross_arm(scratch, wanted, binary):
    """Every deck of each corpus written out in the *other* dialect, validated.

    This is 18-E's second acceptance criterion asked of the grammars rather than
    of the text: `pptx -> DocMark -> odp` must be schema-valid ODF and
    `odp -> DocMark -> pptx` must be schema-valid OOXML. Neither can be
    byte-identical to anything — the two dialects are different packages — so
    what is validated is what each writer *wrote*, found the same way both other
    arms find it: by diffing against the template the deck was generated on.

    A cross conversion always goes through the template, because a preserved
    package of the wrong dialect is not spliced into (`odp/write.rs`,
    `pptx/write.rs`), so the template is the honest baseline for both halves."""
    checked = invalid = 0
    failures = []

    def report(problems, processed, note):
        nonlocal checked, invalid
        for name in sorted(problems):
            checked += 1
            if problems[name]:
                invalid += 1
                print(f"  FAIL  {name}")
                for problem in problems[name]:
                    print(f"        {problem}")
            else:
                extra = f" ({note})" if name in processed else ""
                print(f"  ok    {name}{extra}")

    decks = sorted(p for p in CORPUS.iterdir() if p.suffix in (".pptx", ".pptm"))
    odp_decks = sorted(ODP_CORPUS.glob("*.odp")) if ODP_CORPUS.exists() else []
    if wanted:
        decks = [d for d in decks if d.stem in wanted]
        odp_decks = [d for d in odp_decks if d.stem in wanted]

    for deck in decks:
        print(f"{deck.name} [crossed: written as .odp]")
        workdir = scratch / "cross-odp" / deck.stem
        package, why = crossed(binary, deck, workdir, ".odp")
        if package is None:
            failures.append(f"{deck.name} [crossed to .odp]: {why}")
            print(f"  SKIP  {why}")
            continue
        names = [name for name, _ in written_parts(package, ODF_TEMPLATE)]
        if not names:
            failures.append(f"{deck.name} [crossed to .odp]: no part of the writer's")
            print("  SKIP  the writer produced no part of its own")
            continue
        problems, processed = validate_odf(package, workdir / "parts", only=set(names))
        report(problems, processed, "after foreign markup removal")

    for deck in odp_decks:
        print(f"{deck.name} [crossed: written as .pptx]")
        workdir = scratch / "cross-pptx" / deck.stem
        package, why = crossed(binary, deck, workdir, ".pptx")
        if package is None:
            failures.append(f"{deck.name} [crossed to .pptx]: {why}")
            print(f"  SKIP  {why}")
            continue
        parts = written_parts(package, TEMPLATE)
        if not parts:
            failures.append(f"{deck.name} [crossed to .pptx]: no part of the writer's")
            print("  SKIP  the writer produced no part of its own")
            continue
        problems, processed = validate(parts, workdir / "parts")
        report(problems, processed, "after mc: fallback selection")

    return checked, invalid, failures


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("decks", nargs="*", help="deck names to check (default: all)")
    ap.add_argument("--binary", help="path to the docsai CLI")
    ap.add_argument("--keep", help="directory to leave the packages in")
    args = ap.parse_args()

    if not shutil.which("xmllint"):
        sys.exit("schema_gate: xmllint not found (Debian/Ubuntu: libxml2-utils)")
    if not SCHEMA.exists():
        sys.exit(f"schema_gate: no schema set at {SCHEMA}")
    binary = find_binary(args.binary)

    decks = sorted(p for p in CORPUS.iterdir() if p.suffix in (".pptx", ".pptm"))
    odp_decks = sorted(ODP_CORPUS.glob("*.odp")) if ODP_CORPUS.exists() else []
    wanted_odf = None
    if args.decks:
        wanted = {
            d.removesuffix(".pptx").removesuffix(".pptm").removesuffix(".odp")
            for d in args.decks
        }
        decks = [d for d in decks if d.stem in wanted]
        wanted_odf = wanted
        if not decks and not [d for d in odp_decks if d.stem in wanted]:
            sys.exit(f"schema_gate: no corpus deck named {', '.join(sorted(wanted))}")

    scratch = Path(args.keep) if args.keep else Path(tempfile.mkdtemp(prefix="schema-gate-"))
    checked = invalid = skipped = odp_count = 0
    failures = []
    try:
        for deck in decks:
            for variant, (what, template) in VARIANTS.items():
                print(f"{deck.name} [{variant}: {what}]")
                workdir = scratch / deck.stem / variant
                package, why = package_for(binary, deck, workdir, variant)
                if package is None:
                    skipped += 1
                    print(f"  SKIP  {why}")
                    failures.append(f"{deck.name} [{variant}]: {why}")
                    continue
                parts = written_parts(package, template or deck)
                if not parts:
                    skipped += 1
                    print("  SKIP  the writer produced no part of its own")
                    failures.append(
                        f"{deck.name} [{variant}]: nothing to validate — the "
                        f"package is the one it was written on"
                    )
                    continue
                problems, processed = validate(parts, workdir / "parts")
                for name in sorted(problems):
                    checked += 1
                    if problems[name]:
                        invalid += 1
                        print(f"  FAIL  {name}")
                        for problem in problems[name]:
                            print(f"        {problem}")
                    else:
                        mce = " (after mc: fallback selection)" if name in processed else ""
                        print(f"  ok    {name}{mce}")

        print(f"\n--- ODF: corpus/odp against {ODF_SCHEMA.name} ---")
        odf_checked, odf_invalid, odf_failures = odf_arm(scratch, wanted_odf, binary)
        checked += odf_checked
        invalid += odf_invalid
        failures += odf_failures
        odp_count = odf_checked

        print(f"\n--- cross conversion: each corpus written in the other dialect ---")
        cross_checked, cross_invalid, cross_failures = cross_arm(scratch, wanted_odf, binary)
        checked += cross_checked
        invalid += cross_invalid
        failures += cross_failures
    finally:
        if not args.keep:
            shutil.rmtree(scratch, ignore_errors=True)

    print(
        f"\n{len(decks)} deck(s), {len(decks) * len(VARIANTS)} package(s), "
        f"{checked} part(s) validated ({odp_count} of them .odp), "
        f"{invalid} invalid, {skipped} not measured"
    )
    if invalid:
        return 1
    if failures:
        # A package the gate could not measure is not a pass. Saying so is the
        # whole difference between a gate and a green tick.
        print("nothing was measured for:", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        return 2
    if not checked:
        print("schema_gate: no part was validated at all", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

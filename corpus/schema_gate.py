#!/usr/bin/env python3
"""CI gate 1: the parts this project *writes* into a `.pptx`, validated against
the ECMA-376 XSDs (`docs/technical-analysis-presentations.md` §5.5).

    python3 corpus/schema_gate.py                 # the whole pptx corpus
    python3 corpus/schema_gate.py basic-slides    # one deck, by name
    python3 corpus/schema_gate.py --keep /tmp/g   # leave the packages behind

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

    proc = subprocess.run(
        ["xmllint", "--nonet", "--noout", "--schema", str(SCHEMA), *files],
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
    return problems, processed


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
    if args.decks:
        wanted = {d.removesuffix(".pptx").removesuffix(".pptm") for d in args.decks}
        decks = [d for d in decks if d.stem in wanted]
        if not decks:
            sys.exit(f"schema_gate: no corpus deck named {', '.join(sorted(wanted))}")

    scratch = Path(args.keep) if args.keep else Path(tempfile.mkdtemp(prefix="schema-gate-"))
    checked = invalid = skipped = 0
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
    finally:
        if not args.keep:
            shutil.rmtree(scratch, ignore_errors=True)

    print(
        f"\n{len(decks)} deck(s), {len(decks) * len(VARIANTS)} package(s), "
        f"{checked} part(s) validated, {invalid} invalid, {skipped} not measured"
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

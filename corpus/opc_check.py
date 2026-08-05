#!/usr/bin/env python3
"""Structural check of an OPC package: what a repair dialog is usually about.

Not a schema validation. It checks the four things that actually break a
generated package: XML that does not parse, a part with no content type, an
r:id that resolves to nothing, and a relationship pointing at a part that is
not in the zip.
"""
import sys
import zipfile
import posixpath
import re
from xml.etree import ElementTree as ET

CT_NS = "{http://schemas.openxmlformats.org/package/2006/content-types}"
REL_NS = "{http://schemas.openxmlformats.org/package/2006/relationships}"
R_NS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"


def check(path):
    problems = []
    with zipfile.ZipFile(path) as z:
        names = set(z.namelist())

        if "[Content_Types].xml" not in names:
            return ["[Content_Types].xml missing"]

        # 1. every XML part parses
        for name in sorted(names):
            if name.endswith((".xml", ".rels")):
                try:
                    ET.fromstring(z.read(name))
                except ET.ParseError as exc:
                    problems.append(f"{name}: not well-formed XML: {exc}")

        # 2. every part has a content type
        types = ET.fromstring(z.read("[Content_Types].xml"))
        defaults = {d.get("Extension").lower() for d in types.findall(f"{CT_NS}Default")}
        overrides = {o.get("PartName") for o in types.findall(f"{CT_NS}Override")}
        for name in sorted(names):
            if name == "[Content_Types].xml":
                continue
            ext = name.rsplit(".", 1)[-1].lower() if "." in name else ""
            if f"/{name}" not in overrides and ext not in defaults:
                problems.append(f"{name}: no content type (no Override, no Default for .{ext})")

        # 3. every relationship target exists
        rel_by_part = {}
        for name in sorted(n for n in names if n.endswith(".rels")):
            base = posixpath.dirname(posixpath.dirname(name))
            rels = ET.fromstring(z.read(name))
            owner = posixpath.join(base, posixpath.basename(name)[:-5]) if not name.endswith(
                "/_rels/.rels") else ""
            table = {}
            for rel in rels.findall(f"{REL_NS}Relationship"):
                target, rid = rel.get("Target"), rel.get("Id")
                table[rid] = target
                if rel.get("TargetMode") == "External":
                    continue
                resolved = posixpath.normpath(posixpath.join(base, target))
                if resolved not in names:
                    problems.append(f"{name}: {rid} -> {target} resolves to {resolved}, not in package")
            rel_by_part[owner] = table

        # 4. every r:id/r:embed used in a part is declared in that part's rels
        for name in sorted(n for n in names if n.endswith(".xml") and "/_rels/" not in n):
            body = z.read(name).decode("utf-8", "replace")
            used = set(re.findall(r'r:(?:id|embed|link)="([^"]+)"', body))
            if not used:
                continue
            declared = set(rel_by_part.get(name, {}))
            for rid in sorted(used - declared):
                problems.append(f"{name}: uses {rid}, not declared in its .rels")
    return problems


bad = 0
for path in sys.argv[1:]:
    problems = check(path)
    if problems:
        bad += 1
        print(f"FAIL {path}")
        for p in problems:
            print(f"  {p}")
    else:
        print(f"ok   {path}")
sys.exit(1 if bad else 0)

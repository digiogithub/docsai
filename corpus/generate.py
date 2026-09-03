#!/usr/bin/env python3
"""Deterministic generator for the docsai test corpus.

Every file under ``corpus/docx`` and ``corpus/xlsx`` is produced by this script
so that the corpus is reproducible, reviewable as source (the XML lives here,
not inside an opaque binary) and free of real user data.

Usage::

    python3 corpus/generate.py            # regenerate everything
    python3 corpus/generate.py --check    # fail if the tree is out of date

Design notes:

* Packages are written with a fixed ZIP timestamp and fixed member order, so
  regenerating produces byte-identical archives.
* Media are synthesised in pure Python (no Pillow): PNG via zlib, GIF and EMF
  by hand. That keeps the generator dependency-free on the three CI platforms.
* Each document isolates *one* feature so a failing golden test points at a
  single area of the reader.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import io
import os
import struct
import sys
import zlib
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
FIXED_DATE = (2026, 1, 1, 0, 0, 0)

# --------------------------------------------------------------------------
# Media synthesis
# --------------------------------------------------------------------------


def png(width: int, height: int, rgb: tuple[int, int, int]) -> bytes:
    """Minimal, valid, solid-colour RGB PNG."""

    def chunk(kind: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
        )

    raw = b"".join(b"\x00" + bytes(rgb) * width for _ in range(height))
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def gif(width: int, height: int, rgb: tuple[int, int, int]) -> bytes:
    """Minimal single-colour GIF87a. Used to exercise content sniffing."""
    header = b"GIF87a" + struct.pack("<HH", width, height) + b"\xf0\x00\x00"
    palette = bytes(rgb) + b"\x00\x00\x00"
    descriptor = b"\x2c" + struct.pack("<HHHH", 0, 0, width, height) + b"\x00"
    # LZW stream for an all-zero image, minimum code size 2.
    lzw = b"\x02\x02\x4c\x01\x00"
    return header + palette + descriptor + lzw + b"\x3b"


def emf(width_px: int, height_px: int) -> bytes:
    """Minimal EMR_HEADER + EMR_EOF record pair.

    Not renderable, but structurally an EMF: enough for format sniffing and for
    the "extract as-is, warn, never rasterise" path.
    """
    bounds = struct.pack("<iiii", 0, 0, width_px, height_px)
    frame = struct.pack("<iiii", 0, 0, width_px * 26, height_px * 26)  # .01 mm
    header = struct.pack("<II", 1, 108)  # iType = EMR_HEADER, nSize
    header += bounds + frame
    header += struct.pack("<I", 0x464D4520)  # " EMF" signature
    header += struct.pack("<I", 0x00010000)  # nVersion
    header += struct.pack("<II", 108 + 20, 2)  # nBytes, nRecords
    header += struct.pack("<HH", 0, 0)  # nHandles, sReserved
    header += struct.pack("<III", 0, 0, 0)  # description
    header += struct.pack("<ii", width_px, height_px)  # szlDevice
    header += struct.pack("<ii", width_px * 26, height_px * 26)  # szlMillimeters
    header += struct.pack("<III", 0, 0, 0)  # cbPixelFormat, offPixelFormat, bOpenGL
    eof = struct.pack("<IIIII", 14, 20, 0, 16, 20)  # EMR_EOF
    return header + eof


BLUE_PNG = png(120, 90, (30, 90, 200))
RED_PNG = png(64, 64, (200, 40, 40))
GREEN_GIF = gif(48, 32, (20, 160, 60))
LOGO_EMF = emf(200, 100)

EMU_PER_PX = 9525  # at 96 dpi
EMU_PER_CM = 360000


def px(n: float) -> int:
    return int(round(n * EMU_PER_PX))


def cm(n: float) -> int:
    return int(round(n * EMU_PER_CM))


# --------------------------------------------------------------------------
# Package plumbing
# --------------------------------------------------------------------------

XML_DECL = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'

W_NS = (
    'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" '
    'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" '
    'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
    'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
    'xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" '
    'xmlns:v="urn:schemas-microsoft-com:vml" '
    'xmlns:o="urn:schemas-microsoft-com:office:office" '
    'xmlns:w10="urn:schemas-microsoft-com:office:word"'
)

RELS_NS = 'xmlns="http://schemas.openxmlformats.org/package/2006/relationships"'
REL_BASE = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"

CONTENT_TYPE = {
    ".png": "image/png",
    ".gif": "image/gif",
    ".emf": "image/x-emf",
}


def package_bytes(parts: dict[str, bytes]) -> bytes:
    """An OPC package as bytes. An embedded workbook lives *inside* another
    package and so never has a path of its own; the fixed timestamp still
    applies, or ``--check`` would see a new digest on every run."""
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w", zipfile.ZIP_DEFLATED) as zf:
        for name in sorted(parts):
            info = zipfile.ZipInfo(name, date_time=FIXED_DATE)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o600 << 16
            zf.writestr(info, parts[name])
    return buffer.getvalue()


def write_package(path: Path, parts: dict[str, bytes]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(package_bytes(parts))


def write_odf_package(path: Path, parts: dict[str, bytes]) -> None:
    """ODF package: `mimetype` first and stored uncompressed."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, "w") as zf:
        if "mimetype" in parts:
            info = zipfile.ZipInfo("mimetype", date_time=FIXED_DATE)
            info.compress_type = zipfile.ZIP_STORED
            info.external_attr = 0o600 << 16
            zf.writestr(info, parts["mimetype"])
        for name in sorted(k for k in parts if k != "mimetype"):
            info = zipfile.ZipInfo(name, date_time=FIXED_DATE)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o600 << 16
            zf.writestr(info, parts[name])


def rels_part(entries: list[tuple[str, str, str]], *, root: bool = False) -> bytes:
    """A `_rels` part. ``entries`` are ``(rId, type_suffix, target)``.

    A package-root part addresses core properties through a different namespace
    family than every other relationship, which is why ``root`` exists rather
    than the caller spelling the full type out. A ``kind`` that is already a URL
    is taken as the whole type: the SmartArt drawing part hangs off a Microsoft
    extension namespace that is not under ``REL_BASE`` at all.
    """
    out = []
    for rid, kind, target in entries:
        if kind.startswith("http"):
            kind_url = kind
        elif root and kind == "metadata/core-properties":
            kind_url = (
                "http://schemas.openxmlformats.org/package/2006/relationships/"
                "metadata/core-properties"
            )
        else:
            kind_url = f"{REL_BASE}/{kind}"
        mode = ' TargetMode="External"' if target.startswith("http") else ""
        out.append(f'<Relationship Id="{rid}" Type="{kind_url}" Target="{target}"{mode}/>')
    return (XML_DECL + f"<Relationships {RELS_NS}>" + "".join(out) + "</Relationships>").encode()


def content_types(defaults: dict[str, str], overrides: list[tuple[str, str]]) -> bytes:
    """`[Content_Types].xml` from an extension→type map and a part→type list."""
    return (
        XML_DECL
        + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        + "".join(
            f'<Default Extension="{e}" ContentType="{t}"/>'
            for e, t in sorted(defaults.items())
        )
        + "".join(f'<Override PartName="{p}" ContentType="{c}"/>' for p, c in sorted(overrides))
        + "</Types>"
    ).encode()


def core_props(title: str, author: str = "docsai corpus", lang: str = "es-ES") -> bytes:
    return (
        XML_DECL
        + '<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" '
        'xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" '
        'xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">'
        f"<dc:title>{title}</dc:title><dc:creator>{author}</dc:creator>"
        f"<cp:lastModifiedBy>{author}</cp:lastModifiedBy>"
        f"<dc:language>{lang}</dc:language>"
        '<dcterms:created xsi:type="dcterms:W3CDTF">2026-01-01T00:00:00Z</dcterms:created>'
        '<dcterms:modified xsi:type="dcterms:W3CDTF">2026-01-02T00:00:00Z</dcterms:modified>'
        "</cp:coreProperties>"
    ).encode()


APP_PROPS = (
    XML_DECL
    + '<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">'
    "<Application>docsai-corpus</Application><Company>docsai</Company>"
    "</Properties>"
).encode()


def custom_props(props: dict[str, str]) -> bytes:
    items = []
    for i, (k, v) in enumerate(props.items(), start=2):
        items.append(
            f'<property fmtid="{{D5CDD505-2E9C-101B-9397-08002B2CF9AE}}" pid="{i}" name="{k}">'
            f'<vt:lpwstr xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">{v}</vt:lpwstr>'
            "</property>"
        )
    return (
        XML_DECL
        + '<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties">'
        + "".join(items)
        + "</Properties>"
    ).encode()


DEFAULT_STYLES = """<w:styles {ns}>
  <w:docDefaults>
    <w:rPrDefault><w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/><w:sz w:val="22"/>
    </w:rPr></w:rPrDefault>
    <w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259"/></w:pPr></w:pPrDefault>
  </w:docDefaults>
  <w:style w:type="paragraph" w:default="1" w:styleId="Normal">
    <w:name w:val="Normal"/><w:qFormat/>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="heading 1"/><w:basedOn w:val="Normal"/>
    <w:pPr><w:outlineLvl w:val="0"/><w:spacing w:before="240" w:after="80"/><w:keepNext/></w:pPr>
    <w:rPr><w:rFonts w:ascii="Calibri Light" w:hAnsi="Calibri Light"/><w:color w:val="2E74B5"/><w:sz w:val="32"/></w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Heading2">
    <w:name w:val="heading 2"/><w:basedOn w:val="Normal"/>
    <w:pPr><w:outlineLvl w:val="1"/></w:pPr>
    <w:rPr><w:color w:val="2E74B5"/><w:sz w:val="26"/></w:rPr>
  </w:style>
  <w:style w:type="character" w:styleId="Hyperlink">
    <w:name w:val="Hyperlink"/><w:rPr><w:color w:val="0563C1"/><w:u w:val="single"/></w:rPr>
  </w:style>
  <w:style w:type="table" w:styleId="TableGrid">
    <w:name w:val="Table Grid"/>
  </w:style>
</w:styles>""".format(ns=W_NS)


def build_docx(
    name: str,
    body: str,
    *,
    title: str,
    styles: str = DEFAULT_STYLES,
    numbering: str | None = None,
    media: dict[str, bytes] | None = None,
    rels: list[tuple[str, str, str]] | None = None,
    headers: dict[str, str] | None = None,
    footnotes: str | None = None,
    comments: str | None = None,
    comments_extended: str | None = None,
    custom: dict[str, str] | None = None,
) -> None:
    """Assemble a .docx package.

    ``rels`` entries are ``(rId, type_suffix, target)`` and ``headers`` maps a
    part name (``header1.xml``) to its XML body.
    """
    media = media or {}
    rels = list(rels or [])
    headers = headers or {}

    parts: dict[str, bytes] = {}
    overrides = [
        ('/word/document.xml', "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"),
        ("/word/styles.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"),
        ("/docProps/core.xml", "application/vnd.openxmlformats-package.core-properties+xml"),
        ("/docProps/app.xml", "application/vnd.openxmlformats-officedocument.extended-properties+xml"),
    ]

    parts["word/document.xml"] = (XML_DECL + body).encode()
    parts["word/styles.xml"] = (XML_DECL + styles).encode()
    parts["docProps/core.xml"] = core_props(title)
    parts["docProps/app.xml"] = APP_PROPS

    rels.append(("rIdStyles", "styles", "styles.xml"))

    if numbering is not None:
        parts["word/numbering.xml"] = (XML_DECL + numbering).encode()
        rels.append(("rIdNum", "numbering", "numbering.xml"))
        overrides.append(
            ("/word/numbering.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml")
        )

    if footnotes is not None:
        parts["word/footnotes.xml"] = (XML_DECL + footnotes).encode()
        rels.append(("rIdFn", "footnotes", "footnotes.xml"))
        overrides.append(
            ("/word/footnotes.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml")
        )

    if comments is not None:
        parts["word/comments.xml"] = (XML_DECL + comments).encode()
        rels.append(("rIdCm", "comments", "comments.xml"))
        overrides.append(
            ("/word/comments.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml")
        )

    if comments_extended is not None:
        parts["word/commentsExtended.xml"] = (XML_DECL + comments_extended).encode()
        # A Microsoft extension: its relationship type is not under the
        # ECMA-376 namespace, so the whole URI is stated.
        rels.append((
            "rIdCmEx",
            "http://schemas.microsoft.com/office/2011/relationships/commentsExtended",
            "commentsExtended.xml",
        ))
        overrides.append((
            "/word/commentsExtended.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.commentsExtended+xml",
        ))

    for part, xml in sorted(headers.items()):
        parts[f"word/{part}"] = (XML_DECL + xml).encode()
        kind = "header" if part.startswith("header") else "footer"
        overrides.append(
            (
                f"/word/{part}",
                f"application/vnd.openxmlformats-officedocument.wordprocessingml.{kind}+xml",
            )
        )

    if custom is not None:
        parts["docProps/custom.xml"] = custom_props(custom)
        overrides.append(
            ("/docProps/custom.xml", "application/vnd.openxmlformats-officedocument.custom-properties+xml")
        )

    for fname, blob in sorted(media.items()):
        parts[f"word/media/{fname}"] = blob

    exts = sorted({Path(f).suffix for f in media})
    defaults = ['<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>',
                '<Default Extension="xml" ContentType="application/xml"/>']
    defaults += [f'<Default Extension="{e[1:]}" ContentType="{CONTENT_TYPE[e]}"/>' for e in exts]

    parts["[Content_Types].xml"] = (
        XML_DECL
        + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        + "".join(defaults)
        + "".join(f'<Override PartName="{p}" ContentType="{c}"/>' for p, c in overrides)
        + "</Types>"
    ).encode()

    pkg_rels = [
        ("rId1", "officeDocument", "word/document.xml"),
        ("rId2", "metadata/core-properties", "docProps/core.xml"),
        ("rId3", "extended-properties", "docProps/app.xml"),
    ]
    if custom is not None:
        pkg_rels.append(("rId4", "custom-properties", "docProps/custom.xml"))
    pkg_rel_xml = "".join(
        f'<Relationship Id="{i}" Type="{REL_BASE}/{t}" Target="{tg}"/>' for i, t, tg in pkg_rels
    )
    # core-properties lives in a different namespace family
    pkg_rel_xml = pkg_rel_xml.replace(
        f"{REL_BASE}/metadata/core-properties",
        "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties",
    )
    parts["_rels/.rels"] = (
        XML_DECL + f"<Relationships {RELS_NS}>" + pkg_rel_xml + "</Relationships>"
    ).encode()

    doc_rel_xml = []
    for rid, kind, target in rels:
        mode = ' TargetMode="External"' if target.startswith("http") else ""
        # A relationship type is a whole URI; the shorthand is the common case,
        # not the rule (`commentsExtended` is a Microsoft extension).
        rel_type = kind if kind.startswith("http") else f"{REL_BASE}/{kind}"
        doc_rel_xml.append(
            f'<Relationship Id="{rid}" Type="{rel_type}" Target="{target}"{mode}/>'
        )
    parts["word/_rels/document.xml.rels"] = (
        XML_DECL + f"<Relationships {RELS_NS}>" + "".join(doc_rel_xml) + "</Relationships>"
    ).encode()

    write_package(ROOT / "docx" / name, parts)


# --------------------------------------------------------------------------
# Drawing helpers
# --------------------------------------------------------------------------


def blip_fill(rid: str, crop: str = "") -> str:
    return (
        f'<pic:blipFill><a:blip r:embed="{rid}"/>{crop}'
        "<a:stretch><a:fillRect/></a:stretch></pic:blipFill>"
    )


def graphic(rid: str, w: int, h: int, *, xfrm: str = "", crop: str = "", ln: str = "",
            pic_name: str = "Picture") -> str:
    if not xfrm:
        xfrm = f'<a:xfrm><a:off x="0" y="0"/><a:ext cx="{w}" cy="{h}"/></a:xfrm>'
    return (
        '<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">'
        f'<pic:pic><pic:nvPicPr><pic:cNvPr id="1" name="{pic_name}"/><pic:cNvPicPr/></pic:nvPicPr>'
        + blip_fill(rid, crop)
        + f'<pic:spPr>{xfrm}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>{ln}</pic:spPr>'
        "</pic:pic></a:graphicData></a:graphic>"
    )


def inline_image(rid: str, w: int, h: int, *, alt: str = "", title: str = "",
                 name: str = "Picture 1", doc_id: int = 1, xfrm: str = "",
                 crop: str = "", ln: str = "", hlink: str = "") -> str:
    title_attr = f' title="{title}"' if title else ""
    doc_pr = f'<wp:docPr id="{doc_id}" name="{name}" descr="{alt}"{title_attr}>{hlink}</wp:docPr>'
    return (
        "<w:r><w:drawing>"
        f'<wp:inline distT="0" distB="0" distL="0" distR="0">'
        f'<wp:extent cx="{w}" cy="{h}"/><wp:effectExtent l="0" t="0" r="0" b="0"/>'
        + doc_pr
        + '<wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr>'
        + graphic(rid, w, h, xfrm=xfrm, crop=crop, ln=ln)
        + "</wp:inline></w:drawing></w:r>"
    )


def anchor_image(
    rid: str,
    w: int,
    h: int,
    *,
    rel_h: str = "margin",
    rel_v: str = "paragraph",
    pos_h: str = "",
    pos_v: str = "",
    wrap: str = '<wp:wrapSquare wrapText="bothSides"/>',
    behind: bool = False,
    z: int = 251658240,
    alt: str = "",
    name: str = "Picture",
    doc_id: int = 10,
    xfrm: str = "",
    crop: str = "",
    ln: str = "",
) -> str:
    pos_h = pos_h or "<wp:posOffset>0</wp:posOffset>"
    pos_v = pos_v or "<wp:posOffset>0</wp:posOffset>"
    return (
        "<w:r><w:drawing>"
        f'<wp:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0" '
        f'relativeHeight="{z}" behindDoc="{1 if behind else 0}" locked="0" '
        f'layoutInCell="1" allowOverlap="1">'
        '<wp:simplePos x="0" y="0"/>'
        f'<wp:positionH relativeFrom="{rel_h}">{pos_h}</wp:positionH>'
        f'<wp:positionV relativeFrom="{rel_v}">{pos_v}</wp:positionV>'
        f'<wp:extent cx="{w}" cy="{h}"/><wp:effectExtent l="0" t="0" r="0" b="0"/>'
        f"{wrap}"
        f'<wp:docPr id="{doc_id}" name="{name}" descr="{alt}"/>'
        '<wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr>'
        + graphic(rid, w, h, xfrm=xfrm, crop=crop, ln=ln)
        + "</wp:anchor></w:drawing></w:r>"
    )


SECT_PR = (
    '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>'
    '<w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" '
    'w:header="708" w:footer="708" w:gutter="0"/></w:sectPr>'
)


def document(body: str, sect: str = SECT_PR) -> str:
    return f"<w:document {W_NS}><w:body>{body}{sect}</w:body></w:document>"


def p(runs: str, ppr: str = "") -> str:
    return f"<w:p>{ppr}{runs}</w:p>"


def r(text: str, rpr: str = "") -> str:
    return f'<w:r>{rpr}<w:t xml:space="preserve">{text}</w:t></w:r>'


# --------------------------------------------------------------------------
# DOCX corpus
# --------------------------------------------------------------------------


def docx_basic_text() -> None:
    body = (
        p(r("Primer parrafo del documento."))
        + p(r("Segundo parrafo con un salto") + "<w:r><w:br/></w:r>" + r("de linea manual."))
        + p(r("Caracteres que Markdown escapa: *asterisco* _guion_ #almohadilla "
              "|tuberia| [corchete] &lt;angulo&gt; `backtick` \\barra."))
        + p(r(""))
        + p(r("Parrafo final tras uno vacio."))
    )
    build_docx("basic-text.docx", document(body), title="Texto basico")


def docx_basic_styles() -> None:
    body = (
        p(r("Titulo de nivel 1"), '<w:pPr><w:pStyle w:val="Heading1"/></w:pPr>')
        + p(
            r("negrita", "<w:rPr><w:b/></w:rPr>")
            + r(" ")
            + r("cursiva", "<w:rPr><w:i/></w:rPr>")
            + r(" ")
            + r("tachado", "<w:rPr><w:strike/></w:rPr>")
            + r(" ")
            + r("subrayado", '<w:rPr><w:u w:val="single"/></w:rPr>')
        )
        + p(r("Titulo de nivel 2"), '<w:pPr><w:pStyle w:val="Heading2"/></w:pPr>')
        + p(
            r("rojo", '<w:rPr><w:color w:val="FF0000"/></w:rPr>')
            + r(" ")
            + r("resaltado", '<w:rPr><w:highlight w:val="yellow"/></w:rPr>')
            + r(" ")
            + r("Arial 14", '<w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial"/><w:sz w:val="28"/></w:rPr>')
            + r(" x")
            + r("2", '<w:rPr><w:vertAlign w:val="superscript"/></w:rPr>')
            + r(" H")
            + r("2", '<w:rPr><w:vertAlign w:val="subscript"/></w:rPr>')
            + r("O")
        )
        + p(
            r("Visita ")
            + '<w:hyperlink r:id="rIdLink">'
            + r("el sitio", '<w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr>')
            + "</w:hyperlink>"
            + r(" para mas informacion.")
        )
        + p(r("Parrafo centrado con sangria."),
            '<w:pPr><w:jc w:val="center"/><w:ind w:left="720" w:firstLine="360"/>'
            '<w:spacing w:before="240" w:after="120"/></w:pPr>')
    )
    build_docx(
        "basic-styles.docx",
        document(body),
        title="Estilos basicos",
        rels=[("rIdLink", "hyperlink", "https://example.com/docsai")],
    )


NUMBERING = """<w:numbering {ns}>
  <w:abstractNum w:abstractNumId="0">
    <w:multiLevelType w:val="hybridMultilevel"/>
    <w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/>
      <w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl>
    <w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2)"/>
      <w:lvlJc w:val="left"/><w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr></w:lvl>
    <w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="lowerRoman"/><w:lvlText w:val="%3."/>
      <w:lvlJc w:val="right"/><w:pPr><w:ind w:left="2160" w:hanging="180"/></w:pPr></w:lvl>
  </w:abstractNum>
  <w:abstractNum w:abstractNumId="1">
    <w:multiLevelType w:val="hybridMultilevel"/>
    <w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/>
      <w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl>
    <w:lvl w:ilvl="1"><w:numFmt w:val="bullet"/><w:lvlText w:val="o"/>
      <w:lvlJc w:val="left"/><w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr></w:lvl>
  </w:abstractNum>
  <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
  <w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
</w:numbering>""".format(ns=W_NS)


def li(text: str, num_id: int, ilvl: int) -> str:
    return p(
        r(text),
        f'<w:pPr><w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="{ilvl}"/>'
        f'<w:numId w:val="{num_id}"/></w:numPr></w:pPr>',
    )


def docx_nested_lists() -> None:
    body = (
        p(r("Lista numerada anidada:"))
        + li("Primer punto", 1, 0)
        + li("Sub-punto a", 1, 1)
        + li("Sub-sub-punto i", 1, 2)
        + li("Sub-punto b", 1, 1)
        + li("Segundo punto", 1, 0)
        + p(r("Lista de vinetas:"))
        + li("Vineta uno", 2, 0)
        + li("Vineta anidada", 2, 1)
        + li("Vineta dos", 2, 0)
        + p(r("Parrafo posterior fuera de la lista."))
    )
    styles = DEFAULT_STYLES.replace(
        "</w:styles>",
        '<w:style w:type="paragraph" w:styleId="ListParagraph">'
        '<w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/>'
        "</w:style></w:styles>",
    )
    build_docx(
        "nested-lists.docx",
        document(body),
        title="Listas anidadas",
        styles=styles,
        numbering=NUMBERING,
    )


def docx_long_report() -> None:
    """A document of realistic size, for measuring rather than for parsing.

    Every other corpus document isolates one trait and is a few hundred tokens
    long — too small to say anything about cost: on a 700-token file the outline
    is the document. This one is deliberately ordinary and long, so that
    `docsai outline` can be held to the Phase 10 budget (< 5 % of the document's
    own tokens) on something an agent would plausibly be handed.

    The prose is templated, never random: the corpus must regenerate byte for
    byte.
    """
    topics = [
        "alcance del proyecto",
        "arquitectura de la solucion",
        "modelo de datos",
        "flujo de conversion",
        "gestion de errores",
        "rendimiento medido",
        "estrategia de pruebas",
        "seguridad y permisos",
        "despliegue y operacion",
        "formacion del equipo",
        "riesgos identificados",
        "plan de trabajo",
    ]
    sentences = [
        "El equipo revisa {topic} en cada iteracion y deja constancia escrita de la decision.",
        "La documentacion de {topic} se mantiene junto al codigo para que no envejezca por separado.",
        "Cada cambio en {topic} se acompana de una prueba que falla antes y pasa despues.",
        "Los tiempos dedicados a {topic} se miden con las mismas herramientas en todos los entornos.",
        "Cuando {topic} exige una excepcion, la excepcion se escribe y se firma, nunca se asume.",
        "La revision de {topic} termina con una lista corta de acciones y un responsable por accion.",
    ]
    body = p(r("Informe tecnico de seguimiento"),
             '<w:pPr><w:pStyle w:val="Heading1"/></w:pPr>')
    body += p(r("Este informe recoge el estado de los doce frentes abiertos, "
                "con el detalle acordado en la reunion de arranque."))
    for index, topic in enumerate(topics, start=1):
        body += p(r(f"{index}. Estado de {topic}"),
                  '<w:pPr><w:pStyle w:val="Heading2"/></w:pPr>')
        # Eight paragraphs per section: the outline/document ratio is a
        # function of heading density, and a section of four short paragraphs
        # is thinner than any real report.
        for paragraph in range(8):
            text = " ".join(
                sentences[(paragraph * 2 + offset) % len(sentences)].format(topic=topic)
                for offset in range(4)
            )
            body += p(r(text))
        body += p(r(f"Conclusion de {topic}"),
                  '<w:pPr><w:pStyle w:val="Heading3"/></w:pPr>')
        body += p(r(f"El frente de {topic} avanza segun lo previsto y no requiere "
                    "decisiones fuera del equipo en este ciclo."))
    # The third level is not in DEFAULT_STYLES; without declaring it the
    # conclusions would carry a style the package never defines.
    styles = DEFAULT_STYLES.replace(
        "</w:styles>",
        '<w:style w:type="paragraph" w:styleId="Heading3">'
        '<w:name w:val="heading 3"/><w:basedOn w:val="Normal"/>'
        '<w:pPr><w:outlineLvl w:val="2"/></w:pPr>'
        '<w:rPr><w:color w:val="1F4D78"/><w:sz w:val="24"/></w:rPr>'
        "</w:style></w:styles>",
    )
    build_docx("long-report.docx", document(body), title="Informe largo",
               styles=styles)


def tc(text: str, tcpr: str = "", width: int = 3000) -> str:
    return (
        f'<w:tc><w:tcPr><w:tcW w:w="{width}" w:type="dxa"/>{tcpr}</w:tcPr>'
        + p(r(text))
        + "</w:tc>"
    )


def docx_table_simple() -> None:
    grid = '<w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>'
    rows = (
        "<w:tr>" + tc("Concepto") + tc("T1") + tc("T2") + "</w:tr>"
        "<w:tr>" + tc("Ventas") + tc("100") + tc("200") + "</w:tr>"
        "<w:tr>" + tc("Costes") + tc("40") + tc("60") + "</w:tr>"
    )
    tbl = (
        '<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/>'
        '<w:tblW w:w="9000" w:type="dxa"/></w:tblPr>' + grid + rows + "</w:tbl>"
    )
    build_docx(
        "table-simple.docx",
        document(p(r("Tabla regular:")) + tbl + p(r("Fin."))),
        title="Tabla simple",
    )


def docx_table_merged() -> None:
    grid = ('<w:tblGrid><w:gridCol w:w="2500"/><w:gridCol w:w="2500"/>'
            '<w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>')
    rows = (
        "<w:tr>"
        + tc("Region", "", 2500)
        + tc("Trimestres", '<w:gridSpan w:val="2"/>', 4500)
        + tc("Total", "", 2000)
        + "</w:tr>"
        "<w:tr>"
        + tc("Norte", '<w:vMerge w:val="restart"/>', 2500)
        + tc("100", "", 2500)
        + tc("200", "", 2000)
        + tc("300", "", 2000)
        + "</w:tr>"
        "<w:tr>"
        + tc("", '<w:vMerge/>', 2500)
        + tc("150", "", 2500)
        + tc("250", "", 2000)
        + tc("400", "", 2000)
        + "</w:tr>"
    )
    tbl = (
        '<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/>'
        '<w:tblW w:w="9000" w:type="dxa"/></w:tblPr>' + grid + rows + "</w:tbl>"
    )
    build_docx(
        "table-merged.docx",
        document(p(r("Tabla con combinaciones:")) + tbl),
        title="Tabla combinada",
    )


def docx_images_inline() -> None:
    body = (
        p(r("Imagen en linea a tamano nativo:"))
        + p(inline_image("rIdImg1", px(120), px(90), alt="Diagrama de ventas",
                         title="Figura 1", name="Diagrama"))
        + p(r("Imagen GIF en medio ") + inline_image("rIdImg2", px(48), px(32),
                                                     alt="Icono verde", doc_id=2,
                                                     name="Icono") + r(" del parrafo."))
        + p(r("Imagen EMF vectorial (no renderizable):"))
        + p(inline_image("rIdImg3", cm(4), cm(2), alt="Logo vectorial", doc_id=3, name="LogoEMF"))
    )
    build_docx(
        "images-inline.docx",
        document(body),
        title="Imagenes en linea",
        media={"image1.png": BLUE_PNG, "image2.gif": GREEN_GIF, "image3.emf": LOGO_EMF},
        rels=[
            ("rIdImg1", "image", "media/image1.png"),
            ("rIdImg2", "image", "media/image2.gif"),
            ("rIdImg3", "image", "media/image3.emf"),
        ],
    )


def docx_images_floating() -> None:
    body = (
        p(
            anchor_image(
                "rIdImg1", cm(3.5), cm(2.6),
                rel_h="margin", rel_v="paragraph",
                pos_h=f"<wp:posOffset>{cm(1.2)}</wp:posOffset>",
                pos_v=f"<wp:posOffset>{cm(0.5)}</wp:posOffset>",
                wrap='<wp:wrapSquare wrapText="right"/>',
                z=2, alt="Logo flotante cuadrado", name="Logo",
            )
            + r("Texto que rodea a la imagen flotante anclada al margen.")
        )
        + p(
            anchor_image(
                "rIdImg2", cm(6), cm(4),
                rel_h="page", rel_v="page",
                pos_h="<wp:align>center</wp:align>",
                pos_v="<wp:align>top</wp:align>",
                wrap="<wp:wrapTopAndBottom/>",
                z=3, alt="Banner centrado", name="Banner", doc_id=11,
            )
            + r("Parrafo con imagen anclada a la pagina y alineacion simbolica.")
        )
        + p(
            anchor_image(
                "rIdImg1", cm(10), cm(7.5),
                rel_h="page", rel_v="page",
                pos_h=f"<wp:posOffset>{cm(2)}</wp:posOffset>",
                pos_v=f"<wp:posOffset>{cm(8)}</wp:posOffset>",
                wrap="<wp:wrapNone/>", behind=True, z=1,
                alt="Marca de agua", name="Marca de agua", doc_id=12,
            )
            + r("Parrafo sobre la marca de agua detras del texto.")
        )
    )
    build_docx(
        "images-floating.docx",
        document(body),
        title="Imagenes flotantes",
        media={"image1.png": BLUE_PNG, "image2.png": RED_PNG},
        rels=[
            ("rIdImg1", "image", "media/image1.png"),
            ("rIdImg2", "image", "media/image2.png"),
        ],
    )


def docx_images_transformed() -> None:
    rot45 = 45 * 60000
    xfrm_rot = f'<a:xfrm rot="{rot45}"><a:off x="0" y="0"/><a:ext cx="{cm(4)}" cy="{cm(3)}"/></a:xfrm>'
    xfrm_flip = (f'<a:xfrm flipH="1" flipV="1"><a:off x="0" y="0"/>'
                 f'<a:ext cx="{cm(2)}" cy="{cm(1.5)}"/></a:xfrm>')
    crop = '<a:srcRect l="10000" t="5000" r="20000" b="0"/>'
    border = ('<a:ln w="12700"><a:solidFill><a:srgbClr val="000000"/></a:solidFill>'
              '<a:prstDash val="solid"/></a:ln>')
    body = (
        p(r("Imagen rotada 45 grados:"))
        + p(anchor_image("rIdImg1", cm(4), cm(3), xfrm=xfrm_rot,
                         alt="Rotada", name="Rotada"))
        + p(r("Imagen recortada con borde:"))
        + p(inline_image("rIdImg1", cm(3), cm(2.25), alt="Recortada", doc_id=2,
                         name="Recortada", crop=crop, ln=border))
        + p(r("Imagen volteada y escalada al 50 %:"))
        + p(anchor_image("rIdImg2", cm(2), cm(1.5), xfrm=xfrm_flip,
                         alt="Volteada", name="Volteada", doc_id=13))
    )
    build_docx(
        "images-transformed.docx",
        document(body),
        title="Imagenes transformadas",
        media={"image1.png": BLUE_PNG, "image2.png": RED_PNG},
        rels=[
            ("rIdImg1", "image", "media/image1.png"),
            ("rIdImg2", "image", "media/image2.png"),
        ],
    )


def docx_images_duplicated() -> None:
    body = (
        p(r("El mismo mapa de bits tres veces con geometrias distintas."))
        + p(inline_image("rIdA", px(120), px(90), alt="Original", name="Copia A"))
        + p(inline_image("rIdB", px(60), px(45), alt="Mitad", doc_id=2, name="Copia B"))
        + p(
            anchor_image(
                "rIdC", cm(5), cm(3.75),
                rel_h="margin", rel_v="paragraph",
                pos_h=f"<wp:posOffset>{cm(2)}</wp:posOffset>",
                wrap='<wp:wrapSquare wrapText="largest"/>',
                alt="Flotante", name="Copia C", doc_id=3,
            )
            + r("Tercera aparicion, flotante.")
        )
    )
    # Three distinct relationships and three distinct parts holding identical bytes:
    # the AssetStore must collapse them into one asset.
    build_docx(
        "images-duplicated.docx",
        document(body),
        title="Imagenes duplicadas",
        media={"image1.png": BLUE_PNG, "image2.png": BLUE_PNG, "image3.png": BLUE_PNG},
        rels=[
            ("rIdA", "image", "media/image1.png"),
            ("rIdB", "image", "media/image2.png"),
            ("rIdC", "image", "media/image3.png"),
        ],
    )


def docx_headers_footers() -> None:
    header_default = (
        f"<w:hdr {W_NS}>" + p(r("Cabecera por defecto")) + "</w:hdr>"
    )
    header_first = f"<w:hdr {W_NS}>" + p(r("Cabecera de primera pagina")) + "</w:hdr>"
    footer_default = (
        f"<w:ftr {W_NS}>"
        + p(
            r("Pagina ")
            + '<w:fldSimple w:instr=" PAGE ">' + r("1") + "</w:fldSimple>"
            + r(" de ")
            + '<w:fldSimple w:instr=" NUMPAGES ">' + r("3") + "</w:fldSimple>"
        )
        + "</w:ftr>"
    )
    sect = (
        '<w:sectPr><w:headerReference w:type="default" r:id="rIdH1"/>'
        '<w:headerReference w:type="first" r:id="rIdH2"/>'
        '<w:footerReference w:type="default" r:id="rIdF1"/>'
        '<w:titlePg/><w:pgSz w:w="11906" w:h="16838"/>'
        '<w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" '
        'w:header="708" w:footer="708" w:gutter="0"/><w:cols w:num="2"/></w:sectPr>'
    )
    body = p(r("Cuerpo del documento con cabeceras y pies."))
    build_docx(
        "headers-footers.docx",
        document(body, sect),
        title="Cabeceras y pies",
        headers={
            "header1.xml": header_default,
            "header2.xml": header_first,
            "footer1.xml": footer_default,
        },
        rels=[
            ("rIdH1", "header", "header1.xml"),
            ("rIdH2", "header", "header2.xml"),
            ("rIdF1", "footer", "footer1.xml"),
        ],
    )


def docx_footnotes() -> None:
    fn = (
        f"<w:footnotes {W_NS}>"
        '<w:footnote w:type="separator" w:id="-1">' + p(r("")) + "</w:footnote>"
        '<w:footnote w:type="continuationSeparator" w:id="0">' + p(r("")) + "</w:footnote>"
        '<w:footnote w:id="1">'
        + p('<w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr><w:footnoteRef/></w:r>'
            + r(" Primera nota al pie."))
        + "</w:footnote>"
        '<w:footnote w:id="2">'
        + p('<w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr><w:footnoteRef/></w:r>'
            + r(" Segunda nota, con ") + r("negrita", "<w:rPr><w:b/></w:rPr>") + r("."))
        + "</w:footnote>"
        "</w:footnotes>"
    )
    body = (
        p(r("Texto con nota")
          + '<w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr>'
            '<w:footnoteReference w:id="1"/></w:r>'
          + r(" y sigue."))
        + p(r("Otro parrafo con segunda nota")
            + '<w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr>'
              '<w:footnoteReference w:id="2"/></w:r>'
            + r("."))
    )
    styles = DEFAULT_STYLES.replace(
        "</w:styles>",
        '<w:style w:type="character" w:styleId="FootnoteReference">'
        '<w:name w:val="footnote reference"/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr>'
        "</w:style></w:styles>",
    )
    build_docx("footnotes.docx", document(body), title="Notas al pie",
               styles=styles, footnotes=fn)



# `w14` and `w15` are the namespaces the paragraph id and the thread live in.
# Word declares both on every part that uses them, and lists them as ignorable
# so a consumer that does not know them still opens the file.
W14_NS = (
    'xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" '
    'xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" '
    'xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml" '
    'mc:Ignorable="w14 w15"'
)


def docx_comments_review() -> None:
    """A reviewed document: `word/comments.xml` plus `word/commentsExtended.xml`
    (Phase 16-F, spec §11.3).

    Four facts the reader and the writer both have to get right, one per shape
    of the file:

    * a **commented range** — `w:commentRangeStart` … `w:commentRangeEnd` and
      the `w:commentReference` run that follows — so the anchor covers words
      rather than a point;
    * a **thread**: a reply, joined to the comment it answers by `w14:paraId`
      through `w15:paraIdParent`, which is the only place WordprocessingML
      records a thread. A reader that joined on comment id would build the
      wrong tree, or none;
    * a **resolved** thread (`w15:done="1"`), which lives in the same part and
      nowhere else;
    * a **bare `w:commentReference`** with no range around it: a comment
      anchored at a point in the text, which reads back with an empty anchor.

    The body is real prose for the same reason every other fixture's is: the
    `standard` residue budget is measured over exactly these files, and a
    document whose text is one word makes every marker look enormous."""
    body = (
        p(r("Cierre contable del tercer trimestre"), '<w:pPr><w:pStyle w:val="Heading1"/></w:pPr>')
        + p(r("Enviar la conciliacion bancaria antes del ")
            + '<w:commentRangeStart w:id="1"/>'
            + r("viernes")
            + '<w:commentRangeEnd w:id="1"/>'
            + '<w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:commentReference w:id="1"/></w:r>'
            + '<w:commentRangeStart w:id="2"/><w:commentRangeEnd w:id="2"/>'
            + '<w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:commentReference w:id="2"/></w:r>'
            + r(", porque el consejo imprime el dosier la noche anterior."))
        + p(r("La auditoria revisa el criterio de reconocimiento de ingresos y ")
            + '<w:commentRangeStart w:id="3"/>'
            + r("pide una comparativa con el mismo trimestre del ano pasado")
            + '<w:commentRangeEnd w:id="3"/>'
            + '<w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:commentReference w:id="3"/></w:r>'
            + r("."))
        + p(r("El desglose por region llega la semana que viene y ")
            + r("no cambia el resultado", '<w:rPr><w:b/></w:rPr>')
            + r(" del ejercicio."))
        + p(r("Las cifras provisionales estan ")
            + r("pendientes de confirmacion", '<w:rPr><w:i/><w:color w:val="C00000"/></w:rPr>')
            + r(" por parte de la auditoria externa contratada en marzo."),
            '<w:pPr><w:jc w:val="both"/><w:spacing w:before="120" w:after="120"/>'
            '<w:ind w:left="284" w:right="284"/></w:pPr>')
        + p(r("El comite de direccion revisara el documento el lunes por la manana, ")
            + r("antes de la sesion ordinaria", '<w:rPr><w:b/><w:u w:val="single"/></w:rPr>')
            + r(" del consejo de administracion."),
            '<w:pPr><w:jc w:val="right"/><w:spacing w:line="360" w:lineRule="auto"/></w:pPr>')
        + p(r("La tesoreria cubre catorce meses de gasto corriente al cierre del periodo.")
            + '<w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:commentReference w:id="4"/></w:r>')
    )

    def comment(cid: int, author: str, initials: str, date: str, para: str, text: str) -> str:
        return (
            f'<w:comment w:id="{cid}" w:author="{author}" w:initials="{initials}" w:date="{date}">'
            + f'<w:p w14:paraId="{para}" w14:textId="{para}">'
            + '<w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:annotationRef/></w:r>'
            + r(text)
            + "</w:p></w:comment>"
        )

    comments = (
        f"<w:comments {W_NS} {W14_NS}>"
        + comment(1, "Ada Lovelace", "AL", "2026-05-04T09:12:00Z", "1A2B3C01",
                  "El jueves es la fecha real: el dosier se imprime esa noche.")
        + comment(2, "Grace Hopper", "GH", "2026-05-04T10:01:00Z", "1A2B3C02",
                  "De acuerdo, lo muevo al jueves y aviso a tesoreria.")
        + comment(3, "Ada Lovelace", "AL", "2026-05-04T10:20:00Z", "1A2B3C03",
                  "La comparativa ya esta preparada desde el lunes.")
        + comment(4, "Grace Hopper", "GH", "2026-05-04T11:05:00Z", "1A2B3C04",
                  "Confirmar esta cifra con el equipo de tesoreria.")
        + "</w:comments>"
    )
    extended = (
        f"<w15:commentsEx {W_NS} {W14_NS}>"
        '<w15:commentEx w15:paraId="1A2B3C01" w15:done="0"/>'
        '<w15:commentEx w15:paraId="1A2B3C02" w15:paraIdParent="1A2B3C01" w15:done="0"/>'
        '<w15:commentEx w15:paraId="1A2B3C03" w15:done="1"/>'
        '<w15:commentEx w15:paraId="1A2B3C04" w15:done="0"/>'
        "</w15:commentsEx>"
    )
    styles = DEFAULT_STYLES.replace(
        "</w:styles>",
        '<w:style w:type="character" w:styleId="CommentReference">'
        '<w:name w:val="annotation reference"/><w:rPr><w:sz w:val="16"/></w:rPr>'
        "</w:style></w:styles>",
    )
    build_docx("comments-review.docx", document(body), title="Revision del cierre",
               styles=styles, comments=comments, comments_extended=extended)


CUSTOM_STYLES = DEFAULT_STYLES.replace(
    "</w:styles>",
    """<w:style w:type="paragraph" w:styleId="Destacado">
    <w:name w:val="Destacado"/><w:basedOn w:val="Normal"/><w:qFormat/>
    <w:pPr><w:jc w:val="center"/><w:spacing w:before="120" w:after="120"/>
      <w:shd w:val="clear" w:fill="F2F2F2"/></w:pPr>
    <w:rPr><w:i/><w:color w:val="C00000"/><w:sz w:val="24"/></w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="DestacadoFuerte">
    <w:name w:val="Destacado fuerte"/><w:basedOn w:val="Destacado"/>
    <w:rPr><w:b/></w:rPr>
  </w:style>
  <w:style w:type="character" w:styleId="Enfatico">
    <w:name w:val="Enfatico"/><w:rPr><w:i/><w:color w:val="C00000"/></w:rPr>
  </w:style></w:styles>""",
)


def docx_custom_styles() -> None:
    body = (
        p(r("Parrafo normal sin atributos."))
        + p(r("Parrafo con estilo Destacado."), '<w:pPr><w:pStyle w:val="Destacado"/></w:pPr>')
        + p(r("Estilo heredado con delta directo."),
            '<w:pPr><w:pStyle w:val="DestacadoFuerte"/><w:jc w:val="right"/></w:pPr>')
        + p(r("Texto con ") + r("estilo de caracter", '<w:rPr><w:rStyle w:val="Enfatico"/></w:rPr>')
            + r(" y delta ")
            + r("adicional", '<w:rPr><w:rStyle w:val="Enfatico"/><w:b/></w:rPr>')
            + r("."))
    )
    build_docx(
        "custom-styles.docx",
        document(body),
        title="Estilos personalizados",
        styles=CUSTOM_STYLES,
        custom={"Departamento": "Ventas", "Revision": "3"},
    )


REDUNDANT_STYLES = DEFAULT_STYLES.replace(
    "</w:styles>",
    """<w:style w:type="paragraph" w:styleId="Resaltado">
    <w:name w:val="Resaltado"/><w:basedOn w:val="Normal"/>
    <w:pPr><w:jc w:val="center"/><w:spacing w:before="120"/></w:pPr>
    <w:rPr><w:color w:val="C00000"/><w:sz w:val="28"/></w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="ResaltadoHijo">
    <w:name w:val="Resaltado hijo"/><w:basedOn w:val="Resaltado"/>
    <w:pPr><w:jc w:val="center"/></w:pPr>
    <w:rPr><w:color w:val="C00000"/><w:sz w:val="28"/><w:b/></w:rPr>
  </w:style></w:styles>""",
)


def docx_redundant_formatting() -> None:
    """Formatting the inheritance chain already implies, at every level.

    Word writes this constantly: text typed after applying a style carries the
    style's own run properties again in `w:rPr`, and a derived style repeats
    what it inherits. None of it should reach the DocMark (spec 3.1).
    """
    style_rpr = '<w:rPr><w:color w:val="C00000"/><w:sz w:val="28"/></w:rPr>'
    body = (
        # A heading whose style the `#` level already names.
        p(r("Titulo con estilo implicito"), '<w:pPr><w:pStyle w:val="Heading1"/></w:pPr>')
        # Runs repeating what the paragraph style says.
        + p(
            r("Todo esto ya lo dice el estilo.", style_rpr),
            '<w:pPr><w:pStyle w:val="Resaltado"/></w:pPr>',
        )
        # Paragraph properties repeating what the paragraph style says.
        + p(
            r("Y esto lo dice dos veces.", style_rpr),
            '<w:pPr><w:pStyle w:val="Resaltado"/><w:jc w:val="center"/>'
            '<w:spacing w:before="120"/></w:pPr>',
        )
        # Inherited through two levels, plus one real delta (italic).
        + p(
            r("Heredado dos niveles", '<w:rPr><w:color w:val="C00000"/><w:sz w:val="28"/>'
              "<w:b/><w:i/></w:rPr>")
            + r(" y una diferencia real.", '<w:rPr><w:color w:val="C00000"/><w:sz w:val="28"/>'
                "<w:b/></w:rPr>"),
            '<w:pPr><w:pStyle w:val="ResaltadoHijo"/></w:pPr>',
        )
    )
    build_docx(
        "redundant-formatting.docx",
        document(body),
        title="Formato redundante",
        styles=REDUNDANT_STYLES,
    )


def docx_repeated_formatting() -> None:
    """The same direct formatting, applied by hand over and over.

    What `redundant-formatting.docx` carries is redundancy against a style;
    this is the other half: formatting that no style implies, so the delta
    emission of spec 3.1 has to keep every copy of it. It is what a document
    written without styles looks like — an author who colours each term and
    indents each note by hand — and it is what the attribute-set dictionary
    (spec 3.7) exists to compress: two patterns, twelve nodes carrying them.
    """
    term = (
        '<w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas"/>'
        '<w:color w:val="1F4E79"/><w:sz w:val="24"/></w:rPr>'
    )
    note_ppr = '<w:pPr><w:ind w:left="720"/><w:spacing w:before="120" w:after="120"/></w:pPr>'
    entries = [
        ("convert", "transforma un documento al formato pivote."),
        ("inspect", "describe la estructura sin convertir nada."),
        ("outline", "devuelve el mapa de nodos direccionables."),
        ("tokens", "mide lo que cuesta leer el documento."),
        ("search", "localiza texto y devuelve identificadores."),
        ("roundtrip", "comprueba la identidad de ida y vuelta."),
    ]
    body = ""
    for name, description in entries:
        body += p(r(name, term) + r(f" {description}"))
        body += p(r(f"Nota sobre {name}: sin estilo, con sangria manual."), note_ppr)
    build_docx(
        "repeated-formatting.docx",
        document(body),
        title="Formato repetido",
    )


def docx_images_vml() -> None:
    """Legacy VML picture (`w:pict`), typical of documents converted from .doc."""
    pict = (
        "<w:r><w:pict>"
        '<v:shape id="_x0000_s1026" type="#_x0000_t75" alt="Imagen VML heredada" '
        'style="position:absolute;margin-left:10pt;margin-top:5pt;width:90pt;height:67.5pt;'
        'z-index:1;mso-position-horizontal-relative:margin;'
        'mso-position-vertical-relative:paragraph">'
        '<v:imagedata r:id="rIdVml" o:title="Heredada"/>'
        '<w10:wrap type="square" side="right"/>'
        "</v:shape>"
        "</w:pict></w:r>"
    )
    body = p(pict + r("Parrafo con imagen VML heredada."))
    build_docx(
        "images-vml.docx",
        document(body),
        title="Imagen VML",
        media={"image1.png": RED_PNG},
        rels=[("rIdVml", "image", "media/image1.png")],
    )


def docx_fields_raw() -> None:
    """Fields and structures with no DocMark representation (raw-block path)."""
    sdt = (
        "<w:sdt><w:sdtPr><w:alias w:val=\"Bloque estructurado\"/>"
        '<w:id w:val="1234"/><w:text/></w:sdtPr>'
        "<w:sdtContent>" + p(r("Contenido dentro de un control de contenido.")) + "</w:sdtContent>"
        "</w:sdt>"
    )
    toc = (
        p('<w:r><w:fldChar w:fldCharType="begin"/></w:r>'
          '<w:r><w:instrText xml:space="preserve"> TOC \\o "1-3" \\h </w:instrText></w:r>'
          '<w:r><w:fldChar w:fldCharType="separate"/></w:r>'
          + r("Tabla de contenido generada")
          + '<w:r><w:fldChar w:fldCharType="end"/></w:r>')
    )
    # OMML: block-level maths, which the IR does not model. It travels as a
    # raw-block, which is what this fixture exists to exercise end to end.
    math = (
        '<m:oMathPara xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">'
        "<m:oMath>"
        "<m:sSup><m:e><m:r><m:t>E</m:t></m:r></m:e>"
        "<m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup>"
        "<m:r><m:t> = m c</m:t></m:r>"
        "</m:oMath></m:oMathPara>"
    )
    body = (
        p(r("Antes del control de contenido."))
        + sdt
        + math
        + toc
        + p(r("Fecha: ") + '<w:fldSimple w:instr=" DATE \\@ &quot;dd/MM/yyyy&quot; ">'
            + r("01/01/2026") + "</w:fldSimple>")
        + p(r("Despues."))
    )
    build_docx("fields-raw.docx", document(body), title="Campos y bloques opacos")


# --------------------------------------------------------------------------
# XLSX corpus (consumed from Fase 3; generated now so the corpus is complete)
# --------------------------------------------------------------------------

S_NS = 'xmlns="http://schemas.spreadsheetml.org/2006/main"'
X_NS = (
    'xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
    'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"'
)

BASE_XL_STYLES = f"""<styleSheet {X_NS}>
  <numFmts count="3">
    <numFmt numFmtId="164" formatCode="#,##0.00\\ &quot;EUR&quot;"/>
    <numFmt numFmtId="165" formatCode="dd/mm/yyyy"/>
    <numFmt numFmtId="166" formatCode="0.0%"/>
  </numFmts>
  <fonts count="2">
    <font><sz val="11"/><name val="Calibri"/></font>
    <font><b/><sz val="11"/><color rgb="FFFFFFFF"/><name val="Calibri"/></font>
  </fonts>
  <fills count="3">
    <fill><patternFill patternType="none"/></fill>
    <fill><patternFill patternType="gray125"/></fill>
    <fill><patternFill patternType="solid"><fgColor rgb="FF2E74B5"/></patternFill></fill>
  </fills>
  <borders count="2">
    <border><left/><right/><top/><bottom/><diagonal/></border>
    <border><left style="thin"/><right style="thin"/><top style="thin"/><bottom style="thin"/><diagonal/></border>
  </borders>
  <cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
  <cellXfs count="6">
    <xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>
    <xf numFmtId="164" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>
    <xf numFmtId="165" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>
    <xf numFmtId="166" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>
    <xf numFmtId="0" fontId="1" fillId="2" borderId="1" xfId="0" applyFont="1" applyFill="1" applyBorder="1"/>
    <xf numFmtId="3" fontId="0" fillId="0" borderId="1" xfId="0" applyNumberFormat="1" applyBorder="1"/>
  </cellXfs>
  <cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>
</styleSheet>"""


def build_xlsx(
    name: str,
    sheets: list[tuple[str, str]],
    *,
    title: str,
    shared: list[str] | None = None,
    defined_names: str = "",
    media: dict[str, bytes] | None = None,
    drawings: dict[str, str] | None = None,
    sheet_rels: dict[int, list[tuple[str, str, str]]] | None = None,
) -> None:
    media = media or {}
    drawings = drawings or {}
    sheet_rels = sheet_rels or {}
    parts: dict[str, bytes] = {}
    overrides = [
        ("/xl/workbook.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"),
        ("/xl/styles.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"),
        ("/docProps/core.xml", "application/vnd.openxmlformats-package.core-properties+xml"),
        ("/docProps/app.xml", "application/vnd.openxmlformats-officedocument.extended-properties+xml"),
    ]

    sheet_tags = "".join(
        f'<sheet name="{sname}" sheetId="{i}" r:id="rIdSheet{i}"/>'
        for i, (sname, _) in enumerate(sheets, start=1)
    )
    parts["xl/workbook.xml"] = (
        XML_DECL
        + f"<workbook {X_NS}><sheets>{sheet_tags}</sheets>{defined_names}</workbook>"
    ).encode()
    parts["xl/styles.xml"] = (XML_DECL + BASE_XL_STYLES).encode()
    parts["docProps/core.xml"] = core_props(title)
    parts["docProps/app.xml"] = APP_PROPS

    wb_rels = [
        (f"rIdSheet{i}", "worksheet", f"worksheets/sheet{i}.xml")
        for i in range(1, len(sheets) + 1)
    ]
    wb_rels.append(("rIdStyles", "styles", "styles.xml"))

    for i, (_, xml) in enumerate(sheets, start=1):
        parts[f"xl/worksheets/sheet{i}.xml"] = (XML_DECL + xml).encode()
        overrides.append(
            (
                f"/xl/worksheets/sheet{i}.xml",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml",
            )
        )
        if i in sheet_rels:
            rel_xml = "".join(
                f'<Relationship Id="{rid}" Type="{REL_BASE}/{kind}" Target="{tgt}"/>'
                for rid, kind, tgt in sheet_rels[i]
            )
            parts[f"xl/worksheets/_rels/sheet{i}.xml.rels"] = (
                XML_DECL + f"<Relationships {RELS_NS}>" + rel_xml + "</Relationships>"
            ).encode()

    if shared:
        items = "".join(f"<si><t>{s}</t></si>" for s in shared)
        parts["xl/sharedStrings.xml"] = (
            XML_DECL
            + f'<sst {X_NS} count="{len(shared)}" uniqueCount="{len(shared)}">{items}</sst>'
        ).encode()
        wb_rels.append(("rIdSst", "sharedStrings", "sharedStrings.xml"))
        overrides.append(
            ("/xl/sharedStrings.xml",
             "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml")
        )

    for dname, dxml in sorted(drawings.items()):
        parts[f"xl/drawings/{dname}"] = (XML_DECL + dxml).encode()
        overrides.append(
            (f"/xl/drawings/{dname}", "application/vnd.openxmlformats-officedocument.drawing+xml")
        )

    if media:
        rel_xml = "".join(
            f'<Relationship Id="rIdMedia{n}" Type="{REL_BASE}/image" Target="../media/{f}"/>'
            for n, f in enumerate(sorted(media), start=1)
        )
        parts["xl/drawings/_rels/drawing1.xml.rels"] = (
            XML_DECL + f"<Relationships {RELS_NS}>" + rel_xml + "</Relationships>"
        ).encode()
        for fname, blob in sorted(media.items()):
            parts[f"xl/media/{fname}"] = blob

    exts = sorted({Path(f).suffix for f in media})
    defaults = ['<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>',
                '<Default Extension="xml" ContentType="application/xml"/>']
    defaults += [f'<Default Extension="{e[1:]}" ContentType="{CONTENT_TYPE[e]}"/>' for e in exts]

    parts["[Content_Types].xml"] = (
        XML_DECL
        + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        + "".join(defaults)
        + "".join(f'<Override PartName="{p_}" ContentType="{c}"/>' for p_, c in overrides)
        + "</Types>"
    ).encode()

    parts["xl/_rels/workbook.xml.rels"] = (
        XML_DECL
        + f"<Relationships {RELS_NS}>"
        + "".join(
            f'<Relationship Id="{rid}" Type="{REL_BASE}/{kind}" Target="{tgt}"/>'
            for rid, kind, tgt in wb_rels
        )
        + "</Relationships>"
    ).encode()

    parts["_rels/.rels"] = (
        XML_DECL
        + f"<Relationships {RELS_NS}>"
        + f'<Relationship Id="rId1" Type="{REL_BASE}/officeDocument" Target="xl/workbook.xml"/>'
        + '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/'
        'relationships/metadata/core-properties" Target="docProps/core.xml"/>'
        + f'<Relationship Id="rId3" Type="{REL_BASE}/extended-properties" Target="docProps/app.xml"/>'
        + "</Relationships>"
    ).encode()

    write_package(ROOT / "xlsx" / name, parts)


def sheet(rows: str, extra: str = "", dim: str = "A1:D10") -> str:
    return (
        f'<worksheet {X_NS}><dimension ref="{dim}"/>'
        '<sheetViews><sheetView workbookViewId="0"/></sheetViews>'
        f"<sheetData>{rows}</sheetData>{extra}</worksheet>"
    )


def row(idx: int, cells: str) -> str:
    return f'<row r="{idx}">{cells}</row>'


def c(ref: str, value: str = "", *, t: str = "", s: int = 0, f: str = "") -> str:
    attrs = f' r="{ref}"'
    if t:
        attrs += f' t="{t}"'
    if s:
        attrs += f' s="{s}"'
    inner = (f"<f>{f}</f>" if f else "") + (f"<v>{value}</v>" if value != "" else "")
    return f"<c{attrs}>{inner}</c>"


def xlsx_values_types() -> None:
    rows = (
        row(1, c("A1", "0", t="s", s=4) + c("B1", "1", t="s", s=4))
        + row(2, c("A2", "2", t="s") + c("B2", "42"))
        + row(3, c("A3", "3", t="s") + c("B3", "3.14159"))
        + row(4, c("A4", "4", t="s") + c("B4", "1", t="b"))
        + row(5, c("A5", "5", t="s") + c("B5", "#DIV/0!", t="e"))
        + row(6, c("A6", "6", t="s") + c("B6", "45658", s=2))
        + row(7, c("A7", "7", t="s") + '<c r="B7" t="inlineStr"><is><t>en linea</t></is></c>')
    )
    build_xlsx(
        "values-types.xlsx",
        [("Tipos", sheet(rows, dim="A1:B7"))],
        title="Tipos de valor",
        shared=["Concepto", "Valor", "Entero", "Decimal", "Booleano", "Error", "Fecha", "Cadena"],
    )


def xlsx_formulas_basic() -> None:
    rows = (
        row(1, c("A1", "0", t="s", s=4) + c("B1", "1", t="s", s=4) + c("C1", "2", t="s", s=4))
        + row(2, c("A2", "3", t="s") + c("B2", "100") + c("C2", "200"))
        + row(3, c("A3", "4", t="s") + c("B3", "150") + c("C3", "250"))
        + row(4, c("A4", "5", t="s") + c("B4", "250", f="SUM(B2:B3)")
              + c("C4", "450", f="SUM(C2:C3)"))
        + row(5, c("A5", "6", t="s") + c("B5", "700", f="B4+C4"))
    )
    build_xlsx(
        "formulas-basic.xlsx",
        [("Datos", sheet(rows, dim="A1:C5"))],
        title="Formulas basicas",
        shared=["Producto", "T1", "T2", "Widgets", "Gadgets", "Total", "Suma general"],
        defined_names='<definedNames><definedName name="TOTAL_ANUAL">Datos!$B$5</definedName></definedNames>',
    )


def xlsx_formulas_shared() -> None:
    rows = (
        row(1, c("A1", "10") + c("B1", "20")
            + '<c r="C1"><f t="shared" ref="C1:C3" si="0">A1+B1</f><v>30</v></c>')
        + row(2, c("A2", "11") + c("B2", "21")
              + '<c r="C2"><f t="shared" si="0"/><v>32</v></c>')
        + row(3, c("A3", "12") + c("B3", "22")
              + '<c r="C3"><f t="shared" si="0"/><v>34</v></c>')
        + row(4, '<c r="A4"><f t="array" ref="A4">SUM(A1:A3*B1:B3)</f><v>1274</v></c>')
    )
    build_xlsx(
        "formulas-shared.xlsx",
        [("Compartidas", sheet(rows, dim="A1:C4"))],
        title="Formulas compartidas y de matriz",
    )


def xlsx_number_formats() -> None:
    rows = (
        row(1, c("A1", "0", t="s", s=4) + c("B1", "1", t="s", s=4))
        + row(2, c("A2", "2", t="s") + c("B2", "1234.5", s=1))
        + row(3, c("A3", "3", t="s") + c("B3", "45658", s=2))
        + row(4, c("A4", "4", t="s") + c("B4", "0.175", s=3))
        + row(5, c("A5", "5", t="s") + c("B5", "1234567", s=5))
    )
    build_xlsx(
        "number-formats.xlsx",
        [("Formatos", sheet(rows, dim="A1:B5"))],
        title="Formatos de numero",
        shared=["Concepto", "Valor", "Moneda", "Fecha", "Porcentaje", "Millares"],
    )


def xlsx_merged_cells() -> None:
    rows = (
        row(1, c("A1", "0", t="s", s=4) + c("B1", "") + c("C1", "1", t="s", s=4))
        + row(2, c("A2", "2", t="s") + c("B2", "100") + c("C2", "200"))
        + row(3, c("A3", "3", t="s") + c("B3", "150") + c("C3", "250"))
    )
    extra = (
        '<mergeCells count="2">'
        '<mergeCell ref="A1:B1"/><mergeCell ref="A2:A3"/>'
        "</mergeCells>"
    )
    cols = '<cols><col min="1" max="1" width="18.5" customWidth="1"/></cols>'
    body = sheet(rows, extra, dim="A1:C3").replace("<sheetData>", cols + "<sheetData>")
    build_xlsx(
        "merged-cells.xlsx",
        [("Combinadas", body)],
        title="Celdas combinadas",
        shared=["Cabecera combinada", "Total", "Norte", "Sur"],
    )


def xlsx_images_anchored() -> None:
    xdr = (
        'xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" '
        'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"'
    )

    def pic(rid: str, ident: int, name: str, alt: str, w: int = 0, h: int = 0) -> str:
        ext = f'<a:ext cx="{w}" cy="{h}"/>' if w else ""
        return (
            f'<xdr:pic><xdr:nvPicPr><xdr:cNvPr id="{ident}" name="{name}" descr="{alt}"/>'
            "<xdr:cNvPicPr/></xdr:nvPicPr>"
            f'<xdr:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill>'
            f'<xdr:spPr><a:xfrm><a:off x="0" y="0"/>{ext}</a:xfrm>'
            '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic>'
        )

    drawing = (
        f"<xdr:wsDr {xdr}>"
        "<xdr:twoCellAnchor editAs=\"oneCell\">"
        f'<xdr:from><xdr:col>1</xdr:col><xdr:colOff>{px(12)}</xdr:colOff>'
        f'<xdr:row>1</xdr:row><xdr:rowOff>{px(3)}</xdr:rowOff></xdr:from>'
        "<xdr:to><xdr:col>3</xdr:col><xdr:colOff>0</xdr:colOff>"
        "<xdr:row>7</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>"
        + pic("rIdMedia1", 1, "Logo", "Logo de la empresa")
        + "<xdr:clientData/></xdr:twoCellAnchor>"
        "<xdr:oneCellAnchor>"
        "<xdr:from><xdr:col>5</xdr:col><xdr:colOff>0</xdr:colOff>"
        "<xdr:row>19</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>"
        f'<xdr:ext cx="{px(180)}" cy="{px(60)}"/>'
        + pic("rIdMedia2", 2, "Firma", "Firma", px(180), px(60))
        + "<xdr:clientData/></xdr:oneCellAnchor>"
        "<xdr:absoluteAnchor>"
        f'<xdr:pos x="{cm(5)}" y="{cm(8)}"/><xdr:ext cx="{cm(10)}" cy="{cm(7.5)}"/>'
        + pic("rIdMedia1", 3, "Marca de agua", "Marca de agua", cm(10), cm(7.5))
        + "<xdr:clientData/></xdr:absoluteAnchor>"
        "</xdr:wsDr>"
    )
    rows = row(1, c("A1", "0", t="s", s=4)) + row(2, c("A2", "1", t="s"))
    body = sheet(rows, '<drawing r:id="rIdDraw"/>', dim="A1:A2")
    build_xlsx(
        "images-anchored.xlsx",
        [("Hoja", body)],
        title="Imagenes ancladas",
        shared=["Hoja con imagenes", "Contenido"],
        media={"image1.png": BLUE_PNG, "image2.gif": GREEN_GIF},
        drawings={"drawing1.xml": drawing},
        sheet_rels={1: [("rIdDraw", "drawing", "../drawings/drawing1.xml")]},
    )


# --------------------------------------------------------------------------


# --------------------------------------------------------------------------
# ODF corpus (Phase 4)
# --------------------------------------------------------------------------

ODF_NS = (
    'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
    'xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" '
    'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" '
    'xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" '
    'xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" '
    'xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" '
    'xmlns:xlink="http://www.w3.org/1999/xlink" '
    'xmlns:dc="http://purl.org/dc/elements/1.1/" '
    'xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" '
    'xmlns:number="urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0" '
    'xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" '
    'xmlns:chart="urn:oasis:names:tc:opendocument:xmlns:chart:1.0" '
    'xmlns:dr3d="urn:oasis:names:tc:opendocument:xmlns:dr3d:1.0" '
    'xmlns:math="http://www.w3.org/1998/Math/MathML" '
    'xmlns:form="urn:oasis:names:tc:opendocument:xmlns:form:1.0" '
    'xmlns:script="urn:oasis:names:tc:opendocument:xmlns:script:1.0" '
    'xmlns:config="urn:oasis:names:tc:opendocument:xmlns:config:1.0" '
    'xmlns:ooo="http://openoffice.org/2004/office" '
    'xmlns:ooow="http://openoffice.org/2004/writer" '
    'xmlns:oooc="http://openoffice.org/2004/calc" '
    'xmlns:dom="http://www.w3.org/2001/xml-events" '
    'xmlns:xforms="http://www.w3.org/2002/xforms" '
    'xmlns:xsd="http://www.w3.org/2001/XMLSchema" '
    'xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"'
)

MIME_ODT = b"application/vnd.oasis.opendocument.text"
MIME_ODS = b"application/vnd.oasis.opendocument.spreadsheet"


def odf_manifest(entries: list[tuple[str, str]]) -> bytes:
    lines = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        '<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" '
        'manifest:version="1.3">',
    ]
    for path, media in entries:
        lines.append(
            f'<manifest:file-entry manifest:full-path="{path}" manifest:media-type="{media}"/>'
        )
    lines.append("</manifest:manifest>")
    return ("\n".join(lines) + "\n").encode()


def odf_meta(title: str, *, author: str = "docsai corpus", lang: str = "es-ES") -> bytes:
    return (
        f'<?xml version="1.0" encoding="UTF-8"?>'
        f"<office:document-meta {ODF_NS} office:version=\"1.3\">"
        f"<office:meta>"
        f"<dc:title>{title}</dc:title>"
        f"<meta:initial-creator>{author}</meta:initial-creator>"
        f"<dc:creator>{author}</dc:creator>"
        f"<dc:language>{lang}</dc:language>"
        f"<meta:creation-date>2026-01-01T00:00:00Z</meta:creation-date>"
        f"<dc:date>2026-01-02T00:00:00Z</dc:date>"
        f"<meta:generator>docsai-corpus</meta:generator>"
        f"</office:meta></office:document-meta>"
    ).encode()


def odf_styles_odt(
    *,
    extra_styles: str = "",
    page_width: str = "21.001cm",
    page_height: str = "29.7cm",
    margin: str = "2cm",
    header: str = "",
    footer: str = "",
) -> bytes:
    header_xml = f"<style:header>{header}</style:header>" if header else ""
    footer_xml = f"<style:footer>{footer}</style:footer>" if footer else ""
    return (
        f'<?xml version="1.0" encoding="UTF-8"?>'
        f"<office:document-styles {ODF_NS} office:version=\"1.3\">"
        f"<office:font-face-decls>"
        f'<style:font-face style:name="Liberation Serif" svg:font-family="&apos;Liberation Serif&apos;"/>'
        f'<style:font-face style:name="Liberation Sans" svg:font-family="&apos;Liberation Sans&apos;"/>'
        f"</office:font-face-decls>"
        f"<office:styles>"
        f'<style:default-style style:family="paragraph">'
        f'<style:paragraph-properties fo:line-height="115%" style:tab-stop-distance="1.251cm"/>'
        f'<style:text-properties style:font-name="Liberation Serif" fo:font-size="12pt"/>'
        f"</style:default-style>"
        f'<style:style style:name="Standard" style:family="paragraph" style:class="text"/>'
        f'<style:style style:name="Heading" style:family="paragraph" style:parent-style-name="Standard" '
        f'style:next-style-name="Standard" style:class="text">'
        f'<style:paragraph-properties fo:margin-top="0.423cm" fo:margin-bottom="0.212cm" fo:keep-with-next="always"/>'
        f'<style:text-properties style:font-name="Liberation Sans" fo:font-size="14pt"/>'
        f"</style:style>"
        f'<style:style style:name="Heading_20_1" style:display-name="Heading 1" style:family="paragraph" '
        f'style:parent-style-name="Heading" style:next-style-name="Standard" '
        f'style:default-outline-level="1" style:class="text">'
        f'<style:text-properties fo:font-size="16pt" fo:font-weight="bold" fo:color="#2E74B5"/>'
        f"</style:style>"
        f'<style:style style:name="Heading_20_2" style:display-name="Heading 2" style:family="paragraph" '
        f'style:parent-style-name="Heading" style:default-outline-level="2" style:class="text">'
        f'<style:text-properties fo:font-size="14pt" fo:font-weight="bold" fo:color="#2E74B5"/>'
        f"</style:style>"
        f'<style:style style:name="Emphasis" style:family="text">'
        f'<style:text-properties fo:font-style="italic"/>'
        f"</style:style>"
        f'<style:style style:name="Strong_20_Emphasis" style:display-name="Strong Emphasis" style:family="text">'
        f'<style:text-properties fo:font-weight="bold"/>'
        f"</style:style>"
        f"{extra_styles}"
        f"</office:styles>"
        f"<office:automatic-styles>"
        f'<style:page-layout style:name="Mpm1">'
        f'<style:page-layout-properties fo:page-width="{page_width}" fo:page-height="{page_height}" '
        f'style:print-orientation="portrait" fo:margin-top="{margin}" fo:margin-bottom="{margin}" '
        f'fo:margin-left="{margin}" fo:margin-right="{margin}"/>'
        f"</style:page-layout>"
        f"</office:automatic-styles>"
        f"<office:master-styles>"
        f'<style:master-page style:name="Standard" style:page-layout-name="Mpm1">'
        f"{header_xml}{footer_xml}"
        f"</style:master-page>"
        f"</office:master-styles>"
        f"</office:document-styles>"
    ).encode()


def build_odt(
    name: str,
    body: str,
    *,
    title: str,
    auto_styles: str = "",
    styles_extra: str = "",
    pictures: dict[str, bytes] | None = None,
    header: str = "",
    footer: str = "",
) -> None:
    pictures = pictures or {}
    content = (
        f'<?xml version="1.0" encoding="UTF-8"?>'
        f"<office:document-content {ODF_NS} office:version=\"1.3\">"
        f"<office:scripts/>"
        f"<office:font-face-decls>"
        f'<style:font-face style:name="Liberation Serif" svg:font-family="&apos;Liberation Serif&apos;"/>'
        f"</office:font-face-decls>"
        f"<office:automatic-styles>{auto_styles}</office:automatic-styles>"
        f"<office:body><office:text>"
        f'<text:sequence-decls>'
        f'<text:sequence-decl text:display-outline-level="0" text:name="Illustration"/>'
        f"</text:sequence-decls>"
        f"{body}"
        f"</office:text></office:body></office:document-content>"
    ).encode()
    parts: dict[str, bytes] = {
        "mimetype": MIME_ODT,
        "content.xml": content,
        "styles.xml": odf_styles_odt(extra_styles=styles_extra, header=header, footer=footer),
        "meta.xml": odf_meta(title),
        "settings.xml": odf_settings(ODF_NS),
    }
    manifest_entries = [
        ("/", "application/vnd.oasis.opendocument.text"),
        ("content.xml", "text/xml"),
        ("styles.xml", "text/xml"),
        ("meta.xml", "text/xml"),
        ("settings.xml", "text/xml"),
    ]
    for pic_name, data in sorted(pictures.items()):
        parts[f"Pictures/{pic_name}"] = data
        ext = Path(pic_name).suffix.lower()
        media = CONTENT_TYPE.get(ext, "application/octet-stream")
        manifest_entries.append((f"Pictures/{pic_name}", media))
    parts["META-INF/manifest.xml"] = odf_manifest(manifest_entries)
    write_odf_package(ROOT / "odt" / name, parts)


def odt_p(text: str, style: str = "Standard", inner: str | None = None) -> str:
    body = inner if inner is not None else text
    return f'<text:p text:style-name="{style}">{body}</text:p>'


def odt_h(text: str, level: int = 1, style: str | None = None) -> str:
    style = style or f"Heading_20_{level}"
    return f'<text:h text:style-name="{style}" text:outline-level="{level}">{text}</text:h>'


def odt_span(text: str, style: str) -> str:
    return f'<text:span text:style-name="{style}">{text}</text:span>'


def odt_basic_text() -> None:
    body = (
        odt_p("Primer parrafo del documento.")
        + odt_p("Segundo parrafo con un salto<text:line-break/>de linea manual.")
        + odt_p(
            "Caracteres que Markdown escapa: *asterisco* _guion_ #almohadilla "
            "|tuberia| [corchete] &lt;angulo&gt; `backtick` \\barra."
        )
        + odt_p("")
        + odt_p("Parrafo final tras uno vacio.")
    )
    build_odt("basic-text.odt", body, title="Texto basico")


def odt_basic_styles() -> None:
    auto = (
        '<style:style style:name="P1" style:family="paragraph" style:parent-style-name="Standard">'
        '<style:paragraph-properties fo:text-align="center"/>'
        "</style:style>"
        '<style:style style:name="T1" style:family="text">'
        '<style:text-properties fo:font-weight="bold"/>'
        "</style:style>"
        '<style:style style:name="T2" style:family="text" style:parent-style-name="Emphasis">'
        '<style:text-properties fo:font-weight="bold"/>'
        "</style:style>"
        '<style:style style:name="T3" style:family="text">'
        '<style:text-properties fo:color="#C00000" style:text-underline-style="solid"/>'
        "</style:style>"
    )
    body = (
        odt_h("Titulo principal", 1)
        + odt_h("Subtitulo", 2)
        + odt_p(
            "",
            inner=(
                "Texto con "
                + odt_span("negrita", "T1")
                + ", "
                + odt_span("italica", "Emphasis")
                + " y "
                + odt_span("ambas", "T2")
                + "."
            ),
        )
        + odt_p("", style="P1", inner="Centrado por estilo automatico.")
        + odt_p("", inner="Color " + odt_span("rojo subrayado", "T3") + ".")
        + odt_p(
            "",
            inner=(
                'Enlace a <text:a xlink:href="https://example.com/docsai" xlink:type="simple">'
                "example.com</text:a>."
            ),
        )
    )
    build_odt("basic-styles.odt", body, title="Estilos basicos", auto_styles=auto)


def odt_nested_lists() -> None:
    styles_extra = (
        '<text:list-style style:name="L1">'
        '<text:list-level-style-bullet text:level="1" text:bullet-char="•">'
        '<style:list-level-properties text:space-before="0.635cm" text:min-label-width="0.635cm"/>'
        "</text:list-level-style-bullet>"
        '<text:list-level-style-bullet text:level="2" text:bullet-char="◦">'
        '<style:list-level-properties text:space-before="1.27cm" text:min-label-width="0.635cm"/>'
        "</text:list-level-style-bullet>"
        "</text:list-style>"
        '<text:list-style style:name="L2">'
        '<text:list-level-style-number text:level="1" style:num-suffix="." style:num-format="1">'
        '<style:list-level-properties text:space-before="0.635cm" text:min-label-width="0.635cm"/>'
        "</text:list-level-style-number>"
        '<text:list-level-style-number text:level="2" style:num-suffix="." style:num-format="a">'
        '<style:list-level-properties text:space-before="1.27cm" text:min-label-width="0.635cm"/>'
        "</text:list-level-style-number>"
        "</text:list-style>"
    )
    body = (
        odt_p("Listas anidadas:")
        + '<text:list text:style-name="L1">'
        + "<text:list-item>"
        + odt_p("Uno")
        + '<text:list text:style-name="L1">'
        + "<text:list-item>"
        + odt_p("Uno-A")
        + "</text:list-item>"
        + "<text:list-item>"
        + odt_p("Uno-B")
        + "</text:list-item>"
        + "</text:list>"
        + "</text:list-item>"
        + "<text:list-item>"
        + odt_p("Dos")
        + "</text:list-item>"
        + "</text:list>"
        + '<text:list text:style-name="L2">'
        + "<text:list-item>"
        + odt_p("Primero")
        + '<text:list text:style-name="L2">'
        + "<text:list-item>"
        + odt_p("Primero-a")
        + "</text:list-item>"
        + "</text:list>"
        + "</text:list-item>"
        + "<text:list-item>"
        + odt_p("Segundo")
        + "</text:list-item>"
        + "</text:list>"
    )
    build_odt(
        "nested-lists.odt", body, title="Listas anidadas", styles_extra=styles_extra
    )


def odt_table_simple() -> None:
    body = (
        odt_p("Tabla simple:")
        + '<table:table table:name="Table1">'
        + '<table:table-column table:number-columns-repeated="3"/>'
        + "<table:table-header-rows><table:table-row>"
        + "<table:table-cell office:value-type=\"string\">"
        + odt_p("A")
        + "</table:table-cell>"
        + "<table:table-cell office:value-type=\"string\">"
        + odt_p("B")
        + "</table:table-cell>"
        + "<table:table-cell office:value-type=\"string\">"
        + odt_p("C")
        + "</table:table-cell>"
        + "</table:table-row></table:table-header-rows>"
        + "<table:table-row>"
        + "<table:table-cell office:value-type=\"string\">"
        + odt_p("1")
        + "</table:table-cell>"
        + "<table:table-cell office:value-type=\"string\">"
        + odt_p("2")
        + "</table:table-cell>"
        + "<table:table-cell office:value-type=\"string\">"
        + odt_p("3")
        + "</table:table-cell>"
        + "</table:table-row>"
        + "</table:table>"
    )
    build_odt("table-simple.odt", body, title="Tabla simple")


def odt_table_merged() -> None:
    body = (
        odt_p("Tabla con celdas combinadas:")
        + '<table:table table:name="Table1">'
        + '<table:table-column table:number-columns-repeated="3"/>'
        + "<table:table-row>"
        + '<table:table-cell table:number-columns-spanned="2" office:value-type="string">'
        + odt_p("AB")
        + "</table:table-cell>"
        + "<table:covered-table-cell/>"
        + "<table:table-cell office:value-type=\"string\">"
        + odt_p("C")
        + "</table:table-cell>"
        + "</table:table-row>"
        + "<table:table-row>"
        + '<table:table-cell table:number-rows-spanned="2" office:value-type="string">'
        + odt_p("D")
        + "</table:table-cell>"
        + "<table:table-cell office:value-type=\"string\">"
        + odt_p("E")
        + "</table:table-cell>"
        + "<table:table-cell office:value-type=\"string\">"
        + odt_p("F")
        + "</table:table-cell>"
        + "</table:table-row>"
        + "<table:table-row>"
        + "<table:covered-table-cell/>"
        + "<table:table-cell office:value-type=\"string\">"
        + odt_p("G")
        + "</table:table-cell>"
        + "<table:table-cell office:value-type=\"string\">"
        + odt_p("H")
        + "</table:table-cell>"
        + "</table:table-row>"
        + "</table:table>"
    )
    build_odt("table-merged.odt", body, title="Tabla combinada")


def odt_frame(
    href: str,
    w: str,
    h: str,
    *,
    name: str = "Image1",
    anchor: str = "as-char",
    style: str = "fr1",
    x: str | None = None,
    y: str | None = None,
    z: int | None = None,
) -> str:
    attrs = (
        f'draw:style-name="{style}" draw:name="{name}" text:anchor-type="{anchor}" '
        f'svg:width="{w}" svg:height="{h}"'
    )
    if x is not None:
        attrs += f' svg:x="{x}"'
    if y is not None:
        attrs += f' svg:y="{y}"'
    if z is not None:
        attrs += f' draw:z-index="{z}"'
    return (
        f"<draw:frame {attrs}>"
        f'<draw:image xlink:href="{href}" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>'
        f"</draw:frame>"
    )


def odt_images_inline() -> None:
    auto = (
        '<style:style style:name="fr1" style:family="graphic" style:parent-style-name="Graphics">'
        '<style:graphic-properties style:vertical-pos="top" style:vertical-rel="baseline" '
        'style:horizontal-pos="center" style:horizontal-rel="paragraph" style:mirror="none" '
        'fo:clip="rect(0cm, 0cm, 0cm, 0cm)" draw:luminance="0%" draw:contrast="0%" '
        'draw:red="0%" draw:green="0%" draw:blue="0%" draw:gamma="100%" draw:color-inversion="false" '
        'draw:image-opacity="100%" draw:color-mode="standard"/>'
        "</style:style>"
    )
    styles_extra = (
        '<style:style style:name="Graphics" style:family="graphic">'
        '<style:graphic-properties text:anchor-type="paragraph" svg:x="0cm" svg:y="0cm" '
        'style:wrap="none" style:vertical-pos="top" style:vertical-rel="paragraph" '
        'style:horizontal-pos="center" style:horizontal-rel="paragraph"/>'
        "</style:style>"
    )
    body = odt_p(
        "",
        inner="Antes "
        + odt_frame("Pictures/image1.png", "3cm", "2.25cm", name="Blue")
        + " despues.",
    )
    build_odt(
        "images-inline.odt",
        body,
        title="Imagenes en linea",
        auto_styles=auto,
        styles_extra=styles_extra,
        pictures={"image1.png": BLUE_PNG},
    )


def odt_images_floating() -> None:
    auto = (
        '<style:style style:name="fr1" style:family="graphic" style:parent-style-name="Graphics">'
        '<style:graphic-properties style:wrap="parallel" style:number-wrapped-paragraphs="no-limit" '
        'style:wrap-contour="false" style:vertical-pos="from-top" style:vertical-rel="paragraph" '
        'style:horizontal-pos="from-left" style:horizontal-rel="paragraph" '
        'fo:margin-left="0.2cm" fo:margin-right="0.2cm"/>'
        "</style:style>"
        '<style:style style:name="fr2" style:family="graphic" style:parent-style-name="Graphics">'
        '<style:graphic-properties style:wrap="run-through" style:run-through="background" '
        'style:vertical-pos="from-top" style:vertical-rel="page" '
        'style:horizontal-pos="from-left" style:horizontal-rel="page"/>'
        "</style:style>"
    )
    styles_extra = (
        '<style:style style:name="Graphics" style:family="graphic">'
        '<style:graphic-properties text:anchor-type="paragraph" style:wrap="none"/>'
        "</style:style>"
    )
    body = (
        odt_p(
            "",
            inner=odt_frame(
                "Pictures/image1.png",
                "4cm",
                "3cm",
                name="FloatPara",
                anchor="paragraph",
                style="fr1",
                x="1cm",
                y="0.5cm",
                z=1,
            )
            + "Texto que fluye junto a la imagen flotante anclada al parrafo.",
        )
        + odt_p(
            "",
            inner=odt_frame(
                "Pictures/image2.png",
                "3cm",
                "3cm",
                name="BehindPage",
                anchor="page",
                style="fr2",
                x="2cm",
                y="5cm",
                z=0,
            )
            + "Texto sobre imagen detras de la pagina.",
        )
    )
    build_odt(
        "images-floating.odt",
        body,
        title="Imagenes flotantes",
        auto_styles=auto,
        styles_extra=styles_extra,
        pictures={"image1.png": BLUE_PNG, "image2.png": RED_PNG},
    )


def odt_images_transformed() -> None:
    auto = (
        '<style:style style:name="fr1" style:family="graphic">'
        '<style:graphic-properties style:mirror="horizontal" draw:rotation-angle="15" '
        'fo:clip="rect(0.3cm, 0.4cm, 0.3cm, 0.4cm)" style:wrap="none" '
        'style:vertical-pos="top" style:vertical-rel="baseline"/>'
        "</style:style>"
    )
    body = odt_p(
        "",
        inner=odt_frame(
            "Pictures/image1.png",
            "3cm",
            "3cm",
            name="Transformed",
            anchor="as-char",
            style="fr1",
        ),
    )
    build_odt(
        "images-transformed.odt",
        body,
        title="Imagenes transformadas",
        auto_styles=auto,
        pictures={"image1.png": RED_PNG},
    )


def odt_headers_footers() -> None:
    header = odt_p("Encabezado del documento")
    footer = odt_p(
        "",
        inner='Pagina <text:page-number text:select-page="current">1</text:page-number>'
        " de "
        '<text:page-count>1</text:page-count>',
    )
    body = odt_p("Cuerpo con encabezado y pie.")
    build_odt(
        "headers-footers.odt",
        body,
        title="Encabezados y pies",
        header=header,
        footer=footer,
    )


def odt_footnotes() -> None:
    body = odt_p(
        "",
        inner=(
            "Texto con nota"
            '<text:note text:id="ftn1" text:note-class="footnote">'
            '<text:note-citation>1</text:note-citation>'
            "<text:note-body>"
            + odt_p("Primera nota al pie.")
            + "</text:note-body></text:note>."
        ),
    )
    build_odt("footnotes.odt", body, title="Notas al pie")


def build_ods(
    name: str,
    sheets: list[tuple[str, str]],
    *,
    title: str,
    auto_styles: str = "",
    pictures: dict[str, bytes] | None = None,
) -> None:
    pictures = pictures or {}
    tables = "".join(
        f'<table:table table:name="{n}" table:style-name="ta1">{body}</table:table>'
        for n, body in sheets
    )
    content = (
        f'<?xml version="1.0" encoding="UTF-8"?>'
        f"<office:document-content {ODF_NS} office:version=\"1.3\">"
        f"<office:automatic-styles>"
        f'<style:style style:name="ta1" style:family="table">'
        f'<style:table-properties table:display="true" style:writing-mode="lr-tb"/>'
        f"</style:style>"
        f"{auto_styles}"
        f"</office:automatic-styles>"
        f"<office:body><office:spreadsheet>{tables}</office:spreadsheet></office:body>"
        f"</office:document-content>"
    ).encode()
    parts: dict[str, bytes] = {
        "mimetype": MIME_ODS,
        "content.xml": content,
        "styles.xml": (
            f'<?xml version="1.0" encoding="UTF-8"?>'
            f"<office:document-styles {ODF_NS} office:version=\"1.3\">"
            f"<office:styles/>"
            f"<office:automatic-styles>"
            f'<style:page-layout style:name="pm1">'
            f'<style:page-layout-properties style:writing-mode="lr-tb"/>'
            f"</style:page-layout>"
            f"</office:automatic-styles>"
            f"<office:master-styles>"
            f'<style:master-page style:name="Default" style:page-layout-name="pm1"/>'
            f"</office:master-styles>"
            f"</office:document-styles>"
        ).encode(),
        "meta.xml": odf_meta(title),
    }
    manifest_entries = [
        ("/", "application/vnd.oasis.opendocument.spreadsheet"),
        ("content.xml", "text/xml"),
        ("styles.xml", "text/xml"),
        ("meta.xml", "text/xml"),
    ]
    for pic_name, data in sorted(pictures.items()):
        parts[f"Pictures/{pic_name}"] = data
        ext = Path(pic_name).suffix.lower()
        media = CONTENT_TYPE.get(ext, "application/octet-stream")
        manifest_entries.append((f"Pictures/{pic_name}", media))
    parts["META-INF/manifest.xml"] = odf_manifest(manifest_entries)
    write_odf_package(ROOT / "ods" / name, parts)


def ods_cell(
    value: str = "",
    *,
    vtype: str = "string",
    office_value: str | None = None,
    formula: str | None = None,
    span_cols: int = 1,
    span_rows: int = 1,
    date_value: str | None = None,
    bool_value: str | None = None,
) -> str:
    attrs = [f'office:value-type="{vtype}"']
    if office_value is not None:
        attrs.append(f'office:value="{office_value}"')
    if date_value is not None:
        attrs.append(f'office:date-value="{date_value}"')
    if bool_value is not None:
        attrs.append(f'office:boolean-value="{bool_value}"')
    if formula is not None:
        attrs.append(f'table:formula="{formula}"')
    if span_cols > 1:
        attrs.append(f'table:number-columns-spanned="{span_cols}"')
    if span_rows > 1:
        attrs.append(f'table:number-rows-spanned="{span_rows}"')
    inner = f"<text:p>{value}</text:p>" if value != "" or vtype == "string" else ""
    return f"<table:table-cell {' '.join(attrs)}>{inner}</table:table-cell>"


def ods_row(cells: str, repeat: int = 1) -> str:
    rep = f' table:number-rows-repeated="{repeat}"' if repeat > 1 else ""
    return f"<table:table-row{rep}>{cells}</table:table-row>"


def ods_values_types() -> None:
    rows = (
        ods_row(
            ods_cell("texto")
            + ods_cell("3.14", vtype="float", office_value="3.14")
            + ods_cell("1", vtype="boolean", bool_value="true")
            + ods_cell("2026-01-15", vtype="date", date_value="2026-01-15")
        )
        + ods_row(
            ods_cell("vacio")
            + ods_cell("-2", vtype="float", office_value="-2")
            + ods_cell("0", vtype="boolean", bool_value="false")
            + ods_cell("12.5%", vtype="percentage", office_value="0.125")
        )
    )
    body = '<table:table-column table:number-columns-repeated="4"/>' + rows
    build_ods("values-types.ods", [("Hoja1", body)], title="Tipos de valor")


def ods_formulas_basic() -> None:
    rows = (
        ods_row(
            ods_cell("10", vtype="float", office_value="10")
            + ods_cell("20", vtype="float", office_value="20")
            + ods_cell(
                "30",
                vtype="float",
                office_value="30",
                formula="of:=[.A1]+[.B1]",
            )
        )
        + ods_row(
            ods_cell(
                "30",
                vtype="float",
                office_value="30",
                formula="of:=SUM([.A1:.B1])",
            )
        )
    )
    body = '<table:table-column table:number-columns-repeated="3"/>' + rows
    build_ods("formulas-basic.ods", [("Calc", body)], title="Formulas basicas")


def ods_merged_cells() -> None:
    rows = (
        ods_row(
            ods_cell("AB", span_cols=2)
            + "<table:covered-table-cell/>"
            + ods_cell("C")
        )
        + ods_row(
            ods_cell("D", span_rows=2)
            + ods_cell("E")
            + ods_cell("F")
        )
        + ods_row(
            "<table:covered-table-cell/>"
            + ods_cell("G")
            + ods_cell("H")
        )
    )
    body = '<table:table-column table:number-columns-repeated="3"/>' + rows
    build_ods("merged-cells.ods", [("Hoja", body)], title="Celdas combinadas")


def ods_images_anchored() -> None:
    auto = (
        '<style:style style:name="gr1" style:family="graphic">'
        '<style:graphic-properties style:wrap="none"/>'
        "</style:style>"
    )
    frame = (
        '<draw:frame draw:style-name="gr1" draw:name="Pic1" table:end-cell-address="Hoja.B2" '
        'table:end-x="1cm" table:end-y="1cm" svg:width="2cm" svg:height="1.5cm" svg:x="0.1cm" svg:y="0.1cm">'
        '<draw:image xlink:href="Pictures/image1.png" xlink:type="simple" xlink:show="embed" '
        'xlink:actuate="onLoad"/>'
        "</draw:frame>"
    )
    rows = ods_row(
        f'<table:table-cell office:value-type="string"><text:p>Con imagen</text:p>{frame}</table:table-cell>'
        + ods_cell("otra")
    )
    body = '<table:table-column table:number-columns-repeated="2"/>' + rows
    build_ods(
        "images-anchored.ods",
        [("Hoja", body)],
        title="Imagenes ancladas",
        auto_styles=auto,
        pictures={"image1.png": BLUE_PNG},
    )




# --------------------------------------------------------------------------
# ODP corpus (Phase 18)
# --------------------------------------------------------------------------
# One trait per file, named after the `corpus/pptx` deck it mirrors so that the
# cross-conversion matrix of 18-E can pair the two sides by name. Where ODF has
# no equivalent of a pptx trait — crossed notes, a stale autofit cache, a stale
# chart workbook, `mc:AlternateContent`, a macro-enabled MIME type — there is no
# file here and `corpus/README.md` says why. `presentation-classes.odp` is the
# other direction: a trait ODF has and OOXML does not.

ODP_NS = ODF_NS + (
    ' xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0"'
    ' xmlns:anim="urn:oasis:names:tc:opendocument:xmlns:animation:1.0"'
    ' xmlns:smil="urn:oasis:names:tc:opendocument:xmlns:smil-compatible:1.0"'
)

MIME_ODP = b"application/vnd.oasis.opendocument.presentation"

# A 16:9 page, in the centimetres ODF measures in. The pptx corpus states the
# same geometry in EMU; the unit is the difference, not the trait.
ODP_PAGE_W = "28cm"
ODP_PAGE_H = "15.75cm"
ODP_TITLE_BOX = 'svg:x="1.4cm" svg:y="0.837cm" svg:width="25.2cm" svg:height="2.629cm"'
ODP_OUTLINE_BOX = 'svg:x="1.4cm" svg:y="3.685cm" svg:width="25.2cm" svg:height="9.906cm"'

# The one level ODF has above a page. OOXML has two (layout and master), and
# `style:presentation-page-layout` is not the second: it is a naked list of
# placeholder rectangles with no style of its own. Which side bends is 18-B's
# question; the corpus only has to carry both shapes of it.
ODP_PAGE_LAYOUTS = (
    '<style:presentation-page-layout style:name="AL1T19">'
    '<presentation:placeholder presentation:object="title" '
    'svg:x="1.4cm" svg:y="0.837cm" svg:width="25.2cm" svg:height="2.629cm"/>'
    '<presentation:placeholder presentation:object="outline" '
    'svg:x="1.4cm" svg:y="3.685cm" svg:width="25.2cm" svg:height="9.906cm"/>'
    "</style:presentation-page-layout>"
    '<style:presentation-page-layout style:name="AL2T1">'
    '<presentation:placeholder presentation:object="title" '
    'svg:x="1.4cm" svg:y="0.837cm" svg:width="25.2cm" svg:height="2.629cm"/>'
    "</style:presentation-page-layout>"
)

ODP_STYLES = (
    '<style:default-style style:family="graphic">'
    '<style:graphic-properties draw:fill="none" draw:stroke="none" '
    'style:flow-with-text="false"/>'
    '<style:paragraph-properties fo:margin-left="0cm" fo:margin-right="0cm"/>'
    '<style:text-properties style:font-name="Liberation Sans" fo:font-size="18pt"/>'
    "</style:default-style>"
    '<style:style style:name="standard" style:family="graphic">'
    '<style:graphic-properties draw:stroke="none" draw:fill="none"/>'
    '<style:text-properties style:font-name="Liberation Sans" fo:font-size="18pt"/>'
    "</style:style>"
    '<style:style style:name="objectwitharrow" style:family="graphic" '
    'style:parent-style-name="standard">'
    '<style:graphic-properties draw:stroke="solid" svg:stroke-width="0.15cm" '
    'svg:stroke-color="#000000" draw:marker-end-width="0.7cm"/>'
    "</style:style>"
) + ODP_PAGE_LAYOUTS

# The presentation styles a master's placeholders and a page's frames inherit
# from. They live in `styles.xml`'s automatic styles because that is where
# LibreOffice puts them and where a master page can reference them.
ODP_STYLES_AUTOMATIC = (
    f'<style:page-layout style:name="PM1">'
    f'<style:page-layout-properties fo:page-width="{ODP_PAGE_W}" '
    f'fo:page-height="{ODP_PAGE_H}" fo:margin-top="0cm" fo:margin-bottom="0cm" '
    f'fo:margin-left="0cm" fo:margin-right="0cm" style:print-orientation="landscape"/>'
    f"</style:page-layout>"
    f'<style:style style:name="dp-master" style:family="drawing-page">'
    f'<style:drawing-page-properties draw:fill="none" '
    f'presentation:background-visible="true" presentation:background-objects-visible="true" '
    f'presentation:display-footer="true" presentation:display-page-number="false" '
    f'presentation:display-date-time="true"/>'
    f"</style:style>"
    f'<style:style style:name="Default-title" style:family="presentation">'
    f'<style:graphic-properties draw:fill="none" draw:stroke="none" '
    f'draw:auto-grow-height="false" fo:min-height="2.629cm"/>'
    f'<style:text-properties fo:font-size="40pt"/>'
    f"</style:style>"
    f'<style:style style:name="Default-subtitle" style:family="presentation">'
    f'<style:graphic-properties draw:fill="none" draw:stroke="none" '
    f'draw:auto-grow-height="false"/>'
    f'<style:text-properties fo:font-size="24pt" fo:font-style="italic"/>'
    f"</style:style>"
    f'<style:style style:name="Default-outline1" style:family="presentation">'
    f'<style:graphic-properties draw:fill="none" draw:stroke="none" '
    f'draw:auto-grow-height="false" fo:min-height="9.906cm"/>'
    f'<style:text-properties fo:font-size="28pt"/>'
    f"</style:style>"
    f'<style:style style:name="Default-notes" style:family="presentation">'
    f'<style:graphic-properties draw:fill="none" draw:stroke="none" '
    f'draw:auto-grow-height="true"/>'
    f'<style:text-properties fo:font-size="14pt"/>'
    f"</style:style>"
)

ODP_DEFAULT_MASTER = (
    f'<style:master-page style:name="Default" style:page-layout-name="PM1" '
    f'draw:style-name="dp-master">'
    f'<draw:frame presentation:style-name="Default-title" '
    f'draw:layer="backgroundobjects" {ODP_TITLE_BOX} '
    f'presentation:class="title" presentation:placeholder="true">'
    f"<draw:text-box/></draw:frame>"
    f'<draw:frame presentation:style-name="Default-outline1" '
    f'draw:layer="backgroundobjects" {ODP_OUTLINE_BOX} '
    f'presentation:class="outline" presentation:placeholder="true">'
    f"<draw:text-box/></draw:frame>"
    f"</style:master-page>"
)


def odf_styles_odp(*, extra_styles: str = "", extra_automatic: str = "",
                   masters: str = ODP_DEFAULT_MASTER) -> bytes:
    return (
        f'<?xml version="1.0" encoding="UTF-8"?>'
        f'<office:document-styles {ODP_NS} office:version="1.3">'
        f"<office:font-face-decls>"
        f'<style:font-face style:name="Liberation Sans" '
        f'svg:font-family="&apos;Liberation Sans&apos;"/>'
        f"</office:font-face-decls>"
        f"<office:styles>{ODP_STYLES}{extra_styles}</office:styles>"
        f"<office:automatic-styles>{ODP_STYLES_AUTOMATIC}{extra_automatic}"
        f"</office:automatic-styles>"
        f"<office:master-styles>{masters}</office:master-styles>"
        f"</office:document-styles>"
    ).encode()


def odf_settings(ns: str) -> bytes:
    """A `settings.xml` with one configuration item in it.

    Not decoration: ODF's grammar declares `office:settings` as one or more
    `config:config-item-set`, so the empty `<office:settings/>` the `.odt` and
    `.ods` fixtures carried is invalid ODF, and the RELAX NG arm of
    `schema_gate.py` is what said so (Phase 18-A)."""
    return (
        f'<?xml version="1.0" encoding="UTF-8"?>'
        f'<office:document-settings {ns} office:version="1.3">'
        f"<office:settings>"
        f'<config:config-item-set config:name="ooo:view-settings">'
        f'<config:config-item config:name="VisibleAreaTop" config:type="int">0'
        f"</config:config-item>"
        f"</config:config-item-set>"
        f"</office:settings></office:document-settings>"
    ).encode()


# The presentation automatic styles every page frame in this corpus references.
# A fixture that needs one of its own adds it through `auto_styles`.
ODP_CONTENT_AUTOMATIC = (
    '<style:style style:name="dp1" style:family="drawing-page">'
    '<style:drawing-page-properties presentation:background-visible="true" '
    'presentation:background-objects-visible="true" '
    'presentation:display-footer="true" presentation:display-page-number="false" '
    'presentation:display-date-time="true"/>'
    "</style:style>"
    '<style:style style:name="pr-title" style:family="presentation" '
    'style:parent-style-name="Default-title">'
    '<style:graphic-properties draw:fill="none" draw:stroke="none"/>'
    "</style:style>"
    '<style:style style:name="pr-outline" style:family="presentation" '
    'style:parent-style-name="Default-outline1">'
    '<style:graphic-properties draw:fill="none" draw:stroke="none"/>'
    "</style:style>"
    '<style:style style:name="pr-subtitle" style:family="presentation" '
    'style:parent-style-name="Default-subtitle">'
    '<style:graphic-properties draw:fill="none" draw:stroke="none"/>'
    "</style:style>"
    '<style:style style:name="pr-notes" style:family="presentation" '
    'style:parent-style-name="Default-notes">'
    '<style:graphic-properties draw:fill="none" draw:stroke="none"/>'
    "</style:style>"
    '<style:style style:name="gr-plain" style:family="graphic" '
    'style:parent-style-name="standard">'
    '<style:graphic-properties draw:fill="none" draw:stroke="none"/>'
    "</style:style>"
)


def build_odp(
    name: str,
    pages: str,
    *,
    title: str,
    auto_styles: str = "",
    styles_extra: str = "",
    styles_automatic_extra: str = "",
    masters: str = ODP_DEFAULT_MASTER,
    pictures: dict[str, bytes] | None = None,
    extra_parts: dict[str, tuple[bytes, str]] | None = None,
    settings: str = "",
) -> None:
    """An `.odp` package. `extra_parts` maps a path to `(bytes, media type)`
    and covers the two things a deck can carry that are not pictures: an
    embedded chart sub-document and an OLE stream."""
    pictures = pictures or {}
    extra_parts = extra_parts or {}
    content = (
        f'<?xml version="1.0" encoding="UTF-8"?>'
        f'<office:document-content {ODP_NS} office:version="1.3">'
        f"<office:scripts/>"
        f"<office:font-face-decls>"
        f'<style:font-face style:name="Liberation Sans" '
        f'svg:font-family="&apos;Liberation Sans&apos;"/>'
        f"</office:font-face-decls>"
        f"<office:automatic-styles>{ODP_CONTENT_AUTOMATIC}{auto_styles}"
        f"</office:automatic-styles>"
        f"<office:body><office:presentation>{pages}"
        f'<presentation:settings presentation:full-screen="false"/>{settings}'
        f"</office:presentation></office:body></office:document-content>"
    ).encode()
    parts: dict[str, bytes] = {
        "mimetype": MIME_ODP,
        "content.xml": content,
        "styles.xml": odf_styles_odp(
            extra_styles=styles_extra,
            extra_automatic=styles_automatic_extra,
            masters=masters,
        ),
        "meta.xml": odf_meta(title),
        "settings.xml": odf_settings(ODP_NS),
    }
    manifest_entries = [
        ("/", "application/vnd.oasis.opendocument.presentation"),
        ("content.xml", "text/xml"),
        ("styles.xml", "text/xml"),
        ("meta.xml", "text/xml"),
        ("settings.xml", "text/xml"),
    ]
    for pic_name, data in sorted(pictures.items()):
        parts[f"Pictures/{pic_name}"] = data
        ext = Path(pic_name).suffix.lower()
        manifest_entries.append(
            (f"Pictures/{pic_name}", CONTENT_TYPE.get(ext, "application/octet-stream"))
        )
    for path, (data, media) in sorted(extra_parts.items()):
        parts[path] = data
        manifest_entries.append((path, media))
    parts["META-INF/manifest.xml"] = odf_manifest(sorted(manifest_entries))
    write_odf_package(ROOT / "odp" / name, parts)


# --- page and frame helpers ------------------------------------------------


def odp_page(
    name: str,
    shapes: str,
    *,
    master: str = "Default",
    layout: str = "AL1T19",
    style: str = "dp1",
    notes: str = "",
    extra: str = "",
) -> str:
    layout_attr = (
        f' presentation:presentation-page-layout-name="{layout}"' if layout else ""
    )
    return (
        f'<draw:page draw:name="{name}" draw:style-name="{style}" '
        f'draw:master-page-name="{master}"{layout_attr}>'
        f"{shapes}{extra}{notes}</draw:page>"
    )


def odp_frame(
    inner: str,
    *,
    cls: str | None = None,
    box: str = "",
    style: str = "pr-title",
    style_attr: str = "presentation",
    layer: str = "layout",
    placeholder: bool = False,
    extra: str = "",
) -> str:
    attrs = [f'{style_attr}:style-name="{style}"', f'draw:layer="{layer}"']
    if box:
        attrs.append(box)
    if cls is not None:
        attrs.append(f'presentation:class="{cls}"')
    if placeholder:
        attrs.append('presentation:placeholder="true"')
    if extra:
        attrs.append(extra)
    return f"<draw:frame {' '.join(attrs)}>{inner}</draw:frame>"


def odp_text_box(paragraphs: str) -> str:
    return f"<draw:text-box>{paragraphs}</draw:text-box>"


def odp_title(text: str, *, box: str = ODP_TITLE_BOX) -> str:
    return odp_frame(
        odp_text_box(f"<text:p>{text}</text:p>"),
        cls="title",
        box=box,
        style="pr-title",
    )


def odp_outline(items: list[tuple[str, int]], *, box: str = ODP_OUTLINE_BOX) -> str:
    """An outline placeholder holding a `text:list`. ODF nests a level by
    nesting a whole list inside the *item* above it, where OOXML flattens the
    depth into `a:pPr@lvl`; that difference is the trait."""
    def nest(index: int, level: int) -> tuple[str, int]:
        out = []
        while index < len(items):
            text, depth = items[index]
            if depth < level:
                break
            if depth > level:
                inner, index = nest(index, level + 1)
                if out:
                    out[-1] = out[-1][: -len("</text:list-item>")] + inner + "</text:list-item>"
                else:
                    out.append(f"<text:list-item>{inner}</text:list-item>")
                continue
            out.append(f"<text:list-item><text:p>{text}</text:p></text:list-item>")
            index += 1
        return f"<text:list>{''.join(out)}</text:list>", index

    body, _ = nest(0, 0)
    return odp_frame(odp_text_box(body), cls="outline", box=box, style="pr-outline")


def odp_notes(text: str) -> str:
    lines = "".join(f"<text:p>{line}</text:p>" for line in text.split("\n"))
    return (
        f'<presentation:notes draw:style-name="dp1">'
        f'<draw:page-thumbnail draw:layer="layout" svg:x="3.5cm" svg:y="1cm" '
        f'svg:width="14cm" svg:height="7.875cm" presentation:class="page"/>'
        f'<draw:frame presentation:style-name="pr-notes" draw:layer="layout" '
        f'svg:x="3.5cm" svg:y="9.5cm" svg:width="14cm" svg:height="10cm" '
        f'presentation:class="notes">{odp_text_box(lines)}</draw:frame>'
        f"</presentation:notes>"
    )


def odp_title_body(name: str, title: str, bullets: list[tuple[str, int]],
                   *, notes: str = "") -> str:
    return odp_page(
        name,
        odp_title(title) + odp_outline(bullets),
        notes=odp_notes(notes) if notes else "",
    )


# --- the fixtures ----------------------------------------------------------


def odp_basic_slides() -> None:
    """Two pages, title + outline, nothing else. The floor of the format, and
    the `.odp` half of `pptx/basic-slides.pptx`."""
    build_odp(
        "basic-slides.odp",
        odp_title_body("page1", "Informe trimestral",
                       [("Ingresos al alza", 0), ("Costes estables", 0)])
        + odp_title_body("page2", "Siguientes pasos", [("Cerrar el trimestre", 0)]),
        title="Presentación básica",
    )


def odp_slide_order() -> None:
    """The `.odp` answer to `pptx/slide-order.pptx`, and it is a different
    question.

    A deck has no `p:sldIdLst` here: `draw:page` order in `content.xml` *is* the
    order, and there is no second list to disagree with it. What can still
    mislead a reader is `draw:name`, so the names are crossed on purpose — the
    first page is called `page3`. A reader that sorts by name, as it must for
    `slideN.xml` on the OOXML side, gets this deck backwards."""
    build_odp(
        "slide-order.odp",
        odp_title_body("page3", "Primera en el orden", [("Va la primera", 0)])
        + odp_title_body("page1", "Segunda en el orden", [("Va en medio", 0)])
        + odp_title_body("page2", "Tercera en el orden", [("Va la última", 0)]),
        title="Orden de páginas",
    )


def odp_placeholders_cascade() -> None:
    """A title and an outline that state no geometry at all: position and size
    come from `style:presentation-page-layout`, and the text properties from the
    master page's placeholder. One level of inheritance where OOXML has two — a
    reader that emits any of it has failed the delta rule."""
    build_odp(
        "placeholders-cascade.odp",
        odp_page(
            "page1",
            odp_frame(odp_text_box("<text:p>Todo heredado</text:p>"),
                      cls="title", style="pr-title")
            + odp_frame(
                odp_text_box(
                    "<text:list><text:list-item><text:p>Sin posición propia</text:p>"
                    "</text:list-item><text:list-item><text:p>Sin tipografía propia"
                    "</text:p></text:list-item></text:list>"
                ),
                cls="outline", style="pr-outline"),
        ),
        title="Cascada de páginas maestras",
    )


def odp_placeholders_empty() -> None:
    """An empty placeholder is content: it holds a rectangle the layout gave it
    and a person is expected to fill it. Both spellings of emptiness are here —
    a `draw:text-box` with no paragraph at all, and one holding an empty
    `text:p` — because a writer that rewrote one into the other would break byte
    identity on a frame nobody edited."""
    build_odp(
        "placeholders-empty.odp",
        odp_page(
            "page1",
            odp_title("Título con cuerpo vacío")
            + odp_frame(odp_text_box(""), cls="outline", box=ODP_OUTLINE_BOX,
                        style="pr-outline", placeholder=True)
            + odp_frame(
                odp_text_box("<text:p>Primera línea</text:p><text:p/>"
                             "<text:p>Tras el hueco</text:p>"),
                cls="text",
                box='svg:x="1.4cm" svg:y="13.8cm" svg:width="25.2cm" svg:height="1.4cm"',
                style="pr-outline"),
        ),
        title="Marcadores vacíos",
    )


def odp_presentation_classes() -> None:
    """ODF has a trait OOXML does not, and this is it.

    `presentation:class` names a frame's *role* from a closed vocabulary —
    `title`, `subtitle`, `outline`, `text`, `graphic`, `object`, `notes`,
    `header`, `footer`, `date-time`, `page-number` — where `p:ph` names a type
    **and an index**, so two `outline` frames on an ODF page are
    indistinguishable while `idx="1"` and `idx="2"` are not. The mapping is not
    a bijection, and 18-B has to say which side bends; this is the file that
    holds the ODF side of the question. No `corpus/pptx` deck mirrors it."""
    row = 'svg:width="8.6cm" svg:height="2.4cm"'
    frames = odp_title("El vocabulario de presentation:class")
    for index, cls in enumerate(
        ["subtitle", "outline", "text", "graphic", "object",
         "header", "footer", "date-time", "page-number"]
    ):
        x = 1.4 + (index % 3) * 9.0
        y = 3.7 + (index // 3) * 3.0
        frames += odp_frame(
            odp_text_box(f"<text:p>{cls}</text:p>"),
            cls=cls,
            box=f'svg:x="{x:.1f}cm" svg:y="{y:.1f}cm" {row}',
            style="pr-outline",
        )
    # Two frames of the same class on one page: OOXML would distinguish them by
    # `idx`, ODF has nothing to distinguish them by but document order.
    frames += odp_frame(
        odp_text_box("<text:p>Segundo outline, mismo class</text:p>"),
        cls="outline",
        box='svg:x="1.4cm" svg:y="12.8cm" svg:width="25.2cm" svg:height="2.4cm"',
        style="pr-outline",
    )
    build_odp("presentation-classes.odp", odp_page("page1", frames, layout="AL2T1"),
              title="Clases de presentación")


def odp_bullets_levels() -> None:
    """Nested `text:list` with both bullet kinds. ODF nests lists inside list
    items; OOXML flattens depth into an attribute. Same document, two grammars,
    and the DocMark in the middle has to be identical."""
    numbered = (
        '<text:list-style style:name="Lnum">'
        '<text:list-level-style-number text:level="1" style:num-suffix="." '
        'style:num-format="1"/>'
        "</text:list-style>"
    )
    build_odp(
        "bullets-levels.odp",
        odp_page(
            "page1",
            odp_title("Niveles de viñeta")
            + odp_outline([
                ("Primer nivel", 0),
                ("Segundo nivel", 1),
                ("Tercer nivel", 2),
                ("Vuelve al primero", 0),
            ], box='svg:x="1.4cm" svg:y="3.685cm" svg:width="25.2cm" svg:height="7cm"')
            + odp_frame(
                odp_text_box(
                    '<text:list text:style-name="Lnum">'
                    "<text:list-item><text:p>Uno</text:p></text:list-item>"
                    "<text:list-item><text:p>Dos</text:p></text:list-item>"
                    "</text:list>"
                ),
                cls="text",
                box='svg:x="1.4cm" svg:y="11cm" svg:width="25.2cm" svg:height="3cm"',
                style="pr-outline"),
        ),
        auto_styles=numbered,
        title="Viñetas y niveles",
    )


def odp_notes_speaker() -> None:
    """`presentation:notes` is a child of the page it belongs to, not a part
    with a relationship, which is why there is no `.odp` mirror of
    `pptx/notes-crossed.pptx`: nothing here can be crossed."""
    build_odp(
        "notes-speaker.odp",
        odp_title_body("page1", "Resultados", [("Crecimiento del 12 %", 0)],
                       notes="Insistir en que el 12 % es interanual.\n"
                             "No entrar en el desglose por región.")
        + odp_title_body("page2", "Riesgos", [("Dependencia de un proveedor", 0)],
                         notes="Si preguntan por el proveedor, remitir al anexo."),
        title="Notas del ponente",
    )


def odp_reading_order() -> None:
    """A page whose document order is not the order anybody reads it in, and
    which additionally carries `draw:z-index` — ODF states the stacking order
    explicitly where OOXML leaves it implicit in `spTree` order. The two can
    disagree, and here they do."""
    build_odp(
        "reading-order.odp",
        odp_page(
            "page1",
            odp_frame(odp_text_box("<text:p>Al pie de la página</text:p>"),
                      cls="text", style="pr-outline",
                      box='svg:x="1.4cm" svg:y="12.5cm" svg:width="6cm" svg:height="1.4cm"',
                      extra='draw:z-index="3"')
            + odp_frame(odp_text_box("<text:p>Cuerpo heredado</text:p>"),
                        cls="outline", style="pr-outline", box=ODP_OUTLINE_BOX,
                        extra='draw:z-index="1"')
            + odp_frame(odp_text_box("<text:p>A la derecha</text:p>"),
                        cls="text", style="pr-outline",
                        box='svg:x="15cm" svg:y="5.2cm" svg:width="5cm" svg:height="1.4cm"',
                        extra='draw:z-index="2"')
            + odp_frame(odp_text_box("<text:p>El título va el cuarto</text:p>"),
                        cls="title", style="pr-title", box=ODP_TITLE_BOX,
                        extra='draw:z-index="0"')
            + odp_frame(odp_text_box("<text:p>A la izquierda</text:p>"),
                        cls="text", style="pr-outline",
                        box='svg:x="1.4cm" svg:y="5.21cm" svg:width="5cm" svg:height="1.4cm"',
                        extra='draw:z-index="4"'),
        ),
        title="Orden de lectura",
    )


def odp_raw_preserved() -> None:
    """The things an ODF page can hold that the IR has no node for.

    A `draw:g` group, a `draw:custom-shape` whose `draw:enhanced-geometry` is
    the shape, a page transition expressed as drawing-page properties, and an
    `anim:par` animation tree. The pptx deck of the same name carries the OOXML
    spelling of all four (`p:grpSp`, `a:custGeom`, `p:transition`, `p:timing`),
    which is what makes the pair worth having: the raw block is a different
    fragment on each side and the DocMark stub around it must not be."""
    group = (
        '<draw:g draw:name="Grupo 1" xml:id="grupo1">'
        '<draw:frame draw:style-name="gr-plain" draw:layer="layout" '
        'svg:x="1.4cm" svg:y="6cm" svg:width="4cm" svg:height="1.4cm">'
        "<draw:text-box><text:p>Dentro del grupo</text:p></draw:text-box></draw:frame>"
        '<draw:frame draw:style-name="gr-plain" draw:layer="layout" '
        'svg:x="6cm" svg:y="6cm" svg:width="4cm" svg:height="1.4cm">'
        "<draw:text-box><text:p>También dentro</text:p></draw:text-box></draw:frame>"
        "</draw:g>"
    )
    custom = (
        '<draw:custom-shape draw:style-name="gr-plain" draw:layer="layout" '
        'draw:name="Triángulo libre" svg:x="15cm" svg:y="6cm" '
        'svg:width="4cm" svg:height="4cm">'
        "<text:p>Etiqueta del triángulo</text:p>"
        '<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" '
        'draw:type="isosceles-triangle" '
        'draw:enhanced-path="M 10800 0 L 21600 21600 0 21600 Z N"/>'
        "</draw:custom-shape>"
    )
    transition = (
        '<style:style style:name="dp-fade" style:family="drawing-page">'
        '<style:drawing-page-properties presentation:transition-type="automatic" '
        'presentation:transition-style="fade-from-center" '
        'presentation:transition-speed="slow" '
        'presentation:background-visible="true" '
        'presentation:background-objects-visible="true"/>'
        "</style:style>"
    )
    animation = (
        "<anim:par presentation:node-type=\"timing-root\">"
        '<anim:seq presentation:node-type="main-sequence">'
        '<anim:par smil:begin="indefinite">'
        '<anim:set smil:begin="0s" smil:dur="1s" smil:targetElement="grupo1" '
        'smil:attributeName="visibility" smil:to="visible"/>'
        "</anim:par></anim:seq></anim:par>"
    )
    build_odp(
        "raw-preserved.odp",
        odp_page("page1", odp_title("Lo que no se modela") + group + custom,
                 style="dp-fade", layout="AL2T1", extra=animation),
        auto_styles=transition,
        title="Preservación en bruto",
    )


def odp_tables_simple() -> None:
    """A `table:table` on a page. The same IR `Table` the `.ods` reader fills,
    reached through a drawing frame instead of a spreadsheet — which is exactly
    what `p:graphicFrame` -> `a:tbl` is on the OOXML side."""
    def cell(text: str) -> str:
        return (
            f'<table:table-cell office:value-type="string">'
            f"<text:p>{text}</text:p></table:table-cell>"
        )

    table = (
        '<table:table table:name="Tabla 1">'
        '<table:table-column table:number-columns-repeated="2"/>'
        f"<table:table-row>{cell('Región')}{cell('Ventas')}</table:table-row>"
        f"<table:table-row>{cell('Norte')}{cell('1 200')}</table:table-row>"
        f"<table:table-row>{cell('Sur')}{cell('980')}</table:table-row>"
        "</table:table>"
    )
    build_odp(
        "tables-simple.odp",
        odp_page(
            "page1",
            odp_title("Ventas por región")
            + odp_frame(table, style="gr-plain", style_attr="draw",
                        box='svg:x="1.4cm" svg:y="4.5cm" svg:width="14cm" svg:height="5cm"'),
            layout="AL2T1",
        ),
        title="Tabla simple",
    )


def odp_images_anchored() -> None:
    """A picture with explicit geometry. ODF measures it in centimetres and
    OOXML in EMU; the frame, the alt text and the aspect ratio are the same
    trait, and the unit is what 18-E has to convert."""
    picture = odp_frame(
        '<draw:image xlink:href="Pictures/image1.png" xlink:type="simple" '
        'xlink:show="embed" xlink:actuate="onLoad"/>'
        "<svg:title>Gráfico de barras azul</svg:title>",
        style="gr-plain",
        style_attr="draw",
        box='svg:x="4cm" svg:y="5cm" svg:width="3.175cm" svg:height="2.381cm"',
        extra='draw:name="Imagen 1"',
    )
    build_odp(
        "images-anchored.odp",
        odp_page("page1", odp_title("Con imagen") + picture, layout="AL2T1"),
        pictures={"image1.png": BLUE_PNG},
        title="Imagen anclada",
    )


def odp_shapes_geometry() -> None:
    """Free shapes with ODF's own geometry vocabulary: a `draw:rect` with
    rounded corners, a `draw:custom-shape` naming a preset through
    `draw:type`, and a `draw:line`. `draw:transform` states a rotation as an
    affine matrix where OOXML states an angle in `a:xfrm@rot` — the same
    picture, and not the same number."""
    rect = (
        '<draw:rect draw:style-name="gr-plain" draw:layer="layout" '
        'draw:name="Rectángulo 1" svg:x="1.4cm" svg:y="5cm" '
        'svg:width="6cm" svg:height="2.5cm" draw:corner-radius="0.4cm">'
        "<text:p>Dentro del rectángulo</text:p></draw:rect>"
    )
    arrow = (
        '<draw:custom-shape draw:style-name="gr-plain" draw:layer="layout" '
        'draw:name="Flecha 1" '
        'draw:transform="rotate (-0.5235987755982988) translate (9cm 6cm)" '
        'svg:width="4cm" svg:height="1.6cm">'
        "<text:p/>"
        '<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" '
        'draw:type="right-arrow" '
        'draw:enhanced-path="M 0 5400 L 12960 5400 12960 0 21600 10800 '
        '12960 21600 12960 16200 0 16200 Z N"/>'
        "</draw:custom-shape>"
    )
    line = (
        '<draw:line draw:style-name="objectwitharrow" draw:layer="layout" '
        'draw:name="Conector 1" svg:x1="16cm" svg:y1="6cm" '
        'svg:x2="22cm" svg:y2="6cm"/>'
    )
    build_odp(
        "shapes-geometry.odp",
        odp_page("page1", odp_title("Formas libres") + rect + arrow + line,
                 layout="AL2T1"),
        title="Geometría de formas",
    )


def odp_chart_document(series: list[tuple[str, list[str]]], *,
                       categories: list[str], legend: bool) -> bytes:
    """An embedded chart sub-document.

    The structural difference from OOXML is worth stating once: a `.pptx` chart
    keeps its numbers in an embedded **workbook**, a whole second package, and
    caches them in the chart part — which is what makes
    `pptx/charts-stale-workbook.pptx` possible. An ODF chart holds a
    `table:table` named `local-table` *inside itself*, so there is no second
    copy and no way for one to go stale. That deck has no `.odp` mirror for
    that reason, and `corpus/README.md` says so."""
    columns = "".join(
        f'<table:table-column/>' for _ in range(len(series) + 1)
    )
    header = (
        "<table:table-header-rows><table:table-row>"
        '<table:table-cell office:value-type="string"><text:p/></table:table-cell>'
        + "".join(
            f'<table:table-cell office:value-type="string">'
            f"<text:p>{name}</text:p></table:table-cell>"
            for name, _ in series
        )
        + "</table:table-row></table:table-header-rows>"
    )
    rows = ""
    for index, category in enumerate(categories):
        cells = (
            f'<table:table-cell office:value-type="string">'
            f"<text:p>{category}</text:p></table:table-cell>"
        )
        for _, values in series:
            cells += (
                f'<table:table-cell office:value-type="float" '
                f'office:value="{values[index]}">'
                f"<text:p>{values[index]}</text:p></table:table-cell>"
            )
        rows += f"<table:table-row>{cells}</table:table-row>"
    local_table = (
        f'<table:table table:name="local-table">{columns}{header}'
        f"<table:table-rows>{rows}</table:table-rows></table:table>"
    )

    plotted = "".join(
        f'<chart:series chart:style-name="ch-series" '
        f'chart:values-cell-range-address="local-table.${chr(66 + index)}$2:'
        f'.${chr(66 + index)}${1 + len(categories)}" '
        f'chart:label-cell-address="local-table.${chr(66 + index)}$1" '
        f'chart:class="chart:bar"/>'
        for index in range(len(series))
    )
    legend_xml = (
        '<chart:legend chart:legend-position="end" style:legend-expansion="high"/>'
        if legend else ""
    )
    content = (
        f'<?xml version="1.0" encoding="UTF-8"?>'
        f'<office:document-content {ODP_NS} office:version="1.3">'
        f"<office:automatic-styles>"
        f'<style:style style:name="ch-chart" style:family="chart">'
        f'<style:graphic-properties draw:stroke="none" draw:fill="none"/>'
        f"</style:style>"
        f'<style:style style:name="ch-series" style:family="chart">'
        f'<style:chart-properties chart:japanese-candle-stick="false"/>'
        f"</style:style>"
        f"</office:automatic-styles>"
        f"<office:body><office:chart>"
        f'<chart:chart chart:style-name="ch-chart" chart:class="chart:bar" '
        f'svg:width="12cm" svg:height="8cm">'
        f"<chart:title><text:p>Ventas por región</text:p></chart:title>"
        f"{legend_xml}"
        f"<chart:plot-area>"
        f'<chart:axis chart:dimension="x" chart:name="primary-x"/>'
        f'<chart:axis chart:dimension="y" chart:name="primary-y"/>'
        f"{plotted}"
        f"</chart:plot-area>"
        f"{local_table}"
        f"</chart:chart>"
        f"</office:chart></office:body></office:document-content>"
    )
    return content.encode()


def odp_chart_parts(name: str, series: list[tuple[str, list[str]]], *,
                    categories: list[str], legend: bool
                    ) -> dict[str, tuple[bytes, str]]:
    """The three parts an embedded chart adds to the package: its own
    `content.xml`, its own `styles.xml`, and the directory entry the manifest
    needs so a consumer knows what the sub-document is."""
    styles = (
        f'<?xml version="1.0" encoding="UTF-8"?>'
        f'<office:document-styles {ODP_NS} office:version="1.3">'
        f"<office:styles/></office:document-styles>"
    ).encode()
    return {
        f"{name}/": (b"", "application/vnd.oasis.opendocument.chart"),
        f"{name}/content.xml": (
            odp_chart_document(series, categories=categories, legend=legend),
            "text/xml",
        ),
        f"{name}/styles.xml": (styles, "text/xml"),
    }


def odp_charts_embedded() -> None:
    """One series, drawn from the chart's own `local-table`. The `.odp` half of
    `pptx/charts-embedded.pptx`."""
    build_odp(
        "charts-embedded.odp",
        odp_page(
            "page1",
            odp_title("Ventas por región")
            + odp_frame(
                '<draw:object xlink:href="./Object 1" xlink:type="simple" '
                'xlink:show="embed" xlink:actuate="onLoad"/>',
                style="gr-plain", style_attr="draw",
                box='svg:x="1.4cm" svg:y="4.5cm" svg:width="12cm" svg:height="8cm"',
                extra='draw:name="Gráfico 1"'),
            layout="AL2T1",
        ),
        extra_parts=odp_chart_parts(
            "Object 1",
            [("Ventas", ["1200", "980", "1440"])],
            categories=["Norte", "Sur", "Este"],
            legend=False,
        ),
        title="Gráfico incrustado",
    )


def odp_charts_multi_series() -> None:
    """Two series and a legend: the DocMark data table gains a column, and the
    reader has to keep the series names apart from the categories."""
    build_odp(
        "charts-multi-series.odp",
        odp_page(
            "page1",
            odp_title("Ventas por región y año")
            + odp_frame(
                '<draw:object xlink:href="./Object 1" xlink:type="simple" '
                'xlink:show="embed" xlink:actuate="onLoad"/>',
                style="gr-plain", style_attr="draw",
                box='svg:x="1.4cm" svg:y="4.5cm" svg:width="12cm" svg:height="8cm"',
                extra='draw:name="Gráfico 1"'),
            layout="AL2T1",
        ),
        extra_parts=odp_chart_parts(
            "Object 1",
            [("2025", ["1200", "980", "1440"]), ("2026", ["1310", "1015", "1502"])],
            categories=["Norte", "Sur", "Este"],
            legend=True,
        ),
        title="Gráfico de varias series",
    )


def odp_ole_embedded() -> None:
    """`draw:object-ole` with the replacement image beside it.

    ODF splits what OOXML keeps in one place: the OLE stream is a package
    member of its own, and its preview lives under `ObjectReplacements/` rather
    than as a relationship from the shape. Neither has a DocMark
    representation, so both sides end as a raw block with a visible stub."""
    build_odp(
        "ole-embedded.odp",
        odp_page(
            "page1",
            odp_title("Objeto incrustado")
            + odp_frame(
                '<draw:object-ole xlink:href="./Object 2" xlink:type="simple" '
                'xlink:show="embed" xlink:actuate="onLoad"/>'
                '<draw:image xlink:href="./ObjectReplacements/Object 2" '
                'xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>',
                style="gr-plain", style_attr="draw",
                box='svg:x="4cm" svg:y="5cm" svg:width="6cm" svg:height="4cm"',
                extra='draw:name="Objeto 1"'),
            layout="AL2T1",
        ),
        extra_parts={
            "Object 2": (b"docsai corpus OLE stream placeholder\n",
                         "application/vnd.sun.star.oleobject"),
            "ObjectReplacements/Object 2": (RED_PNG, "image/png"),
        },
        title="OLE incrustado",
    )


def odp_comments_slide() -> None:
    """`office:annotation`: author, date and body, the `.odp` half of
    `pptx/comments-slide.pptx`.

    Where the annotation may sit is a finding, not a detail. ODF 1.3 admits
    `office:annotation` in paragraph content and in a table cell, and **not** as
    a child of `draw:page`, which is where a slide comment intuitively belongs
    and where a first draft of this fixture put it. Two things then said so:
    the RELAX NG arm of `schema_gate.py` («Did not expect element annotation
    there») and LibreOffice, which opened that deck and dropped both comments
    without a word. So the annotations live in the text they are about, which
    is both legal and what survives a renderer.

    The other half of the mirror: OOXML keeps comments in their own part with a
    separate author table, so `pptx/comments-slide.pptx` can have the two fall
    out of step. ODF writes the author into the annotation, so it cannot."""
    def annotation(name: str, author: str, date: str, body: str) -> str:
        return (
            f'<office:annotation office:name="{name}" '
            f'svg:x="24cm" svg:y="1cm" svg:width="3cm" svg:height="2cm">'
            f"<dc:creator>{author}</dc:creator>"
            f"<dc:date>{date}</dc:date>"
            f"<text:p>{body}</text:p>"
            f"</office:annotation>"
        )

    first = annotation("__Annot__1", "Ana Revisora", "2026-01-02T10:15:00",
                       "Confirmar la cifra del norte antes del consejo.")
    second = annotation("__Annot__2", "Luis Editor", "2026-01-02T11:40:00",
                        "De acuerdo, lo verifico.")
    build_odp(
        "comments-slide.odp",
        odp_page(
            "page1",
            odp_frame(
                odp_text_box(
                    f"<text:p>{first}Página comentada</text:p>"
                ),
                cls="title", box=ODP_TITLE_BOX, style="pr-title")
            + odp_frame(
                odp_text_box(
                    "<text:list><text:list-item>"
                    f"<text:p>Cifra pendiente de confirmar{second}</text:p>"
                    "</text:list-item></text:list>"
                ),
                cls="outline", box=ODP_OUTLINE_BOX, style="pr-outline"),
        ),
        title="Comentarios de página",
    )


def odp_forty_slides() -> None:
    """Not a trait but a **size**: forty pages, the `.odp` counterpart of
    `pptx/forty-slides.pptx`. It is what a per-page cost or a streaming reader
    is measured on; every other fixture here is too small to mean anything."""
    pages = "".join(
        odp_title_body(
            f"page{index}",
            f"Página {index}",
            [(f"Punto {index}.1", 0), (f"Punto {index}.2", 0)],
        )
        for index in range(1, 41)
    )
    build_odp("forty-slides.odp", pages, title="Cuarenta páginas")


# --------------------------------------------------------------------------
# Legacy .doc fixtures (Phase 5)
# --------------------------------------------------------------------------
# Built by docsai_office::doc::test_fixture and embedded so generate.py stays
# dependency-free. Regenerate the constants by writing the fixtures with the
# Rust helper and re-base64-encoding them.


BASIC_TEXT_DOC_B64 = (
    "0M8R4KGxGuEAAAAAAAAAAAAAAAAAAAAAPgADAP7/CQAGAAAAAAAAAAAAAAABAAAAAQAAAAAAAAAA"
    "EAAAAgAAAAEAAAD+////AAAAAAAAAAD/////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "///////////////////////////////////////////////////////////////////////////9"
    "/////v////7///8EAAAABQAAAP7/////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "/////////////////////////////////////////////////////////////////////////1IA"
    "bwBvAHQAIABFAG4AdAByAHkAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAWAAUB//////////8BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AwAAAEAEAAAAAAAAVwBvAHIAZABEAG8AYwB1AG0AZQBuAHQAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAABoAAgECAAAA//////////8AAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAzAMAAAAAAAAxAFQAYQBiAGwAZQAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADgACAf///////////////wAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAAAAVAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAA////////"
    "////////AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAA"
    "AAIAAAADAAAABAAAAAUAAAAGAAAABwAAAAgAAAAJAAAACgAAAAsAAAAMAAAADQAAAA4AAAAPAAAA"
    "/v////7/////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "///////////////////////////////////////////////////////////////////////spcEA"
    "AAAJBAAAABK/AAAAAAAAAAAAAAAAAAAAAAAAAA4AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "ABYAzAMAAAAAAAAAAAAAJAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAF0AAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAVAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABIAGUAbABsAG8AIABmAHIA"
    "bwBtACAAUABoAGEAcwBlACAANQANAFMAZQBjAG8AbgBkACAAcABhAHIAYQBnAHIAYQBwAGgADQAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAhAAAAAA"
    "AAAAJAAAAAAAhAMAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
)

ENCRYPTED_DOC_B64 = (
    "0M8R4KGxGuEAAAAAAAAAAAAAAAAAAAAAPgADAP7/CQAGAAAAAAAAAAAAAAABAAAAAQAAAAAAAAAA"
    "EAAAAgAAAAEAAAD+////AAAAAAAAAAD/////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "///////////////////////////////////////////////////////////////////////////9"
    "/////v////7///8EAAAA/v//////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "/////////////////////////////////////////////////////////////////////////1IA"
    "bwBvAHQAIABFAG4AdAByAHkAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAWAAUB//////////8BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AwAAAAAEAAAAAAAAVwBvAHIAZABEAG8AYwB1AG0AZQBuAHQAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAABoAAgECAAAA//////////8AAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAkgMAAAAAAAAxAFQAYQBiAGwAZQAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADgACAf///////////////wAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA8AAAAVAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAA////////"
    "////////AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAA"
    "AAIAAAADAAAABAAAAAUAAAAGAAAABwAAAAgAAAAJAAAACgAAAAsAAAAMAAAADQAAAA4AAAD+////"
    "/v//////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "////////////////////////////////////////////////////////////////////////////"
    "///////////////////////////////////////////////////////////////////////spcEA"
    "AAAJBAAAABO/AAAAAAAAAAAAAAAAAAAAAAAAAA4AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "ABYAkgMAAAAAAAAAAAAABwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAF0AAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAVAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABzAGUAYwByAGUAdAANAAAA"
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACEAAAAAAAAAAHAAAA"
    "AACEAwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
)


def _write_doc(name: str, b64: str) -> None:
    out = ROOT / "doc" / name
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(base64.b64decode(b64))


def doc_basic_text() -> None:
    """Native-path paragraphs for the degraded MS-DOC reader."""
    _write_doc("basic-text.doc", BASIC_TEXT_DOC_B64)


def doc_encrypted() -> None:
    """FIB fEncrypted set — must be rejected with a clear error."""
    _write_doc("encrypted.doc", ENCRYPTED_DOC_B64)


# --------------------------------------------------------------------------
# PPTX corpus (Phase 12 — the spikes read these; there is no reader yet)
# --------------------------------------------------------------------------
#
# Built by hand like every other fixture here, not with `python-pptx`: the plan
# named that library, but this generator is deliberately dependency-free on the
# three CI platforms, and a fixture whose XML lives in an opaque library is a
# fixture nobody can review. The rationale is written down in
# `kb/29-phase-12-plan.md`.
#
# A slide is not a flow document: it is a canvas of positioned shapes whose
# meaning comes from the placeholder cascade slide -> layout -> master. The
# package below is the smallest one that carries a real cascade, because a
# corpus that flattened it would answer none of the questions Phase 12 asks.

P_NS = (
    'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
    'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" '
    'xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"'
)

PML_TYPE = "application/vnd.openxmlformats-officedocument.presentationml"
DML_TYPE = "application/vnd.openxmlformats-officedocument.drawingml"

# A 4:3 deck at 10 x 7.5 in. Round EMU numbers keep the geometry readable in a
# diff, which is the point of generating the corpus rather than recording it.
SLIDE_W = 9144000
SLIDE_H = 6858000


def _scheme_colour(tag: str, value: str) -> str:
    return f'<a:{tag}><a:srgbClr val="{value}"/></a:{tag}>'


# The theme is required by the master and PowerPoint validates it strictly: all
# twelve scheme colours, both font slots, and three entries in each of the four
# format lists. Anything less and the package opens with a repair dialog, which
# is the exact failure Phase 12 exists to measure.
THEME = (
    '<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="docsai">'
    "<a:themeElements>"
    '<a:clrScheme name="docsai">'
    '<a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>'
    '<a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>'
    + _scheme_colour("dk2", "1F3864")
    + _scheme_colour("lt2", "E7E6E6")
    + _scheme_colour("accent1", "1E5AC8")
    + _scheme_colour("accent2", "C82828")
    + _scheme_colour("accent3", "14A03C")
    + _scheme_colour("accent4", "E0A000")
    + _scheme_colour("accent5", "7030A0")
    + _scheme_colour("accent6", "00B0B0")
    + _scheme_colour("hlink", "0563C1")
    + _scheme_colour("folHlink", "954F72")
    + "</a:clrScheme>"
    '<a:fontScheme name="docsai">'
    '<a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>'
    '<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>'
    "</a:fontScheme>"
    '<a:fmtScheme name="docsai">'
    "<a:fillStyleLst>"
    '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>'
    '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>'
    '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>'
    "</a:fillStyleLst>"
    "<a:lnStyleLst>"
    '<a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>'
    '<a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>'
    '<a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>'
    "</a:lnStyleLst>"
    "<a:effectStyleLst>"
    "<a:effectStyle><a:effectLst/></a:effectStyle>"
    "<a:effectStyle><a:effectLst/></a:effectStyle>"
    "<a:effectStyle><a:effectLst/></a:effectStyle>"
    "</a:effectStyleLst>"
    "<a:bgFillStyleLst>"
    '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>'
    '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>'
    '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>'
    "</a:bgFillStyleLst>"
    "</a:fmtScheme>"
    "</a:themeElements>"
    "</a:theme>"
)


def sp_tree(shapes: str, *, name: str = "") -> str:
    """The shape tree every slide, layout and master part wraps its shapes in.

    ``name`` is `p:cSld@name`. Real layouts carry one — "Title and Content" is
    what a deck's author sees — and it is what `docsai inspect` reports as the
    layout of a slide, so at least one part in the corpus has to have it.
    """
    attr = f' name="{name}"' if name else ""
    return (
        f"<p:cSld{attr}><p:spTree>"
        '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>'
        '<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/>'
        '<a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>'
        f"{shapes}"
        "</p:spTree></p:cSld>"
    )


def xfrm(x: int, y: int, cx: int, cy: int) -> str:
    return f'<a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>'


def a_p(text: str, *, lvl: int = 0, bullet: str = "") -> str:
    """A DrawingML paragraph. An empty ``text`` gives an empty paragraph, which
    is content: it holds its place in the box, exactly as the empty `w:p` the
    Phase 1 reader used to drop did.

    ``bullet`` is child content of `a:pPr`, not an attribute: a paragraph may
    carry exactly one `a:pPr`, so the bullet override has to go inside the one
    the level already needs.
    """
    if lvl or bullet:
        attrs = f' lvl="{lvl}"' if lvl else ""
        props = f"<a:pPr{attrs}>{bullet}</a:pPr>" if bullet else f"<a:pPr{attrs}/>"
    else:
        props = ""
    if not text:
        return f"<a:p>{props}<a:endParaRPr/></a:p>"
    return f'<a:p>{props}<a:r><a:rPr lang="es-ES"/><a:t>{text}</a:t></a:r></a:p>'


#: The *other* legal spelling of an empty paragraph. `<a:p/>` and the
#: `<a:p><a:endParaRPr/></a:p>` :func:`a_p` writes say the same thing — a
#: paragraph with no runs and no properties — and both are what real producers
#: emit. The corpus carries one on purpose: the writer only ever produces the
#: long form, so a deck spelling it the short way is the case that proves the
#: writer compares a rewritten body against what the part already *says* rather
#: than against its bytes. It did not, until `kb/73-phase-15-carried.md`.
A_P_SHORT = "<a:p/>"


def tx_body(paragraphs: str, *, body_pr: str = "<a:bodyPr/>") -> str:
    """A `p:txBody`. A text body with **no** `a:p` at all is not schema-valid —
    ECMA-376 `CT_TextBody` requires at least one paragraph, and no application
    writes one — so a caller that passes no paragraph gets the empty paragraph
    a real placeholder carries, `a:endParaRPr` included. Found by
    `corpus/schema_gate.py`."""
    return f"<p:txBody>{body_pr}<a:lstStyle/>{paragraphs or a_p('')}</p:txBody>"


def shape(
    ident: int,
    name: str,
    paragraphs: str,
    *,
    ph: str = "",
    geom: str = "rect",
    frame: str = "",
    body_pr: str = "<a:bodyPr/>",
    sppr_extra: str = "",
) -> str:
    """A `p:sp`. With ``ph`` it is a placeholder and inherits from the layout;
    without it, it is a free shape and must carry its own geometry."""
    nv_pr = f"<p:nvPr>{ph}</p:nvPr>" if ph else "<p:nvPr/>"
    return (
        "<p:sp>"
        f'<p:nvSpPr><p:cNvPr id="{ident}" name="{name}"/>'
        f'<p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr>{nv_pr}</p:nvSpPr>'
        f'<p:spPr>{frame}<a:prstGeom prst="{geom}"><a:avLst/></a:prstGeom>{sppr_extra}</p:spPr>'
        f"{tx_body(paragraphs, body_pr=body_pr)}"
        "</p:sp>"
    )


def ph(kind: str, idx: int | None = None) -> str:
    attr = f' idx="{idx}"' if idx is not None else ""
    return f'<p:ph type="{kind}"{attr}/>'


# The master defines the two placeholder positions the whole corpus inherits,
# plus the text styles a slide that emits no properties of its own falls back
# to. `placeholders-cascade.pptx` is the fixture that proves the fall-back is
# real, so these values have to be the only place they are stated.
SLIDE_MASTER = (
    f'<p:sldMaster {P_NS}>'
    + sp_tree(
        shape(2, "Title Placeholder 1", a_p("Click to edit Master title style"),
              ph=ph("title"),
              frame=xfrm(838200, 365125, 7467600, 1325563))
        + shape(3, "Text Placeholder 2", a_p("Click to edit Master text styles"),
                ph=ph("body", 1),
                frame=xfrm(838200, 1825625, 7467600, 4351338))
    )
    + '<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1"'
    ' accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5"'
    ' accent6="accent6" hlink="hlink" folHlink="folHlink"/>'
    '<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rIdLayout1"/></p:sldLayoutIdLst>'
    "<p:txStyles>"
    '<p:titleStyle><a:lvl1pPr><a:defRPr sz="4400"><a:solidFill>'
    '<a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mj-lt"/></a:defRPr></a:lvl1pPr></p:titleStyle>'
    "<p:bodyStyle>"
    + "".join(
        f'<a:lvl{i}pPr marL="{342900 * i}" indent="-342900">'
        f'<a:buChar char="•"/><a:defRPr sz="{2800 - 400 * (i - 1)}">'
        '<a:solidFill><a:schemeClr val="tx1"/></a:solidFill>'
        f'<a:latin typeface="+mn-lt"/></a:defRPr></a:lvl{i}pPr>'
        for i in range(1, 5)
    )
    + "</p:bodyStyle>"
    "<p:otherStyle><a:lvl1pPr><a:defRPr/></a:lvl1pPr></p:otherStyle>"
    "</p:txStyles>"
    "</p:sldMaster>"
)

SLIDE_LAYOUT = (
    f'<p:sldLayout {P_NS} type="obj" preserve="1">'
    + sp_tree(
        shape(2, "Title 1", "", ph=ph("title"))
        + shape(3, "Content Placeholder 2", "", ph=ph("body", 1)),
        name="Titulo y objetos",
    )
    + "</p:sldLayout>"
)


def build_pptx(
    name: str,
    slides: list[str],
    *,
    title: str,
    order: list[int] | None = None,
    media: dict[str, bytes] | None = None,
    slide_rels: dict[int, list[tuple[str, str, str]]] | None = None,
    notes: dict[int, str] | None = None,
    notes_for: dict[int, int] | None = None,
    extra_parts: dict[str, bytes] | None = None,
    extra_overrides: list[tuple[str, str]] | None = None,
    extra_defaults: dict[str, str] | None = None,
    comment_authors: list[tuple[int, str, str]] | None = None,
    comments: dict[int, str] | None = None,
    macro_enabled: bool = False,
    subdir: str = "pptx",
) -> None:
    """Assemble a .pptx package.

    ``subdir`` is the corpus directory the package lands in. It is `pptx` for
    every deck of the deck corpus and only the Phase 19 legacy sources pass
    anything else: those packages are the *input* to a `.ppt` conversion, not
    decks the gates measure, and living outside `corpus/pptx` is what says so.

    ``slides`` are the `p:sld` bodies in file order (`slide1.xml` first).
    ``order`` is the presentation order as 1-based slide numbers; when it
    differs from the file order it is `p:sldIdLst` that wins, which is the whole
    point of the `slide-order` fixture. ``slide_rels`` entries are
    ``(rId, type_suffix, target)`` relative to `ppt/slides/`.

    ``notes`` are the notes parts by file number; ``notes_for`` maps a slide
    number to the notes part it points at, which defaults to the same number
    because that is PowerPoint's habit. It is only a habit: the relationship is
    what binds a note to a slide, and the `notes-crossed` fixture proves it by
    disagreeing with the numbering.

    ``comment_authors`` are ``(id, name, initials)`` for `ppt/commentAuthors.xml`,
    a deck-wide part hanging off `ppt/presentation.xml`; ``comments`` maps a
    slide number to the body of its `p:cmLst`. The two are separate parts
    because PresentationML keeps them apart: a comment names its author by a
    number that only the author list explains.
    """
    media = media or {}
    slide_rels = slide_rels or {}
    comments = comments or {}
    notes = notes or {}
    notes_for = notes_for or {index: index for index in notes}
    order = order or list(range(1, len(slides) + 1))

    parts: dict[str, bytes] = {}
    defaults = {
        "rels": "application/vnd.openxmlformats-package.relationships+xml",
        "xml": "application/xml",
    }
    defaults.update(extra_defaults or {})
    main_type = (
        "application/vnd.ms-powerpoint.presentation.macroEnabled.main+xml"
        if macro_enabled
        else f"{PML_TYPE}.presentation.main+xml"
    )
    overrides = [
        ("/ppt/presentation.xml", main_type),
        ("/ppt/slideMasters/slideMaster1.xml", f"{PML_TYPE}.slideMaster+xml"),
        ("/ppt/slideLayouts/slideLayout1.xml", f"{PML_TYPE}.slideLayout+xml"),
        ("/ppt/theme/theme1.xml", f"{DML_TYPE}.theme+xml"),
        ("/docProps/core.xml", "application/vnd.openxmlformats-package.core-properties+xml"),
        ("/docProps/app.xml", "application/vnd.openxmlformats-officedocument.extended-properties+xml"),
    ]
    overrides.extend(extra_overrides or [])

    parts["docProps/core.xml"] = core_props(title)
    parts["docProps/app.xml"] = APP_PROPS
    parts["ppt/theme/theme1.xml"] = (XML_DECL + THEME).encode()
    parts["ppt/slideMasters/slideMaster1.xml"] = (XML_DECL + SLIDE_MASTER).encode()
    parts["ppt/slideLayouts/slideLayout1.xml"] = (XML_DECL + SLIDE_LAYOUT).encode()

    parts["ppt/slideMasters/_rels/slideMaster1.xml.rels"] = rels_part([
        ("rIdLayout1", "slideLayout", "../slideLayouts/slideLayout1.xml"),
        ("rIdTheme", "theme", "../theme/theme1.xml"),
    ])
    parts["ppt/slideLayouts/_rels/slideLayout1.xml.rels"] = rels_part([
        ("rIdMaster", "slideMaster", "../slideMasters/slideMaster1.xml"),
    ])

    presentation_rels = [("rIdMaster1", "slideMaster", "slideMasters/slideMaster1.xml")]
    if macro_enabled:
        # Not a real VBA project: an OLE2 header and filler, enough for the part
        # to exist with its content type and relationship. The reader must never
        # look inside it, so a fixture that could actually run something would be
        # a hazard with no test value.
        parts["ppt/vbaProject.bin"] = bytes.fromhex("d0cf11e0a1b11ae1") + b"\x00" * 504
        defaults["bin"] = "application/vnd.ms-office.vbaProject"
        presentation_rels.append((
            "rIdVba",
            "http://schemas.microsoft.com/office/2006/relationships/vbaProject",
            "vbaProject.bin",
        ))
    slide_ids = []
    for position, number in enumerate(order):
        rid = f"rIdSlide{number}"
        presentation_rels.append((rid, "slide", f"slides/slide{number}.xml"))
        slide_ids.append(f'<p:sldId id="{256 + position}" r:id="{rid}"/>')

    for index, body in enumerate(slides, start=1):
        part = f"ppt/slides/slide{index}.xml"
        parts[part] = (XML_DECL + body).encode()
        overrides.append((f"/{part}", f"{PML_TYPE}.slide+xml"))
        entries = [("rIdLayout", "slideLayout", "../slideLayouts/slideLayout1.xml")]
        if index in comments:
            entries.append(("rIdComments", "comments", f"../comments/comment{index}.xml"))
        entries.extend(slide_rels.get(index, []))
        if index in notes_for:
            entries.append(
                ("rIdNotes", "notesSlide", f"../notesSlides/notesSlide{notes_for[index]}.xml")
            )
        parts[f"ppt/slides/_rels/slide{index}.xml.rels"] = rels_part(entries)

    if comment_authors:
        parts["ppt/commentAuthors.xml"] = (XML_DECL + (
            f'<p:cmAuthorLst {P_NS}>'
            + "".join(
                f'<p:cmAuthor id="{ident}" name="{name}" initials="{initials}"'
                f' lastIdx="1" clrIdx="{ident}"/>'
                for ident, name, initials in comment_authors
            )
            + "</p:cmAuthorLst>"
        )).encode()
        overrides.append(("/ppt/commentAuthors.xml", f"{PML_TYPE}.commentAuthors+xml"))
        presentation_rels.append(("rIdCmAuthors", "commentAuthors", "commentAuthors.xml"))
    for index, body in sorted(comments.items()):
        part = f"ppt/comments/comment{index}.xml"
        parts[part] = (XML_DECL + f'<p:cmLst {P_NS}>{body}</p:cmLst>').encode()
        overrides.append((f"/{part}", f"{PML_TYPE}.comments+xml"))
        parts[f"ppt/comments/_rels/comment{index}.xml.rels"] = rels_part([
            ("rIdSlide", "slide", f"../slides/slide{index}.xml"),
        ])

    if notes:
        parts["ppt/notesMasters/notesMaster1.xml"] = (XML_DECL + NOTES_MASTER).encode()
        parts["ppt/notesMasters/_rels/notesMaster1.xml.rels"] = rels_part([
            ("rIdTheme", "theme", "../theme/theme1.xml"),
        ])
        overrides.append(("/ppt/notesMasters/notesMaster1.xml", f"{PML_TYPE}.notesMaster+xml"))
        presentation_rels.append(("rIdNotesMaster", "notesMaster", "notesMasters/notesMaster1.xml"))
        owner = {note: slide for slide, note in notes_for.items()}
        for index, body in sorted(notes.items()):
            part = f"ppt/notesSlides/notesSlide{index}.xml"
            parts[part] = (XML_DECL + body).encode()
            overrides.append((f"/{part}", f"{PML_TYPE}.notesSlide+xml"))
            parts[f"ppt/notesSlides/_rels/notesSlide{index}.xml.rels"] = rels_part([
                ("rIdSlide", "slide", f"../slides/slide{owner.get(index, index)}.xml"),
                ("rIdNotesMaster", "notesMaster", "../notesMasters/notesMaster1.xml"),
            ])

    for filename, blob in sorted(media.items()):
        parts[f"ppt/media/{filename}"] = blob
        extension = Path(filename).suffix
        defaults[extension.lstrip(".")] = CONTENT_TYPE[extension]

    for part, blob in sorted((extra_parts or {}).items()):
        parts[part] = blob

    parts["ppt/presentation.xml"] = (XML_DECL + (
        f'<p:presentation {P_NS}>'
        '<p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rIdMaster1"/></p:sldMasterIdLst>'
        + ("<p:notesMasterIdLst><p:notesMasterId r:id=\"rIdNotesMaster\"/></p:notesMasterIdLst>"
           if notes else "")
        + f'<p:sldIdLst>{"".join(slide_ids)}</p:sldIdLst>'
        f'<p:sldSz cx="{SLIDE_W}" cy="{SLIDE_H}"/>'
        f'<p:notesSz cx="{SLIDE_H}" cy="{SLIDE_W}"/>'
        "</p:presentation>"
    )).encode()
    parts["ppt/_rels/presentation.xml.rels"] = rels_part(presentation_rels)

    parts["[Content_Types].xml"] = content_types(defaults, overrides)
    parts["_rels/.rels"] = rels_part([
        ("rId1", "officeDocument", "ppt/presentation.xml"),
        ("rId2", "metadata/core-properties", "docProps/core.xml"),
        ("rId3", "extended-properties", "docProps/app.xml"),
    ], root=True)

    write_package(ROOT / subdir / name, parts)


NOTES_MASTER = (
    f'<p:notesMaster {P_NS}>'
    + sp_tree(
        shape(2, "Notes Placeholder 1", "", ph=ph("body", 1),
              frame=xfrm(685800, 2971800, 5486400, 4114800))
    )
    + '<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1"'
    ' accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5"'
    ' accent6="accent6" hlink="hlink" folHlink="folHlink"/>'
    "</p:notesMaster>"
)


def slide(shapes: str, *, show_master: bool = True, extra: str = "") -> str:
    """A `p:sld`. ``extra`` is slide-level markup after the shape tree —
    `p:transition`, `p:timing` — which the schema requires in that order."""
    attr = "" if show_master else ' showMasterSp="0"'
    return f'<p:sld {P_NS}{attr}>{sp_tree(shapes)}{extra}</p:sld>'


def notes_slide(index: int, text: str) -> str:
    """A notes slide. The body placeholder is what an agent actually wants to
    read; the slide-image placeholder is what PowerPoint puts above it.

    The root element is `p:notes`, not `p:notesSlide`: the part is named
    `notesSlide` and so is its content type, but ECMA-376 binds `CT_NotesSlide`
    to the element `notes`. Spike P1 found this the hard way — the reader
    refused the part with `expected NotesSlide, found p:notesSlide`.
    """
    return (
        f'<p:notes {P_NS}>'
        + sp_tree(
            '<p:sp><p:nvSpPr><p:cNvPr id="2" name="Slide Image Placeholder 1"/>'
            '<p:cNvSpPr><a:spLocks noGrp="1" noRot="1" noChangeAspect="1"/></p:cNvSpPr>'
            '<p:nvPr><p:ph type="sldImg"/></p:nvPr></p:nvSpPr>'
            "<p:spPr/></p:sp>"
            + shape(3, "Notes Placeholder 2",
                    "".join(a_p(line) for line in text.split("\n")),
                    ph=ph("body", 1))
        )
        + "</p:notes>"
    )


def title_body_slide(title: str, bullets: list[tuple[str, int]]) -> str:
    """The shape every fixture that is not about geometry starts from: a title
    placeholder and a body placeholder, neither carrying a single property of
    its own so the cascade is what decides how they look."""
    return slide(
        shape(2, "Title 1", a_p(title), ph=ph("title"))
        + shape(3, "Content Placeholder 2",
                "".join(a_p(text, lvl=lvl) for text, lvl in bullets),
                ph=ph("body", 1))
    )


def pptx_basic_slides() -> None:
    """Two slides, title + body, nothing else. The floor of the format."""
    build_pptx(
        "basic-slides.pptx",
        [
            title_body_slide("Informe trimestral", [
                ("Ingresos al alza", 0),
                ("Costes estables", 0),
            ]),
            title_body_slide("Siguientes pasos", [("Cerrar el trimestre", 0)]),
        ],
        title="Presentación básica",
    )


def pptx_slide_order() -> None:
    """`p:sldIdLst` disagrees with the file names on purpose: the reader that
    trusts `slideN.xml` gets this deck backwards, and the golden would not say
    so because both orders contain the same three slides."""
    build_pptx(
        "slide-order.pptx",
        [
            title_body_slide("Tercera en el orden", [("Va la última", 0)]),
            title_body_slide("Primera en el orden", [("Va la primera", 0)]),
            title_body_slide("Segunda en el orden", [("Va en medio", 0)]),
        ],
        order=[2, 3, 1],
        title="Orden de diapositivas",
    )


def pptx_placeholders_cascade() -> None:
    """A title and a body that state nothing: position, size, font and bullet
    all come from the layout and the master. A reader that emits any attribute
    for these shapes has failed the delta rule, and the writer that emits them
    back destroys the design."""
    build_pptx(
        "placeholders-cascade.pptx",
        [title_body_slide("Todo heredado", [
            ("Sin posición propia", 0),
            ("Sin tipografía propia", 0),
        ])],
        title="Cascada de placeholders",
    )


def pptx_placeholders_empty() -> None:
    """An empty placeholder is content: it holds a position the layout gave it
    and a user is expected to fill it. Phase 1 shipped a bug that dropped empty
    paragraphs; this is where the same bug would land in a deck.

    The two spellings of an empty paragraph are both here on purpose: the body
    placeholder uses the short `<a:p/>` and the text placeholder the long
    `<a:p><a:endParaRPr/></a:p>`. They say the same thing, and a writer that
    rewrote one into the other would break byte identity on a shape nobody
    edited — which it did until `kb/73-phase-15-carried.md`."""
    build_pptx(
        "placeholders-empty.pptx",
        [slide(
            shape(2, "Title 1", a_p("Título con cuerpo vacío"), ph=ph("title"))
            + shape(3, "Content Placeholder 2", A_P_SHORT, ph=ph("body", 1))
            + shape(4, "Text Placeholder 3", a_p("Primera línea") + a_p("") + a_p("Tras el hueco"),
                    ph=ph("body", 2), frame=xfrm(838200, 4500000, 7467600, 1500000))
        )],
        title="Placeholders vacíos",
    )


def pptx_bullets_levels() -> None:
    """`a:pPr@lvl` nesting with both bullet kinds. Levels map to Markdown list
    depth, which is the one part of a slide that is genuinely a flow document."""
    build_pptx(
        "bullets-levels.pptx",
        [slide(
            shape(2, "Title 1", a_p("Niveles de viñeta"), ph=ph("title"))
            + shape(3, "Content Placeholder 2",
                    a_p("Primer nivel", lvl=0)
                    + a_p("Segundo nivel", lvl=1)
                    + a_p("Tercer nivel", lvl=2)
                    + a_p("Vuelve al primero", lvl=0),
                    ph=ph("body", 1))
            + shape(4, "Text Placeholder 3",
                    a_p("Uno", bullet='<a:buAutoNum type="arabicPeriod"/>')
                    + a_p("Dos", bullet='<a:buAutoNum type="arabicPeriod"/>'),
                    ph=ph("body", 2), frame=xfrm(838200, 5200000, 7467600, 1000000))
        )],
        title="Viñetas y niveles",
    )


def pptx_notes_speaker() -> None:
    """Speaker notes: the cheapest part of a deck to read and the densest in
    intent. An agent asked what a slide is *for* answers from here."""
    build_pptx(
        "notes-speaker.pptx",
        [
            title_body_slide("Resultados", [("Crecimiento del 12 %", 0)]),
            title_body_slide("Riesgos", [("Dependencia de un proveedor", 0)]),
        ],
        notes={
            1: notes_slide(1, "Insistir en que el 12 % es interanual.\nNo entrar en el desglose por región."),
            2: notes_slide(2, "Si preguntan por el proveedor, remitir al anexo."),
        },
        title="Notas del ponente",
    )


def pptx_notes_crossed() -> None:
    """The notes part numbers disagree with the slide numbers on purpose.

    `notesSlide1.xml` belongs to slide 2 and `notesSlide2.xml` to slide 1, which
    is legal: the `notesSlide` relationship in the slide's own `_rels` is what
    binds them. A reader that pairs them by number puts every note under the
    wrong slide — silently, and with text that reads plausibly where it lands.
    """
    build_pptx(
        "notes-crossed.pptx",
        [
            title_body_slide("Primera", [("Contenido de la primera", 0)]),
            title_body_slide("Segunda", [("Contenido de la segunda", 0)]),
        ],
        notes={
            1: notes_slide(2, "Nota de la segunda diapositiva."),
            2: notes_slide(1, "Nota de la primera diapositiva."),
        },
        notes_for={1: 2, 2: 1},
        title="Notas cruzadas",
    )


def pptx_reading_order() -> None:
    """A `p:spTree` whose order is not the order anybody reads the slide in.

    The title comes fourth in the tree and a footnote box comes first, which is
    what happens the moment somebody sends a shape to the back or adds the
    title last: `spTree` order is z-order, not reading order. Two of the free
    boxes are 10 000 EMU apart vertically — under a hundredth of an inch — so
    they are one row, and a reader that compares tops exactly reads them
    top-down instead of left-to-right.

    Every other deck in the corpus is written in an order that happens to
    already be the reading order, so none of them can tell the policy from
    trusting the file.
    """
    build_pptx(
        "reading-order.pptx",
        [slide(
            shape(4, "Pie 1", a_p("Al pie de la diapositiva"),
                  frame=xfrm(838200, 5000000, 2000000, 500000))
            + shape(3, "Content Placeholder 2", a_p("Cuerpo heredado"), ph=ph("body", 1))
            + shape(5, "Etiqueta derecha", a_p("A la derecha"),
                    frame=xfrm(5000000, 2000000, 1500000, 500000))
            + shape(2, "Title 1", a_p("El título va el cuarto"), ph=ph("title"))
            + shape(6, "Marca izquierda", a_p("A la izquierda"),
                    frame=xfrm(838200, 2010000, 1500000, 500000))
        )],
        title="Orden de lectura",
    )


def pptx_raw_preserved() -> None:
    """The four things a slide can hold that the IR has no node for.

    A group (`p:grpSp`), a custom geometry (`a:custGeom`), and the two
    slide-level subtrees a deck animates itself with (`p:transition`,
    `p:timing`). None of them has a Markdown equivalent, all of them must
    survive the round trip as raw fragments behind a visible stub, and none of
    the other decks in the corpus carries one — the connector in
    `shapes-geometry` and the SmartArt in `smartart-fallback` cover the rest.

    The group holds text on purpose: a stub that swallowed it would hide real
    content behind a payload no agent is meant to read.
    """
    group = (
        "<p:grpSp>"
        '<p:nvGrpSpPr><p:cNvPr id="7" name="Grupo 1"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>'
        '<p:grpSpPr><a:xfrm><a:off x="838200" y="2000000"/><a:ext cx="3000000" cy="1200000"/>'
        '<a:chOff x="838200" y="2000000"/><a:chExt cx="3000000" cy="1200000"/></a:xfrm></p:grpSpPr>'
        + shape(8, "Caja agrupada 1", a_p("Dentro del grupo"),
                frame=xfrm(838200, 2000000, 1400000, 500000))
        + shape(9, "Caja agrupada 2", a_p("También dentro"),
                frame=xfrm(2400000, 2000000, 1400000, 500000))
        + "</p:grpSp>"
    )
    # A closed triangle: the path list is the shape, and no preset names it.
    custom = (
        "<p:sp>"
        '<p:nvSpPr><p:cNvPr id="10" name="Triángulo libre"/>'
        '<p:cNvSpPr/><p:nvPr/></p:nvSpPr>'
        f'<p:spPr>{xfrm(5000000, 2000000, 1200000, 1200000)}'
        '<a:custGeom><a:avLst/><a:gdLst/><a:pathLst>'
        '<a:path w="1200000" h="1200000">'
        '<a:moveTo><a:pt x="0" y="1200000"/></a:moveTo>'
        '<a:lnTo><a:pt x="600000" y="0"/></a:lnTo>'
        '<a:lnTo><a:pt x="1200000" y="1200000"/></a:lnTo>'
        "<a:close/></a:path></a:pathLst></a:custGeom></p:spPr>"
        + tx_body(a_p("Etiqueta del triángulo"))
        + "</p:sp>"
    )
    transition = (
        '<p:transition spd="slow" advClick="1"><p:fade/></p:transition>'
    )
    timing = (
        "<p:timing><p:tnLst>"
        '<p:par><p:cTn id="1" dur="indefinite" restart="never" nodeType="tmRoot"/></p:par>'
        "</p:tnLst></p:timing>"
    )
    build_pptx(
        "raw-preserved.pptx",
        [slide(
            shape(2, "Title 1", a_p("Lo que no se modela"), ph=ph("title"))
            + group
            + custom,
            extra=transition + timing,
        )],
        title="Preservación en bruto",
    )


def pptx_tables_simple() -> None:
    """`p:graphicFrame` -> `a:tbl`: a different XML model from `w:tbl`, but the
    IR `Table` already covers it, so this fixture is about the wrapper."""
    def tc(text: str) -> str:
        # A DrawingML table cell holds an `a:txBody`, not the `p:txBody` of a
        # shape: same content model, different namespace, and the XSD is the
        # only thing that ever said so out loud (`corpus/schema_gate.py`).
        return (f"<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>{a_p(text)}"
                f"</a:txBody><a:tcPr/></a:tc>")

    rows = (
        f'<a:tr h="370840">{tc("Región")}{tc("Ventas")}</a:tr>'
        f'<a:tr h="370840">{tc("Norte")}{tc("1 200")}</a:tr>'
        f'<a:tr h="370840">{tc("Sur")}{tc("980")}</a:tr>'
    )
    table = (
        "<p:graphicFrame>"
        '<p:nvGraphicFramePr><p:cNvPr id="4" name="Tabla 1"/>'
        '<p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr>'
        "<p:nvPr/></p:nvGraphicFramePr>"
        f'<p:xfrm><a:off x="838200" y="1825625"/><a:ext cx="7467600" cy="1112520"/></p:xfrm>'
        '<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table">'
        '<a:tbl><a:tblPr firstRow="1" bandRow="1"/>'
        '<a:tblGrid><a:gridCol w="3733800"/><a:gridCol w="3733800"/></a:tblGrid>'
        f"{rows}</a:tbl>"
        "</a:graphicData></a:graphic>"
        "</p:graphicFrame>"
    )
    build_pptx(
        "tables-simple.pptx",
        [slide(
            shape(2, "Title 1", a_p("Ventas por región"), ph=ph("title")) + table
        )],
        title="Tabla simple",
    )


def pptx_images_anchored() -> None:
    """A DrawingML picture with explicit geometry. The model is the one already
    implemented for docx and xlsx — this fixture proves it transfers."""
    picture = (
        "<p:pic>"
        '<p:nvPicPr><p:cNvPr id="4" name="Imagen 1" descr="Gráfico de barras azul"/>'
        '<p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>'
        '<p:blipFill><a:blip r:embed="rIdImg"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>'
        f'<p:spPr>{xfrm(1524000, 2286000, px(120), px(90))}'
        '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>'
        "</p:pic>"
    )
    build_pptx(
        "images-anchored.pptx",
        [slide(
            shape(2, "Title 1", a_p("Con imagen"), ph=ph("title")) + picture
        )],
        media={"image1.png": BLUE_PNG},
        slide_rels={1: [("rIdImg", "image", "../media/image1.png")]},
        title="Imagen anclada",
    )


def pptx_shapes_geometry() -> None:
    """Free shapes with `prstGeom` and a connector. None has a Markdown
    equivalent, so all of them are raw-block candidates with a visible stub —
    the decision `docs/technical-analysis-presentations.md` §2 takes up front."""
    connector = (
        "<p:cxnSp>"
        '<p:nvCxnSpPr><p:cNvPr id="6" name="Conector 1"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr>'
        f'<p:spPr>{xfrm(3000000, 3000000, 1200000, 0)}'
        '<a:prstGeom prst="line"><a:avLst/></a:prstGeom></p:spPr>'
        "</p:cxnSp>"
    )
    build_pptx(
        "shapes-geometry.pptx",
        [slide(
            shape(2, "Title 1", a_p("Formas libres"), ph=ph("title"))
            + shape(4, "Rectángulo 1", a_p("Dentro del rectángulo"),
                    geom="roundRect", frame=xfrm(838200, 2000000, 2000000, 900000))
            + shape(5, "Flecha 1", "", geom="rightArrow",
                    frame=xfrm(3200000, 2200000, 1400000, 500000))
            + connector
        )],
        title="Geometría de formas",
    )


def pptx_autofit_stale() -> None:
    """`a:normAutofit` with a `fontScale` PowerPoint computed for text that is
    no longer there. This is risk P5 in a file: an agent that adds a line has
    no way to recompute the scale, and PowerPoint will not until the box is
    edited by hand. The fixture exists so `Warning::AutofitStale` has something
    to fire on."""
    build_pptx(
        "autofit-stale.pptx",
        [slide(
            shape(2, "Title 1", a_p("Texto que no cabe"), ph=ph("title"))
            + shape(3, "Content Placeholder 2",
                    "".join(a_p(f"Línea {i} de un cuerpo deliberadamente largo", lvl=0)
                            for i in range(1, 9)),
                    ph=ph("body", 1),
                    body_pr='<a:bodyPr><a:normAutofit fontScale="62500" lnSpcReduction="20000"/></a:bodyPr>')
        )],
        title="Autofit obsoleto",
    )


DGM_NS = 'xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram"'
DGM_DRAWING_REL = (
    "http://schemas.microsoft.com/office/2007/relationships/diagramDrawing"
)


def embedded_workbook(values: tuple[str, str, str] = ("1200", "980", "1440")) -> bytes:
    """The workbook a chart carries its own data in. Minimal on purpose: it is
    not here to be read, it is here to be *preserved*. Spike P3 uses it as the
    one part of a deck that is itself a package."""
    rows = (
        f'<row r="1">{c("A1", "0", t="s")}{c("B1", "1", t="s")}</row>'
        f'<row r="2">{c("A2", "2", t="s")}{c("B2", values[0])}</row>'
        f'<row r="3">{c("A3", "3", t="s")}{c("B3", values[1])}</row>'
        f'<row r="4">{c("A4", "4", t="s")}{c("B4", values[2])}</row>'
    )
    strings = ["Región", "Ventas", "Norte", "Sur", "Este"]
    items = "".join(f"<si><t>{s}</t></si>" for s in strings)
    parts = {
        "[Content_Types].xml": content_types(
            {"rels": "application/vnd.openxmlformats-package.relationships+xml",
             "xml": "application/xml"},
            [("/xl/workbook.xml",
              "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"),
             ("/xl/worksheets/sheet1.xml",
              "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"),
             ("/xl/sharedStrings.xml",
              "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml")],
        ),
        "_rels/.rels": rels_part([("rId1", "officeDocument", "xl/workbook.xml")], root=True),
        "xl/workbook.xml": (
            XML_DECL
            + f"<workbook {X_NS}><sheets>"
            '<sheet name="Hoja1" sheetId="1" r:id="rIdSheet1"/>'
            "</sheets></workbook>"
        ).encode(),
        "xl/_rels/workbook.xml.rels": rels_part([
            ("rIdSheet1", "worksheet", "worksheets/sheet1.xml"),
            ("rIdSst", "sharedStrings", "sharedStrings.xml"),
        ]),
        "xl/worksheets/sheet1.xml": (XML_DECL + sheet(rows, dim="A1:B4")).encode(),
        "xl/sharedStrings.xml": (
            XML_DECL
            + f'<sst {X_NS} count="{len(strings)}" uniqueCount="{len(strings)}">{items}</sst>'
        ).encode(),
    }
    return package_bytes(parts)


def _chart_str_ref(formula: str, values: list[str]) -> str:
    """`c:strRef`: a formula, and the cache of what it evaluated to."""
    points = "".join(
        f'<c:pt idx="{i}"><c:v>{v}</c:v></c:pt>' for i, v in enumerate(values)
    )
    return (
        f"<c:strRef><c:f>{formula}</c:f><c:strCache>"
        f'<c:ptCount val="{len(values)}"/>{points}</c:strCache></c:strRef>'
    )


def _chart_num_ref(formula: str, values: list[str | None],
                   format_code: str = "General") -> str:
    """`c:numRef`. A `None` is a **gap**: no `c:pt` at that index, and
    `c:ptCount` still counting it. That is how PowerPoint writes a series with
    a hole in it, and a reader that appends the points it finds puts every
    later value on the wrong category."""
    points = "".join(
        f'<c:pt idx="{i}"><c:v>{v}</c:v></c:pt>'
        for i, v in enumerate(values) if v is not None
    )
    return (
        f"<c:numRef><c:f>{formula}</c:f><c:numCache>"
        f"<c:formatCode>{format_code}</c:formatCode>"
        f'<c:ptCount val="{len(values)}"/>{points}</c:numCache></c:numRef>'
    )


CHART_NS = (
    'xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" '
    'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
    'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"'
)


def _chart_axes() -> str:
    return (
        '<c:catAx><c:axId val="111111111"/>'
        '<c:scaling><c:orientation val="minMax"/></c:scaling>'
        '<c:delete val="0"/><c:axPos val="b"/><c:crossAx val="222222222"/></c:catAx>'
        '<c:valAx><c:axId val="222222222"/>'
        '<c:scaling><c:orientation val="minMax"/></c:scaling>'
        '<c:delete val="0"/><c:axPos val="l"/><c:crossAx val="111111111"/></c:valAx>'
    )


def _chart_frame(rel_id: str = "rIdChart") -> str:
    return (
        "<p:graphicFrame>"
        '<p:nvGraphicFramePr><p:cNvPr id="4" name="Gráfico 1"/>'
        "<p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>"
        '<p:xfrm><a:off x="838200" y="1825625"/><a:ext cx="7467600" cy="3600000"/></p:xfrm>'
        '<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart">'
        '<c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" '
        f'r:id="{rel_id}"/>'
        "</a:graphicData></a:graphic>"
        "</p:graphicFrame>"
    )


def _chart_takeaway(*lines: str) -> str:
    """The body placeholder a chart slide carries beside the picture of its
    numbers. A deck whose only content is an object is not what a deck looks
    like, and the residue budget of risk P4 is measured over the corpus as it
    stands."""
    return shape(3, "Content Placeholder 2",
                 "".join(a_p(line) for line in lines), ph=ph("body", 1))


def _bar_chart_xml() -> str:
    """One `c:barChart`, one series, categories and values cached beside the
    formulas that produced them."""
    numbers = ["1200", "980", "1440"]
    number_points = "".join(
        f'<c:pt idx="{i}"><c:v>{v}</c:v></c:pt>' for i, v in enumerate(numbers)
    )
    return (
        XML_DECL
        + f"<c:chartSpace {CHART_NS}>"
        "<c:chart><c:plotArea><c:layout/>"
        '<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/>'
        '<c:ser><c:idx val="0"/><c:order val="0"/>'
        f'<c:tx>{_chart_str_ref("Hoja1!$B$1", ["Ventas"])}</c:tx>'
        f'<c:cat>{_chart_str_ref("Hoja1!$A$2:$A$4", ["Norte", "Sur", "Este"])}</c:cat>'
        "<c:val><c:numRef><c:f>Hoja1!$B$2:$B$4</c:f><c:numCache>"
        "<c:formatCode>General</c:formatCode>"
        f'<c:ptCount val="{len(numbers)}"/>{number_points}'
        "</c:numCache></c:numRef></c:val>"
        "</c:ser>"
        '<c:axId val="111111111"/><c:axId val="222222222"/>'
        "</c:barChart>"
        + _chart_axes()
        + "</c:plotArea><c:plotVisOnly val=\"1\"/></c:chart>"
        "</c:chartSpace>"
    )


def pptx_charts_embedded() -> None:
    """`c:chart` and the workbook it keeps its numbers in. Two things no reader
    of slide XML can reach: a part in a foreign schema, and a part that is a
    whole package. Both have to come back out of a round trip untouched.

    Since Phase 16-A it is also the fixture where the two copies of the data
    **agree**: the cached series and the embedded workbook say the same thing,
    and reading it must produce no warning at all. A false warning is the same
    failure as a missing one."""
    build_pptx(
        "charts-embedded.pptx",
        [slide(shape(2, "Title 1", a_p("Ventas por zona"), ph=ph("title")) + _chart_frame())],
        slide_rels={1: [("rIdChart", "chart", "../charts/chart1.xml")]},
        extra_parts={
            "ppt/charts/chart1.xml": _bar_chart_xml().encode(),
            "ppt/charts/_rels/chart1.xml.rels": rels_part([
                ("rIdWorkbook", "package", "../embeddings/workbook1.xlsx"),
            ]),
            "ppt/embeddings/workbook1.xlsx": embedded_workbook(),
        },
        extra_overrides=[
            ("/ppt/charts/chart1.xml", f"{DML_TYPE}.chart+xml"),
            ("/ppt/embeddings/workbook1.xlsx",
             "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        ],
        title="Gráfico incrustado",
    )


def pptx_charts_stale_workbook() -> None:
    """The same chart, with a workbook that no longer matches its cache: the
    sheet says 999 where the chart displays 980.

    This is not a corrupt file — it is what a deck looks like after someone
    edits the embedded sheet and the chart is never redrawn. The two copies of
    the truth disagree, the cache is what the slide shows and what the model
    holds, and the reader says so instead of averaging them."""
    build_pptx(
        "charts-stale-workbook.pptx",
        [slide(shape(2, "Title 1", a_p("Ventas por zona"), ph=ph("title"))
               + _chart_takeaway(
                   "El norte encabeza el trimestre con mil doscientas unidades.",
                   "La hoja incrustada ya no coincide con lo que dibuja el gráfico.",
                   "Manda lo que la diapositiva muestra, no lo que la hoja guarda.",
                   "La hoja viaja igualmente como activo, porque dice de dónde salieron.",
               )
               + _chart_frame())],
        slide_rels={1: [("rIdChart", "chart", "../charts/chart1.xml")]},
        extra_parts={
            "ppt/charts/chart1.xml": _bar_chart_xml().encode(),
            "ppt/charts/_rels/chart1.xml.rels": rels_part([
                ("rIdWorkbook", "package", "../embeddings/workbook1.xlsx"),
            ]),
            "ppt/embeddings/workbook1.xlsx": embedded_workbook(("1200", "999", "1440")),
        },
        extra_overrides=[
            ("/ppt/charts/chart1.xml", f"{DML_TYPE}.chart+xml"),
            ("/ppt/embeddings/workbook1.xlsx",
             "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        ],
        title="Gráfico desincronizado",
    )


def pptx_charts_multi_series() -> None:
    """A combo chart with no embedded workbook: two plot elements, three
    series, a title in rich text, a hole in a series, a number format that is
    not `General`, and a multi-level category axis.

    Everything a single-series bar chart cannot exercise. The series are
    numbered across *both* plots, because that is how the legend numbers them;
    the second series states no `c:cat` and still plots against the chart's
    categories; the third's categories are a tree the model cannot hold, which
    is a documented loss with a warning rather than a guess. And there is no
    `_rels` part at all, so the workbook is absent — the caches are still the
    data."""
    multi_lvl_cat = (
        "<c:multiLvlStrRef><c:f>Hoja1!$A$2:$B$4</c:f><c:multiLvlStrCache>"
        '<c:ptCount val="3"/>'
        '<c:lvl><c:pt idx="0"><c:v>Q1</c:v></c:pt>'
        '<c:pt idx="1"><c:v>Q2</c:v></c:pt>'
        '<c:pt idx="2"><c:v>Q3</c:v></c:pt></c:lvl>'
        '<c:lvl><c:pt idx="0"><c:v>2026</c:v></c:pt></c:lvl>'
        "</c:multiLvlStrCache></c:multiLvlStrRef>"
    )
    chart = (
        XML_DECL
        + f"<c:chartSpace {CHART_NS}>"
        "<c:chart>"
        "<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/>"
        '<a:p><a:r><a:rPr lang="es-ES"/><a:t>Ventas por trimestre</a:t></a:r></a:p>'
        "</c:rich></c:tx></c:title>"
        "<c:plotArea><c:layout/>"
        '<c:lineChart><c:grouping val="standard"/>'
        '<c:ser><c:idx val="0"/><c:order val="0"/>'
        f'<c:tx>{_chart_str_ref("Hoja1!$B$1", ["Norte"])}</c:tx>'
        f'<c:cat>{_chart_str_ref("Hoja1!$A$2:$A$4", ["Q1", "Q2", "Q3"])}</c:cat>'
        f'<c:val>{_chart_num_ref("Hoja1!$B$2:$B$4", ["1200", "980", "1440"], "#,##0")}</c:val>'
        "</c:ser>"
        '<c:ser><c:idx val="1"/><c:order val="1"/>'
        f'<c:tx>{_chart_str_ref("Hoja1!$C$1", ["Sur"])}</c:tx>'
        f'<c:val>{_chart_num_ref("Hoja1!$C$2:$C$4", ["800", None, "910"], "#,##0")}</c:val>'
        "</c:ser>"
        '<c:axId val="111111111"/><c:axId val="222222222"/>'
        "</c:lineChart>"
        '<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/>'
        '<c:ser><c:idx val="2"/><c:order val="2"/>'
        f'<c:tx>{_chart_str_ref("Hoja1!$D$1", ["Total"])}</c:tx>'
        f"<c:cat>{multi_lvl_cat}</c:cat>"
        f'<c:val>{_chart_num_ref("Hoja1!$D$2:$D$4", ["2000", "980", "2350"])}</c:val>'
        "</c:ser>"
        '<c:axId val="111111111"/><c:axId val="222222222"/>'
        "</c:barChart>"
        + _chart_axes()
        + "</c:plotArea><c:plotVisOnly val=\"1\"/></c:chart>"
        "</c:chartSpace>"
    )
    build_pptx(
        "charts-multi-series.pptx",
        [slide(shape(2, "Title 1", a_p("Ventas por trimestre"), ph=ph("title"))
               + _chart_takeaway(
                   "Dos zonas y un total, medidos por trimestre.",
                   "El sur no reportó el segundo trimestre: el hueco es un hueco.",
                   "El eje de categorías del total tiene dos niveles y se pierde uno.",
                   "Sin libro incrustado, la caché es todo lo que hay: y basta para leer.",
               )
               + _chart_frame())],
        slide_rels={1: [("rIdChart", "chart", "../charts/chart1.xml")]},
        extra_parts={"ppt/charts/chart1.xml": chart.encode()},
        extra_overrides=[("/ppt/charts/chart1.xml", f"{DML_TYPE}.chart+xml")],
        title="Gráfico combinado",
    )


def pptx_smartart_fallback() -> None:
    """SmartArt: four `dgm:` parts, a fifth in a Microsoft-only schema reached
    through a Microsoft-only relationship type, and an `mc:AlternateContent`
    that hands a plain shape to any consumer that cannot draw diagrams.

    The fallback is what a reader is *allowed* to show. The five parts are what
    it must not lose on the way back out, which is why this fixture is spike
    P3's hardest input."""
    def point(model_id: str, text: str = "", kind: str = "") -> str:
        type_attr = f' type="{kind}"' if kind else ""
        body = (
            "<dgm:t><a:bodyPr/><a:lstStyle/>"
            f'<a:p><a:r><a:rPr lang="es-ES"/><a:t>{text}</a:t></a:r></a:p></dgm:t>'
            if text else "<dgm:t><a:bodyPr/><a:lstStyle/><a:p/></dgm:t>"
        )
        return (
            f'<dgm:pt modelId="{model_id}"{type_attr}>'
            "<dgm:prSet/><dgm:spPr/>"
            f"{body}</dgm:pt>"
        )

    data = (
        XML_DECL
        + f'<dgm:dataModel {DGM_NS} '
        'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">'
        "<dgm:ptLst>"
        + point("{00000000-0000-0000-0000-000000000001}", kind="doc")
        + point("{00000000-0000-0000-0000-000000000002}", "Planificar")
        + point("{00000000-0000-0000-0000-000000000003}", "Ejecutar")
        + point("{00000000-0000-0000-0000-000000000004}", "Revisar")
        + "</dgm:ptLst><dgm:cxnLst/><dgm:bg/><dgm:whole/>"
        "</dgm:dataModel>"
    )
    layout = (
        XML_DECL
        + f'<dgm:layoutDef {DGM_NS} '
        'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
        'uniqueId="urn:microsoft.com/office/officeart/2005/8/layout/process1">'
        '<dgm:title val="Proceso básico"/><dgm:desc val=""/><dgm:catLst/>'
        '<dgm:sampData><dgm:dataModel><dgm:ptLst/><dgm:cxnLst/><dgm:bg/><dgm:whole/>'
        "</dgm:dataModel></dgm:sampData>"
        '<dgm:layoutNode name="linear"><dgm:alg type="lin"/><dgm:shape/>'
        "<dgm:presOf/><dgm:constrLst/></dgm:layoutNode>"
        "</dgm:layoutDef>"
    )
    quick_style = (
        XML_DECL
        + f'<dgm:styleDef {DGM_NS} '
        'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
        'uniqueId="urn:microsoft.com/office/officeart/2005/8/quickstyle/simple1">'
        '<dgm:title val=""/><dgm:desc val=""/><dgm:catLst/>'
        '<dgm:styleLbl name="node0"><dgm:scene3d><a:camera prst="orthographicFront"/>'
        '<a:lightRig rig="threePt" dir="t"/></dgm:scene3d>'
        "<dgm:sp3d/><dgm:txPr/><dgm:style/></dgm:styleLbl>"
        "</dgm:styleDef>"
    )
    colours = (
        XML_DECL
        + f'<dgm:colorsDef {DGM_NS} '
        'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
        'uniqueId="urn:microsoft.com/office/officeart/2005/8/colors/accent1_1">'
        '<dgm:title val=""/><dgm:desc val=""/><dgm:catLst/>'
        '<dgm:styleLbl name="node0">'
        '<dgm:fillClrLst meth="repeat"><a:schemeClr val="accent1"/></dgm:fillClrLst>'
        '<dgm:linClrLst meth="repeat"><a:schemeClr val="lt1"/></dgm:linClrLst>'
        "<dgm:effectClrLst/><dgm:txLinClrLst/>"
        '<dgm:txFillClrLst meth="repeat"><a:schemeClr val="lt1"/></dgm:txFillClrLst>'
        "<dgm:txEffectClrLst/></dgm:styleLbl>"
        "</dgm:colorsDef>"
    )
    drawing = (
        XML_DECL
        + '<dsp:drawing xmlns:dsp="http://schemas.microsoft.com/office/drawing/2008/diagram" '
        'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">'
        "<dsp:spTree>"
        '<dsp:nvGrpSpPr><dsp:cNvPr id="0" name=""/><dsp:cNvGrpSpPr/></dsp:nvGrpSpPr>'
        "<dsp:grpSpPr/>"
        '<dsp:sp modelId="{00000000-0000-0000-0000-000000000002}">'
        '<dsp:nvSpPr><dsp:cNvPr id="0" name=""/><dsp:cNvSpPr/></dsp:nvSpPr>'
        f'<dsp:spPr>{xfrm(838200, 2200000, 2000000, 900000)}'
        '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></dsp:spPr>'
        f"<dsp:txBody><a:bodyPr/><a:lstStyle/>{a_p('Planificar')}</dsp:txBody>"
        "</dsp:sp>"
        "</dsp:spTree></dsp:drawing>"
    )

    diagram_frame = (
        "<p:graphicFrame>"
        '<p:nvGraphicFramePr><p:cNvPr id="4" name="Diagrama 1"/>'
        '<p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr>'
        "<p:nvPr/></p:nvGraphicFramePr>"
        '<p:xfrm><a:off x="838200" y="1825625"/><a:ext cx="7467600" cy="3000000"/></p:xfrm>'
        '<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/diagram">'
        f'<dgm:relIds {DGM_NS} r:dm="rIdDgmData" r:lo="rIdDgmLayout" '
        'r:qs="rIdDgmQuickStyle" r:cs="rIdDgmColors"/>'
        "</a:graphicData></a:graphic>"
        "</p:graphicFrame>"
    )
    alternate = (
        '<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">'
        '<mc:Choice xmlns:dgm14="http://schemas.microsoft.com/office/drawing/2010/diagram" '
        'Requires="dgm14">'
        f"{diagram_frame}"
        "</mc:Choice>"
        "<mc:Fallback>"
        + shape(5, "Diagrama 1 (respaldo)", a_p("Planificar · Ejecutar · Revisar"),
                frame=xfrm(838200, 1825625, 7467600, 3000000))
        + "</mc:Fallback>"
        "</mc:AlternateContent>"
    )

    build_pptx(
        "smartart-fallback.pptx",
        [slide(shape(2, "Title 1", a_p("Proceso"), ph=ph("title")) + alternate)],
        slide_rels={1: [
            ("rIdDgmData", "diagramData", "../diagrams/data1.xml"),
            ("rIdDgmLayout", "diagramLayout", "../diagrams/layout1.xml"),
            ("rIdDgmQuickStyle", "diagramQuickStyle", "../diagrams/quickStyle1.xml"),
            ("rIdDgmColors", "diagramColors", "../diagrams/colors1.xml"),
            ("rIdDgmDrawing", DGM_DRAWING_REL, "../diagrams/drawing1.xml"),
        ]},
        extra_parts={
            "ppt/diagrams/data1.xml": data.encode(),
            "ppt/diagrams/layout1.xml": layout.encode(),
            "ppt/diagrams/quickStyle1.xml": quick_style.encode(),
            "ppt/diagrams/colors1.xml": colours.encode(),
            "ppt/diagrams/drawing1.xml": drawing.encode(),
        },
        extra_overrides=[
            ("/ppt/diagrams/data1.xml", f"{DML_TYPE}.diagramData+xml"),
            ("/ppt/diagrams/layout1.xml", f"{DML_TYPE}.diagramLayout+xml"),
            ("/ppt/diagrams/quickStyle1.xml", f"{DML_TYPE}.diagramStyle+xml"),
            ("/ppt/diagrams/colors1.xml", f"{DML_TYPE}.diagramColors+xml"),
            ("/ppt/diagrams/drawing1.xml",
             "application/vnd.ms-office.drawingml.diagramDrawing+xml"),
        ],
        title="SmartArt con respaldo",
    )


# The `a:graphicData@uri` an embedded object arrives under. Its last segment is
# what the reader names the stub from, exactly as `chart` and `diagram` are.
OLE_URI = "http://schemas.openxmlformats.org/presentationml/2006/ole"


def _ole_preview(ident: int, name: str, rel: str, frame: str) -> str:
    """The picture an OLE object is *seen* as.

    No consumer runs the embedded application to draw a slide: it draws the
    snapshot the producer took when the object was last edited. So the picture
    is not decoration — it is the only thing on the slide a renderer can show,
    and a round trip that lost it would lose the object visually while keeping
    its bytes."""
    return (
        "<p:pic>"
        f'<p:nvPicPr><p:cNvPr id="{ident}" name="{name}"/>'
        '<p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>'
        f'<p:blipFill><a:blip r:embed="{rel}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>'
        f'<p:spPr>{frame}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>'
        "</p:pic>"
    )


def pptx_ole_embedded() -> None:
    """`p:oleObj`: the fixture `kb/72-phase-15-close.md` item 13 names as the
    one the corpus never had, and the code path `kb/43-phase-13-raw.md` says was
    written without a file to prove it on.

    An embedded object reaches a slide two ways, and this deck carries both
    because the reader has two branches for them:

    * **slide 1**, the ECMA-376 spelling: a `p:graphicFrame` whose
      `a:graphicData@uri` ends in `ole`, holding a `p:oleObj` with `p:embed`
      and the preview picture. The stub kind comes from the uri.
    * **slide 2**, the spelling PowerPoint actually writes: the same frame
      inside an `mc:AlternateContent`, with a plain picture in the fallback for
      a consumer that cannot activate the object. The uri is inside the
      `mc:Choice`, so the stub kind comes from finding the `p:oleObj` element
      itself — and neither branch is read, because choosing one is the
      consumer's job.

    The two payloads differ on purpose too: an embedded worksheet is a whole
    OPC package reached through a `package` relationship, an embedded equation
    is an OLE2 compound file reached through an `oleObject` one. Neither is
    ever opened by this project; both must come back byte for byte."""
    worksheet_frame = (
        "<p:graphicFrame>"
        '<p:nvGraphicFramePr><p:cNvPr id="4" name="Hoja de cálculo 1"/>'
        '<p:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/>'
        "</p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr>"
        '<p:xfrm><a:off x="4400000" y="2000000"/><a:ext cx="3600000" cy="2700000"/></p:xfrm>'
        f'<a:graphic><a:graphicData uri="{OLE_URI}">'
        '<p:oleObj spid="_x0000_s1026" name="Hoja de cálculo" r:id="rIdOleWorkbook" '
        'imgW="3600000" imgH="2700000" progId="Excel.Sheet.12">'
        "<p:embed/>"
        + _ole_preview(5, "Vista previa de la hoja", "rIdOleImage",
                       xfrm(4400000, 2000000, 3600000, 2700000))
        + "</p:oleObj>"
        "</a:graphicData></a:graphic>"
        "</p:graphicFrame>"
    )
    equation_frame = (
        "<p:graphicFrame>"
        '<p:nvGraphicFramePr><p:cNvPr id="4" name="Ecuación 1"/>'
        "<p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>"
        '<p:xfrm><a:off x="4400000" y="2400000"/><a:ext cx="2400000" cy="1200000"/></p:xfrm>'
        f'<a:graphic><a:graphicData uri="{OLE_URI}">'
        '<p:oleObj spid="_x0000_s1027" name="Ecuación" r:id="rIdOleEquation" '
        'imgW="2400000" imgH="1200000" progId="Equation.3">'
        "<p:embed/>"
        + _ole_preview(5, "Vista previa de la ecuación", "rIdOleImage",
                       xfrm(4400000, 2400000, 2400000, 1200000))
        + "</p:oleObj>"
        "</a:graphicData></a:graphic>"
        "</p:graphicFrame>"
    )
    alternate = (
        '<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">'
        '<mc:Choice xmlns:v="urn:schemas-microsoft-com:vml" Requires="v">'
        f"{equation_frame}"
        "</mc:Choice>"
        "<mc:Fallback>"
        + _ole_preview(6, "Ecuación (respaldo)", "rIdOleImage",
                       xfrm(4400000, 2400000, 2400000, 1200000))
        + "</mc:Fallback>"
        "</mc:AlternateContent>"
    )
    build_pptx(
        "ole-embedded.pptx",
        [
            slide(
                shape(2, "Title 1", a_p("Presupuesto incrustado"), ph=ph("title"))
                + shape(3, "Content Placeholder 2", "".join(a_p(line) for line in [
                    "La hoja de cálculo viaja dentro del propio archivo.",
                    "Quien la abre edita el original, no una copia enlazada.",
                    "La diapositiva sólo muestra la instantánea que dejó el editor.",
                    "El paquete incrustado tiene que volver byte a byte.",
                ]), ph=ph("body", 1))
                + worksheet_frame
            ),
            slide(
                shape(2, "Title 1", a_p("Fórmula incrustada"), ph=ph("title"))
                + shape(3, "Content Placeholder 2", "".join(a_p(line) for line in [
                    "La ecuación llega envuelta en una alternativa de compatibilidad.",
                    "La rama elegida es cosa de quien la dibuja, no de quien la lee.",
                    "El respaldo es una imagen para quien no sabe activarla.",
                    "Aquí se conservan las dos ramas y no se abre ninguna.",
                ]), ph=ph("body", 1))
                + alternate
            ),
        ],
        media={"image1.png": BLUE_PNG},
        slide_rels={
            1: [
                ("rIdOleWorkbook", "package", "../embeddings/workbook1.xlsx"),
                ("rIdOleImage", "image", "../media/image1.png"),
            ],
            2: [
                ("rIdOleEquation", "oleObject", "../embeddings/oleObject1.bin"),
                ("rIdOleImage", "image", "../media/image1.png"),
            ],
        },
        extra_parts={
            "ppt/embeddings/workbook1.xlsx": embedded_workbook(),
            # An OLE2 compound-file header and filler. Not a real equation: the
            # reader must never look inside an embedded object, so a payload
            # that could actually be activated would be a hazard with no test
            # value — the same reasoning as the macro fixture's `vbaProject`.
            "ppt/embeddings/oleObject1.bin": (
                bytes.fromhex("d0cf11e0a1b11ae1") + b"\x00" * 1016
            ),
        },
        extra_defaults={"bin": "application/vnd.openxmlformats-officedocument.oleObject"},
        extra_overrides=[
            ("/ppt/embeddings/workbook1.xlsx",
             "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        ],
        title="Objetos incrustados",
    )


# PowerPoint's modern comment part: a Microsoft extension, not ECMA-376, and
# the schema `p:cmLst` cannot express. It is here so the reader's refusal has a
# file to refuse; nothing in this project ever parses it.
MODERN_COMMENT = (
    '<p188:cmLst xmlns:p188="http://schemas.microsoft.com/office/powerpoint/2018/8/main">'
    '<p188:cm id="{2C4C5A31-0000-0000-0000-000000000001}" authorId="{AAAAAAAA-0000-0000-0000-000000000001}"'
    ' created="2026-02-03T10:15:00.000">'
    "<p188:txBody><a:bodyPr xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"/>"
    "<a:p xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">"
    "<a:r><a:t>Hilo moderno con respuestas.</a:t></a:r></a:p></p188:txBody>"
    "</p188:cm>"
    "</p188:cmLst>"
)


def pptx_comments_slide() -> None:
    """Comments on a slide: `ppt/comments/*` plus `ppt/commentAuthors.xml`
    (Phase 16-E, spec §11.3).

    Three facts the reader has to get right, one per shape of the file:

    * **slide 1** carries two comments by two different authors, each pinned to
      its own point. The author is a number in the comment and a name in a
      separate deck-wide part, so a reader that never opens the author list
      would produce two anonymous comments and no error.
    * **slide 2** carries a comment whose `authorId` the author list does not
      define. It is kept, unattributed, and warned: a comment attributed to
      nobody in particular would be worse than one attributed to no one.
    * **slide 2** also carries PowerPoint's modern comment part beside the
      classic one, which is what PowerPoint itself writes. It is not read, the
      refusal names the schema, and the part comes back byte for byte.

    Both slides have real prose in their bodies: a fixture whose text is one
    word makes the `standard` residue of `{.slide}` look enormous, and the
    corpus budget is measured over exactly this."""
    comment = (
        '<p:cm authorId="{author}" dt="{dt}" idx="{idx}">'
        '<p:pos x="{x}" y="{y}"/>'
        "<p:text>{text}</p:text>"
        "</p:cm>"
    )
    slide1_comments = (
        comment.format(
            author=1, dt="2026-02-03T09:12:00Z", idx=1, x=1134, y=678,
            text="La cifra de ingresos viene del cierre provisional.",
        )
        + comment.format(
            author=2, dt="2026-02-03T10:04:00Z", idx=2, x=2899, y=1517,
            text="De acuerdo, pero conviene fechar el dato en el pie.",
        )
    )
    slide2_comments = comment.format(
        author=7, dt="2026-02-03T11:30:00Z", idx=1, x=1260, y=942,
        text="Falta el desglose por region.",
    )
    build_pptx(
        "comments-slide.pptx",
        [
            title_body_slide("Cierre del trimestre", [
                ("Los ingresos crecen un doce por ciento respecto al trimestre anterior.", 0),
                ("La rotacion de clientes se mantiene plana desde noviembre.", 0),
                ("El margen bruto mejora dos puntos gracias al cambio de proveedor.", 0),
                ("La tesoreria cubre catorce meses de gasto corriente.", 0),
            ]),
            title_body_slide("Pendientes de revision", [
                ("El desglose por region llega la semana que viene.", 0),
                ("Falta confirmar el dato de rotacion con el equipo comercial.", 0),
                ("La auditoria revisa el criterio de reconocimiento de ingresos.", 0),
                ("El consejo pide una comparativa con el mismo trimestre del ano pasado.", 0),
            ]),
        ],
        comment_authors=[(1, "Ada Lovelace", "AL"), (2, "Grace Hopper", "GH")],
        comments={1: slide1_comments, 2: slide2_comments},
        slide_rels={
            2: [("rIdModernComments", "comments", "../comments/modernComment_1.xml")],
        },
        extra_parts={
            "ppt/comments/modernComment_1.xml": (XML_DECL + MODERN_COMMENT).encode(),
        },
        extra_overrides=[
            ("/ppt/comments/modernComment_1.xml", "application/vnd.ms-powerpoint.comments+xml"),
        ],
        title="Comentarios de revision",
    )


def pptx_macro_enabled() -> None:
    """A `.pptm`: the same deck as `basic-slides`, plus a macro project and the
    macro-enabled main content type. Read as its macro-free equivalent — the
    slides come back whole and a warning says the project was ignored.

    It is a separate fixture rather than a flag on an existing one because the
    extension differs, and every reader that routes on the extension instead of
    the content is exactly what this file is here to catch."""
    build_pptx(
        "macro-enabled.pptm",
        [
            title_body_slide("Informe con macros", [
                ("La macro no se ejecuta", 0),
                ("El texto se lee igual", 0),
            ]),
        ],
        title="Presentación con macros",
        macro_enabled=True,
    )


def pptx_forty_slides() -> None:
    """Forty slides, each with a title, a body and speaker notes.

    Not a trait fixture: the plan's Phase 13 acceptance asks that a 40-slide deck
    reads in under a second, and no real-world deck can live in the repository.
    This one has the right shape — forty slides resolving against the same
    cascade, forty notes parts reached through their own relationships — and not
    the weight of a real deck, which would carry images and embedded objects.
    What it catches is a reader that goes quadratic in the slide count."""
    build_pptx(
        "forty-slides.pptx",
        [
            title_body_slide(f"Sección {index}", [
                (f"Punto {index}.1", 0),
                (f"Punto {index}.2", 1),
            ])
            for index in range(1, 41)
        ],
        title="Deck largo",
        notes={
            index: notes_slide(index, f"Notas de la sección {index}")
            for index in range(1, 41)
        },
    )


# --------------------------------------------------------------------------
# Legacy .ppt fixtures (plan v2 Phase 19-A)
# --------------------------------------------------------------------------
#
# A `.ppt` is an OLE2 compound file, not a ZIP, and nothing in this repository
# writes one. The corpus rule — every byte is produced by this script, and
# `--check` proves the checked-in tree is what it produces — therefore has to be
# met a different way here, and the way is stated rather than implied:
#
# * The two *ordinary* decks are **recorded**, not synthesised. Their bytes were
#   produced once by headless LibreOffice (`--convert-to ppt`) from a `.pptx`
#   this script generates, and they live in this file zlib-compressed and
#   base64-encoded, exactly as the legacy `.doc` fixtures live here. `--check`
#   **regenerates** them: decompressing a constant is deterministic on every
#   Python this project supports, so the digest of the written file is fixed by
#   this file and not by the machine. LibreOffice is a dependency of *refreshing*
#   the constant — a step a person takes deliberately, with the recipe below —
#   and not a dependency of running the generator or the gates. That is the cost
#   and it is paid knowingly: the bytes cannot be reviewed in a diff the way the
#   XML of a `.pptx` can, so their provenance is written down instead.
#
#   The recipe, reproducible by anyone with the same LibreOffice:
#
#       python3 corpus/generate.py
#       soffice --headless --convert-to ppt --outdir /tmp/ppt \
#           corpus/pptx/basic-slides.pptx corpus/ppt/mixed-media.source.pptx
#       python3 -c 'import base64,zlib,sys,textwrap; \
#           print(textwrap.fill(base64.b64encode(zlib.compress(open(sys.argv[1],"rb").read(),9)).decode(),76))' \
#           /tmp/ppt/basic-slides.ppt
#
#   Measured here before it was relied on: LibreOffice 24.2.7.2 writes the same
#   bytes twice in a row, ninety-five seconds apart, and again on a freshly
#   created user profile. It embeds no timestamp and no UUID. That is what makes
#   *recording* the output honest; it is not a promise about another version, and
#   a refresh under a different LibreOffice will produce a different constant,
#   which is a reviewed change to this file and not a machine-dependent digest.
#
# * The two fixtures that exist to be **refused** are *derived* from the first
#   deck by the compound-file surgery below. They are computed, not recorded:
#   this script owns every byte in which they differ from their base, and the
#   difference is one paragraph of readable code each.
#
# None of this is a `.ppt` writer. Phase 19 is read-only and stays read-only; the
# code here writes fixtures, lives in the corpus generator, and reaches no crate
# and no shipped binary.


def ppt_mixed_media_source() -> None:
    """The `.pptx` that `mixed-media.ppt` was converted from.

    It is not part of the deck corpus and no gate measures it: it lands in
    `corpus/ppt/` rather than `corpus/pptx/` precisely so that the round-trip,
    schema and render gates — which read `corpus/pptx` — do not pick it up. It
    exists so the recipe above is runnable by someone who wants to refresh the
    recorded bytes, and so the content of the legacy deck is reviewable as XML
    even though the legacy deck itself is not.

    Three slides, because one slide cannot show a slide boundary; titles and
    body text, because that is what the degraded native route has to recover;
    and a picture on the last slide, because a `.ppt` keeps its bitmaps in a
    `Pictures` stream of its own that the text path never touches.
    """
    picture = (
        "<p:pic>"
        '<p:nvPicPr><p:cNvPr id="4" name="Imagen 1" descr="Rectángulo azul"/>'
        '<p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>'
        '<p:blipFill><a:blip r:embed="rIdImg"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>'
        f'<p:spPr>{xfrm(1524000, 2286000, px(120), px(90))}'
        '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>'
        "</p:pic>"
    )
    build_pptx(
        "mixed-media.source.pptx",
        [
            title_body_slide("Legado en binario", [
                ("Primera viñeta", 0),
                ("Segunda viñeta", 0),
            ]),
            title_body_slide("Segunda diapositiva", [
                ("Nivel cero", 0),
                ("Nivel uno", 1),
            ]),
            slide(
                shape(2, "Title 1", a_p("Con imagen"), ph=ph("title")) + picture
            ),
        ],
        media={"image1.png": BLUE_PNG},
        slide_rels={3: [("rIdImg", "image", "../media/image1.png")]},
        title="Mixta para .ppt",
        subdir="ppt",
    )


# --------------------------------------------------------------------------
# The recorded bytes. See the recipe above for how to refresh them.
# --------------------------------------------------------------------------

BASIC_SLIDES_PPT_Z = (
    "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"
    "eqM+qC+6Bd2K+qH+aAC6Dd2O7kAD0Z1oELoLDUZ3oyFoKBqG7kH3ovvQcHQ/egA9iB5CD6MRaCR6"
    "BI1Co9GjaAwai8ah8WgCegxNRI+jJ9CT6Ck0CU1GU9BU9DSahp5Bz6Lp6Dn0PHoBvYhmoJfQy+gV"
    "NBPNQrPRq+g19Dqag+aiN9A8NB8tQG+ihWgRWoyWoKVoGXoLvY3+h95B76L30PtoOVqBVqIP0Cr0"
    "IVqNPkJr0Fq0Dq1HH6NP0Ab0KdqINqHN6DP0OdqCvkBfoq3oK7QNfY2+QdvRt+g7tAPtRLvQbpSK"
    "9qDv0Q/oR/QT2ov2of3oAPoZ/YJ+Rb+h31EQBRI5/1E8SkCJKAvKirKhJHQCyo5yoJwoF8qNklEK"
    "yoPyonwoPyqACqJCqDAqgoqiE1ExdBIqjkqgkqgUOhmdgkqjMuhUVBaVQ+VRBVQRVUKV0WnodFQF"
    "nYGqomroTFQdnYXORuegc9F56Hx0AboQXYRqoJroYlQL1UZ1UF10CboU1UP1UQN0GbocXYGuRA1R"
    "I3QVaoyaoKboatQMNUfXoBboWtQSXYdaoevRDehG1Bq1QW1RO9QedUAd0U2oE+qMuqCuqBvqjnqg"
    "nuhm1Av1Rn1QX3QLuhX1Q/3RAHQbuh3dgQaiO9EgdBcajO5GQ9BQNAzdg+5F96Hh6H70AHoQPYQe"
    "RiPQSPQIGoVGo0fRGDQWjUPj0QT0GJqIHkdPoCfRU+j3YDAY0pTItBDq/xdCCCH/F0IIIf8XQggh"
    "/xdCCCH/F0IIIf8XQggh/xdCCCH/F0IIIf8XQggh/xdCCCH/F0IIIf8XQggh/xdCCCH/F0IIIf8X"
    "Qggh/xdCCPm/MkEIIeT/Qggh5P9CCCHk/0IIIeT/Qggh5P9CCCHk/0IIIeT/Qggh5P9CCCHk/0II"
    "IeT/Qggh5P9CCCHk/0IIIeT/Qggh5P9CCCHk/0IIIeT/Qggh5P9CCCHk/0IIIeT/Qggh5P9CCCHk"
    "/0IIIf8XQggh/xdCCCH/F0IIIf8XQggh/xdCCCH/F0IIIf8XQggh/xdCCCH/F0IIIf8XQggh/xdC"
    "CCH/F0IIIf8XQggh/xdCCCH/F0IIIf8XQggh/xdCCCH/F0IIIf8XQggh/xdCCCH/F0II+b8yQQgh"
    "5P9CCCHk/0IIIeT/Qggh5P9CCCHk/0IIIeT/Qggh5P9CCCHk/0IIIeT/Qggh5P9CCCHk/0IIIeT/"
    "Qggh5P9CCCHk/0IIIeT/Qggh5P9CCCHk/0IIIeT/Qggh5P9CCCHk/0IIIeT/Qggh/xdCCCH/F0II"
    "If8XQggh/xdCCCH/F0IIIf8XQggh/xdCCCH/F0IIIf8XQggh/xdCCCH/F0IIIf8XQggh/xdCCCH/"
    "F0IIIf8XQggh/xdCCCH/F0IIIf8XQggh/xdCCCH/F0IIIf8XQgj5vzJBCCHk/0IIIeT/Qggh5P9C"
    "CCHk/0IIIeT/Qggh5P9CCCHk/0IIIeT/Qggh5P9CCCHk/0IIIeT/Qggh5P9CCCHk/0IIIeT/Qggh"
    "5P9CCCHk/0IIIeT/Qggh5P9CCCHk/0IIIeT/Qggh5P9CCCH/F0IIIf8XQggh/xdCCCH/F0IIIf8X"
    "Qggh/xdCCCH/F0IIIf8XQggh/xdCCCH/F0IIIf8XQggh/xdCCCH/F0IIIf8XQggh/xdCCCH/F0II"
    "If8XQggh/xdCCCH/F0IIIf8XQggh/xdCCPm/MkEIIeT/Qggh5P9CCCHk/0IIIeT/Qggh5P9CCCHk"
    "/0IIIeT/Qggh5P9CCCHk/0IIIeT/Qggh5P9CCCHk/0IIIeT/Qggh5P9CCCHk/0IIIeT/Qggh5P9C"
    "CCHk/0IIIeT/Qggh5P9CCCHk/0IIIf8XQggh/xdCCCH/F0IIIf8XQggh/xdCCCH/F0IIIf8XQggh"
    "/xdCCCH/F0IIIf8XQggh/xdCCCH/F0IIIf8XQggh/xdCCCH/F0IIIf8XQggh/xdCCCH/F0IIIf8X"
    "Qggh/xdCCCH/F0II+b8yQQgh5P9CCCHk/0IIIeT/Qggh5P9CCCHk/0IIIeT/Qggh5P9CCCHk/0II"
    "IeT/Qggh5P9/NL///rt9/vbbb7+kwbTNdEt//fVXW8SEm+lbxFreRS5Ci823FdZy4TOOJ710ZiYG"
    "73xvyl1I20fvIlvduxVvYn4Nk14OuEWZ2SMhhPz/Lzd/3Cmmp3mXeosDM14XLL21vE4YbbPe8Obq"
    "vvm+AsgXfyZjiJkSb0jf3rmZ3jC+jaaXFd45GeSMigAh5P9/n5q/OdW2bdteeeWVJ598ct68eXv2"
    "7PG55ccffzx58mSWrl692pmqxbB27drHHnts0qRJW7Zs8frtF198MWXKlFdffXXv3r1mjHympqa+"
    "9tprEyZMmDVr1o4dO7zJePfdd8eOHfvcc8999913Fo/Pli2GXbt2sS4xEDPTXid/++23x4wZ88IL"
    "L7j5tmj9+vWkfOHChRYDId966y1Czpw586effmIOXy0xfF2wYMHu3btJ5zPPPPPUU0998skn3p36"
    "4IMPlixZ4uLh88CBA0uXLiUHpk+f/tlnn3m3G50z8n8h5P9/E/83L/r5559vvfXWhIQEdjMuLo7P"
    "7NmzT5w40YL9+OOP11xzDTOzZMmSNWtWJho0aGBuCc2bN2dO0aJFCxYsyMSjjz5q8zdu3Jg/f/5A"
    "mDp16thMHDV37tzMyZYtm22oV69ezP/+++/POussvhYvXpwAbP311193FW9n73xi4zly5HAxxMfH"
    "DxgwgPmk5/TTT2d+iRIlcubMmZycjNvbRhcvXsyu2ebat2/PnE6dOlma2VyVKlUodPhKqWSOzXTL"
    "li3LlSuXmJhoedK/f39X+tSoUSMpKYk8schXrlxZtmxZwpAzhLfMsfKrWbNm0TnjbYwIIeT/f6H5"
    "mxfdcMMN7ODFF1+MZ3766afUoq+44grmUCFnqZn/I488Qt143759VIn5Wr9+fYvn9ttvx9Vt2nwV"
    "52f6gQceYBq7puo7d+5c5lCvZs7ZZ5+NxxLP9u3bBw4ciJ1Sf+Zr165dFy1aZNVpZhYpUoQJbw8M"
    "n1T4ieGiiy6iTk6b4uuvv6bYonAhDCVI586dSTnBWFS9evVSpUpRrvEVz2ctdu3DMF999RVfW7Ro"
    "YWkmbZQCzJk/fz5f161bx3S+fPkogEgAxcp1113HnMcff9zC16xZkwLI/J9dKFSoUJ48eYiEjdJk"
    "mDp1auXKla2JdNttt02bNs3WIm1EsnnzZleQCSHk/3+h/5upzpw5k72rV6+et1No//79VJudH7Zq"
    "1SqYdumTiZtvvpmZZrY255dffuHzxRdfZP7s2bOZps7M9KpVq9zSSy+9lDnWQ+K9HurtFbeQgwYN"
    "IiQlkQtpnnnhhRdSx962bdthY+jTpw8xfP7550w3btyYabzadpBo+dqvXz+mKXf4HDNmDHPefPNN"
    "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"
    "SAgh/z8W/3fG8tJLL/Xr18+enOPtEslgXW9H0O7du994440pU6bwmZqaetRm5ex006ZNh22G/NGZ"
    "Y/08uPph+97N/zds2OCr//u6y7zP/5GZCyH//2v9P3hU4z99q0f3txx1f0XMxzL8tZmTmZTYUgqs"
    "hQsX2g1lvgsQ9vXDDz986623vM92FkLI//9ylzui+79iliDe978ctXt7I/xriwA9e18I8e/2f3Hc"
    "i9HoGyi8F6BVmggh/xdCCCH/F0IIIf//V+J9oL3vDenqDBFCyP//9f7vG+J+pMONhBBC/v9P9H+z"
    "9/fee69FixYVKlQ49dRT69ata09vzsztBkIIIf//W1Xpo5/D4OvScf08TL/88sv2NpPbb7996NCh"
    "9vj9jh07BtOGWXrHxngH3luvkb1ZzPsoiej5vvGoNqLVOyDHO1Q15lAc7365ML47GqIH+UQn1QJ4"
    "H+ngfX6FBfs1DRfeJdttV+OFhJD//6Mxc6tVq1bp0qVtjpUIr7zyint5uveJE+k9sSfoeWxa9Pz0"
    "Rux75/i6m1xs0cVHemEyKAq9L0E4IutOb7u6U0wI+f/ftv7/008/vfjii++9957N3Lx587PPPut9"
    "W8q2bdueeeYZexxo37598+TJs2PHDuf2L730knsX2Lx58+bMmeMz52D4xWFz5869//77R48evWLF"
    "Cm+Ajz766KGHHho+fLglwPvcni+//PKpp5669957n3/+eXsIp3NjFo0fP54GCNG6Nxf4PlNTU59+"
    "+ukhQ4ZQQhEtMcyePdue/8nesY+bNm3ylkHu84MPPnjggQdGjhxpb5B8++23mWOZs2zZslmzZtnr"
    "jGfOnMm+k7YXwkybNs3uGt6zZw+Rs90ZM2ZQRPKVVf7a52AIIeT/Mav09l7dxo0b20x74a+9DNee"
    "omx9Po8//rgFaN68ea5cuSpXrlyqVKmCBQuyqEiRIual5cuXL1CggKvxWvyrVq0qU6YMwXKFYeL0"
    "00+35+136dKFryVKlChbtqx7666thQPHx8cnJibmy5fPXjqA2VoCJk6cyJz8+fOTBiZq1Khhj5L2"
    "evgPP/xwxhlnsJQwxYsXv/TSS3v16kVs9tov9o5FDz/8cNDzkB9L9qRJk1jEXrB6oUKF7r777mLF"
    "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"
    "Sy6xmN9//33rV7QCyLJ648aNdj1a/i/k/39a/d8CDB061NZ68sknvd7utb7PP/+c7RKmRo0af3n/"
    "j1mf80P22ttZcXQ57/V/XyL/zv7fu3dvmzlr1qzMZIKvCeD2fdmyZfXr13fX+hcsWOBiOxb/d4kf"
    "OXKkxXzZZZd5i1d32csO+yuvvFL+L+T/f2b93w0sdEYa3YG8e/duG79RsWJF7/y/0P+po9paGOO4"
    "ceNGjBjxSBq4jX0y85577rH98mbXMfp/BuM//2T/t2EzwI84fvz4DDLBRvC6fj83pP/RRx8lqTYO"
    "B7Jly0abwlvEO/+PObwnk/0/pJaag22iUqVKw4cPnzt37vLly9mXdevWLVmyxAaXqv4v5P9H7f+v"
    "v/76Ufi/ja6HefPmRdch3YWJokWLEqZChQremtsR+f9hx/87/3cBMvB/2yPvHRPR2NKWLVtmUDf+"
    "k/2fpTH9/5133vH5v/cO2fT8367LZCYTmjdvHvRcNeZIu+2222ywmZl/7ty527dvb7eWeHvMjtT/"
    "g1E3r9lXMq1z586+u0scNr5U/i/k/xn7v3cYvM9Djq7/x43Ne+6553z1f2d927Ztsxqad1i78/+C"
    "BQtSkwymc++n8//ixYu7G3hj+n961pdx/w951aBBg0vCXBqGCebgwOQVAVq3bn0c/T9Pnjzbt29P"
    "b2cz8H+7BhrdfeTz/1tuuSXz/u/6f4g840xo1apVMO1Gks2bN7ObzvlZl5aCje2JftRD5v2/TJky"
    "rmiLvgXMdvPLL7+cPHlynz59mjVrRgpr1apVp04dmga0O+T/Qv5/WP+P7qhxV9lo4x+F/0+cONF7"
    "IdXrfi5md3cqp63XnaxOC9iXr94YjBpakyNHji1btgQ9wz8s/LJlyyySNm3auMgNbwvF5/9jx461"
    "tSZNmuQzB19ueK9rH4v/2/2/sHjx4uCh90Fk3v+9zTdvJrgc5pcKHnoV3nsTnM//3UVwd+0mvSZh"
    "165dnf+7q7HnnHPO/PnzvYOBoy8TZ8b/bYAQHu5tXUYXAS550c++sHGn8n8h/z+s/9tIbxzAnUd2"
    "XrvRIHhjZvp/bNHKlSutHnjxxRcHo67/ms/cfffdFvPo0aNj+j8WHQyP6/AOm/Q9/wFmz57tPZF9"
    "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"
)

MIXED_MEDIA_PPT_Z = (
    "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"
    "EELo/4QQQuj/hBBC6P8+cvfuXfn7559/3jb5448//jTBhPy8a6JWJoQQ+v+/xvw92bvMuXPnDrID"
    "fQ4hhND//x1ZAOx9z549U6ZMiYuLa9SoUXWTJk2avPXWW+vXr0ctQNah/xNC6P//JvMHNWrUsHkm"
    "X758O3bsYBZACKH//8v8H8X7Z599FgkSGRlZunTpZs2axcbGduvWrX79+qlSpZIsICQkRLKAO3fu"
    "0P8JIfT/f4f/37p1C4a/ZMmSq1evOhK6A2TR5cuX69SpI1lAyZIlLbUGQgih//+j/V+5vRTv/zCR"
    "kT9SOyhUqJBkAd98840joRWIEELo/3+RM6ueWTUg0y2yVC+W60M6lZOrgZ2W0T76XmScj6wMMP37"
    "779j6cyZM8X/x4wZg5+SL6jNJcuQ8NUuJCYSmmVw0QM3lEX6T9cMS99Woqoaph6qemLZozp2iY8e"
    "puWgvERP5aEPjI8+vAp/H5gsnnaktvU03/V4LVeOflWoCOjXjNsT6uPxWq5kX06rZdeschKW/313"
    "M8kavNz7jvuHfbpuou5Z/P3uu+/8/PyQYl26dBH/14Nym5u4ZjSWOFjsQt/QMtOt6blu6xqmj8nl"
    "GqAl/3V10b+uFub20P6G8bePc16eSDlHHaOlnOD27BDyry//y8SuXbvWr1+/yTPr1q37/vvvLfYu"
    "00ePHp0/f/7o0aOHDh2Kovu8efP27dsngzld7zIpfcHqp0yZMmzYsHfffXfFihXx8fFYevjw4cDA"
    "QKRYq1at9PK/3LDnzp375JNPhg8fPmLEiBkzZmAXKsAjR45s27btypUrluxGNsT85cuXI3rY3bRp"
    "0/bu3StLEcP9+/fv3r372rVrrk4obnzx4sWlS5cikth24sSJSIcbN248lF1bbOfq1atISQSFAHEg"
    "U6dOxc9Lly5Zisdg586da9eu/fbbbz2Vrk+ePIltN2zYcPnyZV/K/2fPnt2+ffuFCxdk5vXr11ev"
    "Xj1u3DicNcRnx44dkpiuZXWk/DoThKCnMILCNYP5p0+fdhtDBIXzgqPYs2ePHqbs4vz587hsRpjI"
    "CZVrA4t++umnb775xnJCjx07hqA2btz466+/ein/45xitS1btty6dctyIOqsnTp1asGCBe+88w7O"
    "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"
)


def _ppt_bytes(blob: str) -> bytes:
    return zlib.decompress(base64.b64decode(blob))


def _write_ppt(name: str, data: bytes) -> None:
    out = ROOT / "ppt" / name
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(data)


# --------------------------------------------------------------------------
# Compound-file surgery, for the two fixtures that exist to be refused
# --------------------------------------------------------------------------
#
# Enough of [MS-CFB] to move a boundary inside an existing file: the FAT, the
# directory and the mini stream. It creates nothing — every file it touches is
# one LibreOffice already wrote — and it asserts the shape it assumes rather
# than trusting it, so a refreshed constant with a different layout fails loudly
# here instead of producing a fixture that means something else.

_CFB_SIGNATURE = b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1"
_FREESECT = 0xFFFFFFFF
_ENDOFCHAIN = 0xFFFFFFFE
_MAXREGSECT = 0xFFFFFFFA


def _cfb_open(data: bytes) -> tuple[bytearray, int, list[int]]:
    """``(bytes, sector size, DIFAT)`` for a compound file this script edits."""
    b = bytearray(data)
    assert bytes(b[:8]) == _CFB_SIGNATURE, "not a compound file"
    sector = 1 << struct.unpack_from("<H", b, 30)[0]
    assert sector == 512, f"unexpected sector size {sector}"
    fat_count = struct.unpack_from("<I", b, 44)[0]
    assert fat_count <= 109, "a DIFAT sector chain is not handled here"
    assert struct.unpack_from("<I", b, 68)[0] >= _ENDOFCHAIN, "unexpected DIFAT chain"
    difat = [struct.unpack_from("<I", b, 76 + 4 * i)[0] for i in range(fat_count)]
    return b, sector, difat


def _fat_slot(sector: int, difat: list[int], index: int) -> int:
    per = sector // 4
    return 512 + difat[index // per] * sector + 4 * (index % per)


def _fat_get(b: bytearray, sector: int, difat: list[int], index: int) -> int:
    return struct.unpack_from("<I", b, _fat_slot(sector, difat, index))[0]


def _fat_set(b: bytearray, sector: int, difat: list[int], index: int, value: int) -> None:
    struct.pack_into("<I", b, _fat_slot(sector, difat, index), value)


def _cfb_chain(b: bytearray, sector: int, difat: list[int], start: int) -> list[int]:
    out: list[int] = []
    while start <= _MAXREGSECT:
        assert start not in out, "cyclic sector chain"
        out.append(start)
        start = _fat_get(b, sector, difat, start)
    return out


def _cfb_entry(b: bytearray, sector: int, difat: list[int], name: str) -> int:
    """File offset of the 128-byte directory entry named ``name``."""
    start = struct.unpack_from("<I", b, 48)[0]
    for sec in _cfb_chain(b, sector, difat, start):
        for slot in range(sector // 128):
            off = 512 + sec * sector + slot * 128
            if b[off + 66] == 0:  # STGTY_INVALID
                continue
            length = struct.unpack_from("<H", b, off + 64)[0]
            if bytes(b[off:off + max(0, length - 2)]).decode("utf-16le") == name:
                return off
    raise KeyError(name)


def _cfb_stream(data: bytes, name: str) -> bytes:
    """The content of a named stream, from either allocation."""
    b, sector, difat = _cfb_open(data)
    entry = _cfb_entry(b, sector, difat, name)
    start = struct.unpack_from("<I", b, entry + 116)[0]
    size = struct.unpack_from("<Q", b, entry + 120)[0]
    if size >= struct.unpack_from("<I", b, 56)[0]:
        chain = _cfb_chain(b, sector, difat, start)
        return b"".join(
            bytes(b[512 + s * sector:512 + (s + 1) * sector]) for s in chain)[:size]
    mini = 1 << struct.unpack_from("<H", b, 32)[0]
    root = _cfb_entry(b, sector, difat, "Root Entry")
    chain = _cfb_chain(b, sector, difat, struct.unpack_from("<I", b, root + 116)[0])
    whole = b"".join(bytes(b[512 + s * sector:512 + (s + 1) * sector]) for s in chain)
    return whole[start * mini:start * mini + size]


def _cfb_truncate_stream(data: bytes, name: str, keep: int) -> bytes:
    """Cut a stream's content to ``keep`` bytes, leaving the container valid.

    The point of the fixture is that the *file* stays consistent — [MS-CFB] has
    no complaint to make about it, and a compound-file library opens it and
    hands back a shorter stream without a word — while every offset the format's
    own bookkeeping records still points where the bytes used to be. The dropped
    sectors are freed **and zeroed**, so the truncated content is gone rather
    than merely unreferenced.
    """
    b, sector, difat = _cfb_open(data)
    entry = _cfb_entry(b, sector, difat, name)
    start = struct.unpack_from("<I", b, entry + 116)[0]
    size = struct.unpack_from("<Q", b, entry + 120)[0]
    cutoff = struct.unpack_from("<I", b, 56)[0]
    assert keep < size, f"{name} is already {size} bytes"
    # Below the cutoff the stream would have to move into the mini stream, which
    # is a different allocation entirely; keeping it above is not a detail.
    assert keep >= cutoff, f"{keep} would push {name} into the mini stream"
    chain = _cfb_chain(b, sector, difat, start)
    kept = (keep + sector - 1) // sector
    for dropped in chain[kept:]:
        _fat_set(b, sector, difat, dropped, _FREESECT)
        b[512 + dropped * sector:512 + (dropped + 1) * sector] = bytes(sector)
    _fat_set(b, sector, difat, chain[kept - 1], _ENDOFCHAIN)
    tail = kept * sector - keep
    if tail:
        last = 512 + chain[kept - 1] * sector
        b[last + sector - tail:last + sector] = bytes(tail)
    struct.pack_into("<Q", b, entry + 120, keep)
    return bytes(b)


def _cfb_patch_mini(data: bytes, name: str, at: int, blob: bytes) -> bytes:
    """Overwrite ``blob`` at ``at`` inside a stream held in the mini stream."""
    b, sector, difat = _cfb_open(data)
    entry = _cfb_entry(b, sector, difat, name)
    size = struct.unpack_from("<Q", b, entry + 120)[0]
    cutoff = struct.unpack_from("<I", b, 56)[0]
    assert size < cutoff, f"{name} is not in the mini stream"
    assert at + len(blob) <= size, "patch runs past the stream"
    mini = 1 << struct.unpack_from("<H", b, 32)[0]
    root = _cfb_entry(b, sector, difat, "Root Entry")
    chain = _cfb_chain(b, sector, difat, struct.unpack_from("<I", b, root + 116)[0])
    at += struct.unpack_from("<I", b, entry + 116)[0] * mini
    sec = chain[at // sector]
    inner = at % sector
    assert inner + len(blob) <= sector, "patch spans two sectors"
    b[512 + sec * sector + inner:512 + sec * sector + inner + len(blob)] = blob
    return bytes(b)


# The PowerPoint Document stream is a flat sequence of records, each an eight
# byte header (`recVer`/`recInstance`, `recType`, `recLen`) followed by
# `recLen` bytes. Walking it is how the cut below is chosen, so that the cut
# lands *inside* a record rather than tidily between two.
def _ppt_records(stream: bytes) -> list[tuple[int, int, int]]:
    """``(offset, recType, recLen)`` for the top-level records of a stream."""
    out, off = [], 0
    while off + 8 <= len(stream):
        _, kind, length = struct.unpack_from("<HHI", stream, off)
        out.append((off, kind, length))
        off += 8 + length
    return out


# [MS-PPT] 2.3.2: the CurrentUserAtom's headerToken is what a reader consults
# to learn whether a document is encrypted, before it parses anything else.
_PPT_TOKEN_PLAIN = 0xE391C05F
_PPT_TOKEN_ENCRYPTED = 0xF3D1C4DF
# Record header (8) + size (4) — the token is the field right after them.
_CURRENT_USER_TOKEN_AT = 12


def _assert_ppt(data: bytes, *, slides: int, pictures: bool) -> bytes:
    """The only gate a `.ppt` fixture can have, so it is not left ungated.

    A `.ppt` is not a package: `schema_gate.py` has no XML to validate and
    `render_gate.py`'s property is a round trip this phase will never have,
    because Phase 19 is read-only. What is left is a structural assertion at
    generation time, and `--check` runs the generators, so this is what stands
    between the corpus and a recorded constant that quietly stopped being the
    deck it claims to be — a LibreOffice that one day drops the picture, or
    flattens two slides into one, fails here rather than in a reader test whose
    expectations someone would then «fix».

    `RT_Slide` is 0x03EE and `RT_MainMaster` is 0x03F8 ([MS-PPT] 2.13.24): a
    slide is counted by its own container and not by anything derived.
    """
    found = sum(1 for _, kind, _ in _ppt_records(_cfb_stream(data, "PowerPoint Document"))
                if kind == 0x03EE)
    assert found == slides, f"expected {slides} RT_Slide containers, found {found}"
    blips = len(_cfb_stream(data, "Pictures"))
    assert bool(blips) == pictures, f"Pictures stream is {blips} bytes"
    token = struct.unpack_from(
        "<I", _cfb_stream(data, "Current User"), _CURRENT_USER_TOKEN_AT)[0]
    assert token == _PPT_TOKEN_PLAIN, f"unexpected headerToken 0x{token:08X}"
    return data


def ppt_basic_slides() -> None:
    """Two slides, title and body: the legacy twin of `pptx/basic-slides.pptx`.

    Its value is the pairing. «With LibreOffice, fidelity equivalent to the
    pptx path» is only measurable against a deck whose pptx form the corpus
    already carries and already pins with a golden.
    """
    _write_ppt("basic-slides.ppt",
               _assert_ppt(_ppt_bytes(BASIC_SLIDES_PPT_Z), slides=2, pictures=False))


def ppt_mixed_media() -> None:
    """Three slides, titles, bullets at two levels, and a picture."""
    _write_ppt("mixed-media.ppt",
               _assert_ppt(_ppt_bytes(MIXED_MEDIA_PPT_Z), slides=3, pictures=True))


def ppt_truncated() -> None:
    """A deck whose `PowerPoint Document` stream stops in the middle.

    Not a file that merely *looks* truncated. The compound file is intact and
    self-consistent, so nothing fails at open; what is gone is the second half
    of the stream every offset in the format points into. Concretely, after the
    cut:

    * `CurrentUserAtom.offsetToCurrentEdit` still addresses the UserEditAtom
      near the original end of the stream, thousands of bytes past what exists.
      A reader that seeks there without checking runs off the end.
    * The cut lands inside a record whose eight-byte header survives and
      declares a length reaching well beyond it, so a walk that trusts `recLen`
      reads past the end on its own, without any seek at all.

    Both are the shape Phase 19-E has to refuse and Phase 20 will fuzz.
    """
    base = _ppt_bytes(BASIC_SLIDES_PPT_Z)
    stream = _cfb_stream(base, "PowerPoint Document")
    # Half the stream, rounded up to a whole sector so the allocation stays
    # tidy and only the *content* is what changed. The two assertions are the
    # fixture's whole point, and they are checked rather than assumed: a
    # refreshed constant that happened to put a record boundary here would
    # produce a file that no longer tests anything, and it would say so.
    cut = -(-(len(stream) // 2) // 512) * 512
    inside = [r for r in _ppt_records(stream) if r[0] < cut < r[0] + 8 + r[2]]
    assert inside, f"the cut at {cut} falls between records, not inside one"
    edit_at = struct.unpack_from(
        "<I", _cfb_stream(base, "Current User"), _CURRENT_USER_TOKEN_AT + 4)[0]
    assert edit_at > cut, "the UserEditAtom would survive the cut"
    _write_ppt("truncated.ppt", _cfb_truncate_stream(base, "PowerPoint Document", cut))


def ppt_encrypted() -> None:
    """A deck that declares itself encrypted, and must be refused by name.

    The honest caveat, because a fixture that overstates what it proves is
    worse than none: the content behind this flag is **not** ciphertext.
    Neither LibreOffice nor anything else in this tree can write RC4-encrypted
    binary PowerPoint — `--convert-to ppt` with `EncryptFile` and a password was
    tried and produces a byte-identical plaintext file — so what is generated
    here is the declaration a reader is required to honour: `headerToken` set to
    `0xF3D1C4DF`, which is [MS-PPT] 2.3.2's «this document is encrypted» and the
    first thing a conforming reader looks at. It proves the refusal. It does not
    prove anything about decryption, and nothing in this phase claims to.
    """
    base = _assert_ppt(_ppt_bytes(BASIC_SLIDES_PPT_Z), slides=2, pictures=False)
    out = _cfb_patch_mini(base, "Current User", _CURRENT_USER_TOKEN_AT,
                          struct.pack("<I", _PPT_TOKEN_ENCRYPTED))
    token = struct.unpack_from(
        "<I", _cfb_stream(out, "Current User"), _CURRENT_USER_TOKEN_AT)[0]
    assert token == _PPT_TOKEN_ENCRYPTED, f"headerToken is 0x{token:08X}"
    assert sum(1 for a, b in zip(base, out) if a != b) == 4, "the patch spread"
    _write_ppt("encrypted.ppt", out)


# --------------------------------------------------------------------------
# The writer's embedded default template (plan v2 Phase 15-G)
# --------------------------------------------------------------------------
#
# Not a corpus fixture: this package is compiled into the binary
# (`crates/docsai-office/assets/default-template.pptx`, `include_bytes!`) and is
# what a deck with **no preserved package** is generated on — a deck typed as
# attribute-free Markdown, which has no skeleton to splice into.
#
# It is generated here rather than recorded as an opaque binary for the reason
# the whole corpus is: a template of unknown provenance cannot be reviewed, and
# every byte of this one is source in this file, under the repository's own
# licence. It carries no slides at all — a slide is what the writer generates —
# and four layouts, which is what automatic layout selection selects among.

TEMPLATE_PATH = (
    ROOT.parent / "crates" / "docsai-office" / "assets" / "default-template.pptx"
)

# 16:9 at 13.333 x 7.5 in, which is what every version of PowerPoint since 2013
# creates by default.
TEMPLATE_W = 12192000
TEMPLATE_H = 6858000

TEMPLATE_TITLE_FRAME = xfrm(838200, 365125, 10515600, 1325563)
TEMPLATE_BODY_FRAME = xfrm(838200, 1825625, 10515600, 4351338)
TEMPLATE_CTR_TITLE_FRAME = xfrm(1524000, 1122363, 9144000, 2387600)
TEMPLATE_SUBTITLE_FRAME = xfrm(1524000, 3602038, 9144000, 1655762)


def template_layout(name: str, kind: str, shapes: str) -> bytes:
    return (XML_DECL + (
        f'<p:sldLayout {P_NS} type="{kind}" preserve="1">'
        + sp_tree(shapes, name=name)
        + "</p:sldLayout>"
    )).encode()


def office_default_template() -> None:
    """The package `docsai-office` embeds and generates a skeleton-less deck on.

    Four layouts, because automatic layout selection needs something to select
    between and the two rules of analysis §6.6 name two of them:

    * `Title Slide` — a centred title and a subtitle: a slide whose content is
      a title and nothing else.
    * `Title and Content` — a title and a body: a title with bullets under it.
    * `Title Only` and `Blank` — what a deck with free shapes on it needs, and
      what a layout chooser has to be able to *not* pick.
    """
    parts: dict[str, bytes] = {}

    master = (
        f'<p:sldMaster {P_NS}>'
        + sp_tree(
            shape(2, "Title Placeholder 1", a_p(""), ph=ph("title"),
                  frame=TEMPLATE_TITLE_FRAME)
            + shape(3, "Text Placeholder 2", a_p(""), ph=ph("body", 1),
                    frame=TEMPLATE_BODY_FRAME),
            name="Office Theme",
        )
        + '<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1"'
        ' accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5"'
        ' accent6="accent6" hlink="hlink" folHlink="folHlink"/>'
        "<p:sldLayoutIdLst>"
        + "".join(
            f'<p:sldLayoutId id="{2147483649 + i}" r:id="rIdLayout{i + 1}"/>'
            for i in range(4)
        )
        + "</p:sldLayoutIdLst>"
        "<p:txStyles>"
        '<p:titleStyle><a:lvl1pPr algn="l"><a:defRPr sz="4400"><a:solidFill>'
        '<a:schemeClr val="tx1"/></a:solidFill>'
        '<a:latin typeface="+mj-lt"/></a:defRPr></a:lvl1pPr></p:titleStyle>'
        "<p:bodyStyle>"
        + "".join(
            f'<a:lvl{i}pPr marL="{342900 * i}" indent="-342900">'
            f'<a:buChar char="•"/><a:defRPr sz="{2800 - 400 * (i - 1)}">'
            '<a:solidFill><a:schemeClr val="tx1"/></a:solidFill>'
            f'<a:latin typeface="+mn-lt"/></a:defRPr></a:lvl{i}pPr>'
            for i in range(1, 5)
        )
        + "</p:bodyStyle>"
        '<p:otherStyle><a:lvl1pPr><a:defRPr sz="1800"><a:solidFill>'
        '<a:schemeClr val="tx1"/></a:solidFill>'
        '<a:latin typeface="+mn-lt"/></a:defRPr></a:lvl1pPr></p:otherStyle>'
        "</p:txStyles>"
        "</p:sldMaster>"
    )
    parts["ppt/slideMasters/slideMaster1.xml"] = (XML_DECL + master).encode()
    parts["ppt/slideMasters/_rels/slideMaster1.xml.rels"] = rels_part(
        [
            (f"rIdLayout{i + 1}", "slideLayout", f"../slideLayouts/slideLayout{i + 1}.xml")
            for i in range(4)
        ]
        + [("rIdTheme", "theme", "../theme/theme1.xml")]
    )

    parts["ppt/slideLayouts/slideLayout1.xml"] = template_layout(
        "Title Slide", "title",
        shape(2, "Title 1", "", ph=ph("ctrTitle"), frame=TEMPLATE_CTR_TITLE_FRAME)
        + shape(3, "Subtitle 2", "", ph=ph("subTitle", 1),
                frame=TEMPLATE_SUBTITLE_FRAME),
    )
    parts["ppt/slideLayouts/slideLayout2.xml"] = template_layout(
        "Title and Content", "obj",
        shape(2, "Title 1", "", ph=ph("title"), frame=TEMPLATE_TITLE_FRAME)
        + shape(3, "Content Placeholder 2", "", ph=ph("body", 1),
                frame=TEMPLATE_BODY_FRAME),
    )
    parts["ppt/slideLayouts/slideLayout3.xml"] = template_layout(
        "Title Only", "titleOnly",
        shape(2, "Title 1", "", ph=ph("title"), frame=TEMPLATE_TITLE_FRAME),
    )
    parts["ppt/slideLayouts/slideLayout4.xml"] = template_layout("Blank", "blank", "")
    for i in range(4):
        parts[f"ppt/slideLayouts/_rels/slideLayout{i + 1}.xml.rels"] = rels_part(
            [("rIdMaster", "slideMaster", "../slideMasters/slideMaster1.xml")]
        )

    parts["ppt/notesMasters/notesMaster1.xml"] = (XML_DECL + NOTES_MASTER).encode()
    parts["ppt/notesMasters/_rels/notesMaster1.xml.rels"] = rels_part(
        [("rIdTheme", "theme", "../theme/theme1.xml")]
    )

    parts["ppt/theme/theme1.xml"] = (XML_DECL + THEME).encode()
    # A table generated onto a slide names a style, and a style reference with
    # no `tableStyles.xml` to resolve it against is a table with no borders.
    parts["ppt/tableStyles.xml"] = (XML_DECL + (
        '<a:tblStyleLst xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"'
        ' def="{2D5ABB26-0587-4C30-8999-92F81FD0307C}"/>'
    )).encode()

    parts["docProps/core.xml"] = core_props("docsai", author="docsai")
    parts["docProps/app.xml"] = APP_PROPS

    # `p:sldIdLst` is present and **empty**: the template carries no slide, and
    # the writer splices the entries of the slides it generates into it.
    parts["ppt/presentation.xml"] = (XML_DECL + (
        f'<p:presentation {P_NS}>'
        '<p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rIdMaster1"/></p:sldMasterIdLst>'
        '<p:notesMasterIdLst><p:notesMasterId r:id="rIdNotesMaster"/></p:notesMasterIdLst>'
        "<p:sldIdLst/>"
        f'<p:sldSz cx="{TEMPLATE_W}" cy="{TEMPLATE_H}"/>'
        f'<p:notesSz cx="{TEMPLATE_H}" cy="{TEMPLATE_W}"/>'
        "</p:presentation>"
    )).encode()
    parts["ppt/_rels/presentation.xml.rels"] = rels_part([
        ("rIdMaster1", "slideMaster", "slideMasters/slideMaster1.xml"),
        ("rIdNotesMaster", "notesMaster", "notesMasters/notesMaster1.xml"),
        ("rIdTableStyles", "tableStyles", "tableStyles.xml"),
        ("rIdTheme", "theme", "theme/theme1.xml"),
    ])

    parts["[Content_Types].xml"] = content_types(
        {
            "rels": "application/vnd.openxmlformats-package.relationships+xml",
            "xml": "application/xml",
        },
        [
            ("/ppt/presentation.xml", f"{PML_TYPE}.presentation.main+xml"),
            ("/ppt/slideMasters/slideMaster1.xml", f"{PML_TYPE}.slideMaster+xml"),
            ("/ppt/notesMasters/notesMaster1.xml", f"{PML_TYPE}.notesMaster+xml"),
            ("/ppt/tableStyles.xml", f"{PML_TYPE}.tableStyles+xml"),
            ("/ppt/theme/theme1.xml", f"{DML_TYPE}.theme+xml"),
            ("/docProps/core.xml",
             "application/vnd.openxmlformats-package.core-properties+xml"),
            ("/docProps/app.xml",
             "application/vnd.openxmlformats-officedocument.extended-properties+xml"),
        ]
        + [
            (f"/ppt/slideLayouts/slideLayout{i + 1}.xml", f"{PML_TYPE}.slideLayout+xml")
            for i in range(4)
        ],
    )
    parts["_rels/.rels"] = rels_part([
        ("rId1", "officeDocument", "ppt/presentation.xml"),
        ("rId2", "metadata/core-properties", "docProps/core.xml"),
        ("rId3", "extended-properties", "docProps/app.xml"),
    ], root=True)

    write_package(TEMPLATE_PATH, parts)


GENERATORS = [
    docx_basic_text,
    docx_basic_styles,
    docx_nested_lists,
    docx_long_report,
    docx_table_simple,
    docx_table_merged,
    docx_images_inline,
    docx_images_floating,
    docx_images_transformed,
    docx_images_duplicated,
    docx_headers_footers,
    docx_footnotes,
    docx_custom_styles,
    docx_redundant_formatting,
    docx_repeated_formatting,
    docx_images_vml,
    docx_fields_raw,
    docx_comments_review,
    xlsx_values_types,
    xlsx_formulas_basic,
    xlsx_formulas_shared,
    xlsx_number_formats,
    xlsx_merged_cells,
    xlsx_images_anchored,
    odt_basic_text,
    odt_basic_styles,
    odt_nested_lists,
    odt_table_simple,
    odt_table_merged,
    odt_images_inline,
    odt_images_floating,
    odt_images_transformed,
    odt_headers_footers,
    odt_footnotes,
    ods_values_types,
    ods_formulas_basic,
    ods_merged_cells,
    ods_images_anchored,
    odp_basic_slides,
    odp_slide_order,
    odp_placeholders_cascade,
    odp_placeholders_empty,
    odp_presentation_classes,
    odp_bullets_levels,
    odp_notes_speaker,
    odp_reading_order,
    odp_raw_preserved,
    odp_tables_simple,
    odp_images_anchored,
    odp_shapes_geometry,
    odp_charts_embedded,
    odp_charts_multi_series,
    odp_ole_embedded,
    odp_comments_slide,
    odp_forty_slides,
    doc_basic_text,
    doc_encrypted,
    pptx_basic_slides,
    pptx_slide_order,
    pptx_placeholders_cascade,
    pptx_placeholders_empty,
    pptx_bullets_levels,
    pptx_notes_speaker,
    pptx_notes_crossed,
    pptx_reading_order,
    pptx_raw_preserved,
    pptx_tables_simple,
    pptx_images_anchored,
    pptx_shapes_geometry,
    pptx_autofit_stale,
    pptx_charts_embedded,
    pptx_charts_stale_workbook,
    pptx_charts_multi_series,
    pptx_smartart_fallback,
    pptx_ole_embedded,
    pptx_comments_slide,
    pptx_macro_enabled,
    pptx_forty_slides,
    ppt_mixed_media_source,
    ppt_basic_slides,
    ppt_mixed_media,
    ppt_truncated,
    ppt_encrypted,
    office_default_template,
]


def digest_tree() -> dict[str, str]:
    out = {}
    # Not corpus, but generated here and checked here: the writer's embedded
    # template is source in this file too, and `--check` is what says so.
    if TEMPLATE_PATH.exists():
        out[str(TEMPLATE_PATH.relative_to(ROOT.parent))] = hashlib.sha256(
            TEMPLATE_PATH.read_bytes()
        ).hexdigest()
    for sub in ("docx", "xlsx", "odt", "ods", "odp", "doc", "ppt", "pptx"):
        d = ROOT / sub
        if not d.exists():
            continue
        for f in sorted(d.iterdir()):
            if f.is_file():
                out[f"{sub}/{f.name}"] = hashlib.sha256(f.read_bytes()).hexdigest()
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true",
                    help="regenerate into memory and fail if the tree differs")
    args = ap.parse_args()

    before = digest_tree()
    for gen in GENERATORS:
        gen()
    after = digest_tree()

    if args.check and before != after:
        changed = sorted(set(before) ^ set(after)) or [
            k for k in after if before.get(k) != after[k]
        ]
        print("corpus out of date, regenerate with corpus/generate.py:", file=sys.stderr)
        for k in changed:
            print(f"  {k}", file=sys.stderr)
        return 1

    print(f"generated {len(after)} generated files under {ROOT.parent}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

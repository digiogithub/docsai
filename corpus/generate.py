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


def write_package(path: Path, parts: dict[str, bytes]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as zf:
        for name in sorted(parts):
            info = zipfile.ZipInfo(name, date_time=FIXED_DATE)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o600 << 16
            zf.writestr(info, parts[name])


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
        doc_rel_xml.append(
            f'<Relationship Id="{rid}" Type="{REL_BASE}/{kind}" Target="{target}"{mode}/>'
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
    body = (
        p(r("Antes del control de contenido."))
        + sdt
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
        "settings.xml": (
            f'<?xml version="1.0" encoding="UTF-8"?>'
            f"<office:document-settings {ODF_NS} office:version=\"1.3\">"
            f"<office:settings/></office:document-settings>"
        ).encode(),
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


GENERATORS = [
    docx_basic_text,
    docx_basic_styles,
    docx_nested_lists,
    docx_table_simple,
    docx_table_merged,
    docx_images_inline,
    docx_images_floating,
    docx_images_transformed,
    docx_images_duplicated,
    docx_headers_footers,
    docx_footnotes,
    docx_custom_styles,
    docx_images_vml,
    docx_fields_raw,
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
    doc_basic_text,
    doc_encrypted,
]


def digest_tree() -> dict[str, str]:
    out = {}
    for sub in ("docx", "xlsx", "odt", "ods", "doc"):
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

    print(f"generated {len(after)} corpus files under {ROOT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

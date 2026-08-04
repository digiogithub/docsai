---
docmark: "1.0"
source-format: docx
title: "Formato repetido"
author: "docsai corpus"
last-modified-by: "docsai corpus"
created: 2026-01-01T00:00:00Z
modified: 2026-01-02T00:00:00Z
language: "es-ES"
application: "docsai-corpus"
page:
  size: A4
  margins: { top: 70.85pt, bottom: 70.85pt, left: 85.05pt, right: 85.05pt, header: 35.4pt, footer: 35.4pt }
  orientation: portrait
style-defaults:
  font: { name: "Calibri", size: 11pt }
  paragraph: { space-after: 8pt, line-height: 1.079 }
styles:
  Heading1:
    type: paragraph
    name: "heading 1"
    based-on: Normal
    font: { name: "Calibri Light", size: 16pt, color: "#2E74B5" }
    paragraph: { space-before: 12pt, space-after: 4pt, keep-with-next: true, outline-level: 0 }
  Heading2:
    type: paragraph
    name: "heading 2"
    based-on: Normal
    font: { size: 13pt, color: "#2E74B5" }
    paragraph: { outline-level: 1 }
  Hyperlink:
    type: character
    font: { color: "#0563C1", underline: single }
  Normal:
    type: paragraph
    default: true
  TableGrid:
    type: table
    name: "Table Grid"
attribute-sets:
  g1: "color=#1F4E79 font=Consolas size=12pt"
  g2: "indent-left=36pt space-after=6pt space-before=6pt"
---

[convert]{.g1} transforma un documento al formato pivote.

Nota sobre convert: sin estilo, con sangria manual. {.g2}

[inspect]{.g1} describe la estructura sin convertir nada.

Nota sobre inspect: sin estilo, con sangria manual. {.g2}

[outline]{.g1} devuelve el mapa de nodos direccionables.

Nota sobre outline: sin estilo, con sangria manual. {.g2}

[tokens]{.g1} mide lo que cuesta leer el documento.

Nota sobre tokens: sin estilo, con sangria manual. {.g2}

[search]{.g1} localiza texto y devuelve identificadores.

Nota sobre search: sin estilo, con sangria manual. {.g2}

[roundtrip]{.g1} comprueba la identidad de ida y vuelta.

Nota sobre roundtrip: sin estilo, con sangria manual. {.g2}

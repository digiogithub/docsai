---
docmark: "1.0"
source-format: docx
title: "Listas anidadas"
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
  ListParagraph:
    type: paragraph
    name: "List Paragraph"
    based-on: Normal
  Normal:
    type: paragraph
    default: true
  TableGrid:
    type: table
    name: "Table Grid"
list-definitions:
  L1:
    levels:
      - { format: decimal, text: "%1.", start: 1, indent: 48px, hanging: 24px }
      - { format: lowerLetter, text: "%2)", start: 1, indent: 96px, hanging: 24px }
      - { format: lowerRoman, text: "%3.", start: 1, indent: 144px, hanging: 12px }
  L2:
    levels:
      - { format: bullet, text: "•", indent: 48px, hanging: 24px }
      - { format: bullet, text: "o", indent: 96px, hanging: 24px }
---

Lista numerada anidada:

1. Primer punto {.ListParagraph list=L1}
   1. Sub-punto a {.ListParagraph list=L1}
      1. Sub-sub-punto i {.ListParagraph list=L1}
   2. Sub-punto b {.ListParagraph}
2. Segundo punto {.ListParagraph}

Lista de vinetas:

- Vineta uno {.ListParagraph list=L2}
  - Vineta anidada {.ListParagraph list=L2}
- Vineta dos {.ListParagraph}

Parrafo posterior fuera de la lista.

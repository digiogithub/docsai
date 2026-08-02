---
docmark: "1.0"
source-format: odt
title: "Listas anidadas"
author: "docsai corpus"
last-modified-by: "docsai corpus"
created: 2026-01-01T00:00:00Z
modified: 2026-01-02T00:00:00Z
language: "es-ES"
application: "docsai-corpus"
page:
  size: A4
  margins: { top: 2cm, bottom: 2cm, left: 2cm, right: 2cm, header: 0px, footer: 0px }
  orientation: portrait
style-defaults:
  font: { name: "Liberation Serif", size: 12pt }
  paragraph: { line-height: 1.15 }
styles:
  Emphasis:
    type: character
    font: { italic: true }
  Heading:
    type: paragraph
    based-on: Standard
    next: Standard
    font: { name: "Liberation Sans", size: 14pt }
    paragraph: { space-before: 11.99pt, space-after: 6.01pt, keep-with-next: true }
  Heading_20_1:
    type: paragraph
    name: "Heading 1"
    based-on: Heading
    next: Standard
    font: { size: 16pt, color: "#2E74B5", bold: true }
    paragraph: { outline-level: 0 }
  Heading_20_2:
    type: paragraph
    name: "Heading 2"
    based-on: Heading
    font: { size: 14pt, color: "#2E74B5", bold: true }
    paragraph: { outline-level: 1 }
  Standard:
    type: paragraph
    default: true
  Strong_20_Emphasis:
    type: character
    name: "Strong Emphasis"
    font: { bold: true }
list-definitions:
  L1:
    levels:
      - { format: bullet, text: "•", indent: 24px, hanging: 24px }
      - { format: bullet, text: "◦", indent: 48px, hanging: 24px }
  L2:
    levels:
      - { format: decimal, text: "%1.", indent: 24px, hanging: 24px }
      - { format: lowerLetter, text: "%2.", indent: 48px, hanging: 24px }
  L3:
    levels:
      - { format: bullet, text: "•", indent: 24px, hanging: 24px }
      - { format: bullet, text: "◦", indent: 48px, hanging: 24px }
  L4:
    levels:
      - { format: decimal, text: "%1.", indent: 24px, hanging: 24px }
      - { format: lowerLetter, text: "%2.", indent: 48px, hanging: 24px }
---

Listas anidadas: {.Standard}

- Uno {.Standard list=L3}
  - Uno-A {.Standard list=L3}
  - Uno-B {.Standard}
- Dos {.Standard}

1. Primero {.Standard list=L4}
   1. Primero-a {.Standard list=L4}
2. Segundo {.Standard}

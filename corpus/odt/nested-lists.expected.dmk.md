---
docmark: "1.1"
source-format: odt
next-id: 5
title: "Listas anidadas"
author: "docsai corpus"
last-modified-by: "docsai corpus"
created: 2026-01-01T00:00:00Z
modified: 2026-01-02T00:00:00Z
language: "es-ES"
application: "docsai-corpus"
page:
  size: A4
  margins: { top: 2cm, bottom: 2cm, left: 2cm, right: 2cm, header: 0, footer: 0 }
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
    font: { color: "#2E74B5", bold: true }
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
      - { format: bullet, text: "•", indent: 18pt, hanging: 18pt }
      - { format: bullet, text: "◦", indent: 36pt, hanging: 18pt }
  L2:
    levels:
      - { format: decimal, text: "%1.", indent: 18pt, hanging: 18pt }
      - { format: lowerLetter, text: "%2.", indent: 36pt, hanging: 18pt }
  L3:
    levels:
      - { format: bullet, text: "•", indent: 18pt, hanging: 18pt }
      - { format: bullet, text: "◦", indent: 36pt, hanging: 18pt }
  L4:
    levels:
      - { format: decimal, text: "%1.", indent: 18pt, hanging: 18pt }
      - { format: lowerLetter, text: "%2.", indent: 36pt, hanging: 18pt }
---

Listas anidadas:

- Uno {list=L3 list-id=n1}
  - Uno-A {list=L3 list-id=n2}
  - Uno-B
- Dos

1. Primero {list=L4 list-id=n3}
   1. Primero-a {list=L4 list-id=n4}
2. Segundo

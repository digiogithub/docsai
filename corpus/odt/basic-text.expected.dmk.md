---
docmark: "1.0"
source-format: odt
title: "Texto basico"
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
    font: { color: "#2E74B5", bold: true }
    paragraph: { outline-level: 1 }
  Standard:
    type: paragraph
    default: true
  Strong_20_Emphasis:
    type: character
    name: "Strong Emphasis"
    font: { bold: true }
---

Primer parrafo del documento.

Segundo parrafo con un salto  
de linea manual.

Caracteres que Markdown escapa: \*asterisco\* \_guion\_ #almohadilla |tuberia| \[corchete\] \<angulo> \`backtick\` \\barra.

[]{.empty}

Parrafo final tras uno vacio.

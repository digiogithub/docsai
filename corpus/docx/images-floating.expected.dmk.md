---
docmark: "1.0"
source-format: docx
title: "Imagenes flotantes"
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
---

![Logo flotante cuadrado](assets/img-40e10599.png){anchor=floating height=2.6cm name=Logo native-size=120x90 relative-to=margin relative-to-v=paragraph width=3.5cm wrap=square wrap-side=right x=1.2cm y=0.5cm z-index=2}Texto que rodea a la imagen flotante anclada al margen.

![Banner centrado](assets/img-8cbae2d9.png){align-h=center align-v=top anchor=floating height=4cm name=Banner native-size=64x64 relative-to=page width=6cm wrap=top-bottom z-index=3}Parrafo con imagen anclada a la pagina y alineacion simbolica.

![Marca de agua](assets/img-40e10599.png){anchor=behind height=7.5cm name="Marca de agua" native-size=120x90 relative-to=page width=10cm wrap=none x=2cm y=8cm z-index=1}Parrafo sobre la marca de agua detras del texto.

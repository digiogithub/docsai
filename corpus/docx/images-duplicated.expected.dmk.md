---
docmark: "1.1"
source-format: docx
next-id: 7
title: "Imagenes duplicadas"
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

El mismo mapa de bits tres veces con geometrias distintas.

![Original](assets/img-40e10599.png){#n2 height=90px name="Copia A" native-size=120x90 width=120px} {#n1}

![Mitad](assets/img-40e10599.png){#n4 height=45px name="Copia B" native-size=120x90 width=60px} {#n3}

![Flotante](assets/img-40e10599.png){#n6 anchor=floating height=3.75cm name="Copia C" native-size=120x90 relative-to=margin relative-to-v=paragraph width=5cm wrap=square wrap-side=largest x=2cm y=0 z-index=251658240}Tercera aparicion, flotante. {#n5}

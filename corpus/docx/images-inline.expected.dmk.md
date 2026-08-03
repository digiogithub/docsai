---
docmark: "1.1"
source-format: docx
next-id: 7
title: "Imagenes en linea"
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

Imagen en linea a tamano nativo:

![Diagrama de ventas](assets/img-40e10599.png){#n2 height=90px name=Diagrama native-size=120x90 title="Figura 1" width=120px} {#n1}

Imagen GIF en medio ![Icono verde](assets/img-7fcbd432.gif){#n4 height=32px name=Icono native-size=48x32 width=48px} del parrafo. {#n3}

Imagen EMF vectorial (no renderizable):

![Logo vectorial](assets/img-a03c5ab4.emf){#n6 height=2cm name=LogoEMF render=unsupported width=4cm} {#n5}

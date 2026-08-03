---
docmark: "1.1"
source-format: odt
next-id: 5
title: "Imagenes flotantes"
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
---

![FloatPara](assets/img-40e10599.png){#n2 anchor=floating height=3cm name=FloatPara relative-to=paragraph width=4cm wrap=square x=1cm y=0.5cm z-index=1}Texto que fluye junto a la imagen flotante anclada al parrafo. {#n1 .Standard}

![BehindPage](assets/img-8cbae2d9.png){#n4 anchor=behind height=3cm name=BehindPage relative-to=page width=3cm wrap=through x=2cm y=5cm z-index=0}Texto sobre imagen detras de la pagina. {#n3 .Standard}

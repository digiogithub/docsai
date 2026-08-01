---
docmark: "1.0"
source-format: docx
title: "Imagenes transformadas"
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

Imagen rotada 45 grados:

![Rotada](assets/img-40e10599.png){anchor=floating height=3cm name=Rotada native-size=120x90 relative-to=margin relative-to-v=paragraph rotation=45 width=4cm wrap=square x=0px y=0px z-index=251658240}

Imagen recortada con borde:

![Recortada](assets/img-40e10599.png){border="1pt solid #000000" crop="10%,5%,20%,0%" height=2.25cm name=Recortada native-size=120x90 width=3cm}

Imagen volteada y escalada al 50 %:

![Volteada](assets/img-8cbae2d9.png){anchor=floating flip=hv height=1.5cm name=Volteada native-size=64x64 relative-to=margin relative-to-v=paragraph width=2cm wrap=square x=0px y=0px z-index=251658240}

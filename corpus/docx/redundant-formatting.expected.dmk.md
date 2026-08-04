---
docmark: "1.1"
source-format: docx
next-id: 2
title: "Formato redundante"
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
  Resaltado:
    type: paragraph
    based-on: Normal
    font: { size: 14pt, color: "#C00000" }
    paragraph: { align: center, space-before: 6pt }
  ResaltadoHijo:
    type: paragraph
    name: "Resaltado hijo"
    based-on: Resaltado
    font: { bold: true }
  TableGrid:
    type: table
    name: "Table Grid"
---

# Titulo con estilo implicito {#n1}

Todo esto ya lo dice el estilo. {.Resaltado}

Y esto lo dice dos veces. {.Resaltado}

*Heredado dos niveles* y una diferencia real. {.ResaltadoHijo}

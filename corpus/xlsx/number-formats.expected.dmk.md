---
docmark: "1.1"
source-format: xlsx
next-id: 2
title: "Formatos de numero"
author: "docsai corpus"
last-modified-by: "docsai corpus"
created: 2026-01-01T00:00:00Z
modified: 2026-01-02T00:00:00Z
language: "es-ES"
application: "docsai-corpus"
workbook:
  active-sheet: "Formatos"
styles:
  Fdc998c06ffdae14:
    type: character
    font: { name: "Calibri", size: 11pt, color: "#FFFFFF", bold: true }
---

# Formatos {#n1 .sheet cols="A:B"}

|       | A          | B          |
| ----- | ---------- | ---------- |
| **1** | Concepto   | Valor      |
| **2** | Moneda     | 1234.5     |
| **3** | Fecha      | 2025-01-01 |
| **4** | Porcentaje | 0.175      |
| **5** | Millares   | 1234567    |

::: {.cell-meta}
- A1:B1: style=Fdc998c06ffdae14
- B2: num-fmt="#,##0.00\\ \"EUR\"" type=number
- B3: num-fmt="dd/mm/yyyy" type=date
- B4: num-fmt=0.0% type=number
- B5: num-fmt="#,##0" type=number
:::

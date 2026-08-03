---
docmark: "1.1"
source-format: xlsx
next-id: 2
title: "Formulas basicas"
author: "docsai corpus"
last-modified-by: "docsai corpus"
created: 2026-01-01T00:00:00Z
modified: 2026-01-02T00:00:00Z
language: "es-ES"
application: "docsai-corpus"
workbook:
  active-sheet: "Datos"
  defined-names:
    TOTAL_ANUAL: "Datos!$B$5"
styles:
  Fdc998c06ffdae14:
    type: character
    font: { name: "Calibri", size: 11pt, color: "#FFFFFF", bold: true }
---

# Datos {#n1 .sheet cols="A:C"}

|       | A            | B   | C   |
| ----- | ------------ | --- | --- |
| **1** | Producto     | T1  | T2  |
| **2** | Widgets      | 100 | 200 |
| **3** | Gadgets      | 150 | 250 |
| **4** | Total        | 250 | 450 |
| **5** | Suma general | 700 |     |

::: {.cell-meta}
- A1:C1: style=Fdc998c06ffdae14
- B2:C3: type=number
- B4: formula="SUM(B2:B3)" type=number
- B5: formula=B4+C4 type=number
- C4: formula="SUM(C2:C3)" type=number
:::

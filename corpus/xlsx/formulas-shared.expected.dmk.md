---
docmark: "1.1"
source-format: xlsx
next-id: 2
title: "Formulas compartidas y de matriz"
author: "docsai corpus"
last-modified-by: "docsai corpus"
created: 2026-01-01T00:00:00Z
modified: 2026-01-02T00:00:00Z
language: "es-ES"
application: "docsai-corpus"
workbook:
  active-sheet: "Compartidas"
---

# Compartidas {#n1 .sheet cols="A:C"}

|       | A    | B   | C   |
| ----- | ---- | --- | --- |
| **1** | 10   | 20  | 30  |
| **2** | 11   | 21  | 32  |
| **3** | 12   | 22  | 34  |
| **4** | 1274 |     |     |

::: {.cell-meta}
- A1:B3: type=number
- A4: array-over=A4 formula="SUM(A1:A3*B1:B3)" type=number
- C1: formula=A1+B1 shared-over="C1:C3" type=number
- C2: formula=A2+B2 shared-over="C1:C3" type=number
- C3: formula=A3+B3 shared-over="C1:C3" type=number
:::

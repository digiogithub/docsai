---
docmark: "1.0"
source-format: ods
title: "Formulas basicas"
author: "docsai corpus"
last-modified-by: "docsai corpus"
created: 2026-01-01T00:00:00Z
modified: 2026-01-02T00:00:00Z
language: "es-ES"
application: "docsai-corpus"
workbook:
  active-sheet: "Calc"
---

# Calc {.sheet cols="A:C"}

|       | A   | B   | C   |
| ----- | --- | --- | --- |
| **1** | 10  | 20  | 30  |
| **2** | 30  |     |     |

::: {.cell-meta}
- A1:B1: type=number
- A2: formula="SUM([.A1:.B1])" formula-dialect=openformula type=number
- C1: formula="[.A1]+[.B1]" formula-dialect=openformula type=number
:::

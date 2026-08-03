# Corpus token budget

What every corpus document costs an LLM, measured with `o200k_base` over the DocMark
`docsai convert` would write. Generated — do not edit by hand:

```bash
DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test token_budget
```

An update that inflates the total by more than 5 % is refused unless
`DOCSAI_ACCEPT_TOKEN_INFLATION=1` says so on purpose.

| Document | full | standard | plain |
|---|---:|---:|---:|
| `docx/basic-styles.docx` | 541 | 301 | 72 |
| `docx/basic-text.docx` | 456 | 226 | 72 |
| `docx/custom-styles.docx` | 583 | 229 | 33 |
| `docx/fields-raw.docx` | 452 | 203 | 33 |
| `docx/footnotes.docx` | 471 | 193 | 39 |
| `docx/headers-footers.docx` | 465 | 239 | 11 |
| `docx/images-duplicated.docx` | 570 | 314 | 61 |
| `docx/images-floating.docx` | 642 | 386 | 89 |
| `docx/images-inline.docx` | 544 | 288 | 72 |
| `docx/images-transformed.docx` | 625 | 369 | 68 |
| `docx/images-vml.docx` | 480 | 240 | 27 |
| `docx/long-report.docx` | 9158 | 8773 | 8545 |
| `docx/nested-lists.docx` | 701 | 272 | 77 |
| `docx/table-merged.docx` | 496 | 260 | 63 |
| `docx/table-simple.docx` | 470 | 234 | 55 |
| `ods/formulas-basic.ods` | 235 | 159 | 54 |
| `ods/images-anchored.ods` | 205 | 192 | 30 |
| `ods/merged-cells.ods` | 218 | 183 | 76 |
| `ods/values-types.ods` | 238 | 180 | 74 |
| `odt/basic-styles.odt` | 537 | 242 | 52 |
| `odt/basic-text.odt` | 514 | 233 | 72 |
| `odt/footnotes.odt` | 457 | 162 | 14 |
| `odt/headers-footers.odt` | 487 | 206 | 8 |
| `odt/images-floating.odt` | 576 | 275 | 54 |
| `odt/images-inline.odt` | 470 | 176 | 16 |
| `odt/images-transformed.odt` | 487 | 193 | 15 |
| `odt/nested-lists.odt` | 741 | 210 | 37 |
| `odt/table-merged.odt` | 530 | 239 | 60 |
| `odt/table-simple.odt` | 486 | 195 | 33 |
| `xlsx/formulas-basic.xlsx` | 351 | 221 | 97 |
| `xlsx/formulas-shared.xlsx` | 314 | 199 | 89 |
| `xlsx/images-anchored.xlsx` | 372 | 290 | 36 |
| `xlsx/merged-cells.xlsx` | 300 | 189 | 71 |
| `xlsx/number-formats.xlsx` | 344 | 201 | 94 |
| `xlsx/values-types.xlsx` | 335 | 219 | 114 |
| **total** | **24851** | **16691** | **10413** |

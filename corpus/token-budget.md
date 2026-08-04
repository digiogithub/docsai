# Corpus token budget

What every corpus document costs an LLM, measured with `o200k_base` over the DocMark
`docsai convert` would write. Generated — do not edit by hand:

```bash
DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test token_budget
```

An update that inflates the total by more than 5 % is refused unless
`DOCSAI_ACCEPT_TOKEN_INFLATION=1` says so on purpose.

| Document | full | agent | standard | plain |
|---|---:|---:|---:|---:|
| `docx/basic-styles.docx` | 535 | 199 | 295 | 72 |
| `docx/basic-text.docx` | 456 | 166 | 226 | 72 |
| `docx/custom-styles.docx` | 583 | 166 | 229 | 33 |
| `docx/fields-raw.docx` | 484 | 175 | 203 | 33 |
| `docx/footnotes.docx` | 471 | 157 | 193 | 39 |
| `docx/headers-footers.docx` | 465 | 167 | 239 | 11 |
| `docx/images-duplicated.docx` | 570 | 189 | 314 | 61 |
| `docx/images-floating.docx` | 642 | 217 | 386 | 89 |
| `docx/images-inline.docx` | 544 | 205 | 288 | 72 |
| `docx/images-transformed.docx` | 625 | 196 | 369 | 68 |
| `docx/images-vml.docx` | 480 | 141 | 240 | 27 |
| `docx/long-report.docx` | 9083 | 8744 | 8698 | 8545 |
| `docx/nested-lists.docx` | 701 | 238 | 272 | 77 |
| `docx/redundant-formatting.docx` | 528 | 156 | 206 | 35 |
| `docx/table-merged.docx` | 496 | 194 | 260 | 63 |
| `docx/table-simple.docx` | 470 | 171 | 234 | 55 |
| `ods/formulas-basic.ods` | 235 | 224 | 159 | 54 |
| `ods/images-anchored.ods` | 205 | 183 | 192 | 30 |
| `ods/merged-cells.ods` | 218 | 223 | 183 | 76 |
| `ods/values-types.ods` | 238 | 195 | 180 | 74 |
| `odt/basic-styles.odt` | 511 | 179 | 222 | 52 |
| `odt/basic-text.odt` | 493 | 166 | 214 | 72 |
| `odt/footnotes.odt` | 446 | 123 | 156 | 14 |
| `odt/headers-footers.odt` | 473 | 150 | 198 | 8 |
| `odt/images-floating.odt` | 566 | 173 | 269 | 54 |
| `odt/images-inline.odt` | 462 | 126 | 173 | 16 |
| `odt/images-transformed.odt` | 479 | 125 | 190 | 15 |
| `odt/nested-lists.odt` | 722 | 171 | 197 | 37 |
| `odt/table-merged.odt` | 521 | 185 | 236 | 60 |
| `odt/table-simple.odt` | 477 | 142 | 192 | 33 |
| `xlsx/formulas-basic.xlsx` | 351 | 277 | 221 | 97 |
| `xlsx/formulas-shared.xlsx` | 314 | 293 | 199 | 89 |
| `xlsx/images-anchored.xlsx` | 372 | 240 | 290 | 36 |
| `xlsx/merged-cells.xlsx` | 300 | 220 | 189 | 71 |
| `xlsx/number-formats.xlsx` | 344 | 216 | 201 | 94 |
| `xlsx/values-types.xlsx` | 335 | 234 | 219 | 114 |
| **total** | **25195** | **15326** | **16732** | **10448** |

# 08 — Phase 5 legacy DOC

What landed for Microsoft Word 97–2003 (`.doc`) reading, the two-level strategy,
and traps for later polish.

---

## Status

**Closed for the core path.** `docsai-office` reads `.doc` natively in degraded
mode; `docsai-convert` can pre-convert via LibreOffice headless to `.docx` when
available. Writing `.doc` remains out of scope.

---

## Delivered

| Item | Where |
|---|---|
| CFB open + FIB + piece table → paragraphs | `docsai-office::doc` |
| Encryption / obfuscation rejected (`ReadError::Encrypted`) | `doc::fib` |
| Embedded BLIP / signature image scan (inline, degraded geometry) | `doc::images` |
| OLE2 disambiguation via CFB directory (`WordDocument` vs `Workbook`/`Book`) | `detect` + `doc::classify_ole2` |
| LibreOffice discovery + `--use-loffice auto\|never\|require` | `docsai-convert::loffice`, CLI |
| Temp-dir `soffice --headless --convert-to docx` then docx pipeline | `pipeline::try_loffice_doc` |
| SUPPORT matrix lists `.doc` read | `docsai-convert::SUPPORT` |
| Corpus + tests | `corpus/doc/`, `tests/doc_native.rs`, `tests/doc_phase5.rs` |

---

## Technical decisions

1. **Two-level strategy unchanged** (technical-analysis §1.3). LibreOffice is
   optional and detected at runtime; the binary never hard-depends on it.
2. **`cfb` crate** for OLE2. Justification matches the analysis table: stable,
   pure-Rust, small. Added as a workspace dependency after advisory check.
3. **Native path is always marked degraded** with an actionable warning that
   points at `--use-loffice` / installing LibreOffice.
4. **LO path keeps `source_format = Doc`** so front matter and reports stay
   honest, even though bytes were pre-converted to docx.
5. **`DOCSAI_LIBREOFFICE` / `LIBREOFFICE_PATH` are strict overrides.** If set to
   a non-executable path, discovery returns `None` instead of falling through to
   `PATH` (predictable CI and operator control).
6. **Synthetic corpus**, not Word-saved binaries. Fixtures are built by
   `doc::test_fixture` and embedded base64 in `corpus/generate.py` so
   `generate.py --check` stays dependency-free. Real LO-saved samples can be
   added later without changing the reader contract.

---

## User-facing behaviour

```bash
docsai convert legacy.doc -o out.dmk.md                 # auto: LO if present, else native
docsai convert legacy.doc -o out.dmk.md --use-loffice never
docsai convert legacy.doc -o out.dmk.md --use-loffice require
```

Without LibreOffice, stderr / `--json` shows the degraded warning. With
`--use-loffice require` and no binary, conversion fails with
`ConvertError::Loffice`.

---

## Known gaps / non-blocking polish

- Headers, footnotes, lists, and tables are not reconstructed on the native path
  (text of the main document story only).
- SummaryInformation parsing is best-effort (title/author scan).
- BLIP extraction is signature / OfficeArt-record based without wrap geometry.
- No end-to-end CI job with real LibreOffice installed (optional enhancement).
- Round-trip for `.doc` goes out as `.docx` (write of `.doc` is out of scope).

---

## Traps

- Piece-table `fc` with the compression flag uses **byte offset = (fc & mask) / 2**
  for ANSI pieces; Unicode pieces use the raw offset.
- `ccpText` counts **characters across pieces**, not bytes; later CP ranges are
  footnotes/headers and must not be concatenated into the body.
- FIB `fWhichTblStm` selects `1Table` vs `0Table`; missing table stream is a hard
  error, not an empty document.

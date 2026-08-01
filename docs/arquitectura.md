# Arquitectura de docsai

Describe la estructura del software: workspace de crates, modelo de documento intermedio (IR),
pipelines de conversión, CLI y servidor MCP. Complementa a `analisis-tecnico.md` (el porqué)
y a `plan-desarrollo.md` (el cuándo).

## 1. Visión general

```
                 ┌────────────────────────── docsai-convert ──────────────────────────┐
  .docx ─┐       │                                                                    │
  .doc  ─┤  readers (docsai-office / docsai-odf)          writers                     │
  .odt  ─┼──────────────►  IR (docsai-model)  ◄──────────────────────┐                │
  .xlsx ─┤                    ▲        │                             │                │
  .xls  ─┤                    │        ▼                             │                │
  .ods  ─┘             parser DocMark  serializador DocMark   .docx/.xlsx/.odt/.ods   │
                        (docsai-docmark)                             ▲                │
                              ▲        │                             │                │
                              │        ▼                             │                │
                          fichero .dmk.md + assets/ ─────────────────┘                │
                 └─────────────────────────────────────────────────────────────────────┘
                                   ▲                          ▲
                            docsai-cli (clap)          docsai-mcp (rmcp, stdio)
```

Regla central: **ningún conversor habla con otro conversor**. Todo pasa por el IR.

## 2. Workspace de crates

| Crate | Tipo | Responsabilidad | Dependencias internas |
|---|---|---|---|
| `docsai-model` | lib | IR + tipos comunes (`Style`, `ConversionReport`, unidades) | ninguna |
| `docsai-docmark` | lib | Serializador y parser DocMark (IR ⇄ .dmk.md + assets) | model |
| `docsai-office` | lib | Readers docx/xlsx/xls/doc y writers docx/xlsx | model |
| `docsai-odf` | lib | Readers/writers odt/ods | model |
| `docsai-convert` | lib | Orquestación: detección de formato, pipelines, gestión de assets, fallback LibreOffice, informes | todas las anteriores |
| `docsai-cli` | bin | Binario `docsai` (subcomandos convert/inspect/roundtrip/mcp) | convert |
| `docsai-mcp` | lib | Implementación del servidor MCP (la CLI lo arranca con `docsai mcp`) | convert |

Notas:
- Un solo binario distribuido (`docsai`); `docsai-mcp` es lib para que el subcomando `mcp`
  viva dentro del mismo ejecutable.
- Features de Cargo por formato (`office-doc`, `odf`, …) para poder compilar variantes mínimas.

## 3. El modelo intermedio (IR) — `docsai-model`

Dos raíces:

```rust
pub enum Document {
    Text(TextDocument),
    Workbook(Workbook),
}

pub struct TextDocument {
    pub meta: DocumentMeta,          // título, autor, fechas, custom props, idioma
    pub styles: StyleCatalog,        // estilos nombrados con herencia (based_on)
    pub list_defs: ListCatalog,
    pub sections: Vec<Section>,      // geometría de página + headers/footers + bloques
}

pub enum Block {
    Paragraph(Paragraph),            // Vec<Inline> + ParaProps (style_id + deltas directos)
    Heading(Heading),                // nivel de esquema + Paragraph
    List(List),                      // árbol ya reconstruido (no pares numId/ilvl)
    Table(Table),                    // grid con spans, col-widths, estilo
    Image(ImageRef),                 // referencia a AssetStore + geometría/anclaje
    TextBox(TextBox),
    Raw(RawFragment),                // escotilla de fidelidad (formato origen + bytes XML)
    // …
}

pub enum Inline {
    Text(String),
    Styled(Vec<Inline>, RunProps),   // RunProps = style_id opcional + deltas (bold, color…)
    Link { target: String, content: Vec<Inline>, props: RunProps },
    Footnote(Vec<Block>),
    Field { kind: FieldKind, cached: String },
    Break(BreakKind),
    ImageInline(ImageRef),
}

pub struct Workbook {
    pub meta: DocumentMeta,
    pub styles: StyleCatalog,
    pub defined_names: Vec<DefinedName>,
    pub sheets: Vec<Sheet>,          // Sheet = grid dispersa de Cell + col/row props + merges + panes
}

pub struct Cell {
    pub value: CellValue,            // Number | Text | Bool | DateTime | Error | Empty
    pub formula: Option<Formula>,    // texto + dialecto (Ooxml | OpenFormula) + shared/array info
    pub num_fmt: Option<NumFmt>,
    pub style_id: Option<StyleId>,
}
```

Principios del IR:
- **Estilo = referencia + delta**, nunca formato aplanado (ver análisis §5.2).
- **Sin dependencia de I/O**: `docsai-model` no sabe de ZIP ni XML; los `ImageRef` apuntan a un
  `AssetStore` abstracto (trait) que materializa `docsai-convert`.
- Todos los tipos `serde`-serializables → `inspect --json` gratis y debugging sencillo.
- Unidades normalizadas a EMU/twips internamente con newtypes (`Length`), con conversión a
  `pt/cm/px` solo al serializar DocMark.

## 4. Contratos de conversión

```rust
pub trait DocumentReader {
    fn detect(path: &Path, sniff: &[u8]) -> DetectScore;      // por contenido, no solo extensión
    fn read(&self, input: &mut dyn ReadSeek, assets: &mut dyn AssetStore)
        -> Result<(Document, ConversionReport), ReadError>;
}

pub trait DocumentWriter {
    fn write(&self, doc: &Document, assets: &dyn AssetStore, out: &mut dyn Write)
        -> Result<ConversionReport, WriteError>;
}

pub struct ConversionReport {
    pub warnings: Vec<Warning>,       // tipadas: UnsupportedElement { kind, location, action }
    pub raw_blocks_emitted: u32,      //          Degraded { what, why } · AssetIssue { … }
    pub stats: ConversionStats,       // párrafos, celdas, imágenes, fórmulas procesadas
}
```

- Los readers **nunca hacen pánico** con entrada corrupta: siempre `Err` tipado.
- `ConversionReport` fluye hasta CLI (stderr legible / `--json`) y hasta la respuesta MCP.
- El comando `roundtrip` compara IR original vs IR tras ida-y-vuelta con un **diff estructural
  de IR normalizado** y produce una métrica de fidelidad (% nodos preservados por categoría).

## 5. CLI (`docsai-cli`)

```
docsai convert <in> [-o <out>] [--to <fmt>] [--fidelity full|standard|plain]
               [--assets-dir <dir>] [--style-map <yaml>] [--max-cells N]
               [--use-loffice auto|never|require] [--json]
docsai inspect <in> [--json]        # metadatos, estilos, hojas, medios, sin convertir
docsai roundtrip <in> [--report <path>] [--json]
docsai formats                       # matriz de soporte actual
docsai mcp                           # servidor MCP por stdio
```

- Formato de destino inferido por extensión de `-o`, forzable con `--to`.
- Entrada/salida por stdin/stdout soportadas (`-` como nombre) para pipelines, excepto formatos
  binarios de salida sobre terminal (protección estándar).
- Códigos de salida: 0 OK, 1 conversión con advertencias severas (`--strict` las hace fatales),
  2 error de entrada, 3 formato no soportado.

## 6. Servidor MCP (`docsai-mcp`)

Basado en `rmcp` (SDK oficial), transporte **stdio**. Logs a stderr exclusivamente.

Tools expuestas (v1):

| Tool | Entrada | Salida |
|---|---|---|
| `convert_to_markdown` | `path` (o `content_base64` + `filename`), `fidelity`, `assets` = `inline-base64`\|`files` | DocMark (texto), assets, `report` |
| `convert_from_markdown` | `markdown`, `target_format`, assets opcionales | fichero base64 o `path` escrito, `report` |
| `inspect_document` | `path`/`content_base64` | JSON de estructura (mismo shape que `inspect --json`) |
| `list_supported_formats` | — | matriz de soporte con estado por dirección |

Decisiones:
- Modo dual **path/base64**: los clientes MCP locales (Claude Desktop/Code) pasan rutas; los
  remotos pueden pasar contenido embebido. Límite de tamaño configurable con variable de entorno.
- Respuestas grandes: DocMark como `text content`; binarios como recurso base64 con MIME correcto.
- El servidor es sin estado; cada tool call es una conversión independiente (sin ficheros
  temporales persistentes salvo que el cliente pida `assets=files`).

## 7. Multiplataforma y distribución

- Sin dependencias de sistema en el camino principal (pure Rust). `soffice` se busca en runtime
  en ubicaciones estándar por SO solo para `.doc`.
- CI: GitHub Actions, matriz ubuntu/windows/macos; artefactos de release con `cargo-dist`
  (tar.gz/zip + instaladores shell/powershell; opcional: Homebrew tap, Scoop/winget, cargo-binstall).
- Targets: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`,
  `aarch64-apple-darwin`, `x86_64-apple-darwin`. LTO + `strip` en release.

## 8. Rendimiento (presupuestos orientativos)

- docx de 100 páginas → DocMark: < 1 s en hardware corriente.
- xlsx de 100k celdas: < 3 s y < 500 MB de RAM (evitar clonar sharedStrings; usar `Cow`).
- Los readers hacen streaming donde el crate lo permita (calamine es lazy por hoja).
- Benchmarks con `criterion` sobre el corpus a partir de la Fase 8; regresiones > 20 % bloquean PR.

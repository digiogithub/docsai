# 02 — Estructura del proyecto

## Árbol

```
docsai/
├── Cargo.toml                  # workspace: 7 miembros + dependencias compartidas
├── .github/workflows/ci.yml    # build/test en 3 SO + fmt/clippy/rustdoc
├── crates/
│   ├── docsai-model/           # el IR. Sin I/O, sin dependencias pesadas
│   ├── docsai-docmark/         # serializador DocMark (el parser es Fase 2)
│   ├── docsai-office/          # lector .docx (xlsx Fase 3, doc Fase 5)
│   ├── docsai-odf/             # esqueleto (Fase 4)
│   ├── docsai-convert/         # orquestación: detección, pipelines, assets
│   ├── docsai-cli/             # binario `docsai`
│   └── docsai-mcp/             # esqueleto (Fase 7)
├── corpus/
│   ├── generate.py             # genera TODOS los ficheros del corpus
│   ├── README.md               # qué aísla cada documento
│   ├── docx/*.docx             # 14 documentos + 14 *.expected.dmk.md (goldens)
│   └── xlsx/*.xlsx             # 6 libros, para la Fase 3
├── docs/                       # documentación de diseño (español)
│   └── spikes/                 # informes de spikes con su decisión
└── kb/                         # esta base de conocimiento
```

## Los siete crates

| Crate | Tipo | Líneas | Responsabilidad | Depende de |
|---|---|---|---|---|
| `docsai-model` | lib | ~3 500 | El IR y los tipos comunes | *(nada del workspace)* |
| `docsai-docmark` | lib | ~1 850 | IR ⇄ DocMark | model |
| `docsai-office` | lib | ~3 500 | Lectores/escritores OOXML y legados | model |
| `docsai-odf` | lib | 21 | Lectores/escritores ODF | model |
| `docsai-convert` | lib | ~500 | Detección, pipelines, gestión de assets | los cuatro anteriores |
| `docsai-cli` | bin | ~200 | El binario `docsai` | convert, model |
| `docsai-mcp` | lib | 26 | Servidor MCP por stdio | convert |

**Regla de dependencia (`AGENTS.md` §3)**: ningún crate de formato importa a otro crate de
formato. `docsai-odf` y `docsai-mcp` existen ya, vacíos, precisamente para que el compilador
haga cumplir esa regla desde el primer día en lugar de descubrirla rota en la Fase 4.

## `docsai-model` — el IR

| Módulo | Contenido |
|---|---|
| `units` | `Length` (newtype sobre EMU), `Size`, `Point`, conversiones exactas y el formato de salida |
| `style` | `StyleCatalog`, `Style`, `FontProps`, `ParaProps`, `DocDefaults` y la resolución de la cascada |
| `list` | `ListCatalog`, `ListDef`, `ListLevel`, `NumFormat` |
| `text` | `TextDocument`, `Section`, `PageGeometry`, `Block`, `Inline`, `Paragraph`, `Table`, `RawFragment` |
| `sheet` | `Workbook`, `Sheet`, `Cell`, `CellRef` (A1), `Formula`, `NumFmt` — poblado en la Fase 3 |
| `image` | `ImageRef`, `ImageGeometry`, `Anchor` y su cortejo (wrap, crop, flip, bordes) |
| `assets` | `AssetStore` (trait), `MemoryAssetStore`, hash de contenido, sniffing y dimensiones |
| `report` | `ConversionReport`, `Warning` tipada, `Severity`, `ConversionStats` |
| `validate` | Invariantes: anclajes de hoja solo en `Workbook`, filas más anchas que la rejilla, anclajes `two-cell` invertidos |

Tres principios que los tipos codifican, y que conviene no romper:

1. **Estilo = referencia + delta.** Cada campo de `FontProps`/`ParaProps` es `Option`: `None`
   significa *heredar*, no *desactivado*. `over()` mezcla la cascada; `minus()` calcula el delta
   que hay que emitir (la «regla de economía» de la spec §3.1).
2. **Todas las longitudes en EMU.** Un solo entero, sin pérdidas, para OOXML (EMU/twips),
   ODF (cm/in) y `.doc` (twips).
3. **El IR no conoce el I/O.** Las imágenes son un `AssetId` detrás de un trait.

## `docsai-office` — el lector docx

| Módulo | Responsabilidad |
|---|---|
| `xml` | Árbol XML sobre `quick-xml` con **spans de bytes** por nodo |
| `package` | ZIP, relaciones OPC, límites anti-bomba, saneado de nombres |
| `detect` | Detección de formato **por contenido**, no por extensión |
| `docx/format` | `w:rPr` y `w:pPr` → los tipos delta del IR |
| `docx/styles` | `styles.xml` → `StyleCatalog`; nivel de encabezado |
| `docx/numbering` | `numbering.xml` → `ListCatalog` (aplana la doble indirección) |
| `docx/drawing` | DrawingML y VML → `ImageRef`/`ImageGeometry` |
| `docx/body` | El recorrido del cuerpo: bloques, inlines, campos, tablas, listas |
| `docx/mod` | Ensamblaje: propiedades, secciones, cabeceras/pies, notas al pie |

El detalle que lo sostiene todo: **cada nodo del árbol XML recuerda su rango de bytes en el
origen**. Un elemento no reconocido se conserva citando los bytes originales, no una
re-serialización, y por eso el raw-block es exacto.

## `docsai-docmark` — el serializador

| Módulo | Responsabilidad |
|---|---|
| `escape` | Tabla de escapado fija por contexto (bloque, celda de tabla, etiqueta de enlace) |
| `attrs` | Bloques `{#id .clase clave="valor"}` con el orden canónico de la spec §8 |
| `units` | Cómo se escribe una longitud, un porcentaje o un número |
| `frontmatter` | El YAML, escrito a mano para garantizar determinismo byte a byte |
| `writer` | El recorrido del IR: bloques, inlines, imágenes, tablas, contenedores |

## Flujo de una conversión

```
fichero.docx
    │  docsai-convert::read_document
    ▼
detect (por contenido) ──► docsai-office::read_docx
                                │  Package::open  → partes en memoria, nombres saneados
                                │  Element::parse → árbol XML con spans
                                │  styles + numbering + footnotes
                                │  read_sections → bloques, con DirAssetStore recibiendo medios
                                ▼
                          (Document, ConversionReport)
                                │  docsai_model::validate
                                ▼
                       docsai-docmark::serialize
                                │  front matter + cuerpo, según --fidelity
                                ▼
                    fichero.dmk.md  +  assets/img-<hash8>.<ext>
```

## Dependencias externas

Todo el núcleo se apoya en cinco crates:

| Crate | Para qué | Dónde |
|---|---|---|
| `serde` | Serialización del IR | model y derivados |
| `thiserror` | Errores tipados de biblioteca | model, docmark, office, convert |
| `quick-xml` | Parsing XML | office |
| `zip` | Contenedores OOXML | office |
| `tracing` | Logs, siempre a stderr | office, convert, cli |

Y en los binarios: `clap` (CLI), `anyhow` (errores de binario), `serde_json` (`--json`),
`tracing-subscriber`. En tests: `proptest` y `comrak`.

Deliberadamente **ausentes**: `docx-rs` (descartado por el spike R1), `image` (basta leer la
cabecera del formato), `serde_yaml` (el front matter se escribe a mano y el crate no está
mantenido) e `insta` (los goldens son ficheros de texto revisables). Cada ausencia está
justificada en [`docs/analisis-tecnico.md`](../docs/analisis-tecnico.md) §4, como exige
`AGENTS.md` §2.

## Dónde están los tests

| Ubicación | Qué cubre |
|---|---|
| Módulos `#[cfg(test)]` de cada fichero | Unidades: conversiones, escapado, cascada de estilos, parsing de fragmentos XML |
| `crates/docsai-model/tests/json_roundtrip.rs` | Round-trip JSON del IR: documentos a mano con todos los nodos + proptest |
| `crates/docsai-office/tests/docx_images.rs` | Geometría de imágenes sobre el corpus real |
| `crates/docsai-office/tests/robustness.rs` | Los 900+ inputs corruptos, path traversal, ZIP sin documento |
| `crates/docsai-convert/tests/goldens.rs` | Goldens, determinismo, finales de línea, `plain` con comrak, rendimiento |

## Convenciones que conviene respetar

- Nombres de código, comentarios y mensajes de commit en **inglés**; la documentación de `docs/`
  y `kb/` en **español** (`AGENTS.md` §5).
- `thiserror` en bibliotecas, `anyhow` sólo en binarios.
- Los parsers **nunca entran en pánico**: sin `unwrap`, `expect` ni indexación sin comprobar en
  el camino de lectura.
- El serializador es **determinista**: mismo IR ⇒ mismos bytes. Cualquier iteración sobre un
  mapa usa `BTreeMap`, nunca `HashMap`.
- Rutas con `std::path`, nunca concatenando `/`.

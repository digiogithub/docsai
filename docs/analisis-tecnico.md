# Análisis técnico

Documento de análisis previo al desarrollo de `docsai`. Cubre: (1) los formatos de entrada/salida
y su complejidad real, (2) el estado del arte en proyectos open source comparables, (3) las
variantes de Markdown extendido candidatas como formato pivote, (4) la evaluación de librerías
Rust disponibles y (5) las decisiones tomadas con sus riesgos.

Fecha del análisis: agosto de 2026.

---

## 1. Los formatos de entrada/salida

### 1.1 OOXML: `.docx` y `.xlsx` (ECMA-376 / ISO 29500)

Ambos son contenedores ZIP con XML dentro. Son los formatos con mejor documentación pública y
mejor soporte de librerías, y por eso anclan las fases 1–3 del plan.

**`.docx` (WordprocessingML)** — partes relevantes dentro del ZIP:

| Parte | Contenido | Relevancia para docsai |
|---|---|---|
| `word/document.xml` | Cuerpo: párrafos (`w:p`), runs (`w:r`), tablas (`w:tbl`) | Núcleo de la conversión |
| `word/styles.xml` | Catálogo de estilos (párrafo, carácter, tabla) + herencia `basedOn` | Debe volcarse al front matter DocMark |
| `word/numbering.xml` | Definiciones de listas (numeración, viñetas, niveles) | Crítico: las listas en OOXML no son anidadas sintácticamente, se reconstruyen desde `numId`/`ilvl` |
| `word/media/*` | Imágenes embebidas (png, jpeg, gif, **wmf/emf**) | Extraer a `assets/` |
| `word/_rels/*.rels` | Relaciones (imágenes, hipervínculos) | Resolución de referencias |
| `word/header*.xml`, `word/footer*.xml` | Cabeceras y pies | Contenedores DocMark dedicados |
| `docProps/core.xml`, `docProps/app.xml` | Propiedades (título, autor, fechas…) | Front matter |
| `word/settings.xml`, secciones `w:sectPr` | Tamaño página, márgenes, columnas | Metadatos de sección |

Puntos duros conocidos de `.docx`:
- **Herencia de formato en 4 niveles**: defaults del documento → estilo de párrafo → estilo de
  carácter → formato directo (`rPr`/`pPr`). Para una conversión fiel hay que *resolver* la cascada
  pero *almacenar* solo la referencia al estilo + los deltas directos (si se aplana todo, la
  conversión inversa produce documentos monstruosos sin estilos reutilizables).
- **Listas**: reconstrucción del árbol a partir de pares `(numId, ilvl)` planos.
- **Campos** (`w:fldSimple`, `w:instrText`): TOC, referencias cruzadas, números de página. Se
  conservan como raw-blocks en v1.
- **Imágenes WMF/EMF**: formatos vectoriales legados de Windows sin soporte de renderizado
  multiplataforma sencillo. Estrategia: extraer tal cual + advertencia; conversión opcional en fases tardías.
- **Revisiones/comentarios** (`w:ins`, `w:del`, `w:comment*`): fuera del alcance v1; se aceptan
  documentos con revisiones tomando la versión "aceptada" y emitiendo advertencia.

**`.xlsx` (SpreadsheetML)** — partes relevantes: `xl/workbook.xml`, `xl/worksheets/sheet*.xml`,
`xl/sharedStrings.xml`, `xl/styles.xml` (formatos de número, fuentes, rellenos, bordes — todo por
índices cruzados), `xl/calcChain.xml`.

Puntos duros de `.xlsx`:
- **Las celdas guardan valor cacheado + fórmula** (`<c><f>SUM(A1:A3)</f><v>42</v></c>`). DocMark
  debe conservar ambos: la fórmula para la bidireccionalidad, el valor para la legibilidad.
- **Fórmulas compartidas** (`t="shared"`) y de matriz (`t="array"`): hay que expandirlas o
  conservar su metadato de rango.
- **Formatos de número** (`numFmt`): son la diferencia entre `45123` y `15/07/2023` — las fechas
  en Excel son números de serie + formato. Conservar `numFmtId`/código de formato por celda es
  obligatorio para no corromper datos en el round-trip.
- Celdas combinadas (`mergeCells`), anchos de columna/altos de fila, paneles congelados,
  validaciones, formato condicional (los tres últimos: metadatos en v1, sin semántica).

### 1.2 ODF: `.odt` y `.ods` (ISO 26300, OASIS OpenDocument)

También ZIP+XML (`content.xml`, `styles.xml`, `meta.xml`, `settings.xml`, `Pictures/`). Modelo
conceptual muy parecido a OOXML pero con diferencias importantes:

- Los **estilos automáticos** (`office:automatic-styles`) representan el formato directo: cada
  fragmento con formato manual genera un estilo anónimo. Hay que "des-automatizar" al leer
  (mapear estilos automáticos a deltas de formato directo en el IR).
- Las fórmulas de `.ods` usan **OpenFormula** con prefijo de espacio de nombres
  (`of:=SUM([.A1:.A3])`) y sintaxis de referencia distinta (`[.A1]` vs `A1`). Para la
  bidireccionalidad OOXML⇄ODF haría falta traducir sintaxis de fórmulas; en v1 se conserva la
  fórmula en su dialecto original anotando `formula-dialect` en la celda.
- ODF está mejor especificado y es más regular que OOXML; el esfuerzo de un parser propio con
  `quick-xml` es asumible y de hecho es el plan para `.odt` (ver §4.3).

### 1.3 Formatos binarios legados: `.doc` (MS-DOC) y `.xls` (BIFF8)

- **`.xls`**: resuelto — `calamine` lo lee de forma nativa (valores y fórmulas). Solo lectura.
- **`.doc`**: es el mayor riesgo técnico del proyecto. Es un formato binario sobre contenedor
  OLE2/CFB con estructuras internas complejas (piece table, FIB, FKPs…). **No existe ningún
  crate Rust maduro que lo lea con estilos e imágenes.** Opciones evaluadas:

| Opción | Esfuerzo | Fidelidad | Dependencias |
|---|---|---|---|
| Parser propio sobre crate `cfb` (spec MS-DOC) | Muy alto (meses) | Alta | Ninguna externa |
| Fallback a LibreOffice headless (`soffice --headless --convert-to docx`) y reusar pipeline docx | Bajo | Muy alta | LibreOffice instalado (opcional, detectado en runtime) |
| `antiword`/`wvWare` como proceso externo | Bajo | Baja (pierde estilos) | Binario externo |
| Extracción de texto plano propia (piece table only) | Medio | Solo texto | Ninguna |

  **Decisión**: estrategia en dos niveles. (a) Fallback LibreOffice headless si está instalado —
  fidelidad máxima con esfuerzo mínimo; (b) extractor nativo de texto+estructura básica sobre
  `cfb` como modo degradado sin dependencias. El parser MS-DOC completo no se aborda salvo que
  la demanda real lo justifique. Esto mantiene el principio "binario único sin runtime externo
  obligatorio": LibreOffice mejora `.doc` pero nunca es requisito.

---

## 2. Estado del arte open source (qué aprender y qué reutilizar)

| Proyecto | Lenguaje | Qué hace | Lecciones para docsai |
|---|---|---|---|
| **Pandoc** | Haskell | Conversión universal vía AST pivote; su Markdown extendido (atributos `{...}`, divs `:::`, front matter) es el más expresivo del mercado | El patrón arquitectónico completo: readers → AST → writers. Su sintaxis de atributos es la base de DocMark. Limitación conocida: fidelidad media-baja en docx complejos (estilos custom, cajas de texto) y soporte xlsx inexistente |
| **MarkItDown** (Microsoft) | Python | Office/PDF/HTML → Markdown "LLM-ready", unidireccional | Valida la demanda del caso de uso MCP/LLM; su pérdida total de estilos es exactamente el hueco que docsai cubre |
| **Docling** (IBM) | Python | Documentos → Markdown/JSON con modelo propio `DoclingDocument` | Confirma la necesidad de un modelo de documento rico como pivote y de exportar a la vez MD legible + metadatos estructurados |
| **mammoth** (.js/Python) | JS/Python | docx → HTML semántico mediante **mapa de estilos configurable** (`Heading1 ⇒ h1`) | El concepto de style-map configurable por el usuario se adopta en docsai (`--style-map`) |
| **html2md / turndown, marker, unoconv/unoserver** | varios | Conversores parciales | unoserver documenta el patrón de fallback LibreOffice headless |
| **rdocx** | Rust | docx read/write + render a PDF/HTML/MD (crate reciente, 2026) | A vigilar como alternativa; demasiado joven para anclar el proyecto hoy |

**Conclusión del estado del arte**: nadie combina hoy (1) binario nativo sin runtime,
(2) bidireccionalidad con estilos, (3) hojas de cálculo con fórmulas y (4) servidor MCP.
Pandoc es el techo de referencia en texto; MarkItDown/Docling en integración LLM. docsai se
posiciona en la intersección vacía.

---

## 3. El formato pivote: variantes de Markdown extendido evaluadas

Requisitos: legible por humanos y visores estándar, atributos arbitrarios en inline y bloque,
metadatos de documento, extensible sin romper parsers, y con ecosistema.

| Candidato | Atributos | Ecosistema | Veredicto |
|---|---|---|---|
| **CommonMark + GFM puro** | ❌ No tiene | Enorme | Insuficiente: sin atributos no hay estilos |
| **Pandoc Markdown** (atributos + fenced divs + spans + YAML) | ✅ Completo | Grande (pandoc lo consume) | **Base elegida.** Sintaxis probada durante una década para exactamente este problema |
| **MyST Markdown** | ✅ (directivas/roles) | Científico/Sphinx | Directivas más verbosas; orientado a publicación, no a round-trip |
| **Djot** (Jyrki/MacFarlane) | ✅ nativo | Pequeño | Técnicamente superior pero rompe la compatibilidad "se ve bien en GitHub" |
| **MDX** | JSX | Web/React | Descartado: no es Markdown legible para no programadores |

**Decisión**: **DocMark = CommonMark + GFM (tablas, tachado, task lists) + subconjunto de
extensiones Pandoc** (atributos `{...}` en encabezados/imágenes/spans/código, fenced divs
`:::`, front matter YAML) **+ extensiones propias para hojas de cálculo** (metadatos de celda)
documentadas en `especificacion-docmark.md`. Beneficio adicional: un fichero DocMark es
procesable por Pandoc directamente con degradación aceptable, lo que da interoperabilidad
gratuita con todo el ecosistema Pandoc (PDF vía LaTeX, HTML, EPUB…).

---

## 4. Evaluación de librerías Rust

### 4.1 Lectura/escritura de documentos

| Crate | Rol propuesto | Estado (2026) | Notas de la evaluación |
|---|---|---|---|
| **`calamine`** | Lectura `.xls`, `.xlsx`, `.xlsb`, `.ods` (valores **y fórmulas**) | Maduro, mantenido, muy usado | Lectura perezosa y rápida; lee fórmulas vía `worksheet_formula()`. **No lee estilos/formatos de número con detalle suficiente** → se complementa con lectura propia de `xl/styles.xml` |
| **`umya-spreadsheet`** | Lectura+escritura `.xlsx` con estilos | Mantenido | Único crate que lee Y escribe xlsx con estilos; parsea todo el workbook en memoria (coste en ficheros grandes; existe `lazy_read`). Candidato principal para la **escritura** xlsx |
| **`rust_xlsxwriter`** | Escritura `.xlsx` (alternativa) | Muy mantenido (port de XlsxWriter) | Excelente API de escritura con fórmulas y formatos, pero solo escritura y no permite "editar" — válido porque docsai regenera desde IR. Decidir vs umya en spike de Fase 3 |
| **`docx-rs` (bokuweb)** | Lectura+escritura `.docx` | El más usado (1M+ descargas) | JSON-friendly, escribe bien; la lectura no expone el 100 % de styles.xml/numbering.xml → complementar con `quick-xml` propio donde falte |
| **`docx-rust`** | Alternativa lectura `.docx` | Menor actividad | Mapeo XML más directo; mantener como referencia |
| **`spreadsheet-ods`** | Lectura+escritura `.ods` | Mantenido | Cubre estilos y fórmulas ODS; evita escribir un writer ODF-spreadsheet propio |
| **(ninguno)** | `.odt` | — | No hay crate maduro para ODT con estilos: **parser/writer propio** sobre `zip` + `quick-xml` (ODF es regular; esfuerzo acotado) |
| **`cfb`** | Contenedor OLE2 para `.doc`/`.xls` legados | Estable | Base del extractor degradado de `.doc` |

### 4.2 Markdown (ruta inversa)

| Crate | Rol | Notas |
|---|---|---|
| **`comrak`** | Parser Markdown del pipeline DocMark→IR | CommonMark+GFM completo, mantiene posiciones, soporta front matter, tiene extensión de atributos limitada; los atributos `{...}` completos y los fenced divs `:::` se procesan en una pasada propia sobre su AST (o pre-lexer) |
| `markdown-rs` / `pulldown-cmark` | Alternativas | pulldown-cmark es más rápido pero orientado a eventos (incómodo para transformación); markdown-rs tiene AST mdast agradable pero menos extensiones nativas |

**Decisión**: `comrak` + capa propia de atributos/divs. El **serializador** DocMark (IR→MD) se
escribe a mano (no se delega en comrak) para controlar byte a byte la salida y garantizar la
idempotencia del round-trip.

### 4.3 Infraestructura

| Crate | Rol |
|---|---|
| `zip` | Contenedores OOXML/ODF |
| `quick-xml` (+ `serde`) | Parsing XML de alto rendimiento donde los crates de formato no llegan |
| `serde` / `serde_yaml` / `serde_json` | Front matter, `inspect --json`, config |
| `clap` (derive) | CLI |
| `rmcp` (SDK oficial de MCP, transporte stdio) | Servidor MCP; macros `#[tool]`; implementa spec 2026-07-28 con compatibilidad hacia atrás |
| `image` | Detección de dimensiones/re-codificación de imágenes; sin soporte WMF/EMF (limitación aceptada) |
| `thiserror` / `anyhow` | Errores |
| `tracing` + `tracing-subscriber` | Logs (siempre a stderr) |
| `tokio` | Solo en `docsai-mcp` (rmcp lo requiere); el núcleo de conversión es síncrono |
| `insta` | Snapshot testing de golden files |
| `cargo-fuzz` | Fuzzing de parsers (Fase 8) |
| `cargo-dist` | Empaquetado de releases multiplataforma |

---

## 5. Decisiones de arquitectura derivadas (resumen)

1. **IR pivote obligatorio** (`docsai-model`): árbol de documento con dos raíces posibles
   (`TextDocument`, `Workbook`) — detalle en `arquitectura.md`. N formatos → 2N conversores en
   lugar de N².
2. **Estilos por referencia + delta**: el IR guarda `style_id` + propiedades directas, y el
   catálogo de estilos viaja completo en el front matter. Así el round-trip reconstruye
   `styles.xml` real y el Markdown sigue limpio.
3. **Assets externos con manifiesto**: imágenes a `assets/` con nombre determinista
   (hash de contenido) para que el round-trip no duplique medios.
4. **`ConversionReport` estructurado**: toda conversión devuelve documento + lista de
   advertencias tipadas (elemento no soportado, degradación, raw-block emitido). La CLI lo
   muestra; el MCP lo devuelve en la respuesta de la tool.
5. **Fallbacks externos opcionales**: LibreOffice headless solo para `.doc`, detectado en
   runtime (`--use-loffice=auto|never|require`).

## 6. Riesgos principales y mitigaciones

| # | Riesgo | Prob. | Impacto | Mitigación |
|---|---|---|---|---|
| R1 | La cascada de estilos OOXML resulta más costosa de lo previsto y retrasa Fase 1 | Alta | Alto | Spike de 1 semana en Fase 0 con documentos reales; limitar v1 a la resolución de 4 niveles sin `tblStyle` condicional |
| R2 | Ningún crate xlsx cubre lectura de estilos con suficiente detalle | Media | Medio | Ya asumido: lectura complementaria propia de `xl/styles.xml` con quick-xml (esfuerzo acotado, formato documentado) |
| R3 | Round-trip no idempotente por ambigüedades de Markdown (escapes, espacios) | Media | Alto | Serializador propio con reglas deterministas + test de idempotencia en CI desde Fase 2 |
| R4 | `.doc` sin LibreOffice decepciona a usuarios | Media | Bajo | Mensajes claros de modo degradado; documentar en README |
| R5 | Divergencia de dialectos de fórmula OOXML/OpenFormula | Alta | Medio | v1 conserva dialecto original + campo `formula-dialect`; traducción automática pospuesta (Fase 9/backlog) |
| R6 | Crates de terceros abandonados a mitad de proyecto | Baja | Medio | Los cuatro crates críticos son de los más usados del ecosistema; el diseño por IR permite sustituir un reader sin tocar el resto |
| R7 | Tamaño del binario crece descontrolado | Baja | Bajo | `cargo bloat` en CI, features opcionales, LTO en release |

## 7. Fuentes consultadas

- [Pandoc User's Guide](https://pandoc.org/MANUAL.html) — sintaxis de atributos, divs y front matter
- [calamine (GitHub)](https://github.com/tafia/calamine) y [docs.rs/calamine](https://docs.rs/calamine)
- [umya-spreadsheet (GitHub)](https://github.com/mathnya/umya-spreadsheet) y comparativa [calamine vs umya-spreadsheet](https://umaranis.com/2026/05/04/reading-excel-files-in-rust-calamine-vs-umya-spreadsheet/)
- [docx-rs (crates.io)](https://crates.io/crates/docx-rs) · [docx-rust (crates.io)](https://crates.io/crates/docx-rust) · [rdocx (lib.rs)](https://lib.rs/crates/rdocx)
- [SDK oficial Rust de MCP — rmcp](https://github.com/modelcontextprotocol/rust-sdk) y [docs.rs/rmcp](https://docs.rs/rmcp)
- Comparativas de conversores: [MarkItDown vs Pandoc](https://www.file2markdown.ai/blog/markitdown-vs-pandoc), [Docling vs MarkItDown](https://www.file2markdown.ai/blog/docling-vs-markitdown), [Real Python sobre MarkItDown](https://realpython.com/python-markitdown/)
- ECMA-376 (OOXML), ISO/IEC 26300 (ODF), especificaciones [MS-DOC]/[MS-XLS] de Microsoft Open Specifications

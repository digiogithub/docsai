# Plan de desarrollo

Plan por fases para implementar `docsai`. Cada fase tiene objetivo, tareas, entregables,
criterios de aceptación y una estimación orientativa (en semanas-persona de un desarrollador
Rust senior; ajustar según equipo). Las fases 1–3 son el corazón del producto; a partir de la
Fase 6 hay margen para paralelizar.

**Regla de gestión**: no se abre una fase sin cerrar los criterios de aceptación de la anterior
(excepto las marcadas como paralelizables). Cada fase termina con un tag `v0.x.0`.

---

## Fase 0 — Fundamentos (2–3 semanas)

**Objetivo**: workspace compilando, IR diseñado, especificación DocMark congelada, corpus inicial
y CI multiplataforma en verde. Es la fase que des-riesga todo lo demás.

Tareas:
1. Crear workspace Cargo con los 7 crates de `arquitectura.md` §2 (aunque casi vacíos).
2. Implementar `docsai-model` v1: tipos del IR (§3 de arquitectura), `StyleCatalog` con
   herencia, `ConversionReport`, newtypes de unidades (`Length` con EMU/twips/pt/cm).
   Todo `serde`-serializable, con tests unitarios de las conversiones de unidades.
3. **Spike de riesgo R1** (timebox 1 semana): leer con `docx-rs` + `quick-xml` tres documentos
   docx reales con estilos custom y verificar que la cascada de 4 niveles es resoluble con la
   información expuesta. Resultado escrito en `docs/spikes/` con decisión: crate + complemento
   propio, o parser OOXML propio completo.
4. Congelar `especificacion-docmark.md` v1.0 (revisión del equipo, resolver los TODO del anexo A
   de tablas complejas).
5. Corpus inicial en `corpus/`: ~15 documentos mínimos hechos a mano (uno por rasgo):
   `docx/basic-text`, `basic-styles`, `nested-lists`, `table-simple`, `table-merged`,
   `images-inline`, `images-floating`, `headers-footers`, `footnotes`, `custom-styles`;
   `xlsx/values-types`, `formulas-basic`, `formulas-shared`, `number-formats`, `merged-cells`.
   Guion de cómo se creó cada uno en `corpus/README.md`.
6. CI GitHub Actions: build+test+clippy+fmt en matriz de 3 SO; caché de cargo; badge en README.
7. Esqueleto CLI (`docsai formats`, `--version`) y plantilla de `insta` para golden tests.

Entregables: workspace verde en CI, `docsai-model` v1, spec congelada, corpus v1, informe del spike.

Criterios de aceptación:
- [ ] `cargo test --workspace` verde en las 3 plataformas.
- [ ] IR serializa/deserializa a JSON con round-trip idéntico (proptest básico).
- [ ] Spike documentado con decisión firmada sobre la estrategia docx.
- [ ] DocMark v1.0 sin TODOs abiertos.

---

## Fase 1 — Lectura DOCX → DocMark (4–6 semanas)

**Objetivo**: `docsai convert x.docx -o x.dmk.md` con estilos, imágenes, tablas, listas,
cabeceras/pies y propiedades. Es la fase más larga: fija los patrones que reutilizan todas las demás.

Tareas:
1. Reader docx (`docsai-office`): apertura ZIP, `document.xml`, resolución de relaciones.
2. Estilos: parseo completo de `styles.xml` → `StyleCatalog`; resolución referencia+delta
   (nunca aplanar); defaults del documento (`docDefaults`).
3. Párrafos y runs: todo el formato inline de la tabla §3.2 de la spec; hipervínculos; breaks.
4. Listas: reconstrucción del árbol desde `numbering.xml` + pares `(numId, ilvl)` → `ListCatalog`.
5. Tablas: grid, `gridSpan`/`vMerge` → rowspan/colspan del IR, anchos, estilo de tabla.
6. Imágenes: DrawingML inline y flotante → `ImageRef` + extracción a `AssetStore` con nombre por
   hash; dimensiones y anclaje; WMF/EMF extraídos tal cual con advertencia.
7. Cabeceras/pies/secciones (`sectPr`), notas al pie/al final, campos simples (PAGE, TOC como
   raw/field), propiedades de documento (core+app+custom).
8. Serializador DocMark (`docsai-docmark`): IR → Markdown según spec §8 (determinista); gestión
   de assets y front matter; modos `--fidelity`.
9. Todo elemento OOXML no reconocido → raw-block + advertencia tipada (cobertura medible).
10. Golden tests de todo el corpus docx; añadir 5+ documentos "del mundo real" anonimizados.

Criterios de aceptación:
- [ ] Los 10 golden docx del corpus pasan.
- [ ] Cero pánicos con corpus corrupto sintético (ZIP truncado, XML malformado): siempre `Err`.
- [ ] Un docx real de 50+ páginas convierte en < 1 s con < 10 raw-blocks.
- [ ] La salida en `--fidelity plain` es CommonMark limpio verificado con comrak.

---

## Fase 2 — Escritura DocMark → DOCX + round-trip (3–5 semanas)

**Objetivo**: cerrar el ciclo bidireccional de texto y montar la infraestructura de fidelidad.

Tareas:
1. Parser DocMark (`docsai-docmark`): comrak + capa propia de atributos `{...}` y fenced divs
   `:::` → IR. Validación de front matter con errores de línea/columna útiles.
2. Writer docx: IR → `document.xml` + `styles.xml` + `numbering.xml` + media + props
   (con `docx-rs` donde llegue; XML directo donde no). Re-inyección de raw-blocks `format=ooxml`.
3. Comando `roundtrip`: docx→md→docx→md; diff estructural de IR normalizado; métrica de
   fidelidad por categoría (texto, estilos, tablas, imágenes, listas) y salida `--json`.
4. Test de **idempotencia del serializador** en CI: `serialize(parse(md)) == md` byte a byte
   sobre todos los goldens.
5. Property testing (proptest): generar IRs aleatorios válidos y verificar IR→md→IR == identidad.
6. Validación externa: los docx generados abren sin diálogo de reparación en Word y LibreOffice
   (checklist manual documentada por release; automatizable después vía LibreOffice headless en CI Linux).

Criterios de aceptación:
- [ ] Round-trip idempotente (2ª pasada == 1ª pasada) en todo el corpus.
- [ ] Métrica de fidelidad ≥ 95 % en texto/estilos/tablas/listas del corpus.
- [ ] docx generados abren limpios en Word y LibreOffice (checklist).
- [ ] Editar un `.dmk.md` a mano (añadir párrafo con estilo existente) y regenerar docx funciona.

---

## Fase 3 — Hojas de cálculo: XLSX/XLS ⇄ DocMark (4–5 semanas)

**Objetivo**: `xlsx` bidireccional con valores, fórmulas y formatos; `xls` lectura.

Tareas:
1. **Spike (3 días)**: decidir writer xlsx — `umya-spreadsheet` (lee y escribe estilos) vs
   `rust_xlsxwriter` (mejor API de escritura pura). Criterio: cuál regenera con mayor fidelidad
   estilos+numFmt desde el IR. Documentar en `docs/spikes/`.
2. Reader xlsx: `calamine` para valores y fórmulas; lectura complementaria propia de
   `xl/styles.xml` (numFmt, fuentes, rellenos, bordes por índice) y de dimensiones/merges/panes
   con `quick-xml`. Fórmulas compartidas y de matriz expandidas con metadato.
3. Tipado de celdas: detección fecha/hora vía numFmt (serial→ISO-8601 y vuelta); booleanos,
   errores (`#DIV/0!`…).
4. Serialización de hojas a DocMark según spec §4 (tabla de valores + `cell-meta` con rangos
   compactados) y parser inverso.
5. Writer xlsx desde IR (crate elegido en el spike): valores, fórmulas (recálculo delegado a
   Excel/LibreOffice al abrir: escribir fórmula sin valor cacheado o con el valor conservado),
   numFmt, estilos, merges, anchos, defined names.
6. Reader xls (calamine) → mismo pipeline (solo lectura; documentar en `formats`).
7. Corpus: añadir libros con fechas, porcentajes, monedas, fórmulas entre hojas, nombres
   definidos, 100k celdas (rendimiento).

Criterios de aceptación:
- [ ] Round-trip xlsx: valores, fórmulas y numFmt intactos en el corpus (fidelidad ≥ 95 %).
- [ ] Un xlsx con fechas sobrevive al round-trip sin corromper serials (test dedicado).
- [ ] xlsx de 100k celdas: < 3 s, < 500 MB RAM.
- [ ] Excel y LibreOffice recalculan sin errores los ficheros generados (checklist).

---

## Fase 4 — ODF: ODT y ODS ⇄ DocMark (3–4 semanas) — *paralelizable con Fase 5*

**Objetivo**: paridad de LibreOffice con los formatos OOXML ya soportados.

Tareas:
1. Reader/writer ODT propio (`docsai-odf`, `zip`+`quick-xml`): content/styles/meta;
   **des-automatización** de estilos automáticos al leer (→ deltas) y re-generación al escribir.
2. Reader ODS con `calamine` (valores/fórmulas) + estilos propios; writer ODS con
   `spreadsheet-ods` (evaluar en mini-spike; si no da la talla, writer propio).
3. Fórmulas OpenFormula: conservar dialecto (`formula-dialect=openformula`); NO traducir aún.
4. Corpus ODF espejo del OOXML (los mismos rasgos), generado con LibreOffice.
5. Nota de alcance: la conversión cruzada docx⇄odt "funciona" vía IR, pero los raw-blocks de un
   dialecto se descartan con advertencia en el otro. Documentar en README.

Criterios de aceptación:
- [ ] Round-trip odt y ods sobre corpus ODF con fidelidad ≥ 90 %.
- [ ] docx→DocMark→odt produce documento correcto para el corpus básico (rasgos comunes).

---

## Fase 5 — DOC legado (2–3 semanas) — *paralelizable con Fase 4*

**Objetivo**: lectura de `.doc` con la estrategia de dos niveles del análisis (§1.3).

Tareas:
1. Detección de LibreOffice en runtime por SO (rutas estándar + PATH); flag
   `--use-loffice auto|never|require`; conversión `soffice --headless --convert-to docx` en
   directorio temporal sandbox y reentrada por el pipeline docx de Fase 1.
2. Extractor nativo degradado: `cfb` + FIB + piece table → texto con párrafos y propiedades
   básicas; imágenes del contenedor si es viable en el timebox. Marcar salida como degradada
   en el `ConversionReport`.
3. Mensajería clara: si no hay LibreOffice, el usuario sabe exactamente qué está perdiendo y cómo
   mejorar el resultado.
4. Tests: corpus `.doc` generado guardando el corpus docx como .doc con Word/LibreOffice.

Criterios de aceptación:
- [ ] Con LibreOffice instalado: fidelidad equivalente a la ruta docx.
- [ ] Sin LibreOffice: texto completo y estructura de párrafos correcta, sin pánicos.
- [ ] `.doc` cifrados/protegidos rechazados con error claro.

---

## Fase 6 — CLI completa y distribución (2–3 semanas)

**Objetivo**: experiencia de producto: la CLI definitiva y binarios instalables en 3 SO.

Tareas:
1. CLI final según `arquitectura.md` §5: `convert`, `inspect`, `roundtrip`, `formats`;
   `--json`, `--strict`, stdin/stdout, códigos de salida; `--style-map` (modo mammoth, spec §5).
2. Mensajes de error y advertencias pulidos (con `miette` o similar para diagnósticos bonitos);
   `--verbose`/`RUST_LOG`.
3. Procesado por lotes: `docsai convert *.docx --out-dir md/` con paralelismo (`rayon`) y
   resumen agregado.
4. `cargo-dist`: releases automáticas por tag con binarios firmados para los 5 targets;
   instaladores shell/powershell; fórmulas Homebrew/Scoop; publicación en crates.io.
5. Documentación de usuario: README definitivo con ejemplos reales, página `--help` cuidada,
   CHANGELOG (keep-a-changelog).

Criterios de aceptación:
- [ ] Instalación en máquina limpia de cada SO con un comando y conversión de prueba OK.
- [ ] `docsai convert` sobre carpeta con 100 documentos mezclados termina con resumen correcto.
- [ ] Todos los errores de usuario previsibles tienen mensaje accionable (revisión de UX escrita).

---

## Fase 7 — Servidor MCP (2 semanas)

**Objetivo**: `docsai mcp` operativo con clientes reales.

Tareas:
1. Implementar `docsai-mcp` con `rmcp` (stdio): las 4 tools de `arquitectura.md` §6 con schemas
   JSON documentados y validación de entrada.
2. Modo path y modo base64; límites de tamaño; timeouts; assets inline vs ficheros.
3. Garantía stdout-limpio: test automático de que ninguna ruta de código escribe en stdout fuera
   del protocolo (logs → stderr).
4. Pruebas de integración con MCP Inspector y con Claude Desktop/Claude Code reales; recetas de
   configuración en README.
5. Considerar (backlog, no bloqueo): tool `apply_edits` para edición guiada de documentos vía
   DocMark en el futuro.

Criterios de aceptación:
- [ ] Un cliente MCP real convierte docx→markdown y markdown→docx de extremo a extremo.
- [ ] Entradas malformadas devuelven errores MCP correctos, nunca cuelgan el servidor.
- [ ] Sesión de 100 conversiones seguidas sin fugas de memoria ni ficheros temporales huérfanos.

---

## Fase 8 — Endurecimiento y calidad (2–3 semanas, parcialmente continua)

**Objetivo**: robustez de producción.

Tareas:
1. Fuzzing con `cargo-fuzz` de los 4 parsers de entrada (docx, xlsx, odf, docmark); corpus de
   fuzzing sembrado con el corpus de tests; ejecutar en CI programada (cron semanal).
2. Suite de documentos adversarios: ZIP bombs (límites de descompresión), XML entity expansion
   (verificar que quick-xml no expande entidades externas), rutas de assets maliciosas
   (path traversal en nombres de media), tamaños extremos.
3. Benchmarks `criterion` + presupuesto de rendimiento en CI (regresión > 20 % bloquea).
4. `cargo audit`/`cargo deny` en CI (licencias + vulnerabilidades); `cargo bloat` informativo.
5. Ampliar corpus con documentos reales variados (informes, plantillas corporativas, hojas
   financieras) y publicar la **matriz de fidelidad** por rasgo en la documentación.
6. Revisión de seguridad: el servidor MCP nunca escribe fuera de los directorios indicados;
   normalización de rutas; sin ejecución de contenido del documento (macros ignoradas SIEMPRE —
   los `.docm/.xlsm` se leen como sus equivalentes sin macros, con advertencia).

Criterios de aceptación:
- [ ] 72 h de fuzzing acumulado sin crashes en los 4 parsers.
- [ ] Suite adversaria completa en verde.
- [ ] Matriz de fidelidad publicada y ≥ objetivos por fase.

---

## Fase 9 — v1.0 y backlog post-1.0 (1 semana + continuo)

**Cierre v1.0**: congelar CLI y formato DocMark 1.0 estable, release notes, anuncio.

Backlog priorizado post-1.0 (no comprometido):
- Traducción de dialectos de fórmula OOXML ⇄ OpenFormula (riesgo R5).
- PowerPoint (`.pptx`/`.odp`) → DocMark de solo lectura.
- Conversión WMF/EMF → PNG/SVG (crate `emf`/librería propia o fallback).
- Comentarios y control de cambios (`w:ins`/`w:del`) como extensiones DocMark (sintaxis CriticMarkup).
- Tool MCP de edición incremental (`apply_edits`).
- Modo biblioteca: publicar `docsai-convert` como crate estable para terceros + bindings WASM.
- Escritura `.doc`/`.xls` vía LibreOffice fallback si hay demanda.

---

## Resumen de calendario (orientativo, 1 desarrollador senior)

| Fase | Duración | Acumulado |
|---|---|---|
| 0 Fundamentos | 2–3 sem | 3 |
| 1 DOCX lectura | 4–6 sem | 9 |
| 2 DOCX escritura + round-trip | 3–5 sem | 14 |
| 3 XLSX/XLS | 4–5 sem | 19 |
| 4 ODF | 3–4 sem | 22* |
| 5 DOC | 2–3 sem | 22* (*paralelas 4‖5 con 2 devs) |
| 6 CLI + distribución | 2–3 sem | 25 |
| 7 MCP | 2 sem | 27 |
| 8 Endurecimiento | 2–3 sem | 30 |
| 9 v1.0 | 1 sem | ~31 sem (~7 meses; ~5–5,5 con 2 devs desde Fase 3) |

## Métricas de seguimiento del proyecto

- **Fidelidad por categoría** (comando `roundtrip` sobre corpus): objetivo ≥ 95 % OOXML, ≥ 90 % ODF.
- **Raw-blocks por documento del corpus real**: tendencia descendente por fase.
- **Cobertura de tests** de los crates de biblioteca ≥ 80 %.
- **Rendimiento**: presupuestos de `arquitectura.md` §8 en CI.

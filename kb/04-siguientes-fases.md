# 04 — Consideraciones para las siguientes fases

Lo que las fases siguientes van a encontrarse ya resuelto, lo que les espera, y las trampas que
ya se conocen.

---

## Fase 2 — Escritura DocMark → DOCX + round-trip

Es la fase inmediata. Cierra el ciclo y monta la infraestructura de fidelidad.

> **Parcialmente hecha.** El parser DocMark → IR ya está, y con él la mitad de
> la infraestructura de fidelidad. Lo que sigue abajo se escribió antes de
> empezarla; lo que se aprendió al hacerla —incluidos dieciséis defectos reales
> y las trampas que heredará el writer— está en
> [05 — Estado de la Fase 2](05-fase-2-estado.md).

### Lo que ya está hecho para ella

| | |
|---|---|
| **La spec está congelada** | v1.0, con el registro de cambios en §10. El parser tiene un contrato fijo contra el que trabajar, no un borrador móvil |
| **El serializador es determinista** | El test de idempotencia `serialize(parse(md)) == md` tiene la mitad de la ecuación ya garantizada |
| **Las longitudes no pierden** | Toda longitud escrita se relee exacta. Sin esto, la métrica de fidelidad tendría un suelo artificial |
| **El IR es completo** | `Block::TextBox`, `Workbook`, anclajes de hoja… ya existen, aunque el lector docx no los produzca todos |
| **El validador de invariantes** | Ya se invoca en cada conversión; el parser hereda esa red |
| **Los raw-blocks son exactos** | Guardan los bytes originales, así que la re-inyección `format=ooxml` es copiar y pegar |
| **Los goldens son la referencia** | 14 documentos con su DocMark esperado: el parser tiene 14 entradas conocidas y su IR esperado |

### Lo que hay que construir

1. **Parser DocMark** (`docsai-docmark`): comrak + capa propia de atributos `{...}` y fenced divs
   `:::`. Comrak ya está en el árbol como dependencia de test, así que la evaluación está hecha.
2. **Writer docx** (`docsai-office`): IR → `document.xml`, `styles.xml`, `numbering.xml`, medios y
   propiedades.
3. **Comando `roundtrip`** con diff estructural del IR normalizado y métrica por categoría.
4. **Test de idempotencia** del serializador sobre todos los goldens.
5. **Property testing**: IR aleatorio → md → IR debe ser la identidad. El generador de IRs
   arbitrarios **ya existe** en `crates/docsai-model/tests/json_roundtrip.rs` y se puede reutilizar
   casi tal cual.

### Trampas ya identificadas

- **El escapado tiene que ser reversible, no sólo correcto.** `escape()` escapa `\` primero, de
  modo que desescapar es determinista. El test `escaping_is_idempotent_in_shape` fija esa
  propiedad; el parser tiene que ser su inverso exacto.
- **Los tres modos de fidelidad no son simétricos.** Sólo `full` es reversible. `standard` y
  `plain` pierden a propósito, y el `roundtrip` sólo tiene sentido sobre `full`.
- **`[]{.empty}`, `[]{.break kind=page}` y `{.field ...}` son sintaxis propia** por encima de
  CommonMark: el parser los necesita explícitamente. Están en la spec §3.1, §3.2 y §10.
- **Un ítem de lista nunca lleva dos bloques `{...}`.** `list=` va dentro del bloque de atributos
  del primer ítem.
- **El writer docx tiene que volver a poner las celdas absorbidas.** El IR marca `covered=true` y
  `colspan`/`rowspan` en la celda que abre el área; OOXML espera `w:gridSpan` y `w:vMerge`.
- **Los cuadros de texto siguen siendo raw-block.** Si la Fase 2 quiere `::: {.textbox}` de
  verdad, hay que ampliar antes el lector — está anotado en el plan.
- **Decidir si `docx-rs` sirve como writer.** Sigue abierto. El spike R1 sólo cerró la lectura;
  el R2 cerró la del parser DocMark, y de paso confirmó que escribir a mano da un control que
  una librería generalista no.
- **Validar en Word y LibreOffice** es criterio de aceptación y no hay LibreOffice en el entorno
  de CI actual: hay que preverlo (checklist manual por release, o `soffice` headless en el runner
  Linux).

---

## Fase 3 — Hojas de cálculo (XLSX/XLS)

### Lo que ya está hecho

- **El corpus xlsx existe**: `values-types`, `formulas-basic`, `formulas-shared`,
  `number-formats`, `merged-cells` e `images-anchored` (los tres anclajes). Generado en la Fase 0
  precisamente para no tener que inventarlo con prisa.
- **El lado `Workbook` del IR está definido y probado**: `Sheet`, `Cell`, `CellValue`, `Formula`
  con su dialecto, `NumFmt`, `CellRef` con notación A1 en ambos sentidos, `ColProps`, `RowProps`,
  `Pane`, `DefinedName`. Todo con round-trip JSON verificado.
- **Los tres anclajes de hoja** están en el modelo de imágenes y el validador ya rechaza usarlos
  fuera de un `Workbook`.
- **`float_roundtrip`** está activado: los valores numéricos no se corrompen al pasar por JSON.

### Lo que hay que decidir y construir

1. **Spike del writer xlsx**: `umya-spreadsheet` contra `rust_xlsxwriter`. Criterio: cuál
   regenera con mayor fidelidad estilos + `numFmt` desde el IR. Documentar en `docs/spikes/`.
2. **Serialización de hojas a DocMark** (spec §4): la sintaxis está especificada pero **no
   implementada**. `docsai_docmark::serialize` devuelve hoy front matter y una advertencia
   `Degraded` explícita para `Document::Workbook`; ese es el punto de entrada.
3. **Lectura de `xl/drawings/drawing*.xml`** propia con `quick-xml`: ni `calamine` ni `umya`
   exponen la geometría completa. El módulo `docx/drawing.rs` es el patrón a imitar; el modelo de
   destino es el mismo.

### Trampas ya identificadas

- **Streaming.** El árbol XML en memoria es cómodo para documentos pero el presupuesto de la hoja
  de 100 k celdas es < 3 s y < 500 MB. Habrá que usar `calamine` (que es perezoso por hoja) para
  valores y reservar el árbol propio para partes pequeñas (`styles.xml`, `drawing*.xml`).
- **Las fechas son números de serie más formato.** El IR guarda `CellValue::DateTime` en ISO-8601
  para que una edición a mano del DocMark no pueda corromperlas; la conversión de ida y vuelta
  serial ⇄ ISO es responsabilidad del lector y el escritor.
- **En anclajes `two-cell` no se serializan `width`/`height`** (spec §4.1): el tamaño lo define la
  rejilla. El escritor de atributos ya lo contempla.
- **Las fórmulas conservan su dialecto**, no se traducen (riesgo R5). `FormulaDialect` ya está en
  el IR.

---

## Fase 4 — ODF (ODT y ODS)

- `docsai-odf` es un esqueleto con la constante `FORMATS`; la regla de dependencia ya la impone
  el compilador.
- El punto duro conocido es la **des-automatización de estilos**: los `office:automatic-styles`
  de ODF representan formato directo, y hay que mapearlos a deltas del IR. El modelo
  «referencia + delta» está preparado para recibirlos: `FontProps::minus()` es exactamente la
  operación inversa.
- `detect()` ya reconoce ODF por `content.xml` + `mimetype`; sólo desempata `.odt` de `.ods` por
  extensión, lo que habrá que afinar leyendo el `mimetype`.
- El modelo de imágenes tiene ya los conceptos que ODF necesita (`as-char`/`char`/`paragraph`/
  `page` → `Inline`/`Floating`, `fo:clip` → `CropRect`).

---

## Fase 5 — DOC legado

- `detect()` ya reconoce el contenedor OLE2 y desempata `.doc` de `.xls` por nombre; queda
  sustituirlo por lectura del directorio CFB.
- La estrategia de dos niveles del análisis §1.3 sigue en pie: fallback a LibreOffice headless
  y extractor nativo degradado.
- El `Warning::ImageGeometryDegraded` que necesita el extractor nativo ya existe y ya se usa
  (para VML), así que el patrón está establecido.

---

## Fases 6 a 9 — Producto, MCP y endurecimiento

- **CLI (Fase 6)**: `convert` y `formats` existen con `--fidelity`, `--assets-dir`, `--json`,
  `--strict`, `--verbose` y los códigos de salida de arquitectura §5. Faltan `inspect`,
  `roundtrip`, `--style-map`, stdin/stdout con `-`, el procesado por lotes y `cargo-dist`.
- **MCP (Fase 7)**: `docsai-mcp` declara las cuatro tools. La regla de stdout limpio ya se
  respeta en la CLI (`tracing` escribe siempre a stderr), así que el test automático de esa
  garantía tiene sentido desde ya.
- **Endurecimiento (Fase 8)**: parte del trabajo está adelantado —topes de descompresión, límite
  de profundidad XML, saneado de rutas, 900+ inputs corruptos en CI—. Lo que falta es `cargo-fuzz`
  de verdad, la suite adversaria completa, los benchmarks con `criterion` y `cargo audit`/`deny`.
  El corpus existente sirve de semilla para el fuzzing.

---

## Deuda técnica conocida

Ninguna de estas cosas bloquea nada, pero conviene tenerlas anotadas:

| Punto | Dónde | Nota |
|---|---|---|
| Documentos reales anonimizados | Fase 1, tarea 10 | El corpus sintético cubre los rasgos uno a uno, pero no las rarezas del mundo real |
| Cuadros de texto como raw-block | `docx/drawing.rs` | El tipo del IR y la sintaxis de la spec existen; falta emitir y reconstruir |
| `w:lvlOverride` no modelado | `docx/numbering.rs` | Emite `Warning::Degraded` |
| Comentarios ignorados | `docx/body.rs` | Fuera de alcance v1; emite advertencia |
| Efectos DrawingML | `docx/drawing.rs` | Se detectan y se avisa, pero aún no se vuelcan a `effects_raw` |
| Árbol XML en memoria | `office/xml.rs` | Revisar para hojas grandes en la Fase 3 |
| Hash FNV para assets | `model/assets.rs` | Suficiente hoy; revisar si el nombre pasa a ser frontera de confianza |

## Reglas que no conviene romper

1. **No adelantar fases.** El orden del plan responde a dependencias reales.
2. **No cambiar la spec DocMark para hacer pasar un test.** Está congelada en v1.0; cualquier
   cambio sube la versión del front matter y documenta la migración.
3. **Nada se degrada en silencio.** Toda pérdida es una `Warning` tipada.
4. **Los parsers nunca entran en pánico.** Hay un test que lo comprueba con 900+ entradas
   corruptas; que siga pasando.
5. **El serializador es determinista.** `BTreeMap` siempre, `HashMap` nunca, en cualquier camino
   que llegue a la salida.

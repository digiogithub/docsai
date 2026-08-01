# Corpus de pruebas

Documentos de prueba versionados. **Un rasgo por fichero**: cuando un golden falla, el fichero
que falla señala el área del lector que se ha roto.

Ningún documento contiene datos reales ni privados (`AGENTS.md` §6).

## Cómo se generan

Todos los ficheros los produce `corpus/generate.py`, sin dependencias más allá de la biblioteca
estándar de Python 3:

```bash
python3 corpus/generate.py          # regenera todo
python3 corpus/generate.py --check  # falla si el árbol está desfasado (lo ejecuta CI)
```

Que el corpus sea **generado y no dibujado a mano** es deliberado:

- El XML de cada documento vive en el generador, donde se revisa en un `git diff` normal; un
  `.docx` hecho con Word es una caja opaca en la revisión.
- Los paquetes se escriben con marca de tiempo y orden de miembros fijos, así que regenerar
  produce archivos byte a byte idénticos y el repositorio no acumula ruido binario.
- Los medios (PNG, GIF, EMF) se sintetizan en Python puro, sin Pillow, para que el generador
  funcione igual en las tres plataformas de CI.

La contrapartida: son documentos *mínimos*, no documentos de Word reales. Los documentos reales
anonimizados que pide la Fase 1 (tarea 10) y los corpus de rendimiento y adversarios de la Fase 8
se añadirán aparte; el test de rendimiento de 50 páginas sintetiza su propio documento en tiempo
de ejecución (`crates/docsai-convert/tests/goldens.rs`).

## Golden files

Cada `docx/<nombre>.docx` tiene al lado su DocMark esperado, `docx/<nombre>.expected.dmk.md`.
Los comparan los tests de `crates/docsai-convert/tests/goldens.rs`. Para actualizarlos:

```bash
DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test goldens
```

El diff resultante **se revisa a mano** antes de confirmarlo: un golden actualizado sin mirar es
un test que ha dejado de comprobar nada.

## Documentos de texto (`docx/`)

| Fichero | Rasgo que aísla |
|---|---|
| `basic-text.docx` | Párrafos, salto de línea manual, párrafo vacío y los caracteres que Markdown escapa |
| `basic-styles.docx` | Negrita, cursiva, tachado, subrayado, color, resaltado, fuente y tamaño, sub/superíndice, hipervínculo, alineación y sangrías |
| `nested-lists.docx` | `numbering.xml` con tres niveles numerados y dos de viñetas; reconstrucción del árbol desde pares `(numId, ilvl)` |
| `table-simple.docx` | Tabla regular con estilo de tabla y rejilla |
| `table-merged.docx` | `gridSpan` y `vMerge` → `colspan`/`rowspan` y celdas absorbidas |
| `images-inline.docx` | Imágenes `wp:inline`: PNG con alt/título/nombre, GIF entre texto, EMF vectorial |
| `images-floating.docx` | `wp:anchor`: offsets relativos al margen con `wrapSquare`, alineación simbólica relativa a la página con `wrapTopAndBottom`, y marca de agua con `behindDoc` |
| `images-transformed.docx` | Rotación 45°, recorte `a:srcRect` con borde `a:ln`, volteo H+V y escala ≠ 100 % |
| `images-duplicated.docx` | El mismo mapa de bits en tres partes distintas del paquete con geometrías distintas: prueba la deduplicación del `AssetStore` |
| `images-vml.docx` | `w:pict` con VML heredado (documentos convertidos desde `.doc`) |
| `headers-footers.docx` | `sectPr` con cabecera por defecto y de primera página, pie con campos `PAGE`/`NUMPAGES`, dos columnas y `titlePg` |
| `footnotes.docx` | `footnotes.xml` con dos notas, una con formato dentro |
| `custom-styles.docx` | Estilo personalizado, estilo heredado con delta directo, estilo de carácter y propiedades personalizadas del documento |
| `fields-raw.docx` | Control de contenido `w:sdt`, campo complejo `TOC` y campo simple `DATE` |

## Hojas de cálculo (`xlsx/`)

Generadas en la Fase 0 para que el corpus esté completo; las consume la **Fase 3**, que es cuando
existirá el lector de `xlsx`.

| Fichero | Rasgo que aísla |
|---|---|
| `values-types.xlsx` | Los seis tipos de celda: entero, decimal, booleano, error, fecha (serial + `numFmt`) y cadena en línea |
| `formulas-basic.xlsx` | Fórmulas con valor cacheado, referencia entre celdas y un nombre definido |
| `formulas-shared.xlsx` | Fórmulas compartidas (`t="shared"`) y de matriz (`t="array"`) |
| `number-formats.xlsx` | Moneda, fecha, porcentaje y millares por `numFmtId` |
| `merged-cells.xlsx` | `mergeCells` horizontal y vertical, y ancho de columna personalizado |
| `images-anchored.xlsx` | Los tres anclajes de hoja: `twoCellAnchor`, `oneCellAnchor` y `absoluteAnchor` |

## Añadir un documento

1. Escribe una función `docx_<rasgo>()` en `generate.py` y añádela a `GENERATORS`.
2. Regenera (`python3 corpus/generate.py`) y añade la fila a la tabla de arriba.
3. Genera su golden y **revisa el diff**.
4. Si el rasgo aún no está implementado, el golden documentará la degradación actual (un
   raw-block, por ejemplo). Eso es correcto: hace visible lo que falta.

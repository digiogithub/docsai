# Especificación DocMark v1.0-draft

**DocMark** es el perfil de Markdown extendido usado por `docsai` como formato pivote textual.
Objetivo: representar documentos de texto (docx/odt/doc) y hojas de cálculo (xlsx/xls/ods) de
forma **legible, editable a mano y con información suficiente para regenerar el documento
original con pérdida mínima**.

Estado: **borrador**. El equipo de implementación debe congelar esta especificación al final de
la Fase 0 (ver plan). Todo cambio posterior incrementa la versión declarada en el front matter.

## 0. Principios

1. **Compatibilidad descendente**: un fichero DocMark es CommonMark+GFM válido. Un visor que
   ignore atributos y contenedores muestra un documento útil.
2. **Sintaxis de atributos Pandoc**: `{#id .clase clave="valor"}` — no se inventa sintaxis nueva
   donde Pandoc ya la tiene.
3. **Todo lo no representable se conserva**, nunca se descarta: mecanismo `raw-block` (§7).
4. **Determinismo**: dos serializaciones del mismo IR producen bytes idénticos (necesario para
   golden tests e idempotencia del round-trip). Final de línea siempre `\n`, UTF-8 sin BOM.

## 1. Convenciones de fichero

- Extensión recomendada: `.dmk.md` (doble extensión: los editores lo tratan como Markdown).
- Un documento de texto ⇒ un fichero. Un libro de cálculo ⇒ un fichero con una sección H1 por hoja.
- Los medios se extraen a un directorio hermano `assets/` (configurable con `--assets-dir`).

## 2. Front matter (YAML)

Bloque YAML inicial delimitado por `---`. Campos definidos:

```yaml
---
docmark: "1.0"                  # versión de esta especificación (obligatorio)
source-format: docx             # docx | doc | odt | xlsx | xls | ods (obligatorio al convertir)
title: "Informe Anual"          # docProps / meta.xml
author: "Ana Pérez"
created: 2026-03-01T10:00:00Z
modified: 2026-07-15T09:30:00Z
language: es-ES
custom-properties:              # propiedades personalizadas del documento, tal cual
  Departamento: "Ventas"

page:                           # geometría de página / sección por defecto (documentos de texto)
  size: A4                      # o width/height explícitos
  margins: { top: 2.5cm, bottom: 2.5cm, left: 3cm, right: 3cm }
  orientation: portrait

styles:                         # catálogo de estilos del documento original (§3)
  Heading1:
    type: paragraph
    based-on: Normal
    font: { name: "Calibri Light", size: 16pt, color: "#2E74B5", bold: true }
    paragraph: { space-before: 12pt, space-after: 4pt, keep-with-next: true }
  Emphatic:
    type: character
    font: { italic: true, color: "#C00000" }

list-definitions:               # numbering.xml / estilos de lista ODF normalizados
  L1:
    levels:
      - { format: decimal, text: "%1.", indent: 0.63cm }
      - { format: lowerLetter, text: "%2)", indent: 1.27cm }
---
```

Reglas:
- Claves en kebab-case. Unidades explícitas (`pt`, `cm`, `px`, `%`); colores `#RRGGBB`.
- El bloque `styles` es un **catálogo**: el cuerpo referencia estilos por nombre mediante clases
  (`{.Heading1}`); el writer inverso lo usa para regenerar `styles.xml`/`styles.xml` ODF.
- Los campos desconocidos se conservan (los parsers no deben rechazarlos): forward-compatible.

## 3. Bloques de texto

### 3.1 Encabezados y párrafos

```markdown
# Título del capítulo {.Heading1}

Un párrafo normal (estilo por defecto, sin atributos).

Párrafo con estilo y formato directo. {.Quote align=center space-after=12pt}
```

- El nivel `#` refleja el nivel de esquema (outline level); la clase indica el estilo real.
- Atributos de párrafo van en `{...}` **al final del bloque**: `align`, `indent`,
  `space-before/after`, `line-height`, `background`, `border`, `page-break-before`, etc.
- Regla de economía: si el formato coincide exactamente con lo que define el estilo, **no** se
  emiten atributos redundantes (mantiene el Markdown limpio y el diff estable).

### 3.2 Formato inline

| Original | DocMark |
|---|---|
| negrita / cursiva / tachado | `**x**`, `*x*`, `~~x~~` (nativo GFM) |
| subrayado | `[texto]{.underline}` |
| color, resaltado, fuente, tamaño | `[texto]{color="#FF0000" highlight="yellow" font="Arial" size=14pt}` |
| estilo de carácter | `[texto]{.Emphatic}` |
| sub/superíndice | `[x]{.sub}` / `[x]{.sup}` |
| salto de línea manual | doble espacio final (hard break) |
| hipervínculo | `[texto](https://...)` — con atributos si tiene estilo: `[texto](url){.Hyperlink}` |
| nota al pie | `[^1]` + definición al final (sintaxis Pandoc/GFM footnotes) |

### 3.3 Listas

Listas Markdown estándar; la definición tipográfica vive en `list-definitions` y se referencia
en el primer ítem cuando no es la lista por defecto:

```markdown
1. Primer punto {list=L1}
2. Segundo punto
   a) Sub-punto (el marcador real lo define L1 nivel 2)
```

### 3.4 Tablas (documentos de texto)

Tablas GFM cuando son regulares. Una tabla con celdas combinadas, anchos fijos o estilo de tabla
se envuelve en un contenedor que aporta los metadatos:

```markdown
::: {.table style=TableGrid col-widths="3cm,5cm,5cm"}
| Concepto | T1 | T2 |
|---|---|---|
| Ventas {rowspan=2} | 100 | 200 |
| | 300 | 400 |
:::
```

- `rowspan`/`colspan` en la **primera celda** del área combinada; las celdas absorbidas quedan vacías.
- Si la estructura excede lo representable en GFM (tablas anidadas, celdas multipárrafo), la tabla
  completa se emite como contenedor `::: {.table complex=true}` con filas como sub-bloques
  `::: {.row}` / `::: {.cell}` (formato detallado en el anexo A del futuro doc de implementación).

### 3.5 Imágenes y objetos

Toda imagen se extrae a `assets/` y se referencia con la sintaxis estándar de imagen Markdown
más un conjunto normalizado de atributos que capturan la geometría completa del original
(tamaño, posición, anclaje, recorte, rotación…). El mismo modelo de atributos aplica en
documentos de texto y en hojas de cálculo (§4.1).

```markdown
Imagen en línea (fluye con el texto):

![Diagrama de ventas](assets/img-3f2a91.png){width=450px height=300px title="Figura 1"}

Imagen flotante con posición y ajuste de texto:

![Logo](assets/img-9c04b7.png){#img-logo width=3.5cm height=3.5cm
  anchor=floating relative-to=margin x=1.2cm y=0.5cm
  wrap=square wrap-side=right z-index=2
  rotation=0 crop="0,0,10%,0" native-size=800x800 dpi=300
  name="Logo corporativo" link="https://example.com"}
```

**Atributos de imagen normalizados** (los no aplicables se omiten; unidades explícitas):

| Atributo | Significado | Origen típico |
|---|---|---|
| `width`, `height` | Tamaño **mostrado** (obligatorios siempre) | `wp:extent`, `svg:width/height` |
| `native-size` | Dimensiones en píxeles del bitmap original (`AxB`) | cabecera del fichero |
| `dpi` | Resolución declarada, si difiere de 96 | metadatos del bitmap |
| `anchor` | `inline` (defecto) \| `floating` \| `behind` \| `front` | `wp:inline`/`wp:anchor`, `text:anchor-type` |
| `relative-to` | Referencia de posición: `page` \| `margin` \| `paragraph` \| `character` | `wp:positionH/V @relativeFrom` |
| `x`, `y` | Offsets desde la referencia (solo flotantes) | `wp:posOffset`, `svg:x/y` |
| `align-h`, `align-v` | Alineación simbólica (`left/center/right`, `top/middle/bottom`) cuando el original usa alineación en vez de offset | `wp:align` |
| `wrap` | `square` \| `tight` \| `through` \| `top-bottom` \| `none` | `wp:wrapSquare…`, `style:wrap` |
| `wrap-side` | `both` \| `left` \| `right` \| `largest` | `@wrapText` |
| `z-index` | Orden de apilamiento entre objetos flotantes | `@relativeHeight` |
| `rotation` | Grados en sentido horario | `a:xfrm @rot` |
| `flip` | `h` \| `v` \| `hv` | `a:xfrm @flipH/V` |
| `crop` | Recorte `"izq,arr,der,abj"` en % del original | `a:srcRect`, `fo:clip` |
| `border` | Borde simple `"1pt solid #000000"` (bordes complejos → raw) | `pic:spPr` |
| `name` | Nombre interno del objeto en el documento | `wp:docPr @name` |
| `title` | Título/leyenda | `wp:docPr @title` |
| `link` | Hipervínculo sobre la imagen | `a:hlinkClick` |
| `external-src` | URL original si la imagen estaba **enlazada**, no embebida | `r:link`, `xlink:href` |

Reglas:
- El texto alternativo (accesibilidad, `wp:docPr @descr` / `svg:desc`) va en el campo alt
  estándar de Markdown `![…]`, no en un atributo — así lo muestran todos los visores.
- Nombre de fichero: `img-<hash8>.<ext>` (hash del contenido → estable entre conversiones y
  con deduplicación: N apariciones del mismo bitmap comparten fichero, cada una con sus
  atributos de geometría propios).
- `width`/`height` son **siempre** obligatorios en la serialización aunque coincidan con el
  tamaño nativo: el round-trip no debe depender de releer el bitmap.
- WMF/EMF se extraen con su extensión original y geometría completa; el serializador añade
  `render=unsupported` como pista para visores (advertencia en el informe).
- Efectos de imagen sin representación (sombras, biseles, estilos 3D DrawingML) se conservan
  como raw-block asociado mediante `effects-raw=<id>` que referencia un `::: {.raw}` contiguo.
- Objetos no imagen (OLE, SmartArt, gráficos incrustados): se extraen a `assets/` y se
  referencian con `![...](assets/obj-xxx.bin){.embedded-object content-type="..."}`, más
  advertencia en el informe.

### 3.6 Secciones, cabeceras y pies, cuadros de texto

```markdown
::: {.header scope=default}
Texto de cabecera — página [n.º]{.field field=PAGE}
:::

::: {.section columns=2 page-size=A4 orientation=landscape}
… contenido de la sección …
:::

::: {.textbox x=5cm y=2cm width=6cm}
Contenido del cuadro de texto.
:::
```

Los campos dinámicos (número de página, fecha, TOC) se representan como spans `{.field field=...}`
con su último valor conocido como texto visible.

## 4. Hojas de cálculo

Cada hoja es una sección `#` con contenedor de metadatos; los datos van en tabla GFM.
**Regla de oro: la celda muestra el valor; la fórmula y el formato viajan en metadatos.**

```markdown
---
docmark: "1.0"
source-format: xlsx
workbook:
  active-sheet: Ventas
  defined-names:
    TOTAL_ANUAL: "Ventas!$D$10"
---

# Ventas {.sheet cols="A:D" col-widths="12,9,9,11" frozen="A2"}

| | A | B | C | D |
|---|---|---|---|---|
| **1** | Producto | T1 | T2 | Total |
| **2** | Widgets | 100 | 200 | 300 |
| **3** | Gadgets | 150 | 250 | 400 |

::: {.cell-meta}
- D2: formula="SUM(B2:C2)" num-fmt="#,##0"
- D3: formula="SUM(B3:C3)" num-fmt="#,##0"
- A1:D1: style=HeaderRow
- B2:D3: type=number num-fmt="#,##0"
:::
```

Reglas:
- Primera columna/fila de la tabla = coordenadas (generadas, en negrita); permiten que el bloque
  `cell-meta` use referencias A1 legibles.
- `cell-meta` admite rangos (`B2:D3`) para compactar metadatos repetidos; el parser expande.
- `formula` se guarda **sin** `=` inicial y en el dialecto original; `formula-dialect: openformula`
  se añade cuando la fuente es ODS.
- Tipos de celda: `number | text | bool | date | error` (+ `num-fmt` con el código de formato).
  Las fechas se muestran en la tabla en ISO-8601 y se restauran como serial+formato al escribir.
- Celdas combinadas: `A5:C5: merge=true` (el valor vive en la celda superior-izquierda).
- Estilos de celda (fuente, borde, relleno) se catalogan en `styles:` del front matter y se
  referencian con `style=`.
- Hojas enormes: por defecto se vuelca el rango usado completo; `--max-cells` permite truncar
  **solo en modo unidireccional** (nunca al preparar un round-trip; truncar invalida la vuelta).

### 4.1 Imágenes en hojas de cálculo

Las hojas también llevan imágenes (logos, capturas, diagramas) ancladas a la rejilla. Se
declaran en un bloque `sheet-images` al final de cada hoja, usando la misma sintaxis y los
mismos atributos de imagen de §3.5 más los atributos de anclaje propios de hoja de cálculo:

```markdown
::: {.sheet-images}
![Logo de la empresa](assets/img-9c04b7.png){anchor=two-cell
  from="B2" from-offset="12px,3px" to="D8" to-offset="0,0" move-with-cells=true size-with-cells=false}

![Firma](assets/img-11ab42.png){anchor=one-cell from="F20" from-offset="0,0" width=180px height=60px}

![Marca de agua](assets/img-77cd01.png){anchor=absolute x=5cm y=8cm width=10cm height=10cm}
:::
```

Reglas:
- `anchor` en hojas: `two-cell` (de celda a celda; la imagen se mueve/estira con la rejilla,
  según `move-with-cells`/`size-with-cells`) | `one-cell` (celda origen + tamaño fijo) |
  `absolute` (posición absoluta). Corresponden a `xdr:twoCellAnchor`/`oneCellAnchor`/
  `absoluteAnchor` de OOXML y a los anclajes celda/hoja de ODF.
- En `two-cell` **no se serializan** `width`/`height` (el tamaño lo define la rejilla); en
  `one-cell` y `absolute` son obligatorios, como en §3.5.
- `from`/`to` usan referencias A1; los offsets dentro de la celda van en `from-offset`/`to-offset`.
- El resto de propiedades (rotación, recorte, alt, hipervínculo, `native-size`…) funcionan igual
  que en §3.5.
- Gráficos (charts) nativos de la hoja no son imágenes: en v1 se conservan como raw-block con
  advertencia (backlog: exportarlos también como imagen de cortesía en modo unidireccional).

## 5. Mapeo de estilos configurable (modo "publicación")

Inspirado en mammoth: un fichero opcional `style-map.yaml` permite proyectar estilos propios a
elementos Markdown puros (`MiTítulo ⇒ h2`, `CódigoFuente ⇒ code-block`) para quien quiera salida
limpia sin metadatos. Este modo es **unidireccional por definición** y la CLI lo marca así.

## 6. Grados de fidelidad (`--fidelity`)

| Modo | Contenido | Uso |
|---|---|---|
| `full` (defecto) | Todo: atributos, catálogos, raw-blocks | Round-trip |
| `standard` | Atributos principales, sin raw-blocks ni catálogo completo | Markdown rico legible |
| `plain` | CommonMark+GFM puro, sin atributos | Consumo LLM/RAG estilo MarkItDown |

## 7. Escotilla de fidelidad: raw-blocks

Fragmentos sin representación DocMark (campos complejos, SmartArt, dibujos DrawingML, contenido
firmado…) se conservan opacos:

````markdown
::: {.raw format=ooxml part="word/document.xml" id=raw-0007}
```xml
<w:sdt>…contenido original…</w:sdt>
```
:::
````

- El writer inverso re-inyecta el fragmento tal cual si el destino coincide con `format`;
  si no, lo omite con advertencia.
- Un editor humano puede borrar un raw-block sabiendo que solo pierde ese elemento.
- El comando `docsai convert` reporta cuántos raw-blocks emitió (métrica de cobertura: el
  objetivo de cada fase es reducirlos).

## 8. Reglas de escape y determinismo del serializador

- Se escapan solo los caracteres que cambiarían el significado en CommonMark (`*_#|[]<>` según
  contexto), con tabla de decisión fija documentada en el código.
- Atributos: orden canónico (id, clases ordenadas alfabéticamente, claves ordenadas); valores
  siempre con comillas dobles salvo números/identificadores simples.
- Tablas GFM: columnas alineadas con padding fijo si la tabla < 120 cols; sin padding si excede.
- Estas reglas son **normativas**: el test de idempotencia (`parse(serialize(ir)) == ir` y
  `serialize(parse(md)) == md`) las verifica en CI.

## 9. Compatibilidad con Pandoc

DocMark en modo `full` es parseable por `pandoc -f markdown` con estas salvedades documentadas:
los bloques `cell-meta` y `raw` aparecen como divs genéricos, y los atributos no estándar se
conservan como atributos de div/span. Esto es intencional: da salida gratuita a PDF/HTML/EPUB
vía Pandoc sin que docsai tenga que implementarla.

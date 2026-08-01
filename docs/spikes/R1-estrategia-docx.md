# Spike R1 — Estrategia de lectura DOCX

**Riesgo mitigado**: R1 del análisis técnico §6 — *«La cascada de estilos OOXML resulta más
costosa de lo previsto y retrasa Fase 1»*.

**Pregunta**: ¿basta `docx-rs` (+ complemento propio con `quick-xml` donde falte) para la
Fase 1, o hace falta un parser OOXML propio completo?

**Fecha**: agosto 2026 · **Versión evaluada**: `docx-rs` 0.4.22 · **Toolchain**: rustc 1.94.1

**Decisión**: **parser propio sobre `zip` + `quick-xml`.** `docx-rs` no se usa en el camino de
lectura. Se mantiene como referencia de mapeo XML y como posible base del *writer* de Fase 2
(decisión independiente, a revisar en su momento).

---

## 1. Método

Se generó el corpus de la Fase 0 (`corpus/generate.py`) y se leyó con `docx_rs::read_docx`,
serializando el resultado a JSON para inspeccionar qué información sobrevive. Se comprobó
rasgo a rasgo contra lo que exige la especificación DocMark y el IR de `arquitectura.md` §3.

Adicionalmente se sometió al lector a 903 entradas corruptas sintéticas (truncados cada 7 bytes
y volteos de un byte cada 13 bytes sobre `images-floating.docx`) capturando *panics*.

Documentos usados: `basic-styles`, `custom-styles`, `nested-lists`, `footnotes`, `fields-raw`,
`headers-footers`, `images-inline`, `images-floating`, `images-transformed`, `images-vml`.

## 2. Resultados

### 2.1 Lo que `docx-rs` sí resuelve

| Rasgo | Estado | Nota |
|---|---|---|
| `styles.xml` con `docDefaults` | ✅ | `runPropertyDefault` y `paragraphPropertyDefault` expuestos |
| `basedOn`, `styleType`, `name` | ✅ | Suficiente para reconstruir la herencia |
| Formato de párrafo/run como **delta** | ✅ | El modelo separa `property.style` del formato directo, que es exactamente el principio «referencia + delta» |
| `numbering.xml` | ✅ | `abstractNums` + `numberings` + niveles con `format`, `lvlText`, indentación |
| Cabeceras y pies | ✅ | Resueltos y adjuntos a `sectionProperty` (`header`, `firstHeader`, `footer`) |
| `sectPr` (tamaño, márgenes, `titlePg`) | ✅ | `columns` se expone pero no el resto de `w:cols` |
| Campos complejos (`fldChar`/`instrText`) | ✅ | `instrTextString` conservado |
| Tablas con `gridSpan`/`vMerge` | ✅ | Presentes en `tableCellProperty` |

La cascada de 4 niveles **es resoluble** con lo que expone: el riesgo R1, tal como estaba
formulado (los *estilos*), no se materializa. Es el resto lo que falla.

### 2.2 Lo que `docx-rs` pierde

Medido sobre el corpus, no inferido de la documentación.

**Imágenes** — el modelo `Pic` expone `size`, `positionType`, `positionH/V`, `relativeFromH/V`,
`distT/B/L/R`, `relativeHeight` y `rot`. No expone:

| Atributo DocMark §3.5 | Origen OOXML | En `docx-rs` |
|---|---|---|
| `wrap`, `wrap-side` | `wp:wrapSquare/Tight/Through/TopAndBottom` | ❌ ausente |
| `anchor=behind` | `wp:anchor @behindDoc` | ❌ ausente |
| `crop` | `a:srcRect` | ❌ ausente |
| `flip` | `a:xfrm @flipH/@flipV` | ❌ ausente |
| `rotation` | `a:xfrm @rot` | ⚠️ campo `rot: u16` presente pero devuelve `0` para `rot="2700000"` (45°); además `u16` no puede representar 60000ᵃᵛᵒˢ de grado ni valores negativos |
| alt (`![…]`) | `wp:docPr @descr` | ❌ ausente |
| `title`, `name` | `wp:docPr @title/@name` | ❌ ausente |
| `link` | `a:hlinkClick` | ❌ ausente |
| `external-src` | `r:link` | ❌ ausente |
| `border` | `pic:spPr/a:ln` | ❌ ausente |
| bytes del medio | `word/media/*` | ❌ no se cargan al leer (`image` es «for writer only») |

**VML legado** (`w:pict`, `images-vml.docx`): se colapsa a un nodo `shape` genérico de 813 bytes
de JSON total; se pierden el `r:id` de `v:imagedata`, el `style` (posición y tamaño), el `alt`
y el `w10:wrap`. Pérdida total del objeto.

**Notas al pie** (`footnotes.docx`): `w:footnoteReference` se descarta — el run queda con
`children: []`. `word/footnotes.xml` no se expone en la API.

**Campos simples** (`fields-raw.docx`): `w:fldSimple` pierde el atributo `w:instr`; solo queda
el texto cacheado. «Fecha: 01/01/2026» deja de ser un campo `DATE`.

**Escotilla de fidelidad**: `w:sdt` se representa como `structuredDataTag` con `alias: null` y
sin el XML original. No hay ningún mecanismo genérico que conserve los bytes de un elemento
desconocido, que es justo lo que necesita el `raw-block` de la spec §7 y el criterio de
cobertura medible de la Fase 1 (tarea 9).

**Ruido en el modelo**: cada estilo leído sale con un `tableProperty` con bordes inventados
(`single/2/000000`) aunque el estilo sea de párrafo — habría que filtrarlo para no contaminar
el catálogo del front matter.

### 2.3 Robustez frente a entrada corrupta

```
903 entradas corruptas (truncados + volteos de byte)
  → Ok: 88   Err: 611   PANIC: 204
```

Un 23 % de las entradas corruptas provoca *panic*. El criterio de aceptación de la Fase 1 es
explícito: *«Cero pánicos con corpus corrupto sintético (ZIP truncado, XML malformado): siempre
`Err`»*. Envolver todo el reader en `catch_unwind` no es una mitigación aceptable (no funciona
con `panic=abort`, y `AGENTS.md` §6 exige que los parsers devuelvan `Err`, no que se recuperen).

## 3. Análisis

El complemento propio que haría falta para cerrar los huecos incluye: todo `w:drawing`
(DrawingML completo), todo `w:pict` (VML), `word/footnotes.xml`, `w:fldSimple`, la captura de
elementos desconocidos para raw-blocks y la carga de `word/media/*`. Es decir: **el grueso de
`document.xml`**. Lo que quedaría delegado en `docx-rs` es `styles.xml`, `numbering.xml` y el
árbol de párrafos/tablas — la parte más mecánica y mejor documentada del formato.

Mantener las dos rutas implica además:

- Dos parsers XML en el binario (`xml-rs` dentro de `docx-rs`, `quick-xml` en el nuestro).
- Reconciliar dos árboles distintos del mismo `document.xml` (el de `docx-rs` y el nuestro para
  drawings/campos), con el riesgo de desincronización de posiciones.
- Una dependencia de la que dependemos para lo fácil y no para lo difícil, con un riesgo de
  *panic* que hay que asumir o parchear aguas arriba.

## 4. Decisión

**Parser propio sobre `zip` + `quick-xml`** en `docsai-office`, con estas consecuencias:

1. **Sin `docx-rs` en `docsai-office`.** Se anota en `analisis-tecnico.md` §4.1 conforme a la
   regla de `AGENTS.md` §2 (no se sustituye una dependencia clave sin dejar constancia).
2. **Un solo recorrido** de `document.xml` con `quick-xml` en modo evento, que produce el IR
   directamente y captura como raw-block cualquier elemento no reconocido, con su parte y su
   ruta. Esto hace la cobertura medible desde el primer día.
3. **Sin `unwrap`/`expect`/índices sin comprobar** en el camino de lectura: todo error es un
   `ReadError` tipado. El criterio «cero pánicos» se verifica con un test de corrupción
   sintética equivalente al de este spike, ejecutado en CI desde la Fase 1.
4. **`quick-xml` con entidades externas deshabilitadas** (comportamiento por defecto: no expande
   entidades externas), lo que adelanta parte de la Fase 8.
5. El coste estimado del parser propio (≈2 semanas de las 4–6 de la Fase 1) es comparable al del
   complemento que habría que escribir de todos modos, y elimina la reconciliación de árboles.

### Riesgos que introduce esta decisión

| Riesgo | Mitigación |
|---|---|
| Más superficie propia que mantener | Corpus + golden tests desde la Fase 0; el parser cubre solo lo que el IR modela, el resto va a raw-block |
| Rasgos OOXML olvidados por desconocimiento | La captura genérica de elementos desconocidos los hace **visibles** (advertencia + raw-block) en vez de silenciosos |
| El writer de Fase 2 podría necesitar `docx-rs` | Decisión independiente y posterior; escribir es mucho más simple que leer (controlamos el XML de salida) y probablemente también se haga a mano |

## 5. Estado del riesgo R1

**Cerrado.** La cascada de estilos no es el cuello de botella; el modelo de imágenes sí lo era, y
la decisión de parser propio lo neutraliza. R8 (diversidad de modelos de imagen) queda cubierto
por la misma decisión: DrawingML y VML se leen en el mismo recorrido hacia `ImageGeometry`.

## 6. Reproducir este spike

El programa de sondeo vivió fuera del árbol (no se versiona una dependencia que hemos
descartado). Para reproducirlo:

```bash
python3 corpus/generate.py
cargo new /tmp/spike-docx && cd /tmp/spike-docx
cargo add docx-rs@0.4.22 serde_json
# leer los documentos de corpus/docx con read_docx() y serializar `docx.document` a JSON;
# para la prueba de robustez, truncar y voltear bytes del .docx y contar catch_unwind(Err)
```

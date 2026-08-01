# 01 — Resumen de las fases 0 y 1

## Qué se puede hacer hoy

```bash
docsai convert informe.docx -o informe.dmk.md   # extrae assets/ junto al .md
docsai convert informe.docx                      # a stdout
docsai convert informe.docx --fidelity plain     # CommonMark limpio para LLM/RAG
docsai convert informe.docx -o out.md --json     # informe de conversión en JSON
docsai formats                                   # matriz de soporte de este binario
```

Un `.docx` entra y sale un fichero DocMark con:

- **Propiedades** del documento (core, app y personalizadas) en el front matter.
- **Catálogo de estilos** completo, con `w:docDefaults` y la cadena `basedOn`, sin aplanar.
- **Párrafos y runs**: negrita, cursiva, tachado, subrayado (los siete estilos), color,
  resaltado, fuente, tamaño, sub/superíndice, versalitas, mayúsculas, hipervínculos, saltos de
  línea, página y columna, y tabuladores.
- **Listas** reconstruidas en árbol desde los pares planos `(numId, ilvl)`, con su definición
  tipográfica en `list-definitions`.
- **Tablas** con `gridSpan` y `vMerge` resueltos a `colspan`/`rowspan` y celdas absorbidas
  marcadas; contenedor `::: {.table complex=true}` para lo que GFM no admite.
- **Imágenes** con la geometría entera: tamaño mostrado y nativo, anclaje en línea o flotante,
  base de posición por eje, offsets o alineación simbólica, ajuste de texto y lado, orden Z,
  rotación, volteos, recorte, borde, texto alternativo, título, nombre interno e hipervínculo.
  Tanto DrawingML como **VML heredado**.
- **Secciones** partidas en cada `sectPr`, con cabeceras y pies por ámbito (defecto, primera
  página, pares).
- **Notas al pie** insertadas en el punto de su referencia.
- **Campos** simples y complejos, conservando la instrucción original.
- **Raw-blocks** con los bytes exactos de todo elemento OOXML no reconocido.

Lo que **no** se puede hacer: la conversión inversa (Fase 2), hojas de cálculo (Fase 3), ODF
(Fase 4), `.doc` (Fase 5) y el servidor MCP (Fase 7).

## Fase 0 — Fundamentos

| Tarea del plan | Estado |
|---|---|
| 1. Workspace con los 7 crates | ✅ |
| 2. `docsai-model` v1 (IR, unidades, imágenes, `AssetStore`, validador) | ✅ |
| 3. Spike de riesgo R1 | ✅ [`docs/spikes/R1-estrategia-docx.md`](../docs/spikes/R1-estrategia-docx.md) |
| 4. Congelar la spec DocMark v1.0 | ✅ anexo A resuelto, registro de cambios en §10 |
| 5. Corpus inicial | ✅ 14 docx + 6 xlsx generados por `corpus/generate.py` |
| 6. CI GitHub Actions en 3 SO | ✅ build, test, corpus al día, fmt, clippy, rustdoc |
| 7. Esqueleto de CLI | ✅ superado: la CLI ya hace `convert` completo |

**Criterios de aceptación**: los cuatro cumplidos y verificados con tests
(`cargo test --workspace` verde, round-trip JSON del IR con proptest, spike documentado con
decisión firmada, spec sin TODO).

## Fase 1 — Lectura DOCX → DocMark

| Tarea del plan | Estado |
|---|---|
| 1. Reader docx (ZIP, `document.xml`, relaciones) | ✅ |
| 2. Estilos con resolución referencia + delta | ✅ |
| 3. Párrafos y runs con todo el formato inline de la spec §3.2 | ✅ |
| 4. Listas desde `numbering.xml` | ✅ |
| 5. Tablas con spans, anchos y estilo | ✅ |
| 6. Imágenes DrawingML + VML, con deduplicación por hash | ✅ |
| 7. Secciones, cabeceras/pies, notas al pie, campos, propiedades | ✅ |
| 8. Serializador DocMark con modos de fidelidad | ✅ |
| 9. Raw-block + advertencia tipada para lo no reconocido | ✅ |
| 10. Golden tests del corpus + 5 documentos reales anonimizados | ⚠️ goldens sí; documentos reales **pendientes** |

**Criterios de aceptación**, todos verificados por tests:

- ✅ Los goldens del corpus pasan, incluidos los tres de imágenes con geometría completa.
- ✅ Cero pánicos con corpus corrupto sintético: 900+ entradas (truncados y volteos de byte)
  devuelven `Err` (`crates/docsai-office/tests/robustness.rs`).
- ✅ Documento de 50+ páginas en menos de 1 s con menos de 10 raw-blocks.
- ✅ `--fidelity plain` es CommonMark limpio, verificado parseándolo con comrak.

## Métricas

| | |
|---|---|
| Tests que pasan | 164 |
| Código de producción | ~9 500 líneas de Rust |
| Código de test | ~1 200 líneas en ficheros de integración, más los módulos `#[cfg(test)]` |
| Documentos del corpus | 14 docx + 6 xlsx, con sus 14 goldens |
| Dependencias externas del núcleo | `serde`, `thiserror`, `quick-xml`, `zip`, `tracing` |
| `unsafe` | Cero (`#![forbid(unsafe_code)]` en todos los crates de biblioteca) |

## Tres defectos que el corpus destapó

Los tres estaban en el camino de la fidelidad, y ninguno se habría visto sin pasar documentos
reales por el pipeline:

1. **Formato de longitudes con pérdida.** Un margen de Word de 1417 twips se escribía `2.499cm`
   y al releerlo daba un valor distinto: habría movido márgenes e imágenes en cada ida y vuelta.
   Ahora se elige la primera unidad que representa la longitud de forma **exacta**
   (`px` → `cm` → `pt` → `emu`), y hay un test que lo verifica para cada caso.
2. **Párrafos vacíos desaparecían en silencio.** Una línea en blanco no sobrevive a Markdown.
   En modo `full` se escriben ahora como `[]{.empty}`.
3. **Estilo de hipervínculo emitido dos veces**, dentro y fuera de la etiqueta.

Además, el propio spike destapó un error en el generador del corpus (`w:drawing` colocado fuera
de un `w:r`, que es OOXML inválido) antes de que contaminara los goldens.

## Qué quedó fuera, y por qué

| Elemento | Motivo |
|---|---|
| **Documentos reales anonimizados** (Fase 1, tarea 10) | Requiere disponer de documentos reales. No bloquea la Fase 2; el corpus sintético ya cubre los rasgos uno a uno |
| **Cuadros de texto DrawingML** (`wps:txbx`) | Viajan como raw-block. El tipo `Block::TextBox` existe en el IR y `::: {.textbox}` está en la spec, pero emitirlo sin el writer que lo reconstruya es adelantar trabajo de la Fase 2 |
| **Validación en Word y LibreOffice** | Criterio de la Fase 2 (aún no se generan `.docx`), y no hay LibreOffice en el entorno |
| **Comentarios y control de cambios** | Fuera de alcance de la v1 según el análisis §1.1. Las revisiones se toman como aceptadas y se reporta cuántas |
| **`w:lvlOverride` en listas** | No modelado en v1; se emite advertencia `Degraded` |
| **Lector xlsx** | Fase 3. Los ficheros del corpus ya existen para entonces |

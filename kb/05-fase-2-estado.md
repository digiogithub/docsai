# 05 — Estado de la Fase 2

Qué hay construido de la Fase 2, qué falta, y qué se aprendió por el camino.
**La fase no está cerrada**: falta la mitad que escribe `.docx`.

## Lo entregado

| Tarea del plan | Estado |
|---|---|
| 1. Parser DocMark → IR | ✅ completo (front matter, bloques, inlines, contenedores, raw-blocks) |
| 2. Writer docx | ❌ **no empezado** |
| 3. Comando `roundtrip` | ❌ no empezado (depende del writer) |
| 4. Test de idempotencia `serialize(parse(md)) == md` | ✅ verde y determinista sobre los 14 goldens |
| 5. Property testing IR → md → IR | ⚠️ escrito y ejecutable, **con residuo conocido** (ver abajo) |
| 6. Validación en Word/LibreOffice | ❌ depende del writer |

Lo que se puede hacer hoy que antes no:

```rust
let (documento, informe) = docsai_convert::read_docmark(Path::new("informe.dmk.md"), None)?;
```

Un `.dmk.md` vuelve al IR con sus estilos, listas, tablas, imágenes (resueltas
contra `assets/` por nombre de fichero), notas al pie, campos, cabeceras, pies,
secciones y raw-blocks. `docsai formats` ya declara DocMark como legible.

## El spike R2: por qué el parser es propio

El plan asumía «comrak + capa propia». Se midió antes de escribir nada
([`docs/spikes/R2-parser-docmark.md`](../docs/spikes/R2-parser-docmark.md)) y
el resultado fue concluyente: al pasarle la salida real del serializador,
comrak **se come el `:::` de cierre como una fila más de la tabla**, deja todos
los atributos como texto suelto en tres formas distintas, y ni siquiera
convierte `[texto]{.clase}` en un nodo. La estructura de bloques tenía que ser
nuestra de todas formas.

Decisión: parser propio, y **comrak se queda de verificador independiente** del
test `plain_is_clean_commonmark`. Si fuera también el parser, ese test sólo
comprobaría que el parser se entiende consigo mismo.

## La forma normal, y por qué hace falta

Serializar **no es inyectivo**: varios IR escriben el mismo DocMark. La regla de
economía borra el formato que el estilo ya da; los marcadores `**`/`*`/`~~`
salen de las mismas propiedades que el `[…]{…}` que los rodea; el nivel de un
encabezado viaja en el número de `#`. Así que «IR → md → IR es la identidad» es
falso tal cual, y la afirmación correcta es:

```text
parse(serialize(x))     == normalize(x)
serialize(normalize(x)) == serialize(x)
```

`docsai_docmark::normalize` aplica exactamente esos aplanamientos y está
documentada regla a regla, cada una nombrando la decisión del writer que imita.

## Los dieciséis defectos que el property testing destapó

Todos corregidos. Se listan porque cada uno era real y varios eran **pérdida
silenciosa de datos** —el tipo de fallo que un golden no ve porque el golden
también estaba mal—:

| # | Defecto | Consecuencia |
|---|---|---|
| 1 | `pt()` redondeaba a centésimas de punto | Un `space-after` de -1 EMU se escribía `0pt`: mismo error que la Fase 1 corrigió en `len()`, una función más allá |
| 2 | `escape()` suponía que cada run empieza línea | `[Text("a"), Text("#")]` y `[Text("a#")]` escribían bytes distintos |
| 3 | El escapado de `&` dependía de dónde partía el run | `&` + `A` en runs distintos salía sin escapar |
| 4 | `~` sólo se escapaba a principio de línea | `~~~A~~` no es tachado: los delimitadores se fundían con el texto |
| 5 | `find_unescaped` avanzaba por bytes | **Pánico** con `é` o `ñ` tras una barra: viola la regla 4 de `AGENTS.md` |
| 6 | Emparejamiento de énfasis ingenuo | `*a**a***` (cursiva con negrita dentro) se leía como asteriscos sueltos |
| 7 | Sin regla de flanqueo | El writer emitía `a~~[x](u)~~`, que CommonMark no lee como tachado |
| 8 | Marcadores pegados a marcadores | `**a****b**` es *un* negrita con `a****b` dentro, no dos |
| 9 | `strike=false`, `underline=none`, `caps=false`… | Se descartaban en silencio: DocMark 1.0 sólo sabe apagar negrita y cursiva |
| 10 | Salto de línea dentro de encabezado, celda o etiqueta de enlace | Partía el bloque en dos |
| 11 | Dos saltos de línea seguidos | Dejaban una línea en blanco, que termina el párrafo |
| 12 | Salto de línea al final del párrafo | Perdía el `\n` y dejaba dos espacios sueltos |
| 13 | `!` de texto delante de un enlace | `![](url)`: el enlace se convertía en imagen |
| 14 | La tabla compleja no escribía `header-row` ni `col-widths` | Se perdían al releer |
| 15 | Un `\|` a principio de línea abría una tabla | Cortaba un párrafo en dos |
| 16 | Énfasis sobre contenido en blanco | `~~ ~~` son cuatro caracteres, no un tachado |

## El residuo, y por qué está marcado `#[ignore]`

Las cuatro propiedades de `tests/roundtrip_property.rs` están **marcadas
`#[ignore]` a propósito**, no por lentas. Siguen fallando en una minoría de los
documentos generados, y los fallos son reales.

```bash
cargo test -p docsai-docmark --test roundtrip_property -- --ignored
```

Tasa medida al cerrar: `arbitrary_documents` 3/10 verde, las otras tres 9/10.
El test de idempotencia sobre el corpus —el criterio que el plan fija de
verdad— pasa de forma determinista y **no** está ignorado.

**La causa raíz no es un fallo suelto, es de diseño**: `normalize` es una
*segunda implementación a mano* de las decisiones del writer, y mantener dos
descripciones de la misma cosa en sincronía es exactamente lo que ha ido
fallando. Cada corrección de las dieciséis exigió tocar los dos lados y acertar
en el orden.

**Cómo cerrarlo** (para quien siga): quitar la duplicación en vez de parchearla.
Definir la forma normal como `parse ∘ serialize` en lugar de reimplementarla, y
quedarse con las dos propiedades que entonces no pueden desincronizarse:

```text
serialize(parse(serialize(x))) == serialize(x)          // la salida es punto fijo
parse(serialize(parse(serialize(x)))) == parse(serialize(x))   // el IR también
```

Son justo los criterios que el plan pide («2ª pasada == 1ª pasada») y no
necesitan un modelo paralelo. Los ayudantes que el writer sí necesita
—`cannot_carry_emphasis`, `collapse_emphasis`, `merge_adjacent_text`, los
caracteres de borde— son decisiones suyas y deberían vivir en `writer.rs`.

## Consideraciones para el writer docx (tarea 2)

Lo que el parser deja preparado:

- **El IR que sale del parser está en forma normal**, así que el writer docx
  recibe siempre la misma forma para el mismo documento.
- **Las imágenes ya resuelven a bytes reales**: `read_docmark` carga
  `assets/` en un `DirAssetStore` y las casa por nombre de fichero. El
  re-empaquetado a `word/media/` es copiar bytes, sin recomprimir.
- **Los raw-blocks conservan los bytes exactos** (spec §7): re-inyectar un
  `format=ooxml` es pegar la cadena.
- **`ImageGeometry` está completa** y `docx/drawing.rs` es el mapa inverso a
  imitar.

Trampas que el parser ya destapó y que el writer heredará:

- Un documento multisección **pierde márgenes por sección**: el contenedor
  `::: {.section}` sólo lleva `columns`, `page-size` y `orientation` (spec §3.6),
  y el front matter describe una sola geometría. El writer no puede inventarla.
- `ParaFormat::run_direct` (formato de la marca de párrafo) **no tiene sintaxis
  DocMark**; el serializador lo descarta. Si el writer docx lo necesita, hay que
  subir la spec a 1.1.
- Apagar tachado, subrayado, versalitas o sub/superíndice sobre un estilo
  tampoco se puede escribir (§3.2 sólo define `bold=false` e `italic=false`).
  Hoy se reporta como `Warning::Degraded`; es el **primer candidato a DocMark
  1.1**, y cambiarlo obliga a subir la versión del front matter y documentar la
  migración (`AGENTS.md` §2).

## Reglas nuevas que conviene no romper

1. **El writer y la forma normal se editan juntos.** Cada regla de
   `normalize.rs` nombra en su comentario la decisión de `writer.rs` que imita.
   Cambiar una sin la otra rompe el round-trip, y el síntoma aparece lejos.
2. **El orden importa.** Aplanar a una línea, quitar saltos que abren línea y
   fusionar runs adyacentes no conmutan: un salto convertido en espacio ya no se
   puede quitar. Los dos lados aplican las tres en el mismo orden.
3. **Antes de mirar los bordes de un run, fusiona.** Si el escapado o el
   flanqueo dependen del primer o último carácter, ese carácter tiene que ser el
   que de verdad se escribe.
4. **comrak no es el parser.** Es el verificador independiente del modo
   `plain`; en cuanto sea las dos cosas, el test deja de decir nada.

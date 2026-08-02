# Spike R2 — ¿sirve comrak como base del parser DocMark?

**Fecha**: Fase 2, antes de escribir el parser.
**Pregunta**: el plan (Fase 2, tarea 1) asume «comrak + capa propia de atributos `{...}` y fenced
divs `:::`». ¿Aguanta esa división de trabajo cuando se le da la salida real del serializador?

**Método**: pasar por `comrak 0.39` (con `table`, `strikethrough` y `footnotes` activadas) un
fragmento con los cinco constructos que el serializador emite de verdad —contenedor `:::` con
tabla dentro, párrafo con atributos, imagen con atributos, span `[x]{.clase}` y nota al pie— y
mirar el AST resultante.

## Qué devuelve comrak

```
Paragraph
  Text("::: {.table col-widths=\"200px,200px\" header-row=false style=TableGrid}")
Table
  TableRow(true)
    TableCell → Text("a")            TableCell → Text("b")
  TableRow(false)
    TableCell → Text("Ventas {rowspan=2}")   TableCell → Text("100")
  TableRow(false)
    TableCell → Text(":::")          TableCell             ← el cierre, comido por la tabla
Paragraph
  Text("Parrafo con estilo. {.Destacado align=center}")
Paragraph
  Image(url="assets/img-1.png") → Text("Logo")
  Text("{width=3cm height=2cm anchor=floating}")
Paragraph
  Text("Texto con [estilo]{.Enfatico} y ")   FootnoteReference(1)   Text(" nota.")
Paragraph
  Text("[]{.empty}")
FootnoteDefinition(1) → Paragraph → Text("La nota.")
```

## Lectura

1. **Los contenedores no sobreviven.** El `:::` de apertura queda como párrafo de texto y —lo
   grave— el `:::` de cierre **es absorbido como una fila más de la tabla**. La estructura de
   bloques que DocMark necesita se destruye antes de que la «capa propia» pueda mirarla. Para
   evitarlo hay que trocear los contenedores *antes* de llamar a comrak, es decir: la estructura
   de bloques ya es nuestra.
2. **Todos los atributos vuelven como texto suelto.** El de párrafo dentro del `Text`, el de
   imagen como nodo hermano del `Image`, el de celda dentro del `TableCell`. Cada uno exige
   recortar y reasociar nodos de texto a mano, con reglas distintas según dónde aparezca.
3. **`[estilo]{.Enfatico}` ni siquiera es un nodo.** Sin `(...)` detrás no hay `Link`, así que
   todo el formato inline de la §3.2 —estilos de carácter, color, fuente, campos, saltos,
   `[]{.empty}`— es trabajo nuestro de todas formas.

Lo que comrak sí resolvería: énfasis, destino de enlace, desescapado, troceo de celdas y
referencias a notas. Es real, pero es la parte pequeña y mecánica.

## Decisión

**Parser propio**, coherente con el serializador, que ya se escribió a mano por la misma razón:
la idempotencia byte a byte no admite las opiniones de formato de una librería. DocMark en modo
`full` es un subconjunto normativo que nosotros mismos generamos; parsearlo con una librería
generalista significa reconstruir después lo que ella ha aplanado.

**Y comrak se queda donde ya estaba y donde vale más: de verificador independiente.** El test
`plain_is_clean_commonmark` parsea la salida `--fidelity plain` con comrak. Si comrak pasara a
ser también el parser, esa verificación dejaría de ser independiente: comprobaría que nuestro
parser se entiende consigo mismo. Manteniéndolo fuera, el test sigue diciendo algo.

**Coste asumido**: el parser sólo entiende el subconjunto normativo de la spec. CommonMark que
un humano podría escribir a mano y que el serializador nunca emite —encabezados setext, viñetas
con `+`, bloques de código indentados— no se interpreta. No se descarta en silencio: una línea
que el parser no sabe leer viaja como texto del párrafo, y lo que no encaja como bloque emite
`Warning::Degraded`. Ampliar el subconjunto es aditivo y no rompe nada de lo escrito.

**Cuándo revisarlo**: si DocMark deja de ser mayoritariamente máquina-a-máquina y la edición
manual pasa a ser el caso de uso principal, conviene volver a medir: entonces la tolerancia a
CommonMark arbitrario vale más que el control byte a byte.

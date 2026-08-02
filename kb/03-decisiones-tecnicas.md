# 03 — Decisiones técnicas

Las decisiones que no se deducen del plan y que un futuro colaborador podría deshacer sin
querer. Cada una lleva su motivo y su coste.

## 1. Parser docx propio, sin `docx-rs`

**Decisión**: la lectura `.docx` se hace con `zip` + `quick-xml` propios.

**Motivo** (spike R1, medido sobre el corpus, no supuesto):

- `docx-rs` resuelve bien estilos, numeración y cabeceras/pies. Pero pierde **casi todo el modelo
  de imagen**: ajuste de texto y lado, `behindDoc`, recorte, volteos, texto alternativo, título,
  nombre del objeto, hipervínculo sobre la imagen y las imágenes enlazadas. La rotación existe
  como campo pero devuelve `0` para un `rot="2700000"`, y el tipo es `u16`, que no puede
  representar sesentamilésimas de grado ni valores negativos.
- Descarta `w:footnoteReference` y el `w:instr` de `w:fldSimple`.
- El VML (`w:pict`) se colapsa a un nodo genérico sin `r:id`, sin estilo y sin `alt`.
- No conserva elementos desconocidos, que es justo lo que el raw-block necesita.
- **204 pánicos de 903 entradas corruptas** (23 %), contra un criterio de aceptación que exige
  siempre `Err`.

Las imágenes son requisito de primera clase del proyecto y la mitad de los criterios de la
Fase 1. Lo que quedaba delegable en `docx-rs` era la parte mecánica; el complemento propio
habría sido el grueso de `document.xml` de todos modos, con dos parsers XML en el binario y dos
árboles del mismo documento que reconciliar.

**Coste asumido**: más superficie propia que mantener. Se mitiga con el corpus, los goldens y la
captura genérica de elementos desconocidos, que hace *visible* lo que falta en vez de silencioso.

**Detalle**: `docx-rs` sigue siendo candidato para el **writer** de la Fase 2. Es una decisión
independiente y aún abierta: escribir es mucho más simple que leer, porque el XML de salida lo
controlamos nosotros.

## 2. Árbol XML con spans de bytes

**Decisión**: `quick-xml` alimenta un árbol en memoria donde **cada nodo recuerda el rango de
bytes que ocupa en el origen**.

**Motivo**: un raw-block tiene que conservar los bytes originales. Re-serializar el subárbol
daría XML equivalente pero no idéntico, y la re-inyección de la Fase 2 dejaría de ser exacta. Con
el rango basta cortar la cadena de origen. Añadido: un árbol es mucho más revisable que una
máquina de estados sobre eventos, y las partes OOXML caben en memoria de sobra.

**Coste**: memoria proporcional al tamaño de la parte XML. Aceptable para documentos; habrá que
revisarlo en la Fase 3 para hojas de 100 k celdas, donde `calamine` ya hace streaming por hoja.

## 3. Coincidencia por nombre local, no por espacio de nombres

**Decisión**: los elementos se buscan por su nombre local (`p`, no `w:p`).

**Motivo**: los prefijos OOXML son convencionales pero no obligatorios, y toda la navegación del
lector es contextual (hijos de un padre conocido), así que no hay ambigüedad. Un documento que
renombre sus prefijos se lee igual.

**Excepción**: los atributos con significado distinto según el espacio de nombres —`r:embed` y
`r:link` junto a un `a:blip`— se buscan por nombre **cualificado** con `attr_qualified`.

## 4. Formato de longitudes: exactitud antes que legibilidad

**Decisión**: al escribir una longitud se elige la **primera unidad que la representa
exactamente**, en el orden `px` (96 dpi) → `cm` → `pt` → `emu`.

**Motivo**: un margen de Word de 1417 twips no es un número redondo de centímetros. Escribirlo
`2.499cm` lo mueve 155 EMU en cada ida y vuelta, y esos errores se acumulan. Ahora sale
`70.85pt`, que es exacto. `emu` es la escotilla final y siempre existe.

**Coste**: alguna longitud se lee peor (`85.05pt` en vez de «unos 3 cm»). Es el precio de que el
round-trip de la Fase 2 pueda aspirar al 95 % de fidelidad.

## 5. `Document` externamente etiquetado en serde

**Decisión**: `#[serde(rename_all = "kebab-case")]` sin `tag`, a diferencia de `Block` e
`Inline`, que sí son adyacentemente etiquetados.

**Motivo**: un enum *internamente* etiquetado hace que serde almacene el contenido en un búfer
`Content` intermedio, y ese búfer **reescribe toda clave de mapa como cadena**. Eso rompe los
mapas con clave entera de `Workbook` (`cols`, `rows`). Es un fallo que sólo aparece al
deserializar y con un mensaje poco claro (`invalid type: string "0", expected u32`).

**Relacionado**: `CellRef` implementa `Serialize`/`Deserialize` a mano como su referencia A1
(`"B2"`), para poder ser clave de mapa y de paso dejar el `inspect --json` legible.

## 6. `serde_json` con la feature `float_roundtrip`

**Decisión**: activada en el workspace.

**Motivo**: sin ella, el parser rápido de coma flotante de `serde_json` puede desplazar un valor
un ULP. Sobre celdas de hoja de cálculo eso es **corromper datos del usuario en silencio**. Se
detectó comparando IR antes y después de un round-trip JSON en el test de proptest.

## 7. Hash no criptográfico para los assets

**Decisión**: FNV-1a de 64 bits, mezclando la longitud, renderizado en 16 dígitos hex.

**Motivo**: el id sólo tiene que ser estable y libre de colisiones en la práctica para los medios
de un documento. Evita meter `sha2` en `docsai-model`, que es el crate que debe permanecer
ligero. El trait `AssetStore` permite que la capa de conversión sustituya el hash sin tocar el
modelo.

**Cuándo revisarlo**: si algún día el nombre del asset se usa como frontera de confianza. Hoy no
lo es.

## 8. Sniffing de imágenes propio, sin el crate `image`

**Decisión**: leer la cabecera de PNG, JPEG, GIF, BMP, TIFF, WebP, EMF y WMF a mano.

**Motivo**: docsai **nunca recodifica** un mapa de bits; sólo necesita nombrarlo (extensión y
content-type) y medirlo (dimensiones nativas). Eso son unas decenas de líneas contra un árbol de
dependencias considerable en el binario final.

**Detalle de seguridad**: la extensión sale del **contenido**, nunca del nombre que traiga el
documento.

## 9. Front matter YAML escrito a mano

**Decisión**: sin librería YAML.

**Motivo**: la spec exige determinismo byte a byte, el esquema es pequeño y conocido, y
`serde_yaml` está sin mantenimiento activo. Escribirlo a mano da control total sobre el orden de
las claves y el entrecomillado.

**Coste**: hay que mantener el emisor a mano. Cuando la Fase 2 escriba el parser, ambos lados
tendrán que moverse juntos; el test de idempotencia lo hará evidente si se desincronizan.

## 10. Golden files como ficheros de texto, no snapshots de `insta`

**Decisión**: `corpus/docx/<nombre>.expected.dmk.md` junto al documento.

**Motivo**: es lo que prescribe `AGENTS.md` §6, y un golden que es DocMark legible se revisa en
el diff como cualquier otro texto. Un `.snap` de `insta` añade un formato intermedio entre el
revisor y lo que el programa produce.

Actualizar es deliberado: `DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test goldens`,
y **se revisa el diff**.

## 11. Corpus generado, no dibujado a mano

**Decisión**: `corpus/generate.py` produce los 20 ficheros; CI comprueba que el árbol está al día.

**Motivo**: el XML de cada documento vive en el generador, donde se revisa; un `.docx` hecho con
Word es opaco en la revisión. Los paquetes salen con marca de tiempo y orden de miembros fijos,
así que regenerar da archivos idénticos byte a byte y el repositorio no acumula ruido binario.
Los medios se sintetizan en Python puro para que funcione igual en las tres plataformas de CI.

**Coste**: son documentos *mínimos*, no documentos de Word reales. Los reales anonimizados siguen
pendientes (Fase 1, tarea 10). La decisión valió su precio de inmediato: el spike destapó que el
generador colocaba `w:drawing` fuera de un `w:r`, que es OOXML inválido, antes de que eso
contaminara ningún golden.

## 12. `w:sdt` se aplana en lugar de conservarse opaco

**Decisión**: un control de contenido emite sus bloques hijos y una advertencia `Degraded`, en
vez de un raw-block con todo dentro.

**Motivo**: meterlo entero en un raw-block escondería texto perfectamente legible detrás de un
bloque opaco, y en modo `plain` desaparecería del todo. El texto es lo que el usuario quiere ver.

**Coste**: se pierden las propiedades del control (`w:alias`, binding de datos). Está reportado,
no es silencioso, y la Fase 2 tendrá que decidir si las necesita.

## 13. La advertencia es parte de la salida, no un adorno

Cada degradación emite una `Warning` **tipada** con severidad. La CLI las cuenta en stderr, las
detalla con `--verbose` y las serializa con `--json`; el código de salida 1 marca una conversión
que perdió algo. `--strict` sube el listón para que cuenten también las menores.

Es la regla 3 de `AGENTS.md` hecha código: **nada se degrada en silencio**.

## 14. Límites de seguridad desde el principio

Aunque el endurecimiento es la Fase 8, tres cosas eran demasiado baratas como para posponerlas:

- **Tope de descompresión**: 512 MB por paquete, 128 MB por parte.
- **Límite de profundidad XML**: 256 niveles.
- **Saneado de nombres de miembro**: un `word/media/../../evil.png` nunca llega a ser una parte
  ni un asset. Hay un test dedicado.

## 15. Parser DocMark propio, sin comrak

**Decisión**: la lectura DocMark → IR se escribe a mano, como el serializador.

**Motivo** (spike R2, medido sobre la salida real, no supuesto): al pasarle un
documento con los cinco constructos que el serializador emite, comrak deja el
`:::` de apertura como párrafo de texto y **absorbe el de cierre como una fila
más de la tabla**; devuelve los atributos como texto suelto en tres formas
distintas según dónde aparezcan; y no convierte `[texto]{.clase}` en ningún
nodo. La estructura de bloques y todo el formato inline eran nuestros de todas
formas.

**Y comrak gana con el cambio**: sigue siendo el verificador *independiente* del
modo `plain`. Si fuera también el parser, ese test comprobaría que el parser se
entiende consigo mismo.

**Coste asumido**: el parser entiende el subconjunto normativo de la spec.
CommonMark que un humano podría escribir y que el serializador nunca emite
—encabezados setext, viñetas con `+`, bloques de código indentados— no se
interpreta; viaja como texto, y lo que no encaja emite `Warning::Degraded`.

## 16. La forma normal existe porque serializar no es inyectivo

**Decisión**: `docsai_docmark::normalize` describe qué aplanamientos hace el
serializador, y el round-trip se enuncia sobre ella:
`parse(serialize(x)) == normalize(x)`.

**Motivo**: la regla de economía, los marcadores de énfasis y el número de `#`
colapsan varios IR sobre los mismos bytes. Sin nombrar ese colapso, «IR → md →
IR es la identidad» es simplemente falso y el test no se puede escribir.

**Coste, y es alto**: `normalize` es una segunda implementación a mano de las
decisiones del writer, y mantenerlas sincronizadas es lo que ha ido fallando.
Cómo quitarla está en [`kb/05-fase-2-estado.md`](05-fase-2-estado.md).

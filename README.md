# docsai

**Conversor bidireccional de documentos Office / LibreOffice ⇄ Markdown extendido, escrito en Rust.**

`docsai` es un binario único y multiplataforma (Windows, Linux, macOS) que convierte documentos ofimáticos a un perfil de Markdown extendido — **DocMark** — diseñado para conservar la máxima información posible (estilos, imágenes, propiedades, fórmulas) y permitir la **conversión inversa con pérdida mínima de formato**. Puede usarse como herramienta CLI o como **servidor MCP (Model Context Protocol) por stdio**, para integrarse con asistentes de IA como Claude.

> **Estado del proyecto: Fases 0 y 1 completadas.**
> Ya se convierte `.docx` → DocMark con estilos, listas, tablas, imágenes (con toda su
> geometría), cabeceras, pies, notas al pie, campos y propiedades. La conversión inversa
> (DocMark → `.docx`) es la Fase 2 y todavía no existe; las hojas de cálculo llegan en la
> Fase 3. Ver [`docs/plan-desarrollo.md`](docs/plan-desarrollo.md).

```bash
cargo run -p docsai-cli -- convert informe.docx -o informe.dmk.md
cargo run -p docsai-cli -- formats
```

---

## Formatos soportados (objetivo)

✅ = funciona hoy · 🕓 = planificado, con la fase que lo trae · ➖ = fuera de alcance

| Formato | Extensión | Lectura | Escritura | Notas |
|---|---|---|---|---|
| Word OOXML | `.docx` | ✅ | 🕓 Fase 2 | Estilos, imágenes, tablas, listas, cabeceras/pies, notas al pie, campos, propiedades |
| Word binario | `.doc` | 🕓 Fase 5 | ➖ | Solo lectura (vía parser nativo o fallback LibreOffice headless) |
| Excel OOXML | `.xlsx` | 🕓 Fase 3 | 🕓 Fase 3 | Valores **y fórmulas**, formatos de número, celdas combinadas, imágenes ancladas |
| Excel binario | `.xls` | 🕓 Fase 3 | ➖ | Solo lectura (calamine) |
| OpenDocument Text | `.odt` | 🕓 Fase 4 | 🕓 Fase 4 | Equivalente libre de `.docx` |
| OpenDocument Spreadsheet | `.ods` | 🕓 Fase 4 | 🕓 Fase 4 | Equivalente libre de `.xlsx` |
| Markdown extendido | `.dmk.md` | 🕓 Fase 2 | ✅ | Formato pivote **DocMark** (superconjunto de CommonMark + GFM) |

`docsai formats` imprime esta misma matriz según lo que el binario realmente sabe hacer.

La escritura de `.doc` y `.xls` (formatos binarios legados) queda fuera de alcance de forma deliberada: la ruta de salida recomendada hacia el ecosistema Microsoft es siempre OOXML (`.docx` / `.xlsx`).

## ¿Qué es DocMark?

DocMark es un perfil de Markdown extendido definido en este proyecto (ver [`docs/especificacion-docmark.md`](docs/especificacion-docmark.md)). Es **Markdown legible y editable a mano**, que se renderiza de forma razonable en GitHub o cualquier visor CommonMark, pero que añade capas de metadatos para no perder información:

- **Front matter YAML** con las propiedades del documento (título, autor, idioma…) y el **catálogo de estilos** original.
- **Atributos inline y de bloque** `{#id .clase clave="valor"}` (sintaxis compatible con Pandoc) para anclar estilos, dimensiones de imagen, propiedades de celda, etc.
- **Contenedores fenced** `::: {...}` para secciones, cuadros de texto, cabeceras y pies.
- **Tablas extendidas** con metadatos por celda (fórmulas, tipos, formatos de número, combinaciones) para hojas de cálculo.
- **Activos externos**: las imágenes se extraen a un directorio `assets/` (deduplicadas por hash de contenido) y se referencian con un modelo completo de atributos de geometría: tamaño mostrado y nativo, posición y anclaje (en línea, flotante con wrap y z-order, o anclada a celdas en hojas de cálculo), rotación, recorte, volteo, texto alternativo e hipervínculo.
- **Escotilla de fidelidad** (`raw-block`) para fragmentos sin representación Markdown posible, que se conservan de forma opaca y se restauran en la conversión inversa.

Ejemplo mínimo:

```markdown
---
docmark: "1.0"
source-format: docx
title: "Informe Anual"
styles:
  Heading1: { font: "Calibri Light", size: 16pt, color: "#2E74B5" }
---

# Informe Anual {.Heading1}

Texto con **negrita** y [color]{color="#FF0000"} personalizado.

![Diagrama de ventas](assets/img-001.png){width=450px height=300px anchor=inline}
```

## Uso (CLI)

Lo que funciona hoy:

```bash
docsai convert informe.docx -o informe.dmk.md      # extrae assets/ junto al .md
docsai convert informe.docx                         # a stdout, sin escribir medios al lado
docsai convert informe.docx --fidelity plain        # CommonMark limpio, para LLM/RAG
docsai convert informe.docx -o out.md --json        # informe de conversión en JSON
docsai formats                                      # matriz de soporte de este binario
```

Grados de fidelidad (`--fidelity`, spec §6): `full` (defecto, apto para ida y vuelta),
`standard` (Markdown rico sin catálogos ni raw-blocks) y `plain` (CommonMark+GFM puro).

Códigos de salida: `0` correcto, `1` conversión con pérdidas (`--strict` incluye las advertencias
menores), `2` error de entrada, `3` formato no soportado.

Planificado:

```bash
docsai convert informe.dmk.md -o informe.docx   # Fase 2
docsai convert ventas.xlsx -o ventas.dmk.md      # Fase 3
docsai inspect informe.docx                      # Fase 6
docsai roundtrip informe.docx                    # Fase 2
```

## Uso previsto (servidor MCP, Fase 7)

```bash
docsai mcp        # arranca el servidor MCP por stdio
```

Registro en un cliente MCP (p. ej. Claude Desktop / Claude Code):

```json
{
  "mcpServers": {
    "docsai": { "command": "docsai", "args": ["mcp"] }
  }
}
```

Tools MCP previstas: `convert_to_markdown`, `convert_from_markdown`, `inspect_document`, `list_supported_formats`. Ver detalle en [`docs/arquitectura.md`](docs/arquitectura.md).

## Documentación del proyecto

| Documento | Contenido |
|---|---|
| [`docs/analisis-tecnico.md`](docs/analisis-tecnico.md) | Análisis de formatos, librerías Rust evaluadas, proyectos open source previos (Pandoc, MarkItDown, Docling, mammoth…), decisiones y riesgos |
| [`docs/especificacion-docmark.md`](docs/especificacion-docmark.md) | Especificación del formato DocMark (Markdown extendido) v1.0-draft |
| [`docs/arquitectura.md`](docs/arquitectura.md) | Arquitectura del software: workspace de crates, modelo de documento intermedio (IR), CLI, servidor MCP |
| [`docs/plan-desarrollo.md`](docs/plan-desarrollo.md) | Plan de desarrollo detallado en 9 fases, con entregables, criterios de aceptación, estimaciones y estrategia de pruebas |
| [`AGENTS.md`](AGENTS.md) | Guía operativa para desarrolladores y agentes de IA que trabajen en este repositorio |
| [`corpus/README.md`](corpus/README.md) | El corpus de pruebas: qué aísla cada documento y cómo se regenera |
| [`docs/spikes/`](docs/spikes/) | Informes de los spikes de riesgo, con la decisión que cerró cada uno |
| [`kb/`](kb/) | Base de conocimiento: qué hay construido, cómo está estructurado, las decisiones técnicas y lo que espera a las fases siguientes |

## Desarrollo

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
python3 corpus/generate.py --check     # el corpus es generado, no dibujado a mano
```

Los golden files viven junto al corpus (`corpus/docx/*.expected.dmk.md`). Para actualizarlos:
`DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test goldens`, y **se revisa el diff**.

## Principios de diseño

1. **Binario único, sin runtime externo**: pura biblioteca Rust siempre que sea posible; los fallbacks externos (LibreOffice headless para `.doc`) son opcionales y detectados en tiempo de ejecución, nunca requeridos.
2. **IR pivote**: todos los formatos convergen en un modelo de documento intermedio (inspirado en el AST de Pandoc y en DoclingDocument); los conversores nunca se hablan entre sí directamente.
3. **Fidelidad medible**: la pérdida de formato no se estima, se mide — el comando `roundtrip` y la suite de tests de ida y vuelta forman parte del producto.
4. **Markdown primero legible, después completo**: los metadatos extendidos degradan con elegancia; un visor Markdown normal muestra un documento útil aunque ignore los atributos.
5. **Los datos del usuario nunca se pierden en silencio**: lo que no se pueda representar se conserva en bloques raw o se reporta como advertencia explícita.

## Licencia

Ver [LICENSE](LICENSE).

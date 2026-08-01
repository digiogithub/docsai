# AGENTS.md — Guía para desarrolladores y agentes de IA

Este fichero es la referencia operativa para cualquier persona o agente de IA que trabaje
en el repositorio `docsai`. Léelo completo antes de tocar código.

## 1. Qué es este proyecto

`docsai` es un binario Rust multiplataforma (Windows/Linux/macOS) que convierte documentos
Office (`.doc`, `.docx`, `.xls`, `.xlsx`) y LibreOffice (`.odt`, `.ods`) a un Markdown
extendido llamado **DocMark**, y de vuelta, con pérdida mínima de formato. Se invoca como
CLI o como servidor **MCP por stdio**.

**Estado actual**: **Fases 0 y 1 cerradas**. Existe el workspace con los siete crates, el IR
(`docsai-model`), el lector `.docx` (`docsai-office`), el serializador DocMark
(`docsai-docmark`), la orquestación (`docsai-convert`) y la CLI con `convert` y `formats`.
`docsai-odf` y `docsai-mcp` son esqueletos que sólo fijan las reglas de dependencia.

Lo siguiente es la **Fase 2**: parser DocMark, writer `.docx` y el comando `roundtrip`.

## 2. Documentos que debes leer antes de implementar

Orden de lectura obligatorio:

1. `docs/plan-desarrollo.md` — el plan por fases. **Identifica en qué fase está el proyecto
   antes de escribir nada.** No implementes elementos de fases futuras.
2. `docs/arquitectura.md` — estructura del workspace, modelo IR, contratos entre crates.
3. `docs/especificacion-docmark.md` — el formato Markdown extendido. Es un contrato:
   cualquier cambio requiere subir la versión del campo `docmark` del front matter y
   documentar la migración.
4. `docs/analisis-tecnico.md` — por qué se eligió cada librería. No sustituyas una
   dependencia clave (calamine, docx-rs, rmcp, comrak…) sin dejar constancia escrita del
   motivo en ese documento.

## 3. Estructura del repositorio (objetivo)

```
docsai/
├── Cargo.toml                # workspace raíz
├── crates/
│   ├── docsai-model/         # IR: modelo de documento intermedio (sin I/O, sin deps pesadas)
│   ├── docsai-docmark/       # serializador + parser de DocMark (IR ⇄ Markdown extendido)
│   ├── docsai-office/        # lectores/escritores OOXML: docx, xlsx (+ xls, doc lectura)
│   ├── docsai-odf/           # lectores/escritores ODF: odt, ods
│   ├── docsai-convert/       # orquestación: pipelines, detección de formato, assets, informes de fidelidad
│   ├── docsai-cli/           # binario CLI (clap)
│   └── docsai-mcp/           # servidor MCP stdio (rmcp)
├── docs/                     # documentación de diseño (este conjunto)
├── corpus/                   # documentos de prueba versionados (ver §6)
└── tests/                    # tests de integración y round-trip
```

Reglas de dependencia entre crates (violarlas es un error de arquitectura):

- `docsai-model` no depende de ningún otro crate del workspace.
- `docsai-docmark`, `docsai-office`, `docsai-odf` dependen **solo** de `docsai-model`.
- `docsai-convert` depende de los cuatro anteriores. `docsai-cli` y `docsai-mcp`
  dependen solo de `docsai-convert` (y `docsai-model` para tipos).
- Ningún crate de formato importa a otro crate de formato.

## 4. Comandos de trabajo

Cuando exista el workspace, estos son los comandos canónicos (deben mantenerse verdes):

```bash
cargo build --workspace
cargo test --workspace                 # unit + integración + round-trip
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run -p docsai-cli -- convert corpus/docx/basic-styles.docx -o /tmp/out.dmk.md
python3 corpus/generate.py --check     # el corpus es generado; CI comprueba que está al día
```

Actualizar un golden es un acto deliberado, y su diff se revisa a mano:

```bash
DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test goldens
```

CI (GitHub Actions) ejecuta la matriz `{ubuntu-latest, windows-latest, macos-latest}` ×
`{stable}`. Un PR no se fusiona con CI en rojo.

## 5. Convenciones de código

- **Edición Rust 2021+ / toolchain stable**. Nada de `nightly` en el árbol principal.
- `rustfmt` por defecto y `clippy -D warnings`. Sin excepciones sin comentario `#[allow]` justificado.
- Errores: `thiserror` en las bibliotecas, `anyhow` solo en los binarios (`docsai-cli`, `docsai-mcp`).
- Logging: `tracing`. En el servidor MCP **jamás escribir en stdout** salvo el protocolo
  (stdout es el canal JSON-RPC); logs siempre a stderr.
- Nombres de código, mensajes de commit y comentarios de código en **inglés**;
  la documentación de `docs/` se mantiene en español.
- Commits: Conventional Commits (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`),
  con ámbito de crate cuando aplique: `feat(office): read numbering.xml`.
- `unsafe` prohibido salvo justificación documentada en el PR (no debería ser necesario).
- Toda función pública de los crates de biblioteca lleva doc-comment con ejemplo cuando sea razonable.

## 6. Estrategia de pruebas (resumen; detalle en el plan §Fase 0 y §Fase 8)

- **Corpus versionado** en `corpus/`: documentos pequeños creados a propósito, un rasgo
  por fichero (`basic-styles.docx`, `nested-lists.docx`, `formulas-basic.xlsx`…). Nunca
  añadir documentos con datos reales o privados.
- **Golden files**: cada documento del corpus tiene su DocMark esperado al lado
  (`.expected.dmk.md`). Los tests comparan la salida con el golden; actualizar un golden
  requiere revisar el diff a mano.
- **Round-trip tests**: `Office → DocMark → Office → DocMark` debe producir DocMark
  idéntico en la segunda pasada (idempotencia). La comparación primera-vs-segunda pasada
  del fichero Office se hace a nivel de IR normalizado, no de bytes.
- **Fuzzing** (`cargo-fuzz`) sobre los parsers de entrada a partir de la Fase 8; los
  parsers nunca deben entrar en pánico con entrada corrupta: siempre `Err`, nunca `panic!`.

## 7. Reglas específicas para agentes de IA

1. **No amplíes el alcance.** Si la tarea pide la Fase N, no adelantes trabajo de la fase
   N+1 "ya que estás". El plan define el orden por dependencias reales.
2. **No cambies la especificación DocMark para hacer pasar un test.** Si el formato no
   puede representar algo, documenta la limitación y usa el mecanismo `raw-block` descrito
   en la especificación.
3. **Nunca degrades la fidelidad en silencio.** Toda pérdida de información en una
   conversión debe emitir una advertencia estructurada (ver `ConversionReport` en
   `docs/arquitectura.md`).
4. **No añadas dependencias pesadas sin justificación.** El objetivo es un binario único y
   razonablemente pequeño. Antes de añadir un crate: ¿está mantenido?, ¿es pure-Rust?,
   ¿qué añade al tamaño del binario? Deja la justificación en el PR.
5. **Documenta al terminar.** Si tu cambio altera comportamiento visible (CLI, formato,
   tools MCP), actualiza README.md y el documento de `docs/` correspondiente en el mismo PR.
6. **Verifica en las tres plataformas mentales.** Rutas con `std::path` (nunca concatenar
   con `/`), finales de línea (el serializador DocMark emite siempre `\n`; el parser acepta
   `\r\n`), y nada de dependencias de herramientas POSIX en el código de producción.
7. Trabaja en ramas y push a la rama que se te indique; no hagas push a `main`.

## 8. Definición de "hecho" (Definition of Done)

Una tarea/fase se considera terminada cuando:

- [ ] Compila y pasa `cargo test --workspace` en las tres plataformas de CI.
- [ ] `clippy` y `fmt` limpios.
- [ ] Tests nuevos cubren el comportamiento añadido (incluidos casos de error).
- [ ] Golden files y corpus actualizados si aplica.
- [ ] Documentación actualizada (README, docs/, `--help` de la CLI).
- [ ] Los criterios de aceptación de la fase correspondiente en `docs/plan-desarrollo.md`
      están marcados y verificados.

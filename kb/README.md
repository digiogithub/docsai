# Base de conocimiento de `docsai`

Documentación de trabajo sobre **lo que hay construido**, cómo está organizado y qué hay que
tener en cuenta al abordar las fases siguientes. Complementa —no sustituye— la documentación de
diseño de [`docs/`](../docs/), que describe el proyecto completo tal como se concibió.

Diferencia práctica entre ambas carpetas:

| Carpeta | Responde a |
|---|---|
| [`docs/`](../docs/) | Qué se quiere construir y por qué: análisis, spec DocMark, arquitectura, plan por fases |
| `kb/` (esta) | Qué está construido hoy, cómo, y qué se sabe ya sobre lo que viene |

## Índice

| Documento | Contenido |
|---|---|
| [01 — Resumen de las fases 0 y 1](01-resumen-fases-0-1.md) | Qué se entregó, criterios de aceptación, qué quedó fuera y por qué |
| [02 — Estructura del proyecto](02-estructura-proyecto.md) | Crates, módulos, reglas de dependencia y por dónde entra cada cosa |
| [03 — Decisiones técnicas](03-decisiones-tecnicas.md) | Las decisiones no obvias, con su motivo y su coste |
| [04 — Consideraciones para las siguientes fases](04-siguientes-fases.md) | Lo que la Fase 2 y posteriores encontrarán ya resuelto, y lo que les espera |

## Estado en una línea

**Fases 0 y 1 cerradas**: `docsai convert x.docx -o x.dmk.md` funciona con estilos, listas,
tablas, imágenes (geometría completa), cabeceras, pies, notas al pie, campos y propiedades. La
vuelta (DocMark → `.docx`) es la Fase 2 y no existe todavía.

## Comprobaciones rápidas

```bash
cargo test --workspace                                  # 164 tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
python3 corpus/generate.py --check                      # el corpus está al día
cargo run -p docsai-cli -- formats                      # matriz de soporte real del binario
```

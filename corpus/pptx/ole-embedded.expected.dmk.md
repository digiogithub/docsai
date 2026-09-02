---
docmark: "1.2"
source-format: pptx
next-id: 7
title: "Objetos incrustados"
author: "docsai corpus"
last-modified-by: "docsai corpus"
created: 2026-01-01T00:00:00Z
modified: 2026-01-02T00:00:00Z
language: "es-ES"
application: "docsai-corpus"
layouts:
  "ppt/slideLayouts/slideLayout1.xml": { name: "Titulo y objetos", master: "ppt/slideMasters/slideMaster1.xml", title: 0, body: 1 }
skeleton: assets/_skeleton/deck-c2724224.pptx
---

## Presupuesto incrustado {#n1 .slide layout="ppt/slideLayouts/slideLayout1.xml"}

- La hoja de cálculo viaja dentro del propio archivo. {list-id=n2}
- Quien la abre edita el original, no una copia enlazada.
- La diapositiva sólo muestra la instantánea que dejó el editor.
- El paquete incrustado tiene que volver byte a byte.

::: {#n3 .ole name="Hoja de cálculo 1" pos="4400000emu,2000000emu" raw=raw-0001 size="10cm,7.5cm"}
:::

## Fórmula incrustada {#n4 .slide layout="ppt/slideLayouts/slideLayout1.xml"}

- La ecuación llega envuelta en una alternativa de compatibilidad. {list-id=n5}
- La rama elegida es cosa de quien la dibuja, no de quien la lee.
- El respaldo es una imagen para quien no sabe activarla.
- Aquí se conservan las dos ramas y no se abre ninguna.

::: {#n6 .ole raw=raw-0002}
:::

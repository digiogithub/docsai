---
tags:
    - phase-16
    - cache
    - mcp
    - performance
    - assets
---
# 79 — Phase 16-H: the conversion cache

Task 5 of [[75-phase-16-plan]], and the last technical increment of the phase. An agent that works
on one document asks several questions about it in a row — `estimate_tokens`, then
`outline_document`, then `search_document`, then `read_selection` — and before this increment each
of those parsed the same bytes from scratch. The MCP protocol stays stateless; what changes is that
the server stops doing the same work four times.

The acceptance criterion has two halves and only one of them is about speed: **the cache
demonstrably removes re-parsing, and disabling it changes no output.** The second half is the one
that decided every design question below.

## A hit must be indistinguishable from a miss

That is the whole rule. A conversion that is merely *fast* and subtly different is not an
optimisation, it is a corruption with a stopwatch attached. Three things follow from it, and each
is a decision that could have gone the easy way.

### The warnings are part of the answer

`read_document` returns a `ConversionReport` next to the IR, and the cache stores it with the
document. A cached read that returned the tree and dropped the warnings would report a degradation
on the first call and stay silent on the second, third and fourth — the exact silent degradation
AGENTS.md §7 rule 3 forbids, made intermittent, which is worse than making it permanent. The test
that compares cached and uncached output compares the report, not only the DocMark.

### The asset store is rebuilt, not bypassed

A read does not only return a document: it fills the caller's `AssetStore`, and for a
`DirAssetStore` that means files on disk. A hit that skipped the read would leave that store empty,
the DocMark would point at `assets/img-….png` files nobody wrote, and `convert_to_markdown` would
return no assets at all.

The way out is a property the project already had and did not have to invent: **asset ids are
content hashes** (`docsai_model::assets::content_hash`, FNV-1a, the same function the plan named).
`RecordingStore` wraps the caller's store during a real read and remembers every distinct byte
string that was put into it, in put order; a hit replays those puts into whatever store the caller
brought. Same bytes in, same ids out, same manifest, same files, same order. The test asserts the
two directories hold the same names.

### The key covers everything, on purpose

Four parts, and the reason for each:

| Part | Why |
|---|---|
| the **origin** — the canonical path, or `bytes:<filename>` | A DocMark file resolves its `src=` sidecars against **its own directory** (`parse_with_base`). The same bytes in two directories are two different documents, and a content hash alone would serve the first one's answer for the second. |
| the **content hash** and the **byte length** | The bytes themselves. The length is kept as a separate field as well: it costs eight bytes and it makes a hash collision need a length collision too. |
| the **options fingerprint** | Everything in `ConvertOptions` except the cache handle. |

The fingerprint deserves its own note, because it is deliberately **wider than it needs to be**.
Today a read consumes only the bytes, the filename hint and `use_loffice`; `fidelity` is applied
when the document is serialised, not when it is read. Keying on fidelity, precision, the style map
and the rest anyway costs hits and buys correctness, and the trade is not close: a key that is too
wide loses a cache hit, a key that is too narrow returns the wrong document.

To keep it that way, `ConvertOptions::cache_fingerprint` **destructures the struct exhaustively**.
Adding a field to `ConvertOptions` stops that function compiling, and whoever adds it has to decide
whether it changes a read. The alternative — a hand-written list of fields — is a bug waiting for
the next increment that adds an option.

## What the cache refuses to do

The instruction for this increment said: if some input cannot be hashed, refuse to serve rather
than serve wrongly. Four refusals, each of which reads normally and stores nothing:

- **The caller kept no cache.** `ConvertOptions::cache` is `None` by default.
- **The asset store already holds something.** The recording is only the puts *this read* makes; if
  the store arrived non-empty, replaying them would not reproduce its state. No caller in the tree
  does this today, and the guard is there so that the first one that does is slow rather than wrong.
- **The input cannot be named.** A path that will not canonicalise, bytes that cannot be read.
- **The source is a legacy `.doc`.** Its read may or may not have gone through LibreOffice
  depending on what was installed *at that moment* (`try_loffice_doc`, and the `Degraded` warning it
  attaches). That is a property of the machine, not of the document, and no key can carry it
  honestly. `.doc` is never cached, and that is a decision rather than an oversight.

Errors are never cached either: only a successful read is stored.

## What a hit costs, measured rather than assumed

The `Document` is handed out by value — callers mutate it (`apply_style_map`) — so a hit clones the
tree. If that clone cost what a read costs, the cache would be theatre. It does not, and the
benchmark prints both numbers rather than claiming it:

```
hit cost on forty-slides.pptx: one read 20.160053ms, one clone of the tree 77.991µs
repeated-call benchmark on forty-slides.pptx × 20 outline calls
  without cache: 678.130347ms (33.906517ms per call)
  with cache:    311.449533ms (15.572476ms per call)
  reads: 20 uncached vs 1 cached
```

A clone is **260× cheaper than a read** on the largest deck in the corpus. The end-to-end win on
`outline_document` is 2.2× rather than 260×, and the honest reason is worth writing down: an
outline's cost is not only the read. It serialises the document to DocMark and counts its tokens on
every call, and the cache removes none of that. On a small `.docx` (`long-report.docx`, 4.5 KB,
a 1.7 ms read) the same benchmark shows only 13%. **The cache is worth what the parse was worth**,
and no more. Caching the serialised DocMark or the token counts is a different optimisation, and it
belongs to whoever measures that those dominate.

The value is held behind an `Arc`, so the entry is shared and only the tree handed to the caller is
copied.

## Where it lives, and why not in a `static`

The cache is **a value the caller owns**: `ConvertOptions::cache: Option<Arc<ConvertCache>>`.

- The MCP server builds one in `McpConfig::from_env` and holds it for the life of the process. That
  server is the entire reason the increment exists.
- The CLI passes `None`. It is one shot, never reads a document twice, and a cache would cost it a
  content hash and the memory to hold a tree nobody will ask for again.
- Every test brings its own, which is why the read counters can be asserted on at all: a process
  global would make them a shared mutable that any parallel test could move.

A `static` was the obvious alternative and was refused for that last reason above all. It would also
have made "disabled" a global mode rather than a per-call option, and the acceptance criterion needs
disabling to be reachable from a test.

Interior mutability is a `std::sync::Mutex`. A **poisoned lock is recovered**
(`PoisonError::into_inner`) rather than treated as a permanent failure: the guarded state is a
vector and some counters, nothing in the critical section can panic, and turning one unrelated panic
elsewhere in the process into a cache that never works again would trade a bug for a permanent
slowdown.

`ConvertCache`'s `Debug` is written by hand and prints its bounds and its size. `ConvertOptions`
derives `Debug` and is printed in traces; a cache that dumped whole document trees into a log line
would be unusable.

## The single choke point

`docsai_convert::service::load_document` is the one function every read-only primitive goes through
— `outline_input`, `search_input`, `select_input`, `inspect_input`, `token_budget_input`, and both
shapes of `convert_to_markdown`. The cache is there and nowhere else. `pipeline::convert_file`, the
CLI's path, is untouched: not one byte of the one-shot conversion path changed.

## The bound

Two of them, and both are needed:

- **64 MiB of payload.** The MCP server's own input ceiling is 50 MiB by default
  (`DOCSAI_MCP_MAX_INPUT_BYTES`), so one maximal document still fits and the cache is never dead
  weight for the largest thing it is allowed to see.
- **16 entries.** An agent session works on a handful of documents at a time; the lookup is a linear
  scan, and each entry costs a whole tree. Sixteen keeps the scan free and the count sane.

An entry heavier than the whole capacity is not stored at all — it would evict everything to hold a
document the next insert throws away.

Both are settable (`ConvertCache::with_capacity`), and the operator's lever is
`DOCSAI_MCP_CACHE_BYTES`, where `0` turns the cache off entirely.

### Known loss: the weight is the input, not the tree

An entry is charged **its input length plus the media bytes it carries**. The media are exact — they
are the bytes the cache actually holds. The `Document` tree is *not* measured: the IR has no size
hook, and adding one to `docsai-model` is another increment's work. The input length stands in for
it, on the argument that the IR is roughly proportional to what it was parsed from.

That proxy is good for text-heavy documents and good for media-heavy ones (where the media dominate
and are counted exactly). It is worst for a small package that expands into a large tree. The
consequence is bounded and one-directional: the cache can hold somewhat more memory than 64 MiB of
*tree*, never more than 64 MiB of *input plus media*. It is written here rather than discovered in a
profiler.

## What the tests prove

`crates/docsai-convert/tests/cache.rs`, ten tests, plus twelve unit tests in the cache module and
two in the MCP tools.

- **Disabling it changes no output** — the important one. Four corpus documents (a docx with media,
  a long docx, a deck, a workbook) × four fidelity levels × three conversions each (uncached, cold
  cached, warm cached), asserting the DocMark, the assets **and** the report are identical.
- **A hit still writes the asset files**, and the two directories match name for name.
- **It removes re-parsing**, counted rather than timed. `CacheStats::misses` is the parse counter:
  five different questions about one document cost one read; four MCP tool calls cost one read. A
  wall-clock assertion would be a coin toss on a loaded CI machine, and there is none.
- **The key discriminates**: four fidelities are four entries; two documents are two entries; the
  same bytes as a path and as base64 are two entries; and **the same bytes in two directories are
  two entries**, which is the DocMark-sidecar trap the origin is in the key for.
- **Eviction works**, by bytes and by entry count, and the evicted document reads again correctly.
- **Errors are refused, not cached.**
- The MCP `stdio_protocol` and `wire_cost` suites now run **with** a cache, so the whole protocol
  surface is exercised over it.

## Verification

```
cargo test --workspace          # see the report; run alongside a parallel increment in docsai-office
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## Next

16-I — the phase closes: acceptance criteria answered with evidence, and what is carried forward
written down.

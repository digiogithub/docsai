//! The read cache (Phase 16-H).
//!
//! The acceptance criterion has two halves and both are here: the cache
//! removes re-parsing, and turning it off changes nothing at all. The first is
//! measured by counting reads, never by a clock — a wall-clock assertion is a
//! flaky test on a loaded CI machine. The second is measured by comparing
//! DocMark, assets *and* warnings, because a cached read that dropped its
//! warnings would degrade silently.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use docsai_convert::{
    convert_to_markdown, inspect_input, outline_input, search_input, select_input,
    token_budget_input, AssetMode, ConvertCache, ConvertOptions, Fidelity, Query, Selector,
    SourceInput,
};

fn corpus(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(relative)
}

const FIDELITIES: [Fidelity; 4] = [
    Fidelity::Full,
    Fidelity::Agent,
    Fidelity::Standard,
    Fidelity::Plain,
];

fn options(fidelity: Fidelity, cache: Option<&Arc<ConvertCache>>) -> ConvertOptions {
    ConvertOptions {
        fidelity,
        cache: cache.cloned(),
        ..Default::default()
    }
}

/// Every document a read of which is worth comparing: text with media, a deck
/// carrying a preserved package, and a workbook.
const DOCUMENTS: [&str; 4] = [
    "docx/images-inline.docx",
    "docx/long-report.docx",
    "pptx/basic-slides.pptx",
    "xlsx/formulas-basic.xlsx",
];

#[test]
fn disabling_the_cache_changes_no_output() {
    for document in DOCUMENTS {
        let path = corpus(document);
        if !path.exists() {
            panic!("corpus document {document} is missing");
        }
        for fidelity in FIDELITIES {
            let plain = convert_to_markdown(
                SourceInput::Path(&path),
                &options(fidelity, None),
                AssetMode::Inline,
            )
            .expect("uncached conversion");

            // A cache of its own per case, so the first call is a miss and the
            // second is necessarily a hit.
            let cache = ConvertCache::shared();
            let cold = convert_to_markdown(
                SourceInput::Path(&path),
                &options(fidelity, Some(&cache)),
                AssetMode::Inline,
            )
            .expect("cold cached conversion");
            let warm = convert_to_markdown(
                SourceInput::Path(&path),
                &options(fidelity, Some(&cache)),
                AssetMode::Inline,
            )
            .expect("warm cached conversion");

            assert_eq!(cache.stats().misses, 1, "{document} at {fidelity:?}");
            assert_eq!(cache.stats().hits, 1, "{document} at {fidelity:?}");

            for (label, other) in [("cold", &cold), ("warm", &warm)] {
                assert_eq!(
                    plain.markdown, other.markdown,
                    "{label} DocMark differs for {document} at {fidelity:?}"
                );
                assert_eq!(
                    plain.report, other.report,
                    "{label} warnings differ for {document} at {fidelity:?}"
                );
                assert_eq!(
                    plain.assets, other.assets,
                    "{label} assets differ for {document} at {fidelity:?}"
                );
                assert_eq!(plain.source_format, other.source_format);
            }
        }
    }
}

#[test]
fn a_hit_still_writes_the_asset_files() {
    let path = corpus("docx/images-inline.docx");
    let cache = ConvertCache::shared();

    let uncached_dir = tempfile::tempdir().expect("tempdir");
    let uncached = convert_to_markdown(
        SourceInput::Path(&path),
        &options(Fidelity::Full, None),
        AssetMode::Files {
            dir: Some(uncached_dir.path().to_path_buf()),
        },
    )
    .expect("uncached");

    let warm_dir = tempfile::tempdir().expect("tempdir");
    for _ in 0..2 {
        let _ = convert_to_markdown(
            SourceInput::Path(&path),
            &options(Fidelity::Full, Some(&cache)),
            AssetMode::Files {
                dir: Some(warm_dir.path().to_path_buf()),
            },
        )
        .expect("cached");
    }
    let warm = convert_to_markdown(
        SourceInput::Path(&path),
        &options(Fidelity::Full, Some(&cache)),
        AssetMode::Files {
            dir: Some(warm_dir.path().to_path_buf()),
        },
    )
    .expect("cached");

    assert_eq!(cache.stats().misses, 1, "one read for three conversions");
    assert_eq!(warm.markdown, uncached.markdown);
    assert_eq!(warm.report, uncached.report);
    assert_eq!(warm.assets, uncached.assets);

    let names = |dir: &Path| {
        let mut found: Vec<String> = std::fs::read_dir(dir)
            .expect("read dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        found
    };
    assert_eq!(
        names(warm_dir.path()),
        names(uncached_dir.path()),
        "a hit must leave the same files on disk as a real read"
    );
    assert!(!names(warm_dir.path()).is_empty(), "the fixture has media");
}

#[test]
fn three_different_questions_cost_one_read() {
    let path = corpus("docx/long-report.docx");
    let cache = ConvertCache::shared();
    let options = options(Fidelity::Full, Some(&cache));

    let outline = outline_input(SourceInput::Path(&path), &options, None).expect("outline");
    let query = Query::new("the").expect("query");
    let search = search_input(SourceInput::Path(&path), &options, &query).expect("search");
    let selector: Selector = "type:heading".parse().expect("selector");
    let selection = select_input(SourceInput::Path(&path), &options, &selector).expect("select");
    let report = inspect_input(SourceInput::Path(&path), &options).expect("inspect");
    let budget = token_budget_input(SourceInput::Path(&path), &options).expect("tokens");

    let stats = cache.stats();
    assert_eq!(stats.misses, 1, "one parse for five questions");
    assert_eq!(stats.hits, 4);
    assert_eq!(stats.entries, 1);

    // And the answers are the answers: the same five questions without a cache.
    let bare = self::options(Fidelity::Full, None);
    assert_eq!(
        outline.nodes.len(),
        outline_input(SourceInput::Path(&path), &bare, None)
            .expect("outline")
            .nodes
            .len()
    );
    assert_eq!(
        search.hits.len(),
        search_input(SourceInput::Path(&path), &bare, &query)
            .expect("search")
            .hits
            .len()
    );
    assert_eq!(
        selection.docmark,
        select_input(SourceInput::Path(&path), &bare, &selector)
            .expect("select")
            .docmark
    );
    assert_eq!(
        report.stats.paragraphs,
        inspect_input(SourceInput::Path(&path), &bare)
            .expect("inspect")
            .stats
            .paragraphs
    );
    assert_eq!(
        budget.render_text(),
        token_budget_input(SourceInput::Path(&path), &bare)
            .expect("tokens")
            .render_text()
    );
}

#[test]
fn the_key_separates_fidelities() {
    let path = corpus("docx/long-report.docx");
    let cache = ConvertCache::shared();
    let mut rendered = Vec::new();
    for fidelity in FIDELITIES {
        rendered.push(
            convert_to_markdown(
                SourceInput::Path(&path),
                &options(fidelity, Some(&cache)),
                AssetMode::Inline,
            )
            .expect("convert")
            .markdown,
        );
    }
    let stats = cache.stats();
    assert_eq!(stats.misses, 4, "four fidelities are four reads");
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.entries, 4);
    rendered.dedup();
    assert_eq!(rendered.len(), 4, "the four fidelities render differently");
}

#[test]
fn the_key_separates_documents_and_origins() {
    let cache = ConvertCache::shared();
    let first = corpus("docx/basic-text.docx");
    let second = corpus("docx/table-simple.docx");
    for path in [&first, &second, &first] {
        let _ = convert_to_markdown(
            SourceInput::Path(path),
            &options(Fidelity::Full, Some(&cache)),
            AssetMode::Inline,
        )
        .expect("convert");
    }
    assert_eq!(cache.stats().misses, 2, "two documents are two reads");
    assert_eq!(cache.stats().hits, 1);

    // The same bytes arriving as bytes rather than as a path are a different
    // origin: a DocMark input resolves its sidecars against its directory, so
    // the two must not share an entry.
    let bytes = std::fs::read(&first).expect("read");
    let from_bytes = convert_to_markdown(
        SourceInput::Bytes {
            data: &bytes,
            filename: "basic-text.docx",
        },
        &options(Fidelity::Full, Some(&cache)),
        AssetMode::Inline,
    )
    .expect("convert");
    assert_eq!(cache.stats().misses, 3);
    let from_path = convert_to_markdown(
        SourceInput::Path(&first),
        &options(Fidelity::Full, Some(&cache)),
        AssetMode::Inline,
    )
    .expect("convert");
    assert_eq!(from_bytes.markdown, from_path.markdown);
}

#[test]
fn a_docmark_source_is_keyed_by_its_directory() {
    // The same bytes in two directories are two documents: DocMark resolves
    // its `src=` sidecars against its own parent. If the key were the content
    // hash alone the second read would be served the first one's answer.
    let text = "---\ntitle: Same bytes\n---\n\n# Heading\n\nBody.\n";
    let first = tempfile::tempdir().expect("tempdir");
    let second = tempfile::tempdir().expect("tempdir");
    let a = first.path().join("doc.dmk.md");
    let b = second.path().join("doc.dmk.md");
    std::fs::write(&a, text).expect("write");
    std::fs::write(&b, text).expect("write");

    let cache = ConvertCache::shared();
    for path in [&a, &b] {
        let _ = convert_to_markdown(
            SourceInput::Path(path),
            &options(Fidelity::Full, Some(&cache)),
            AssetMode::Inline,
        )
        .expect("convert");
    }
    assert_eq!(
        cache.stats().misses,
        2,
        "identical bytes in two directories are two entries"
    );
    assert_eq!(cache.stats().entries, 2);
}

#[test]
fn the_cache_is_bounded_and_evicts() {
    // A capacity that fits one of these documents but not two.
    let first = corpus("docx/long-report.docx");
    let second = corpus("docx/basic-styles.docx");
    let weight = std::fs::metadata(&first).expect("meta").len() as usize;
    let cache = Arc::new(ConvertCache::with_capacity(weight + 512, 16));

    let read = |path: &Path| {
        convert_to_markdown(
            SourceInput::Path(path),
            &options(Fidelity::Full, Some(&cache)),
            AssetMode::Inline,
        )
        .expect("convert")
    };
    read(&first);
    assert_eq!(cache.stats().entries, 1);
    read(&second);
    assert_eq!(cache.stats().entries, 1, "the older entry was evicted");
    assert!(cache.stats().bytes <= cache.capacity_bytes());
    assert!(cache.stats().evictions >= 1);

    // And the evicted document simply reads again, correctly.
    let again = read(&first);
    assert_eq!(cache.stats().misses, 3);
    assert_eq!(
        again.markdown,
        convert_to_markdown(
            SourceInput::Path(&first),
            &options(Fidelity::Full, None),
            AssetMode::Inline
        )
        .expect("convert")
        .markdown
    );
}

#[test]
fn an_error_is_never_cached() {
    let cache = ConvertCache::shared();
    let missing = corpus("docx/there-is-no-such-file.docx");
    for _ in 0..2 {
        let err = convert_to_markdown(
            SourceInput::Path(&missing),
            &options(Fidelity::Full, Some(&cache)),
            AssetMode::Inline,
        );
        assert!(err.is_err());
    }
    assert_eq!(cache.stats().entries, 0);
    // The key could not even be built (nothing to canonicalise), so the cache
    // refused rather than counting a read it did not watch.
    assert_eq!(cache.stats().refusals, 2);
    assert_eq!(cache.stats().hits, 0);
}

#[test]
fn clearing_the_cache_costs_a_read_and_no_correctness() {
    let path = corpus("docx/table-simple.docx");
    let cache = ConvertCache::shared();
    let before = convert_to_markdown(
        SourceInput::Path(&path),
        &options(Fidelity::Full, Some(&cache)),
        AssetMode::Inline,
    )
    .expect("convert");
    cache.clear();
    let after = convert_to_markdown(
        SourceInput::Path(&path),
        &options(Fidelity::Full, Some(&cache)),
        AssetMode::Inline,
    )
    .expect("convert");
    assert_eq!(cache.stats().misses, 2);
    assert_eq!(before.markdown, after.markdown);
    assert_eq!(before.report, after.report);
}

/// The repeated-call benchmark the acceptance criterion asks for.
///
/// It asserts on the read count only. The times are printed — run with
/// `cargo test -p docsai-convert --test cache -- --nocapture` to see them —
/// because a wall-clock assertion on a shared CI machine is a coin toss.
#[test]
fn repeated_call_benchmark() {
    const ROUNDS: u32 = 20;
    // The largest deck in the corpus: forty slides plus the preserved package
    // the reader stores as an asset, which is the shape the cache exists for.
    let path = corpus("pptx/forty-slides.pptx");

    let cache = ConvertCache::shared();
    let run = |cache: Option<&Arc<ConvertCache>>| {
        // One untimed call first: the first read of a process pays for page
        // faults and lazily built tables that have nothing to do with either
        // arm of the comparison.
        let options = options(Fidelity::Full, cache);
        let _ = outline_input(SourceInput::Path(&path), &options, None).expect("outline");
        let start = Instant::now();
        for _ in 0..ROUNDS {
            let _ = outline_input(SourceInput::Path(&path), &options, None).expect("outline");
        }
        start.elapsed()
    };
    let uncached = run(None);
    let cached = run(Some(&cache));

    let stats = cache.stats();
    assert_eq!(stats.misses, 1, "{ROUNDS} + 1 calls, one read");
    assert_eq!(stats.hits, u64::from(ROUNDS));

    // What a hit actually costs: the document is handed out by value, so a hit
    // clones the tree. If that clone cost what a read costs the cache would be
    // theatre, so it is measured rather than assumed.
    let read_start = Instant::now();
    let mut document = None;
    for _ in 0..ROUNDS {
        let mut store = docsai_model::MemoryAssetStore::new();
        let (read, _, _) = docsai_convert::read_path(&path, &mut store).expect("read");
        document = Some(read);
    }
    let one_read = read_start.elapsed() / ROUNDS;
    let document = document.expect("read");
    let clone_start = Instant::now();
    for _ in 0..ROUNDS {
        let _ = document.clone();
    }
    let one_clone = clone_start.elapsed() / ROUNDS;

    println!(
        "hit cost on {}: one read {one_read:?}, one clone of the tree {one_clone:?}",
        path.file_name().unwrap().to_string_lossy(),
    );
    println!(
        "repeated-call benchmark on {} × {ROUNDS} outline calls\n  \
         without cache: {:?} ({:?} per call)\n  \
         with cache:    {:?} ({:?} per call)\n  \
         reads: {} uncached vs {} cached",
        path.file_name().unwrap().to_string_lossy(),
        uncached,
        uncached / ROUNDS,
        cached,
        cached / ROUNDS,
        ROUNDS,
        stats.misses,
    );
}

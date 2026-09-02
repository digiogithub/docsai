//! The objects that stay opaque, confirmed end to end (Phase 16-D,
//! `kb/80-phase-16-opaque.md`).
//!
//! SmartArt, OLE, transitions and animations have read as a stub plus a sidecar
//! raw fragment since 13-I (`kb/43-phase-13-raw.md`) and have been written back
//! since 15-D (`kb/65-phase-15-raw.md`). What had never been asserted anywhere
//! is the **whole loop** on packages that carry them: pptx → IR → DocMark →
//! IR → pptx, with the bytes of the object's own parts compared at the end.
//!
//! Three claims are made here, one per kind, and each is a separate test so a
//! failure names which half broke:
//!
//! 1. **The reader tells an agent what it is looking at** — the stub's class,
//!    its text and the fragment's markup, not merely "something was here".
//! 2. **The package is the package that was read**, byte for byte, including
//!    the parts no reader of this project ever opens: an embedded OPC package,
//!    an OLE2 compound file, a diagram's five parts.
//! 3. **A trip through DocMark does not lose the object**, and an edit made in
//!    the DocMark reaches the package beside it. This is the half the writer's
//!    own tests cannot see: the deck parser leaves `Presentation::raw` empty,
//!    so every stub comes back pointing at a payload nothing carries, and the
//!    object survives only because the preserved skeleton holds it.
//!
//! Nothing here is a new feature. Every defect these tests found is written
//! down in the KB entry, fixed or carried.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use docsai_docmark::{Fidelity, Options, RawPolicy};
use docsai_model::presentation::{Presentation, RawShapeKind, ShapeKind};
use docsai_model::{AssetStore, Document, Format, MemoryAssetStore, Warning};

fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/pptx")
        .join(name)
}

fn read(path: &Path) -> (Document, MemoryAssetStore, docsai_model::ConversionReport) {
    let mut assets = MemoryAssetStore::new();
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (document, report) = docsai_office::read_pptx(file, &mut assets)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    (document, assets, report)
}

fn write(
    document: &Document,
    assets: &dyn AssetStore,
) -> (Vec<u8>, docsai_model::ConversionReport) {
    let mut out = Cursor::new(Vec::new());
    let report = docsai_office::write_pptx(document, assets, &mut out).expect("the deck writes");
    (out.into_inner(), report)
}

fn deck(document: &Document) -> &Presentation {
    match document {
        Document::Presentation(deck) => deck,
        other => panic!("not a deck: {}", other.shape_name()),
    }
}

/// One part of a package, decompressed. `None` when the package has no such
/// member, which is a distinction the assertions below depend on.
fn part(package: &[u8], name: &str) -> Option<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(package)).ok()?;
    let mut bytes = Vec::new();
    zip.by_name(name).ok()?.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn text_part(package: &[u8], name: &str) -> String {
    let bytes = part(package, name).unwrap_or_else(|| panic!("`{name}` is not in the package"));
    String::from_utf8(bytes).unwrap_or_else(|_| panic!("`{name}` is not text"))
}

/// Every raw stub of a slide, in reading order, with the fragment it points at.
fn stubs(deck: &Presentation, slide: usize) -> Vec<(&docsai_model::presentation::RawShape, &str)> {
    deck.slides[slide]
        .shapes
        .iter()
        .filter_map(|shape| match &shape.kind {
            ShapeKind::Raw(raw) => Some(raw),
            _ => None,
        })
        .map(|raw| {
            let id = raw.raw.as_ref().expect("a stub without its markup");
            let fragment = deck
                .raw
                .iter()
                .find(|fragment| &fragment.id == id)
                .unwrap_or_else(|| panic!("`{}` points at no fragment", id.as_str()));
            (raw, fragment.content.as_str())
        })
        .collect()
}

fn options(fidelity: Fidelity) -> Options {
    Options {
        fidelity,
        source_format: Format::Pptx,
        // The sidecar is the pipeline's default and the loop below deliberately
        // does not write one: a stub whose payload nobody carries is the common
        // case, and the object has to survive it (15-D decision 4).
        raw: RawPolicy::Sidecar,
        ..Default::default()
    }
}

/// The loop this increment exists to assert: read the package, write DocMark,
/// parse it back, write a package. `edit` is applied to the DocMark text in
/// between, so a test can prove the deck is still editable while the opaque
/// object rides along untouched.
fn round_trip(name: &str, edit: impl Fn(String) -> String) -> (String, Vec<u8>) {
    let (document, mut assets, _) = read(&corpus(name));
    let (markdown, _) = docsai_docmark::serialize(&document, &assets, &options(Fidelity::Full));
    let edited = edit(markdown.clone());
    let (parsed, _) = docsai_docmark::parse(&edited, &mut assets).expect("the DocMark parses");
    assert!(
        deck(&parsed).skeleton.is_some(),
        "{name}: the preserved package did not survive the DocMark round trip"
    );
    let (package, _) = write(&parsed, &assets);
    (markdown, package)
}

fn unedited(markdown: String) -> String {
    markdown
}

/// The names of the warnings a report carries, for the assertions that a loss
/// was reported — and, just as much, that one was not invented.
fn raw_block_warnings(report: &docsai_model::ConversionReport) -> Vec<&Warning> {
    report
        .warnings
        .iter()
        .filter(|warning| {
            matches!(
                warning,
                Warning::UnsupportedElement { action, .. } if action == "raw-block"
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 1. What the reader hands an agent
// ---------------------------------------------------------------------------

/// An embedded object arrives two ways and both come back as `ole`.
///
/// Slide 1 is the ECMA-376 spelling — a `p:graphicFrame` whose
/// `a:graphicData@uri` ends in `ole` — and slide 2 is the one PowerPoint
/// writes, wrapped in `mc:AlternateContent` with a picture in the fallback.
/// The second is the branch `wrapped_kind` exists for and the reason this
/// fixture has two slides: naming the stub from the uri alone would call it
/// `other`, which under-informs exactly the agent the stub is for.
#[test]
fn an_embedded_object_reads_as_an_ole_stub_over_its_own_markup() {
    let (document, _, report) = read(&corpus("ole-embedded.pptx"));
    let deck = deck(&document);

    let (worksheet, worksheet_markup) = stubs(deck, 0)
        .into_iter()
        .next()
        .expect("slide 1 has an object");
    assert_eq!(worksheet.kind, RawShapeKind::Ole);
    assert!(
        worksheet_markup.starts_with("<p:graphicFrame>")
            && worksheet_markup.contains("<p:oleObj ")
            && worksheet_markup.contains(r#"progId="Excel.Sheet.12""#),
        "the worksheet's markup is not the frame that was read: {worksheet_markup}"
    );

    let (equation, equation_markup) = stubs(deck, 1)
        .into_iter()
        .next()
        .expect("slide 2 has an object");
    assert_eq!(equation.kind, RawShapeKind::Ole);
    assert!(
        equation_markup.starts_with("<mc:AlternateContent")
            && equation_markup.contains("<mc:Choice")
            && equation_markup.contains("<mc:Fallback>")
            && equation_markup.contains(r#"progId="Equation.3""#),
        "the equation lost a branch of its alternate content: {equation_markup}"
    );

    // Two objects, two fragments, two warnings — and nothing else claimed.
    assert_eq!(deck.raw.len(), 2, "one fragment per object, no more");
    assert_eq!(raw_block_warnings(&report).len(), 2);
}

/// SmartArt: one stub over the whole `mc:AlternateContent`, and the fallback
/// text on the stub so the diagram's words are not hidden behind a payload no
/// agent is meant to read.
#[test]
fn smart_art_reads_as_one_stub_that_still_says_what_the_diagram_says() {
    let (document, _, report) = read(&corpus("smartart-fallback.pptx"));
    let deck = deck(&document);

    let found = stubs(deck, 0);
    assert_eq!(found.len(), 1, "the pair is preserved whole, as one stub");
    let (stub, markup) = found[0];
    assert_eq!(stub.kind, RawShapeKind::SmartArt);
    assert_eq!(stub.text, "Planificar · Ejecutar · Revisar");
    assert!(
        markup.contains("<dgm:relIds") && markup.contains("<mc:Fallback>"),
        "neither branch may be dropped: {markup}"
    );
    assert_eq!(raw_block_warnings(&report).len(), 1);
}

/// A transition and an animation timeline are not shapes: they hang off
/// `p:sld` beside the shape tree, and `read_slide` looked at nothing but
/// `cSld/spTree` until 13-I. They come back as slide-level fragments, in the
/// order the part states them.
#[test]
fn a_transition_and_a_timeline_read_as_slide_level_fragments() {
    let (document, _, _) = read(&corpus("raw-preserved.pptx"));
    let deck = deck(&document);

    let names: Vec<&str> = deck.slides[0]
        .raw
        .iter()
        .map(|id| {
            let fragment = deck
                .raw
                .iter()
                .find(|fragment| &fragment.id == id)
                .expect("the slide points at a fragment of the deck");
            fragment
                .content
                .split([' ', '>'])
                .next()
                .expect("a fragment starts with its own tag")
        })
        .collect();
    assert_eq!(names, ["<p:transition", "<p:timing"]);
}

// ---------------------------------------------------------------------------
// 2. The package that was read
// ---------------------------------------------------------------------------

/// The parts no reader of this project ever opens come back byte for byte.
///
/// `a_written_deck_is_the_package_that_was_read` already compares the whole
/// archive for every corpus deck. This one is narrower and louder: it names the
/// individual members that carry an opaque object, so a regression says *which*
/// object was lost instead of "the package differs".
#[test]
fn the_parts_an_opaque_object_lives_in_survive_a_plain_round_trip() {
    let cases: &[(&str, &[&str])] = &[
        (
            "ole-embedded.pptx",
            &[
                "ppt/embeddings/workbook1.xlsx",
                "ppt/embeddings/oleObject1.bin",
                "ppt/media/image1.png",
                "ppt/slides/slide1.xml",
                "ppt/slides/slide2.xml",
            ],
        ),
        (
            "smartart-fallback.pptx",
            &[
                "ppt/diagrams/data1.xml",
                "ppt/diagrams/layout1.xml",
                "ppt/diagrams/quickStyle1.xml",
                "ppt/diagrams/colors1.xml",
                "ppt/diagrams/drawing1.xml",
                "ppt/slides/slide1.xml",
            ],
        ),
        ("raw-preserved.pptx", &["ppt/slides/slide1.xml"]),
    ];

    for (name, parts) in cases {
        let original = std::fs::read(corpus(name)).expect("reads the deck");
        let (document, assets, _) = read(&corpus(name));
        let (written, report) = write(&document, &assets);
        assert!(
            written == original,
            "{name}: the package is not the one that was read"
        );
        for member in *parts {
            assert!(
                part(&written, member) == part(&original, member),
                "{name}: `{member}` came out different"
            );
        }
        // Nothing was lost, so nothing may be reported as lost (15-D decision
        // 1): a false warning is the same failure as a missing one.
        assert!(
            report.warnings.is_empty(),
            "{name}: the writer invented a warning about a deck it did not change: {:?}",
            report.warnings
        );
    }
}

// ---------------------------------------------------------------------------
// 3. The whole loop, through DocMark
// ---------------------------------------------------------------------------

/// An embedded object survives pptx → DocMark → pptx, payload parts included.
///
/// The DocMark carries a stub and a `raw=` id; the deck parser loads no
/// sidecar, so the model that comes back points at a fragment nobody supplied.
/// The object is in the written package all the same, because it is in the
/// preserved skeleton — and the two embedded payloads, which no code in this
/// repository has ever parsed, are the same bytes they went in as.
#[test]
fn an_embedded_object_survives_the_docmark_loop() {
    let original = std::fs::read(corpus("ole-embedded.pptx")).expect("reads the deck");
    let (markdown, package) = round_trip("ole-embedded.pptx", unedited);

    // The agent's half: the DocMark says an OLE object is on each slide.
    assert_eq!(markdown.matches(".ole").count(), 2, "{markdown}");

    let slide1 = text_part(&package, "ppt/slides/slide1.xml");
    assert!(
        slide1.contains("<p:oleObj ") && slide1.contains(r#"progId="Excel.Sheet.12""#),
        "the worksheet did not survive the loop: {slide1}"
    );
    let slide2 = text_part(&package, "ppt/slides/slide2.xml");
    assert!(
        slide2.contains("<mc:AlternateContent")
            && slide2.contains("<mc:Fallback>")
            && slide2.contains(r#"progId="Equation.3""#),
        "the equation did not survive the loop: {slide2}"
    );
    for member in [
        "ppt/embeddings/workbook1.xlsx",
        "ppt/embeddings/oleObject1.bin",
    ] {
        assert!(
            part(&package, member) == part(&original, member),
            "`{member}` did not come back out of the loop unchanged"
        );
    }
}

/// SmartArt survives the loop with all five of its parts.
///
/// Spike P3 §2.4 measured a naive rewrite dropping four of the five with no
/// visible change at all, which is the failure this assertion is shaped
/// against: the count is what matters, not that "a diagram is there".
#[test]
fn smart_art_survives_the_docmark_loop_with_all_five_parts() {
    let original = std::fs::read(corpus("smartart-fallback.pptx")).expect("reads the deck");
    let (markdown, package) = round_trip("smartart-fallback.pptx", unedited);
    assert!(markdown.contains(".smartart"), "{markdown}");

    for member in [
        "ppt/diagrams/data1.xml",
        "ppt/diagrams/layout1.xml",
        "ppt/diagrams/quickStyle1.xml",
        "ppt/diagrams/colors1.xml",
        "ppt/diagrams/drawing1.xml",
    ] {
        assert!(
            part(&package, member) == part(&original, member),
            "`{member}` did not come back out of the loop unchanged"
        );
    }
    let slide = text_part(&package, "ppt/slides/slide1.xml");
    assert!(
        slide.contains("<dgm:relIds") && slide.contains("<mc:Fallback>"),
        "the alternate content lost a branch: {slide}"
    );
}

/// A transition and an animation timeline survive the loop.
///
/// They are the two the DocMark says nothing about at all: they are not shapes,
/// so they have no stub, and the only thing that carries them across is the
/// preserved package. That is worth an assertion precisely because there is
/// nothing in the Markdown to notice their absence by.
#[test]
fn a_transition_and_a_timeline_survive_the_docmark_loop() {
    let (_, package) = round_trip("raw-preserved.pptx", unedited);
    let slide = text_part(&package, "ppt/slides/slide1.xml");
    for markup in ["<p:grpSp>", "<a:custGeom>", "<p:transition", "<p:timing>"] {
        assert!(
            slide.contains(markup),
            "`{markup}` did not survive the loop: {slide}"
        );
    }
}

/// The deck is still editable while the objects ride along.
///
/// A loop that preserved everything by refusing to write would pass every
/// assertion above. This is the one that says the writer is doing its job: the
/// retyped title is in the package, and the object beside it is untouched.
#[test]
fn an_edit_beside_an_opaque_object_reaches_the_package() {
    for (name, part_name, from, to) in [
        (
            "ole-embedded.pptx",
            "ppt/slides/slide1.xml",
            "Presupuesto incrustado",
            "Presupuesto revisado",
        ),
        (
            "smartart-fallback.pptx",
            "ppt/slides/slide1.xml",
            "Proceso",
            "Flujo de trabajo",
        ),
        (
            "raw-preserved.pptx",
            "ppt/slides/slide1.xml",
            "Lo que no se modela",
            "Lo que sigue ahí",
        ),
    ] {
        let (markdown, package) = round_trip(name, |text| {
            let edited = text.replace(from, to);
            assert_ne!(edited, text, "{name}: the fixture no longer says `{from}`");
            edited
        });
        assert!(markdown.contains(from), "{name}: the title was not written");
        let slide = text_part(&package, part_name);
        assert!(slide.contains(to), "{name}: the edit is not in the package");
        assert!(
            !slide.contains(from),
            "{name}: the old title is still in the package"
        );
        for markup in ["<p:oleObj ", "<dgm:relIds", "<p:transition"] {
            // Only the deck that had it, but the one that had it keeps it.
            let original = text_part(
                &std::fs::read(corpus(name)).expect("reads the deck"),
                part_name,
            );
            assert_eq!(
                slide.contains(markup),
                original.contains(markup),
                "{name}: `{markup}` was gained or lost by an edit next to it"
            );
        }
    }
}

/// Rule 8: the stub is at every fidelity level that writes containers, and at
/// `plain` the object leaves a warning rather than a silence.
///
/// This is the reader-facing half of "confirmed end to end": an agent asking
/// for the cheapest useful rendering still learns that the slide holds an
/// object it must not delete.
#[test]
fn the_stub_survives_every_fidelity_level_and_plain_says_what_it_dropped() {
    let (document, assets, _) = read(&corpus("ole-embedded.pptx"));
    for fidelity in [Fidelity::Full, Fidelity::Agent, Fidelity::Standard] {
        let (markdown, _) = docsai_docmark::serialize(&document, &assets, &options(fidelity));
        assert_eq!(
            markdown.matches(".ole").count(),
            2,
            "{fidelity:?} lost a stub: {markdown}"
        );
    }
    let (markdown, report) =
        docsai_docmark::serialize(&document, &assets, &options(Fidelity::Plain));
    assert!(
        !markdown.contains(".ole"),
        "plain writes no containers: {markdown}"
    );
    let dropped = report
        .warnings
        .iter()
        .filter(|warning| {
            matches!(warning, Warning::UnsupportedElement { action, .. }
                if action.starts_with("dropped"))
        })
        .count();
    assert_eq!(dropped, 2, "plain dropped an object without saying so");
}

/// The strongest form of the claim: a deck nobody edited comes back out of the
/// **whole loop** as the package that went in — not merely with its objects
/// intact, but byte for byte.
///
/// A round trip through DocMark is not the writer's own no-op path: the model
/// has been through a serialiser and a parser, its raw fragments are gone, its
/// node ids are new, and the slides are rewritten from the text that survived.
/// That the archive still hashes the same is what makes an edit measurable as
/// an edit.
#[test]
fn a_deck_nobody_edited_comes_back_out_of_the_loop_byte_for_byte() {
    for name in [
        "ole-embedded.pptx",
        "smartart-fallback.pptx",
        "raw-preserved.pptx",
    ] {
        let original = std::fs::read(corpus(name)).expect("reads the deck");
        let (_, package) = round_trip(name, unedited);
        assert!(
            package == original,
            "{name}: the loop did not give back the package it was given"
        );
    }
}
/// A deck written **without** its preserved package still says what it lost.
///
/// The generation path (15-G) cannot re-emit an opaque object: no template
/// supplies a diagram's five parts or an embedded worksheet. What it owes the
/// caller is the truth, and this asserts all of it — one `Degraded` per object,
/// naming the slide, plus the label the stub carried standing where the object
/// was, so the slide is not silently short of a shape.
///
/// The slide-level half of it was **missing until 16-D**: a `p:transition` and
/// a `p:timing` live beside the shape tree, the shape loop never saw them, and
/// a generated deck dropped both without a word. `write_gen::slide_xml` now
/// reports them.
#[test]
fn a_deck_generated_without_its_package_names_every_object_it_could_not_write() {
    for (name, objects, slide_level) in [
        ("ole-embedded.pptx", 2, 0),
        ("smartart-fallback.pptx", 1, 0),
        // A group, a custom geometry, and the transition and timing together.
        ("raw-preserved.pptx", 2, 1),
    ] {
        let (mut document, assets, _) = read(&corpus(name));
        match &mut document {
            Document::Presentation(deck) => deck.skeleton = None,
            _ => unreachable!(),
        }
        let (package, report) = write(&document, &assets);

        let degraded: Vec<&str> = report
            .warnings
            .iter()
            .filter_map(|warning| match warning {
                Warning::Degraded { why, .. } => Some(why.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            degraded
                .iter()
                .filter(|why| why.starts_with("a preserved object"))
                .count(),
            objects,
            "{name}: an object was written or lost without a word: {degraded:#?}"
        );
        assert_eq!(
            degraded
                .iter()
                .filter(|why| why.contains("beside its shape tree"))
                .count(),
            slide_level,
            "{name}: the slide-level markup was dropped in silence: {degraded:#?}"
        );

        // The object is gone — it could not be otherwise — but the slide is not
        // short of a shape: the stub's label stands where the object was.
        let slide = text_part(&package, "ppt/slides/slide1.xml");
        assert!(
            !slide.contains("<p:oleObj ") && !slide.contains("<dgm:relIds"),
            "{name}: a generated slide cannot hold the original markup: {slide}"
        );
        if name == "smartart-fallback.pptx" {
            assert!(
                slide.contains("Planificar · Ejecutar · Revisar"),
                "{name}: the diagram's own words did not reach the generated slide"
            );
        }
    }
}

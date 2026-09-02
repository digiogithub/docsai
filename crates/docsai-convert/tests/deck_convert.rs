//! A deck through the conversion pipeline (plan v2 Phase 14-J).
//!
//! 13-K refused `convert` on a presentation because DocMark-P did not exist:
//! the serializer would have handed back an empty body and a caller
//! redirecting stdout to a file would have got a success that lost every
//! slide. The profile exists now, so the refusal is gone and what replaces it
//! is this: the deck leaves the pipeline as a file that references only files
//! that were written, and reads back in.

use std::path::{Path, PathBuf};

use docsai_convert::{convert_file, ConvertOptions};
use docsai_docmark::Fidelity;
use docsai_model::{Document, Format, MemoryAssetStore};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/pptx")
}

fn decks() -> Vec<PathBuf> {
    let dir = corpus();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("pptx") | Some("pptm")
            )
        })
        .collect();
    paths.sort();
    paths
}

/// A directory of its own per test, so two of them cannot share an `assets/`.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("docsai-deck-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creates the scratch directory");
    dir
}

fn options(fidelity: Fidelity) -> ConvertOptions {
    ConvertOptions {
        fidelity,
        ..ConvertOptions::default()
    }
}

#[test]
fn every_deck_of_the_corpus_converts_to_docmark() {
    let dir = scratch("convert");
    for deck in decks() {
        let stem = deck.file_stem().unwrap().to_string_lossy().into_owned();
        let output = dir.join(format!("{stem}.dmk.md"));
        let outcome = convert_file(&deck, Some(&output), &options(Fidelity::Full))
            .unwrap_or_else(|e| panic!("{}: {e}", deck.display()));

        assert_eq!(outcome.source_format, Format::Pptx);
        assert_eq!(outcome.target_format, Format::DocMark);
        let written = std::fs::read_to_string(&output).expect("the deck was written");
        assert_eq!(written, outcome.markdown);
        assert!(
            written.contains(".slide"),
            "{stem}: the converted deck has no slide"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every path the front matter and the body name is a file that exists.
///
/// A converted document is only usable where it stands, so a reference that
/// resolves on the machine that wrote it and nowhere else is the failure this
/// guards: the `skeleton:` line names `_skeleton/deck-<hash>.pptx`, built from
/// the content hash rather than from the package's own name, and it is the
/// pipeline that has to put the bytes there.
#[test]
fn the_converted_deck_references_only_files_that_were_written() {
    let dir = scratch("assets");
    for deck in decks() {
        let stem = deck.file_stem().unwrap().to_string_lossy().into_owned();
        let output = dir.join(format!("{stem}/{stem}.dmk.md"));
        let outcome = convert_file(&deck, Some(&output), &options(Fidelity::Full))
            .unwrap_or_else(|e| panic!("{}: {e}", deck.display()));
        let base = output.parent().unwrap();

        for reference in references(&outcome.markdown) {
            let path = base.join(&reference);
            assert!(
                path.is_file(),
                "{stem}: `{reference}` is referenced and was not written"
            );
            assert!(
                outcome
                    .assets_written
                    .iter()
                    .any(|written| written == &path),
                "{stem}: `{reference}` exists but is not in the outcome's asset list"
            );
        }

        // The package is one asset, so it is one file: the store writes what a
        // reader puts in under `img-<hash>.<ext>`, which for a deck's skeleton
        // would be the same bytes a second time under a name nothing points at.
        let stray: Vec<_> = std::fs::read_dir(base.join("assets"))
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|name| name.ends_with(".pptx") || name.ends_with(".pptm"))
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            stray.is_empty(),
            "{stem}: a package was left loose in `assets/`: {stray:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_converted_deck_reads_back_as_a_deck() {
    let dir = scratch("roundtrip");
    for deck in decks() {
        let stem = deck.file_stem().unwrap().to_string_lossy().into_owned();
        let output = dir.join(format!("{stem}/{stem}.dmk.md"));
        convert_file(&deck, Some(&output), &options(Fidelity::Full))
            .unwrap_or_else(|e| panic!("{}: {e}", deck.display()));

        // Nothing is seeded here: the parser gets the file and the directory
        // beside it, which is all a person who received the document has.
        let markdown = std::fs::read_to_string(&output).expect("reads the deck back");
        let mut assets = MemoryAssetStore::new();
        let (document, _) =
            docsai_docmark::parse_with_base(&markdown, output.parent(), &mut assets)
                .unwrap_or_else(|e| panic!("{stem}: {e}"));
        let Document::Presentation(parsed) = document else {
            panic!("{stem}: came back as something that is not a deck");
        };
        assert!(!parsed.slides.is_empty(), "{stem}: no slides came back");
        assert!(
            parsed.skeleton.is_some(),
            "{stem}: the preserved package was not found beside the document"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `standard` and `plain` write no `skeleton:` (spec §11.2 rule 6), so the
/// pipeline writes no package either: an unreferenced copy of the original
/// deck beside a document meant to be readable is exactly the surprise that
/// level exists to avoid.
#[test]
fn the_readable_levels_leave_no_package_behind() {
    let dir = scratch("levels");
    for fidelity in [Fidelity::Standard, Fidelity::Plain] {
        for deck in decks() {
            let stem = deck.file_stem().unwrap().to_string_lossy().into_owned();
            let output = dir.join(format!("{}-{stem}/{stem}.md", fidelity.as_str()));
            let outcome = convert_file(&deck, Some(&output), &options(fidelity))
                .unwrap_or_else(|e| panic!("{}: {e}", deck.display()));
            assert!(
                !outcome.markdown.contains("skeleton:"),
                "{stem} at {}: the reference is a full/agent one",
                fidelity.as_str()
            );
            // Not only `_skeleton/`: the reader stores the package whatever
            // the level, so before 14-K it landed flat in `assets/` under an
            // image's name — a copy of the whole original next to a document
            // whose point is being readable.
            let base = output.parent().unwrap();
            assert!(
                !base.join("assets/_skeleton").exists(),
                "{stem} at {}: a package was written that nothing refers to",
                fidelity.as_str()
            );
            let package_bytes = std::fs::read(&deck).expect("reads the original deck");
            let loose: Vec<_> = std::fs::read_dir(base.join("assets"))
                .map(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .filter(|e| {
                            std::fs::read(e.path()).is_ok_and(|bytes| bytes == package_bytes)
                        })
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            assert!(
                loose.is_empty(),
                "{stem} at {}: the original package is beside the document as {loose:?}",
                fidelity.as_str()
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The Phase 13 criterion deferred into this phase (plan v2, Phase 13):
/// *«`--fidelity agent` on that deck is ≤ 15 % of the `full` token count»*,
/// from analysis §6.5 — everything an agent can edit as text, everything else
/// collapsed to one line with its id.
///
/// It is **not met**, and it is written down rather than softened. Over the
/// corpus `agent` measures 96–102 % of `full`: the two differ by the
/// `fidelity:` line and nothing else, because what `agent` drops — formatting
/// (`Fidelity::formatting`) — these decks barely carry, and what §6.5 wanted
/// dropped, the geometry of shapes nobody edits by hand, is written at `agent`
/// by `Fidelity::measurements`. Closing that gap changes what a level means,
/// which is a change to the specification and not to a test (`AGENTS.md` §7
/// rule 2), so it is a decision to take, with its own increment.
///
/// The measurement itself lives here so the number is one command away:
///
/// ```text
/// cargo test -p docsai-convert --test deck_convert -- --ignored --nocapture
/// ```
#[test]
#[ignore = "analysis §6.5 target not met: `agent` is ~100 % of `full`, not 15 %"]
fn agent_fidelity_is_at_most_fifteen_percent_of_full() {
    let mut over = Vec::new();
    for deck in decks() {
        let stem = deck.file_stem().unwrap().to_string_lossy().into_owned();
        let full = tokens(&deck, Fidelity::Full);
        let agent = tokens(&deck, Fidelity::Agent);
        let percent = 100.0 * agent as f64 / full as f64;
        println!("{stem:28} full={full:6} agent={agent:6} ({percent:.0} %)");
        if percent > 15.0 {
            over.push(format!("{stem}: {percent:.0} %"));
        }
    }
    assert!(
        over.is_empty(),
        "{} deck(s) over the 15 % target:\n{}",
        over.len(),
        over.join("\n")
    );
}

/// What a deck costs an LLM at a level, through the same counter `docsai
/// tokens` uses.
fn tokens(deck: &Path, fidelity: Fidelity) -> usize {
    docsai_convert::tokens::token_report_path(deck, &options(fidelity))
        .unwrap_or_else(|e| panic!("{}: {e}", deck.display()))
        .total
}

/// Every path a DocMark document points at: the `skeleton:` line and the
/// `(…)` of an image.
fn references(markdown: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in markdown.lines() {
        if let Some(path) = line.strip_prefix("skeleton: ") {
            out.push(path.trim().to_string());
        }
        let mut rest = line;
        while let Some(start) = rest.find("](") {
            let after = &rest[start + 2..];
            let Some(end) = after.find(')') else { break };
            let path = &after[..end];
            if !path.starts_with("http") {
                out.push(path.to_string());
            }
            rest = &after[end..];
        }
    }
    out
}

/// The cycle the phase exists to close (plan v2 Phase 15-B): a deck converted
/// to DocMark, edited as text, and written back into the package it came from.
///
/// The pipeline is what is under test here, not the writer — the writer's own
/// tests live in `docsai-office`. What can only be seen from here is that the
/// `skeleton:` reference written on the way out is resolved on the way back in,
/// so the package the edit lands in is the one the document names.
#[test]
fn a_deck_edited_as_docmark_goes_back_into_its_package() {
    let dir = scratch("write-back");
    let markdown = dir.join("deck.dmk.md");
    convert_file(
        &corpus().join("basic-slides.pptx"),
        Some(&markdown),
        &options(Fidelity::Full),
    )
    .expect("converts to DocMark");

    let source = std::fs::read_to_string(&markdown).expect("reads the deck back");
    let edited = source.replace("Informe trimestral", "Informe reescrito");
    assert_ne!(edited, source, "the fixture no longer holds the title");
    std::fs::write(&markdown, &edited).expect("writes the edit");

    let package = dir.join("deck.pptx");
    let outcome = convert_file(&markdown, Some(&package), &options(Fidelity::Full))
        .expect("writes the package back");
    assert_eq!(outcome.target_format, Format::Pptx);

    // Read the written package with the reader, which is the only opinion that
    // counts about what is in it.
    let mut assets = MemoryAssetStore::new();
    let file = std::fs::File::open(&package).expect("the package was written");
    let (document, _) = docsai_convert::read_document(file, Some("deck.pptx"), &mut assets)
        .map(|(document, _, report)| (document, report))
        .expect("reads the written package");
    let Document::Presentation(deck) = document else {
        panic!("the written package is not a deck");
    };
    assert_eq!(
        deck.slides[0].title().as_deref(),
        Some("Informe reescrito"),
        "the edit is not in the package"
    );
    assert_eq!(deck.slides.len(), 2, "a slide was lost on the way back");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 15-C through the pipeline: a table cell edited in the Markdown is in the
/// package, and the grid around it is the one that was read.
#[test]
fn a_table_cell_edited_as_docmark_reaches_the_package() {
    let dir = scratch("write-back-table");
    let markdown = dir.join("deck.dmk.md");
    convert_file(
        &corpus().join("tables-simple.pptx"),
        Some(&markdown),
        &options(Fidelity::Full),
    )
    .expect("converts to DocMark");

    let source = std::fs::read_to_string(&markdown).expect("reads the deck back");
    let edited = source.replace("Norte", "Noroeste");
    assert_ne!(edited, source, "the fixture no longer holds the cell");
    std::fs::write(&markdown, &edited).expect("writes the edit");

    let package = dir.join("deck.pptx");
    convert_file(&markdown, Some(&package), &options(Fidelity::Full))
        .expect("writes the package back");

    let mut assets = MemoryAssetStore::new();
    let file = std::fs::File::open(&package).expect("the package was written");
    let (document, _) = docsai_convert::read_document(file, Some("deck.pptx"), &mut assets)
        .map(|(document, _, report)| (document, report))
        .expect("reads the written package");
    let Document::Presentation(deck) = document else {
        panic!("the written package is not a deck");
    };
    let table = deck.slides[0]
        .shapes
        .iter()
        .find_map(|shape| match &shape.kind {
            docsai_model::presentation::ShapeKind::Table(table) => Some(table),
            _ => None,
        })
        .expect("the table survived the round trip");
    assert_eq!(table.rows.len(), 3, "the grid changed shape");
    let cell = format!("{:?}", table.rows[1].cells[0]);
    assert!(
        cell.contains("Noroeste"),
        "the edit is not in the package: {cell}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A picture pointed at another file in the Markdown: the new file's bytes are
/// the ones that end up in `ppt/media/`, and the deck declares the extension it
/// had never carried.
#[test]
fn an_image_swapped_in_docmark_reaches_the_package() {
    // A 1×1 transparent GIF: a real file, and an extension this deck lacks.
    const TINY_GIF: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff\
                              !\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\
                              \x00\x00\x02\x02D\x01\x00;";
    let dir = scratch("write-back-image");
    let markdown = dir.join("deck.dmk.md");
    convert_file(
        &corpus().join("images-anchored.pptx"),
        Some(&markdown),
        &options(Fidelity::Full),
    )
    .expect("converts to DocMark");

    std::fs::write(dir.join("assets").join("otra.gif"), TINY_GIF).expect("writes the new image");
    let source = std::fs::read_to_string(&markdown).expect("reads the deck back");
    let start = source.find("](assets/").expect("the deck names its image");
    let end = source[start..].find(')').expect("a closing bracket") + start;
    let edited = format!("{}](assets/otra.gif{}", &source[..start], &source[end..]);
    std::fs::write(&markdown, &edited).expect("writes the edit");

    let package = dir.join("deck.pptx");
    convert_file(&markdown, Some(&package), &options(Fidelity::Full))
        .expect("writes the package back");

    let bytes = std::fs::read(&package).expect("the package was written");
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a readable package");
    let mut media = Vec::new();
    std::io::Read::read_to_end(
        &mut zip
            .by_name("ppt/media/image2.gif")
            .expect("the new media part"),
        &mut media,
    )
    .expect("reads it");
    assert_eq!(media, TINY_GIF, "the image was not stored as its own bytes");

    let mut types = String::new();
    std::io::Read::read_to_string(
        &mut zip
            .by_name("[Content_Types].xml")
            .expect("the content types"),
        &mut types,
    )
    .expect("reads it");
    assert!(
        types.contains("Extension=\"gif\""),
        "the new extension is not declared: {types}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 15-D through the pipeline: a preserved object survives the round trip.
///
/// This is the case the writer's own tests cannot see, because the deck parser
/// does not load the raw sidecar: `Presentation::raw` comes back **empty**, so
/// the stub in the Markdown points at a payload nothing carries. The group and
/// the custom geometry are in the written package all the same — they are in
/// the preserved skeleton, at the z-order they were read at — and the writer
/// says nothing about them, because nothing was lost.
#[test]
fn a_preserved_object_survives_the_docmark_round_trip() {
    let dir = scratch("write-back-raw");
    let markdown = dir.join("deck.dmk.md");
    convert_file(
        &corpus().join("raw-preserved.pptx"),
        Some(&markdown),
        &options(Fidelity::Full),
    )
    .expect("converts to DocMark");

    let source = std::fs::read_to_string(&markdown).expect("reads the deck back");
    let edited = source.replace("Lo que no se modela", "Lo que sigue ahí");
    assert_ne!(edited, source, "the fixture no longer holds the title");
    std::fs::write(&markdown, &edited).expect("writes the edit");

    let package = dir.join("deck.pptx");
    convert_file(&markdown, Some(&package), &options(Fidelity::Full))
        .expect("writes the package back");

    let slide = {
        let file = std::fs::File::open(&package).expect("the package was written");
        let mut zip = zip::ZipArchive::new(file).expect("the written package is a zip");
        let mut xml = String::new();
        std::io::Read::read_to_string(
            &mut zip.by_name("ppt/slides/slide1.xml").expect("slide 1"),
            &mut xml,
        )
        .expect("slide 1 is text");
        xml
    };
    assert!(
        slide.contains("Lo que sigue ahí"),
        "the edit is not in the package"
    );
    for markup in ["<p:grpSp>", "<a:custGeom>", "<p:transition", "<p:timing>"] {
        assert!(
            slide.contains(markup),
            "`{markup}` did not survive the round trip: {slide}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// 15-E through the pipeline: a note retyped in the Markdown is in the notes
/// page, and the notes page nobody touched is the one that was read.
#[test]
fn a_note_edited_as_docmark_reaches_its_notes_page() {
    let dir = scratch("write-back-notes");
    let markdown = dir.join("deck.dmk.md");
    convert_file(
        &corpus().join("notes-speaker.pptx"),
        Some(&markdown),
        &options(Fidelity::Full),
    )
    .expect("converts to DocMark");

    let source = std::fs::read_to_string(&markdown).expect("reads the deck back");
    let edited = source.replace(
        "Insistir en que el 12 % es interanual.",
        "Insistir en que es interanual, y en el margen.",
    );
    assert_ne!(edited, source, "the fixture no longer holds the note");
    std::fs::write(&markdown, &edited).expect("writes the edit");

    let package = dir.join("deck.pptx");
    convert_file(&markdown, Some(&package), &options(Fidelity::Full))
        .expect("writes the package back");

    let notes = part_of(&package, "ppt/notesSlides/notesSlide1.xml");
    assert!(
        notes.contains("Insistir en que es interanual, y en el margen."),
        "the edited note is not in the notes page: {notes}"
    );
    assert!(
        notes.contains("No entrar en el desglose por región."),
        "the rest of the note was lost: {notes}"
    );
    // The page is a printed page: its thumbnail placeholder is furniture the
    // writer never touches.
    assert!(
        notes.contains("sldImg"),
        "the notes page lost its furniture"
    );
    let untouched = part_of(&package, "ppt/notesSlides/notesSlide2.xml");
    assert!(
        untouched.contains("Si preguntan por el proveedor"),
        "the notes page nobody edited changed: {untouched}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 15-E through the pipeline: a slide deleted as text leaves the package, and
/// takes its own parts with it.
#[test]
fn a_slide_deleted_in_docmark_leaves_the_package() {
    let dir = scratch("write-back-removed");
    let markdown = dir.join("deck.dmk.md");
    convert_file(
        &corpus().join("basic-slides.pptx"),
        Some(&markdown),
        &options(Fidelity::Full),
    )
    .expect("converts to DocMark");

    let source = std::fs::read_to_string(&markdown).expect("reads the deck back");
    let at = source
        .find("## Siguientes pasos")
        .expect("the fixture no longer holds the second slide");
    std::fs::write(&markdown, &source[..at]).expect("writes the edit");

    let package = dir.join("deck.pptx");
    convert_file(&markdown, Some(&package), &options(Fidelity::Full))
        .expect("writes the package back");

    let mut assets = MemoryAssetStore::new();
    let file = std::fs::File::open(&package).expect("the package was written");
    let (document, _, _) = docsai_convert::read_document(file, Some("deck.pptx"), &mut assets)
        .expect("reads the written package");
    let Document::Presentation(deck) = document else {
        panic!("the written package is not a deck");
    };
    assert_eq!(
        deck.slides.len(),
        1,
        "the deleted slide is still in the deck"
    );
    assert_eq!(
        deck.slides[0].title().as_deref(),
        Some("Informe trimestral")
    );

    let file = std::fs::File::open(&package).expect("the package was written");
    let mut zip = zip::ZipArchive::new(file).expect("the written package is a zip");
    assert!(
        zip.by_name("ppt/slides/slide2.xml").is_err(),
        "the deleted slide's text is still in the file"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// One part of a written package, as text.
fn part_of(package: &Path, part: &str) -> String {
    let file = std::fs::File::open(package).expect("the package was written");
    let mut zip = zip::ZipArchive::new(file).expect("the written package is a zip");
    let mut xml = String::new();
    std::io::Read::read_to_string(
        &mut zip.by_name(part).unwrap_or_else(|e| panic!("{part}: {e}")),
        &mut xml,
    )
    .expect("the part is text");
    xml
}

/// 15-F through the pipeline: the one thing the writer deliberately does not
/// preserve. The body of `autofit-stale.pptx` carries a `fontScale` PowerPoint
/// computed for eight lines; an agent that retypes one of them has made that
/// number a lie no reader can detect, so it leaves with the text it was
/// computed for, and the report names the shape.
#[test]
fn an_edited_body_leaves_its_stale_autofit_scale_behind() {
    let dir = scratch("write-back-autofit");
    let markdown = dir.join("deck.dmk.md");
    convert_file(
        &corpus().join("autofit-stale.pptx"),
        Some(&markdown),
        &options(Fidelity::Full),
    )
    .expect("converts to DocMark");

    let source = std::fs::read_to_string(&markdown).expect("reads the deck back");
    let edited = source.replace(
        "Línea 3 de un cuerpo deliberadamente largo",
        "Línea 3, mucho más corta",
    );
    assert_ne!(edited, source, "the fixture no longer holds the body");
    std::fs::write(&markdown, &edited).expect("writes the edit");

    let package = dir.join("deck.pptx");
    let outcome = convert_file(&markdown, Some(&package), &options(Fidelity::Full))
        .expect("writes the package back");

    let slide = part_of(&package, "ppt/slides/slide1.xml");
    assert!(
        slide.contains("<a:normAutofit/>"),
        "the stale scale is still in the package: {slide}"
    );
    assert!(
        slide.contains("Línea 3, mucho más corta"),
        "the edit is not in the package: {slide}"
    );
    assert!(
        outcome.report.warnings.iter().any(|warning| matches!(
            warning,
            docsai_model::report::Warning::AutofitStale { what, scale }
                if *scale == 62_500 && what.contains("Content Placeholder 2")
        )),
        "the report does not name the shape whose scale was dropped: {:?}",
        outcome.report.warnings
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The acceptance criterion of 15-G, and the one the whole increment owns: an
/// agent writes **plain Markdown** — no `{.slide}`, no `layouts:`, no
/// `skeleton:`, not one attribute in the body — and gets a package a
/// presentation tool opens, with a layout chosen from the shape of the content.
///
/// The front matter still has to say `source-format: pptx`, because that is
/// what tells the parser a `##` heading is a slide rather than a heading in a
/// text document. Everything below it is what a person would type.
#[test]
fn attribute_free_markdown_becomes_a_deck() {
    let dir = scratch("plain-markdown");
    let markdown = dir.join("deck.dmk.md");
    std::fs::write(
        &markdown,
        "---\ndocmark: \"1.2\"\nsource-format: pptx\ntitle: Deck a mano\n---\n\n\
         ## Resultados del trimestre\n\n- Ingresos al alza\n- Costes estables\n\n\
         ## Cierre\n",
    )
    .expect("writes the Markdown");

    let package = dir.join("deck.pptx");
    let outcome = convert_file(&markdown, Some(&package), &options(Fidelity::Full))
        .expect("writes the package");
    assert_eq!(outcome.target_format, Format::Pptx);

    // The parts of an OPC package that has to hold together: the two slides,
    // their relationships, the design they were written on, and the three
    // places that have to agree that a slide exists at all.
    let names = part_names(&package);
    for part in [
        "[Content_Types].xml",
        "_rels/.rels",
        "docProps/core.xml",
        "ppt/presentation.xml",
        "ppt/_rels/presentation.xml.rels",
        "ppt/slideMasters/slideMaster1.xml",
        "ppt/slideLayouts/slideLayout1.xml",
        "ppt/slideLayouts/slideLayout2.xml",
        "ppt/theme/theme1.xml",
        "ppt/slides/slide1.xml",
        "ppt/slides/slide2.xml",
        "ppt/slides/_rels/slide1.xml.rels",
        "ppt/slides/_rels/slide2.xml.rels",
    ] {
        assert!(
            names.contains(&part.to_string()),
            "`{part}` is not in {names:?}"
        );
    }

    let types = part_of(&package, "[Content_Types].xml");
    for part in ["/ppt/slides/slide1.xml", "/ppt/slides/slide2.xml"] {
        assert!(
            types.contains(&format!(
                "<Override PartName=\"{part}\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
            )),
            "`{part}` has no content type: {types}"
        );
    }

    let deck_rels = part_of(&package, "ppt/_rels/presentation.xml.rels");
    for target in ["slides/slide1.xml", "slides/slide2.xml"] {
        assert!(
            deck_rels.contains(&format!("Target=\"{target}\"")),
            "the deck does not relate `{target}`: {deck_rels}"
        );
    }
    let presentation = part_of(&package, "ppt/presentation.xml");
    assert_eq!(
        presentation.matches("<p:sldId ").count(),
        2,
        "the slide list does not hold both slides: {presentation}"
    );

    // Rule 1: a title with bullets is «Title and Content». Rule 2: a title on
    // its own is «Title Slide», and its title placeholder is the centred one
    // that layout declares.
    assert!(
        part_of(&package, "ppt/slides/_rels/slide1.xml.rels").contains("slideLayout2.xml"),
        "the first slide is not on «Title and Content»"
    );
    assert!(part_of(&package, "ppt/slideLayouts/slideLayout2.xml")
        .contains("name=\"Title and Content\""));
    assert!(
        part_of(&package, "ppt/slides/_rels/slide2.xml.rels").contains("slideLayout1.xml"),
        "the second slide is not on «Title Slide»"
    );
    assert!(part_of(&package, "ppt/slides/slide2.xml").contains("<p:ph type=\"ctrTitle\"/>"));

    // Every relationship of every part points at a part the package carries: a
    // dangling one is what P3 measured LibreOffice accepting in silence.
    for rels in names.iter().filter(|name| name.ends_with(".rels")) {
        let dir = rels.rsplit_once("_rels/").expect("a `_rels` part").0;
        for target in part_of(&package, rels).split("Target=\"").skip(1) {
            let target = target.split('"').next().unwrap_or_default();
            if target.contains("://") {
                continue;
            }
            let mut resolved: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
            for step in target.split('/') {
                match step {
                    ".." => {
                        resolved.pop();
                    }
                    "." => {}
                    name => resolved.push(name),
                }
            }
            let resolved = resolved.join("/");
            assert!(
                names.contains(&resolved),
                "`{rels}` points at `{resolved}`, which the package does not carry"
            );
        }
    }

    // And the writer says what it did rather than leaving the caller to find
    // out that the design is not the deck's own.
    assert!(
        outcome.report.warnings.iter().any(|warning| matches!(
            warning,
            docsai_model::report::Warning::Degraded { what, why }
                if what.contains("design") && why.contains("embedded default template")
        )),
        "the report does not say the deck was written on the template: {:?}",
        outcome.report.warnings
    );

    // The package reads back as the deck that was typed.
    let mut assets = MemoryAssetStore::new();
    let file = std::fs::File::open(&package).expect("the package was written");
    let (document, ..) = docsai_convert::read_document(file, Some("deck.pptx"), &mut assets)
        .expect("reads the written package");
    let Document::Presentation(deck) = document else {
        panic!("what was written is not a deck");
    };
    assert_eq!(deck.slides.len(), 2);
    assert_eq!(
        deck.slides[0].title().as_deref(),
        Some("Resultados del trimestre")
    );
    assert_eq!(deck.slides[1].title().as_deref(), Some("Cierre"));

    let _ = std::fs::remove_dir_all(&dir);
}

/// Every member of a written package, by name.
fn part_names(package: &Path) -> Vec<String> {
    let file = std::fs::File::open(package).expect("the package was written");
    let zip = zip::ZipArchive::new(file).expect("the written package is a zip");
    zip.file_names().map(str::to_string).collect()
}

//! The `.odp` writer through the pipeline (Phase 18, increment 18-D).
//!
//! Three things are asserted here, and the first of them is the phase's own
//! first acceptance criterion:
//!
//! 1. **`.odp` round trip with DocMark identity on the second pass** over the
//!    seventeen fixtures of `corpus/odp`.
//! 2. **Byte identity for an untouched `.odp`.** ODF's `mimetype` has to be the
//!    first member of the archive and stored uncompressed, which no rebuilt
//!    package can promise; the writer copies the archive entry by entry, so it
//!    keeps both facts without stating either.
//! 3. **15-B's rule**: a deck nobody edited produces no edit at all, and one
//!    that was edited produces exactly the edit that was made — in the one part
//!    that carries it, with every other member of the package untouched.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};

use docsai_convert::{convert_file, ConvertOptions};
use docsai_docmark::Fidelity;
use docsai_model::{Document, Format, MemoryAssetStore};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/odp")
}

fn decks() -> Vec<PathBuf> {
    let dir = corpus();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("odp"))
        .collect();
    paths.sort();
    paths
}

/// A directory of its own per test, so two of them cannot share an `assets/`.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("docsai-odp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creates the scratch directory");
    dir
}

fn options() -> ConvertOptions {
    ConvertOptions {
        fidelity: Fidelity::Full,
        ..ConvertOptions::default()
    }
}

/// Every member of a package, by name, in archive order.
fn members(path: &Path) -> Vec<(String, Vec<u8>)> {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut zip = zip::ZipArchive::new(file).expect("a zip archive");
    let mut out = Vec::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).expect("an entry");
        let name = entry.name().to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("reads the entry");
        out.push((name, bytes));
    }
    out
}

/// The phase's first acceptance criterion.
#[test]
fn every_odp_round_trips_with_docmark_identity_on_the_second_pass() {
    let decks = decks();
    assert_eq!(
        decks.len(),
        17,
        "the corpus moved: {} fixtures",
        decks.len()
    );
    let dir = scratch("roundtrip");
    for deck in &decks {
        let stem = deck.file_stem().unwrap().to_string_lossy().into_owned();
        let work = dir.join(&stem);
        std::fs::create_dir_all(&work).expect("creates the working directory");
        let first = work.join("first.dmk.md");
        let outcome = convert_file(deck, Some(&first), &options())
            .unwrap_or_else(|e| panic!("{stem}: converts to DocMark: {e}"));
        let one = outcome.markdown.clone();

        let written = work.join(format!("{stem}.odp"));
        convert_file(&first, Some(&written), &options())
            .unwrap_or_else(|e| panic!("{stem}: converts back to .odp: {e}"));

        let second = work.join("second.dmk.md");
        let outcome = convert_file(&written, Some(&second), &options())
            .unwrap_or_else(|e| panic!("{stem}: converts the written package: {e}"));
        assert_eq!(
            one, outcome.markdown,
            "{stem}: the second DocMark pass differs from the first"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The property the plan asked for by name, and the one `mimetype` threatens.
#[test]
fn an_untouched_odp_comes_back_byte_for_byte() {
    let dir = scratch("bytes");
    for deck in &decks() {
        let stem = deck.file_stem().unwrap().to_string_lossy().into_owned();
        let work = dir.join(&stem);
        std::fs::create_dir_all(&work).expect("creates the working directory");
        let markdown = work.join("deck.dmk.md");
        convert_file(deck, Some(&markdown), &options())
            .unwrap_or_else(|e| panic!("{stem}: converts to DocMark: {e}"));
        let written = work.join(format!("{stem}.odp"));
        convert_file(&markdown, Some(&written), &options())
            .unwrap_or_else(|e| panic!("{stem}: converts back: {e}"));

        let before = std::fs::read(deck).expect("reads the fixture");
        let after = std::fs::read(&written).expect("reads what was written");
        assert_eq!(
            before, after,
            "{stem}: the round trip is not byte for byte the package that was read"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The two ODF rules a rebuilt package would have to remember, and that a
/// copied one cannot forget.
#[test]
fn mimetype_is_the_first_member_and_stored_uncompressed() {
    let dir = scratch("mimetype");
    let deck = corpus().join("basic-slides.odp");
    let markdown = dir.join("deck.dmk.md");
    convert_file(&deck, Some(&markdown), &options()).expect("converts to DocMark");
    let written = dir.join("out.odp");
    convert_file(&markdown, Some(&written), &options()).expect("converts back");

    let file = std::fs::File::open(&written).expect("the package was written");
    let mut zip = zip::ZipArchive::new(file).expect("a zip archive");
    let first = zip.by_index_raw(0).expect("a first member");
    assert_eq!(first.name(), "mimetype", "mimetype is not the first member");
    assert_eq!(
        first.compression(),
        zip::CompressionMethod::Stored,
        "mimetype is compressed, so the package cannot be sniffed"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 15-B's rule, in the direction that proves it: a deck the model *has* edited
/// changes exactly one member of the package, and the change is the edit.
#[test]
fn a_retitled_page_changes_content_xml_and_nothing_else() {
    let dir = scratch("retitle");
    let deck = corpus().join("basic-slides.odp");
    let markdown = dir.join("deck.dmk.md");
    convert_file(&deck, Some(&markdown), &options()).expect("converts to DocMark");

    let source = std::fs::read_to_string(&markdown).expect("reads the DocMark back");
    let edited = source.replace("Informe trimestral", "Informe reescrito");
    assert_ne!(edited, source, "the fixture no longer holds that title");
    std::fs::write(&markdown, &edited).expect("writes the edit");

    let written = dir.join("out.odp");
    convert_file(&markdown, Some(&written), &options()).expect("writes the package back");

    let before: BTreeMap<String, Vec<u8>> = members(&deck).into_iter().collect();
    let after: BTreeMap<String, Vec<u8>> = members(&written).into_iter().collect();
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "the round trip added or removed a member"
    );
    let moved: Vec<&String> = before
        .keys()
        .filter(|name| before[*name] != after[*name])
        .collect();
    assert_eq!(
        moved,
        vec!["content.xml"],
        "exactly one part carries the edit"
    );

    let content = String::from_utf8(after["content.xml"].clone()).expect("utf-8");
    assert!(
        content.contains("Informe reescrito"),
        "the new title is not in the part"
    );
    assert!(
        !content.contains("Informe trimestral"),
        "the old title is still in the part"
    );

    // And the writer's own reader agrees about what it wrote.
    let mut assets = MemoryAssetStore::new();
    let file = std::fs::File::open(&written).expect("the package was written");
    let (document, _) = docsai_odf::read_odp(file, &mut assets).expect("reads it back");
    let Document::Presentation(read_back) = document else {
        panic!("what was written is not a deck");
    };
    assert_eq!(
        read_back.slides[0].title().as_deref(),
        Some("Informe reescrito")
    );
    assert_eq!(read_back.slides.len(), 2, "a page was lost on the way back");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Speaker notes are a body like any other, and they live in the page rather
/// than in a part of their own — so the same splice reaches them.
#[test]
fn edited_speaker_notes_reach_the_package() {
    let dir = scratch("notes");
    let deck = corpus().join("notes-speaker.odp");
    let markdown = dir.join("deck.dmk.md");
    convert_file(&deck, Some(&markdown), &options()).expect("converts to DocMark");

    let source = std::fs::read_to_string(&markdown).expect("reads the DocMark back");
    let edited = source.replace("es interanual", "es interanual y consolidado");
    assert_ne!(edited, source, "the fixture no longer holds that note");
    std::fs::write(&markdown, &edited).expect("writes the edit");

    let written = dir.join("out.odp");
    convert_file(&markdown, Some(&written), &options()).expect("writes the package back");

    let mut assets = MemoryAssetStore::new();
    let file = std::fs::File::open(&written).expect("the package was written");
    let (document, _) = docsai_odf::read_odp(file, &mut assets).expect("reads it back");
    let Document::Presentation(read_back) = document else {
        panic!("what was written is not a deck");
    };
    let notes = read_back.slides[0].notes.as_ref().expect("the notes page");
    let text: String = notes
        .iter()
        .map(|block| match block {
            docsai_model::text::Block::Paragraph(p) => p.plain_text(),
            _ => String::new(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("es interanual y consolidado"),
        "the edited note is not in the package: {text}"
    );
    assert!(
        text.contains("No entrar en el desglose"),
        "the note that was not edited was lost: {text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// [`kb/98-phase-18-odp-reader.md`]'s second finding for this increment, made a
/// test rather than a hope.
///
/// A page transition is declared on a `style:style` of family `drawing-page`,
/// and **that style may live in `styles.xml`** — the corpus's own is in
/// `content.xml`, so a splicer that assumed every raw fragment belonged to
/// `content.xml` would drop it and no corpus test would notice. The fixture is
/// therefore built here: `raw-preserved.odp` with its `dp-fade` style moved
/// across, which is a legal `.odp` and one the reader reads identically.
#[test]
fn a_transition_declared_in_styles_xml_survives_the_round_trip() {
    const FADE: &str = "presentation:transition-style=\"fade-from-center\"";
    let dir = scratch("styles-transition");
    let deck = corpus().join("raw-preserved.odp");

    let mut parts = members(&deck);
    let content_index = parts
        .iter()
        .position(|(name, _)| name == "content.xml")
        .expect("content.xml");
    let styles_index = parts
        .iter()
        .position(|(name, _)| name == "styles.xml")
        .expect("styles.xml");
    let content = String::from_utf8(parts[content_index].1.clone()).expect("utf-8");
    let start = content
        .find("<style:style style:name=\"dp-fade\"")
        .expect("dp-fade");
    let end = content[start..].find("</style:style>").expect("its end") + start + 14;
    let moved = content[start..end].to_string();
    assert!(
        moved.contains(FADE),
        "the fixture no longer has a transition"
    );
    let content = format!("{}{}", &content[..start], &content[end..]);

    let styles = String::from_utf8(parts[styles_index].1.clone()).expect("utf-8");
    let anchor = "</office:automatic-styles>";
    let at = styles
        .find(anchor)
        .expect("styles.xml has automatic styles");
    let styles = format!("{}{moved}{}", &styles[..at], &styles[at..]);

    parts[content_index].1 = content.into_bytes();
    parts[styles_index].1 = styles.into_bytes();

    let fixture = dir.join("styles-transition.odp");
    write_package(&fixture, &parts);

    // The reader sees the transition where it now is.
    let mut assets = MemoryAssetStore::new();
    let file = std::fs::File::open(&fixture).expect("the fixture was written");
    let (document, _) = docsai_odf::read_odp(file, &mut assets).expect("reads the fixture");
    let Document::Presentation(deck) = document else {
        panic!("not a deck");
    };
    let from_styles = deck
        .raw
        .iter()
        .find(|fragment| fragment.content.contains(FADE))
        .expect("the transition is preserved");
    assert_eq!(
        from_styles.part, "styles.xml",
        "the fragment does not name the part it came from"
    );

    // And the round trip keeps it, because the writer only ever edits
    // `content.xml` and copies every other member as it stands.
    let markdown = dir.join("deck.dmk.md");
    convert_file(&fixture, Some(&markdown), &options()).expect("converts to DocMark");
    let written = dir.join("out.odp");
    convert_file(&markdown, Some(&written), &options()).expect("converts back");

    let after: BTreeMap<String, Vec<u8>> = members(&written).into_iter().collect();
    let styles = String::from_utf8(after["styles.xml"].clone()).expect("utf-8");
    assert!(
        styles.contains(FADE),
        "the transition declared in styles.xml did not survive the round trip"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A deck with no preserved package is written on the embedded template, and
/// the substitution is said out loud.
///
/// This **reverses 18-D**, which refused such a deck by name because there was
/// no ODF template to fall back on and inventing a master page would have been
/// a design nobody chose, silently substituted. 18-E ships the template — the
/// cross conversion needs it, since a deck read from a `.pptx` has an OPC
/// package and therefore no ODF package at all — so the second half of that
/// argument is what stands: the substitution happens and it is warned.
#[test]
fn a_deck_with_no_preserved_package_is_written_on_the_embedded_template() {
    let dir = scratch("no-skeleton");
    let deck = corpus().join("basic-slides.odp");
    let markdown = dir.join("deck.dmk.md");
    convert_file(&deck, Some(&markdown), &options()).expect("converts to DocMark");

    let source = std::fs::read_to_string(&markdown).expect("reads the DocMark back");
    let stripped: String = source
        .lines()
        .filter(|line| !line.starts_with("skeleton:"))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_ne!(
        stripped, source,
        "the DocMark carried no skeleton to remove"
    );
    let orphan = dir.join("orphan.dmk.md");
    std::fs::write(&orphan, stripped).expect("writes the stripped DocMark");

    let written = dir.join("out.odp");
    let report = convert_file(&orphan, Some(&written), &options())
        .expect("a deck with no package is written on the template");
    assert!(
        report.report.warnings.iter().any(|w| {
            let message = w.message();
            message.contains("no preserved package") && message.contains("template")
        }),
        "the substitution was not warned: {:?}",
        report.report.warnings
    );

    // And the file is a real `.odp`: both slides, both titles.
    let back = dir.join("back.dmk.md");
    convert_file(&written, Some(&back), &options()).expect("reads the written deck back");
    let text = std::fs::read_to_string(&back).expect("reads the DocMark");
    assert!(text.contains("Informe trimestral"), "{text}");
    assert!(text.contains("Siguientes pasos"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Writes a package the way the corpus generator does: `mimetype` first and
/// stored, everything else deflated.
fn write_package(path: &Path, parts: &[(String, Vec<u8>)]) {
    let mut buffer = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in parts {
            let options = if name == "mimetype" { stored } else { deflated };
            zip.start_file(name.as_str(), options).expect("starts");
            zip.write_all(bytes).expect("writes");
        }
        zip.finish().expect("finishes the archive");
    }
    buffer.rewind().expect("rewinds");
    std::fs::write(path, buffer.into_inner()).expect("writes the package");
}

/// `docsai formats` has to agree with the crate.
#[test]
fn the_support_matrix_says_odp_writes() {
    assert!(docsai_convert::can_write(Format::Odp));
    assert!(docsai_convert::can_read(Format::Odp));
}

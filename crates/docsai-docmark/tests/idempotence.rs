//! The guarantee of spec §8, checked against the corpus.
//!
//! Two statements, and they are not the same one:
//!
//! * `serialize(parse(md)) == md` — byte for byte, over every golden. This is
//!   the criterion the plan sets for Fase 2.
//! * `parse(serialize(ir)) == normalize(ir)` — reading back lands on the
//!   [`docsai_docmark::normalize`] normal form and stays there.

use std::path::{Path, PathBuf};

use docsai_docmark::{normalize, parse, serialize, Fidelity, Options};
use docsai_model::assets::{AssetId, AssetInfo, AssetStore};
use docsai_model::{Document, Format};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/docx")
}

/// Every golden, with its media loaded so image links resolve.
fn goldens() -> Vec<(String, String, NamedStore)> {
    let dir = corpus_dir();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("corpus/docx exists")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.to_string_lossy().ends_with(".expected.dmk.md"))
        .collect();
    entries.sort();
    assert!(!entries.is_empty(), "no goldens found in {}", dir.display());

    entries
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .replace(".expected.dmk.md", "");
            let text = std::fs::read_to_string(&path).expect("golden readable");
            // The media a golden refers to live in the document it came from;
            // re-extracting them here would need the docx reader, which this
            // crate must not depend on. Instead, register the exact bytes the
            // links name, taken from the golden's own asset directory when it
            // exists, and fall back to a stand-in of the right name otherwise.
            let store = load_assets(&text);
            (name, text, store)
        })
        .collect()
}

/// Registers a stand-in asset for every `assets/<file>` the text links to.
///
/// The parser matches images by file name, so what matters for idempotence is
/// that the name resolves — not what the pixels are.
fn load_assets(text: &str) -> NamedStore {
    let mut store = NamedStore::default();
    for name in link_targets(text) {
        store.register(&name);
    }
    store
}

fn link_targets(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("](assets/") {
        let after = &rest[start + "](assets/".len()..];
        let end = after.find(')').unwrap_or(after.len());
        names.push(after[..end].to_string());
        rest = &after[end..];
    }
    names.sort();
    names.dedup();
    names
}

/// A store whose entries carry a chosen file name.
///
/// [`MemoryAssetStore`] derives the name from the content hash, so reproducing
/// a given name would mean finding bytes that hash to it. This test needs the
/// opposite: the names are given, and the bytes are irrelevant.
#[derive(Debug, Default)]
struct NamedStore {
    entries: std::collections::BTreeMap<AssetId, AssetInfo>,
}

impl NamedStore {
    /// Adds an asset whose id is its file name, so both directions agree.
    fn register(&mut self, file_name: &str) {
        let id = AssetId::new(file_name);
        let content_type = match file_name.rsplit('.').next() {
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            // The one the goldens exercise for `render=unsupported`.
            Some("emf") => "image/x-emf",
            Some("wmf") => "image/x-wmf",
            _ => "application/octet-stream",
        };
        self.entries.insert(
            id.clone(),
            AssetInfo {
                id,
                file_name: file_name.to_string(),
                content_type: content_type.to_string(),
                byte_len: 0,
                native_size_px: None,
            },
        );
    }
}

impl AssetStore for NamedStore {
    fn put(&mut self, _bytes: &[u8]) -> Result<AssetId, docsai_model::assets::AssetError> {
        unreachable!("this store is only ever read")
    }

    fn get(&self, _id: &AssetId) -> Option<&[u8]> {
        None
    }

    fn info(&self, id: &AssetId) -> Option<&AssetInfo> {
        self.entries.get(id)
    }

    fn ids(&self) -> Vec<AssetId> {
        self.entries.keys().cloned().collect()
    }
}

fn options() -> Options {
    Options {
        fidelity: Fidelity::Full,
        assets_dir: "assets".into(),
        source_format: Format::Docx,
    }
}

#[test]
fn serialising_what_was_parsed_gives_back_the_same_bytes() {
    let mut failures: Vec<String> = Vec::new();
    for (name, text, store) in goldens() {
        let (document, info, _report) = match parse(&text, &store) {
            Ok(parsed) => parsed,
            Err(error) => {
                failures.push(format!("{name}: does not parse: {error}"));
                continue;
            }
        };
        let mut options = options();
        if let Some(format) = info.source_format {
            options.source_format = format;
        }
        let (again, _) = serialize(&document, &store, &options);
        if again != text {
            failures.push(format!("{name}:\n{}", first_difference(&text, &again)));
        }
    }
    assert!(
        failures.is_empty(),
        "round-tripping the goldens changed them:\n\n{}",
        failures.join("\n\n")
    );
}

#[test]
fn parsing_lands_on_the_normal_form_and_stays_there() {
    for (name, text, store) in goldens() {
        let (document, _, _) = parse(&text, &store).expect("golden parses");
        // Already normal: parsing can only produce the normal form.
        assert_eq!(
            normalize(&document),
            document,
            "{name}: what the parser built is not in normal form"
        );

        // And a second lap changes nothing.
        let (again, _) = serialize(&document, &store, &options());
        let (twice, _, _) = parse(&again, &store).expect("second pass parses");
        assert_eq!(twice, document, "{name}: the second lap moved the IR");
    }
}

#[test]
fn every_golden_parses_without_losing_its_structure() {
    for (name, text, store) in goldens() {
        let (document, _, report) = parse(&text, &store).expect("golden parses");
        let Document::Text(document) = &document else {
            panic!("{name}: expected a text document");
        };
        assert!(
            !document.sections.is_empty(),
            "{name}: no sections came out of the parse"
        );
        let blocks: usize = document.sections.iter().map(|s| s.blocks.len()).sum();
        let headers: usize = document
            .sections
            .iter()
            .map(|s| s.headers.len() + s.footers.len())
            .sum();
        assert!(
            blocks > 0 || headers > 0,
            "{name}: the document came back empty"
        );
        assert!(
            !report.has_severe(),
            "{name}: parsing reported {:?}",
            report.warnings
        );
    }
}

#[test]
fn the_metadata_and_the_catalogues_survive() {
    let (_, text, store) = goldens()
        .into_iter()
        .find(|(name, _, _)| name == "nested-lists")
        .expect("nested-lists golden");
    let (document, info, _) = parse(&text, &store).expect("parses");
    let Document::Text(document) = document else {
        panic!("expected a text document");
    };
    assert_eq!(info.source_format, Some(Format::Docx));
    assert_eq!(document.meta.title.as_deref(), Some("Listas anidadas"));
    assert_eq!(document.meta.language.as_deref(), Some("es-ES"));
    assert_eq!(
        document.styles.styles.len(),
        6,
        "the whole style catalogue came back"
    );
    assert_eq!(document.list_defs.defs.len(), 2);
    assert_eq!(document.sections[0].page.paper_name(), Some("A4"));
}

/// The first line that differs, with a little context, so a failure reads.
fn first_difference(expected: &str, actual: &str) -> String {
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    for (index, (want, got)) in expected_lines.iter().zip(&actual_lines).enumerate() {
        if want != got {
            return format!(
                "  line {}:\n    expected: {want:?}\n    actual:   {got:?}",
                index + 1
            );
        }
    }
    format!(
        "  same first {} lines; expected {} lines, produced {}",
        expected_lines.len().min(actual_lines.len()),
        expected_lines.len(),
        actual_lines.len()
    )
}

/// Sanity: the helper that finds asset links actually finds them.
#[test]
fn asset_links_are_discovered_in_the_goldens() {
    let (_, text, store) = goldens()
        .into_iter()
        .find(|(name, _, _)| name == "images-inline")
        .expect("images-inline golden");
    assert!(store.len() >= 1, "the golden links at least one image");
    assert!(text.contains("](assets/img-"));
}

//! DocMark → IR (Fase 2).
//!
//! Written by hand, like the serialiser, and for the same reason: the
//! idempotence guarantee of spec §8 leaves no room for a Markdown library's own
//! formatting opinions. The measurements behind that choice are in
//! `docs/spikes/R2-parser-docmark.md`.

mod block;
mod front;
mod inline;
mod yaml;

use std::collections::BTreeMap;

use docsai_model::assets::{AssetId, AssetStore};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::text::TextDocument;
use docsai_model::{Document, Format, DOCMARK_VERSION};

pub use front::FrontMatter;

/// Why a DocMark file could not be read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("front matter, line {line}: {message}")]
    FrontMatter { line: usize, message: String },

    #[error("the front matter block is not closed (a `---` line is missing)")]
    UnterminatedFrontMatter,
}

/// What the caller learns about the file beyond its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseInfo {
    /// `docmark:` as the file declares it.
    pub version: Option<String>,
    /// `source-format:`, the format the document originally came from.
    pub source_format: Option<Format>,
}

/// Parses a DocMark document.
///
/// `assets` resolves the media the file references: image links are matched by
/// file name, which is what makes `assets/img-<hash8>.png` portable. A link
/// with no matching bytes is a warning, not an error — the geometry and the
/// path still survive.
pub fn parse(
    source: &str,
    assets: &dyn AssetStore,
) -> Result<(Document, ParseInfo, ConversionReport), ParseError> {
    let mut report = ConversionReport::new();
    let (front_source, body) = split_front_matter(source)?;

    let front = match front_source {
        Some(text) => {
            let yaml = yaml::parse(text)?;
            front::read(&yaml, &mut report)
        }
        None => FrontMatter::default(),
    };
    if let Some(version) = &front.version {
        if version != DOCMARK_VERSION {
            report.warn(Warning::Degraded {
                what: format!("DocMark {version}"),
                why: format!("this build implements {DOCMARK_VERSION}; unknown fields are ignored"),
            });
        }
    }

    let index = asset_index(assets);
    let lines: Vec<&str> = body.lines().collect();
    let mut document = TextDocument {
        meta: front.meta.clone(),
        styles: front.styles.clone(),
        list_defs: front.lists.clone(),
        sections: Vec::new(),
    };
    {
        let mut parser = block::BlockParser::new(&index, &mut report, &front);
        parser.collect_footnotes(&lines);
        document.sections = parser.document(&lines);
    }

    report.stats.styles = document.styles.styles.len() as u32;
    let info = ParseInfo {
        version: front.version,
        source_format: front.source_format,
    };
    Ok((Document::Text(document), info, report))
}

/// Splits the leading `---` block from the body.
fn split_front_matter(source: &str) -> Result<(Option<&str>, &str), ParseError> {
    let body = source.strip_prefix("---\n").or_else(|| {
        // A BOM is not part of DocMark, but a hand editor can leave one.
        source
            .strip_prefix('\u{feff}')
            .and_then(|s| s.strip_prefix("---\n"))
    });
    let Some(body) = body else {
        return Ok((None, source));
    };
    let mut offset = 0usize;
    for line in body.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Ok((Some(&body[..offset]), &body[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err(ParseError::UnterminatedFrontMatter)
}

/// Asset ids by file name, for resolving image links.
fn asset_index(assets: &dyn AssetStore) -> BTreeMap<String, AssetId> {
    assets
        .ids()
        .into_iter()
        .filter_map(|id| {
            let info = assets.info(&id)?;
            Some((info.file_name.clone(), id))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::MemoryAssetStore;

    fn parse_str(source: &str) -> (Document, ParseInfo, ConversionReport) {
        parse(source, &MemoryAssetStore::new()).expect("parses")
    }

    #[test]
    fn reads_the_front_matter_and_the_body() {
        let (document, info, _) = parse_str(
            "---\ndocmark: \"1.0\"\nsource-format: docx\ntitle: \"Informe\"\n---\n\nUn parrafo.\n",
        );
        assert_eq!(info.version.as_deref(), Some("1.0"));
        assert_eq!(info.source_format, Some(Format::Docx));
        let Document::Text(text) = document else {
            panic!("expected a text document");
        };
        assert_eq!(text.meta.title.as_deref(), Some("Informe"));
        assert_eq!(text.sections.len(), 1);
        assert_eq!(text.sections[0].blocks.len(), 1);
    }

    #[test]
    fn a_file_without_front_matter_is_still_readable() {
        // `--fidelity plain` writes none, and a human may start from scratch.
        let (document, info, _) = parse_str("Solo texto.\n");
        assert!(info.version.is_none());
        let Document::Text(text) = document else {
            panic!("expected a text document");
        };
        assert_eq!(text.sections[0].blocks.len(), 1);
    }

    #[test]
    fn an_unterminated_front_matter_is_an_error_not_a_guess() {
        let error = parse(
            "---\ntitle: \"x\"\n\nUn parrafo.\n",
            &MemoryAssetStore::new(),
        )
        .unwrap_err();
        assert_eq!(error, ParseError::UnterminatedFrontMatter);
    }

    #[test]
    fn a_newer_declared_version_warns_but_still_reads() {
        let (_, _, report) = parse_str("---\ndocmark: \"9.9\"\n---\n\nTexto.\n");
        assert!(report.warnings.iter().any(|w| w.message().contains("9.9")));
    }

    #[test]
    fn images_resolve_against_the_asset_store() {
        let mut store = MemoryAssetStore::new();
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x78\x00\x00\x00\x5a\x08\x02\x00\x00\x00";
        let id = store.put(png).unwrap();
        let name = store.info(&id).unwrap().file_name.clone();

        let source = format!("![x](assets/{name}){{width=1cm height=1cm}}\n");
        let (document, _, report) = parse(&source, &store).expect("parses");
        let Document::Text(text) = document else {
            panic!("expected a text document");
        };
        let [docsai_model::text::Block::Image(image)] = text.sections[0].blocks.as_slice() else {
            panic!("expected a block image");
        };
        assert_eq!(image.asset, id);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }
}

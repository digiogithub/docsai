//! DocMark: the extended-Markdown pivot format.
//!
//! Fase 1 lands the **serialiser** (IR → DocMark). The parser (DocMark → IR)
//! is Fase 2 and deliberately absent here.
//!
//! The output is normative and deterministic (spec §8): two serialisations of
//! the same IR are byte-identical, line endings are always `\n`, and the
//! encoding is UTF-8 without BOM.
//!
//! ```
//! use docsai_docmark::{serialize, Options};
//! use docsai_model::{Document, MemoryAssetStore, text::TextDocument};
//!
//! let doc = Document::Text(TextDocument::default());
//! let (markdown, _report) = serialize(&doc, &MemoryAssetStore::new(), &Options::default());
//! assert!(markdown.starts_with("---\ndocmark: \"1.0\"\n"));
//! ```

#![forbid(unsafe_code)]

pub mod attrs;
pub mod escape;
mod frontmatter;
pub mod units;
mod writer;

use docsai_model::assets::AssetStore;
use docsai_model::{ConversionReport, Document, Format};

/// How much of the IR reaches the output (spec §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fidelity {
    /// Everything: attributes, catalogues and raw-blocks. Round-trip grade.
    #[default]
    Full,
    /// The main attributes, without catalogues or raw-blocks.
    Standard,
    /// Plain CommonMark + GFM, for LLM and RAG consumption.
    Plain,
}

impl Fidelity {
    pub fn as_str(self) -> &'static str {
        match self {
            Fidelity::Full => "full",
            Fidelity::Standard => "standard",
            Fidelity::Plain => "plain",
        }
    }

    /// Parses a `--fidelity` value.
    pub fn parse(value: &str) -> Option<Fidelity> {
        match value.trim().to_ascii_lowercase().as_str() {
            "full" => Some(Fidelity::Full),
            "standard" => Some(Fidelity::Standard),
            "plain" => Some(Fidelity::Plain),
            _ => None,
        }
    }
}

impl std::fmt::Display for Fidelity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Serialisation settings.
#[derive(Debug, Clone)]
pub struct Options {
    pub fidelity: Fidelity,
    /// Directory the image links point at, relative to the `.dmk.md` file.
    pub assets_dir: String,
    /// The format the document came from, recorded in the front matter.
    pub source_format: Format,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            fidelity: Fidelity::Full,
            assets_dir: "assets".into(),
            source_format: Format::Docx,
        }
    }
}

/// Serialises a document to DocMark.
pub fn serialize(
    doc: &Document,
    assets: &dyn AssetStore,
    options: &Options,
) -> (String, ConversionReport) {
    let mut out = String::new();
    frontmatter::write(&mut out, doc, options.source_format, options.fidelity);

    match doc {
        Document::Text(text) => {
            let writer = writer::Writer::new(options, &text.styles, assets);
            let (body, mut report) = writer.finish(text);
            out.push_str(&body);
            report.stats.styles = text.styles.styles.len() as u32;
            (out, report)
        }
        Document::Workbook(_) => {
            // Spreadsheet serialisation is Fase 3; emitting a half-formed sheet
            // would be worse than saying so.
            let mut report = ConversionReport::new();
            report.warn(docsai_model::Warning::Degraded {
                what: "workbook".into(),
                why: "spreadsheet serialisation arrives in Fase 3".into(),
            });
            (out, report)
        }
    }
}

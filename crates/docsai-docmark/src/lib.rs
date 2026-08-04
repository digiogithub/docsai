//! DocMark: the extended-Markdown pivot format.
//!
//! Serialiser (IR → DocMark) and parser (DocMark → IR). The output is normative
//! and deterministic (spec §8): two serialisations of the same IR are
//! byte-identical, line endings are always `\n`, and the encoding is UTF-8
//! without BOM.
//!
//! ```
//! use docsai_docmark::{serialize, parse, Options};
//! use docsai_model::{Document, MemoryAssetStore, text::TextDocument};
//!
//! let doc = Document::Text(TextDocument::default());
//! let (markdown, _report) = serialize(&doc, &MemoryAssetStore::new(), &Options::default());
//! assert!(markdown.starts_with("---\ndocmark: \"1.0\"\n"));
//! let mut assets = MemoryAssetStore::new();
//! let (parsed, _) = parse(&markdown, &mut assets).unwrap();
//! assert!(parsed.is_text());
//! ```

#![forbid(unsafe_code)]

pub mod attrs;
mod dict;
mod error;
pub mod escape;
mod frontmatter;
mod frontmatter_parse;
mod ids;
mod parser;
pub mod raw;
mod sheet_parser;
mod sheet_writer;
pub mod units;
mod writer;
mod yaml;

pub use error::ParseError;
pub use ids::NodeFragment;
pub use parser::{parse, parse_with_base};

use docsai_model::addressing::IdPolicy;
use docsai_model::assets::AssetStore;
use docsai_model::{ConversionReport, Document, Format};

use std::collections::BTreeSet;

use crate::dict::AttrDict;
use crate::ids::IdSource;

/// How much of the IR reaches the output (spec §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fidelity {
    /// Everything: attributes, catalogues and raw-blocks. Round-trip grade.
    #[default]
    Full,
    /// A projection for agents (spec §6.1): the text, the structure and every
    /// node address, and nothing an agent cannot edit — no catalogues, no
    /// measurements, no raw payloads. Read whole, written back node by node.
    Agent,
    /// The main attributes, without catalogues or raw-blocks.
    Standard,
    /// Plain CommonMark + GFM, for LLM and RAG consumption.
    Plain,
}

impl Fidelity {
    pub fn as_str(self) -> &'static str {
        match self {
            Fidelity::Full => "full",
            Fidelity::Agent => "agent",
            Fidelity::Standard => "standard",
            Fidelity::Plain => "plain",
        }
    }

    /// Parses a `--fidelity` value.
    pub fn parse(value: &str) -> Option<Fidelity> {
        match value.trim().to_ascii_lowercase().as_str() {
            "full" => Some(Fidelity::Full),
            "agent" => Some(Fidelity::Agent),
            "standard" => Some(Fidelity::Standard),
            "plain" => Some(Fidelity::Plain),
            _ => None,
        }
    }

    /// Whether appearance reaches the output: measurements, colours, geometry,
    /// column widths. `agent` keeps the *names* of things and drops the rest,
    /// which is the whole reason it is cheaper than `standard` on a document
    /// whose cost is its formatting.
    pub fn formatting(self) -> bool {
        matches!(self, Fidelity::Full | Fidelity::Standard)
    }

    /// Whether the level addresses nodes by default. `agent` output exists to
    /// be written back node by node, so an id on every node is not optional.
    pub fn addresses(self) -> bool {
        matches!(self, Fidelity::Full | Fidelity::Agent)
    }
}

impl std::fmt::Display for Fidelity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where the bytes of a raw-block live (spec §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RawPolicy {
    /// In the body, inside a fenced code block. Self-contained, and the reason
    /// a document with heavy raw fragments costs what it costs to read.
    Inline,
    /// In `<assets>/_raw/<id>.xml`, referenced by `src=`. The body keeps the
    /// stub, so an agent sees that something is there without paying for it.
    #[default]
    Sidecar,
}

impl RawPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            RawPolicy::Inline => "inline",
            RawPolicy::Sidecar => "sidecar",
        }
    }

    /// Parses a `--raw` value.
    pub fn parse(value: &str) -> Option<RawPolicy> {
        match value.trim().to_ascii_lowercase().as_str() {
            "inline" => Some(RawPolicy::Inline),
            "sidecar" => Some(RawPolicy::Sidecar),
            _ => None,
        }
    }
}

impl std::fmt::Display for RawPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Serialisation settings.
#[derive(Debug, Clone)]
pub struct Options {
    pub fidelity: Fidelity,
    /// What happens to node ids (spec §11.1). `plain` output never carries
    /// them whatever this says: it is CommonMark only.
    pub ids: IdPolicy,
    /// Directory the image links point at, relative to the `.dmk.md` file.
    pub assets_dir: String,
    /// The format the document came from, recorded in the front matter.
    pub source_format: Format,
    /// Where raw-block bytes go (spec §7). Only `full` emits raw-blocks at
    /// all, so this is inert at the lossy levels.
    pub raw: RawPolicy,
    /// Decimals a readable unit may use before a length falls back to `emu`
    /// (spec §2). Raising it buys readable units for lengths that two decimals
    /// cannot name exactly; it never rounds, so it never costs accuracy.
    pub precision: u8,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            fidelity: Fidelity::Full,
            ids: IdPolicy::Assign,
            assets_dir: "assets".into(),
            source_format: Format::Docx,
            raw: RawPolicy::default(),
            precision: docsai_model::units::DEFAULT_PRECISION,
        }
    }
}

/// Serialises a document to DocMark.
///
/// The body is rendered first: it is what allocates node ids, and the front
/// matter has to declare the resulting `next-id` (and the format version the
/// ids imply).
pub fn serialize(
    doc: &Document,
    assets: &dyn AssetStore,
    options: &Options,
) -> (String, ConversionReport) {
    let (markdown, report, _) = run(doc, assets, options, false);
    (markdown, report)
}

/// Serialises and reports what each addressable node wrote.
///
/// Same output as [`serialize`], byte for byte — the fragments are a view of
/// the run, not a different one — plus the DocMark each addressed node
/// contributed. That is what makes a node's cost measurable (`docsai tokens`)
/// instead of guessed from its text length.
///
/// Fragments come out innermost first — a node is reported when it is
/// finished, so a footnote arrives before the paragraph containing it — and
/// nested ones overlap: node costs deliberately add up to more than the
/// document. Nodes without an id contribute no fragment, so a run under
/// [`IdPolicy::Never`] (or at `plain`) reports nothing.
pub fn serialize_traced(
    doc: &Document,
    assets: &dyn AssetStore,
    options: &Options,
) -> (String, ConversionReport, Vec<NodeFragment>) {
    run(doc, assets, options, true)
}

fn run(
    doc: &Document,
    assets: &dyn AssetStore,
    options: &Options,
    trace: bool,
) -> (String, ConversionReport, Vec<NodeFragment>) {
    // `plain` is CommonMark and carries no attributes at all (spec §6).
    let policy = if options.fidelity == Fidelity::Plain {
        IdPolicy::Never
    } else {
        options.ids
    };
    // Pass one counts repeated attribute patterns and writes exactly the bytes
    // a run without a dictionary would write (spec §3.7). Only if it finds
    // something worth naming does the body get rendered again, through the
    // dictionary this time — so the common document, which repeats nothing,
    // still pays a single pass.
    let mut dict = if dictionary_applies(doc, options) {
        AttrDict::collecting()
    } else {
        AttrDict::off()
    };
    let (mut ids, mut body, mut report) = render_body(doc, assets, options, policy, trace, &dict);
    if dict.is_collecting() {
        dict = dict.build(&taken_class_names(doc));
        if !dict.is_empty() {
            let second = render_body(doc, assets, options, policy, trace, &dict);
            ids = second.0;
            body = second.1;
            report = second.2;
        }
    }

    let mut out = String::new();
    frontmatter::write(&mut out, doc, options, ids.next_id(), &dict);
    out.push_str(&body);
    (out, report, ids.into_fragments())
}

fn render_body(
    doc: &Document,
    assets: &dyn AssetStore,
    options: &Options,
    policy: IdPolicy,
    trace: bool,
    dict: &AttrDict,
) -> (IdSource, String, ConversionReport) {
    let mut ids = if trace {
        IdSource::traced(doc, policy)
    } else {
        IdSource::new(doc, policy)
    };
    let (body, report) = match doc {
        Document::Text(text) => {
            let writer = writer::Writer::new(options, &text.styles, assets, &mut ids, dict);
            let (body, mut report) = writer.finish(text);
            report.stats.styles = text.styles.styles.len() as u32;
            (body, report)
        }
        Document::Workbook(book) => {
            let (body, mut report) = sheet_writer::write_workbook(book, assets, options, &mut ids);
            report.stats.styles = book.styles.styles.len() as u32;
            (body, report)
        }
    };
    (ids, body, report)
}

/// Whether this document, at this level, gets a dictionary.
///
/// Only where there is formatting to compress, and only for text documents: a
/// sheet already compacts identical cell metadata into ranges
/// (`sheet_writer::collect_cell_meta`), so a dictionary over it would be a
/// second answer to a question already answered.
fn dictionary_applies(doc: &Document, options: &Options) -> bool {
    doc.is_text() && options.fidelity.formatting()
}

/// Every class name the document already means something by. A generated
/// dictionary name landing on one of these would change what a node is.
fn taken_class_names(doc: &Document) -> BTreeSet<String> {
    let mut taken: BTreeSet<String> = dict::RESERVED_CLASSES
        .iter()
        .map(|c| (*c).to_string())
        .collect();
    if let Document::Text(text) = doc {
        taken.extend(text.styles.styles.keys().map(|id| id.as_str().to_string()));
        taken.extend(text.list_defs.defs.keys().map(|id| id.to_string()));
    }
    taken
}

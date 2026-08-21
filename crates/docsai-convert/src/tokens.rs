//! Measured token cost of a document (plan v2, Phase 10).
//!
//! An agent pays for the document in tokens, so the budget has to be a real
//! number: this module counts with a BPE tokenizer over the DocMark that would
//! actually be written, never over an estimate like `bytes / 4`. The tokenizer
//! choice (`tiktoken-rs`, `o200k_base`) and its alternatives are recorded in
//! `docs/technical-analysis.md` §4.4.
//!
//! Per-node costs come from [`docsai_docmark::serialize_traced`], which reports
//! the exact DocMark each addressed node contributed. They **nest**: a section's
//! fragment contains its headings', so the node costs deliberately add up to
//! more than the document total.

use std::path::Path;

use docsai_docmark::{serialize_traced, Fidelity, NodeFragment, Options as DocMarkOptions};
use docsai_model::addressing::IdPolicy;
use docsai_model::assets::AssetStore;
use docsai_model::{Document, Format, NodeId, NodeKind};
use serde::Serialize;

use crate::assets::DirAssetStore;
use crate::pipeline::{read_path_with_options, ConvertOptions};
use crate::service::{with_scratch_document, SourceInput};
use crate::ConvertError;

/// The encoding every count in this crate uses.
///
/// Named in the report on purpose: changing tokenizer changes every number, and
/// that has to be visible in the diff of the token golden, never silent.
pub const ENCODING: &str = "o200k_base";

/// Characters of node text kept as a preview.
const PREVIEW_CHARS: usize = 60;

/// Counts the tokens of a string.
pub fn count(text: &str) -> usize {
    tiktoken_rs::o200k_base_singleton().count_ordinary(text)
}

/// What one addressable node costs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct NodeTokens {
    pub id: NodeId,
    pub kind: NodeKind,
    pub tokens: usize,
    pub bytes: usize,
    /// First line of the node's own text, for recognising it in a listing.
    pub preview: String,
}

/// The cost of a document at one fidelity level.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct TokenReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub source_format: String,
    pub fidelity: String,
    pub encoding: &'static str,
    /// Whole file: front matter and body.
    pub total: usize,
    pub front_matter: usize,
    pub body: usize,
    pub bytes: usize,
    /// Per-node costs, innermost first. Nested nodes are counted twice on
    /// purpose (see the module docs), so this does not sum to `total`.
    pub nodes: Vec<NodeTokens>,
}

impl TokenReport {
    /// The heaviest nodes first, at most `limit` of them.
    pub fn heaviest(&self, limit: usize) -> Vec<&NodeTokens> {
        let mut nodes: Vec<&NodeTokens> = self.nodes.iter().collect();
        nodes.sort_by(|a, b| b.tokens.cmp(&a.tokens).then(a.id.0.cmp(&b.id.0)));
        nodes.truncate(limit);
        nodes
    }
}

/// Counts an in-memory document without touching the filesystem.
pub fn token_report(
    doc: &Document,
    assets: &dyn AssetStore,
    options: &DocMarkOptions,
) -> TokenReport {
    let (markdown, _, fragments) = serialize_traced(doc, assets, options);
    report_from(
        &markdown,
        &fragments,
        options.fidelity,
        options.source_format,
    )
}

/// Reads `input` and reports what it would cost as DocMark.
pub fn token_report_path(
    input: &Path,
    options: &ConvertOptions,
) -> Result<TokenReport, ConvertError> {
    // Media never reach the output — only their links do — so a throwaway
    // directory is enough to satisfy the asset store.
    let dir = tempfile::tempdir().map_err(|source| ConvertError::Io {
        path: input.display().to_string(),
        source,
    })?;
    let mut assets = DirAssetStore::new(dir.path());
    let (doc, source_format, _) = read_path_with_options(input, &mut assets, options)?;
    let docmark = DocMarkOptions {
        fidelity: options.fidelity,
        ids: options.id_policy(),
        assets_dir: "assets".into(),
        source_format,
        raw: options.raw,
        precision: options.precision,
        dictionary: true,
    };
    let mut report = token_report(&doc, &assets, &docmark);
    report.path = Some(input.display().to_string());
    Ok(report)
}

/// Every fidelity level, in the order an agent should consider them.
pub const FIDELITY_LEVELS: [Fidelity; 4] = [
    Fidelity::Full,
    Fidelity::Agent,
    Fidelity::Standard,
    Fidelity::Plain,
];

/// What one fidelity level costs, without the per-node detail.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct FidelityCost {
    pub fidelity: &'static str,
    pub total: usize,
    pub front_matter: usize,
    pub body: usize,
    pub bytes: usize,
    /// Addressable nodes at this level; `0` where the level writes no ids, and
    /// therefore where `read_selection` has nothing to address.
    pub nodes: usize,
}

/// What a document costs at every fidelity level.
///
/// The one call that answers "can I afford to read this, and at which level" —
/// a few dozen tokens against a document of any size. Reading happens once and
/// the four levels are serialized from the same IR, so this costs one parse,
/// not four.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct TokenBudget {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub source_format: String,
    pub encoding: &'static str,
    pub levels: Vec<FidelityCost>,
}

impl TokenBudget {
    /// The form an agent reads: one line per level.
    pub fn render_text(&self) -> String {
        let mut out = format!("{} {}\n", self.source_format, self.encoding);
        for level in &self.levels {
            out.push_str(&format!(
                "{} {} tokens {} bytes {} nodes\n",
                level.fidelity, level.total, level.bytes, level.nodes
            ));
        }
        out
    }
}

/// Costs a path or in-memory document at every fidelity level (the MCP
/// `estimate_tokens` tool).
pub fn token_budget_input(
    source: SourceInput<'_>,
    options: &ConvertOptions,
) -> Result<TokenBudget, ConvertError> {
    let (mut budget, label) = with_scratch_document(source, options, true, |doc, assets, base| {
        let levels = FIDELITY_LEVELS
            .iter()
            .map(|&fidelity| {
                let level_options = DocMarkOptions {
                    fidelity,
                    // The per-fidelity default, not the caller's override: the
                    // point of the table is what each level *costs as itself*.
                    ids: if fidelity.addresses() {
                        IdPolicy::Assign
                    } else {
                        IdPolicy::Never
                    },
                    ..base.clone()
                };
                let report = token_report(doc, assets, &level_options);
                FidelityCost {
                    fidelity: fidelity.as_str(),
                    total: report.total,
                    front_matter: report.front_matter,
                    body: report.body,
                    bytes: report.bytes,
                    nodes: report.nodes.len(),
                }
            })
            .collect();
        TokenBudget {
            path: None,
            source_format: base.source_format.as_str().to_string(),
            encoding: ENCODING,
            levels,
        }
    })?;
    budget.path = label;
    Ok(budget)
}

fn report_from(
    markdown: &str,
    fragments: &[NodeFragment],
    fidelity: Fidelity,
    source_format: Format,
) -> TokenReport {
    let body_start = front_matter_end(markdown);
    let front_matter = count(&markdown[..body_start]);
    let body = count(&markdown[body_start..]);
    TokenReport {
        path: None,
        source_format: source_format.as_str().to_string(),
        fidelity: fidelity.as_str().to_string(),
        encoding: ENCODING,
        // Counted whole rather than as `front_matter + body`: BPE merges
        // across the boundary, so the parts do not have to add up.
        total: count(markdown),
        front_matter,
        body,
        bytes: markdown.len(),
        nodes: fragments
            .iter()
            .map(|fragment| NodeTokens {
                id: fragment.id.clone(),
                kind: fragment.kind,
                tokens: count(&fragment.markdown),
                bytes: fragment.markdown.len(),
                preview: preview(&fragment.markdown),
            })
            .collect(),
    }
}

/// Offset of the first byte after the front matter, or 0 when there is none.
pub(crate) fn front_matter_end(markdown: &str) -> usize {
    let Some(rest) = markdown.strip_prefix("---\n") else {
        return 0;
    };
    match rest.find("\n---\n") {
        Some(offset) => "---\n".len() + offset + "\n---\n".len(),
        None => 0,
    }
}

/// One line of the node's text, whitespace collapsed and cut to
/// [`PREVIEW_CHARS`].
///
/// The machinery is stripped first — attribute blocks, fenced-div markers and
/// table rules. A preview is there to be *recognised* by a reader who already
/// has the id next to it; repeating `{#n4 .ListParagraph list=L1}` would cost
/// tokens to say what the id column says better.
pub(crate) fn preview(markdown: &str) -> String {
    let stripped = strip_machinery(markdown);
    let mut text = String::new();
    let mut spaced = true;
    for c in stripped.chars() {
        if c.is_whitespace() {
            if !spaced {
                text.push(' ');
                spaced = true;
            }
            continue;
        }
        text.push(c);
        spaced = false;
        if text.chars().count() > PREVIEW_CHARS {
            break;
        }
    }
    let text = text.trim_end();
    if text.chars().count() > PREVIEW_CHARS {
        let cut: String = text.chars().take(PREVIEW_CHARS - 1).collect();
        format!("{cut}…")
    } else {
        text.to_string()
    }
}

/// Drops what DocMark writes for machines: `:::` fences, attribute blocks and
/// the `|---|` rule of a table.
pub(crate) fn strip_machinery(markdown: &str) -> String {
    let mut out = String::new();
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(":::") || is_table_rule(trimmed) {
            continue;
        }
        let mut depth = 0usize;
        for c in line.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ if depth == 0 => out.push(c),
                _ => {}
            }
        }
        out.push('\n');
    }
    out
}

/// `| --- | ---: |` and friends, which say nothing about the content.
fn is_table_rule(line: &str) -> bool {
    line.starts_with('|')
        && line
            .chars()
            .all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counting_is_a_real_tokenizer_not_a_byte_estimate() {
        // "indivisible" is one word but several tokens; 4 bytes per token
        // would put it at ~3, the tokenizer knows better than that.
        assert!(count("indivisible") >= 2);
        assert_eq!(count(""), 0);
    }

    #[test]
    fn front_matter_is_split_off_the_body() {
        let markdown = "---\ndocmark: \"1.0\"\n---\n\n# Title\n";
        let end = front_matter_end(markdown);
        assert_eq!(&markdown[end..], "\n# Title\n");
        assert_eq!(front_matter_end("# No front matter\n"), 0);
    }

    #[test]
    fn previews_are_one_short_line() {
        assert_eq!(preview("# Title  {#n1}\n\nmore"), "# Title more");
        assert_eq!(
            preview("::: {.table}\n| a | b |\n|---|---|\n| 1 | 2 |\n:::"),
            "| a | b | | 1 | 2 |",
            "fences and rules are machinery, the cells are the content"
        );
        let long = "word ".repeat(40);
        let preview = preview(&long);
        assert!(preview.chars().count() <= PREVIEW_CHARS, "{preview}");
        assert!(preview.ends_with('…'), "{preview}");
    }
}

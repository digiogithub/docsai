//! Finding text in a document without reading it (plan v2, Phase 11, task 7).
//!
//! `outline` says what a document contains; `read --select` returns a part of
//! it. Between the two sits the question an agent actually asks: *where does it
//! say "revenue"?* Answering with the document defeats the point, so this
//! answers with **an address plus the words around the match**.
//!
//! Four rules shape the result:
//!
//! - **The unit is the DocMark block**, not the addressed node. Ordinary prose
//!   paragraphs carry no id — by design, spec §11.1: "ordinary prose is reached
//!   by relative path" — so a search that only looked at addressed nodes would
//!   find nothing in a report that is mostly prose. Searching the text a
//!   conversion would write finds everything the document says.
//! - **An address is what the document gives.** A block that carries an id is
//!   reported at that id ([`Location::Node`]), with the selector that reads it;
//!   a block that carries none is reported **relative to the last id before
//!   it** ([`Location::Relative`], `n12.b2`), which is the notation the spec
//!   already defines for exactly this case.
//! - **A hit is a handle, not content.** The context window is short on
//!   purpose: enough to recognise the match, never enough to serve as a copy of
//!   the document.
//! - **A footnote is reported at the block that refers to it.** Its text is in
//!   the document and has to be findable, but a footnote cannot be selected on
//!   its own (see [`mod@crate::select`]), so its selector names the referring block
//!   rather than an id `read` would refuse.
//!
//! What this does *not* do is close the loop for a relative hit: `read
//! --select` has no `.bN` term, because returning an unaddressed paragraph
//! needs a body it cannot stitch from fragments. Search says so in the hit
//! rather than handing back an address that would not read.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use docsai_docmark::{serialize_traced, NodeFragment, Options as DocMarkOptions};
use docsai_model::addressing::Etag;
use docsai_model::assets::AssetStore;
use docsai_model::{Document, NodeId, NodeKind};
use serde::Serialize;

use crate::pipeline::ConvertOptions;
use crate::select::document_order;
use crate::service::{with_scratch_document, SourceInput};
use crate::tokens::{count, front_matter_end, strip_machinery};
use crate::ConvertError;

/// Characters of context kept either side of a match.
const CONTEXT_CHARS: usize = 48;

/// Snippets reported for one block, however many times it matched.
///
/// A block that says the word twenty times is one place to go and one edit to
/// make; the twentieth snippet costs tokens to repeat what the count says.
const MAX_SNIPPETS: usize = 3;

/// Hits listed before the result only counts the rest.
const DEFAULT_LIMIT: usize = 20;

/// What a query could not be read as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryError(String);

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for QueryError {}

/// What to look for, and how much to say about each match.
///
/// The text is matched **case-insensitively, as a literal**: no globbing, no
/// regular expressions. The plan asks for a query, and a literal is what an
/// agent quoting a phrase out of an outline preview actually has; a pattern
/// language would be a second syntax to get wrong and a dependency to justify,
/// for a case that has not appeared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub text: String,
    /// Characters kept either side of a match.
    pub context: usize,
    /// Hits listed; the rest are counted, not listed. `None` lists all.
    pub limit: Option<usize>,
}

impl Query {
    pub fn new(text: impl Into<String>) -> Result<Self, QueryError> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(QueryError("a query needs something to look for".into()));
        }
        Ok(Query {
            text,
            context: CONTEXT_CHARS,
            limit: Some(DEFAULT_LIMIT),
        })
    }
}

impl FromStr for Query {
    type Err = QueryError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Query::new(text)
    }
}

impl fmt::Display for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// One occurrence, with the words around it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Snippet {
    pub before: String,
    /// The matched text as the document writes it, not as the query does.
    pub matched: String,
    pub after: String,
}

/// Where a match is, in the terms the document can express.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "where", rename_all = "kebab-case")]
pub enum Location {
    /// The block carries an id: an addressed node, readable on its own.
    Node {
        /// Position in document order, 1-based: the `sN` that `outline` prints
        /// and `read --select` accepts.
        position: usize,
        id: NodeId,
        kind: NodeKind,
        etag: Etag,
        /// What reading that node would cost, its children included.
        tokens: usize,
        /// The selector that reads it — `#id`, except for a footnote, where it
        /// is the block that refers to it.
        select: String,
    },
    /// The block carries no id: prose, addressed relative to the last id
    /// before it (spec §11.1).
    Relative {
        /// The id the path counts from; absent before the document's first id.
        #[serde(skip_serializing_if = "Option::is_none")]
        anchor: Option<NodeId>,
        /// Blocks after the anchor, 1-based.
        block: usize,
        /// `n12.b2`, or `.b2` when there is no anchor yet.
        path: String,
        /// What this block costs on its own.
        tokens: usize,
    },
}

/// One block that matched.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Hit {
    #[serde(flatten)]
    pub location: Location,
    /// Occurrences in this block, including the ones no snippet shows.
    pub matches: usize,
    pub snippets: Vec<Snippet>,
}

impl Hit {
    /// The address as it is written for a reader.
    pub fn address(&self) -> String {
        match &self.location {
            Location::Node { position, id, .. } => format!("s{position} #{}", id.0),
            Location::Relative { path, .. } => path.clone(),
        }
    }

    /// The selector that reads this hit, when one exists.
    pub fn select(&self) -> Option<&str> {
        match &self.location {
            Location::Node { select, .. } => Some(select),
            Location::Relative { .. } => None,
        }
    }
}

/// Where a query appears in a document.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct SearchResults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub query: String,
    pub source_format: String,
    pub fidelity: String,
    /// Occurrences found, across every block, before any limit.
    pub matches: usize,
    /// Blocks that matched, before any limit.
    pub blocks: usize,
    /// Hits dropped by `limit`; `0` when everything is listed.
    pub omitted: usize,
    /// What this result costs to read, in its text form.
    pub tokens: usize,
    /// What reading the whole document would have cost, for comparison.
    pub document_tokens: usize,
    pub hits: Vec<Hit>,
}

impl SearchResults {
    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }

    /// The form an agent reads, and the one `tokens` measures.
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        for hit in &self.hits {
            let kind = match &hit.location {
                Location::Node { kind, .. } => kind.as_str(),
                Location::Relative { .. } => "text",
            };
            let tokens = match &hit.location {
                Location::Node { tokens, .. } | Location::Relative { tokens, .. } => tokens,
            };
            out.push_str(&format!(
                "{} {} {} tokens ×{}\n",
                hit.address(),
                kind,
                tokens,
                hit.matches
            ));
            for snippet in &hit.snippets {
                out.push_str(&format!(
                    "  …{}«{}»{}…\n",
                    snippet.before, snippet.matched, snippet.after
                ));
            }
        }
        out
    }
}

/// Searches an in-memory document.
///
/// `options.ids` decides what the hits can be *addressed* by, not what can be
/// found: a level that writes no id still writes text, and a match in it is
/// reported relative to nothing (`.b7`). That is honest — the level chosen made
/// the document unaddressable — and it keeps `search` from silently finding
/// less than the document says.
pub fn search(
    doc: &Document,
    assets: &dyn AssetStore,
    options: &DocMarkOptions,
    query: &Query,
) -> SearchResults {
    let (markdown, _, fragments) = serialize_traced(doc, assets, options);
    let body = &markdown[front_matter_end(&markdown)..];

    let positions: HashMap<&str, usize> = document_order(&fragments)
        .into_iter()
        .enumerate()
        .map(|(position, index)| (fragments[index].id.0.as_str(), position + 1))
        .collect();
    let by_id: HashMap<&str, &NodeFragment> = fragments
        .iter()
        .map(|fragment| (fragment.id.0.as_str(), fragment))
        .collect();
    let footnotes: Vec<&NodeFragment> = fragments
        .iter()
        .filter(|fragment| fragment.kind == NodeKind::Footnote)
        .collect();

    let needle = fold_case(&query.text);
    let mut hits = Vec::new();
    let mut matches = 0usize;
    let mut anchor: Option<NodeId> = None;
    let mut block_number = 0usize;

    for block in blocks(body) {
        // The footnote definitions at the foot of the document carry no id —
        // the reference does — so they are recognised by their marker and
        // resolved back to the node they belong to.
        let id = footnote_marker(block)
            .and_then(|index| footnotes.get(index - 1))
            .map(|fragment| fragment.id.clone())
            .or_else(|| block_id(block));
        match &id {
            Some(id) => {
                anchor = Some(id.clone());
                block_number = 0;
            }
            None => block_number += 1,
        }

        let text = flatten(block);
        let found = occurrences(&text, &needle);
        if found.is_empty() {
            continue;
        }
        matches += found.len();

        let location = match id
            .as_ref()
            .and_then(|id| by_id.get(id.0.as_str()).map(|fragment| (id, *fragment)))
        {
            Some((id, fragment)) => Location::Node {
                position: positions.get(id.0.as_str()).copied().unwrap_or(0),
                id: id.clone(),
                kind: fragment.kind,
                etag: fragment.etag.clone(),
                tokens: count(&fragment.markdown),
                select: select_for(&fragments, fragment),
            },
            None => Location::Relative {
                anchor: anchor.clone(),
                block: block_number,
                path: match &anchor {
                    Some(anchor) => format!("{}.b{block_number}", anchor.0),
                    None => format!(".b{block_number}"),
                },
                tokens: count(block),
            },
        };
        hits.push(Hit {
            matches: found.len(),
            snippets: found
                .iter()
                .take(MAX_SNIPPETS)
                .map(|(start, end)| snippet(&text, *start, *end, query.context))
                .collect(),
            location,
        });
    }

    let blocks = hits.len();
    let omitted = match query.limit {
        Some(limit) if hits.len() > limit => {
            let omitted = hits.len() - limit;
            hits.truncate(limit);
            omitted
        }
        _ => 0,
    };

    let mut results = SearchResults {
        path: None,
        query: query.text.clone(),
        source_format: options.source_format.as_str().to_string(),
        fidelity: options.fidelity.as_str().to_string(),
        matches,
        blocks,
        omitted,
        tokens: 0,
        document_tokens: count(&markdown),
        hits,
    };
    results.tokens = count(&results.render_text());
    results
}

/// Reads `input` and searches it.
pub fn search_path(
    input: &Path,
    options: &ConvertOptions,
    query: &Query,
) -> Result<SearchResults, ConvertError> {
    search_input(SourceInput::Path(input), options, query)
}

/// Searches a path or an in-memory document (the MCP `search_document` tool).
pub fn search_input(
    source: SourceInput<'_>,
    options: &ConvertOptions,
    query: &Query,
) -> Result<SearchResults, ConvertError> {
    // The dictionary rewrites attributes, which the matcher strips anyway;
    // searching the document a conversion would actually write keeps the node
    // costs reported here comparable with `outline`'s.
    let (mut results, label) =
        with_scratch_document(source, options, true, |doc, assets, docmark| {
            search(doc, assets, docmark, query)
        })?;
    results.path = label;
    Ok(results)
}

/// The blocks of a body: runs of non-blank lines, in document order.
fn blocks(body: &str) -> impl Iterator<Item = &str> {
    body.split("\n\n").map(str::trim).filter(|b| !b.is_empty())
}

/// The block-level id of a block, if it declares one.
///
/// The **last** id in the block is the block's own: an attribute block is
/// written after what it describes, so an earlier one belongs to something
/// inline — a footnote reference, an image — that sits inside the text rather
/// than owning it.
fn block_id(block: &str) -> Option<NodeId> {
    let mut found = None;
    let mut rest = block;
    while let Some(start) = rest.find("{#") {
        let after = &rest[start + 2..];
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
            .unwrap_or(after.len());
        if end > 0 {
            found = Some(NodeId::new(&after[..end]));
        }
        rest = &after[end..];
    }
    found
}

/// The index of the footnote a definition block defines: `[^3]: …` is 3.
fn footnote_marker(block: &str) -> Option<usize> {
    let rest = block.strip_prefix("[^")?;
    let (number, _) = rest.split_once("]:")?;
    number.parse().ok()
}

/// The selector that reads the node behind a hit.
///
/// A footnote is addressed at its reference and written at the foot of the
/// document, so `read --select` refuses it and this names the referring block
/// instead — the mirror of what `select` does when it carries a definition
/// along with the block that needs it. The reference is the only place the
/// footnote's id is written, which is what makes it findable.
fn select_for(fragments: &[NodeFragment], fragment: &NodeFragment) -> String {
    if fragment.kind != NodeKind::Footnote {
        return format!("#{}", fragment.id.0);
    }
    let mark = format!("{{#{}}}", fragment.id.0);
    for candidate in fragments {
        if candidate.kind == NodeKind::Footnote || candidate.id == fragment.id {
            continue;
        }
        if candidate.markdown.contains(&mark) {
            return format!("#{}", candidate.id.0);
        }
    }
    // Nothing refers to it, which a re-read would drop anyway: say the id
    // rather than invent a selector that reads something else.
    format!("#{}", fragment.id.0)
}

/// A block's text as one line: machinery stripped, whitespace collapsed.
fn flatten(markdown: &str) -> Vec<char> {
    let stripped = strip_machinery(markdown);
    let mut out = Vec::new();
    let mut spaced = true;
    for c in stripped.chars() {
        if c.is_whitespace() {
            if !spaced && !out.is_empty() {
                out.push(' ');
                spaced = true;
            }
            continue;
        }
        out.push(c);
        spaced = false;
    }
    while out.last() == Some(&' ') {
        out.pop();
    }
    out
}

/// One lowercase char per input char.
///
/// Deliberately *not* `str::to_lowercase`: that expands some characters into
/// several (`İ` into two), which would shift every offset after them and make a
/// snippet quote the wrong span. Keeping the mapping one-to-one costs the
/// handful of characters whose lowering is not a single char, and buys exact
/// positions on every document that is not one of them.
fn fold_case(text: &str) -> Vec<char> {
    text.chars().map(fold_char).collect()
}

fn fold_char(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Every occurrence of `needle` in `text`, as char ranges.
fn occurrences(text: &[char], needle: &[char]) -> Vec<(usize, usize)> {
    if needle.is_empty() || needle.len() > text.len() {
        return Vec::new();
    }
    let folded: Vec<char> = text.iter().copied().map(fold_char).collect();
    let mut found = Vec::new();
    let mut start = 0usize;
    while start + needle.len() <= folded.len() {
        if folded[start..start + needle.len()] == *needle {
            found.push((start, start + needle.len()));
            // Overlapping matches are one reading counted twice: step past it.
            start += needle.len();
        } else {
            start += 1;
        }
    }
    found
}

fn snippet(text: &[char], start: usize, end: usize, context: usize) -> Snippet {
    let before_from = start.saturating_sub(context);
    let after_to = (end + context).min(text.len());
    Snippet {
        before: text[before_from..start].iter().collect(),
        matched: text[start..end].iter().collect(),
        after: text[end..after_to].iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_docmark::Fidelity;
    use docsai_model::text::{Block, Heading, Paragraph, Section, TextDocument};
    use docsai_model::MemoryAssetStore;

    fn doc() -> Document {
        Document::Text(TextDocument {
            sections: vec![Section {
                blocks: vec![
                    Block::Heading(Heading {
                        id: None,
                        level: 1,
                        paragraph: Paragraph::text("Revenue"),
                    }),
                    Block::Paragraph(Paragraph::text("Revenue grew, and revenue will grow.")),
                    Block::Paragraph(Paragraph::text("Nothing to see here.")),
                ],
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    fn results(query: &str) -> SearchResults {
        search(
            &doc(),
            &MemoryAssetStore::new(),
            &DocMarkOptions::default(),
            &query.parse::<Query>().unwrap(),
        )
    }

    #[test]
    fn an_addressed_block_is_reported_at_its_id() {
        let results = results("revenue");
        assert_eq!(results.matches, 3, "case-insensitive, twice in the prose");
        let Location::Node {
            id, kind, select, ..
        } = &results.hits[0].location
        else {
            panic!("the heading carries an id: {:?}", results.hits[0].location);
        };
        assert_eq!(*kind, NodeKind::Heading);
        assert_eq!(*select, format!("#{}", id.0));
    }

    #[test]
    fn prose_is_found_and_addressed_relative_to_the_last_id() {
        // The point of the whole module: an ordinary paragraph carries no id
        // (spec §11.1), and it still has to be findable.
        let results = results("grew");
        assert_eq!(results.hits.len(), 1);
        let hit = &results.hits[0];
        assert_eq!(hit.matches, 1);
        assert!(hit.select().is_none(), "no selector reads a relative path");
        assert_eq!(hit.address(), "n1.b1");
        assert_eq!(hit.snippets[0].matched, "grew");
        assert!(hit.snippets[0].before.ends_with("Revenue "));
    }

    #[test]
    fn a_snippet_is_bounded_by_the_context_asked_for() {
        // On a three-line document the hits cost more than the document —
        // context around every block *is* the document there. What has to hold
        // on any document is the bound; the corpus test measures the saving on
        // one where it means something.
        let mut query = Query::new("revenue").unwrap();
        query.context = 5;
        let results = search(
            &doc(),
            &MemoryAssetStore::new(),
            &DocMarkOptions::default(),
            &query,
        );
        for hit in &results.hits {
            for snippet in &hit.snippets {
                assert!(snippet.before.chars().count() <= 5, "{snippet:?}");
                assert!(snippet.after.chars().count() <= 5, "{snippet:?}");
            }
        }
    }

    #[test]
    fn a_lossy_level_still_finds_the_text_it_can_no_longer_address() {
        let options = DocMarkOptions {
            fidelity: Fidelity::Plain,
            ..Default::default()
        };
        let results = search(
            &doc(),
            &MemoryAssetStore::new(),
            &options,
            &"revenue".parse().unwrap(),
        );
        assert_eq!(results.matches, 3);
        assert!(
            results.hits.iter().all(|hit| hit.select().is_none()),
            "plain carries no id to address anything by"
        );
    }

    #[test]
    fn a_query_that_cannot_be_read_says_what_it_wanted() {
        assert!("   ".parse::<Query>().is_err());
        assert!(Query::new("").is_err());
    }

    #[test]
    fn the_block_level_id_is_the_last_one_written() {
        assert_eq!(block_id("## Q3 {#n4 .Heading2}"), Some(NodeId::new("n4")));
        assert_eq!(
            block_id("A note[^1]{#n9} in prose. {#n5}"),
            Some(NodeId::new("n5")),
            "the trailing attribute block owns the paragraph, the inline one does not"
        );
        assert_eq!(block_id("plain prose"), None);
        assert_eq!(footnote_marker("[^3]: the note"), Some(3));
    }

    #[test]
    fn folding_case_keeps_one_char_per_char() {
        assert_eq!(fold_case("Aİb").len(), 3, "offsets must not shift");
    }
}

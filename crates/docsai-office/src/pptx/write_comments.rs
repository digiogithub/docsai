//! Slide comments written back (Phase 16-F; spec §11.3).
//!
//! The rule 16-C set for a chart holds here word for word, one part further
//! out: `ppt/comments/commentN.xml` is **not** in
//! [`SkeletonRef::rebuilt_parts`](docsai_model::presentation::SkeletonRef) and
//! 16-E did not put it there, so a comment is written by *splicing into the
//! very part that was read*. Every byte of that part which is not a `p:cm` the
//! model changed — its XML declaration, its namespace declarations, the
//! `p:extLst` of a comment nobody touched — is the byte that came in.
//!
//! And the question «has anything changed?» is asked exactly once, in the
//! model's own units, by the model's own reader
//! ([`comments::read_one_part`](super::comments::read_one_part)). A deck whose
//! comments nobody touched therefore produces no edit at all — not to the
//! comment part, not to its `_rels`, not to `ppt/commentAuthors.xml`, not to
//! `[Content_Types].xml` — and **no warning**.
//!
//! ## The author table is matched, not renumbered
//!
//! DocMark names an author and never carries `authorId` (spec §11.3 rule 4), so
//! a writer has to re-establish the number. It does so by **looking the name up
//! in the table the package already has**: an author the deck knows keeps the id
//! the deck gave them, and only a name no `p:cmAuthor` states is minted a new
//! one, appended after the last. Two consequences, and the first is the reason:
//!
//! * a round trip that changed nothing renumbers nothing, because every author
//!   resolves to the id that was already there — which is what lets the
//!   preserved `p:cm` bytes stand;
//! * an author the document introduced does not disturb anybody else's id, so
//!   a comment part this writer never touched still resolves correctly.
//!
//! ## What a classic `p:cmLst` cannot hold, and is therefore warned
//!
//! `CT_Comment` is an author, a date, a point and a string. So a **reply**, a
//! **resolved** flag and anything in a comment's body that is not a plain
//! paragraph have nowhere to go, and each is a specific warning naming the
//! comment it belongs to. Those are 16-E's read-side losses seen from the other
//! side: `p:cmLst` is a flat list and the reader says so where it produces
//! `replies: []`.

use std::collections::{BTreeMap, BTreeSet};

use docsai_model::addressing::comment_without_ids;
use docsai_model::presentation::Slide;
use docsai_model::report::ConversionReport;
use docsai_model::text::{Block, Comment, CommentAnchor};

use crate::package::{ContentTypes, Package};
use crate::xml::Element;

use super::comments::{self, Authors};
use super::write::{esc_attr, free_part, splice, PartRels};
use super::PML;

/// The relationship type both comment parts hang off a slide with.
pub(super) const COMMENTS_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";
/// The relationship type `ppt/commentAuthors.xml` hangs off the deck with.
pub(super) const AUTHORS_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/commentAuthors";

/// What one slide's comments cost the writer.
#[derive(Default)]
pub(super) struct Written {
    /// Parts to file into the rebuild, already rendered.
    pub parts: Vec<(String, Vec<u8>)>,
    /// Parts the written package must not carry at all: the comment part of a
    /// slide whose last comment was deleted.
    pub removed: BTreeSet<String>,
    /// `[Content_Types].xml` overrides the package does not state yet.
    pub overrides: BTreeMap<String, String>,
    /// One clause per loss, completing «slide N: …».
    pub kept: Vec<String>,
}

impl Written {
    fn absorb(&mut self, other: Written) {
        self.parts.extend(other.parts);
        self.removed.extend(other.removed);
        self.overrides.extend(other.overrides);
        self.kept.extend(other.kept);
    }
}

/// The deck-wide half of writing comments: the author table, and the part names
/// already in use.
pub(super) struct Comments<'a> {
    package: &'a Package,
    types: ContentTypes,
    /// The deck's own part, whose relationships name `ppt/commentAuthors.xml`.
    main: String,
    authors: Authors,
    /// Authors this write added, in the order they were minted.
    minted: Vec<(i64, String, String)>,
    /// Part names the package uses, plus every one this writer handed out.
    taken: BTreeSet<String>,
    /// The largest `idx` each author id has used anywhere in the deck, which is
    /// what `p:cmAuthor@lastIdx` states.
    last_idx: BTreeMap<i64, i64>,
}

impl<'a> Comments<'a> {
    pub(super) fn new(package: &'a Package, main: &str) -> Comments<'a> {
        let types = ContentTypes::read(package);
        let rels = package.relationships(main);
        // The report this reader would write is thrown away on purpose: it is
        // the *reading* of the package this writer is about to edit, and every
        // one of its warnings was already emitted when the document was read.
        let mut quiet = ConversionReport::new();
        let authors = Authors::read(package, &types, &rels, &mut quiet);
        Comments {
            package,
            types,
            main: main.to_string(),
            authors,
            minted: Vec::new(),
            taken: package.part_names().map(str::to_string).collect(),
            last_idx: BTreeMap::new(),
        }
    }

    /// The id for an author, minting one only for a name the deck does not know.
    fn author_id(&mut self, comment: &Comment) -> Option<i64> {
        let name = comment.author.as_deref()?;
        if let Some(id) = self.authors.id_of(name, comment.initials.as_deref()) {
            return Some(id);
        }
        if let Some((id, _, _)) = self
            .minted
            .iter()
            .find(|(_, minted, _)| minted == name)
            .cloned()
        {
            return Some(id);
        }
        let id = self
            .minted
            .last()
            .map(|(id, _, _)| id + 1)
            .unwrap_or_else(|| self.authors.next_id());
        // `p:cmAuthor@initials` is required by the schema. An author DocMark
        // names without them gets the first letter of each word of the name,
        // which is what every producer does and what a reader will show.
        let initials = comment
            .initials
            .clone()
            .unwrap_or_else(|| initials_of(name));
        self.minted.push((id, name.to_string(), initials));
        Some(id)
    }

    /// Records that `author` has now used `idx`, for `p:cmAuthor@lastIdx`.
    fn used(&mut self, author: Option<i64>, idx: i64) {
        if let Some(author) = author {
            let slot = self.last_idx.entry(author).or_insert(0);
            *slot = (*slot).max(idx);
        }
    }

    /// One slide's comment part, written, spliced, created or removed.
    pub(super) fn slide(&mut self, slide_part: &str, slide: &Slide) -> Written {
        let rels = self.package.relationships(slide_part);
        let part = comments::part(&self.types, &rels);
        match (part, slide.comments.is_empty()) {
            (None, true) => Written::default(),
            (None, false) => self.create(slide_part, slide),
            (Some(part), _) => self.edit(&part, slide_part, slide),
        }
    }

    /// The deck-wide parts, once every slide has been through [`Comments::slide`].
    ///
    /// Empty — and therefore no edit to `ppt/commentAuthors.xml`, to the deck's
    /// relationships or to the content types — whenever no author had to be
    /// minted, which is every deck whose comments nobody touched.
    pub(super) fn finish(&mut self) -> Written {
        let mut out = Written::default();
        if self.minted.is_empty() && self.last_idx.is_empty() {
            return out;
        }
        let entries: String = self
            .minted
            .iter()
            .map(|(id, name, initials)| {
                format!(
                    "<p:cmAuthor id=\"{id}\" name=\"{}\" initials=\"{}\" lastIdx=\"{}\" clrIdx=\"{id}\"/>",
                    esc_attr(name),
                    esc_attr(initials),
                    self.last_idx.get(id).copied().unwrap_or(1).max(1),
                )
            })
            .collect();
        if entries.is_empty() {
            return out;
        }
        match self.authors.part_name().map(str::to_string) {
            // The table exists: the new authors are spliced in before the
            // closing tag and every `p:cmAuthor` the deck had keeps its bytes.
            Some(part) => {
                let Ok(source) = self.package.text(&part) else {
                    out.kept.push(format!(
                        "an author the document names, because `{part}` is not readable \
                         as XML and the deck's author table could not be extended"
                    ));
                    return out;
                };
                let Some(at) = closing_tag(source, "cmAuthorLst") else {
                    out.kept.push(format!(
                        "an author the document names, because `{part}` does not close a \
                         `p:cmAuthorLst` for one to be added to"
                    ));
                    return out;
                };
                let spliced = format!("{}{entries}{}", &source[..at], &source[at..]);
                out.parts.push((part, spliced.into_bytes()));
            }
            // No table at all: the part, its relationship on the deck's own
            // part, and its content type.
            None => {
                // The canonical name first: `ppt/commentAuthors.xml` is what
                // every producer writes and what a human looking at the package
                // expects, and `free_part` would spell it `commentAuthors1.xml`.
                let part = if self.taken.insert("ppt/commentAuthors.xml".to_string()) {
                    "ppt/commentAuthors.xml".to_string()
                } else {
                    self.free("ppt", "commentAuthors")
                };
                let xml = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
                     <p:cmAuthorLst xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
                     {entries}</p:cmAuthorLst>"
                );
                out.overrides
                    .insert(part.clone(), format!("{PML}.commentAuthors+xml"));
                out.parts.push((part.clone(), xml.into_bytes()));
                let mut rels = PartRels::new(self.package, &self.main);
                rels.relate(AUTHORS_REL, &part, false);
                if let Some((name, xml)) = rels.rewritten() {
                    out.parts.push((name, xml.into_bytes()));
                }
            }
        }
        out
    }

    /// A part name nothing in the package or in this write uses.
    fn free(&mut self, dir: &str, stem: &str) -> String {
        let part = free_part(&self.taken, dir, stem);
        self.taken.insert(part.clone());
        part
    }

    /// A slide that has comments and no part to hold them.
    fn create(&mut self, slide_part: &str, slide: &Slide) -> Written {
        let mut out = Written::default();
        let part = self.free("ppt/comments", "comment");
        let mut body = String::new();
        for (offset, comment) in slide.comments.iter().enumerate() {
            let idx = offset as i64 + 1;
            body.push_str(&self.render(comment, idx, &mut out.kept));
        }
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
             <p:cmLst xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
             {body}</p:cmLst>"
        );
        out.overrides
            .insert(part.clone(), format!("{PML}.comments+xml"));
        out.parts.push((part.clone(), xml.into_bytes()));
        let mut rels = PartRels::new(self.package, slide_part);
        rels.relate(COMMENTS_REL, &part, false);
        if let Some((name, xml)) = rels.rewritten() {
            out.parts.push((name, xml.into_bytes()));
        }
        out
    }

    /// A slide whose comment part exists: spliced, or taken out of the package.
    fn edit(&mut self, part: &str, slide_part: &str, slide: &Slide) -> Written {
        let mut out = Written::default();
        let Ok(source) = self.package.text(part) else {
            out.kept.push(format!(
                "its comments, whose part `{part}` is not readable as XML"
            ));
            return out;
        };
        let Ok(root) = Element::parse(part, source.as_bytes()) else {
            out.kept
                .push(format!("its comments, whose part `{part}` did not parse"));
            return out;
        };

        // What the part says, read by the reader that read it in the first
        // place. Equal means no edit: not to this part, not to anything.
        let mut quiet = ConversionReport::new();
        let said = comments::read_one_part(self.package, part, &self.authors, &mut quiet)
            .unwrap_or_default();
        let wanted: Vec<Comment> = slide.comments.iter().map(comment_without_ids).collect();
        if said == wanted {
            for (offset, comment) in said.iter().enumerate() {
                let idx = stated_idx(&root, offset).unwrap_or(offset as i64 + 1);
                let author = comment
                    .author
                    .as_deref()
                    .and_then(|name| self.authors.id_of(name, comment.initials.as_deref()));
                self.used(author, idx);
            }
            return out;
        }

        if slide.comments.is_empty() {
            // The last comment was deleted. The part goes, and so do the
            // relationship that names it and the override that types it — an
            // override naming a part the package does not carry is exactly the
            // inconsistency an OPC consumer refuses.
            let rels = PartRels::new(self.package, slide_part);
            out.removed.insert(part.to_string());
            if let Some((name, xml)) = rels.without(part) {
                out.parts.push((name, xml.into_bytes()));
            }
            return out;
        }

        let stated: Vec<&Element> = root.children_named("cm").collect();
        // Every `idx` the part states, so a `p:cm` this writer generates beside
        // one it copied verbatim cannot collide with it.
        let mut used: BTreeSet<i64> = stated.iter().filter_map(|cm| cm.attr_i64("idx")).collect();
        let mut next_idx = used.iter().next_back().copied().unwrap_or(0) + 1;

        let mut body = String::new();
        for (offset, comment) in slide.comments.iter().enumerate() {
            match (said.get(offset), stated.get(offset)) {
                // Unchanged, in place: its own bytes, `p:extLst` and all.
                (Some(was), Some(element)) if Some(was) == wanted.get(offset) => {
                    body.push_str(&source[element.span.clone()]);
                    let idx = element.attr_i64("idx").unwrap_or(offset as i64 + 1);
                    let author = comment
                        .author
                        .as_deref()
                        .and_then(|name| self.authors.id_of(name, comment.initials.as_deref()));
                    self.used(author, idx);
                }
                _ => {
                    // Reuse the slot's own `idx` when there is one, so an edit
                    // to a comment's text does not renumber the part.
                    let idx = match stated.get(offset).and_then(|cm| cm.attr_i64("idx")) {
                        Some(idx) => idx,
                        None => {
                            while !used.insert(next_idx) {
                                next_idx += 1;
                            }
                            next_idx
                        }
                    };
                    if let Some(element) = stated.get(offset) {
                        if element.child("extLst").is_some() {
                            out.kept.push(format!(
                                "the extension list of comment {idx} in `{part}`, which was \
                                 rewritten because the document changed what it says"
                            ));
                        }
                    }
                    body.push_str(&self.render(comment, idx, &mut out.kept));
                }
            }
        }

        let edit = match (stated.first(), stated.last()) {
            (Some(first), Some(last)) => (first.span.start..last.span.end, body),
            // An empty `p:cmLst` has no child span to write between.
            _ => {
                let prefix = if root.prefix.is_empty() {
                    String::new()
                } else {
                    format!("{}:", root.prefix)
                };
                let open = root.span.start
                    + source[root.span.clone()]
                        .find('>')
                        .map(|at| at + 1)
                        .unwrap_or(0);
                // A self-closed `<p:cmLst/>` is replaced whole; an open one has
                // its body written between the tags it already has.
                if source[root.span.clone()].ends_with("/>") {
                    (
                        root.span.clone(),
                        format!("<{prefix}cmLst>{body}</{prefix}cmLst>"),
                    )
                } else {
                    (open..open, body)
                }
            }
        };
        let xml = splice(source, vec![edit]);
        if xml != source {
            out.parts.push((part.to_string(), xml.into_bytes()));
        }
        out
    }

    /// One `p:cm`, and every loss it costs named as it happens.
    fn render(&mut self, comment: &Comment, idx: i64, kept: &mut Vec<String>) -> String {
        let author = self.author_id(comment);
        self.used(author, idx);
        let who = describe(comment);
        if comment.author.is_none() {
            // `authorId` is required, and 0 is the id PowerPoint gives the
            // author it does not know either. Said rather than assumed.
            kept.push(format!(
                "the author of {who}, which this deck's author table does not name: it \
                 was written as author 0"
            ));
        }
        if !comment.replies.is_empty() {
            kept.push(format!(
                "the {} repl{} under {who}, because `p:cmLst` is a flat list and \
                 `CT_Comment` states no parent",
                comment.replies.len(),
                if comment.replies.len() == 1 {
                    "y"
                } else {
                    "ies"
                }
            ));
        }
        if comment.resolved {
            kept.push(format!(
                "the resolved mark on {who}, because `CT_Comment` has no done flag"
            ));
        }
        let (x, y) = match &comment.anchor {
            CommentAnchor::Point(point) => (point.x.eighth_points(), point.y.eighth_points()),
            _ => {
                // `p:pos` is required by the schema, so there is no way to say
                // «nowhere». The corner is stated, and so is the fact that the
                // document never said where.
                kept.push(format!(
                    "the position of {who}, which the document does not state and \
                     `CT_Comment` requires: it was pinned to the top-left corner"
                ));
                (0, 0)
            }
        };
        let dt = match comment.date.as_deref() {
            Some(date) if is_xsd_date_time(date) => format!(" dt=\"{}\"", esc_attr(date)),
            Some(other) => {
                kept.push(format!(
                    "the date of {who} (`{other}`), which `CT_Comment` states as an \
                     xsd:dateTime and this is not"
                ));
                String::new()
            }
            None => String::new(),
        };
        format!(
            "<p:cm authorId=\"{}\"{dt} idx=\"{idx}\"><p:pos x=\"{x}\" y=\"{y}\"/><p:text>{}</p:text></p:cm>",
            author.unwrap_or(0),
            crate::pptx::write_text::esc_text(&flatten(&comment.blocks, &who, kept))
        )
    }
}

/// Where the closing tag of `local` starts, whatever prefix it carries.
///
/// The last one in the part, because the element it closes is the root: a
/// `p:cmAuthorLst` has no nested one to be confused with.
fn closing_tag(source: &str, local: &str) -> Option<usize> {
    let mut at = None;
    let mut from = 0;
    while let Some(found) = source[from..].find("</") {
        let start = from + found;
        let rest = &source[start + 2..];
        let end = rest.find('>')?;
        if rest[..end].trim_end().rsplit(':').next() == Some(local) {
            at = Some(start);
        }
        from = start + 2 + end;
    }
    at
}

/// The `idx` the part's `n`-th `p:cm` states.
fn stated_idx(root: &Element, n: usize) -> Option<i64> {
    root.children_named("cm").nth(n)?.attr_i64("idx")
}

/// A comment's body as the one string `p:text` holds.
///
/// The reader turns each line of that string into a paragraph, so the writer
/// turns each paragraph back into a line. Anything else a comment's blocks
/// carry — a list, a table, a picture, bold — has nowhere to go in a string,
/// and is named rather than dropped in silence.
fn flatten(blocks: &[Block], who: &str, kept: &mut Vec<String>) -> String {
    let mut lines = Vec::new();
    let mut other = 0usize;
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => lines.push(paragraph.plain_text()),
            _ => other += 1,
        }
    }
    if other > 0 {
        kept.push(format!(
            "{other} block(s) of {who} that are not a paragraph, because `p:text` is one \
             string and holds no structure"
        ));
    }
    lines.join("\n")
}

/// «the comment by X», or «the comment», for a warning to complete.
fn describe(comment: &Comment) -> String {
    match comment.author.as_deref() {
        Some(author) => format!("the comment by {author}"),
        None => "the unattributed comment".to_string(),
    }
}

/// Initials for an author the document names without any.
fn initials_of(name: &str) -> String {
    let initials: String = name
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .collect();
    if initials.is_empty() {
        "?".to_string()
    } else {
        initials
    }
}

/// Whether a string is an `xsd:dateTime` this writer is willing to put in an
/// attribute the schema gate validates.
///
/// Deliberately narrow: `date=` in DocMark is whatever the source said, and a
/// hand-typed document can say anything. A value that is not a date is dropped
/// with a warning rather than written into a part that would then fail to open.
fn is_xsd_date_time(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 19 {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| range.clone().all(|i| bytes[i].is_ascii_digit());
    digits(0..4)
        && bytes[4] == b'-'
        && digits(5..7)
        && bytes[7] == b'-'
        && digits(8..10)
        && (bytes[10] == b'T' || bytes[10] == b't')
        && digits(11..13)
        && bytes[13] == b':'
        && digits(14..16)
        && bytes[16] == b':'
        && digits(17..19)
        && value[19..]
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | ':' | 'Z' | 'z'))
}

/// Every slide's comments, and the deck-wide parts they need.
///
/// One entry point so the order is stated once: the slides first, because what
/// they write is what decides which authors exist and which `idx` each has
/// used, and the deck's own parts afterwards.
pub(super) fn write(
    package: &Package,
    main: &str,
    slides: &[(String, &Slide)],
) -> (Written, Vec<(usize, String)>) {
    let mut state = Comments::new(package, main);
    let mut out = Written::default();
    let mut warnings = Vec::new();
    for (index, (part, slide)) in slides.iter().enumerate() {
        let mut written = state.slide(part, slide);
        for why in std::mem::take(&mut written.kept) {
            warnings.push((index, why));
        }
        out.absorb(written);
    }
    let mut deck = state.finish();
    for why in std::mem::take(&mut deck.kept) {
        warnings.push((0, why));
    }
    out.absorb(deck);
    (out, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_are_the_first_letter_of_each_word() {
        assert_eq!(initials_of("Ada Lovelace"), "AL");
        assert_eq!(initials_of("Grace"), "G");
        assert_eq!(initials_of("  "), "?");
    }

    #[test]
    fn only_a_real_instant_reaches_the_dt_attribute() {
        assert!(is_xsd_date_time("2026-02-03T09:12:00Z"));
        assert!(is_xsd_date_time("2026-02-03T09:12:00.000000000"));
        assert!(is_xsd_date_time("2026-02-03T09:12:00+01:00"));
        assert!(!is_xsd_date_time("ayer"));
        assert!(!is_xsd_date_time("2026-02-03"));
        assert!(!is_xsd_date_time("2026-02-03T09:12:00 (aprox)"));
    }
}

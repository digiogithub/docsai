//! Slide comments (Phase 16, increment 16-E; spec §11.3).
//!
//! Two parts and one rule apiece.
//!
//! * `ppt/commentAuthors.xml` (`p:cmAuthorLst`) is a **deck-wide** table of who
//!   may comment, read once and reached through the *presentation* part's own
//!   relationships. Its ids are local to the package — author 2 in one deck is
//!   nobody in another — so they are resolved here and never reach the IR
//!   (spec §11.3 rule 4).
//! * `ppt/comments/comment1.xml` (`p:cmLst`) belongs to **one slide**, and is
//!   reached through that slide's `comments` relationship rather than through
//!   the number in the part name — the same rule that binds a notes page to
//!   its slide ([`super::notes`]), and for the same reason: the numbering is a
//!   habit PowerPoint happens to follow.
//!
//! Neither part is added to `SkeletonRef::rebuilt_parts`. Reading a comment
//! does not license regenerating the part that holds it, so a deck whose
//! comments nobody touched still comes back byte for byte; 16-F writes one back
//! by *splicing* into the very part that was read, exactly as 16-C does with a
//! chart.
//!
//! **The unit of `p:cm/p:pos`, settled with a file** (16-E carried this open;
//! spec §11.3). The schema says `a:CT_Point2D`, whose `x`/`y` are
//! `ST_Coordinate` and therefore EMU — and every real deck contradicts it: the
//! example in ECMA-376 itself is `<p:pos x="1426" y="660"/>`, which as EMU is
//! 1/640 of an inch from the corner of a ten-inch slide. The unit is
//! PowerPoint's **master unit**, 1/576 in, which is also an eighth of a point,
//! and the evidence is a file rather than a reading of prose: an `.odp`
//! carrying one `office:annotation` at `svg:x="5cm"` converted by LibreOffice
//! to `.pptx` comes out as `<p:pos x="1134" y="680"/>`, and 5 cm is exactly
//! 1134 × 1/576 in. LibreOffice's exporter says the same in source
//! (`sd/source/filter/eppt/pptx-epptooxml.cxx`, `convertMm100ToMasterUnit`),
//! and its importer is reverse-engineered from what PowerPoint writes.
//! So [`Length::from_eighth_points`] on the way in and
//! [`Length::eighth_points`] on the way out, and a comment DocMark places at
//! `2.5cm` is 2.5 cm from the corner of the slide.
//!
//! **What is not read, and is said out loud.** Since 2021 PowerPoint also
//! writes threaded, text-anchored comments into `ppt/comments/modernComment_*.xml`
//! under a Microsoft extension schema (`application/vnd.ms-powerpoint.comments+xml`)
//! that ECMA-376 does not define and that expresses replies and text anchors
//! `p:cmLst` cannot. Those parts are warned, one by one, and preserved.

use std::collections::BTreeMap;

use docsai_model::report::{ConversionReport, Warning};
use docsai_model::text::{Block, Comment, CommentAnchor, Paragraph};
use docsai_model::units::{Length, Point};

use crate::error::ReadError;
use crate::package::{ContentTypes, Package, Relationships};

use super::PML;

/// The content type of the classic, ECMA-376 comment list.
const CT_COMMENTS: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.comments+xml";
/// The content type of PowerPoint's modern comment part, which this reader does
/// not model. Named as a constant so the warning cannot drift from the test.
const CT_MODERN_COMMENTS: &str = "application/vnd.ms-powerpoint.comments+xml";

/// Who may comment, by the id the comment parts refer to.
#[derive(Debug, Default)]
pub(super) struct Authors {
    by_id: BTreeMap<i64, (String, Option<String>)>,
    /// True when the deck declares no author part at all, which makes an
    /// unresolved id a different (and more explicable) fact.
    part: Option<String>,
}

impl Authors {
    /// Reads `ppt/commentAuthors.xml` through the presentation's relationships.
    ///
    /// A deck with no author part is normal — most decks have no comments — so
    /// its absence is not a warning. A part that is declared and unreadable is.
    pub(super) fn read(
        package: &Package,
        types: &ContentTypes,
        rels: &Relationships,
        report: &mut ConversionReport,
    ) -> Authors {
        let mut authors = Authors::default();
        let Some(rel) = rels.of_kind("commentAuthors").find(|rel| !rel.external) else {
            return authors;
        };
        if !types.is(&rel.target, &format!("{PML}.commentAuthors+xml")) {
            report.warn(Warning::Degraded {
                what: format!("comment authors `{}`", rel.target),
                why: "the package declares it as something else".into(),
            });
            return authors;
        }
        let root = match super::parse(package, &rel.target) {
            Ok(root) => root,
            Err(error) => {
                report.warn(Warning::Degraded {
                    what: format!("comment authors `{}`", rel.target),
                    why: error.to_string(),
                });
                return authors;
            }
        };
        authors.part = Some(rel.target.clone());
        for author in root.children_named("cmAuthor") {
            let Some(id) = author.attr_i64("id") else {
                continue;
            };
            let name = author.attr("name").unwrap_or_default().to_string();
            let initials = author
                .attr("initials")
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            authors.by_id.insert(id, (name, initials));
        }
        authors
    }

    /// The name and initials behind an author id.
    ///
    /// `None` is not a failure of the file's syntax: it is a comment whose
    /// author the deck does not define, and the caller keeps the comment and
    /// warns rather than attributing it to nobody in particular.
    fn get(&self, id: i64) -> Option<&(String, Option<String>)> {
        self.by_id.get(&id)
    }

    fn where_from(&self) -> &str {
        match &self.part {
            Some(part) => part,
            None => "no comment-authors part",
        }
    }

    /// The part the table was read from, when the deck declares one.
    pub(super) fn part_name(&self) -> Option<&str> {
        self.part.as_deref()
    }

    /// The id the deck already gives this author, matched on what DocMark
    /// carries: the display name, and the initials when both sides state them.
    ///
    /// This is what keeps a round trip that changed nothing from renumbering
    /// the table (16-F): the writer never mints an id for a name the package
    /// already knows, so every `authorId` in every preserved `p:cm` still
    /// resolves to the same person.
    pub(super) fn id_of(&self, name: &str, initials: Option<&str>) -> Option<i64> {
        self.by_id
            .iter()
            .find(|(_, (known, known_initials))| {
                known == name
                    && match (initials, known_initials.as_deref()) {
                        (Some(wanted), Some(known)) => wanted == known,
                        // One side is silent: the name decides. A DocMark
                        // document written at a level that keeps `author=` and
                        // drops nothing else is still the same person.
                        _ => true,
                    }
            })
            .map(|(id, _)| *id)
    }

    /// The first id no author in the table uses.
    pub(super) fn next_id(&self) -> i64 {
        self.by_id.keys().next_back().map(|id| id + 1).unwrap_or(1)
    }
}

/// The classic comment part of one slide, when its relationships name one.
///
/// Told apart from PowerPoint's modern comment part by **content type**, for
/// the reason [`read`] gives: both hang off the slide with a relationship whose
/// last segment is `comments`.
pub(super) fn part(types: &ContentTypes, slide_rels: &Relationships) -> Option<String> {
    slide_rels
        .of_kind("comments")
        .find(|rel| {
            !rel.external
                && match types.of(&rel.target) {
                    Some(CT_MODERN_COMMENTS) => false,
                    Some(other) => other == CT_COMMENTS,
                    // No declaration at all: the relationship already said what
                    // it is, and [`read`] reads it, so the writer writes it.
                    None => true,
                }
        })
        .map(|rel| rel.target.clone())
}

/// What one comment part says, as the model holds it.
///
/// The writer's whole comparison runs through this: «has anything changed?» is
/// asked once, in the model's own units, by the model's own reader, so there is
/// no second notion of what a comment part says for the two to drift apart
/// (the rule 16-C states for charts).
pub(super) fn read_one_part(
    package: &Package,
    part: &str,
    authors: &Authors,
    report: &mut ConversionReport,
) -> Result<Vec<Comment>, ReadError> {
    read_part(package, part, authors, report)
}

/// The comment parts of one slide, when its relationships name any.
///
/// Every returned comment is anchored at a point ([`CommentAnchor::Point`]) or
/// unanchored; a slide comment is never anchored over text, because `p:cm` has
/// no way to say so.
pub(super) fn read(
    package: &Package,
    types: &ContentTypes,
    slide_part: &str,
    slide_rels: &Relationships,
    authors: &Authors,
    report: &mut ConversionReport,
) -> Result<Vec<Comment>, ReadError> {
    let mut out = Vec::new();
    for rel in slide_rels.of_kind("comments") {
        if rel.external {
            report.warn(Warning::Degraded {
                what: format!("comments of `{slide_part}`"),
                why: "the comment part lives outside the package".into(),
            });
            continue;
        }
        match types.of(&rel.target) {
            Some(CT_MODERN_COMMENTS) => {
                // True and specific (`AGENTS.md` §7 rule 3): the part exists,
                // it is preserved, and what it holds is a schema this reader
                // does not model rather than something that failed to parse.
                report.warn(Warning::UnsupportedElement {
                    kind: "p188:cmLst".into(),
                    location: rel.target.clone(),
                    action: format!(
                        "not read: `{CT_MODERN_COMMENTS}` is PowerPoint's modern comment schema, \
                         which carries replies and text anchors `p:cmLst` cannot; the part is \
                         preserved unchanged"
                    ),
                });
                continue;
            }
            Some(CT_COMMENTS) => {}
            Some(other) => {
                report.warn(Warning::Degraded {
                    what: format!("comments of `{slide_part}`"),
                    why: format!("`{}` is declared as `{other}`", rel.target),
                });
                continue;
            }
            // No declaration at all. The relationship already said what it is,
            // and refusing here would lose a comment over a missing override.
            None => {}
        }
        out.extend(read_part(package, &rel.target, authors, report)?);
    }
    Ok(out)
}

fn read_part(
    package: &Package,
    part: &str,
    authors: &Authors,
    report: &mut ConversionReport,
) -> Result<Vec<Comment>, ReadError> {
    let root = super::parse(package, part)?;
    let mut out = Vec::new();
    for cm in root.children_named("cm") {
        let index = cm.attr_i64("idx").unwrap_or(out.len() as i64 + 1);
        let (author, initials) = match cm.attr_i64("authorId") {
            Some(id) => match authors.get(id) {
                Some((name, initials)) => (Some(name.clone()), initials.clone()),
                None => {
                    report.warn(Warning::Degraded {
                        what: format!("comment {index} of `{part}`"),
                        why: format!("author id {id} is not defined by {}", authors.where_from()),
                    });
                    (None, None)
                }
            },
            None => (None, None),
        };

        // `p:pos` is required by the schema, and a comment without one is a
        // comment nobody can point at on the slide. Kept, and said.
        let anchor = match cm.child("pos") {
            Some(pos) => CommentAnchor::Point(Point::new(
                Length::from_eighth_points(pos.attr_i64("x").unwrap_or_default()),
                Length::from_eighth_points(pos.attr_i64("y").unwrap_or_default()),
            )),
            None => {
                report.warn(Warning::Degraded {
                    what: format!("comment {index} of `{part}`"),
                    why: "no `p:pos`: it is kept, but nothing says where on the slide it sits"
                        .into(),
                });
                CommentAnchor::Unanchored
            }
        };

        let text = cm.child("text").map(|t| t.text()).unwrap_or_default();
        out.push(Comment {
            id: None,
            author,
            initials,
            date: cm.attr("dt").map(str::to_string),
            // `p:cm` has no resolution flag; only the modern part does, and
            // claiming `false` from a file that never said it would be a
            // fabricated answer to a question this format cannot ask.
            resolved: false,
            anchor,
            blocks: text_blocks(&text),
            // Nor a thread: `p:cmLst` is a flat list, and two comments at the
            // same spot are two comments, not a conversation.
            replies: Vec::new(),
        });
    }
    Ok(out)
}

/// A comment's text as blocks. `p:text` is one string, so its line breaks are
/// what separates one paragraph from the next.
fn text_blocks(text: &str) -> Vec<Block> {
    if text.is_empty() {
        return Vec::new();
    }
    text.replace("\r\n", "\n")
        .split('\n')
        .map(|line| Block::Paragraph(Paragraph::text(line)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_per_paragraph() {
        let blocks = text_blocks("first\nsecond");
        assert_eq!(blocks.len(), 2);
        assert!(text_blocks("").is_empty());
    }
}

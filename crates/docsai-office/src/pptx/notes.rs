//! Speaker notes (Phase 13, increment 13-F).
//!
//! A notes slide is reached through the **slide's own relationships**, never by
//! index: `notesSlide7.xml` belonging to the seventh slide is a convention
//! PowerPoint happens to follow and a converter is free to break. A deck whose
//! notes were attached in a different order would otherwise get every note put
//! under the wrong slide — a silent, plausible-looking corruption.
//!
//! The part's root element is `p:notes`, not `p:notesSlide`: ECMA-376 binds
//! `CT_NotesSlide` to the element `notes`, and the part name is only a name.
//! Spike P1 hit this; the corpus generator carries the same note.

use docsai_model::presentation::PhType;
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::text::Block;

use crate::error::ReadError;
use crate::package::{ContentTypes, Package, Relationships};

use super::cascade::{LevelStyles, Theme};
use super::{text, PML};

/// Reads the notes of one slide.
///
/// `None` when the slide has no notes part at all; `Some(vec![])` when it has
/// one that says nothing. The writer needs to tell those apart — the second is
/// an empty notes page a presenter can type into, and dropping it would delete
/// a part the deck declares.
pub(super) fn read(
    package: &Package,
    types: &ContentTypes,
    slide_part: &str,
    slide_rels: &Relationships,
    theme: &Theme,
    report: &mut ConversionReport,
) -> Result<Option<Vec<Block>>, ReadError> {
    let Some(rel) = slide_rels.of_kind("notesSlide").next() else {
        return Ok(None);
    };
    if rel.external {
        report.warn(Warning::Degraded {
            what: format!("speaker notes of `{slide_part}`"),
            why: "the notes part lives outside the package".into(),
        });
        return Ok(None);
    }
    if !types.is(&rel.target, &format!("{PML}.notesSlide+xml")) {
        report.warn(Warning::Degraded {
            what: format!("speaker notes of `{slide_part}`"),
            why: format!("`{}` is not declared as a notes slide", rel.target),
        });
        return Ok(None);
    }

    let root = super::parse(package, &rel.target)?;
    let rels = package.relationships(&rel.target);
    let Some(tree) = root.path(&["cSld", "spTree"]) else {
        report.warn(Warning::Degraded {
            what: format!("speaker notes `{}`", rel.target),
            why: "no shape tree".into(),
        });
        return Ok(Some(Vec::new()));
    };

    let mut blocks = Vec::new();
    for shape in tree.children() {
        match shape.name.as_str() {
            "nvGrpSpPr" | "grpSpPr" => continue,
            "sp" => {
                let ph = shape.path(&["nvSpPr", "nvPr", "ph"]);
                let ph_type = ph.map(|ph| PhType::parse(ph.attr("type").unwrap_or_default()));
                // The notes page is a printed page: it carries a thumbnail of
                // the slide, a header, a footer, a date and a page number
                // around the text. None of that is a note, and all of it is
                // regenerated from the notes master.
                if ph_type.as_ref().is_some_and(|ph| !ph.is_body()) {
                    continue;
                }
                let Some(tx_body) = shape.child("txBody") else {
                    continue;
                };
                let ctx = text::TextCtx {
                    rels: &rels,
                    // Notes are prose, not bullets: the notes master's
                    // `bodyStyle` is what would bullet them, and it is not in
                    // the slide cascade. Bulleting them anyway would invent a
                    // list in the Markdown that the deck never had.
                    bulleted: false,
                    theme,
                    inherited: &LevelStyles::default(),
                };
                blocks.extend(text::read_body(tx_body, &ctx, report));
            }
            name => report.warn(Warning::UnsupportedElement {
                kind: format!("p:{name}"),
                location: rel.target.clone(),
                action: "skipped: a notes page holds only its text".into(),
            }),
        }
    }
    Ok(Some(blocks))
}

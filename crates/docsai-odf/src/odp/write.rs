//! The `.odp` writer (Phase 18, increment 18-D).
//!
//! The reader's counterpart, and — because the fidelity mechanism is the one
//! spike P3 settled and the plan told this increment to obey — a **splicer**
//! rather than a generator:
//!
//! > **Preserve everything; rebuilding is the exception.**
//!
//! A deck read by [`super::read`] carries its whole original package in the
//! asset store, opaque and byte for byte ([`super::skeleton`]). This writer
//! puts that package back **entry by entry**, copying each member's stored
//! bytes without an inflate/deflate cycle, and substitutes only the byte ranges
//! of `content.xml` whose content the model no longer agrees with.
//!
//! ## Why the archive itself is part of the fidelity, and more so than in OPC
//!
//! [`Package::write_to`] is a `BTreeMap` re-zipped: it alphabetises the
//! members, re-deflates every one of them and stamps a fixed timestamp on each.
//! Even when every byte of every part is identical that is a different file.
//! For ODF one member makes the point sharper than any OPC package can:
//! **`mimetype` must be the first member of the archive and must be stored
//! uncompressed**, because that is what lets a consumer sniff the format from
//! the first bytes of the file (ODF 1.3 §3.3). `write_to` does honour those two
//! rules — it is the reason it special-cases `mimetype` — but it honours them
//! by *rebuilding*, and a rebuilt archive is not the archive that was read.
//!
//! So the re-emission here works on the ZIP: [`reemit`] copies each entry raw,
//! in the original order, keeping its compression method, its CRC, its
//! timestamp and its external attributes. `mimetype` therefore stays first and
//! stored because it *was* first and stored, which costs nothing and needs no
//! special case at all. **A `.odp` nobody edited comes back byte for byte.**
//!
//! ## What "edited" means, and what it deliberately does not mean
//!
//! 15-B's rule, unchanged by the dialect: nothing is written unless it differs
//! from what the part already says, and the comparison is made **in the model's
//! own terms against the model's own reader** — never against the part's bytes.
//! Concretely, [`write_odp`] re-reads the preserved package with
//! [`super::read_package`], which is the very function that produced the deck
//! in the first place, and compares body against body. Bytes could not be the
//! comparison here even if one wanted them to be: an ODF text body is written
//! against automatic styles this writer has to invent, so its markup is legal
//! and equivalent without being identical, and byte-comparing it would report
//! an edit on every single body of every unedited deck.
//!
//! The comparison is made on the *rendered* body rather than on the two
//! `Vec<Block>` directly, for one reason that is not an optimisation: a deck
//! that has been through DocMark carries stable node ids
//! ([`docsai_model::NodeId`]) that a package never states, so the two block
//! trees differ on every paragraph while saying exactly the same thing. What
//! the question actually is — *would writing this change what the part says?* —
//! is answered by writing both and comparing, which is blind to everything the
//! writer does not emit and to nothing that it does.
//!
//! ## What is spliced, and what is refused by name
//!
//! Spliced: the text body of a placeholder, of a free text box, and of a
//! speaker-notes frame. Those are the bodies DocMark-P round-trips as text and
//! the ones a person edits.
//!
//! Refused, each with a `Degraded` warning naming the construct rather than a
//! silent copy: a slide added or removed, a shape added or removed, a table's
//! cells, a picture swapped, a chart's data, and a preserved object's payload.
//! Each of those is a part of the package this increment does not write into,
//! and each warning fires **only when the model actually differs from the
//! package** — a loss that did not happen must not be reported as one
//! (`AGENTS.md` §7 rule 3).
//!
//! There is no embedded ODF template and no generated-from-nothing package: a
//! deck with no preserved skeleton is an error by name, not a file that looks
//! like a success.

use std::collections::BTreeMap;
use std::io::{Cursor, Seek, Write};
use std::ops::Range;

use docsai_model::assets::AssetStore;
use docsai_model::presentation::{Presentation, Shape, ShapeKind, SkeletonRef};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::text::{Block, Comment};
use docsai_model::{Document, MemoryAssetStore};

use crate::odt::write::{automatic_style_bodies, guess_image_mime, write_blocks, AutoStyles};
use crate::odt::write::{esc_attr, WriterCtx};
use crate::package::Package;
use crate::write_error::WriteError;
use crate::xml::Element;

use super::{read_package, CONTENT_PART};

/// The prefix every automatic style this writer invents carries.
///
/// A spliced body needs styles of its own, and the package it is spliced into
/// already has a `P1`. `ds` is short for the project and is not a name any
/// producer of these fixtures emits, so a `dsP1` cannot collide with one.
const STYLE_PREFIX: &str = "ds";

/// The manifest, which has to learn about a picture the package did not carry.
const MANIFEST_PART: &str = "META-INF/manifest.xml";

/// Writes a presentation as a `.odp` package.
pub fn write_odp<W: Write + Seek>(
    document: &Document,
    assets: &dyn AssetStore,
    writer: W,
) -> Result<ConversionReport, WriteError> {
    let Document::Presentation(deck) = document else {
        return Err(WriteError::Invalid(format!(
            "cannot write {} as .odp",
            document.shape_name()
        )));
    };
    let mut report = ConversionReport::new();
    let skeleton = skeleton_bytes(deck, assets)?;
    let package = Package::open(Cursor::new(skeleton)).map_err(|e| {
        WriteError::Invalid(format!("the preserved package cannot be read back: {e}"))
    })?;
    let source = package
        .text(CONTENT_PART)
        .map_err(|e| WriteError::Invalid(format!("{CONTENT_PART}: {e}")))?
        .to_string();
    let root = Element::parse(CONTENT_PART, source.as_bytes())
        .map_err(|e| WriteError::Invalid(format!("{CONTENT_PART}: {e}")))?;

    // The package as the model's own reader sees it: the only reference an
    // edit can honestly be measured against.
    let mut scratch = MemoryAssetStore::new();
    let (as_read, _) = read_package(&package, None, &mut scratch)
        .map_err(|e| WriteError::Invalid(format!("the preserved package no longer reads: {e}")))?;
    let Document::Presentation(before) = as_read else {
        return Err(WriteError::Invalid(
            "the preserved package is not a presentation".into(),
        ));
    };

    let mut plan = Plan::default();
    {
        let mut ctx = WriterCtx {
            assets,
            report: &mut report,
            auto: AutoStyles::with_prefix(STYLE_PREFIX),
            pictures: BTreeMap::new(),
            pic_seq: 0,
            note_seq: 0,
        };
        plan_edits(deck, &before, &root, &source, &mut ctx, &mut plan)?;
        if !ctx.auto.is_empty() {
            plan.styles = automatic_style_bodies(&ctx.auto);
        }
        plan.pictures = std::mem::take(&mut ctx.pictures);
    }

    let mut replacements: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut additions: Vec<(String, Vec<u8>)> = Vec::new();

    if !plan.styles.is_empty() {
        match style_anchor(&root, &plan.styles) {
            Some(edit) => plan.edits.push(edit),
            None => report.warn(Warning::Degraded {
                what: "the automatic styles a rewritten text body needs".into(),
                why: format!(
                    "`{CONTENT_PART}` declares no <office:automatic-styles> and no \
                     <office:body> to put one before, so the rewritten paragraphs \
                     keep their text and lose their direct formatting"
                ),
            }),
        }
    }

    if !plan.edits.is_empty() {
        replacements.insert(CONTENT_PART.to_string(), apply(&source, &mut plan.edits)?);
    }

    if !plan.pictures.is_empty() {
        let entries: Vec<(String, String)> = plan
            .pictures
            .keys()
            .map(|path| (path.clone(), guess_image_mime(path)))
            .collect();
        for (path, bytes) in &plan.pictures {
            additions.push((path.clone(), bytes.clone()));
        }
        match manifest_with(&package, &entries) {
            Ok(bytes) => {
                replacements.insert(MANIFEST_PART.to_string(), bytes);
            }
            Err(why) => {
                // The picture bytes would be in the archive and unlisted, which
                // is a package no consumer is obliged to read. Saying so and
                // leaving them out is the honest half of the two.
                additions.clear();
                report.warn(Warning::Degraded {
                    what: format!("{} picture(s) an edited text body added", entries.len()),
                    why: format!("`{MANIFEST_PART}` could not be extended: {why}"),
                });
            }
        }
    }

    reemit(skeleton, &replacements, &additions, writer)?;
    Ok(report)
}

/// The bytes of the preserved package.
///
/// Unlike the `.pptx` writer there is no embedded template to fall back on, and
/// that is a decision rather than an omission: a `.odp` written from nothing
/// would need a master page, a page layout and a presentation style family this
/// project does not ship, and inventing one silently is exactly the
/// «success that lost the design» the pptx side warns about. A deck with no
/// skeleton is refused by name.
fn skeleton_bytes<'a>(
    deck: &Presentation,
    assets: &'a dyn AssetStore,
) -> Result<&'a [u8], WriteError> {
    let Some(SkeletonRef { asset, .. }) = deck.skeleton.as_ref() else {
        return Err(WriteError::Invalid(
            "this deck carries no preserved package, and the `.odp` writer splices \
             into the package a deck was read from rather than generating one: \
             convert it to .pptx, or keep the `skeleton:` file beside its DocMark"
                .into(),
        ));
    };
    assets.get(asset).ok_or_else(|| {
        WriteError::Invalid(format!(
            "the deck names its preserved package as `{}` and the asset store does not hold it",
            asset.as_str()
        ))
    })
}

// ---------------------------------------------------------------------------
// The plan: byte ranges of `content.xml` and what replaces them
// ---------------------------------------------------------------------------

/// One replacement of a byte range of the source part.
struct Edit {
    at: Range<usize>,
    text: String,
}

#[derive(Default)]
struct Plan {
    edits: Vec<Edit>,
    /// The `<style:style>` elements the rewritten bodies referred to.
    styles: String,
    /// Pictures a rewritten body brought with it, by package path.
    pictures: BTreeMap<String, Vec<u8>>,
}

/// Applies the edits to `source`, innermost first and never overlapping.
fn apply(source: &str, edits: &mut [Edit]) -> Result<Vec<u8>, WriteError> {
    edits.sort_by_key(|e| (e.at.start, e.at.end));
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0usize;
    for edit in edits.iter() {
        if edit.at.start < cursor {
            // Two edits over the same bytes would produce a part whose content
            // depends on the order they were planned in. Refusing is the only
            // answer that cannot be wrong in a way nobody sees.
            return Err(WriteError::Invalid(format!(
                "two rewrites of `{CONTENT_PART}` overlap at byte {}",
                edit.at.start
            )));
        }
        out.push_str(&source[cursor..edit.at.start]);
        out.push_str(&edit.text);
        cursor = edit.at.end;
    }
    out.push_str(&source[cursor..]);
    Ok(out.into_bytes())
}

/// Where the automatic styles a rewritten body needs can be spliced in.
///
/// Inside `<office:automatic-styles>` when there is one — after its last child,
/// so no start tag has to be found by scanning attribute values — and otherwise
/// as a whole new element before `<office:body>`.
fn style_anchor(root: &Element, styles: &str) -> Option<Edit> {
    if let Some(auto) = root.child("automatic-styles") {
        if let Some(last) = auto.children().last() {
            return Some(Edit {
                at: last.span.end..last.span.end,
                text: styles.to_string(),
            });
        }
        // Written in the empty form, or with nothing but whitespace in it: the
        // whole element is replaced by one that has the styles in it.
        return Some(Edit {
            at: auto.span.clone(),
            text: format!("{}{styles}</{}>", open_tag(auto), qualified(auto)),
        });
    }
    let body = root.child("body")?;
    Some(Edit {
        at: body.span.start..body.span.start,
        text: format!("<office:automatic-styles>{styles}</office:automatic-styles>"),
    })
}

fn qualified(element: &Element) -> String {
    if element.prefix.is_empty() {
        element.name.clone()
    } else {
        format!("{}:{}", element.prefix, element.name)
    }
}

/// The element's start tag, rebuilt from the parse rather than sliced out of
/// the source: an attribute value may legally contain `>`, so scanning for one
/// is a bug waiting for the file that has it.
fn open_tag(element: &Element) -> String {
    let mut out = format!("<{}", qualified(element));
    for (name, value) in &element.attrs {
        out.push_str(&format!(r#" {}="{}""#, name, esc_attr(value)));
    }
    out.push('>');
    out
}

// ---------------------------------------------------------------------------
// Deciding what changed
// ---------------------------------------------------------------------------

/// The body a shape carries, when it carries one this writer can splice.
fn body_of(shape: &Shape) -> Option<&Vec<Block>> {
    match &shape.kind {
        ShapeKind::Placeholder(ph) => Some(&ph.body),
        ShapeKind::TextBox { body } => Some(body),
        _ => None,
    }
}

/// What a shape is, in one word, for a warning that has to name it.
fn kind_name(shape: &Shape) -> &'static str {
    match &shape.kind {
        ShapeKind::Placeholder(_) => "placeholder",
        ShapeKind::TextBox { .. } => "text box",
        ShapeKind::Picture(_) => "picture",
        ShapeKind::Table(_) => "table",
        ShapeKind::Chart(_) => "chart",
        ShapeKind::Group(_) => "group",
        ShapeKind::Raw(_) => "preserved object",
    }
}

/// Renders a body the way this writer would write it, into a throwaway report.
///
/// The rendering is the comparison's unit: two bodies are «the same» exactly
/// when this writer would emit the same markup for both, which is blind to the
/// node ids a package never states and to nothing that reaches the file.
fn render(blocks: &[Block], assets: &dyn AssetStore) -> Result<String, WriteError> {
    let mut discard = ConversionReport::new();
    let mut ctx = WriterCtx {
        assets,
        report: &mut discard,
        auto: AutoStyles::with_prefix(STYLE_PREFIX),
        pictures: BTreeMap::new(),
        pic_seq: 0,
        note_seq: 0,
    };
    let mut out = String::new();
    write_blocks(blocks, &mut ctx, &mut out)?;
    Ok(out)
}

/// Renders a body for real: into the caller's report and style table, so that
/// what it loses is reported and what it invents is declared.
fn render_into(
    blocks: &[Block],
    ctx: &mut WriterCtx<'_>,
    element: &Element,
) -> Result<Edit, WriteError> {
    let mut inner = String::new();
    write_blocks(blocks, ctx, &mut inner)?;
    Ok(Edit {
        at: element.span.clone(),
        text: format!("{}{inner}</{}>", open_tag(element), qualified(element)),
    })
}

/// Whether two shapes of a kind this writer does not rewrite say different
/// things.
///
/// A table and a picture carry a [`docsai_model::NodeId`] after a DocMark round
/// trip and never carry one in a package, so `==` on the kind would report an
/// edit on every table of every deck nobody touched. Both are `Block`s the ODF
/// block writer can render, so they get the same treatment a body does. A
/// chart, a group and a preserved object carry no id at all, and are compared
/// as they stand.
fn kinds_differ(now: &Shape, was: &Shape, assets: &dyn AssetStore) -> Result<bool, WriteError> {
    match (&now.kind, &was.kind) {
        (ShapeKind::Table(a), ShapeKind::Table(b)) => {
            Ok(
                render(std::slice::from_ref(&Block::Table(a.clone())), assets)?
                    != render(std::slice::from_ref(&Block::Table(b.clone())), assets)?,
            )
        }
        (ShapeKind::Picture(a), ShapeKind::Picture(b)) => {
            Ok(
                render(std::slice::from_ref(&Block::Image(a.clone())), assets)?
                    != render(std::slice::from_ref(&Block::Image(b.clone())), assets)?,
            )
        }
        (a, b) => Ok(a != b),
    }
}

/// How a shape is recognised on the other side of a round trip.
///
/// A placeholder is its class and its index, which is what ODF and the IR both
/// state about it; anything else is its kind and the `draw:name` it carries.
///
/// **An absent index and index 0 are one index.** 18-B gives the first frame of
/// a `presentation:class` on a page no index at all, because that is what the
/// file says; DocMark-P's rule 1 writes the layout's primary body as the
/// slide's implicit blocks and the parser hands it back at index 0. The two
/// name the same frame, and treating them as different would report an edit on
/// every body of every deck nobody touched.
#[derive(PartialEq, Eq)]
enum Identity {
    Placeholder(String, u32),
    Named(&'static str, Option<String>),
}

/// A shape's identity, in words a warning can carry.
fn say(shape: &Shape) -> String {
    match &shape.kind {
        ShapeKind::Placeholder(ph) => match ph.idx {
            Some(idx) => format!("a `{}` placeholder at index {idx}", ph.ph_type.as_str()),
            None => format!("a `{}` placeholder", ph.ph_type.as_str()),
        },
        _ => match &shape.name {
            Some(name) => format!("a {} named `{name}`", kind_name(shape)),
            None => format!("a {}", kind_name(shape)),
        },
    }
}

fn identity(shape: &Shape) -> Identity {
    match &shape.kind {
        ShapeKind::Placeholder(ph) => {
            Identity::Placeholder(ph.ph_type.as_str().to_string(), ph.idx.unwrap_or(0))
        }
        _ => Identity::Named(kind_name(shape), shape.name.clone()),
    }
}

/// Which package shape each model shape is, or `None` for one the package does
/// not have.
///
/// **Position is the pairing when the counts agree**, and that is not a
/// shortcut: a deck's shapes reach DocMark in reading order and come back in
/// the order they were written, so the *n*-th shape of the model is the *n*-th
/// shape of the package unless one was added or removed. Pairing by identity
/// instead would be worse, because DocMark-P does not carry every ODF class —
/// `corpus/odp/presentation-classes.odp`'s `subtitle` frame comes back as a
/// body, and an identity walk would hand its text to the `outline` frame beside
/// it. Position gets that one right and says so.
///
/// When the counts differ, position cannot be trusted and the pairing is a
/// greedy walk in document order: strictly by identity first, then by kind
/// alone for what is left, so a renamed shape still finds its frame while a
/// *deleted* one leaves a hole the caller is told about rather than a
/// neighbour being rewritten with somebody else's text.
fn align(now: &[Shape], was: &[Shape]) -> Vec<Option<usize>> {
    if now.len() == was.len() {
        return (0..now.len()).map(Some).collect();
    }
    let mut paired: Vec<Option<usize>> = vec![None; now.len()];
    let mut taken = vec![false; was.len()];
    for strict in [true, false] {
        for (slot, shape) in now.iter().enumerate() {
            if paired[slot].is_some() {
                continue;
            }
            let key = identity(shape);
            let found = was.iter().enumerate().find(|(index, other)| {
                !taken[*index]
                    && if strict {
                        identity(other) == key
                    } else {
                        kind_name(other) == kind_name(shape)
                    }
            });
            if let Some((index, _)) = found {
                taken[index] = true;
                paired[slot] = Some(index);
            }
        }
    }
    paired
}

/// The children of a `draw:page` that the reader counted as shapes, in the
/// document order that [`Shape::z_index`] indexes.
fn shape_elements(page: &Element) -> Vec<&Element> {
    page.children()
        .filter(|child| match child.name.as_str() {
            "annotation" | "notes" | "forms" | "layer-set" => false,
            name => !super::is_animation(name, child),
        })
        .collect()
}

fn plan_edits(
    deck: &Presentation,
    before: &Presentation,
    root: &Element,
    source: &str,
    ctx: &mut WriterCtx<'_>,
    plan: &mut Plan,
) -> Result<(), WriteError> {
    let Some(body) = root.path(&["body", "presentation"]) else {
        return Err(WriteError::Invalid(format!(
            "`{CONTENT_PART}` has no <office:presentation> to write into"
        )));
    };
    let pages: Vec<&Element> = body.children_named("page").collect();
    if pages.len() != before.slides.len() {
        return Err(WriteError::Invalid(format!(
            "`{CONTENT_PART}` has {} <draw:page> and its own reader found {} slide(s)",
            pages.len(),
            before.slides.len()
        )));
    }
    if deck.slides.len() != before.slides.len() {
        // Writing a page ODF does not have means writing a `draw:page`, its
        // master reference and its page layout from the model, which is the
        // work 18-D did not take on. It is named rather than copied silently.
        ctx.report.warn(Warning::Degraded {
            what: "the deck's slide list".into(),
            why: format!(
                "the model has {} slide(s) and the preserved package {}; this writer \
                 splices into the pages the package has and does not add or remove one, \
                 so the package's pages were kept",
                deck.slides.len(),
                before.slides.len()
            ),
        });
        return Ok(());
    }

    for (index, ((page, was), now)) in pages
        .iter()
        .zip(&before.slides)
        .zip(&deck.slides)
        .enumerate()
    {
        let label = now
            .name
            .clone()
            .or_else(|| was.name.clone())
            .unwrap_or_else(|| format!("{}", index + 1));
        let elements = shape_elements(page);
        let paired = align(&now.shapes, &was.shapes);
        for (position, shape) in now.shapes.iter().enumerate() {
            let Some(old) = paired[position].map(|index| &was.shapes[index]) else {
                // A shape the model states and the package does not have: a
                // `draw:frame` would have to be written, with a class, a
                // presentation style and a rectangle nobody stated. Named
                // rather than guessed at.
                ctx.report.warn(Warning::Degraded {
                    what: format!("a {} on page `{label}`", kind_name(shape)),
                    why: "the preserved package has no frame it corresponds to, and this \
                          writer splices into the frames the package has rather than \
                          adding one, so it was not written"
                        .into(),
                });
                continue;
            };
            let Some(element) = elements.get(old.z_index as usize) else {
                ctx.report.warn(Warning::Degraded {
                    what: format!("a shape of page `{label}`"),
                    why: "its position in the package could not be recovered, so it was \
                          left as it was"
                        .into(),
                });
                continue;
            };
            if identity(shape) != identity(old) {
                // The frame's `presentation:class` is not something this writer
                // rewrites: it is the shape's identity in the package, and the
                // cascade, the master and the page layout all resolve through
                // it. So a model that names a different one is told that the
                // package's was kept. `corpus/odp/presentation-classes.odp`
                // fires this on a deck nobody edited, and truly: DocMark-P
                // consumes a `subtitle` frame as rule 1's implicit body when
                // the layout declares no body index, and it comes back a body.
                ctx.report.warn(Warning::Degraded {
                    what: format!("the identity of a shape on page `{label}`"),
                    why: format!(
                        "the model calls it {} and the package {}; a frame's \
                         presentation:class is what the cascade resolves through, and \
                         this writer does not rewrite one, so the package's was kept",
                        say(shape),
                        say(old)
                    ),
                });
            }
            match (body_of(shape), body_of(old)) {
                (Some(new_body), Some(old_body)) => {
                    if render(new_body, ctx.assets)? == render(old_body, ctx.assets)? {
                        continue;
                    }
                    let Some(text_box) = element.child("text-box") else {
                        ctx.report.warn(Warning::Degraded {
                            what: format!("the text of a shape on page `{label}`"),
                            why: "the frame it came from has no <draw:text-box> to write \
                                  into, so the edit was not applied"
                                .into(),
                        });
                        continue;
                    };
                    plan.edits.push(render_into(new_body, ctx, text_box)?);
                }
                (None, None) => {
                    if kinds_differ(shape, old, ctx.assets)? {
                        ctx.report.warn(Warning::Degraded {
                            what: format!("the {} on page `{label}`", kind_name(old)),
                            why: "this writer rewrites text bodies and copies everything \
                                  else, so the change the model states was not written"
                                .into(),
                        });
                    }
                }
                _ => ctx.report.warn(Warning::Degraded {
                    what: format!("a shape on page `{label}`"),
                    why: format!(
                        "the model calls it a {} and the package a {}; this writer does \
                         not turn one kind of shape into another, so the package's was kept",
                        kind_name(shape),
                        kind_name(old)
                    ),
                }),
            }
        }
        for (index, old) in was.shapes.iter().enumerate() {
            if paired.contains(&Some(index)) {
                continue;
            }
            // The other direction, and the one `placeholders-empty.odp` shows:
            // the package has a frame the model no longer states. It is kept —
            // this writer removes nothing — and the difference is named,
            // because a caller who deleted a shape and got it back has to learn
            // that from the report rather than from the file.
            ctx.report.warn(Warning::Degraded {
                what: format!("the {} on page `{label}`", kind_name(old)),
                why: "the model no longer states it, and this writer splices into the \
                      package rather than rebuilding it, so the frame was kept"
                    .into(),
            });
        }
        plan_notes(page, was, now, &label, ctx, plan)?;
        if comments_differ(&now.comments, &was.comments, ctx.assets)? {
            ctx.report.warn(Warning::Degraded {
                what: format!("the comments of page `{label}`"),
                why: "an ODF annotation is written by no writer in this project yet, so \
                      the package's annotations were kept"
                    .into(),
            });
        }
    }
    let _ = source;
    Ok(())
}

/// Whether two comment lists say different things.
///
/// Compared field by field rather than with `PartialEq`, for the same reason
/// the bodies are compared rendered: a deck that has been through DocMark
/// carries a [`docsai_model::NodeId`] on every comment and a package states
/// none, so `==` would report an edit on every annotation of every deck nobody
/// touched — a false warning, which `AGENTS.md` §7 rule 3 forbids exactly as
/// firmly as a missing one.
fn comments_differ(
    now: &[Comment],
    was: &[Comment],
    assets: &dyn AssetStore,
) -> Result<bool, WriteError> {
    if now.len() != was.len() {
        return Ok(true);
    }
    for (a, b) in now.iter().zip(was) {
        if a.author != b.author
            || a.initials != b.initials
            || a.date != b.date
            || a.resolved != b.resolved
            || a.anchor != b.anchor
            || render(&a.blocks, assets)? != render(&b.blocks, assets)?
            || comments_differ(&a.replies, &b.replies, assets)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The speaker notes of one page.
///
/// A `presentation:notes` is a child of its page, and every text frame in it
/// contributes to the one `Vec<Block>` the IR holds — which is why a page with
/// two note frames cannot be written back frame by frame. The corpus has one
/// per page, and a page with more is named rather than guessed at.
fn plan_notes(
    page: &Element,
    was: &docsai_model::presentation::Slide,
    now: &docsai_model::presentation::Slide,
    label: &str,
    ctx: &mut WriterCtx<'_>,
    plan: &mut Plan,
) -> Result<(), WriteError> {
    let new_notes = now.notes.as_deref().unwrap_or(&[]);
    let old_notes = was.notes.as_deref().unwrap_or(&[]);
    if render(new_notes, ctx.assets)? == render(old_notes, ctx.assets)? {
        return Ok(());
    }
    let boxes: Vec<&Element> = page
        .children_named("notes")
        .flat_map(|notes| notes.children_named("frame"))
        .filter_map(|frame| frame.child("text-box"))
        .collect();
    match boxes.len() {
        0 => ctx.report.warn(Warning::Degraded {
            what: format!("the speaker notes of page `{label}`"),
            why: "the package has no <presentation:notes> frame to write them into, and \
                  this writer splices rather than adding a notes page"
                .into(),
        }),
        1 => plan.edits.push(render_into(new_notes, ctx, boxes[0])?),
        n => ctx.report.warn(Warning::Degraded {
            what: format!("the speaker notes of page `{label}`"),
            why: format!(
                "the package spreads them over {n} frames and the IR holds one body, so \
                 which frame each block belongs to is not recoverable; the notes were kept \
                 as they were"
            ),
        }),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The manifest
// ---------------------------------------------------------------------------

/// `META-INF/manifest.xml` with `entries` added to it, spliced rather than
/// rewritten so that everything already declared keeps its bytes.
fn manifest_with(package: &Package, entries: &[(String, String)]) -> Result<Vec<u8>, String> {
    let source = package
        .text(MANIFEST_PART)
        .map_err(|e| e.to_string())?
        .to_string();
    let root = Element::parse(MANIFEST_PART, source.as_bytes()).map_err(|e| e.to_string())?;
    let last = root
        .children()
        .last()
        .ok_or_else(|| "it declares no file entry".to_string())?;
    let mut added = String::new();
    for (path, mime) in entries {
        if root
            .children()
            .any(|e| e.attr("full-path") == Some(path.as_str()))
        {
            continue;
        }
        added.push_str(&format!(
            r#"<manifest:file-entry manifest:full-path="{}" manifest:media-type="{}"/>"#,
            esc_attr(path),
            esc_attr(mime)
        ));
    }
    let at = last.span.end;
    Ok(format!("{}{added}{}", &source[..at], &source[at..]).into_bytes())
}

// ---------------------------------------------------------------------------
// The archive
// ---------------------------------------------------------------------------

/// Copies an ODF package entry by entry, preserving what a rebuilt one loses.
///
/// Member order, compression method, CRC and timestamp all come from the source
/// entry, and the compressed bytes are copied as they stand: no entry is
/// inflated on the way through, so no entry can come out re-deflated by a
/// different encoder into different bytes. `mimetype` therefore stays the first
/// member and stays stored, with no rule about it written down anywhere here.
///
/// The `zip` writer will not leave the **external file attributes** of a
/// central-directory entry alone — it rebuilds them from the source entry's
/// Unix mode and ORs `S_IFREG` in — so a package written by Python's `zipfile`
/// with mode `0o600` would come back as `0o100600`. Nothing in ODF reads that
/// field, but the deck's *identity* does: a preserved package is addressed by
/// the content hash of its bytes, so a permission bit that moves renames the
/// file and makes an untouched round trip look like an edit. They are put back
/// as they were once the archive is built.
fn reemit<W: Write + Seek>(
    package: &[u8],
    replacements: &BTreeMap<String, Vec<u8>>,
    additions: &[(String, Vec<u8>)],
    mut writer: W,
) -> Result<(), WriteError> {
    let mut source = zip::ZipArchive::new(Cursor::new(package))?;
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..source.len() {
        let entry = source.by_index_raw(index)?;
        match replacements.get(entry.name()) {
            Some(bytes) => {
                let options = entry.options();
                let name = entry.name().to_string();
                drop(entry);
                out.start_file(name, options)?;
                out.write_all(bytes)?;
            }
            None => out.raw_copy_file(entry)?,
        }
    }
    for (name, bytes) in additions {
        out.start_file(
            name.as_str(),
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .last_modified_time(
                    zip::DateTime::from_date_and_time(2020, 1, 1, 0, 0, 0)
                        .unwrap_or_else(|_| zip::DateTime::default_for_write()),
                ),
        )?;
        out.write_all(bytes)?;
    }
    let mut bytes = out.finish()?.into_inner();
    restore_external_attributes(package, &mut bytes);
    writer.write_all(&bytes)?;
    Ok(())
}

/// Puts each entry's **external file attributes** and its **version needed to
/// extract** back to what the source package said, so a package copied through
/// [`reemit`] is byte for byte the one that was read.
///
/// Two fields, and neither is one the `zip` writer will state verbatim:
///
/// * The external attributes are rebuilt from the source entry's Unix mode with
///   `S_IFREG` ORed in, so `0o600` comes back `0o100600`. They live only in the
///   central directory.
/// * The version needed to extract is derived from the compression method — 1.0
///   for a stored entry, 2.0 for a deflated one — while Python's `zipfile`,
///   which wrote every fixture in `corpus/odp`, stamps 2.0 on all of them, and
///   the same two bytes appear again as the *version made by*. **`mimetype` is
///   stored, so this is the one field that moves on the one member ODF cares
///   most about**, and it is written in three places: twice in the central
///   directory record and once in the entry's own local header.
///
/// Both corrections are fixed-width overwrites at offsets the central directory
/// states, never a re-encode. An archive this simple reader does not fully
/// understand — zip64, truncated, a record that does not start where the
/// previous one ended — is left exactly as the writer produced it: these are
/// cosmetic corrections, and a wrong guess at an offset would not be.
fn restore_external_attributes(source: &[u8], written: &mut [u8]) {
    const LOCAL: [u8; 4] = [b'P', b'K', 3, 4];
    let Some(before) = central_directory(source) else {
        return;
    };
    let Some(after) = central_directory(written) else {
        return;
    };
    let originals: BTreeMap<&str, &DirectoryEntry> = before
        .iter()
        .map(|entry| (entry.name.as_str(), entry))
        .collect();
    for entry in &after {
        let Some(original) = originals.get(entry.name.as_str()) else {
            continue;
        };
        written[entry.attributes_at..entry.attributes_at + 4]
            .copy_from_slice(&original.external_attributes);
        written[entry.versions_at..entry.versions_at + 4].copy_from_slice(&original.versions);
        // The "version needed" again, in the local header the record points at.
        let at = entry.local_header_at;
        if at + 6 <= written.len() && written[at..at + 4] == LOCAL {
            written[at + 4..at + 6].copy_from_slice(&original.versions[2..4]);
        }
    }
}

/// One central-directory record: the member's name, the two fields the `zip`
/// writer will not state verbatim, and where each of them lives.
struct DirectoryEntry {
    name: String,
    external_attributes: [u8; 4],
    attributes_at: usize,
    /// The *version made by* and the *version needed to extract*, in that
    /// order, as the four bytes the record states them in.
    versions: [u8; 4],
    versions_at: usize,
    local_header_at: usize,
}

/// The central directory of a ZIP archive, read by walking it from the
/// end-of-central-directory record rather than by scanning for signatures — a
/// deflated part can contain any four bytes, including `PK\x01\x02`.
///
/// `None` for anything this simple reader does not fully understand: a zip64
/// archive, a truncated one, a record that does not start where the previous
/// one ended.
fn central_directory(archive: &[u8]) -> Option<Vec<DirectoryEntry>> {
    const EOCD: [u8; 4] = [b'P', b'K', 5, 6];
    const RECORD: [u8; 4] = [b'P', b'K', 1, 2];
    let start = archive.len().checked_sub(22)?;
    let eocd = (0..=start.min(65_535))
        .map(|back| start - back)
        .find(|at| archive[*at..*at + 4] == EOCD)?;
    let count = u16::from_le_bytes([archive[eocd + 10], archive[eocd + 11]]) as usize;
    let mut at = u32::from_le_bytes([
        archive[eocd + 16],
        archive[eocd + 17],
        archive[eocd + 18],
        archive[eocd + 19],
    ]) as usize;
    if at == u32::MAX as usize {
        // zip64: the offset is in a record this reader does not parse.
        return None;
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        if at + 46 > archive.len() || archive[at..at + 4] != RECORD {
            return None;
        }
        let name_len = u16::from_le_bytes([archive[at + 28], archive[at + 29]]) as usize;
        let extra_len = u16::from_le_bytes([archive[at + 30], archive[at + 31]]) as usize;
        let comment_len = u16::from_le_bytes([archive[at + 32], archive[at + 33]]) as usize;
        let name_at = at + 46;
        let name = std::str::from_utf8(archive.get(name_at..name_at + name_len)?)
            .ok()?
            .to_string();
        entries.push(DirectoryEntry {
            name,
            external_attributes: [
                archive[at + 38],
                archive[at + 39],
                archive[at + 40],
                archive[at + 41],
            ],
            attributes_at: at + 38,
            versions: [
                archive[at + 4],
                archive[at + 5],
                archive[at + 6],
                archive[at + 7],
            ],
            versions_at: at + 4,
            local_header_at: u32::from_le_bytes([
                archive[at + 42],
                archive[at + 43],
                archive[at + 44],
                archive[at + 45],
            ]) as usize,
        });
        at = name_at + name_len + extra_len + comment_len;
    }
    Some(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::presentation::{PhType, Placeholder};

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/odp")
            .join(name)
    }

    fn ph(kind: PhType, idx: Option<u32>) -> Shape {
        Shape::new(
            0,
            ShapeKind::Placeholder(Placeholder {
                ph_type: kind,
                idx,
                ..Default::default()
            }),
        )
    }

    /// 15-B's rule at the crate's own door: read a deck, write it straight back
    /// with no DocMark in between, and nothing at all is different — not the
    /// bytes, and not the report.
    #[test]
    fn a_deck_nobody_edited_produces_no_edit_at_all() {
        let path = fixture("basic-slides.odp");
        let original = std::fs::read(&path).expect("the fixture");
        let mut assets = docsai_model::MemoryAssetStore::new();
        let (document, _) =
            super::super::read(Cursor::new(&original), &mut assets).expect("reads the deck");

        let mut out = Cursor::new(Vec::new());
        let report = write_odp(&document, &assets, &mut out).expect("writes the deck");
        assert!(
            report.warnings.is_empty(),
            "an untouched deck warned: {:?}",
            report.warnings
        );
        assert_eq!(
            out.into_inner(),
            original,
            "an untouched deck did not come back byte for byte"
        );
    }

    /// A deck with no preserved package is an error and says which one.
    #[test]
    fn a_deck_with_no_skeleton_is_refused() {
        let deck = Presentation::default();
        let assets = docsai_model::MemoryAssetStore::new();
        let error = write_odp(
            &Document::Presentation(deck),
            &assets,
            &mut Cursor::new(Vec::new()),
        )
        .expect_err("a deck with no package cannot be written");
        assert!(
            error.to_string().contains("preserved package"),
            "the refusal does not say why: {error}"
        );
    }

    /// Position is the pairing when the counts agree, even when the classes do
    /// not: DocMark-P does not carry every ODF class, and pairing by identity
    /// would hand a `subtitle` frame's text to the `outline` frame beside it.
    #[test]
    fn shapes_pair_by_position_when_the_counts_agree() {
        let now = vec![
            ph(PhType::Title, None),
            ph(PhType::Body, Some(0)),
            ph(PhType::Body, Some(1)),
        ];
        let was = vec![
            ph(PhType::Title, None),
            ph(PhType::Subtitle, None),
            ph(PhType::Body, Some(1)),
        ];
        assert_eq!(align(&now, &was), vec![Some(0), Some(1), Some(2)]);
    }

    /// An absent index and index 0 name the same frame: the reader gives the
    /// first frame of a class no index, and DocMark-P's implicit body comes
    /// back at 0.
    #[test]
    fn an_absent_index_and_index_zero_are_one_identity() {
        assert!(identity(&ph(PhType::Body, None)) == identity(&ph(PhType::Body, Some(0))));
        assert!(identity(&ph(PhType::Body, None)) != identity(&ph(PhType::Body, Some(1))));
    }

    /// When a shape is missing, the survivors still find their own frames and
    /// the one the model no longer states is left unpaired for the caller to be
    /// told about.
    #[test]
    fn a_deleted_shape_leaves_a_hole_rather_than_shifting_its_neighbours() {
        let now = vec![
            ph(PhType::Title, None),
            ph(PhType::Other("text".into()), None),
        ];
        let was = vec![
            ph(PhType::Title, None),
            ph(PhType::Body, None),
            ph(PhType::Other("text".into()), None),
        ];
        let paired = align(&now, &was);
        assert_eq!(paired, vec![Some(0), Some(2)]);
        assert!(!paired.contains(&Some(1)), "the deleted frame was claimed");
    }
}

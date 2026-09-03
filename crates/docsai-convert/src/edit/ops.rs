//! The ten operations of `docs/edit-operations.md` §6, applied to the IR.
//!
//! Every one of them is a function on the document model. Nothing here knows
//! what a `.pptx` is; the writers underneath do, and §8.1's join — a writer
//! refusal fails the whole request — is the transaction's business, not this
//! module's.
//!
//! # Ordering, and what it costs a caller
//!
//! §4: the operations form an ordered list applied to one in-memory document,
//! and §5.3: a target is resolved **at the moment its operation is applied**.
//! Both are implemented literally — [`Session::apply`] resolves inside the loop
//! and never before it. The consequence is the one §5.3 states and it is worth
//! saying in the code as well as in the document: *ids are stable, positions are
//! not.*
//!
//! - `[delete n7, replace_text n9]` — the second operation is unaffected,
//!   whatever `n7` was and wherever it was.
//! - `[delete n7, replace_text doc/4]` — if `n7` was `doc/2`, then `doc/4` now
//!   names what was `doc/5`. The caller wrote both against one reading of the
//!   document and the document moved under it.
//!
//! A caller that batches a structural change before an index-path target is
//! therefore addressing a document it has not read. The cure is free: every
//! operation that creates a node returns its id (§5.4), and an id target is
//! immune. This is the honest rule rather than the convenient one — the
//! convenient one, resolving every target against the document as it was when
//! the request arrived, would make `insert_after doc/1` followed by
//! `insert_after doc/2` insert both fragments in the same place.

// A refusal is a large value and a rare one: it carries the operation's name,
// its target, and both sides of the mismatch, because §8 requires that. Boxing
// it would put an allocation on every refusal path to save moving a struct that
// is only built when a request is already being rejected — and the ordinary
// path, `Ok(())`, pays nothing either way.
#![allow(clippy::result_large_err)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use docsai_model::addressing::{
    check_if_match, ids_in_document_order, resolve_path, with_place_mut, with_seq_mut, Etag,
    NodeId, Place, PlaceMut, PreconditionError, SeqKind, SeqMut, Step, Target, TargetError,
};
use docsai_model::assets::AssetStore;
use docsai_model::image::ImageRef;
use docsai_model::presentation::{Shape, ShapeKind, Slide};
use docsai_model::sheet::{CellRef, CellValue, Formula, FormulaDialect, Sheet};
use docsai_model::style::{Align, FontProps, StyleId, Underline};
use docsai_model::text::{
    Block, Comment, Inline, ListItem, Paragraph, RunProps, Section, TableRow,
};
use docsai_model::units::Length;
use docsai_model::{ConversionReport, Document, Format, Warning};

use super::fragment::{self, Fragment, FragmentError};
use super::request::{DocClass, Operation, Refusal, Refused, Scalar};

/// The largest cell address the formats this project writes accept, as the
/// **0-based** indices `CellRef` uses: xlsx and ods agree on 16 384 columns, and
/// xlsx's 1 048 576 rows is the smaller of the two row limits, so it is the one
/// an address is checked against.
const COLS: u32 = 16_384;
const ROWS: u32 = 1_048_576;

/// One batch, mid-application.
pub(crate) struct Session<'a> {
    doc: &'a mut Document,
    store: &'a mut dyn AssetStore,
    /// Where a relative `path` in `replace_image`, and a relative image in an
    /// inserted fragment, are resolved from: the directory of the document
    /// being edited.
    base: Option<PathBuf>,
    /// The package the document came from, where it came from one. `set_cell`
    /// needs it and nothing else does: a formula is stored in the sheet's own
    /// dialect (§6.7), and a workbook with no formula in it yet has nothing to
    /// infer one from.
    format: Option<Format>,
    pub report: ConversionReport,
    created: Vec<NodeId>,
    deleted: Vec<String>,
    changed: Vec<String>,
}

impl<'a> Session<'a> {
    pub(crate) fn new(
        doc: &'a mut Document,
        store: &'a mut dyn AssetStore,
        base: Option<PathBuf>,
        format: Option<Format>,
    ) -> Self {
        Session {
            doc,
            store,
            base,
            format,
            report: ConversionReport::new(),
            created: Vec::new(),
            deleted: Vec::new(),
            changed: Vec::new(),
        }
    }

    pub(crate) fn created(&self) -> &[NodeId] {
        &self.created
    }

    pub(crate) fn deleted(&self) -> &[String] {
        &self.deleted
    }

    pub(crate) fn changed(&self) -> &[String] {
        &self.changed
    }

    /// The whole-list check of §4, run before anything is applied.
    ///
    /// It covers everything knowable without touching the document: the target
    /// grammar, the `after`/`before` pair of `move`, `set_props`'s closed key
    /// set, the two forms of `replace_image`, and the class refusals §6.6 and
    /// §6.8 say are «raised before the target is even resolved». A typo costs
    /// nothing and says so.
    pub(crate) fn validate(&self, operations: &[Operation]) -> Result<(), Refused> {
        let class = DocClass::of(self.doc);
        for (index, op) in operations.iter().enumerate() {
            let refuse = |refusal| Refused {
                index,
                op: op.name().to_string(),
                refusal,
            };
            // §6.6 and §6.8: the class refusal comes «before the target is
            // even resolved», so it comes before the target is even parsed.
            let wrong_class = match op {
                Operation::DuplicateSlide { .. } if class != DocClass::Presentation => {
                    Some(("duplicate_slide", "presentations"))
                }
                Operation::SetNotes { .. } if class != DocClass::Presentation => {
                    Some(("set_notes", "presentations"))
                }
                Operation::SetCell { .. } if class != DocClass::Workbook => {
                    Some(("set_cell", "workbooks"))
                }
                _ => None,
            };
            if let Some((op_name, applies_to)) = wrong_class {
                return Err(refuse(Refusal::NotApplicableToClass {
                    op: op_name,
                    class,
                    applies_to,
                }));
            }
            Target::parse(op.target()).map_err(|e| refuse(target_refusal(e)))?;
            if let Some(etag) = op.if_match() {
                if !Etag::new(etag).is_valid() {
                    return Err(refuse(Refusal::MalformedTarget {
                        target: op.target().to_string(),
                        reason: format!("`{etag}` is not an etag: six lowercase hex digits"),
                    }));
                }
            }
            match op {
                Operation::Move {
                    after,
                    before,
                    target,
                    ..
                } => {
                    match (after, before) {
                        (Some(_), None) | (None, Some(_)) => {}
                        _ => {
                            return Err(refuse(Refusal::MissingField {
                                op: "move",
                                field: "exactly one of `after` or `before`",
                            }))
                        }
                    }
                    let destination = after.as_ref().or(before.as_ref()).map(String::as_str);
                    if let Some(destination) = destination {
                        Target::parse(destination).map_err(|e| refuse(target_refusal(e)))?;
                        if destination.trim_start_matches('#') == target.trim_start_matches('#') {
                            return Err(refuse(Refusal::CycleRefused {
                                target: target.clone(),
                                destination: destination.to_string(),
                            }));
                        }
                    }
                }
                Operation::SetProps { props, .. } => {
                    for key in props.keys() {
                        if !PROPERTY_KEYS.contains(&key.as_str()) {
                            return Err(refuse(Refusal::UnknownField {
                                op: "set_props",
                                field: key.clone(),
                                known: PROPERTY_LIST,
                            }));
                        }
                    }
                }
                Operation::SetCell { value: None, .. } => {
                    return Err(refuse(Refusal::MissingField {
                        op: "set_cell",
                        field: "value",
                    }))
                }
                Operation::SetNotes { content: None, .. } => {
                    return Err(refuse(Refusal::MissingField {
                        op: "set_notes",
                        field: "content",
                    }))
                }
                Operation::ReplaceImage {
                    path,
                    content_base64,
                    filename,
                    ..
                } => match (path, content_base64) {
                    (Some(_), None) => {}
                    (None, Some(_)) if filename.is_some() => {}
                    (None, Some(_)) => {
                        return Err(refuse(Refusal::MissingField {
                            op: "replace_image",
                            field: "filename",
                        }))
                    }
                    _ => {
                        return Err(refuse(Refusal::MissingField {
                            op: "replace_image",
                            field: "exactly one of `path` or `content_base64`",
                        }))
                    }
                },
                _ => {}
            }
        }
        Ok(())
    }

    /// Applies one operation, resolving its target now (§5.3).
    pub(crate) fn apply(&mut self, index: usize, op: &Operation) -> Result<(), Refused> {
        self.apply_inner(op).map_err(|refusal| Refused {
            index,
            op: op.name().to_string(),
            refusal,
        })
    }

    fn apply_inner(&mut self, op: &Operation) -> Result<(), Refusal> {
        match op {
            Operation::ReplaceText { target, text, .. } => self.replace_text(op, target, text),
            Operation::SetProps { target, props, .. } => {
                let props = props.clone();
                self.set_props(op, target, &props)
            }
            Operation::InsertAfter {
                target, content, ..
            } => self.insert(op, target, content, After::Yes),
            Operation::InsertBefore {
                target, content, ..
            } => self.insert(op, target, content, After::No),
            Operation::Delete { target, .. } => self.delete(op, target),
            Operation::Move {
                target,
                after,
                before,
                ..
            } => {
                let (destination, side) = match (after, before) {
                    (Some(a), None) => (a.as_str(), After::Yes),
                    (None, Some(b)) => (b.as_str(), After::No),
                    _ => {
                        return Err(Refusal::MissingField {
                            op: "move",
                            field: "exactly one of `after` or `before`",
                        })
                    }
                };
                self.move_node(op, target, destination, side)
            }
            Operation::DuplicateSlide { target, .. } => self.duplicate_slide(op, target),
            Operation::SetCell {
                target,
                cell,
                value,
                ..
            } => self.set_cell(op, target, cell, value.as_ref().and_then(|v| v.as_ref())),
            Operation::SetNotes {
                target, content, ..
            } => self.set_notes(op, target, content.as_ref().and_then(|c| c.as_deref())),
            Operation::ReplaceImage {
                target,
                path,
                content_base64,
                filename,
                alt,
                ..
            } => self.replace_image(
                op,
                target,
                path.as_deref(),
                content_base64.as_deref(),
                filename.as_deref(),
                alt.as_deref(),
            ),
        }
    }

    /// §5.3 plus §7 in one step: resolve the target now, then check `if_match`.
    fn locate(&self, op: &Operation, target: &str) -> Result<(Target, Vec<Step>), Refusal> {
        let parsed = Target::parse(target).map_err(target_refusal)?;
        let (place, path) = resolve_path(self.doc, &parsed).map_err(target_refusal)?;
        let etag = op.if_match().map(Etag::new);
        check_if_match(&place, &parsed, etag.as_ref()).map_err(precondition_refusal)?;
        Ok((parsed, path))
    }

    /// Records a target as changed, under the name the caller used.
    ///
    /// §6.1 and its neighbours say an operation «returns `changed: [target]`»,
    /// and §5.6's example shows an id, because an id *is* the canonical form of
    /// a target that names an addressable node. A paragraph of ordinary prose
    /// carries no id at all (Phase 10: «a plain run of text is reached by
    /// relative path instead, which keeps the id noise out of ordinary prose»),
    /// so `replace_text doc/3` has nothing but `doc/3` to report — and `doc/3`
    /// is exactly what the caller's next `if_match` will name.
    fn note(&mut self, target: &str) {
        if !self.changed.iter().any(|t| t == target) {
            self.changed.push(target.to_string());
        }
    }

    // ---------------------------------------------------------------- 6.1

    fn replace_text(&mut self, op: &Operation, target: &str, text: &str) -> Result<(), Refusal> {
        let (parsed, path) = self.locate(op, target)?;
        let canonical = parsed.canonical();
        {
            let (place, _) = resolve_path(self.doc, &parsed).map_err(target_refusal)?;
            text_kind_check("replace_text", &canonical, &place)?;
        }
        let collapsed = with_place_mut(self.doc, &path, &mut |place| match place {
            PlaceMut::Block(Block::Paragraph(p)) => set_paragraph_text(p, text),
            PlaceMut::Block(Block::Heading(h)) => set_paragraph_text(&mut h.paragraph, text),
            PlaceMut::Shape(shape) => match &mut shape.kind {
                ShapeKind::Placeholder(ph) => set_body_text(&mut ph.body, text),
                ShapeKind::TextBox { body } => set_body_text(body, text),
                _ => 0,
            },
            _ => 0,
        })
        .unwrap_or(0);
        // §6.1: nothing is warned when one run, or several identical ones, were
        // replaced — a warning fired on every retitle would be false in the
        // ordinary case.
        if collapsed > 1 {
            self.report.warn(Warning::Degraded {
                what: format!("`{canonical}`"),
                why: format!(
                    "replace_text collapsed {collapsed} distinct run styles onto one string; \
                     the properties of the first run were kept"
                ),
            });
        }
        self.note(&canonical);
        Ok(())
    }

    // ---------------------------------------------------------------- 6.2

    fn set_props(
        &mut self,
        op: &Operation,
        target: &str,
        props: &std::collections::BTreeMap<String, Option<Scalar>>,
    ) -> Result<(), Refusal> {
        let (parsed, path) = self.locate(op, target)?;
        let canonical = parsed.canonical();
        {
            let (place, _) = resolve_path(self.doc, &parsed).map_err(target_refusal)?;
            text_kind_check("set_props", &canonical, &place)?;
        }
        let plan = PropPlan::parse(props, self.doc.styles())?;
        with_place_mut(self.doc, &path, &mut |place| match place {
            PlaceMut::Block(Block::Paragraph(p)) => plan.apply_paragraph(p),
            PlaceMut::Block(Block::Heading(h)) => plan.apply_paragraph(&mut h.paragraph),
            PlaceMut::Shape(shape) => {
                let body = match &mut shape.kind {
                    ShapeKind::Placeholder(ph) => &mut ph.body,
                    ShapeKind::TextBox { body } => body,
                    _ => return,
                };
                for block in body.iter_mut() {
                    match block {
                        Block::Paragraph(p) => plan.apply_paragraph(p),
                        Block::Heading(h) => plan.apply_paragraph(&mut h.paragraph),
                        _ => {}
                    }
                }
            }
            _ => {}
        });
        self.note(&canonical);
        Ok(())
    }

    // ---------------------------------------------------------------- 6.3

    fn insert(
        &mut self,
        op: &Operation,
        target: &str,
        content: &str,
        side: After,
    ) -> Result<(), Refusal> {
        let name = op.name();
        let (parsed, path) = self.locate(op, target)?;
        let canonical = parsed.canonical();

        // `doc` is a sequence element of nothing, and §5.1 says it exists so a
        // caller can say «at the top» in a document whose section is not
        // addressable. So `insert_before doc` prepends and `insert_after doc`
        // appends to the document's own sequence.
        let (seq_path, at) = if path.is_empty() {
            (None, side)
        } else {
            (Some(path.clone()), side)
        };
        let _ = at;

        let (kind, len, index) = match &seq_path {
            Some(path) => with_seq_mut(self.doc, path, &mut |seq, index| {
                (seq.kind(), seq.len(), index)
            })
            .ok_or_else(|| Refusal::WrongKind {
                op: name,
                target: canonical.clone(),
                got: describe_path(self.doc, &parsed),
                takes: "a node that is an element of a sequence",
                because: String::new(),
            })?,
            None => {
                let kind = root_seq_kind(self.doc);
                let len = root_seq_len(self.doc);
                (kind, len, if side == After::Yes { len } else { 0 })
            }
        };
        let position = match (&seq_path, side) {
            (None, After::Yes) => len,
            (None, After::No) => 0,
            (Some(_), After::Yes) => index + 1,
            (Some(_), After::No) => index,
        };

        let fragment = fragment::parse_for(
            kind,
            content,
            self.base.as_deref(),
            self.store,
            &mut self.report,
        )
        .map_err(|e| fragment_refusal(e, &canonical, kind))?;

        let taken = ids_of_fragment(&fragment);
        let present: BTreeSet<NodeId> = ids_in_document_order(self.doc)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        for id in &taken {
            if present.contains(id) {
                return Err(Refusal::DuplicateId { id: id.0.clone() });
            }
        }

        let before: BTreeSet<NodeId> = present;
        let spliced = match &seq_path {
            Some(path) => with_seq_mut(self.doc, path, &mut |seq, _| {
                splice(seq, position, &fragment)
            })
            .unwrap_or(false),
            None => root_splice(self.doc, position, &fragment),
        };
        if !spliced {
            return Err(Refusal::ContentKindMismatch {
                target: canonical,
                expected: kind.to_string(),
                got: fragment.kind().to_string(),
            });
        }
        self.register_created(&before);
        Ok(())
    }

    // ---------------------------------------------------------------- 6.4

    fn delete(&mut self, op: &Operation, target: &str) -> Result<(), Refusal> {
        let (parsed, path) = self.locate(op, target)?;
        let canonical = parsed.canonical();
        if path.is_empty() {
            return Err(Refusal::WrongKind {
                op: "delete",
                target: canonical,
                got: "document".into(),
                takes: "a node that is an element of a sequence, a comment or a footnote",
                because: ": the document is not inside anything".into(),
            });
        }
        self.refuse_last_of_kind(&parsed, &canonical)?;

        // A footnote and an inline comment are reached *through* the inline
        // that holds them; the element to remove is that inline.
        let mut path = path;
        let through_inline = matches!(
            path.last(),
            Some(Step::IntoFootnote | Step::IntoComment | Step::IntoImage)
        );
        let into_comment = matches!(path.last(), Some(Step::IntoComment));
        if through_inline {
            path.pop();
        }

        let removed = with_seq_mut(self.doc, &path, &mut |seq, index| {
            remove_at(seq, index, into_comment)
        })
        .unwrap_or(false);
        if !removed {
            return Err(Refusal::WrongKind {
                op: "delete",
                target: canonical,
                got: describe_path(self.doc, &parsed),
                takes: "a node that is an element of a sequence, a comment or a footnote",
                because: String::new(),
            });
        }
        self.deleted.push(canonical);
        Ok(())
    }

    fn refuse_last_of_kind(&self, parsed: &Target, canonical: &str) -> Result<(), Refusal> {
        let (place, _) = resolve_path(self.doc, parsed).map_err(target_refusal)?;
        let last = |kind| {
            Err(Refusal::LastOfKind {
                target: canonical.to_string(),
                kind,
            })
        };
        match (&place, &*self.doc) {
            (Place::Slide(_), Document::Presentation(deck)) if deck.slides.len() <= 1 => {
                last("slide")
            }
            (Place::Sheet(_), Document::Workbook(book)) => {
                let visible = book.sheets.iter().filter(|s| !s.hidden).count();
                let this_one_visible = matches!(&place, Place::Sheet(s) if !s.hidden);
                if book.sheets.len() <= 1 || (this_one_visible && visible <= 1) {
                    last("visible sheet")
                } else {
                    Ok(())
                }
            }
            (Place::Section(_), Document::Text(text)) if text.sections.len() <= 1 => {
                last("section")
            }
            _ => Ok(()),
        }
    }

    // ---------------------------------------------------------------- 6.5

    fn move_node(
        &mut self,
        op: &Operation,
        target: &str,
        destination: &str,
        side: After,
    ) -> Result<(), Refusal> {
        let (parsed, path) = self.locate(op, target)?;
        let canonical = parsed.canonical();
        let dest_parsed = Target::parse(destination).map_err(target_refusal)?;
        let (_, dest_path) = resolve_path(self.doc, &dest_parsed).map_err(target_refusal)?;
        let dest_canonical = dest_parsed.canonical();

        if path.is_empty() || dest_path.is_empty() {
            return Err(Refusal::WrongKind {
                op: "move",
                target: if path.is_empty() {
                    canonical
                } else {
                    dest_canonical
                },
                got: "document".into(),
                takes: "a node that is an element of a sequence",
                because: ": the document is not inside anything".into(),
            });
        }
        // §6.5 `CycleRefused`: the destination is inside the target's subtree,
        // which includes the target itself.
        if dest_path.len() >= path.len() && dest_path[..path.len()] == path[..] {
            return Err(Refusal::CycleRefused {
                target: canonical,
                destination: dest_canonical,
            });
        }

        let kinds = with_seq_mut(self.doc, &path, &mut |seq, _| seq.kind());
        let dest_kinds = with_seq_mut(self.doc, &dest_path, &mut |seq, _| seq.kind());
        match (kinds, dest_kinds) {
            (Some(a), Some(b)) if a == b => {}
            (Some(a), Some(b)) => {
                return Err(Refusal::WrongKind {
                    op: "move",
                    target: canonical,
                    got: a.to_string(),
                    takes: "a destination whose sequence holds the same kind",
                    because: format!(": `{dest_canonical}` sits among {b}"),
                })
            }
            _ => {
                return Err(Refusal::WrongKind {
                    op: "move",
                    target: canonical,
                    got: describe_path(self.doc, &parsed),
                    takes: "a node that is an element of a sequence",
                    because: String::new(),
                })
            }
        }

        // §6.5 `GridMismatch`, checked before anything is detached.
        if kinds == Some(SeqKind::Rows) {
            // The borrow checker forbids two live `with_seq_mut` calls, so the
            // two widths are taken one after the other rather than compared in
            // one closure.
            let got = row_width(self.doc, &path);
            let takes = row_width(self.doc, &dest_path);
            if let (Some(got), Some(takes)) = (got, takes) {
                if got != takes {
                    return Err(Refusal::GridMismatch {
                        target: canonical,
                        destination: dest_canonical,
                        got,
                        takes,
                    });
                }
            }
        }

        let taken = with_seq_mut(self.doc, &path, &mut |seq, index| take_at(seq, index))
            .flatten()
            .ok_or_else(|| Refusal::WrongKind {
                op: "move",
                target: canonical.clone(),
                got: describe_path(self.doc, &parsed),
                takes: "a node that is an element of a sequence",
                because: String::new(),
            })?;

        // The removal moved every later position in the sequence the target
        // came out of. The destination is adjusted rather than re-resolved:
        // re-resolving would silently retarget `move n8 after doc/5` onto a
        // different node, which is the class of surprise §5.3 exists to bound.
        let dest_path = shift_after_removal(&path, dest_path);
        let index = with_seq_mut(self.doc, &dest_path, &mut |_, index| index).ok_or_else(|| {
            Refusal::UnknownTarget {
                target: dest_canonical.clone(),
                reason: format!("unknown target `{dest_canonical}`: it is in no sequence"),
            }
        })?;
        let at = if side == After::Yes { index + 1 } else { index };
        let placed = with_seq_mut(self.doc, &dest_path, &mut |seq, _| put_at(seq, at, &taken))
            .unwrap_or(false);
        if !placed {
            return Err(Refusal::WrongKind {
                op: "move",
                target: canonical,
                got: "a node".into(),
                takes: "a destination whose sequence holds the same kind",
                because: String::new(),
            });
        }
        self.note(&canonical);
        Ok(())
    }

    // ---------------------------------------------------------------- 6.6

    fn duplicate_slide(&mut self, op: &Operation, target: &str) -> Result<(), Refusal> {
        let class = DocClass::of(self.doc);
        if class != DocClass::Presentation {
            return Err(Refusal::NotApplicableToClass {
                op: "duplicate_slide",
                class,
                applies_to: "presentations",
            });
        }
        let (parsed, path) = self.locate(op, target)?;
        let canonical = parsed.canonical();
        let index = match (path.as_slice(), &self.doc) {
            ([Step::Slide(i)], _) => *i,
            _ => {
                return Err(Refusal::WrongKind {
                    op: "duplicate_slide",
                    target: canonical,
                    got: describe_path(self.doc, &parsed),
                    takes: "a slide",
                    because: String::new(),
                })
            }
        };
        let Document::Presentation(deck) = self.doc else {
            unreachable!("the class was checked above");
        };
        let source = deck.slides[index].clone();
        let comments = source.comments.len();
        let mut copy = source;
        copy.comments.clear();
        clear_slide_ids(&mut copy);
        let fresh = refresh_raw_ids(deck, &mut copy);
        let _ = fresh;
        deck.slides.insert(index + 1, copy);

        if comments > 0 {
            // §6.6: only when the source carries comments, because on a slide
            // with none there is nothing true to say.
            self.report.warn(Warning::Degraded {
                what: format!("`{canonical}`"),
                why: format!(
                    "duplicate_slide did not copy {comments} comment(s): a comment asserts that a \
                     named person wrote something about a slide they never saw"
                ),
            });
        }
        let before: BTreeSet<NodeId> = Default::default();
        let _ = before;
        // Everything under the copy lost its id above, so the created set is
        // exactly what the assignment pass hands out.
        let known: BTreeSet<NodeId> = ids_in_document_order(self.doc)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        self.register_created(&known);
        Ok(())
    }

    // ---------------------------------------------------------------- 6.7

    fn set_cell(
        &mut self,
        op: &Operation,
        target: &str,
        cell: &str,
        value: Option<&Scalar>,
    ) -> Result<(), Refusal> {
        let class = DocClass::of(self.doc);
        if class != DocClass::Workbook {
            return Err(Refusal::NotApplicableToClass {
                op: "set_cell",
                class,
                applies_to: "workbooks",
            });
        }
        let (parsed, path) = self.locate(op, target)?;
        let canonical = parsed.canonical();
        {
            let (place, _) = resolve_path(self.doc, &parsed).map_err(target_refusal)?;
            match place {
                Place::Sheet(_) => {}
                other => {
                    return Err(Refusal::WrongKind {
                        op: "set_cell",
                        target: canonical,
                        got: other.describe().to_string(),
                        takes: "a sheet",
                        because: table_cell_hint(&other),
                    })
                }
            }
        }
        let reference = CellRef::parse_a1(cell).ok_or_else(|| Refusal::AddressOutOfRange {
            cell: cell.to_string(),
        })?;
        if reference.col >= COLS || reference.row >= ROWS {
            return Err(Refusal::AddressOutOfRange {
                cell: cell.to_string(),
            });
        }

        let covered = with_place_mut(self.doc, &path, &mut |place| match place {
            PlaceMut::Sheet(sheet) => sheet
                .merges
                .iter()
                .find(|range| covers(range, &reference) && range.start != reference)
                .map(|range| range.a1()),
            _ => None,
        })
        .flatten();
        if let Some(merge) = covered {
            return Err(Refusal::CoveredCell {
                cell: cell.to_string(),
                merge,
            });
        }

        let fallback = match self.format {
            Some(Format::Ods) => FormulaDialect::OpenFormula,
            _ => FormulaDialect::Ooxml,
        };
        let applied = with_place_mut(self.doc, &path, &mut |place| match place {
            PlaceMut::Sheet(sheet) => {
                write_cell(sheet, reference, value, fallback);
                true
            }
            _ => false,
        })
        .unwrap_or(false);
        if !applied {
            return Err(Refusal::WrongKind {
                op: "set_cell",
                target: canonical,
                got: describe_path(self.doc, &parsed),
                takes: "a sheet",
                because: String::new(),
            });
        }
        // §6.7's recalculation question, settled in 17-E and no longer a
        // warning. Writing a formula still clears the cached value — a new
        // formula over an old number displays a number that is simply wrong —
        // but both writers now *ask* for the recalculation rather than leaving
        // the caller to:
        //
        // * **xlsx**: `xl/workbook.xml` gains `<calcPr fullCalcOnLoad="1"/>`
        //   whenever a formula cell has no cached value (`xlsx/write.rs`).
        // * **ods**: no flag exists and none is needed. ODF stores the cached
        //   result in `office:value`, so a formula cell without one has nothing
        //   to display and the application computes it; measured against
        //   LibreOffice Calc, which returns 42 for an `of:=[.A1]*[.A2]` cell
        //   written with no `office:value` at all.
        //
        // A warning here would therefore be a false warning (§0.5), which is as
        // forbidden as a silent degradation.
        self.note(&canonical);
        Ok(())
    }

    // ---------------------------------------------------------------- 6.8

    fn set_notes(
        &mut self,
        op: &Operation,
        target: &str,
        content: Option<&str>,
    ) -> Result<(), Refusal> {
        let class = DocClass::of(self.doc);
        if class != DocClass::Presentation {
            return Err(Refusal::NotApplicableToClass {
                op: "set_notes",
                class,
                applies_to: "presentations",
            });
        }
        let (parsed, path) = self.locate(op, target)?;
        let canonical = parsed.canonical();
        {
            let (place, _) = resolve_path(self.doc, &parsed).map_err(target_refusal)?;
            match place {
                Place::Slide(_) => {}
                other => {
                    return Err(Refusal::WrongKind {
                        op: "set_notes",
                        target: canonical,
                        got: other.describe().to_string(),
                        takes: "a slide",
                        because: String::new(),
                    })
                }
            }
        }
        let blocks = match content.map(str::trim) {
            None | Some("") => None,
            Some(_) => {
                let content = content.unwrap_or_default();
                let fragment = fragment::parse_for(
                    SeqKind::Blocks,
                    content,
                    self.base.as_deref(),
                    self.store,
                    &mut self.report,
                )
                .map_err(|e| fragment_refusal(e, &canonical, SeqKind::Blocks))?;
                match fragment {
                    Fragment::Blocks(blocks) => Some(blocks),
                    other => {
                        return Err(Refusal::ContentKindMismatch {
                            target: canonical,
                            expected: "blocks".into(),
                            got: other.kind().to_string(),
                        })
                    }
                }
            }
        };
        with_place_mut(self.doc, &path, &mut |place| {
            if let PlaceMut::Slide(slide) = place {
                slide.notes = blocks.clone();
            }
        });
        self.note(&canonical);
        Ok(())
    }

    // ---------------------------------------------------------------- 6.9

    fn replace_image(
        &mut self,
        op: &Operation,
        target: &str,
        path_arg: Option<&str>,
        content_base64: Option<&str>,
        filename: Option<&str>,
        alt: Option<&str>,
    ) -> Result<(), Refusal> {
        let (parsed, path) = self.locate(op, target)?;
        let canonical = parsed.canonical();
        {
            let (place, _) = resolve_path(self.doc, &parsed).map_err(target_refusal)?;
            let image = image_of(&place).ok_or_else(|| Refusal::WrongKind {
                op: "replace_image",
                target: canonical.clone(),
                got: place.describe().to_string(),
                takes: "an image, or a shape that is a picture",
                because: String::new(),
            })?;
            if image.external_src.is_some() {
                return Err(Refusal::ExternalImage {
                    target: canonical.clone(),
                });
            }
        }

        let bytes = self.image_bytes(path_arg, content_base64, filename)?;
        let asset = self.store.put(&bytes).map_err(|e| Refusal::Unaddressable {
            reason: format!("the replacement picture could not be stored: {e}"),
        })?;

        let stretched = with_place_mut(self.doc, &path, &mut |place| {
            let image = match place {
                PlaceMut::Image(image) => image,
                PlaceMut::Block(Block::Image(image)) => image,
                PlaceMut::Shape(shape) => match &mut shape.kind {
                    ShapeKind::Picture(image) => image,
                    _ => return None,
                },
                _ => return None,
            };
            image.asset = asset.clone();
            if let Some(alt) = alt {
                image.alt = alt.to_string();
            }
            Some(image.geometry.display_size)
        })
        .flatten();

        if let Some(drawn) = stretched {
            self.warn_if_stretched(&canonical, &bytes, drawn);
        }
        self.note(&canonical);
        Ok(())
    }

    fn image_bytes(
        &self,
        path_arg: Option<&str>,
        content_base64: Option<&str>,
        filename: Option<&str>,
    ) -> Result<Vec<u8>, Refusal> {
        match (path_arg, content_base64) {
            (Some(path), None) => {
                let path = Path::new(path);
                let resolved = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    match &self.base {
                        Some(base) => base.join(path),
                        None => path.to_path_buf(),
                    }
                };
                std::fs::read(&resolved).map_err(|e| Refusal::Unaddressable {
                    reason: format!("{}: {e}", resolved.display()),
                })
            }
            (None, Some(encoded)) => {
                let _ = filename;
                use base64::Engine;
                base64::engine::general_purpose::STANDARD
                    .decode(encoded.trim())
                    .map_err(|e| Refusal::Unaddressable {
                        reason: format!("content_base64 is not base64: {e}"),
                    })
            }
            _ => Err(Refusal::MissingField {
                op: "replace_image",
                field: "exactly one of `path` or `content_base64`",
            }),
        }
    }

    fn warn_if_stretched(&mut self, target: &str, bytes: &[u8], drawn: docsai_model::units::Size) {
        let Some((w, h)) = image_pixels(bytes) else {
            return;
        };
        let drawn_w = drawn.width.emu() as f64;
        let drawn_h = drawn.height.emu() as f64;
        if w == 0 || h == 0 || drawn_w <= 0.0 || drawn_h <= 0.0 {
            return;
        }
        let picture = w as f64 / h as f64;
        let box_ratio = drawn_w / drawn_h;
        // §6.9: the 1 % floor keeps the warning off rounding.
        if (picture - box_ratio).abs() / box_ratio > 0.01 {
            self.report.warn(Warning::ImageGeometryDegraded {
                what: format!("`{target}`"),
                why: format!(
                    "the new picture is {picture:.3}:1 and is drawn at {box_ratio:.3}:1, so it \
                     will be visibly stretched; the geometry a person chose was kept"
                ),
            });
        }
    }

    /// Assigns ids to whatever an operation just created and records them.
    fn register_created(&mut self, before: &BTreeSet<NodeId>) {
        docsai_model::addressing::assign_ids(self.doc, docsai_model::addressing::IdPolicy::Assign);
        for (id, _) in ids_in_document_order(self.doc) {
            if !before.contains(&id) && !self.created.contains(&id) {
                self.created.push(id);
            }
        }
    }
}

// ------------------------------------------------------------------ helpers

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum After {
    Yes,
    No,
}

const PROPERTY_KEYS: &[&str] = &[
    "bold",
    "italic",
    "strike",
    "small_caps",
    "caps",
    "underline",
    "color",
    "highlight",
    "size",
    "font",
    "align",
    "style",
];

const PROPERTY_LIST: &str = "bold, italic, strike, small_caps, caps, underline, color, highlight, \
                             size, font, align, style";

fn target_refusal(error: TargetError) -> Refusal {
    match error {
        TargetError::Malformed { target, reason } => Refusal::MalformedTarget {
            target,
            reason: reason.to_string(),
        },
        TargetError::UnknownId { ref target, .. }
        | TargetError::IndexOutOfRange { ref target, .. }
        | TargetError::NotIndexable { ref target, .. } => Refusal::UnknownTarget {
            target: target.clone(),
            reason: error.to_string(),
        },
        TargetError::AmbiguousId { .. } | TargetError::ReservedId { .. } => {
            Refusal::DocumentDefect {
                reason: error.to_string(),
            }
        }
    }
}

fn precondition_refusal(error: PreconditionError) -> Refusal {
    match error {
        PreconditionError::Target(e) => target_refusal(e),
        PreconditionError::StaleEtag {
            target,
            expected,
            actual,
        } => Refusal::StaleEtag {
            target,
            expected: expected.0,
            actual: actual.0,
        },
        PreconditionError::NoEtag { target, kind } => Refusal::WrongKind {
            op: "if_match",
            target,
            got: kind.to_string(),
            takes: "a node with an etag",
            because: ": a precondition that is silently not checked is worse than none".into(),
        },
    }
}

fn fragment_refusal(error: FragmentError, target: &str, into: SeqKind) -> Refusal {
    match error {
        FragmentError::Parse(e) => Refusal::ContentKindMismatch {
            target: target.to_string(),
            expected: into.to_string(),
            got: format!("content that does not parse: {e}"),
        },
        FragmentError::Mismatch { got } => Refusal::ContentKindMismatch {
            target: target.to_string(),
            expected: into.to_string(),
            got,
        },
        FragmentError::NoFragmentKind { reason } => Refusal::ContentKindMismatch {
            target: target.to_string(),
            expected: into.to_string(),
            got: reason.to_string(),
        },
    }
}

fn describe_path(doc: &Document, target: &Target) -> String {
    match resolve_path(doc, target) {
        Ok((place, _)) => place.describe().to_string(),
        Err(_) => "nothing".to_string(),
    }
}

/// §6.1 and §6.2 share their kinds row, and both name the two refusals a caller
/// will meet on purpose: a chart, and a table or a row.
fn text_kind_check(op: &'static str, target: &str, place: &Place<'_>) -> Result<(), Refusal> {
    let ok = match place {
        Place::Block(Block::Paragraph(_) | Block::Heading(_)) => true,
        Place::Shape(shape) => matches!(
            shape.kind,
            ShapeKind::Placeholder(_) | ShapeKind::TextBox { .. }
        ),
        _ => false,
    };
    if ok {
        return Ok(());
    }
    let because = match place {
        Place::Shape(shape) if matches!(shape.kind, ShapeKind::Chart(_)) => {
            ": a preserved chart's shape is not editable — its numbers are edited by rewriting its \
             DocMark table (§9)"
                .to_string()
        }
        Place::Comment(_) => {
            ": a comment records what a named person said, and nothing in version 1 edits one \
             (§6.10)"
                .to_string()
        }
        Place::Block(Block::Table(_)) | Place::Row(_) | Place::Shape(_) => {
            ": the paragraph inside the cell is the target, reached by path".to_string()
        }
        _ => String::new(),
    };
    Err(Refusal::WrongKind {
        op,
        target: target.to_string(),
        got: place.describe().to_string(),
        takes: "a paragraph, a heading, or a shape that is a placeholder or a text box",
        because,
    })
}

fn table_cell_hint(place: &Place<'_>) -> String {
    match place {
        Place::Cell(_) | Place::Row(_) | Place::Block(Block::Table(_)) => {
            ": a table cell is a sequence of blocks, not a value — address the paragraph inside it \
             with `replace_text` (§6.7)"
                .to_string()
        }
        _ => String::new(),
    }
}

fn image_of<'a>(place: &Place<'a>) -> Option<&'a ImageRef> {
    match place {
        Place::Image(image) => Some(image),
        Place::Block(Block::Image(image)) => Some(image),
        Place::Shape(shape) => match &shape.kind {
            ShapeKind::Picture(image) => Some(image),
            _ => None,
        },
        _ => None,
    }
}

/// §6.1's replacement, and the number of distinct run styles it collapsed.
fn set_paragraph_text(paragraph: &mut Paragraph, text: &str) -> usize {
    let mut runs = Vec::new();
    runs_of(&paragraph.content, &RunProps::default(), &mut runs);
    let mut distinct: Vec<RunProps> = Vec::new();
    for run in &runs {
        if !distinct.contains(run) {
            distinct.push(run.clone());
        }
    }
    let first = runs.first().cloned().unwrap_or_default();
    paragraph.content = if first.is_empty() {
        vec![Inline::Text(text.to_string())]
    } else {
        vec![Inline::Styled {
            content: vec![Inline::Text(text.to_string())],
            props: first,
        }]
    };
    distinct.len()
}

fn set_body_text(body: &mut Vec<Block>, text: &str) -> usize {
    let mut runs = Vec::new();
    for block in body.iter() {
        match block {
            Block::Paragraph(p) => runs_of(&p.content, &RunProps::default(), &mut runs),
            Block::Heading(h) => runs_of(&h.paragraph.content, &RunProps::default(), &mut runs),
            _ => {}
        }
    }
    let mut distinct: Vec<RunProps> = Vec::new();
    for run in &runs {
        if !distinct.contains(run) {
            distinct.push(run.clone());
        }
    }
    let format = body
        .iter()
        .find_map(|b| match b {
            Block::Paragraph(p) => Some(p.format.clone()),
            Block::Heading(h) => Some(h.paragraph.format.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let first = runs.first().cloned().unwrap_or_default();
    let content = if first.is_empty() {
        vec![Inline::Text(text.to_string())]
    } else {
        vec![Inline::Styled {
            content: vec![Inline::Text(text.to_string())],
            props: first,
        }]
    };
    let mut paragraph = Paragraph::new(content);
    paragraph.format = format;
    *body = vec![Block::Paragraph(paragraph)];
    distinct.len()
}

fn runs_of(inlines: &[Inline], inherited: &RunProps, out: &mut Vec<RunProps>) {
    for inline in inlines {
        match inline {
            Inline::Text(t) if !t.is_empty() => out.push(inherited.clone()),
            Inline::Styled { content, props } | Inline::Link { content, props, .. } => {
                let effective = if props.is_empty() {
                    inherited.clone()
                } else {
                    props.clone()
                };
                runs_of(content, &effective, out);
            }
            _ => {}
        }
    }
}

/// §6.2's property set, parsed once and applied to as many paragraphs as the
/// target holds.
struct PropPlan {
    font: Vec<(&'static str, Option<Scalar>)>,
    align: Option<Option<Align>>,
    style: Option<Option<StyleId>>,
}

impl PropPlan {
    fn parse(
        props: &std::collections::BTreeMap<String, Option<Scalar>>,
        catalog: &docsai_model::style::StyleCatalog,
    ) -> Result<PropPlan, Refusal> {
        let mut plan = PropPlan {
            font: Vec::new(),
            align: None,
            style: None,
        };
        for (key, value) in props {
            match key.as_str() {
                "align" => {
                    plan.align = Some(match value {
                        None => None,
                        Some(Scalar::Text(t)) => Some(match t.as_str() {
                            "left" => Align::Left,
                            "center" => Align::Center,
                            "right" => Align::Right,
                            "justify" => Align::Justify,
                            _ => {
                                return Err(Refusal::UnknownField {
                                    op: "set_props",
                                    field: format!("align: {t}"),
                                    known: "left, center, right, justify",
                                })
                            }
                        }),
                        Some(other) => {
                            return Err(Refusal::UnknownField {
                                op: "set_props",
                                field: format!("align: {other}"),
                                known: "left, center, right, justify",
                            })
                        }
                    });
                }
                "style" => {
                    plan.style = Some(match value {
                        None => None,
                        Some(Scalar::Text(t)) => {
                            let id = StyleId::new(t.clone());
                            if !catalog.styles.contains_key(&id) {
                                return Err(Refusal::UnknownStyle { style: t.clone() });
                            }
                            Some(id)
                        }
                        Some(other) => {
                            return Err(Refusal::UnknownStyle {
                                style: other.to_string(),
                            })
                        }
                    });
                }
                other => {
                    let known = PROPERTY_KEYS.iter().find(|k| **k == other).ok_or_else(|| {
                        Refusal::UnknownField {
                            op: "set_props",
                            field: other.to_string(),
                            known: PROPERTY_LIST,
                        }
                    })?;
                    validate_font_value(known, value.as_ref())?;
                    plan.font.push((known, value.clone()));
                }
            }
        }
        Ok(plan)
    }

    fn apply_paragraph(&self, paragraph: &mut Paragraph) {
        if let Some(align) = &self.align {
            paragraph.format.direct.align = *align;
        }
        if let Some(style) = &self.style {
            paragraph.format.style = style.clone();
        }
        for (key, value) in &self.font {
            set_font(&mut paragraph.format.run_direct, key, value.as_ref());
            // §6.2: the property also reaches every run of the paragraph, so
            // the paragraph reads uniformly. A caller that says «bold this
            // paragraph» and gets one stubbornly non-bold word has been failed.
            apply_font(&mut paragraph.content, key, value.as_ref());
        }
    }
}

fn validate_font_value(key: &str, value: Option<&Scalar>) -> Result<(), Refusal> {
    let bad = |known: &'static str| Refusal::UnknownField {
        op: "set_props",
        field: format!(
            "{key}: {}",
            value.map(|v| v.to_string()).unwrap_or_default()
        ),
        known,
    };
    match (key, value) {
        (_, None) => Ok(()),
        ("bold" | "italic" | "strike" | "small_caps" | "caps", Some(Scalar::Bool(_))) => Ok(()),
        ("bold" | "italic" | "strike" | "small_caps" | "caps", _) => Err(bad("true or false")),
        ("underline", Some(Scalar::Text(t))) => match underline(t) {
            Some(_) => Ok(()),
            None => Err(bad("none, single, double, thick, dotted, dashed, wave")),
        },
        ("underline", _) => Err(bad("none, single, double, thick, dotted, dashed, wave")),
        ("color", Some(Scalar::Text(t))) if is_rgb(t) => Ok(()),
        ("color", _) => Err(bad("RRGGBB")),
        ("highlight", Some(Scalar::Text(t))) if HIGHLIGHTS.contains(&t.as_str()) => Ok(()),
        ("highlight", _) => Err(bad(HIGHLIGHT_LIST)),
        ("size", Some(Scalar::Text(t))) if Length::parse(t).is_some() => Ok(()),
        ("size", _) => Err(bad("a DocMark length such as `11pt`")),
        ("font", Some(Scalar::Text(_))) => Ok(()),
        ("font", _) => Err(bad("a font name")),
        _ => Ok(()),
    }
}

/// `w:highlight/@val` is an **enumeration**, not a colour: `ST_HighlightColor`
/// names sixteen highlights and `none`. 17-A's §6.2 typed the key `RRGGBB` by
/// analogy with `color`, and the validator took any string at all, so
/// `highlight: "FFFF00"` was written verbatim into the package and produced a
/// `w:highlight` Word does not accept — a silent no-op reported as a change,
/// which is the exact failure §6.2's own paragraph on `bould` refuses. 17-H
/// measured it and this list is the fix; §6.2 was corrected to match.
const HIGHLIGHTS: &[&str] = &[
    "none",
    "black",
    "blue",
    "cyan",
    "darkBlue",
    "darkCyan",
    "darkGray",
    "darkGreen",
    "darkMagenta",
    "darkRed",
    "darkYellow",
    "green",
    "lightGray",
    "magenta",
    "red",
    "white",
    "yellow",
];

const HIGHLIGHT_LIST: &str = "none, black, blue, cyan, darkBlue, darkCyan, darkGray, darkGreen, \
     darkMagenta, darkRed, darkYellow, green, lightGray, magenta, red, white, yellow";

fn set_font(font: &mut FontProps, key: &str, value: Option<&Scalar>) {
    let flag = |value: Option<&Scalar>| match value {
        Some(Scalar::Bool(b)) => Some(*b),
        _ => None,
    };
    match key {
        "bold" => font.bold = flag(value),
        "italic" => font.italic = flag(value),
        "strike" => font.strike = flag(value),
        "small_caps" => font.small_caps = flag(value),
        "caps" => font.caps = flag(value),
        "underline" => {
            font.underline = match value {
                Some(Scalar::Text(t)) => underline(t),
                _ => None,
            }
        }
        "color" => {
            font.color = match value {
                Some(Scalar::Text(t)) => Some(normalise_rgb(t)),
                _ => None,
            }
        }
        "highlight" => {
            font.highlight = match value {
                Some(Scalar::Text(t)) => Some(t.clone()),
                _ => None,
            }
        }
        "size" => {
            font.size = match value {
                Some(Scalar::Text(t)) => Length::parse(t),
                _ => None,
            }
        }
        "font" => {
            font.name = match value {
                Some(Scalar::Text(t)) => Some(t.clone()),
                _ => None,
            }
        }
        _ => {}
    }
}

/// §6.2's «applies to the paragraph's whole content», on the runs themselves.
///
/// `ParaFormat::run_direct` alone does not deliver it, and that is measured
/// rather than assumed: ODF writes it into the paragraph's own text
/// properties, where it does reach the text, but docx writes it to
/// `w:pPr/w:rPr` and pptx to `a:endParaRPr` — the properties of the paragraph
/// *mark*, which format nothing a reader sees. `run_direct` is still set,
/// because it is what text typed at the end of the paragraph inherits; the
/// runs are what make the paragraph read the way the caller asked.
///
/// Clearing (`null`) only clears: there is nothing to wrap, because a bare
/// `Inline::Text` overrides nothing.
fn apply_font(inlines: &mut [Inline], key: &str, value: Option<&Scalar>) {
    for inline in inlines.iter_mut() {
        match inline {
            Inline::Styled { content, props } | Inline::Link { content, props, .. } => {
                set_font(&mut props.direct, key, value);
                // A descendant that overrode the property would still win.
                clear_font(content, key);
            }
            Inline::Text(text) if value.is_some() && !text.is_empty() => {
                let mut props = RunProps::default();
                set_font(&mut props.direct, key, value);
                let taken = std::mem::replace(inline, Inline::Text(String::new()));
                *inline = Inline::Styled {
                    content: vec![taken],
                    props,
                };
            }
            _ => {}
        }
    }
}

fn clear_font(inlines: &mut [Inline], key: &str) {
    for inline in inlines.iter_mut() {
        match inline {
            Inline::Styled { content, props } | Inline::Link { content, props, .. } => {
                set_font(&mut props.direct, key, None);
                clear_font(content, key);
            }
            Inline::Footnote(_) | Inline::Comment(_) => {}
            _ => {}
        }
    }
}

fn underline(name: &str) -> Option<Underline> {
    Some(match name {
        "none" => Underline::None,
        "single" => Underline::Single,
        "double" => Underline::Double,
        "thick" => Underline::Thick,
        "dotted" => Underline::Dotted,
        "dashed" => Underline::Dashed,
        "wave" => Underline::Wave,
        _ => return None,
    })
}

fn is_rgb(text: &str) -> bool {
    let bare = text.strip_prefix('#').unwrap_or(text);
    bare.len() == 6 && bare.bytes().all(|b| b.is_ascii_hexdigit())
}

fn normalise_rgb(text: &str) -> String {
    let bare = text.strip_prefix('#').unwrap_or(text);
    format!("#{bare}")
}

fn covers(range: &docsai_model::sheet::CellRange, cell: &CellRef) -> bool {
    cell.col >= range.start.col
        && cell.col <= range.end.col
        && cell.row >= range.start.row
        && cell.row <= range.end.row
}

fn write_cell(
    sheet: &mut Sheet,
    reference: CellRef,
    value: Option<&Scalar>,
    fallback: FormulaDialect,
) {
    // A formula already in the sheet is the most reliable statement of the
    // dialect the sheet speaks; the package's own format is the fallback.
    let dialect = sheet
        .cells
        .values()
        .find_map(|cell| cell.formula.as_ref().map(|f| f.dialect))
        .unwrap_or(fallback);
    let Some(value) = value else {
        sheet.cells.remove(&reference);
        return;
    };
    let entry = sheet.cells.entry(reference).or_default();
    match value {
        Scalar::Text(text) if text.starts_with('=') => {
            entry.formula = Some(Formula {
                text: text.trim_start_matches('=').to_string(),
                // The sheet's own dialect where it has one: §6.7 stores a
                // formula verbatim and does not translate between dialects.
                dialect: entry.formula.as_ref().map(|f| f.dialect).unwrap_or(dialect),
                shared_over: None,
                array_over: None,
            });
            // §6.7: a new formula over an old cached value would display a
            // number that is simply wrong until something recalculates.
            entry.value = CellValue::Empty;
        }
        Scalar::Text(text) => {
            entry.formula = None;
            entry.value = CellValue::Text(text.clone());
        }
        Scalar::Bool(b) => {
            entry.formula = None;
            entry.value = CellValue::Bool(*b);
        }
        Scalar::Int(i) => {
            entry.formula = None;
            entry.value = CellValue::Number(*i as f64);
        }
        Scalar::Float(f) => {
            entry.formula = None;
            entry.value = CellValue::Number(*f);
        }
    }
}

fn root_seq_kind(doc: &Document) -> SeqKind {
    match doc {
        Document::Text(text) if text.sections.len() == 1 => SeqKind::Blocks,
        Document::Text(_) => SeqKind::Sections,
        Document::Workbook(_) => SeqKind::Sheets,
        Document::Presentation(_) => SeqKind::Slides,
    }
}

fn root_seq_len(doc: &Document) -> usize {
    match doc {
        Document::Text(text) => match text.sections.as_slice() {
            [only] => only.blocks.len(),
            sections => sections.len(),
        },
        Document::Workbook(book) => book.sheets.len(),
        Document::Presentation(deck) => deck.slides.len(),
    }
}

fn root_splice(doc: &mut Document, at: usize, fragment: &Fragment) -> bool {
    match (doc, fragment) {
        (Document::Text(text), Fragment::Blocks(blocks)) if text.sections.len() == 1 => {
            let target = &mut text.sections[0].blocks;
            let at = at.min(target.len());
            splice_vec(target, at, blocks.clone());
            true
        }
        (Document::Text(text), Fragment::Sections(sections)) if text.sections.len() != 1 => {
            let at = at.min(text.sections.len());
            splice_vec(&mut text.sections, at, sections.clone());
            true
        }
        (Document::Workbook(book), Fragment::Sheets(sheets)) => {
            let at = at.min(book.sheets.len());
            splice_vec(&mut book.sheets, at, sheets.clone());
            true
        }
        (Document::Presentation(deck), Fragment::Slides(slides)) => {
            let at = at.min(deck.slides.len());
            splice_vec(&mut deck.slides, at, slides.clone());
            true
        }
        _ => false,
    }
}

fn splice(seq: SeqMut<'_>, at: usize, fragment: &Fragment) -> bool {
    match (seq, fragment) {
        (SeqMut::Sections(v), Fragment::Sections(f)) => splice_vec(v, at, f.clone()),
        (SeqMut::Blocks(v), Fragment::Blocks(f)) => splice_vec(v, at, f.clone()),
        (SeqMut::Rows(v), Fragment::Rows(f)) => splice_vec(v, at, f.clone()),
        (SeqMut::ListItems(v), Fragment::ListItems(f)) => splice_vec(v, at, f.clone()),
        (SeqMut::Sheets(v), Fragment::Sheets(f)) => splice_vec(v, at, f.clone()),
        (SeqMut::Slides(v), Fragment::Slides(f)) => splice_vec(v, at, f.clone()),
        (SeqMut::Shapes(v), Fragment::Shapes(f)) => splice_vec(v, at, f.clone()),
        (SeqMut::Images(v), Fragment::Images(f)) => splice_vec(v, at, f.clone()),
        _ => return false,
    }
    true
}

/// The number of cells in the row a path names, for §6.5's `GridMismatch`.
fn row_width(doc: &mut Document, path: &[Step]) -> Option<usize> {
    with_seq_mut(doc, path, &mut |seq, index| match seq {
        SeqMut::Rows(rows) => rows.get(index).map(|r| r.cells.len()),
        _ => None,
    })
    .flatten()
}

fn splice_vec<T>(target: &mut Vec<T>, at: usize, items: Vec<T>) {
    let at = at.min(target.len());
    let tail = target.split_off(at);
    target.extend(items);
    target.extend(tail);
}

/// One node, detached from its sequence.
enum Taken {
    Section(Box<Section>),
    Block(Box<Block>),
    Inline(Box<Inline>),
    ListItem(Box<ListItem>),
    Row(Box<TableRow>),
    Sheet(Box<Sheet>),
    Slide(Box<Slide>),
    Shape(Box<Shape>),
    Comment(Box<Comment>),
    Image(Box<ImageRef>),
}

fn take_at(seq: SeqMut<'_>, index: usize) -> Option<Taken> {
    macro_rules! take {
        ($v:expr, $variant:ident) => {{
            if index >= $v.len() {
                return None;
            }
            Some(Taken::$variant(Box::new($v.remove(index))))
        }};
    }
    match seq {
        SeqMut::Sections(v) => take!(v, Section),
        SeqMut::Blocks(v) => take!(v, Block),
        SeqMut::Inlines(v) => take!(v, Inline),
        SeqMut::ListItems(v) => take!(v, ListItem),
        SeqMut::Rows(v) => take!(v, Row),
        SeqMut::Sheets(v) => take!(v, Sheet),
        SeqMut::Slides(v) => take!(v, Slide),
        SeqMut::Shapes(v) => take!(v, Shape),
        SeqMut::Comments(v) => take!(v, Comment),
        SeqMut::Images(v) => take!(v, Image),
        SeqMut::Cells(_) => None,
    }
}

fn put_at(seq: SeqMut<'_>, at: usize, taken: &Taken) -> bool {
    macro_rules! put {
        ($v:expr, $value:expr) => {{
            let at = at.min($v.len());
            $v.insert(at, $value);
            true
        }};
    }
    match (seq, taken) {
        (SeqMut::Sections(v), Taken::Section(x)) => put!(v, (**x).clone()),
        (SeqMut::Blocks(v), Taken::Block(x)) => put!(v, (**x).clone()),
        (SeqMut::Inlines(v), Taken::Inline(x)) => put!(v, (**x).clone()),
        (SeqMut::ListItems(v), Taken::ListItem(x)) => put!(v, (**x).clone()),
        (SeqMut::Rows(v), Taken::Row(x)) => put!(v, (**x).clone()),
        (SeqMut::Sheets(v), Taken::Sheet(x)) => put!(v, (**x).clone()),
        (SeqMut::Slides(v), Taken::Slide(x)) => put!(v, (**x).clone()),
        (SeqMut::Shapes(v), Taken::Shape(x)) => put!(v, (**x).clone()),
        (SeqMut::Comments(v), Taken::Comment(x)) => put!(v, (**x).clone()),
        (SeqMut::Images(v), Taken::Image(x)) => put!(v, (**x).clone()),
        _ => false,
    }
}

/// §6.4's removal. `unwrap_comment` is the one case where removing an element
/// would remove *document content* with it: an `Inline::Comment` carries the
/// text it is anchored over, and deleting the annotation must leave that text
/// where it was.
fn remove_at(seq: SeqMut<'_>, index: usize, unwrap_comment: bool) -> bool {
    match seq {
        SeqMut::Inlines(v) => {
            if index >= v.len() {
                return false;
            }
            let removed = v.remove(index);
            if unwrap_comment {
                if let Inline::Comment(comment) = &removed {
                    let covered = comment.covered().to_vec();
                    splice_vec(v, index, covered);
                }
            }
            true
        }
        SeqMut::Sections(v) => remove_from(v, index),
        SeqMut::Blocks(v) => remove_from(v, index),
        SeqMut::ListItems(v) => remove_from(v, index),
        SeqMut::Rows(v) => remove_from(v, index),
        SeqMut::Sheets(v) => remove_from(v, index),
        SeqMut::Slides(v) => remove_from(v, index),
        SeqMut::Shapes(v) => remove_from(v, index),
        SeqMut::Comments(v) => remove_from(v, index),
        SeqMut::Images(v) => remove_from(v, index),
        SeqMut::Cells(v) => remove_from(v, index),
    }
}

fn remove_from<T>(v: &mut Vec<T>, index: usize) -> bool {
    if index >= v.len() {
        return false;
    }
    v.remove(index);
    true
}

/// Adjusts a path for the removal of the element `removed` names.
fn shift_after_removal(removed: &[Step], mut path: Vec<Step>) -> Vec<Step> {
    let depth = removed.len();
    if path.len() < depth || path[..depth - 1] != removed[..depth - 1] {
        return path;
    }
    let (Some(gone), Some(here)) = (
        step_index(&removed[depth - 1]),
        step_index(&path[depth - 1]),
    ) else {
        return path;
    };
    if same_sequence(&removed[depth - 1], &path[depth - 1]) && here > gone {
        path[depth - 1] = with_index(&path[depth - 1], here - 1);
    }
    path
}

fn step_index(step: &Step) -> Option<usize> {
    Some(match step {
        Step::Section(i)
        | Step::Block(i)
        | Step::ListItem(i)
        | Step::Row(i)
        | Step::Cell(i)
        | Step::Sheet(i)
        | Step::SheetImage(i)
        | Step::Slide(i)
        | Step::NotesBlock(i)
        | Step::Shape(i)
        | Step::Comment(i)
        | Step::CommentCovered(i)
        | Step::Inline(i) => *i,
        Step::HeaderBlock(_, i) | Step::FooterBlock(_, i) => *i,
        Step::PictureImage | Step::IntoFootnote | Step::IntoComment | Step::IntoImage => {
            return None
        }
    })
}

fn same_sequence(a: &Step, b: &Step) -> bool {
    match (a, b) {
        (Step::HeaderBlock(j, _), Step::HeaderBlock(k, _)) => j == k,
        (Step::FooterBlock(j, _), Step::FooterBlock(k, _)) => j == k,
        _ => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}

fn with_index(step: &Step, index: usize) -> Step {
    match step {
        Step::Section(_) => Step::Section(index),
        Step::Block(_) => Step::Block(index),
        Step::ListItem(_) => Step::ListItem(index),
        Step::Row(_) => Step::Row(index),
        Step::Cell(_) => Step::Cell(index),
        Step::Sheet(_) => Step::Sheet(index),
        Step::SheetImage(_) => Step::SheetImage(index),
        Step::Slide(_) => Step::Slide(index),
        Step::NotesBlock(_) => Step::NotesBlock(index),
        Step::Shape(_) => Step::Shape(index),
        Step::Comment(_) => Step::Comment(index),
        Step::CommentCovered(_) => Step::CommentCovered(index),
        Step::Inline(_) => Step::Inline(index),
        Step::HeaderBlock(j, _) => Step::HeaderBlock(*j, index),
        Step::FooterBlock(j, _) => Step::FooterBlock(*j, index),
        other => *other,
    }
}

fn ids_of_fragment(fragment: &Fragment) -> Vec<NodeId> {
    // The fragment is not yet in a document, so the cheapest total answer is to
    // wrap it in one and ask the same walker the document is asked.
    let mut ids = Vec::new();
    macro_rules! push {
        ($id:expr) => {
            push_id($id, &mut ids)
        };
    }
    match fragment {
        Fragment::Sections(v) => {
            for section in v {
                push!(&section.id);
                block_ids(&section.blocks, &mut ids);
            }
        }
        Fragment::Blocks(v) => block_ids(v, &mut ids),
        Fragment::Rows(v) => {
            for row in v {
                push!(&row.id);
                for cell in &row.cells {
                    block_ids(&cell.blocks, &mut ids);
                }
            }
        }
        Fragment::ListItems(v) => {
            for item in v {
                block_ids(&item.blocks, &mut ids);
            }
        }
        Fragment::Sheets(v) => {
            for sheet in v {
                push!(&sheet.id);
                for image in &sheet.images {
                    push!(&image.id);
                }
            }
        }
        Fragment::Slides(v) => {
            for slide in v {
                push!(&slide.id);
                for shape in &slide.shapes {
                    shape_ids(shape, &mut ids);
                }
                if let Some(notes) = &slide.notes {
                    block_ids(notes, &mut ids);
                }
            }
        }
        Fragment::Shapes(v) => {
            for shape in v {
                shape_ids(shape, &mut ids);
            }
        }
        Fragment::Images(v) => {
            for image in v {
                push!(&image.id);
            }
        }
    }
    ids
}

fn block_ids(blocks: &[Block], ids: &mut Vec<NodeId>) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => push_id(&p.id, ids),
            Block::Heading(h) => push_id(&h.id, ids),
            Block::List(l) => {
                push_id(&l.id, ids);
                for item in &l.items {
                    block_ids(&item.blocks, ids);
                }
            }
            Block::Table(t) => {
                push_id(&t.id, ids);
                for row in &t.rows {
                    push_id(&row.id, ids);
                    for cell in &row.cells {
                        block_ids(&cell.blocks, ids);
                    }
                }
            }
            Block::Image(i) => push_id(&i.id, ids),
            Block::TextBox(b) => block_ids(&b.blocks, ids),
            Block::Raw(_) => {}
        }
    }
}

fn shape_ids(shape: &Shape, ids: &mut Vec<NodeId>) {
    push_id(&shape.id, ids);
    match &shape.kind {
        ShapeKind::Placeholder(ph) => block_ids(&ph.body, ids),
        ShapeKind::TextBox { body } => block_ids(body, ids),
        ShapeKind::Picture(image) => push_id(&image.id, ids),
        ShapeKind::Table(table) => {
            for row in &table.rows {
                push_id(&row.id, ids);
                for cell in &row.cells {
                    block_ids(&cell.blocks, ids);
                }
            }
        }
        ShapeKind::Group(children) => {
            for child in children {
                shape_ids(child, ids);
            }
        }
        ShapeKind::Chart(_) | ShapeKind::Raw(_) => {}
    }
}

fn push_id(id: &Option<NodeId>, ids: &mut Vec<NodeId>) {
    if let Some(id) = id {
        ids.push(id.clone());
    }
}

/// §5.4: «the copy and every addressable node inside it get fresh ids. Nothing
/// is shared with the source: two nodes with one id is the failure mode ids
/// exist to prevent.»
fn clear_slide_ids(slide: &mut Slide) {
    slide.id = None;
    for shape in &mut slide.shapes {
        clear_shape_ids(shape);
    }
    if let Some(notes) = &mut slide.notes {
        clear_block_ids(notes);
    }
}

fn clear_shape_ids(shape: &mut Shape) {
    shape.id = None;
    match &mut shape.kind {
        ShapeKind::Placeholder(ph) => clear_block_ids(&mut ph.body),
        ShapeKind::TextBox { body } => clear_block_ids(body),
        ShapeKind::Picture(image) => image.id = None,
        ShapeKind::Table(table) => {
            for row in &mut table.rows {
                row.id = None;
                for cell in &mut row.cells {
                    clear_block_ids(&mut cell.blocks);
                }
            }
        }
        ShapeKind::Group(children) => {
            for child in children {
                clear_shape_ids(child);
            }
        }
        ShapeKind::Chart(_) | ShapeKind::Raw(_) => {}
    }
}

fn clear_block_ids(blocks: &mut [Block]) {
    for block in blocks.iter_mut() {
        match block {
            Block::Paragraph(p) => p.id = None,
            Block::Heading(h) => {
                h.id = None;
                h.paragraph.id = None;
            }
            Block::List(l) => {
                l.id = None;
                for item in &mut l.items {
                    clear_block_ids(&mut item.blocks);
                }
            }
            Block::Table(t) => {
                t.id = None;
                for row in &mut t.rows {
                    row.id = None;
                    for cell in &mut row.cells {
                        clear_block_ids(&mut cell.blocks);
                    }
                }
            }
            Block::Image(i) => i.id = None,
            Block::TextBox(b) => clear_block_ids(&mut b.blocks),
            Block::Raw(_) => {}
        }
    }
}

/// §6.6: «the slide-level raw fragments (a transition, a timing tree), each
/// re-registered under a fresh `RawId` so the two slides do not share a sidecar
/// payload.»
fn refresh_raw_ids(deck: &mut docsai_model::presentation::Presentation, copy: &mut Slide) -> usize {
    use docsai_model::image::RawId;
    let mut taken: BTreeSet<String> = deck.raw.iter().map(|f| f.id.0.clone()).collect();
    let fresh = |taken: &mut BTreeSet<String>, from: &RawId| {
        for n in 1.. {
            let candidate = format!("{}-copy{n}", from.0);
            if taken.insert(candidate.clone()) {
                return RawId::new(candidate);
            }
        }
        unreachable!("the loop returns")
    };
    let mut added = Vec::new();
    let mut count = 0;
    for id in &mut copy.raw {
        let Some(source) = deck.raw.iter().find(|f| f.id == *id) else {
            continue;
        };
        let mut clone = source.clone();
        clone.id = fresh(&mut taken, id);
        *id = clone.id.clone();
        added.push(clone);
        count += 1;
    }
    deck.raw.extend(added);
    count
}

/// Pixel dimensions of a raster picture, for §6.9's aspect-ratio warning.
///
/// A header reader and deliberately not a decoder. The four formats an Office
/// package actually embeds are the four here; anything else — an SVG, an EMF, a
/// format this function does not know — answers `None`, which turns the warning
/// **off**. That is the right failure direction: §6.9's warning exists to tell a
/// caller its logo will look squashed, and a guess about a picture nobody could
/// measure would be a false warning, which §0.5 forbids as firmly as a missing
/// one. Pulling in an image decoder to widen the set would put a codec in the
/// path of every edit for a warning.
fn image_pixels(bytes: &[u8]) -> Option<(u32, u32)> {
    // PNG: signature, then IHDR's width and height as big-endian u32.
    if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        return Some((w, h));
    }
    // GIF: little-endian u16 pair right after the six-byte signature.
    if bytes.len() >= 10 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        let w = u16::from_le_bytes(bytes[6..8].try_into().ok()?) as u32;
        let h = u16::from_le_bytes(bytes[8..10].try_into().ok()?) as u32;
        return Some((w, h));
    }
    // BMP: the DIB header's signed 32-bit width and height; a negative height
    // is a top-down bitmap and its magnitude is the size.
    if bytes.len() >= 26 && bytes.starts_with(b"BM") {
        let w = i32::from_le_bytes(bytes[18..22].try_into().ok()?).unsigned_abs();
        let h = i32::from_le_bytes(bytes[22..26].try_into().ok()?).unsigned_abs();
        return Some((w, h));
    }
    // JPEG: walk the marker chain to the first frame header (SOF0..SOF15,
    // skipping the four that are not frames).
    if bytes.len() >= 4 && bytes.starts_with(b"\xff\xd8") {
        let mut at = 2usize;
        while at + 9 < bytes.len() {
            if bytes[at] != 0xff {
                at += 1;
                continue;
            }
            let marker = bytes[at + 1];
            if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
                let h = u16::from_be_bytes(bytes[at + 5..at + 7].try_into().ok()?) as u32;
                let w = u16::from_be_bytes(bytes[at + 7..at + 9].try_into().ok()?) as u32;
                return Some((w, h));
            }
            let length = u16::from_be_bytes(bytes[at + 2..at + 4].try_into().ok()?) as usize;
            at += 2 + length.max(2);
        }
    }
    None
}

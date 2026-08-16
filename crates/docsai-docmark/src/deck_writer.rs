//! Presentation → DocMark-P (spec §11.2).
//!
//! A slide is an `##` heading carrying `.slide`, and the heading *is* the title
//! placeholder; the layout's primary body placeholder is written as ordinary
//! blocks under it. Which shape is which is a catalogue lookup, and the
//! lookup is [`implicit_shapes`] — the same function the addressing walk asks,
//! so what is implicit here and what is addressable there cannot disagree.
//!
//! Everything a slide holds beyond those two shapes — the other placeholders,
//! free shapes, connectors, pictures, tables, notes — is written by the
//! increments after this one. Until then each of them is a warning, never a
//! silent omission (`AGENTS.md` §7 rule 3).

use docsai_model::addressing::{implicit_shapes, NodeKind};
use docsai_model::assets::AssetStore;
use docsai_model::presentation::{Presentation, Shape, ShapeKind, Slide};
use docsai_model::report::{ConversionReport, Warning};

use crate::attrs::Attrs;
use crate::dict::AttrDict;
use crate::ids::IdSource;
use crate::writer::Writer;
use crate::{Fidelity, Options};

/// Serialises a presentation body (front matter excluded).
pub fn write_presentation(
    deck: &Presentation,
    assets: &dyn AssetStore,
    options: &Options,
    ids: &mut IdSource,
    dict: &AttrDict,
) -> (String, ConversionReport) {
    let mut out = String::new();
    let mut writer = Writer::new(options, &deck.styles, assets, ids, dict);

    for (index, slide) in deck.slides.iter().enumerate() {
        if !out.is_empty() {
            out.push('\n');
        }
        write_slide(&mut out, deck, slide, index, &mut writer, options);
    }

    let mut report = writer.into_report();
    report.stats.slides = deck.slides.len() as u32;
    report.stats.styles = deck.styles.styles.len() as u32;

    while out.ends_with("\n\n") {
        out.pop();
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    (out, report)
}

fn write_slide(
    out: &mut String,
    deck: &Presentation,
    slide: &Slide,
    index: usize,
    writer: &mut Writer,
    options: &Options,
) {
    let plain = options.fidelity == Fidelity::Plain;
    let location = format!("slide {}", index + 1);

    // The title first and the body second, exactly as `implicit_shapes`
    // returns them; a slide may have either, both or neither.
    let implicit = implicit_shapes(slide, &deck.layouts);
    let title = implicit
        .iter()
        .copied()
        .find(|&i| is_title(&slide.shapes[i]));
    let body = implicit
        .iter()
        .copied()
        .find(|&i| !is_title(&slide.shapes[i]));

    let start = out.len();
    let mark = writer.ids().mark();
    let mut slide_id = None;
    let mut seed = Attrs::new();
    if !plain {
        // The slide's own id goes on the heading: it is the only line the
        // slide owns, which is why the two implicit shapes take none.
        slide_id = writer.ids().take(slide);
        if let Some(id) = slide_id.clone() {
            seed.id(id);
        }
        seed.class("slide");
        if let Some(layout) = &slide.layout {
            seed.set("layout", layout.as_str());
        }
        if let Some(section) = &slide.section {
            // `p14:sectionLst`, written on every slide of the section rather
            // than once at its start: a slide read on its own must still know
            // where it belongs (spec §11.1, partial reads).
            seed.set("section", section);
        }
        seed.set_flag("hidden", slide.hidden);
        if options.fidelity.addresses() {
            // `p:cSld@name` is what the writer puts back, not something a
            // reader needs; the levels that do not write back drop it.
            seed.set_opt("name", slide.name.as_deref());
        }
    }

    let title_blocks = title
        .map(|i| placeholder_body(&slide.shapes[i]))
        .unwrap_or_default();
    let heading = writer.render_slide_heading(title_blocks, seed, &location);
    push_block(out, &heading);

    if let Some(i) = body {
        let blocks = placeholder_body(&slide.shapes[i]);
        let rendered = writer.render_slide_blocks(blocks);
        push_block(out, &rendered);
    }

    let report = writer.report_mut();
    for (i, shape) in slide.shapes.iter().enumerate() {
        if implicit.contains(&i) {
            continue;
        }
        report.warn(Warning::UnsupportedElement {
            kind: shape_kind(shape),
            location: location.clone(),
            action: "skipped: shape containers are not written yet".into(),
        });
    }
    if slide.notes.as_ref().is_some_and(|notes| !notes.is_empty()) {
        report.warn(Warning::UnsupportedElement {
            kind: "notes".into(),
            location: location.clone(),
            action: "skipped: speaker notes are not written yet".into(),
        });
    }
    for raw in &slide.raw {
        report.warn(Warning::RawBlockDropped {
            id: raw.as_str().to_string(),
            format: "pml".into(),
        });
    }

    // The slide is one node — its id is on the heading — so what it cost is
    // the heading plus everything written under it.
    let markdown = out[start..].to_string();
    writer
        .ids()
        .record(slide_id.as_deref(), NodeKind::Slide, &markdown, mark);
}

/// Appends a rendered block, blank-line separated.
fn push_block(out: &mut String, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with("\n\n") {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(text.trim_end_matches('\n'));
    out.push('\n');
}

fn is_title(shape: &Shape) -> bool {
    matches!(&shape.kind, ShapeKind::Placeholder(ph) if ph.ph_type.is_title())
}

fn placeholder_body(shape: &Shape) -> &[docsai_model::text::Block] {
    match &shape.kind {
        ShapeKind::Placeholder(ph) => &ph.body,
        _ => &[],
    }
}

/// What a shape is, as a warning names it.
fn shape_kind(shape: &Shape) -> String {
    match &shape.kind {
        ShapeKind::Placeholder(ph) => format!("placeholder {}", ph.ph_type),
        ShapeKind::TextBox { .. } => "textbox".into(),
        ShapeKind::Picture(_) => "picture".into(),
        ShapeKind::Table(_) => "table".into(),
        ShapeKind::Chart(_) => "chart".into(),
        ShapeKind::Group(_) => "group".into(),
        ShapeKind::Raw(raw) => raw.kind.as_str().into(),
    }
}

//! Presentation → DocMark-P (spec §11.2).
//!
//! A slide is an `##` heading carrying `.slide`, and the heading *is* the title
//! placeholder; the layout's primary body placeholder is written as ordinary
//! blocks under it. Which shape is which is a catalogue lookup, and the
//! lookup is [`implicit_shapes`] — the same function the addressing walk asks,
//! so what is implicit here and what is addressable there cannot disagree.
//!
//! Every *other* shape is a container — `::: {.ph idx=…}` for a placeholder,
//! `::: {.shape geom=…}` for a free shape, `::: {.connector …}` for a
//! connector — because Markdown has no shape and the alternative is losing
//! where the author put things (spec §11.2 rule 4).
//!
//! A picture and a table are the exception: Markdown has both, so they are
//! written as an image line and as a GFM table, carrying their *shape's* id
//! and placement. What Markdown has no form for at all — a chart, SmartArt,
//! an OLE object, a media clip, a custom geometry — is a visible stub over the
//! raw fragment that holds it (rule 8), never a silent omission
//! (`AGENTS.md` §7 rule 3).

use docsai_model::addressing::{implicit_shapes, NodeKind};
use docsai_model::assets::AssetStore;
use docsai_model::image::{Flip, ImageRef};
use docsai_model::presentation::{PhType, Presentation, Shape, ShapeKind, Slide};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::text::{Block, Table};

use crate::attrs::Attrs;
use crate::dict::AttrDict;
use crate::escape::{escape, TextContext};
use crate::ids::IdSource;
use crate::units::number;
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
    let mut writer = Writer::new(options, &deck.styles, assets, ids, dict).for_deck();

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

    for (i, shape) in slide.shapes.iter().enumerate() {
        if implicit.contains(&i) {
            continue;
        }
        write_shape(out, shape, writer, options, &location);
    }

    if let Some(notes) = &slide.notes {
        write_notes(out, notes, writer, options, &location);
    }

    let report = writer.report_mut();
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

/// Writes the speaker notes of a slide (spec §11.2 rule 5).
///
/// The one node whose *syntax* depends on the fidelity level: a container at
/// `full` and `agent`, a blockquote at `standard`. A blockquote is native
/// CommonMark and cannot collide with slide content — PresentationML has no
/// blockquote for a placeholder to occupy — so the level that has to stay
/// hand-editable pays no syntax for its notes.
fn write_notes(
    out: &mut String,
    notes: &[Block],
    writer: &mut Writer,
    options: &Options,
    location: &str,
) {
    if options.fidelity == Fidelity::Plain {
        if !notes.is_empty() {
            writer.report_mut().warn(Warning::UnsupportedElement {
                kind: "notes".into(),
                location: location.into(),
                action: "dropped: plain writes what the slide shows".into(),
            });
        }
        return;
    }

    let body = writer.render_slide_blocks(notes);
    if options.fidelity.addresses() {
        // An empty notes slide is written as an empty container: the deck has
        // one, and «has an empty notes slide» and «has none» are different
        // things to the writer that puts the package back.
        let mut attrs = Attrs::new();
        attrs.class("notes");
        let rendered = if body.trim().is_empty() {
            format!("::: {}\n:::", attrs.render_with(writer.dict()))
        } else {
            format!(
                "::: {}\n{}\n:::",
                attrs.render_with(writer.dict()),
                body.trim_end()
            )
        };
        push_block(out, &rendered);
        return;
    }

    // `standard`: a blockquote, and an empty notes slide leaves nothing. There
    // is nothing to quote, and a lone `>` is noise in a document that does not
    // write back.
    if body.trim().is_empty() {
        return;
    }
    push_block(out, &blockquote(&body));
}

/// A rendered run of blocks as a CommonMark blockquote.
fn blockquote(body: &str) -> String {
    body.trim_end()
        .lines()
        .map(|line| {
            if line.is_empty() {
                // `>` alone, with no trailing space: a blank line inside a
                // blockquote still belongs to it, and trailing spaces do not
                // survive a round trip.
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Writes one shape that the layout does not make implicit (spec §11.2 rule 4).
///
/// A placeholder becomes `.ph`, a text box or a preset shape `.shape`, a
/// connector `.connector`, a chart `.chart` and every other unmodelled object
/// the class its stub carries. Pictures, tables and groups leave here for a
/// form of their own.
fn write_shape(
    out: &mut String,
    shape: &Shape,
    writer: &mut Writer,
    options: &Options,
    location: &str,
) {
    let plain = options.fidelity == Fidelity::Plain;
    // Geometry, indices, names and raw references are what the writing levels
    // put back and the reading levels do not need (rules 6 and 7).
    let addresses = options.fidelity.addresses();

    // What the container is, what it holds, and the raw fragment behind it.
    let class;
    let mut blocks: &[Block] = &[];
    let mut label = "";
    let mut raw = None;
    // What only one kind of shape knows, folded into its container.
    let mut extra = Attrs::new();
    match &shape.kind {
        // Three kinds are not containers at all: Markdown has an image, a
        // table and — for a group — nothing to say beyond its children.
        ShapeKind::Picture(image) => {
            write_picture(out, shape, image, writer, options);
            return;
        }
        ShapeKind::Table(table) => {
            write_table(out, shape, table, writer, options, location);
            return;
        }
        ShapeKind::Group(children) => {
            write_group(out, shape, children, writer, options, location);
            return;
        }
        ShapeKind::Chart(chart) => {
            // A chart is a rule-8 stub until Phase 16 turns its series into a
            // table. The title is what a reader can see, so it is the body
            // rather than an attribute, and it survives `plain`.
            class = "chart";
            label = chart.title.as_deref().unwrap_or_default();
            raw = chart.raw.as_ref();
            extra.set_opt("kind", chart.kind.clone());
            if let Some(path) = chart.workbook.as_ref().and_then(|w| writer.asset_path(w)) {
                // Where the numbers are. Not a raw payload — the workbook is a
                // real asset beside the document — so every level that writes
                // a container writes it.
                extra.set("data", path);
            }
        }
        ShapeKind::Placeholder(ph) => {
            if ph.ph_type.is_furniture() && !addresses {
                // A slide number, a date or a footer is inherited from the
                // layout and holds no authored content; a reader gains a box
                // per slide and loses nothing when it goes.
                writer.report_mut().warn(Warning::UnsupportedElement {
                    kind: shape_kind(shape),
                    location: location.into(),
                    action: "dropped: slide furniture is inherited from the layout".into(),
                });
                return;
            }
            class = "ph";
            blocks = &ph.body;
        }
        ShapeKind::TextBox { body } => {
            class = "shape";
            blocks = body;
        }
        ShapeKind::Raw(shape_raw) => {
            // `.shape`, `.connector`, `.smartart`, `.ole`, `.media`,
            // `.object`: the class is what the stub is, and the stub is at
            // every level (rule 8). Nothing here is skipped any more.
            class = shape_raw.kind.as_str();
            label = shape_raw.text.as_str();
            raw = shape_raw.raw.as_ref();
        }
    }

    if plain {
        // No containers at `plain`: a slide is heading, bullets and images.
        // The text a shape holds still reaches the reader — only the box does
        // not.
        let body = writer.render_slide_blocks(blocks);
        push_block(out, &body);
        if !label.is_empty() {
            push_block(out, &escape(label, TextContext::Block));
        }
        if blocks.is_empty() && label.is_empty() {
            writer.report_mut().warn(Warning::UnsupportedElement {
                kind: shape_kind(shape),
                location: location.into(),
                action: "dropped: plain writes no shape containers".into(),
            });
        }
        return;
    }

    let mark = writer.ids().mark();
    let mut attrs = Attrs::new();
    // The id comes before the body, which is the order the addressing walk
    // visits them in: a shape, then what it holds.
    let id = writer.ids().take(shape);
    if let Some(id) = id.clone() {
        attrs.id(id);
    }
    attrs.class(class);

    if let Some(ph) = shape.placeholder() {
        // `p:ph@idx` is what matches this shape to its layout placeholder, so
        // it is only useful to the levels that write back. The type is what a
        // *reader* needs — a footer and a chart slot are not the same box —
        // and `body` is the PresentationML default, so it stays unwritten.
        if addresses {
            attrs.set_opt("idx", ph.idx.map(|idx| idx.to_string()));
        }
        if ph.ph_type != PhType::Body {
            attrs.set("type", ph.ph_type.as_str());
        }
    }

    if let Some(preset) = &shape.geometry.preset {
        // `geom=` is identity, not measurement: it is the only thing that says
        // a box is an arrow, so it survives at `standard` where `pos=` does
        // not.
        attrs.set("geom", preset.as_str());
    }
    if addresses {
        attrs.merge(&placement(shape, writer, true));
    }
    attrs.merge(&extra);

    if let Some(raw_id) = raw {
        if addresses {
            attrs.set("raw", raw_id.as_str());
        } else {
            // The stub stays — rule 8 keeps it at every level — but the bytes
            // it points at are not in this document.
            writer.report_mut().warn(Warning::RawBlockDropped {
                id: raw_id.as_str().to_string(),
                format: "pml".into(),
            });
        }
    }

    let mut body = writer.render_slide_blocks(blocks);
    if !label.is_empty() {
        // The text a preset shape shows. A stub that swallows an arrow's
        // label is a silent loss.
        body = escape(label, TextContext::Block);
    }

    let rendered = if body.trim().is_empty() {
        format!("::: {}\n:::", attrs.render_with(writer.dict()))
    } else {
        format!(
            "::: {}\n{}\n:::",
            attrs.render_with(writer.dict()),
            body.trim_end()
        )
    };
    writer
        .ids()
        .record(id.as_deref(), NodeKind::Shape, &rendered, mark);
    push_block(out, &rendered);
}

/// Where a shape sits, as the levels that write back need it (rule 7).
///
/// `size` is skipped for a picture: the image line already carries its own
/// `width`/`height`, and the same measurement written twice is one that can
/// disagree with itself.
fn placement(shape: &Shape, writer: &Writer, with_size: bool) -> Attrs {
    let geometry = &shape.geometry;
    let mut attrs = Attrs::new();
    attrs.set_opt("name", shape.name.clone());
    if let Some(pos) = geometry.pos {
        attrs.set(
            "pos",
            format!(
                "{},{}",
                writer.drawing_length(pos.x),
                writer.drawing_length(pos.y)
            ),
        );
    }
    if with_size {
        if let Some(size) = geometry.size {
            attrs.set(
                "size",
                format!(
                    "{},{}",
                    writer.drawing_length(size.width),
                    writer.drawing_length(size.height)
                ),
            );
        }
    }
    if geometry.rotation_deg != 0.0 {
        attrs.set("rotation", number(geometry.rotation_deg));
    }
    if geometry.flip != Flip::None {
        attrs.set("flip", geometry.flip.as_str());
    }
    attrs
}

/// A picture shape: an image line, not a container.
///
/// Markdown has an image, so rule 4 does not apply — a `:::` around it would
/// be a box a reader cannot use. The shape's id and position ride on the
/// image's own attribute block, which is where the addressing walk expects
/// them: a picture shape is one node, not a shape holding an image.
fn write_picture(
    out: &mut String,
    shape: &Shape,
    image: &ImageRef,
    writer: &mut Writer,
    options: &Options,
) {
    let mark = writer.ids().mark();
    let mut attrs = Attrs::new();
    let mut id = None;
    if options.fidelity != Fidelity::Plain {
        id = writer.ids().take(shape);
        if let Some(id) = id.clone() {
            attrs.id(id);
        }
        if options.fidelity.addresses() {
            attrs.merge(&placement(shape, writer, false));
        }
    }
    let rendered = writer.image_line(image, attrs);
    writer
        .ids()
        .record(id.as_deref(), NodeKind::Shape, &rendered, mark);
    push_block(out, &rendered);
}

/// A table shape: the GFM table the document writer already knows how to
/// write, carrying the shape's id and placement instead of a table id of its
/// own — the walk addresses the shape, not the table inside it.
fn write_table(
    out: &mut String,
    shape: &Shape,
    table: &Table,
    writer: &mut Writer,
    options: &Options,
    location: &str,
) {
    if table.rows.is_empty() {
        // A GFM table needs a row to be a table at all, and an empty one would
        // render as a header rule over nothing.
        writer.report_mut().warn(Warning::UnsupportedElement {
            kind: "table".into(),
            location: location.into(),
            action: "dropped: the table has no rows".into(),
        });
        return;
    }

    let mark = writer.ids().mark();
    let mut seed = Attrs::new();
    let mut id = None;
    if options.fidelity != Fidelity::Plain {
        id = writer.ids().take(shape);
        if let Some(id) = id.clone() {
            seed.id(id);
        }
        if options.fidelity.addresses() {
            seed.merge(&placement(shape, writer, true));
        }
    }
    let rendered = writer.render_slide_table(table, seed);
    writer
        .ids()
        .record(id.as_deref(), NodeKind::Shape, &rendered, mark);
    push_block(out, &rendered);
}

/// A group: a container holding the shapes it groups.
///
/// Every shape inside a group is addressable in its own right — the walk says
/// so — so the group is a box around them and never a substitute for them.
/// Flattening it would lose the grouping silently, which rule 3 forbids.
fn write_group(
    out: &mut String,
    shape: &Shape,
    children: &[Shape],
    writer: &mut Writer,
    options: &Options,
    location: &str,
) {
    if options.fidelity == Fidelity::Plain {
        // No container, but the children still reach the reader in order.
        for child in children {
            write_shape(out, child, writer, options, location);
        }
        return;
    }

    let mark = writer.ids().mark();
    let mut attrs = Attrs::new();
    let id = writer.ids().take(shape);
    if let Some(id) = id.clone() {
        attrs.id(id);
    }
    attrs.class("group");
    if options.fidelity.addresses() {
        attrs.merge(&placement(shape, writer, true));
    }

    let mut body = String::new();
    for child in children {
        write_shape(&mut body, child, writer, options, location);
    }

    let rendered = if body.trim().is_empty() {
        format!("::: {}\n:::", attrs.render_with(writer.dict()))
    } else {
        format!(
            "::: {}\n{}\n:::",
            attrs.render_with(writer.dict()),
            body.trim_end()
        )
    };
    writer
        .ids()
        .record(id.as_deref(), NodeKind::Shape, &rendered, mark);
    push_block(out, &rendered);
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

//! The slide the package does not have (Phase 15, increment 15-G).
//!
//! Everywhere else this writer *splices*: the part is the one that was read and
//! what comes from the model is the `a:p` sequence inside it ([[63-phase-15-slides]]).
//! That rule needs a part to splice into, and two decks do not have one: a deck
//! typed as attribute-free Markdown, which never had a package at all, and a
//! deck whose model carries a slide the package does not (the refusal 15-E
//! wrote down).
//!
//! This module is the other half: a `p:sld` **written from the model**, against
//! a layout chosen from the shape of the content. It does not replace the
//! splice and it never runs over a part that exists — a preserved deck goes on
//! behaving exactly as it did, byte for byte. It is the floor under the case
//! where there is nothing to preserve.
//!
//! **What chooses the layout** is the content, and the rules are stated rather
//! than guessed at (analysis §6.6):
//!
//! | the slide holds | the layout |
//! | --- | --- |
//! | a title and one body placeholder | the layout with a title and a body — «Title and Content» |
//! | a title and nothing else | the layout whose title is a *centred* one — «Title Slide» |
//! | a layout the model names, and the package has it | that one, and no rule is applied |
//! | anything else | the title-and-content layout, and a warning saying so |
//!
//! The rules pick a layout by its **placeholders**, never by its name: this
//! module runs against the embedded template, whose layouts are known, and
//! against a user's own deck, whose layouts are named in whatever language the
//! deck was authored in.

use docsai_model::image::ImageRef;
use docsai_model::presentation::{
    ChartRef, LayoutId, PhType, Placeholder, RawShape, Shape, ShapeKind, Slide,
};
use docsai_model::text::{Block, Table};
use docsai_model::units::{Length, Size};

use crate::package::{ContentTypes, Package};
use crate::xml::Element;

use super::write_text::{self, Links};

/// The namespaces a generated part declares on its root element.
const NS: &str = "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
                  xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
                  xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"";

/// The built-in «No Style, Table Grid» table style: a generated table names it
/// so that it has borders. A style reference the deck cannot resolve is a table
/// drawn with nothing at all.
const DEFAULT_TABLE_STYLE: &str = "{2D5ABB26-0587-4C30-8999-92F81FD0307C}";

/// What a generated part needs from outside itself: relationship ids.
///
/// The same one call wide as [`Links`], for the same reason: this module writes
/// markup and knows nothing about how a `_rels` part is stored or numbered.
pub(super) trait Parts: Links {
    /// The relationship id for an image's bytes, minting the media part when
    /// the package does not already carry it.
    fn image(&mut self, image: &ImageRef) -> Result<String, String>;
}

/// A layout of the package, as the chooser sees it.
pub(super) struct LayoutOption {
    /// The part name, which is also the [`LayoutId`] the IR uses.
    pub part: String,
    /// `p:cSld@name`.
    pub name: String,
    /// The layout's title placeholder: its type and `idx`.
    pub title: Option<(PhType, Option<u32>)>,
    /// Its first body-like placeholder, which is where a slide's blocks go —
    /// a subtitle included, because that is what a title slide holds them in.
    pub body: Option<(PhType, Option<u32>)>,
    /// Its first *content* placeholder: a body or an object, and never a
    /// subtitle. The difference is what tells «Title and Content» from «Title
    /// Slide», whose subtitle is body-like to every other part of this crate.
    pub content: Option<(PhType, Option<u32>)>,
    /// How many placeholders it declares, so the chooser can prefer the plain
    /// «Title and Content» over a two-content or a picture-with-caption layout.
    pub placeholders: usize,
}

/// The layouts the package declares, in part-name order.
///
/// Read here rather than through [`super::read_layout`] because the choice
/// needs three facts and not the cascade: a layout's part, its name and the
/// placeholders it declares. Nothing generated depends on what a placeholder
/// looks like — that is exactly what the layout is for.
pub(super) fn layouts_of(package: &Package) -> Vec<LayoutOption> {
    let types = ContentTypes::read(package);
    let kind = format!("{}.slideLayout+xml", super::PML);
    let mut parts: Vec<String> = types.parts_of(&kind).map(str::to_string).collect();
    parts.sort();
    parts
        .into_iter()
        .filter_map(|part| {
            let source = package.text(&part).ok()?;
            let root = Element::parse(&part, source.as_bytes()).ok()?;
            let tree = root.path(&["cSld", "spTree"])?;
            let declared: Vec<(PhType, Option<u32>)> = tree
                .children_named("sp")
                .filter_map(|sp| {
                    let ph = sp.path(&["nvSpPr", "nvPr", "ph"])?;
                    Some((
                        PhType::parse(ph.attr("type").unwrap_or_default()),
                        ph.attr_i64("idx").and_then(|n| u32::try_from(n).ok()),
                    ))
                })
                .collect();
            Some(LayoutOption {
                name: super::csld_name(&root).unwrap_or_else(|| part.clone()),
                title: declared.iter().find(|(t, _)| t.is_title()).cloned(),
                body: declared.iter().find(|(t, _)| t.is_body()).cloned(),
                content: declared
                    .iter()
                    .find(|(t, _)| matches!(t, PhType::Body | PhType::Object))
                    .cloned(),
                placeholders: declared.len(),
                part,
            })
        })
        .collect()
}

/// Which layout a generated slide is written against, and why.
pub(super) struct Choice {
    pub layout: usize,
    /// `Some` when no rule matched and the fall-back was used: the words a
    /// warning carries. `None` is a slide the rules answered for, and a slide
    /// nobody has to be told about.
    pub unmatched: Option<String>,
}

/// Chooses the layout for one slide from the shape of its content.
///
/// A layout the model **names** wins outright when the package has it: that is
/// not a guess at all, it is the deck saying what it wants — the case of a
/// slide added to a deck that was read from a `.pptx`.
pub(super) fn choose(layouts: &[LayoutOption], slide: &Slide) -> Option<Choice> {
    if let Some(named) = slide.layout.as_ref().map(LayoutId::as_str) {
        if let Some(index) = layouts.iter().position(|layout| layout.part == named) {
            return Some(Choice {
                layout: index,
                unmatched: None,
            });
        }
    }

    // The census the rules are stated over: what the slide actually holds.
    let mut titles = 0;
    let mut bodies = 0;
    let mut others = 0;
    for shape in &slide.shapes {
        match &shape.kind {
            ShapeKind::Placeholder(ph) if ph.ph_type.is_title() => titles += 1,
            ShapeKind::Placeholder(ph) if ph.ph_type.is_body() => bodies += 1,
            _ => others += 1,
        }
    }

    // The fall-back, and the answer to rule 1: a title with content under it.
    // Ordered by how plain the layout is — a centred title is a title slide
    // and not a content one, and the fewer placeholders the better, so that a
    // two-content or a picture-with-caption layout is never what a title and
    // some bullets land on.
    let content = layouts
        .iter()
        .enumerate()
        .filter(|(_, layout)| layout.title.is_some() && layout.content.is_some())
        .min_by_key(|(index, layout)| {
            (
                matches!(&layout.title, Some((PhType::CenterTitle, _))),
                layout.placeholders,
                *index,
            )
        })
        .map(|(index, _)| index);

    if titles == 1 && bodies == 1 && others == 0 {
        if let Some(index) = content {
            return Some(Choice {
                layout: index,
                unmatched: None,
            });
        }
    }
    if titles == 1 && bodies == 0 && others == 0 {
        // A title and nothing else is a title slide, which is the layout whose
        // title is *centred*; a deck with none falls through to the rest.
        let title_slide = layouts
            .iter()
            .position(|layout| {
                matches!(&layout.title, Some((PhType::CenterTitle, _))) && layout.body.is_some()
            })
            .or_else(|| {
                layouts
                    .iter()
                    .position(|layout| layout.title.is_some() && layout.body.is_none())
            });
        if let Some(index) = title_slide {
            return Some(Choice {
                layout: index,
                unmatched: None,
            });
        }
    }

    let index = content.or_else(|| layouts.iter().position(|l| l.title.is_some()))?;
    Some(Choice {
        layout: index,
        unmatched: Some(format!(
            "it holds {}, which matches neither of the rules that choose a layout \
             from content (a title with body text is «Title and Content», a title \
             alone is «Title Slide»), so it was written against `{}`",
            census(titles, bodies, others),
            layouts[index].name
        )),
    })
}

fn census(titles: usize, bodies: usize, others: usize) -> String {
    let mut said = Vec::new();
    for (n, one, many) in [
        (titles, "a title", "titles"),
        (bodies, "body text", "body placeholders"),
        (others, "one other shape", "other shapes"),
    ] {
        match n {
            0 => {}
            1 => said.push(one.to_string()),
            n => said.push(format!("{n} {many}")),
        }
    }
    if said.is_empty() {
        return "nothing at all".into();
    }
    said.join(" and ")
}

/// One slide written from the model, against the layout that was chosen.
///
/// `kept` collects what could not be generated, in the words a warning carries.
/// Every one of them is a shape whose markup only the original package could
/// have supplied — a chart, a diagram, an embedded object — and for those the
/// slide states a text box holding the stub's own label, so that the slide says
/// what is missing rather than silently having a hole in it.
pub(super) fn slide_xml(
    slide: &Slide,
    layout: &LayoutOption,
    canvas: Size,
    parts: &mut dyn Parts,
    kept: &mut Vec<String>,
) -> Result<String, String> {
    let mut out = String::from("<p:sld ");
    out.push_str(NS);
    out.push('>');
    out.push_str("<p:cSld");
    if let Some(name) = &slide.name {
        out.push_str(&format!(" name=\"{}\"", super::write::esc_attr(name)));
    }
    out.push_str(
        "><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/>\
         </p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/>\
         <a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>",
    );

    // A free shape needs a position, and a model that came from plain Markdown
    // states none. They are laid out down the layout's content area, in reading
    // order, sharing it — which is a *choice* this writer makes and not
    // something recovered from the document.
    let free: Vec<usize> = slide
        .shapes
        .iter()
        .enumerate()
        .filter(|(_, shape)| !is_placeholder(shape) && shape.geometry.pos.is_none())
        .map(|(index, _)| index)
        .collect();

    let mut body_used = false;
    for (id, (index, shape)) in (2u32..).zip(slide.shapes.iter().enumerate()) {
        let frame = match free.iter().position(|&i| i == index) {
            Some(slot) => slice(canvas, slot, free.len()),
            None => match (shape.geometry.pos, shape.geometry.size) {
                (Some(pos), Some(size)) => (pos.x, pos.y, size.width, size.height),
                (Some(pos), None) => {
                    let (_, _, cx, cy) = slice(canvas, 0, 1);
                    (pos.x, pos.y, cx, cy)
                }
                _ => slice(canvas, 0, 1),
            },
        };
        match &shape.kind {
            ShapeKind::Placeholder(ph) => {
                out.push_str(&placeholder_xml(
                    shape,
                    ph,
                    layout,
                    &mut body_used,
                    id,
                    parts,
                )?);
            }
            ShapeKind::TextBox { body } => {
                out.push_str(&text_box_xml(
                    shape_name(shape, id, "TextBox"),
                    id,
                    frame,
                    &write_text::write_body(body, false, None, parts)?,
                ));
            }
            ShapeKind::Picture(image) => {
                out.push_str(&picture_xml(shape, image, id, frame, parts)?);
            }
            ShapeKind::Table(table) => {
                out.push_str(&table_xml(shape, table, id, frame, parts)?);
            }
            other => {
                // A chart, a diagram, an OLE object: markup no model holds and
                // no template can supply. The label the stub carries is written
                // where the object would have been, so that the slide states
                // what is not there.
                kept.push(unbuildable(other));
                let label = stub_label(other);
                let paragraph = if label.is_empty() {
                    "<a:p/>".to_string()
                } else {
                    format!(
                        "<a:p><a:r><a:rPr lang=\"en-US\"/><a:t>{}</a:t></a:r></a:p>",
                        write_text::esc_text(&label)
                    )
                };
                out.push_str(&text_box_xml(
                    shape_name(shape, id, "Object"),
                    id,
                    frame,
                    &paragraph,
                ));
            }
        }
    }

    out.push_str("</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>");
    Ok(out)
}

/// The notes page of a generated slide: the one body the reader looks for.
pub(super) fn notes_xml(blocks: &[Block], parts: &mut dyn Parts) -> Result<String, String> {
    let paragraphs = write_text::write_body(blocks, false, None, parts)?;
    let paragraphs = if paragraphs.is_empty() {
        "<a:p/>".to_string()
    } else {
        paragraphs
    };
    Ok(format!(
        "<p:notes {NS}><p:cSld><p:spTree>\
         <p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
         <p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/>\
         <a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>\
         <p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Notes Placeholder 1\"/>\
         <p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr>\
         <p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/>\
         <p:txBody><a:bodyPr/><a:lstStyle/>{paragraphs}</p:txBody></p:sp>\
         </p:spTree></p:cSld></p:notes>"
    ))
}

fn is_placeholder(shape: &Shape) -> bool {
    matches!(shape.kind, ShapeKind::Placeholder(_))
}

/// Why a shape could not be generated, in the words a warning carries.
fn unbuildable(kind: &ShapeKind) -> String {
    let what = match kind {
        ShapeKind::Chart(_) => "a chart, whose data lives in a part of its own",
        ShapeKind::Group(_) => "a group, whose children the IR keeps but whose markup it does not",
        ShapeKind::Raw(_) => "a preserved object, which is markup only its own package holds",
        _ => "a shape this writer cannot generate",
    };
    format!(
        "{what}: the slide was written from the model rather than spliced into a \
         preserved one, and a text box carrying its label stands where it was"
    )
}

fn stub_label(kind: &ShapeKind) -> String {
    match kind {
        ShapeKind::Chart(ChartRef { title, .. }) => title.clone().unwrap_or_default(),
        ShapeKind::Raw(RawShape { text, .. }) => text.clone(),
        _ => String::new(),
    }
}

fn shape_name(shape: &Shape, id: u32, fallback: &str) -> String {
    shape
        .name
        .clone()
        .unwrap_or_else(|| format!("{fallback} {id}"))
}

/// The content area of the canvas, divided between the shapes that share it.
///
/// The fractions are the ones the embedded template's own body placeholder
/// uses, expressed against the slide so that a deck with a different canvas
/// still gets a margin rather than a shape hanging off the edge.
fn slice(canvas: Size, slot: usize, total: usize) -> (Length, Length, Length, Length) {
    let width = canvas.width.emu().max(1);
    let height = canvas.height.emu().max(1);
    let x = width / 16;
    let y = height / 4;
    let cx = width - 2 * x;
    let full = height - y - height / 16;
    let total = total.max(1) as i64;
    let gap = if total > 1 { full / (total * 12) } else { 0 };
    let cy = (full - gap * (total - 1)) / total;
    let slot = slot as i64;
    (
        Length::from_emu(x),
        Length::from_emu(y + slot * (cy + gap)),
        Length::from_emu(cx),
        Length::from_emu(cy.max(1)),
    )
}

fn xfrm(frame: (Length, Length, Length, Length), prefix: &str) -> String {
    let (x, y, cx, cy) = frame;
    format!(
        "<{prefix}xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></{prefix}xfrm>",
        x.emu(),
        y.emu(),
        cx.emu().max(1),
        cy.emu().max(1)
    )
}

/// A placeholder written against the chosen layout's own identity.
///
/// The `p:ph` a slide states is what binds it to the layout, so the type and
/// the `idx` come from the **layout** and not from the model: a title written
/// against «Title Slide» is that layout's `ctrTitle`, and writing `title` there
/// would leave the box unmatched and the text at the master's position.
fn placeholder_xml(
    shape: &Shape,
    ph: &Placeholder,
    layout: &LayoutOption,
    body_used: &mut bool,
    id: u32,
    parts: &mut dyn Parts,
) -> Result<String, String> {
    let (ph_type, idx) = if ph.ph_type.is_title() {
        layout
            .title
            .clone()
            .unwrap_or_else(|| (ph.ph_type.clone(), ph.idx))
    } else if ph.ph_type.is_body() && !*body_used {
        *body_used = true;
        layout
            .body
            .clone()
            .unwrap_or_else(|| (ph.ph_type.clone(), ph.idx))
    } else {
        (ph.ph_type.clone(), ph.idx)
    };
    let idx = match idx {
        Some(idx) => format!(" idx=\"{idx}\""),
        None => String::new(),
    };
    let paragraphs = write_text::write_body(&ph.body, ph_type.is_body(), None, parts)?;
    let paragraphs = if paragraphs.is_empty() {
        "<a:p/>".to_string()
    } else {
        paragraphs
    };
    let name = shape_name(
        shape,
        id,
        if ph_type.is_title() {
            "Title"
        } else {
            "Content Placeholder"
        },
    );
    Ok(format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{}\"/>\
         <p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr>\
         <p:nvPr><p:ph type=\"{}\"{idx}/></p:nvPr></p:nvSpPr><p:spPr/>\
         <p:txBody><a:bodyPr/><a:lstStyle/>{paragraphs}</p:txBody></p:sp>",
        super::write::esc_attr(&name),
        ph_type.as_str(),
    ))
}

fn text_box_xml(
    name: String,
    id: u32,
    frame: (Length, Length, Length, Length),
    paragraphs: &str,
) -> String {
    let paragraphs = if paragraphs.is_empty() {
        "<a:p/>"
    } else {
        paragraphs
    };
    format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/>\
         </p:nvSpPr><p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/>\
         </p:spPr><p:txBody><a:bodyPr wrap=\"square\"/><a:lstStyle/>{paragraphs}</p:txBody></p:sp>",
        super::write::esc_attr(&name),
        xfrm(frame, "a:"),
    )
}

/// A `p:pic` over the bytes the asset store holds.
///
/// The size is the model's when it states one — a `full` round trip does — and
/// the content area otherwise, because a picture with no extent is a picture
/// nobody sees.
fn picture_xml(
    shape: &Shape,
    image: &ImageRef,
    id: u32,
    frame: (Length, Length, Length, Length),
    parts: &mut dyn Parts,
) -> Result<String, String> {
    if image.external_src.is_some() {
        return Err(
            "a linked picture, whose bytes are not in the document at all, on a slide \
             this writer generated"
                .into(),
        );
    }
    let rel = parts.image(image)?;
    let size = image.geometry.display_size;
    let frame = if size.width.emu() > 0 && size.height.emu() > 0 {
        (frame.0, frame.1, size.width, size.height)
    } else {
        frame
    };
    let name = shape_name(shape, id, "Picture");
    let descr = if image.alt.is_empty() {
        String::new()
    } else {
        format!(" descr=\"{}\"", super::write::esc_attr(&image.alt))
    };
    Ok(format!(
        "<p:pic><p:nvPicPr><p:cNvPr id=\"{id}\" name=\"{}\"{descr}/><p:cNvPicPr/><p:nvPr/>\
         </p:nvPicPr><p:blipFill><a:blip r:embed=\"{}\"/><a:stretch><a:fillRect/></a:stretch>\
         </p:blipFill><p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>",
        super::write::esc_attr(&image.name.clone().unwrap_or(name)),
        super::write::esc_attr(&rel),
        xfrm(frame, "a:"),
    ))
}

/// An `a:tbl` written from the model, inside the `p:graphicFrame` that holds it.
///
/// This is the writer [[64-phase-15-media]] pointed at 15-G for, and it is here
/// on the terms that increment set: it writes a table onto a slide **being
/// generated**, where there is no grid to preserve. A table inside a preserved
/// slide is still spliced cell by cell, and a preserved grid the model moved is
/// still copied and named — regenerating one would throw away the row heights,
/// the cell properties and the band flags the IR does not model.
fn table_xml(
    shape: &Shape,
    table: &Table,
    id: u32,
    frame: (Length, Length, Length, Length),
    parts: &mut dyn Parts,
) -> Result<String, String> {
    let columns = table.width().max(1);
    let width = frame.2.emu().max(1);
    let mut widths: Vec<i64> = table.col_widths.iter().map(|w| w.emu()).collect();
    if widths.len() != columns || widths.iter().any(|w| *w <= 0) {
        widths = (0..columns).map(|_| width / columns as i64).collect();
    }
    let mut grid = String::new();
    for column in &widths {
        grid.push_str(&format!("<a:gridCol w=\"{column}\"/>"));
    }

    let height = frame.3.emu().max(1);
    let row_height = (height / table.rows.len().max(1) as i64).max(1);
    let mut rows = String::new();
    for row in &table.rows {
        rows.push_str(&format!("<a:tr h=\"{row_height}\">"));
        let mut written = 0usize;
        for cell in &row.cells {
            let span = cell.colspan.max(1) as usize;
            let mut attrs = String::new();
            if span > 1 {
                attrs.push_str(&format!(" gridSpan=\"{span}\""));
            }
            if cell.rowspan > 1 {
                attrs.push_str(&format!(" rowSpan=\"{}\"", cell.rowspan));
            }
            if cell.covered {
                attrs.push_str(" vMerge=\"1\"");
            }
            let paragraphs = write_text::write_body(&cell.blocks, false, None, parts)?;
            let paragraphs = if paragraphs.is_empty() {
                "<a:p/>".to_string()
            } else {
                paragraphs
            };
            rows.push_str(&format!(
                "<a:tc{attrs}><a:txBody><a:bodyPr/><a:lstStyle/>{paragraphs}</a:txBody>\
                 <a:tcPr/></a:tc>"
            ));
            // A cell swallowed horizontally is not in the IR grid at all — the
            // reader dropped it — so the columns a span covers are written back
            // as the `hMerge` cells the format requires in every row.
            for _ in 1..span {
                rows.push_str(
                    "<a:tc hMerge=\"1\"><a:txBody><a:bodyPr/><a:lstStyle/><a:p/>\
                               </a:txBody><a:tcPr/></a:tc>",
                );
            }
            written += span;
        }
        // Every row of an `a:tbl` states as many cells as the grid has columns.
        for _ in written..columns {
            rows.push_str(
                "<a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p/></a:txBody><a:tcPr/></a:tc>",
            );
        }
        rows.push_str("</a:tr>");
    }

    let style = table
        .style
        .as_ref()
        .map(|id| id.as_str().to_string())
        .unwrap_or_else(|| DEFAULT_TABLE_STYLE.to_string());
    let band = if table.header_row {
        " firstRow=\"1\""
    } else {
        ""
    };
    Ok(format!(
        "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"{}\"/>\
         <p:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></p:cNvGraphicFramePr>\
         <p:nvPr/></p:nvGraphicFramePr>{}<a:graphic><a:graphicData \
         uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\">\
         <a:tbl><a:tblPr{band} bandRow=\"1\"><a:tableStyleId>{}</a:tableStyleId></a:tblPr>\
         <a:tblGrid>{grid}</a:tblGrid>{rows}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>",
        super::write::esc_attr(&shape_name(shape, id, "Table")),
        xfrm(frame, "p:"),
        super::write::esc_attr(&style),
    ))
}

/// The content types a generated part declares, by the part's own name.
pub(super) fn content_type_of(part: &str) -> Option<String> {
    if part.starts_with("ppt/slides/") {
        Some(format!("{}.slide+xml", super::PML))
    } else if part.starts_with("ppt/notesSlides/") {
        Some(format!("{}.notesSlide+xml", super::PML))
    } else {
        None
    }
}

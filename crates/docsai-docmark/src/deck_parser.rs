//! DocMark-P → IR (spec §11.2).
//!
//! The mirror of [`deck_writer`](crate::deck_writer): a slide is an `##`
//! heading, the heading *is* the title placeholder, the blocks under it are the
//! layout's primary body placeholder, and every `:::` container is a shape.
//!
//! Two things this parser does that the writer has no counterpart for.
//!
//! It is **tolerant**: `.slide` is what the writer puts on a heading, and a
//! deck written by hand carries no attributes at all. A `#` or `##` heading
//! opens a slide either way, content before the first heading opens an untitled
//! one, and a container class it does not know becomes a text box with a
//! warning rather than an error — analysis §6.6, and the reason a human can
//! draft a deck in a text editor.
//!
//! And it never guesses **silently**: what it cannot place is warned through
//! the same [`ConversionReport`] the readers use, because a parser that drops a
//! container quietly is the same failure as a writer that does (`AGENTS.md` §7
//! rule 3).

use std::path::Path;

use docsai_model::addressing::NodeId;
use docsai_model::assets::AssetStore;
use docsai_model::image::{Flip, RawId};
use docsai_model::presentation::{
    ChartRef, LayoutCatalog, LayoutId, PhType, Placeholder, Presentation, RawShape, RawShapeKind,
    Shape, ShapeGeometry, ShapeKind, SkeletonRef, Slide,
};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::text::Block;
use docsai_model::units::{Length, Point, Size};
use docsai_model::{Document, Format};

use crate::attrs::Attrs;
use crate::error::ParseError;
use crate::frontmatter_parse::FrontMatter;
use crate::parser::{
    apply_table_attrs, looks_like_table, split_one_fence, split_tight_attrs, split_top_level,
    split_trailing_attrs, BodyParser,
};

/// True when the document should be parsed as a deck rather than as text.
///
/// The front matter answers it outright for anything this crate wrote; the body
/// scan is for a fragment pasted without one. `.slide` is the marker rule 1
/// requires at every level but `plain`, and a `plain` deck *is* a text document
/// — it has no slides to find, only headings — which is why nothing here tries
/// to guess one from `##` alone.
pub fn looks_like_deck(fm: &FrontMatter, body: &str) -> bool {
    if fm.source_format == Format::Pptx {
        return true;
    }
    if !fm.layouts.is_empty() || fm.skeleton.is_some() {
        return true;
    }
    body.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("##")
            && !trimmed.starts_with("###")
            && split_trailing_attrs(trimmed)
                .1
                .is_some_and(|attrs| attrs.has_class("slide"))
    })
}

/// Parses the body of a presentation DocMark file.
pub fn parse_deck(
    body: &str,
    body_line: usize,
    fm: FrontMatter,
    base_dir: Option<&Path>,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Document, ParseError> {
    let mut parser = BodyParser::new(base_dir, assets, report, &fm);

    let skeleton = match &fm.skeleton {
        Some(path) => match parser.find_skeleton(path)? {
            Some(asset) => Some(SkeletonRef {
                asset,
                rebuilt_parts: Vec::new(),
            }),
            None => {
                // The package the writer of Phase 15 re-injects slides into.
                // Without it a deck can still be read; it cannot be written
                // back over its original, and that is worth saying out loud.
                parser.report.warn(Warning::AssetIssue {
                    asset: path.clone(),
                    why: "the preserved package is not beside the document".into(),
                });
                None
            }
        },
        None => None,
    };

    let mut slides = Vec::new();
    for chunk in split_slides(body, body_line) {
        slides.push(parse_slide(&chunk, &fm.layouts, &mut parser)?);
    }

    report.stats.slides = slides.len() as u32;
    report.stats.styles = fm.styles.styles.len() as u32;

    Ok(Document::Presentation(Presentation {
        meta: fm.meta,
        addressing: fm.addressing,
        styles: fm.styles,
        layouts: fm.layouts,
        // `p:sldSz` is not a front-matter key: the canvas lives in the
        // preserved package, and a deck read from DocMark alone does not
        // invent one.
        slide_size: Size::default(),
        slides,
        skeleton,
        // Slide-level raw fragments travel in the sidecar, which the deck
        // writer does not reference yet; there is nothing here to read back.
        raw: Vec::new(),
    }))
}

/// One slide as it appears in the body: its heading line and everything under
/// it, up to the next heading.
struct SlideChunk {
    /// Line of the heading in the file, for the errors this slide can raise.
    line: usize,
    /// What follows the `##`, attribute block included. `None` for content
    /// that precedes the first heading.
    heading: Option<String>,
    body: String,
}

/// Splits the body into slides at every top-level `#` or `##` heading.
///
/// Top-level: a heading inside a `:::` container or a fenced code block is
/// content, not a new slide. `#` opens one as readily as `##` because a deck
/// drafted by hand starts its slides with whatever heading level the author
/// felt like; the writer only ever emits `##`, so nothing round-trips through
/// the first case.
fn split_slides(body: &str, body_line: usize) -> Vec<SlideChunk> {
    let mut out: Vec<SlideChunk> = Vec::new();
    let mut fence = 0i32;
    let mut code = false;
    for (offset, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            code = !code;
        } else if !code {
            if trimmed.starts_with(":::") {
                if trimmed.trim_start_matches(':').trim().is_empty() {
                    fence = (fence - 1).max(0);
                } else {
                    fence += 1;
                }
            } else if fence == 0 {
                if let Some(heading) = slide_heading(trimmed) {
                    out.push(SlideChunk {
                        line: body_line + offset,
                        heading: Some(heading.to_string()),
                        body: String::new(),
                    });
                    continue;
                }
            }
        }
        match out.last_mut() {
            Some(slide) => {
                slide.body.push_str(line);
                slide.body.push('\n');
            }
            None if trimmed.is_empty() => {}
            None => {
                // Content before the first heading: a slide with no title,
                // which is what a layout without a title placeholder writes.
                out.push(SlideChunk {
                    line: body_line + offset,
                    heading: None,
                    body: format!("{line}\n"),
                });
            }
        }
    }
    out
}

/// What follows the hashes of a slide heading, or `None` if the line is not
/// one. An empty string is a heading with no title — `##` — which rule 1
/// writes for a slide whose layout has no title placeholder.
fn slide_heading(line: &str) -> Option<&str> {
    let hashes = line.len() - line.trim_start_matches('#').len();
    if !(1..=2).contains(&hashes) {
        return None;
    }
    let rest = &line[hashes..];
    if rest.is_empty() {
        return Some("");
    }
    rest.strip_prefix(' ').map(str::trim)
}

fn parse_slide(
    chunk: &SlideChunk,
    layouts: &LayoutCatalog,
    parser: &mut BodyParser<'_>,
) -> Result<Slide, ParseError> {
    let heading = chunk.heading.as_deref().unwrap_or_default();
    let attrs = split_trailing_attrs(heading)
        .1
        .map(|a| parser.expanded(a))
        .unwrap_or_default();

    let mut slide = Slide {
        id: attrs.id_ref().map(NodeId::new),
        layout: attrs.get("layout").map(LayoutId::new),
        name: attrs.get("name").map(str::to_string),
        hidden: attrs.get("hidden") == Some("true"),
        section: attrs.get("section").map(str::to_string),
        ..Default::default()
    };

    // The heading is the title placeholder (rule 1). Its id is the *slide's*:
    // the two implicit shapes take none, so a paragraph id read off this line
    // would address the wrong node.
    let (mut title, _) = parser.parse_paragraph_line(heading, false)?;
    title.id = None;
    if !title.content.is_empty() {
        let ph_type = slide
            .layout
            .as_ref()
            .and_then(|id| layouts.layout(id))
            .and_then(|layout| layout.title())
            .map(|ph| ph.ph_type.clone())
            .unwrap_or(PhType::Title);
        slide.shapes.push(placeholder_shape(
            0,
            ph_type,
            None,
            vec![Block::Paragraph(title)],
        ));
    }

    // Everything under the heading, in writing order: the implicit body first,
    // then a shape per container.
    let mut body_blocks: Vec<Block> = Vec::new();
    let mut shapes: Vec<Shape> = Vec::new();
    for text in split_top_level(&chunk.body) {
        let trimmed = text.trim_start_matches('\n');
        if trimmed.trim().is_empty() {
            continue;
        }
        if trimmed.trim_start().starts_with(":::") {
            let (attrs, inner, _) = split_one_fence(trimmed, chunk.line)?;
            let attrs = parser.expanded(attrs);
            if attrs.has_class("notes") {
                // An empty container is an empty notes page, which is not the
                // same document as a slide with no notes page at all.
                slide.notes = Some(parser.parse_normal_blocks(inner, chunk.line)?);
                continue;
            }
            if let Some(shape) = parse_shape(&attrs, inner, chunk.line, parser)? {
                shapes.push(shape);
            }
            continue;
        }
        if is_blockquote(trimmed) {
            // `standard` writes the notes as a blockquote, and PresentationML
            // has no blockquote for slide content to be mistaken for (rule 5).
            slide.notes = Some(parser.parse_normal_blocks(&unquote(trimmed), chunk.line)?);
            continue;
        }
        if looks_like_table(trimmed) {
            let table = parser.parse_gfm_table(trimmed, chunk.line)?;
            shapes.push(Shape::new(0, ShapeKind::Table(table)));
            continue;
        }
        if let Some(shape) = try_picture(trimmed, parser)? {
            shapes.push(shape);
            continue;
        }
        body_blocks.extend(parser.parse_normal_blocks(trimmed, chunk.line)?);
    }

    if !body_blocks.is_empty() {
        // Rule 2: the blocks at slide level are the layout's primary body
        // placeholder, and its `idx` is what makes the writer treat it as
        // implicit again (`implicit_shapes`).
        let body = slide
            .layout
            .as_ref()
            .and_then(|id| layouts.layout(id))
            .and_then(|layout| layout.body());
        let (ph_type, idx) = match body {
            Some(ph) => (ph.ph_type.clone(), ph.idx),
            None => (PhType::Body, None),
        };
        slide
            .shapes
            .push(placeholder_shape(0, ph_type, idx, body_blocks));
    }
    slide.shapes.extend(shapes);
    // Z-order is not written: the body is in reading order, and the source
    // order of a deck that came from DocMark is the order it was read in.
    for (index, shape) in slide.shapes.iter_mut().enumerate() {
        shape.z_index = index as u32;
    }
    Ok(slide)
}

fn placeholder_shape(z_index: u32, ph_type: PhType, idx: Option<u32>, body: Vec<Block>) -> Shape {
    Shape::new(
        z_index,
        ShapeKind::Placeholder(Placeholder {
            ph_type,
            idx,
            body,
            ..Default::default()
        }),
    )
}

/// One `:::` container: what its class says it is (rules 4 and 8).
fn parse_shape(
    attrs: &Attrs,
    body: &str,
    line: usize,
    parser: &mut BodyParser<'_>,
) -> Result<Option<Shape>, ParseError> {
    let class = attrs.classes().first().map(String::as_str).unwrap_or("");
    let kind = match class {
        "ph" => ShapeKind::Placeholder(Placeholder {
            ph_type: attrs.get("type").map(PhType::parse).unwrap_or(PhType::Body),
            idx: attrs.get("idx").and_then(|v| v.parse().ok()),
            body: parser.parse_normal_blocks(body, line)?,
            ..Default::default()
        }),
        "table" => {
            let mut table = parser.parse_gfm_table(body.trim(), line)?;
            apply_table_attrs(&mut table, attrs);
            // The container's `#id` addresses the *shape*: a table shape is one
            // node, not a shape holding a table.
            table.id = None;
            ShapeKind::Table(table)
        }
        "group" => {
            let mut children = Vec::new();
            let mut rest = body.trim_start_matches('\n');
            while !rest.trim().is_empty() {
                if !rest.trim_start().starts_with(":::") {
                    // A group holds shapes and nothing else; text directly
                    // inside one is not something the writer can emit.
                    return Err(ParseError::unexpected(
                        line,
                        "a `.group` container holds `:::` shapes only",
                    ));
                }
                let (child_attrs, child_body, next) = split_one_fence(rest, line)?;
                let child_attrs = parser.expanded(child_attrs);
                if let Some(child) = parse_shape(&child_attrs, child_body, line, parser)? {
                    children.push(child);
                }
                rest = next;
            }
            for (index, child) in children.iter_mut().enumerate() {
                child.z_index = index as u32;
            }
            ShapeKind::Group(children)
        }
        "chart" => ShapeKind::Chart(ChartRef {
            kind: attrs.get("kind").map(str::to_string),
            title: label(body, parser, line)?,
            workbook: match attrs.get("data") {
                Some(path) => parser.find_asset(path)?,
                None => None,
            },
            raw: attrs.get("raw").map(RawId::new),
        }),
        "connector" | "smartart" | "ole" | "media" | "object" => ShapeKind::Raw(RawShape {
            kind: raw_kind(class),
            raw: attrs.get("raw").map(RawId::new),
            text: label(body, parser, line)?.unwrap_or_default(),
        }),
        // `.shape` is two things: a free text box, and the stub of a shape
        // Markdown has no form for (rule 8). What tells them apart is `raw=`:
        // a stub is a marker over markup that only the original package can
        // reproduce, and a text box is content. `geom=` says nothing here —
        // a text box with a rounded outline carries a preset too, and reading
        // it as a stub would freeze editable text into an opaque object.
        //
        // Without `raw=` — at `standard`, which drops it — a stub does come
        // back as a text box. That level does not write back, so what it costs
        // is nothing it was going to spend.
        "shape" if attrs.get("raw").is_some() => ShapeKind::Raw(RawShape {
            kind: RawShapeKind::Shape,
            raw: attrs.get("raw").map(RawId::new),
            text: label(body, parser, line)?.unwrap_or_default(),
        }),
        "shape" => ShapeKind::TextBox {
            body: parser.parse_normal_blocks(body, line)?,
        },
        other => {
            // Tolerance, not silence: the text survives as a box on the slide
            // and the reader is told the class meant nothing here.
            parser.report.warn(Warning::UnsupportedElement {
                kind: if other.is_empty() {
                    "container".into()
                } else {
                    format!(".{other}")
                },
                location: format!("line {line}"),
                action: "read as a text box: not a DocMark-P container class".into(),
            });
            ShapeKind::TextBox {
                body: parser.parse_normal_blocks(body, line)?,
            }
        }
    };

    let mut shape = Shape::new(0, kind);
    shape.id = attrs.id_ref().map(NodeId::new);
    shape.name = attrs.get("name").map(str::to_string);
    shape.geometry = geometry(attrs);
    Ok(Some(shape))
}

/// The text a stub shows, as one line. A stub that swallowed an arrow's label
/// would be the silent loss rule 8 exists to prevent.
fn label(
    body: &str,
    parser: &mut BodyParser<'_>,
    line: usize,
) -> Result<Option<String>, ParseError> {
    if body.trim().is_empty() {
        return Ok(None);
    }
    let text = parser
        .parse_normal_blocks(body, line)?
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(p) => Some(p.plain_text()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    Ok(Some(text).filter(|t| !t.is_empty()))
}

fn raw_kind(class: &str) -> RawShapeKind {
    match class {
        "connector" => RawShapeKind::Connector,
        "smartart" => RawShapeKind::SmartArt,
        "ole" => RawShapeKind::Ole,
        "media" => RawShapeKind::Media,
        _ => RawShapeKind::Other,
    }
}

/// Where a container says its shape sits (rule 7). Absent means *inherited*,
/// which is what the levels that write no geometry leave behind.
fn geometry(attrs: &Attrs) -> ShapeGeometry {
    ShapeGeometry {
        pos: attrs
            .get("pos")
            .and_then(pair)
            .map(|(x, y)| Point::new(x, y)),
        size: attrs
            .get("size")
            .and_then(pair)
            .map(|(w, h)| Size::new(w, h)),
        preset: attrs.get("geom").map(str::to_string),
        rotation_deg: attrs
            .get("rotation")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0),
        flip: match attrs.get("flip") {
            Some("h") => Flip::H,
            Some("v") => Flip::V,
            Some("hv") => Flip::HV,
            _ => Flip::None,
        },
    }
}

/// A `"x,y"` pair of lengths, as `pos=` and `size=` are written.
fn pair(value: &str) -> Option<(Length, Length)> {
    let (first, second) = value.split_once(',')?;
    Some((Length::parse(first.trim())?, Length::parse(second.trim())?))
}

/// A picture shape: an image line, with the shape's own address and placement
/// on the image's attribute block (rule 4's exception).
fn try_picture(text: &str, parser: &mut BodyParser<'_>) -> Result<Option<Shape>, ParseError> {
    let Some(mut image) = parser.try_parse_block_image(text)? else {
        return Ok(None);
    };
    // An image's attribute block attaches tightly, with no space before its
    // `{`: the shape's address and placement ride on the image's own block.
    let attrs = split_tight_attrs(text.trim())
        .1
        .map(|a| parser.expanded(a))
        .unwrap_or_default();
    let mut shape = Shape::new(0, ShapeKind::Picture(image.clone()));
    shape.id = image.id.take();
    shape.name = image.name.clone();
    shape.geometry = geometry(&attrs);
    // The size stays the image's own: it is written once, on the image line,
    // and the same measurement in two places is one that can disagree.
    shape.geometry.size = None;
    shape.kind = ShapeKind::Picture(image);
    Ok(Some(shape))
}

fn is_blockquote(text: &str) -> bool {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .all(|line| line.trim_start().starts_with('>'))
}

/// The content of a blockquote: one `> ` per line, and a bare `>` for the
/// blank lines inside it.
fn unquote(text: &str) -> String {
    text.lines()
        .map(|line| {
            let rest = line.trim_start().trim_start_matches('>');
            rest.strip_prefix(' ').unwrap_or(rest)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

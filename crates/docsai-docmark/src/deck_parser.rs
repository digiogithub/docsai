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
    ChartRef, ChartSeries, LayoutCatalog, LayoutId, PhType, Placeholder, Presentation, RawShape,
    RawShapeKind, Shape, ShapeGeometry, ShapeKind, SkeletonRef, Slide,
};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::text::Block;
use docsai_model::units::{Length, Point, Size};
use docsai_model::{Document, Format};

use crate::attrs::Attrs;
use crate::error::ParseError;
use crate::escape::unescape;
use crate::frontmatter_parse::FrontMatter;
use crate::parser::{
    apply_table_attrs, looks_like_table, split_one_fence, split_table_row, split_tight_attrs,
    split_top_level, split_trailing_attrs, BodyParser,
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
            if attrs.has_class("comment") {
                // A slide comment is pinned to the slide, not anchored over
                // text, so it never carries a `ref=` (spec §11.3 rule 2). One
                // that does is a text-document comment in a deck: kept on the
                // slide, and said.
                if attrs.get("ref").is_some() {
                    parser.report.warn(Warning::Degraded {
                        what: "slide comment".into(),
                        why: "it carries a `ref=`, which a slide has no anchor for".into(),
                    });
                }
                let comment = parser.parse_comment(&attrs, inner, chunk.line)?;
                slide.comments.push(comment);
                continue;
            }
            if attrs.has_class("notes") {
                // An empty container is an empty notes page, which is not the
                // same document as a slide with no notes page at all.
                add_notes(&mut slide, parser.parse_normal_blocks(inner, chunk.line)?);
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
            add_notes(
                &mut slide,
                parser.parse_normal_blocks(&unquote(trimmed), chunk.line)?,
            );
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
        "chart" => {
            // The body of a chart is what it *shows* and what it *plots*: a
            // title, and the table of spec §11.2. The delimiter row is what
            // separates them, so a hand-typed chart needs no marker and the
            // order of the two is not load-bearing.
            let (prose, table) = split_chart_table(body);
            let (categories, series) = match &table {
                Some(text) => parse_chart_table(text, line, parser),
                None => (Vec::new(), Vec::new()),
            };
            ShapeKind::Chart(ChartRef {
                kind: attrs.get("kind").map(str::to_string),
                title: label(&prose, parser, line)?,
                workbook: match attrs.get("data") {
                    Some(path) => parser.find_asset(path)?,
                    None => None,
                },
                categories,
                series,
                raw: attrs.get("raw").map(RawId::new),
            })
        }
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

/// Splits a `.chart` body into the prose it shows and the table it plots.
///
/// The table is found by its **delimiter row** — the `| --- | --- |` GFM
/// requires — and runs from the header line above it to the first line that is
/// not part of a table. Everything else, before or after, is prose. Nothing
/// here depends on the writer's own layout: a table a person typed, unpadded
/// and with no leading pipe, is found by the same rule.
fn split_chart_table(body: &str) -> (String, Option<String>) {
    let lines: Vec<&str> = body.lines().collect();
    let mut start = None;
    for (index, line) in lines.iter().enumerate() {
        if !is_delimiter_row(line) || index == 0 {
            continue;
        }
        // The header is the line above it, and a delimiter row with nothing
        // above it heads no table.
        if lines[index - 1].contains('|') {
            start = Some(index - 1);
            break;
        }
    }
    let Some(start) = start else {
        return (body.to_string(), None);
    };
    let mut end = start + 2;
    while end < lines.len() && lines[end].contains('|') {
        end += 1;
    }
    let table = lines[start..end].join("\n");
    let mut prose: Vec<&str> = lines[..start].to_vec();
    prose.extend_from_slice(&lines[end..]);
    (prose.join("\n"), Some(table))
}

fn is_delimiter_row(line: &str) -> bool {
    let line = line.trim();
    !line.is_empty()
        && line.contains('|')
        && line.contains('-')
        && line.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// The chart's table, read back into categories and series.
///
/// The mirror of [`crate::deck_writer`]'s writer, and the same four rules: the
/// first column is the categories, an empty cell is a **gap**, trailing empty
/// category cells are not categories — which is how a category-less chart
/// reads back — and `format=` on a series header is its `c:formatCode`.
///
/// A cell that is not a number is a gap **and a warning**: reading it as a zero
/// would draw a line through the floor where the author wrote a typo
/// (`AGENTS.md` §7 rule 3).
fn parse_chart_table(
    text: &str,
    line: usize,
    parser: &mut BodyParser<'_>,
) -> (Vec<String>, Vec<ChartSeries>) {
    let mut rows: Vec<Vec<String>> = Vec::new();
    for raw in text.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || !trimmed.contains('|') || is_delimiter_row(trimmed) {
            continue;
        }
        rows.push(split_table_row(trimmed));
    }
    if rows.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let header = rows.remove(0);
    let columns = rows
        .iter()
        .map(Vec::len)
        .chain(std::iter::once(header.len()))
        .max()
        .unwrap_or(1)
        .max(1);

    // The first header cell names the category axis, and the IR has nowhere to
    // put that. Our own writer leaves it empty, so this fires only on a table
    // somebody typed — which is when something is really being dropped.
    if let Some(axis) = header
        .first()
        .map(|cell| cell.trim())
        .filter(|c| !c.is_empty())
    {
        // `Degraded`, not `UnsupportedElement`: one word of an axis heading is
        // a Minor loss beside a value read as a gap, and 16-A says the same
        // thing about the chart losses on the way in.
        parser.report.warn(Warning::Degraded {
            what: format!("chart table at line {line}"),
            why: format!(
                "the category axis label `{axis}` is dropped: a chart's axis carries no title in the model"
            ),
        });
    }

    let mut categories: Vec<String> = rows
        .iter()
        .map(|row| unescape(row.first().map(String::as_str).unwrap_or_default()))
        .collect();
    while categories.last().is_some_and(|c| c.is_empty()) {
        categories.pop();
    }

    let mut series = Vec::with_capacity(columns - 1);
    for column in 1..columns {
        let (name, number_format) = match header.get(column) {
            Some(cell) => {
                let (text, attrs) = split_trailing_attrs(cell.trim());
                let name = unescape(text.trim());
                (
                    Some(name).filter(|n| !n.is_empty()),
                    attrs.and_then(|a| a.get("format").map(str::to_string)),
                )
            }
            None => (None, None),
        };
        let values = rows
            .iter()
            .map(|row| {
                let cell = row.get(column).map(|c| c.trim()).unwrap_or_default();
                if cell.is_empty() {
                    // A gap. This is the one place the format has to be read
                    // exactly as a spreadsheet reads a blank cell.
                    return None;
                }
                match cell.parse::<f64>() {
                    Ok(value) if value.is_finite() => Some(value),
                    _ => {
                        parser.report.warn(Warning::UnsupportedElement {
                            kind: format!("chart value `{cell}`"),
                            location: format!("line {line}"),
                            action: "read as a gap: not a number a chart can plot".into(),
                        });
                        None
                    }
                }
            })
            .collect();
        series.push(ChartSeries {
            name,
            values,
            number_format,
        });
    }
    (categories, series)
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

/// Adds blocks to the slide's notes page, keeping what is already there.
///
/// A slide has one notes page, and a reviewer who writes a second blockquote
/// under a slide that already had notes means «and also this» — so the two
/// become two paragraphs of one page. Replacing would drop text the reader can
/// see on screen without a word, which is the silent loss `AGENTS.md` §7 rule 3
/// forbids. Found by the P4 hand-edit gate (14-K), which is what the gate is
/// for.
fn add_notes(slide: &mut Slide, blocks: Vec<Block>) {
    match &mut slide.notes {
        Some(notes) => notes.extend(blocks),
        None => slide.notes = Some(blocks),
    }
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

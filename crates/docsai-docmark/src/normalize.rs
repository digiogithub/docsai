//! The DocMark **normal form** of an IR.
//!
//! Serialising is not injective: several IRs write the same DocMark. A run
//! whose direct formatting merely repeats its style is written without it (the
//! economy rule, spec §3.1); a run nested inside another with only emphasis
//! collapses into one; the heading depth is carried by the `#` count rather
//! than by `outline-level`.
//!
//! `normalize` applies exactly those collapses, which makes the statement the
//! round-trip actually guarantees precise and testable:
//!
//! * `normalize` is idempotent, and
//! * `serialize(normalize(x)) == serialize(x)` — normalising changes no byte of
//!   the output, and
//! * `parse(serialize(x)) == normalize(x)` — reading back lands on the normal
//!   form, and stays there for every further round-trip.
//!
//! Every rule below therefore mirrors one decision in `writer`, and the two are
//! meant to be edited together.

use docsai_model::style::{FontProps, StyleCatalog};
use docsai_model::text::{
    Block, HeaderFooter, Inline, List, Paragraph, RunProps, Section, Table, TableCell, TextDocument,
};
use docsai_model::Document;

/// Rewrites a document into its DocMark normal form.
pub fn normalize(document: &Document) -> Document {
    match document {
        Document::Text(text) => Document::Text(normalize_text(text)),
        // Spreadsheets have no DocMark form yet (Fase 3), so they have no
        // normal form either.
        other => other.clone(),
    }
}

fn normalize_text(document: &TextDocument) -> TextDocument {
    let styles = document.styles.clone();
    let mut sections: Vec<Section> = document
        .sections
        .iter()
        .map(|section| normalize_section(section, &styles))
        .collect();
    // A written document always describes one page geometry, so reading it
    // always yields at least one section. A document with none is degenerate,
    // and its normal form is a document with one empty section.
    if sections.is_empty() {
        sections.push(Section::default());
    }
    TextDocument {
        meta: document.meta.clone(),
        styles: styles.clone(),
        list_defs: document.list_defs.clone(),
        sections,
    }
}

fn normalize_section(section: &Section, styles: &StyleCatalog) -> Section {
    Section {
        page: section.page,
        headers: section
            .headers
            .iter()
            .map(|h| normalize_header(h, styles))
            .collect(),
        footers: section
            .footers
            .iter()
            .map(|f| normalize_header(f, styles))
            .collect(),
        blocks: normalize_blocks(&section.blocks, styles, 0),
    }
}

fn normalize_header(part: &HeaderFooter, styles: &StyleCatalog) -> HeaderFooter {
    HeaderFooter {
        scope: part.scope,
        blocks: normalize_blocks(&part.blocks, styles, 0),
    }
}

fn normalize_blocks(blocks: &[Block], styles: &StyleCatalog, depth: u8) -> Vec<Block> {
    blocks
        .iter()
        .filter_map(|block| normalize_block(block, styles, depth))
        .collect()
}

fn normalize_block(block: &Block, styles: &StyleCatalog, depth: u8) -> Option<Block> {
    Some(match block {
        Block::Paragraph(p) => {
            let paragraph = normalize_paragraph(p, styles);
            // A paragraph holding nothing but a picture writes exactly what a
            // block-level image writes, so that is what it reads back as.
            match paragraph.content.as_slice() {
                [Inline::Image(image)] if paragraph.format.is_empty() => {
                    Block::Image(image.clone())
                }
                _ => Block::Paragraph(paragraph),
            }
        }
        Block::Heading(h) => {
            let mut heading = h.clone();
            heading.level = h.level.clamp(1, 6);
            let mut paragraph = h.paragraph.clone();
            // An ATX heading is one line (`writer::flatten_to_one_line`).
            paragraph.content = flatten_to_one_line(&paragraph.content);
            heading.paragraph = normalize_paragraph(&paragraph, styles);
            // The `#` count carries the depth, so the writer omits the
            // attribute and reading cannot bring it back.
            heading.paragraph.format.direct.outline_level = None;
            Block::Heading(heading)
        }
        Block::List(list) => {
            let items: Vec<_> = list
                .items
                .iter()
                .map(|item| {
                    let mut blocks = normalize_blocks(&item.blocks, styles, depth + 1);
                    // An item opens with the line its marker sits on, and that
                    // line is a paragraph — it is where `list=` is written.
                    if let Some(Block::Image(image)) = blocks.first() {
                        blocks[0] =
                            Block::Paragraph(Paragraph::new(vec![Inline::Image(image.clone())]));
                    }
                    docsai_model::text::ListItem { blocks }
                })
                // An item with nothing in it leaves no marker behind.
                .filter(|item| !item.blocks.is_empty())
                .collect();
            if items.is_empty() {
                return None;
            }
            Block::List(List {
                def: list.def.clone(),
                ordered: list.ordered,
                // Nesting is what the Markdown indentation expresses; any other
                // value cannot survive a round-trip.
                level: depth,
                items,
            })
        }
        Block::Table(table) => Block::Table(normalize_table(table, styles)),
        Block::Image(image) => Block::Image(image.clone()),
        Block::TextBox(text_box) => {
            let mut normalized = text_box.clone();
            normalized.blocks = normalize_blocks(&text_box.blocks, styles, depth);
            Block::TextBox(normalized)
        }
        Block::Raw(raw) => {
            let mut normalized = raw.clone();
            // The fence is written with the content trimmed of trailing blank
            // lines, so they cannot come back.
            normalized.content = raw.content.trim_end().to_string();
            Block::Raw(normalized)
        }
    })
}

fn normalize_table(table: &Table, styles: &StyleCatalog) -> Table {
    let mut normalized = Table {
        style: table.style.clone(),
        col_widths: table.col_widths.clone(),
        rows: table.rows.clone(),
        header_row: table.header_row,
    };
    for (index, row) in normalized.rows.iter_mut().enumerate() {
        // Only the first row's header flag is written (as `header-row`).
        row.is_header = index == 0 && table.header_row;
        for cell in &mut row.cells {
            *cell = normalize_cell(cell, styles, table.is_complex());
        }
    }
    normalized
}

fn normalize_cell(cell: &TableCell, styles: &StyleCatalog, complex: bool) -> TableCell {
    let mut normalized = cell.clone();
    normalized.colspan = cell.colspan.max(1);
    normalized.rowspan = cell.rowspan.max(1);
    if !complex {
        // The writer flattens a GFM cell to one line *before* rendering it, so
        // the breaks are already spaces by the time anything else looks at them.
        for block in &mut normalized.blocks {
            if let Block::Paragraph(p) = block {
                p.content = flatten_to_one_line(&p.content);
            }
        }
    }
    normalized.blocks = normalize_blocks(&normalized.blocks, styles, 0);
    if complex {
        // The complex container writes no cell width or background.
        normalized.width = None;
        normalized.background = None;
    } else {
        // A GFM cell holds one line, so its paragraph keeps no formatting of
        // its own and a cell width has nowhere to be written. The rendered
        // line is trimmed at both ends before it is padded into the grid.
        normalized.width = None;
        for block in &mut normalized.blocks {
            if let Block::Paragraph(p) = block {
                p.format = Default::default();
                trim_edges(&mut p.content);
            }
        }
        // A GFM row has no block structure inside it, so a cell's content is
        // read back as one paragraph — a picture there stays inline, unlike a
        // picture standing alone between two paragraphs.
        for block in &mut normalized.blocks {
            if let Block::Image(image) = block {
                *block = Block::Paragraph(Paragraph::new(vec![Inline::Image(image.clone())]));
            }
        }
        // A cell whose paragraph says nothing writes an empty cell, and an
        // empty cell reads back as a cell with no blocks at all.
        normalized
            .blocks
            .retain(|block| !matches!(block, Block::Paragraph(p) if p.content.is_empty()));
    }
    normalized
}

fn normalize_paragraph(paragraph: &Paragraph, styles: &StyleCatalog) -> Paragraph {
    let mut format = paragraph.format.clone();
    // The economy rule: what the style already says is not written, so it
    // cannot be read back either.
    let resolved = styles.resolve(format.style.as_ref());
    format.direct = format.direct.minus(&resolved.paragraph);
    // Formatting of the paragraph mark has no DocMark representation (§3.1
    // covers the paragraph's own properties only). The serialiser warns.
    format.run_direct = FontProps::default();

    // Breaks go first, before anything turns one into a space: the writer
    // drops them on the raw content too, and a break dropped after being
    // flattened would leave the space behind.
    let mut content = paragraph.content.clone();
    drop_breaks_that_open_a_line(&mut content, true);
    let mut content = normalize_inlines(&content, styles);
    // A line of Markdown cannot end in spaces without becoming a hard break, so
    // the serialiser trims them and reading cannot bring them back.
    trim_trailing_spaces(&mut content);
    Paragraph { format, content }
}

/// Drops the spaces that would be eaten at the end of a rendered line, and the
/// hard break that has no line left to break.
fn trim_trailing_spaces(content: &mut Vec<Inline>) {
    while matches!(
        content.last(),
        Some(Inline::Break(docsai_model::text::BreakKind::Line))
    ) {
        content.pop();
    }
    if let Some(Inline::Text(text)) = content.last_mut() {
        let trimmed = text.trim_end_matches(' ');
        if trimmed.len() != text.len() {
            *text = trimmed.to_string();
        }
    }
    if matches!(content.last(), Some(Inline::Text(t)) if t.is_empty()) {
        content.pop();
    }
}

/// Drops the whitespace a table cell loses at both ends.
fn trim_edges(content: &mut Vec<Inline>) {
    if let Some(Inline::Text(text)) = content.first_mut() {
        let trimmed = text.trim_start();
        if trimmed.len() != text.len() {
            *text = trimmed.to_string();
        }
    }
    if matches!(content.first(), Some(Inline::Text(t)) if t.is_empty()) {
        content.remove(0);
    }
    if let Some(Inline::Text(text)) = content.last_mut() {
        let trimmed = text.trim_end();
        if trimmed.len() != text.len() {
            *text = trimmed.to_string();
        }
    }
    if matches!(content.last(), Some(Inline::Text(t)) if t.is_empty()) {
        content.pop();
    }
}

fn normalize_inlines(inlines: &[Inline], styles: &StyleCatalog) -> Vec<Inline> {
    normalize_inlines_between(inlines, styles, None, None)
}

/// As [`normalize_inlines`], told what sits either side of the whole run.
fn normalize_inlines_between(
    inlines: &[Inline],
    styles: &StyleCatalog,
    before: Option<char>,
    after: Option<char>,
) -> Vec<Inline> {
    // Merge first, exactly as the writer does, so that a run's neighbours are
    // the same characters on both sides of the round-trip.
    let inlines = crate::writer::merge_adjacent_text(inlines);
    let mut out: Vec<Inline> = Vec::new();
    for (index, inline) in inlines.iter().enumerate() {
        let (left, right) = neighbours(&inlines, index, before, after);
        for normalized in normalize_inline(inline, styles, left, right) {
            // Adjacent text is written as one run and reads back as one run.
            match (out.last_mut(), &normalized) {
                (Some(Inline::Text(previous)), Inline::Text(next)) => previous.push_str(next),
                _ => out.push(normalized),
            }
        }
    }
    out.retain(|inline| !matches!(inline, Inline::Text(t) if t.is_empty()));
    out
}

fn normalize_inline(
    inline: &Inline,
    styles: &StyleCatalog,
    before: Option<char>,
    after: Option<char>,
) -> Vec<Inline> {
    match inline {
        Inline::Text(text) => vec![Inline::Text(text.clone())],
        Inline::Styled { content, props } => {
            let mut props = normalize_props(props, styles);
            // An attribute block wraps the content in brackets, and markers
            // wrap it in punctuation; either way the content's neighbours stop
            // being the run's own. A run that writes nothing is transparent,
            // and its neighbours pass straight through.
            let (inner_before, inner_after) = if writes_attributes(&props) {
                (Some('['), Some(']'))
            } else {
                (before, after)
            };
            let content = collapse_emphasis(
                normalize_inlines_between(content, styles, inner_before, inner_after),
                &mut props,
            );
            // A child repeating this run's formatting adds nothing.
            let content = strip_inherited_emphasis(content, &props.direct);
            let content = normalize_inlines_between(&content, styles, inner_before, inner_after);
            drop_invisible_emphasis(&mut props, &content, inner_before, inner_after);
            if content.is_empty() {
                // A run with nothing in it writes nothing.
                return Vec::new();
            }
            // A run that says nothing is not a run: the writer emits its
            // content bare.
            if props.is_empty() {
                return content;
            }
            vec![Inline::Styled { content, props }]
        }
        Inline::Link {
            target,
            content,
            props,
        } => {
            let mut props = normalize_props(props, styles);
            // A link label is one line, like a heading or a cell.
            let content = normalize_inlines(&flatten_to_one_line(content), styles);
            // The link's own character style is not repeated inside its label.
            let content = strip_link_style(content, &props, styles);
            let content = collapse_emphasis(content, &mut props);
            // Inside the label the neighbours are the link's own brackets.
            drop_invisible_emphasis(&mut props, &content, Some('['), Some(']'));
            let _ = (before, after);
            vec![Inline::Link {
                target: target.clone(),
                content,
                props,
            }]
        }
        Inline::Footnote(blocks) => vec![Inline::Footnote(normalize_blocks(blocks, styles, 0))],
        Inline::Field {
            kind,
            cached,
            instruction,
        } => {
            // The instruction is only written when it says more than the kind.
            let instruction = if instruction.trim() == kind.as_str() {
                String::new()
            } else {
                instruction.trim().to_string()
            };
            vec![Inline::Field {
                kind: kind.clone(),
                cached: cached.clone(),
                instruction,
            }]
        }
        Inline::Break(kind) => vec![Inline::Break(*kind)],
        Inline::Image(image) => vec![Inline::Image(image.clone())],
        Inline::Raw(raw) => vec![Inline::Raw(raw.clone())],
    }
}

/// Mirrors the `Break(BreakKind::Line) if starts_line` arm of `writer`.
///
/// A hard break with nothing before it on the line would leave a whitespace-only
/// line, which Markdown reads as the end of the paragraph. Every other inline
/// opens with a visible character — a marker, a bracket or its own text — so
/// after one of them a break is safe; and inside a run or a link the opening
/// bracket or marker already fills the line.
pub(crate) fn drop_breaks_that_open_a_line(content: &mut Vec<Inline>, mut at_line_start: bool) {
    let mut index = 0usize;
    while index < content.len() {
        match &mut content[index] {
            Inline::Break(docsai_model::text::BreakKind::Line) => {
                if at_line_start {
                    content.remove(index);
                    continue;
                }
                at_line_start = true;
            }
            Inline::Styled { content: inner, .. } | Inline::Link { content: inner, .. } => {
                drop_breaks_that_open_a_line(inner, false);
                at_line_start = false;
            }
            _ => at_line_start = false,
        }
        index += 1;
    }
}

/// Mirrors `writer::flatten_to_one_line`: a hard break becomes a space and an
/// inline raw fragment cannot travel, in any construct that is one line long.
fn flatten_to_one_line(inlines: &[Inline]) -> Vec<Inline> {
    let mut out = Vec::new();
    for inline in inlines {
        match inline {
            Inline::Break(docsai_model::text::BreakKind::Line) => {
                out.push(Inline::Text(" ".into()))
            }
            Inline::Raw(_) => {}
            Inline::Styled { content, props } => out.push(Inline::Styled {
                content: flatten_to_one_line(content),
                props: props.clone(),
            }),
            Inline::Link {
                target,
                content,
                props,
            } => out.push(Inline::Link {
                target: target.clone(),
                content: flatten_to_one_line(content),
                props: props.clone(),
            }),
            other => out.push(other.clone()),
        }
    }
    out
}

/// The character a run of inlines starts with, once rendered.
///
/// Only the *classification* matters — whitespace, punctuation or neither — so
/// the constructs that open with a bracket or a marker can all report `[`: they
/// are punctuation either way. Escaping cannot change the classification
/// either, since only punctuation is ever escaped.
fn leading_char(inline: &Inline) -> Option<char> {
    match inline {
        Inline::Text(text) => text.chars().next().map(escaped_as),
        // A hard break renders as two spaces and a newline.
        Inline::Break(docsai_model::text::BreakKind::Line) => Some(' '),
        Inline::Raw(_) => Some('\n'),
        Inline::Image(_) => Some('!'),
        Inline::Styled { content, props } => match run_opening_char(props) {
            Some(c) => Some(c),
            None => content.first().and_then(leading_char),
        },
        _ => Some('['),
    }
}

/// What a character of text actually renders as: itself, or the backslash
/// that escapes it.
fn escaped_as(c: char) -> char {
    if crate::escape::is_always_escaped(c) {
        '\\'
    } else {
        c
    }
}

/// The character a run writes in front of its content, or `None` when it
/// writes nothing and is therefore transparent.
///
/// The exact character matters, not just its class: two emphasis runs whose
/// markers touch merge into one longer run, and `**a****b**` is a single bold
/// run containing `a****b`.
fn run_opening_char(props: &RunProps) -> Option<char> {
    if writes_attributes(props) {
        return Some('[');
    }
    marker_char(props)
}

/// The outermost marker a run draws: bold and italic use `*`, strike `~`, and
/// the order is bold outside italic outside strike.
pub(crate) fn marker_char(props: &RunProps) -> Option<char> {
    let font = &props.direct;
    if font.bold == Some(true) || font.italic == Some(true) {
        Some('*')
    } else if font.strike == Some(true) {
        Some('~')
    } else {
        None
    }
}

/// The character a run of inlines ends with, once rendered.
fn trailing_char(inline: &Inline) -> Option<char> {
    match inline {
        Inline::Text(text) => text.chars().next_back().map(escaped_as),
        Inline::Break(docsai_model::text::BreakKind::Line) => Some('\n'),
        Inline::Raw(_) => Some('\n'),
        Inline::Styled { content, props } => {
            if writes_attributes(props) {
                // `[…]{…}` ends in the attribute block's brace.
                Some('}')
            } else {
                match marker_char(props) {
                    Some(c) => Some(c),
                    None => content.last().and_then(trailing_char),
                }
            }
        }
        _ => Some('}'),
    }
}

/// True when emphasis markers around this content would not be emphasis.
///
/// CommonMark only honours a delimiter run that *flanks*: `a~~[x](u)~~` is five
/// literal characters, not strike-through, and `* a*` is not italic. The two
/// neighbours decide it, so they are passed in — within a paragraph they are
/// the siblings either side, and across any construct boundary they are
/// punctuation or nothing, which comes to the same answer.
///
/// Mirrors `writer::wrap_styled`; the two must agree exactly or the round-trip
/// does not close.
pub(crate) fn cannot_carry_emphasis(
    content: &[Inline],
    before: Option<char>,
    after: Option<char>,
    props: &RunProps,
) -> bool {
    let Some(delimiter) = marker_char(props) else {
        return true;
    };
    // A run can draw more than one marker, and the outermost one's neighbour
    // is then the marker inside it, not the content: `**~~x~~**` puts a `~`
    // right after the `**`.
    let inner_marker = (delimiter == '*' && props.direct.strike == Some(true)).then_some('~');
    let (Some(content_first), Some(content_last)) = (
        content.first().and_then(leading_char),
        content.last().and_then(trailing_char),
    ) else {
        return true;
    };
    // The innermost marker sits against the content itself, and no marker
    // flanks whitespace: `~~ ~~` is four characters of text.
    if content_first.is_whitespace() || content_last.is_whitespace() {
        return true;
    }
    let first = inner_marker.unwrap_or(content_first);
    let last = inner_marker.unwrap_or(content_last);
    // Markers that touch other markers of the same character merge into one
    // longer run, which means something else entirely.
    if [Some(first), Some(last), before, after].contains(&Some(delimiter)) {
        return true;
    }
    !crate::escape::left_flanking(before, Some(first))
        || !crate::escape::right_flanking(Some(last), after)
}

/// Drops the emphasis a run inherits from the run around it.
///
/// `bold` inside `bold` says nothing new, and writing it would put `**` next
/// to `**`. The parent already covers the child, so the child keeps only what
/// it adds.
pub(crate) fn strip_inherited_emphasis(content: Vec<Inline>, parent: &FontProps) -> Vec<Inline> {
    content
        .into_iter()
        .map(|inline| match inline {
            Inline::Styled { content, mut props } => {
                for (child, parent) in [
                    (&mut props.direct.bold, parent.bold),
                    (&mut props.direct.italic, parent.italic),
                    (&mut props.direct.strike, parent.strike),
                ] {
                    if parent == Some(true) && *child == Some(true) {
                        *child = None;
                    }
                }
                if props.is_empty() {
                    // Nothing left to say: the run is its content.
                    return Inline::Styled {
                        content,
                        props: RunProps::default(),
                    };
                }
                Inline::Styled { content, props }
            }
            other => other,
        })
        .collect()
}

/// True when a run writes an attribute block, which wraps its content in
/// brackets.
///
/// Mirrors `writer::style_attrs`: everything it can write, except the three
/// emphasis flags when they are *on* — those become markers instead.
pub(crate) fn writes_attributes(props: &RunProps) -> bool {
    if props.style.is_some() {
        return true;
    }
    let font = &props.direct;
    let markers_only = FontProps {
        bold: font.bold.filter(|b| !b),
        italic: font.italic.filter(|i| !i),
        strike: None,
        ..font.clone()
    };
    !markers_only.is_empty()
}

/// The neighbours of the inline at `index` within its own run.
pub(crate) fn neighbours(
    inlines: &[Inline],
    index: usize,
    before: Option<char>,
    after: Option<char>,
) -> (Option<char>, Option<char>) {
    let left = match index.checked_sub(1) {
        Some(previous) => inlines.get(previous).and_then(trailing_char),
        None => before,
    };
    let right = match inlines.get(index + 1) {
        Some(next) => leading_char(next),
        None => after,
    };
    (left, right)
}

/// Drops emphasis that has nothing to show for itself.
fn drop_invisible_emphasis(
    props: &mut RunProps,
    content: &[Inline],
    before: Option<char>,
    after: Option<char>,
) {
    if cannot_carry_emphasis(content, before, after, props) {
        props.direct.bold = props.direct.bold.filter(|b| !b);
        props.direct.italic = props.direct.italic.filter(|i| !i);
        props.direct.strike = props.direct.strike.filter(|s| !s);
    }
}

fn normalize_props(props: &RunProps, styles: &StyleCatalog) -> RunProps {
    let resolved = styles.resolve(props.style.as_ref());
    let mut direct = props.direct.minus(&resolved.font);
    // DocMark 1.0 can switch off bold and italic (spec §3.2) and nothing else.
    // The serialiser reports each of these as a degradation; here they simply
    // cannot come back.
    if direct.strike == Some(false) {
        direct.strike = None;
    }
    if direct.underline == Some(docsai_model::style::Underline::None) {
        direct.underline = None;
    }
    if direct.small_caps == Some(false) {
        direct.small_caps = None;
    }
    if direct.caps == Some(false) {
        direct.caps = None;
    }
    if direct.vert_align == Some(docsai_model::style::VertAlign::Baseline) {
        direct.vert_align = None;
    }
    RunProps {
        style: props.style.clone(),
        direct,
    }
}

/// Folds a lone nested run that carries nothing but emphasis into its parent.
///
/// `writer::wrap_styled` draws `**`, `*` and `~~` from the same properties that
/// produce the surrounding span, so two nested runs write as one and can only
/// read back as one.
pub(crate) fn collapse_emphasis(content: Vec<Inline>, props: &mut RunProps) -> Vec<Inline> {
    let [Inline::Styled {
        content: inner,
        props: inner_props,
    }] = content.as_slice()
    else {
        return content;
    };
    if inner_props.style.is_some() || !is_emphasis_only(&inner_props.direct) {
        return content;
    }
    // Only when the outer run has nothing to say about the same flag. A
    // `bold=false` around a `**bold**` is a contradiction the source really
    // holds, and folding the two would throw one of them away.
    let pairs = [
        (props.direct.bold, inner_props.direct.bold),
        (props.direct.italic, inner_props.direct.italic),
        (props.direct.strike, inner_props.direct.strike),
    ];
    if pairs
        .iter()
        .any(|(outer, inner)| inner.is_some() && outer.is_some())
    {
        return content;
    }
    for (outer, inner) in [
        (&mut props.direct.bold, inner_props.direct.bold),
        (&mut props.direct.italic, inner_props.direct.italic),
        (&mut props.direct.strike, inner_props.direct.strike),
    ] {
        if outer.is_none() {
            *outer = inner;
        }
    }
    let inner = inner.clone();
    collapse_emphasis(inner, props)
}

fn is_emphasis_only(font: &FontProps) -> bool {
    let only_markers = FontProps {
        bold: None,
        italic: None,
        strike: None,
        ..font.clone()
    }
    .is_empty();
    only_markers
        && [font.bold, font.italic, font.strike]
            .iter()
            .all(|f| *f != Some(false))
}

/// Mirrors `writer::strip_redundant_style`.
fn strip_link_style(content: Vec<Inline>, props: &RunProps, styles: &StyleCatalog) -> Vec<Inline> {
    let Some(style) = &props.style else {
        return content;
    };
    let mut out = Vec::new();
    for inline in content {
        match inline {
            Inline::Styled {
                content: inner,
                props: inner_props,
            } if inner_props.style.as_ref() == Some(style) => {
                let stripped = normalize_props(
                    &RunProps {
                        style: None,
                        direct: inner_props.direct,
                    },
                    styles,
                );
                if stripped.is_empty() {
                    out.extend(inner);
                } else {
                    out.push(Inline::Styled {
                        content: inner,
                        props: stripped,
                    });
                }
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::style::{Style, StyleId, StyleType};
    use docsai_model::text::{Heading, ListItem, ParaFormat, Section, TableRow};

    fn catalog() -> StyleCatalog {
        let mut styles = StyleCatalog::default();
        let mut enfatico = Style::new("Enfatico", StyleType::Character);
        enfatico.font.italic = Some(true);
        styles.insert(enfatico);
        let mut hyperlink = Style::new("Hyperlink", StyleType::Character);
        hyperlink.font.color = Some("#0563C1".into());
        styles.insert(hyperlink);
        styles
    }

    fn document(blocks: Vec<Block>) -> Document {
        Document::Text(TextDocument {
            styles: catalog(),
            sections: vec![Section {
                blocks,
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    fn normalized_blocks(blocks: Vec<Block>) -> Vec<Block> {
        let Document::Text(text) = normalize(&document(blocks)) else {
            panic!("expected a text document");
        };
        text.sections.into_iter().next().unwrap().blocks
    }

    #[test]
    fn nested_emphasis_runs_collapse_into_one() {
        let inner = Inline::Styled {
            content: vec![Inline::Text("x".into())],
            props: RunProps::direct(FontProps {
                bold: Some(true),
                ..Default::default()
            }),
        };
        let outer = Inline::Styled {
            content: vec![inner],
            props: RunProps {
                style: Some(StyleId::new("Enfatico")),
                direct: FontProps::default(),
            },
        };
        let blocks = normalized_blocks(vec![Block::Paragraph(Paragraph::new(vec![outer]))]);
        let [Block::Paragraph(p)] = blocks.as_slice() else {
            panic!("expected a paragraph");
        };
        let [Inline::Styled { props, content }] = p.content.as_slice() else {
            panic!("expected exactly one run, got {:?}", p.content);
        };
        assert_eq!(props.style.as_ref().unwrap().as_str(), "Enfatico");
        assert_eq!(props.direct.bold, Some(true));
        assert_eq!(content, &vec![Inline::Text("x".into())]);
    }

    #[test]
    fn formatting_the_style_already_gives_is_dropped() {
        let run = Inline::Styled {
            content: vec![Inline::Text("x".into())],
            props: RunProps {
                style: Some(StyleId::new("Enfatico")),
                direct: FontProps {
                    italic: Some(true), // exactly what Enfatico says
                    bold: Some(true),   // something new
                    ..Default::default()
                },
            },
        };
        let blocks = normalized_blocks(vec![Block::Paragraph(Paragraph::new(vec![run]))]);
        let [Block::Paragraph(p)] = blocks.as_slice() else {
            panic!("expected a paragraph");
        };
        let [Inline::Styled { props, .. }] = p.content.as_slice() else {
            panic!("expected one run");
        };
        assert_eq!(props.direct.italic, None, "already implied by the style");
        assert_eq!(props.direct.bold, Some(true));
    }

    #[test]
    fn a_run_that_says_nothing_disappears() {
        let run = Inline::Styled {
            content: vec![Inline::Text("x".into())],
            props: RunProps::default(),
        };
        let blocks = normalized_blocks(vec![Block::Paragraph(Paragraph::new(vec![run]))]);
        let [Block::Paragraph(p)] = blocks.as_slice() else {
            panic!("expected a paragraph");
        };
        assert_eq!(p.content, vec![Inline::Text("x".into())]);
    }

    #[test]
    fn adjacent_text_runs_merge() {
        let blocks = normalized_blocks(vec![Block::Paragraph(Paragraph::new(vec![
            Inline::Text("uno ".into()),
            Inline::Text("dos".into()),
            Inline::Text(String::new()),
        ]))]);
        let [Block::Paragraph(p)] = blocks.as_slice() else {
            panic!("expected a paragraph");
        };
        assert_eq!(p.content, vec![Inline::Text("uno dos".into())]);
    }

    #[test]
    fn a_links_own_style_is_not_repeated_inside_its_label() {
        let link = Inline::Link {
            target: "https://example.com".into(),
            content: vec![Inline::Styled {
                content: vec![Inline::Text("sitio".into())],
                props: RunProps {
                    style: Some(StyleId::new("Hyperlink")),
                    direct: FontProps::default(),
                },
            }],
            props: RunProps {
                style: Some(StyleId::new("Hyperlink")),
                direct: FontProps::default(),
            },
        };
        let blocks = normalized_blocks(vec![Block::Paragraph(Paragraph::new(vec![link]))]);
        let [Block::Paragraph(p)] = blocks.as_slice() else {
            panic!("expected a paragraph");
        };
        let [Inline::Link { content, .. }] = p.content.as_slice() else {
            panic!("expected a link");
        };
        assert_eq!(content, &vec![Inline::Text("sitio".into())]);
    }

    #[test]
    fn a_headings_outline_level_lives_in_its_hash_count() {
        let blocks = normalized_blocks(vec![Block::Heading(Heading {
            level: 9,
            paragraph: Paragraph {
                format: ParaFormat {
                    direct: docsai_model::style::ParaProps {
                        outline_level: Some(3),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                content: vec![Inline::Text("T".into())],
            },
        })]);
        let [Block::Heading(h)] = blocks.as_slice() else {
            panic!("expected a heading");
        };
        assert_eq!(h.level, 6, "clamped to what Markdown can write");
        assert_eq!(h.paragraph.format.direct.outline_level, None);
    }

    #[test]
    fn list_levels_follow_the_nesting() {
        let inner = Block::List(List {
            def: None,
            ordered: true,
            level: 7,
            items: vec![ListItem {
                blocks: vec![Block::Paragraph(Paragraph::text("b"))],
            }],
        });
        let outer = Block::List(List {
            def: None,
            ordered: true,
            level: 4,
            items: vec![ListItem {
                blocks: vec![Block::Paragraph(Paragraph::text("a")), inner],
            }],
        });
        let blocks = normalized_blocks(vec![outer]);
        let [Block::List(list)] = blocks.as_slice() else {
            panic!("expected a list");
        };
        assert_eq!(list.level, 0);
        let [_, Block::List(nested)] = list.items[0].blocks.as_slice() else {
            panic!("expected a nested list");
        };
        assert_eq!(nested.level, 1);
    }

    #[test]
    fn normalising_twice_changes_nothing_more() {
        let document = document(vec![
            Block::Paragraph(Paragraph::new(vec![
                Inline::Text("a".into()),
                Inline::Styled {
                    content: vec![Inline::Styled {
                        content: vec![Inline::Text("b".into())],
                        props: RunProps::direct(FontProps {
                            italic: Some(true),
                            ..Default::default()
                        }),
                    }],
                    props: RunProps::direct(FontProps {
                        bold: Some(true),
                        ..Default::default()
                    }),
                },
            ])),
            Block::Table(Table {
                rows: vec![TableRow {
                    cells: vec![TableCell::text("x")],
                    is_header: true,
                }],
                header_row: true,
                ..Default::default()
            }),
        ]);
        let once = normalize(&document);
        let twice = normalize(&once);
        assert_eq!(once, twice, "normalisation must be a fixed point");
    }
}

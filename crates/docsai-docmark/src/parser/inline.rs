//! DocMark inline syntax → IR (spec §3.2, §3.5).
//!
//! The exact inverse of `writer::render_inline`. Two shapes of the writer's
//! output drive the whole design:
//!
//! * Emphasis markers are drawn from the *same* `RunProps` that produces the
//!   surrounding span, so `[**negrita**]{.Enfatico}` is **one** run, not a run
//!   inside a run. Reading therefore pulls the markers back *into* the span's
//!   properties ([`absorb_emphasis`]).
//! * A span's attribute block never has a space before its `{`, while a
//!   paragraph's always does. That is what lets a paragraph end in a span
//!   without its attributes being mistaken for the paragraph's own.

use std::collections::BTreeMap;

use docsai_model::assets::AssetId;
use docsai_model::image::{
    Anchor, AxisPos, CellAnchor, CropRect, Flip, HVPos, ImageGeometry, ImageRef, RawId, RelBase,
    SimpleBorder, WrapMode, WrapSide,
};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::sheet::CellRef;
use docsai_model::style::{FontProps, Underline, VertAlign};
use docsai_model::text::{Block, BreakKind, FieldKind, Inline, RunProps};
use docsai_model::units::{Length, Point, Size};

use crate::attrs::Attrs;
use crate::escape::{left_flanking, right_flanking, unescape};
use crate::units::{parse_len, parse_number, parse_percent};

/// Classes that mean something to the reader rather than naming a style.
const RESERVED_CLASSES: &[&str] = &[
    "underline",
    "sup",
    "sub",
    "small-caps",
    "caps",
    "field",
    "break",
    "empty",
];

/// Shared state for one document's inline parsing.
pub struct Inliner<'a> {
    /// Asset ids by file name, so `assets/img-1a2b.png` finds its bytes.
    pub assets: &'a BTreeMap<String, AssetId>,
    /// Footnote definitions collected from the end of the document.
    pub footnotes: &'a BTreeMap<String, Vec<Block>>,
    pub report: &'a mut ConversionReport,
}

impl Inliner<'_> {
    /// Parses a run of inline content.
    ///
    /// Two passes, as CommonMark itself does it: everything that is *not* an
    /// emphasis delimiter is resolved first, then the delimiter runs are paired
    /// with a stack. One pass cannot do it — `*a**a***` is an italic containing
    /// a bold, and which run opens what is only decided once the whole line is
    /// in view.
    pub fn parse(&mut self, text: &str) -> Vec<Inline> {
        let tokens = self.tokenise(text);
        self.resolve_emphasis(tokens)
    }

    /// First pass: text, escapes, brackets and breaks become nodes; runs of
    /// `*` and `~` are left for the second pass, with their flanking recorded.
    fn tokenise(&mut self, text: &str) -> Vec<Token> {
        let mut out: Vec<Token> = Vec::new();
        let mut buf = String::new();
        let mut cursor = 0usize;

        while cursor < text.len() {
            let rest = &text[cursor..];

            // A hard break: two spaces at the end of a line (spec §3.2).
            if let Some(after) = rest.strip_prefix("  \n") {
                push_text(&mut out, &mut buf);
                out.push(Token::node(Inline::Break(BreakKind::Line)));
                cursor += rest.len() - after.len();
                continue;
            }

            let c = rest.chars().next().unwrap_or('\0');
            match c {
                '\\' => {
                    // The escape marker itself never reaches the IR.
                    let escaped = rest[1..].chars().next();
                    match escaped {
                        Some(escaped) => {
                            buf.push(escaped);
                            cursor += 1 + escaped.len_utf8();
                        }
                        None => {
                            buf.push('\\');
                            cursor += 1;
                        }
                    }
                }
                '!' if rest[1..].starts_with('[') => match self.image(rest) {
                    Some((image, used)) => {
                        push_text(&mut out, &mut buf);
                        out.push(Token::node(image));
                        cursor += used;
                    }
                    None => {
                        buf.push('!');
                        cursor += 1;
                    }
                },
                '[' => match self.bracketed(rest) {
                    Some((inlines, used)) => {
                        push_text(&mut out, &mut buf);
                        out.extend(inlines.into_iter().map(Token::node));
                        cursor += used;
                    }
                    None => {
                        buf.push('[');
                        cursor += 1;
                    }
                },
                '*' | '~' => {
                    let len = rest.len() - rest.trim_start_matches(c).len();
                    // Flanking is judged against the source, so the character
                    // before the run is the one in `text` — an escaped `\*`
                    // counts as the punctuation it is.
                    let before = text[..cursor].chars().next_back();
                    let after = rest[len..].chars().next();
                    push_text(&mut out, &mut buf);
                    out.push(Token::Run {
                        ch: c,
                        len,
                        can_open: left_flanking(before, after),
                        can_close: right_flanking(before, after),
                    });
                    cursor += len;
                }
                _ => {
                    buf.push(c);
                    cursor += c.len_utf8();
                }
            }
        }
        push_text(&mut out, &mut buf);
        out
    }

    /// Second pass: pairs the delimiter runs, innermost first.
    ///
    /// This is CommonMark's own algorithm, cut down to the two delimiters
    /// DocMark uses. The "rule of three" is what keeps `*a**a***` from being
    /// read as an italic `a` followed by loose asterisks: a run that can both
    /// open and close cannot pair when the two lengths sum to a multiple of
    /// three, unless both are themselves multiples of three.
    fn resolve_emphasis(&mut self, tokens: Vec<Token>) -> Vec<Inline> {
        let mut out: Vec<Token> = Vec::new();
        for token in tokens {
            let Token::Run {
                ch,
                mut len,
                can_open,
                can_close,
            } = token
            else {
                out.push(token);
                continue;
            };

            while can_close && len > 0 {
                let Some(position) = out.iter().rposition(|t| {
                    matches!(t, Token::Run { ch: c, len: l, can_open: true, .. } if *c == ch && *l > 0)
                }) else {
                    break;
                };
                let Token::Run {
                    len: open_len,
                    can_close: open_can_close,
                    ..
                } = out[position]
                else {
                    break;
                };
                if (open_can_close || can_open)
                    && (open_len + len) % 3 == 0
                    && !(open_len % 3 == 0 && len % 3 == 0)
                {
                    break;
                }
                // Strike-through is exactly two; emphasis takes two when both
                // sides can spare them, one otherwise.
                let take = match ch {
                    '~' if open_len >= 2 && len >= 2 => 2,
                    '~' => break,
                    _ if open_len >= 2 && len >= 2 => 2,
                    _ => 1,
                };

                let inner = out.split_off(position + 1);
                let content = self.resolve_emphasis(inner);
                if content.is_empty() {
                    // Nothing between the delimiters: they are literal text.
                    out.push(Token::Run {
                        ch,
                        len,
                        can_open,
                        can_close,
                    });
                    len = 0;
                    break;
                }
                let mut props = RunProps::default();
                match (ch, take) {
                    ('~', _) => props.direct.strike = Some(true),
                    (_, 2) => props.direct.bold = Some(true),
                    _ => props.direct.italic = Some(true),
                }
                let content = absorb_emphasis(content, &mut props);

                if let Token::Run { len: open_len, .. } = &mut out[position] {
                    *open_len -= take;
                    if *open_len == 0 {
                        out.remove(position);
                    }
                }
                out.push(Token::node(Inline::Styled { content, props }));
                len -= take;
            }

            if len > 0 {
                out.push(Token::Run {
                    ch,
                    len,
                    can_open,
                    can_close,
                });
            }
        }
        flatten(out)
    }

    /// Anything opening with `[`: a footnote reference, a link, or a span.
    ///
    /// Returns a list because `[]{.empty}` contributes nothing at all.
    fn bracketed(&mut self, text: &str) -> Option<(Vec<Inline>, usize)> {
        let close = find_close_bracket(text)?;
        let label = &text[1..close];
        let after = &text[close + 1..];

        // `[^1]`: a reference to a definition collected at the end.
        if let Some(name) = label.strip_prefix('^') {
            if !after.starts_with('(') {
                let blocks = self.footnotes.get(name).cloned().unwrap_or_else(|| {
                    self.report.warn(Warning::Degraded {
                        what: format!("footnote [^{name}]"),
                        why: "no definition found in the document".into(),
                    });
                    Vec::new()
                });
                return Some((vec![Inline::Footnote(blocks)], close + 1));
            }
        }

        if after.starts_with('(') {
            let (target, used) = read_destination(after)?;
            let mut consumed = close + 1 + used;
            let mut props = RunProps::default();
            if let Some((attrs, attr_len)) = read_attr_block(&text[consumed..]) {
                props = self.run_props(attrs);
                consumed += attr_len;
            }
            let content = absorb_emphasis(self.parse(label), &mut props);
            return Some((
                vec![Inline::Link {
                    target,
                    content,
                    props,
                }],
                consumed,
            ));
        }

        let (attrs, attr_len) = read_attr_block(after)?;
        let consumed = close + 1 + attr_len;
        Some((self.span(label, attrs), consumed))
    }

    /// A `[texto]{…}` span: a field, a break, an empty marker or a styled run.
    fn span(&mut self, label: &str, mut attrs: Attrs) -> Vec<Inline> {
        if attrs.take_class("empty") {
            // The marker for a paragraph with no content; it carries nothing
            // of its own.
            return Vec::new();
        }
        if attrs.take_class("break") {
            let kind = match attrs.get("kind") {
                Some("page") => BreakKind::Page,
                Some("column") => BreakKind::Column,
                _ => BreakKind::Line,
            };
            return vec![Inline::Break(kind)];
        }
        if attrs.take_class("field") {
            let instruction = attrs.take("instr").unwrap_or_default();
            let name = attrs.get("field").unwrap_or_default().to_string();
            // `instr` is only written when it says more than the name does.
            let kind = FieldKind::from_instruction(&name);
            return vec![Inline::Field {
                kind,
                cached: unescape(label),
                instruction,
            }];
        }

        let mut props = self.run_props(attrs);
        let content = absorb_emphasis(self.parse(label), &mut props);
        if content.is_empty() {
            return Vec::new();
        }
        Inline::styled(content, props)
    }

    /// Rebuilds a run's properties from its attribute block, undoing
    /// `writer::style_attrs`.
    fn run_props(&mut self, mut attrs: Attrs) -> RunProps {
        let mut font = FontProps {
            color: attrs.take("color"),
            highlight: attrs.take("highlight"),
            name: attrs.take("font"),
            size: attrs.take("size").as_deref().and_then(parse_len),
            ..Default::default()
        };
        if attrs.take_class("underline") {
            font.underline = Some(
                attrs
                    .take("underline")
                    .as_deref()
                    .and_then(super::front::parse_underline)
                    .unwrap_or(Underline::Single),
            );
        }
        if attrs.take_class("sup") {
            font.vert_align = Some(VertAlign::Superscript);
        }
        if attrs.take_class("sub") {
            font.vert_align = Some(VertAlign::Subscript);
        }
        if attrs.take_class("small-caps") {
            font.small_caps = Some(true);
        }
        if attrs.take_class("caps") {
            font.caps = Some(true);
        }
        // Only `false` is ever written: `true` travels as `**`/`*` markers.
        if attrs.flag("bold") == Some(false) {
            font.bold = Some(false);
            attrs.take("bold");
        }
        if attrs.flag("italic") == Some(false) {
            font.italic = Some(false);
            attrs.take("italic");
        }

        let style = first_style_class(&attrs).map(docsai_model::style::StyleId::new);
        RunProps {
            style,
            direct: font,
        }
    }

    /// `![alt](assets/img-1a2b.png){…}`.
    fn image(&mut self, text: &str) -> Option<(Inline, usize)> {
        let (image, used) = self.image_ref(text)?;
        Some((Inline::Image(image), used))
    }

    /// Reads an image and its geometry; shared with the block-level form.
    pub fn image_ref(&mut self, text: &str) -> Option<(ImageRef, usize)> {
        let rest = text.strip_prefix('!')?;
        let close = find_close_bracket(rest)?;
        let alt = unescape(&rest[1..close]);
        let (path, used) = read_destination(&rest[close + 1..])?;
        let mut consumed = 1 + close + 1 + used;

        let mut attrs = match read_attr_block(&text[consumed..]) {
            Some((attrs, len)) => {
                consumed += len;
                attrs
            }
            None => Attrs::new(),
        };

        let file_name = path.rsplit('/').next().unwrap_or(&path);
        let asset = match self.assets.get(file_name) {
            Some(id) => id.clone(),
            None => {
                // Not fatal: the geometry and the link survive, and the writer
                // will say so again when it cannot pack the bytes.
                self.report.warn(Warning::AssetIssue {
                    asset: path.clone(),
                    why: "referenced by the document but not present in the asset store".into(),
                });
                AssetId::new(file_name)
            }
        };

        let mut image = ImageRef::new(asset, self.geometry(&mut attrs));
        image.alt = alt;
        image.name = attrs.take("name");
        image.title = attrs.take("title");
        image.link = attrs.take("link");
        image.external_src = attrs.take("external-src");
        image.effects_raw = attrs.take("effects-raw").map(RawId::new);
        // `render=unsupported` is a hint the serialiser re-derives from the
        // content type; it is not part of the IR.
        attrs.take("render");
        Some((image, consumed))
    }

    fn geometry(&mut self, attrs: &mut Attrs) -> ImageGeometry {
        let width = attrs.take("width").as_deref().and_then(parse_len);
        let height = attrs.take("height").as_deref().and_then(parse_len);
        ImageGeometry {
            display_size: Size::new(width.unwrap_or_default(), height.unwrap_or_default()),
            native_size_px: attrs.take("native-size").as_deref().and_then(parse_native),
            dpi: attrs.take("dpi").and_then(|d| d.parse().ok()),
            anchor: self.anchor(attrs),
            rotation_deg: attrs
                .take("rotation")
                .as_deref()
                .and_then(parse_number)
                .unwrap_or(0.0),
            flip: match attrs.take("flip").as_deref() {
                Some("h") => Flip::H,
                Some("v") => Flip::V,
                Some("hv") => Flip::HV,
                _ => Flip::None,
            },
            crop: attrs.take("crop").as_deref().and_then(parse_crop),
            border: attrs.take("border").as_deref().and_then(parse_border),
            z_index: attrs.take("z-index").and_then(|z| z.parse().ok()),
        }
    }

    fn anchor(&mut self, attrs: &mut Attrs) -> Anchor {
        let kind = attrs.take("anchor").unwrap_or_else(|| "inline".into());
        match kind.as_str() {
            "floating" | "behind" | "front" => {
                let relative_to_h = attrs
                    .take("relative-to")
                    .as_deref()
                    .and_then(parse_rel_base)
                    .unwrap_or(RelBase::Margin);
                let relative_to_v = attrs
                    .take("relative-to-v")
                    .as_deref()
                    .and_then(parse_rel_base)
                    .unwrap_or(relative_to_h);
                Anchor::Floating {
                    relative_to_h,
                    relative_to_v,
                    position: HVPos {
                        h: axis(attrs, "x", "align-h"),
                        v: axis(attrs, "y", "align-v"),
                    },
                    wrap: attrs
                        .take("wrap")
                        .as_deref()
                        .and_then(parse_wrap)
                        .unwrap_or(WrapMode::Square),
                    wrap_side: attrs
                        .take("wrap-side")
                        .as_deref()
                        .and_then(parse_wrap_side)
                        .unwrap_or(WrapSide::Both),
                    behind_text: kind == "behind",
                }
            }
            "two-cell" => Anchor::SheetTwoCell {
                from: cell_anchor(attrs, "from", "from-offset"),
                to: cell_anchor(attrs, "to", "to-offset"),
                move_with_cells: attrs.flag("move-with-cells").unwrap_or(true),
                size_with_cells: attrs.flag("size-with-cells").unwrap_or(false),
            },
            "one-cell" => Anchor::SheetOneCell {
                from: cell_anchor(attrs, "from", "from-offset"),
            },
            "absolute" => Anchor::SheetAbsolute {
                pos: Point::new(
                    attrs
                        .take("x")
                        .as_deref()
                        .and_then(parse_len)
                        .unwrap_or_default(),
                    attrs
                        .take("y")
                        .as_deref()
                        .and_then(parse_len)
                        .unwrap_or_default(),
                ),
            },
            "inline" => Anchor::Inline,
            other => {
                self.report.warn(Warning::ImageGeometryDegraded {
                    what: format!("anchor={other}"),
                    why: "not a DocMark 1.0 anchor; read as inline".into(),
                });
                Anchor::Inline
            }
        }
    }
}

/// Pulls a nested pure-emphasis run up into `props`.
///
/// The writer draws `**`/`*`/`~~` from the same properties as the span around
/// them, so `[**x**]{.Enfatico}` must come back as one run carrying both the
/// style and the bold — not as a bold run inside a styled run.
fn absorb_emphasis(content: Vec<Inline>, props: &mut RunProps) -> Vec<Inline> {
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
    absorb_emphasis(inner, props)
}

/// True when a run says nothing beyond the three markers Markdown can draw.
///
/// `Some(false)` does not count: formatting *switched off* over a style is
/// written as an attribute, not as a marker, so the writer keeps such a run
/// nested and merging it would change the output.
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

/// The first class that is not one of the reader's own keywords: the style id.
fn first_style_class(attrs: &Attrs) -> Option<String> {
    attrs
        .classes()
        .iter()
        .find(|c| !RESERVED_CLASSES.contains(&c.as_str()))
        .cloned()
}

fn axis(attrs: &mut Attrs, offset_key: &str, align_key: &str) -> AxisPos {
    if let Some(keyword) = attrs
        .take(align_key)
        .as_deref()
        .and_then(parse_align_keyword)
    {
        return AxisPos::Align(keyword);
    }
    AxisPos::Offset(
        attrs
            .take(offset_key)
            .as_deref()
            .and_then(parse_len)
            .unwrap_or_default(),
    )
}

fn cell_anchor(attrs: &mut Attrs, cell_key: &str, offset_key: &str) -> CellAnchor {
    let cell = attrs
        .take(cell_key)
        .as_deref()
        .and_then(CellRef::parse_a1)
        .unwrap_or(CellRef::new(0, 0));
    let (x, y) = match attrs.take(offset_key) {
        Some(pair) => {
            let mut parts = pair.split(',');
            (
                parts.next().and_then(parse_len).unwrap_or_default(),
                parts.next().and_then(parse_len).unwrap_or_default(),
            )
        }
        None => (Length::ZERO, Length::ZERO),
    };
    CellAnchor::new(cell, x, y)
}

fn parse_rel_base(value: &str) -> Option<RelBase> {
    Some(match value {
        "page" => RelBase::Page,
        "margin" => RelBase::Margin,
        "paragraph" => RelBase::Paragraph,
        "character" => RelBase::Character,
        "line" => RelBase::Line,
        "column" => RelBase::Column,
        _ => return None,
    })
}

fn parse_wrap(value: &str) -> Option<WrapMode> {
    Some(match value {
        "square" => WrapMode::Square,
        "tight" => WrapMode::Tight,
        "through" => WrapMode::Through,
        "top-bottom" => WrapMode::TopBottom,
        "none" => WrapMode::None,
        _ => return None,
    })
}

fn parse_wrap_side(value: &str) -> Option<WrapSide> {
    Some(match value {
        "both" => WrapSide::Both,
        "left" => WrapSide::Left,
        "right" => WrapSide::Right,
        "largest" => WrapSide::Largest,
        _ => return None,
    })
}

fn parse_align_keyword(value: &str) -> Option<docsai_model::image::AlignKeyword> {
    use docsai_model::image::AlignKeyword::*;
    Some(match value {
        "left" => Left,
        "center" => Center,
        "right" => Right,
        "inside" => Inside,
        "outside" => Outside,
        "top" => Top,
        "middle" => Middle,
        "bottom" => Bottom,
        _ => return None,
    })
}

/// `120x90`.
fn parse_native(value: &str) -> Option<(u32, u32)> {
    let (w, h) = value.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// `10%,5%,20%,0%`.
fn parse_crop(value: &str) -> Option<CropRect> {
    let parts: Vec<&str> = value.split(',').collect();
    if parts.len() != 4 {
        return None;
    }
    Some(CropRect {
        left: parse_percent(parts[0])?,
        top: parse_percent(parts[1])?,
        right: parse_percent(parts[2])?,
        bottom: parse_percent(parts[3])?,
    })
}

/// `1pt solid #000000`.
fn parse_border(value: &str) -> Option<SimpleBorder> {
    let mut parts = value.split_whitespace();
    Some(SimpleBorder {
        width: parse_len(parts.next()?)?,
        style: parts.next()?.to_string(),
        color: parts.next()?.to_string(),
    })
}

/// Reads `(destination)`, with or without the `<>` the writer adds when the
/// destination holds spaces or parentheses.
fn read_destination(text: &str) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'(') {
        return None;
    }
    let mut depth = 0usize;
    for index in 0..bytes.len() {
        match bytes[index] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    let inner = &text[1..index];
                    let target = match inner.strip_prefix('<').and_then(|t| t.strip_suffix('>')) {
                        Some(bracketed) => bracketed.replace("%3E", ">"),
                        None => inner.to_string(),
                    };
                    return Some((target, index + 1));
                }
            }
            _ => {}
        }
    }
    None
}

/// Reads an attribute block sitting immediately at the start of `text`.
fn read_attr_block(text: &str) -> Option<(Attrs, usize)> {
    if !text.starts_with('{') {
        return None;
    }
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut in_quotes = false;
    for index in 0..bytes.len() {
        match bytes[index] {
            b'"' if index == 0 || bytes[index - 1] != b'\\' => in_quotes = !in_quotes,
            b'{' if !in_quotes => depth += 1,
            b'}' if !in_quotes => {
                depth -= 1;
                if depth == 0 {
                    return Attrs::parse(&text[..=index]).map(|a| (a, index + 1));
                }
            }
            _ => {}
        }
    }
    None
}

/// Byte index of the `]` closing the `[` that starts `text`.
///
/// Walks characters rather than bytes: the text is arbitrary UTF-8, and a
/// backslash escaping an `é` must skip both of its bytes, not one.
fn find_close_bracket(text: &str) -> Option<usize> {
    let mut chars = text.char_indices();
    let mut depth = 0usize;
    while let Some((index, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// One piece of a tokenised line: something already resolved, or a run of
/// emphasis delimiters still waiting to be paired.
#[derive(Debug, Clone)]
enum Token {
    // Boxed: an `Inline` dwarfs a delimiter run, and a line becomes a whole
    // vector of these.
    Node(Box<Inline>),
    Run {
        ch: char,
        len: usize,
        can_open: bool,
        can_close: bool,
    },
}

impl Token {
    fn node(inline: Inline) -> Token {
        Token::Node(Box::new(inline))
    }
}

fn push_text(out: &mut Vec<Token>, buf: &mut String) {
    if !buf.is_empty() {
        out.push(Token::node(Inline::Text(std::mem::take(buf))));
    }
}

/// Turns whatever delimiters were never paired back into the text they are,
/// and merges the runs of text that result.
fn flatten(tokens: Vec<Token>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    for token in tokens {
        let inline = match token {
            Token::Node(inline) => *inline,
            Token::Run { ch, len, .. } => Inline::Text(ch.to_string().repeat(len)),
        };
        match (out.last_mut(), inline) {
            (Some(Inline::Text(previous)), Inline::Text(next)) => previous.push_str(&next),
            (_, inline) => out.push(inline),
        }
    }
    out.retain(|inline| !matches!(inline, Inline::Text(t) if t.is_empty()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> (Vec<Inline>, ConversionReport) {
        let assets = BTreeMap::new();
        let footnotes = BTreeMap::new();
        let mut report = ConversionReport::new();
        let inlines = {
            let mut inliner = Inliner {
                assets: &assets,
                footnotes: &footnotes,
                report: &mut report,
            };
            inliner.parse(text)
        };
        (inlines, report)
    }

    fn only(text: &str) -> Inline {
        let (inlines, _) = parse(text);
        assert_eq!(inlines.len(), 1, "expected one inline from `{text}`");
        inlines.into_iter().next().unwrap()
    }

    #[test]
    fn plain_text_comes_back_unescaped() {
        let (inlines, _) = parse(r"Caracteres \*asterisco\* y \\barra.");
        assert_eq!(
            inlines,
            vec![Inline::Text("Caracteres *asterisco* y \\barra.".into())]
        );
    }

    #[test]
    fn emphasis_markers_become_run_properties() {
        let Inline::Styled { props, content } = only("**negrita**") else {
            panic!("expected a styled run");
        };
        assert_eq!(props.direct.bold, Some(true));
        assert_eq!(content, vec![Inline::Text("negrita".into())]);

        let Inline::Styled { props, .. } = only("***ambos***") else {
            panic!("expected a styled run");
        };
        assert_eq!(props.direct.bold, Some(true));
        assert_eq!(props.direct.italic, Some(true));
    }

    #[test]
    fn a_span_absorbs_the_emphasis_inside_it() {
        // The writer draws both from one `RunProps`; reading must not invent
        // a second, nested run.
        let Inline::Styled { props, content } = only("[**adicional**]{.Enfatico}") else {
            panic!("expected a styled run");
        };
        assert_eq!(props.style.as_ref().unwrap().as_str(), "Enfatico");
        assert_eq!(props.direct.bold, Some(true));
        assert_eq!(content, vec![Inline::Text("adicional".into())]);
    }

    #[test]
    fn nested_markers_collapse_into_one_run() {
        let Inline::Styled { props, content } = only("***~~todo~~***") else {
            panic!("expected a styled run");
        };
        assert_eq!(props.direct.bold, Some(true));
        assert_eq!(props.direct.italic, Some(true));
        assert_eq!(props.direct.strike, Some(true));
        assert_eq!(content, vec![Inline::Text("todo".into())]);
    }

    #[test]
    fn character_formatting_reads_from_the_attribute_block() {
        let Inline::Styled { props, .. } = only(r#"[texto]{color=#FF0000 font=Arial size=14pt}"#)
        else {
            panic!("expected a styled run");
        };
        assert_eq!(props.direct.color.as_deref(), Some("#FF0000"));
        assert_eq!(props.direct.name.as_deref(), Some("Arial"));
        assert_eq!(props.direct.size, Some(Length::from_pt(14.0)));

        let Inline::Styled { props, .. } = only("[x]{.underline underline=double}") else {
            panic!("expected a styled run");
        };
        assert_eq!(props.direct.underline, Some(Underline::Double));

        let Inline::Styled { props, .. } = only("[x]{.underline}") else {
            panic!("expected a styled run");
        };
        assert_eq!(props.direct.underline, Some(Underline::Single));

        let Inline::Styled { props, .. } = only("[x]{.sup}") else {
            panic!("expected a styled run");
        };
        assert_eq!(props.direct.vert_align, Some(VertAlign::Superscript));

        let Inline::Styled { props, .. } = only("[x]{bold=false}") else {
            panic!("expected a styled run");
        };
        assert_eq!(
            props.direct.bold,
            Some(false),
            "off is not the same as unset"
        );
    }

    #[test]
    fn links_carry_their_destination_and_style() {
        let Inline::Link {
            target,
            content,
            props,
        } = only("[el sitio](https://example.com/docsai){.Hyperlink}")
        else {
            panic!("expected a link");
        };
        assert_eq!(target, "https://example.com/docsai");
        assert_eq!(content, vec![Inline::Text("el sitio".into())]);
        assert_eq!(props.style.as_ref().unwrap().as_str(), "Hyperlink");
    }

    #[test]
    fn a_bracketed_destination_survives_its_spaces() {
        let Inline::Link { target, .. } = only("[x](<https://example.com/a b>)") else {
            panic!("expected a link");
        };
        assert_eq!(target, "https://example.com/a b");
    }

    #[test]
    fn fields_keep_their_instruction_and_cached_value() {
        let Inline::Field {
            kind,
            cached,
            instruction,
        } = only(r#"[Tabla de contenido]{.field field=TOC instr="TOC \\o \"1-3\" \\h"}"#)
        else {
            panic!("expected a field");
        };
        assert_eq!(kind, FieldKind::Toc);
        assert_eq!(cached, "Tabla de contenido");
        assert_eq!(instruction, r#"TOC \o "1-3" \h"#);
    }

    #[test]
    fn breaks_and_empty_markers() {
        assert_eq!(only("[]{.break kind=page}"), Inline::Break(BreakKind::Page));
        let (inlines, _) = parse("uno  \ndos");
        assert_eq!(
            inlines,
            vec![
                Inline::Text("uno".into()),
                Inline::Break(BreakKind::Line),
                Inline::Text("dos".into()),
            ]
        );
        let (empty, _) = parse("[]{.empty}");
        assert!(empty.is_empty(), "the empty marker contributes nothing");
    }

    #[test]
    fn a_paragraph_can_end_in_a_span_without_losing_it() {
        let (inlines, _) = parse("Pagina [1]{.field field=PAGE} de [3]{.field field=NUMPAGES}");
        assert_eq!(inlines.len(), 4);
        assert!(matches!(inlines[1], Inline::Field { .. }));
        assert!(matches!(inlines[3], Inline::Field { .. }));
    }

    #[test]
    fn images_rebuild_their_whole_geometry() {
        let text = concat!(
            "![Logo flotante](assets/img-40e10599.png){anchor=floating height=2.6cm ",
            "name=Logo native-size=120x90 relative-to=margin relative-to-v=paragraph ",
            "width=3.5cm wrap=square wrap-side=right x=1.2cm y=0.5cm z-index=2}"
        );
        let Inline::Image(image) = only(text) else {
            panic!("expected an image");
        };
        assert_eq!(image.alt, "Logo flotante");
        assert_eq!(image.name.as_deref(), Some("Logo"));
        assert_eq!(image.geometry.display_size.width, Length::from_cm(3.5));
        assert_eq!(image.geometry.native_size_px, Some((120, 90)));
        assert_eq!(image.geometry.z_index, Some(2));
        let Anchor::Floating {
            relative_to_h,
            relative_to_v,
            position,
            wrap,
            wrap_side,
            behind_text,
        } = image.geometry.anchor
        else {
            panic!("expected a floating anchor");
        };
        assert_eq!(relative_to_h, RelBase::Margin);
        assert_eq!(relative_to_v, RelBase::Paragraph);
        assert_eq!(position.h, AxisPos::Offset(Length::from_cm(1.2)));
        assert_eq!(wrap, WrapMode::Square);
        assert_eq!(wrap_side, WrapSide::Right);
        assert!(!behind_text);
    }

    #[test]
    fn image_transforms_read_back() {
        let Inline::Image(image) = only(
            r#"![x](assets/img-1.png){border="1pt solid #000000" crop="10%,5%,20%,0%" flip=hv rotation=45 height=1cm width=1cm}"#,
        ) else {
            panic!("expected an image");
        };
        assert_eq!(image.geometry.rotation_deg, 45.0);
        assert_eq!(image.geometry.flip, Flip::HV);
        let crop = image.geometry.crop.expect("crop");
        assert_eq!(
            (crop.left, crop.top, crop.right, crop.bottom),
            (10.0, 5.0, 20.0, 0.0)
        );
        let border = image.geometry.border.expect("border");
        assert_eq!(border.width, Length::from_pt(1.0));
        assert_eq!(border.style, "solid");
        assert_eq!(border.color, "#000000");
    }

    #[test]
    fn a_missing_asset_is_reported_not_swallowed() {
        let (_, report) = parse("![x](assets/img-desaparecida.png){width=1cm height=1cm}");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| matches!(w, Warning::AssetIssue { .. })),
            "a referenced file with no bytes must reach the report"
        );
    }

    #[test]
    fn an_unmatched_bracket_is_just_text() {
        let (inlines, _) = parse("un [corchete sin cerrar");
        assert_eq!(
            inlines,
            vec![Inline::Text("un [corchete sin cerrar".into())]
        );
    }
}

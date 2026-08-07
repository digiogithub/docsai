//! DrawingML text: `p:txBody` into IR blocks.
//!
//! The run model is the one the docx reader already uses — a run inside a
//! placeholder is the same run as a run inside a paragraph — but the XML is
//! DrawingML's, not WordprocessingML's: properties are attributes rather than
//! child elements, sizes are hundredths of a point, and the bullet lives in
//! `a:pPr` instead of in a separate numbering part.
//!
//! What makes a paragraph a **list item** here is the one judgement call. A
//! body placeholder bullets its paragraphs by inheritance — the master's
//! `p:bodyStyle` carries `a:buChar` for every level and the slide says nothing
//! — so a reader that only honoured explicit bullets would turn every deck into
//! flat paragraphs and lose the structure spike P2 measured DocMark-P against.
//! The rule is therefore: explicit `a:buNone` wins, then an explicit
//! `a:buAutoNum`/`a:buChar`, then the shape's own default.
//!
//! Run properties are **resolved and then subtracted**: `+mj-lt` and
//! `a:schemeClr` become the font and the colour the theme names, and whatever
//! the shape's cascade already decided at that outline level is dropped. What a
//! run stores is its delta, which for most slides is nothing at all. The
//! resolution itself lives in [`cascade`](super::cascade).

use docsai_model::report::{ConversionReport, Warning};
use docsai_model::style::{Align, FontProps, LineHeight, ParaProps, Underline, VertAlign};
use docsai_model::text::{
    Block, BreakKind, FieldKind, Inline, List, ListItem, ParaFormat, Paragraph, RunProps,
};
use docsai_model::units::Length;

use crate::package::Relationships;
use crate::xml::Element;

use super::cascade::{LevelStyles, Theme};

/// What a shape's text needs from around it.
pub(super) struct TextCtx<'a> {
    pub rels: &'a Relationships,
    /// True when this shape's paragraphs are bullets unless they say
    /// otherwise: what a body placeholder inherits from the master.
    pub bulleted: bool,
    /// The theme `+mj-lt` and `a:schemeClr` resolve against.
    pub theme: &'a Theme,
    /// What this shape's runs inherit, per outline level. What a run states and
    /// the cascade already decided is not a delta and is not stored.
    pub inherited: &'a LevelStyles,
}

/// Reads a `p:txBody` into blocks.
pub(super) fn read_body(
    tx_body: &Element,
    ctx: &TextCtx<'_>,
    report: &mut ConversionReport,
) -> Vec<Block> {
    let flat: Vec<Flat> = tx_body
        .children_named("p")
        .map(|p| read_paragraph(p, ctx, report))
        .collect();
    rebuild_lists(flat, report)
}

/// A paragraph before the list tree is rebuilt.
enum Flat {
    /// A plain paragraph, or an empty one: an empty paragraph is content, and
    /// it is deliberately *not* a list item — a bullet with nothing after it is
    /// not what the slide shows.
    Block(Paragraph),
    Item {
        ordered: bool,
        level: u8,
        paragraph: Paragraph,
    },
}

fn read_paragraph(p: &Element, ctx: &TextCtx<'_>, report: &mut ConversionReport) -> Flat {
    let p_pr = p.child("pPr");
    // The outline level decides what the paragraph inherits, so it is read
    // before the runs rather than after them.
    let level = p_pr
        .and_then(|pr| pr.attr_i64("lvl"))
        .unwrap_or(0)
        .clamp(0, 8) as u8;
    let inherited = ctx.inherited.at(level);

    let mut paragraph = Paragraph {
        id: None,
        format: ParaFormat {
            style: None,
            direct: p_pr.map(para_props).unwrap_or_default(),
            run_direct: p
                .child("endParaRPr")
                .map(|rpr| font_props(rpr, ctx.theme, report).minus(&inherited))
                .unwrap_or_default(),
        },
        content: Vec::new(),
    };

    for child in p.children() {
        match child.name.as_str() {
            "r" => {
                let props = RunProps::direct(
                    child
                        .child("rPr")
                        .map(|rpr| font_props(rpr, ctx.theme, report).minus(&inherited))
                        .unwrap_or_default(),
                );
                let text = child.child("t").map(|t| t.deep_text()).unwrap_or_default();
                let content = vec![Inline::Text(text)];
                let content = match child.child("rPr").and_then(|rpr| link_target(rpr, ctx)) {
                    Some(target) => vec![Inline::Link {
                        target,
                        content,
                        props,
                    }],
                    None => Inline::styled(content, props),
                };
                paragraph.content.extend(content);
            }
            "br" => paragraph.content.push(Inline::Break(BreakKind::Line)),
            "fld" => paragraph.content.push(read_field(child)),
            // `a:pPr` and `a:endParaRPr` are formatting, already read.
            _ => {}
        }
    }

    match bullet_of(p_pr, ctx, &paragraph) {
        Some(ordered) => Flat::Item {
            ordered,
            level,
            paragraph,
        },
        None => Flat::Block(paragraph),
    }
}

/// Whether the paragraph is a list item, and whether the list is numbered.
fn bullet_of(p_pr: Option<&Element>, ctx: &TextCtx<'_>, paragraph: &Paragraph) -> Option<bool> {
    // An empty paragraph holds its place; it does not hold a bullet.
    if paragraph.is_empty() {
        return None;
    }
    if let Some(p_pr) = p_pr {
        if p_pr.child("buNone").is_some() {
            return None;
        }
        if p_pr.child("buAutoNum").is_some() {
            return Some(true);
        }
        if p_pr.child("buChar").is_some() {
            return Some(false);
        }
        // A nested paragraph is an item even where the bullet is inherited:
        // its indent means nothing outside a list.
        if p_pr.attr_i64("lvl").unwrap_or(0) > 0 {
            return Some(false);
        }
    }
    ctx.bulleted.then_some(false)
}

/// `a:fld`: the slide number and the date, which behave like `w:fldSimple`.
fn read_field(fld: &Element) -> Inline {
    let kind = fld.attr("type").unwrap_or_default();
    let cached = fld.child("t").map(|t| t.deep_text()).unwrap_or_default();
    Inline::Field {
        kind: match kind {
            // A slide number is the deck's page number; DocMark has one field
            // for "where am I in the document" and this is it.
            "slidenum" => FieldKind::Page,
            other if other.starts_with("datetime") => FieldKind::Date,
            other => FieldKind::from_instruction(other),
        },
        cached,
        // The DrawingML type verbatim, so the writer puts back the same field
        // and not a Word instruction that means roughly the same thing.
        instruction: kind.to_string(),
    }
}

/// The hyperlink target of `a:rPr/a:hlinkClick`, when it has one.
fn link_target(rpr: &Element, ctx: &TextCtx<'_>) -> Option<String> {
    let link = rpr.child("hlinkClick")?;
    let id = link
        .attrs
        .iter()
        .find(|(name, _)| name.ends_with(":id"))
        .map(|(_, value)| value.as_str())?;
    Some(ctx.rels.get(id)?.target.clone())
}

/// Reads `a:rPr`/`a:endParaRPr` into character-level properties, **resolved**
/// against the theme but not yet subtracted from what the run inherits: the
/// cascade reads `a:defRPr` through this same function, and a reference has to
/// mean the same thing on both sides of the comparison.
pub(super) fn font_props(rpr: &Element, theme: &Theme, report: &mut ConversionReport) -> FontProps {
    let mut font = FontProps::default();

    if let Some(size) = rpr.attr_i64("sz") {
        // Hundredths of a point, where WordprocessingML uses halves.
        font.size = Some(Length::from_pt(size as f64 / 100.0));
    }
    font.bold = rpr.attr("b").map(is_true);
    font.italic = rpr.attr("i").map(is_true);
    font.strike = rpr
        .attr("strike")
        .map(|value| !value.eq_ignore_ascii_case("noStrike"));
    font.underline = rpr.attr("u").map(|value| match value {
        "none" => Underline::None,
        "dbl" | "wavyDbl" => Underline::Double,
        "heavy" | "wavyHeavy" | "dottedHeavy" | "dashedHeavy" => Underline::Thick,
        "dotted" | "dotDash" | "dotDotDash" => Underline::Dotted,
        "dash" | "dashLong" => Underline::Dashed,
        "wavy" => Underline::Wave,
        _ => Underline::Single,
    });
    if let Some(baseline) = rpr.attr_i64("baseline") {
        font.vert_align = Some(match baseline.cmp(&0) {
            std::cmp::Ordering::Greater => VertAlign::Superscript,
            std::cmp::Ordering::Less => VertAlign::Subscript,
            std::cmp::Ordering::Equal => VertAlign::Baseline,
        });
    }
    match rpr.attr("cap") {
        Some("all") => font.caps = Some(true),
        Some("small") => font.small_caps = Some(true),
        Some("none") => {
            font.caps = Some(false);
            font.small_caps = Some(false);
        }
        _ => {}
    }
    font.name = rpr
        .child("latin")
        .and_then(|latin| latin.attr("typeface"))
        // `+mj-lt` and `+mn-lt` are theme references, not font names: what is
        // stored is the font the theme names, because writing `+mj-lt` as a
        // typeface would ask for a font nobody has installed.
        .and_then(|typeface| theme.font(typeface));
    font.color = rpr
        .child("solidFill")
        .and_then(|fill| solid_colour(fill, theme, report));
    font.highlight = rpr
        .child("highlight")
        .and_then(|fill| solid_colour(fill, theme, report));

    font
}

/// The `#rrggbb` of a fill.
///
/// `a:srgbClr` states it outright; `a:schemeClr` names a slot the master maps
/// into the theme, and resolving it is what makes a slide's colour comparable
/// with the one it inherits. A colour model this reader cannot resolve is
/// reported rather than dropped in silence.
fn solid_colour(fill: &Element, theme: &Theme, report: &mut ConversionReport) -> Option<String> {
    if let Some(srgb) = fill.child("srgbClr").and_then(|c| c.attr("val")) {
        if srgb.len() == 6 && srgb.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(format!("#{}", srgb.to_ascii_lowercase()));
        }
    }
    if let Some(scheme) = fill.child("schemeClr") {
        let val = scheme.attr("val").unwrap_or_default();
        // A transform — `a:lumMod`, `a:alpha`, `a:tint` — changes the colour the
        // slot resolves to. The base colour still travels; the shade it was
        // shifted to does not, and that is a loss worth naming.
        if let Some(transform) = scheme.children().next() {
            report.warn(Warning::Degraded {
                what: format!("a:schemeClr val=\"{val}\""),
                why: format!("the `a:{}` transform is not applied", transform.name),
            });
        }
        let resolved = theme.colour(val);
        if resolved.is_none() {
            report.warn(Warning::Degraded {
                what: format!("a:schemeClr val=\"{val}\""),
                why: if theme.is_empty() {
                    "the deck declares no theme to resolve it against".into()
                } else {
                    "the theme does not define that colour".into()
                },
            });
        }
        return resolved;
    }
    if let Some(other) = fill.children().next() {
        report.warn(Warning::Degraded {
            what: format!("a:{}", other.name),
            why: "colour model not resolved to a hex value".into(),
        });
    }
    None
}

/// Reads `a:pPr` into paragraph-level deltas. The bullet is read separately;
/// `lvl` is list depth, not indentation, and never lands here.
fn para_props(p_pr: &Element) -> ParaProps {
    // DrawingML has one `indent`: negative is a hanging indent, positive a
    // first-line one. The IR keeps them apart.
    let indent = p_pr.attr_i64("indent").unwrap_or(0);

    ParaProps {
        align: p_pr.attr("algn").and_then(|value| match value {
            "l" => Some(Align::Left),
            "ctr" => Some(Align::Center),
            "r" => Some(Align::Right),
            "just" | "justLow" | "dist" | "thaiDist" => Some(Align::Justify),
            _ => None,
        }),
        indent_left: p_pr.attr_i64("marL").map(Length::from_emu),
        indent_right: p_pr.attr_i64("marR").map(Length::from_emu),
        indent_hanging: (indent < 0).then(|| Length::from_emu(-indent)),
        indent_first_line: (indent > 0).then(|| Length::from_emu(indent)),
        space_before: p_pr.child("spcBef").and_then(spacing),
        space_after: p_pr.child("spcAft").and_then(spacing),
        line_height: p_pr.child("lnSpc").and_then(|spc| {
            if let Some(pct) = spc.child("spcPct").and_then(|e| e.attr_i64("val")) {
                // Thousandths of a percent in the source, thousandths of a line
                // in the IR: 100 000 (100 %) is one line.
                Some(LineHeight::Multiple((pct / 100) as i32))
            } else {
                spacing(spc).map(LineHeight::Exact)
            }
        }),
        ..Default::default()
    }
}

/// `a:spcBef`/`a:spcAft`/`a:lnSpc` as an absolute length, when they state one.
fn spacing(element: &Element) -> Option<Length> {
    let points = element.child("spcPts")?.attr_i64("val")?;
    Some(Length::from_pt(points as f64 / 100.0))
}

/// DrawingML booleans are `1`/`0` or `true`/`false`, never OOXML's absent-means-true.
fn is_true(value: &str) -> bool {
    matches!(value, "1" | "true" | "on")
}

/// Rebuilds the list tree from the flat `(ordered, level)` stream.
fn rebuild_lists(flat: Vec<Flat>, report: &mut ConversionReport) -> Vec<Block> {
    let mut out = Vec::new();
    let mut run: Vec<(bool, u8, Paragraph)> = Vec::new();

    for item in flat {
        match item {
            Flat::Item {
                ordered,
                level,
                paragraph,
            } => {
                // Numbered and bulleted paragraphs are different lists even
                // when they sit next to each other at the same level.
                if run.first().is_some_and(|(first, first_level, _)| {
                    *first != ordered && *first_level == level
                }) {
                    flush_list(&mut run, report, &mut out);
                }
                run.push((ordered, level, paragraph));
            }
            Flat::Block(paragraph) => {
                flush_list(&mut run, report, &mut out);
                report.stats.paragraphs = report.stats.paragraphs.saturating_add(1);
                out.push(Block::Paragraph(paragraph));
            }
        }
    }
    flush_list(&mut run, report, &mut out);
    out
}

fn flush_list(
    run: &mut Vec<(bool, u8, Paragraph)>,
    report: &mut ConversionReport,
    out: &mut Vec<Block>,
) {
    if run.is_empty() {
        return;
    }
    report.stats.lists = report.stats.lists.saturating_add(1);
    report.stats.paragraphs = report.stats.paragraphs.saturating_add(run.len() as u32);
    let items = std::mem::take(run);
    out.push(build_list(&items));
}

/// Turns a run of list paragraphs into a nested [`List`], exactly as the docx
/// reader does with `w:numPr` — the tree shape is the format's, the algorithm
/// is not.
fn build_list(items: &[(bool, u8, Paragraph)]) -> Block {
    let base = items[0].1;
    let mut list = List {
        id: None,
        def: None,
        ordered: items[0].0,
        level: base,
        items: Vec::new(),
    };

    let mut i = 0;
    while i < items.len() {
        if items[i].1 <= base {
            list.items.push(ListItem {
                blocks: vec![Block::Paragraph(items[i].2.clone())],
            });
            i += 1;
        } else {
            let start = i;
            while i < items.len() && items[i].1 > base {
                i += 1;
            }
            let nested = build_list(&items[start..i]);
            match list.items.last_mut() {
                Some(parent) => parent.blocks.push(nested),
                // A deeper item with no parent above it is malformed, and it
                // still has to survive as content.
                None => list.items.push(ListItem {
                    blocks: vec![nested],
                }),
            }
        }
    }
    Block::List(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS: &str = r#"xmlns:a="urn:a" xmlns:r="urn:r""#;

    fn read(xml: &str, bulleted: bool) -> (Vec<Block>, ConversionReport) {
        read_with(xml, bulleted, &Theme::default(), &LevelStyles::default())
    }

    fn read_with(
        xml: &str,
        bulleted: bool,
        theme: &Theme,
        inherited: &LevelStyles,
    ) -> (Vec<Block>, ConversionReport) {
        let root = Element::parse("t.xml", xml.as_bytes()).unwrap();
        let rels = Relationships::default();
        let mut report = ConversionReport::new();
        let ctx = TextCtx {
            rels: &rels,
            bulleted,
            theme,
            inherited,
        };
        let blocks = read_body(&root, &ctx, &mut report);
        (blocks, report)
    }

    fn text_of(block: &Block) -> String {
        match block {
            Block::Paragraph(p) => p.plain_text(),
            _ => String::new(),
        }
    }

    #[test]
    fn a_body_placeholder_bullets_what_the_master_bullets() {
        // The slide states no bullet at all; the master's `bodyStyle` does. A
        // reader that only honoured explicit bullets would flatten every deck.
        let xml = format!(
            r#"<p:txBody {NS} xmlns:p="urn:p"><a:bodyPr/>
                <a:p><a:r><a:t>Ingresos al alza</a:t></a:r></a:p>
                <a:p><a:r><a:t>Costes estables</a:t></a:r></a:p>
            </p:txBody>"#
        );
        let (blocks, _) = read(&xml, true);
        let [Block::List(list)] = blocks.as_slice() else {
            panic!("expected one list, got {blocks:?}");
        };
        assert!(!list.ordered);
        assert_eq!(list.items.len(), 2);

        // The same body in a title placeholder is two paragraphs.
        let (plain, _) = read(&xml, false);
        assert_eq!(plain.len(), 2);
        assert_eq!(text_of(&plain[0]), "Ingresos al alza");
    }

    #[test]
    fn levels_nest_and_numbering_starts_its_own_list() {
        let xml = format!(
            r#"<p:txBody {NS} xmlns:p="urn:p"><a:bodyPr/>
                <a:p><a:pPr lvl="0"/><a:r><a:t>Primero</a:t></a:r></a:p>
                <a:p><a:pPr lvl="1"/><a:r><a:t>Dentro</a:t></a:r></a:p>
                <a:p><a:pPr lvl="2"/><a:r><a:t>Más dentro</a:t></a:r></a:p>
                <a:p><a:pPr lvl="0"/><a:r><a:t>Vuelve</a:t></a:r></a:p>
            </p:txBody>"#
        );
        let (blocks, report) = read(&xml, true);
        let [Block::List(list)] = blocks.as_slice() else {
            panic!("expected one list, got {blocks:?}");
        };
        assert_eq!(list.items.len(), 2, "two top-level items");
        assert_eq!(text_of(&list.items[0].blocks[0]), "Primero");
        let Block::List(nested) = &list.items[0].blocks[1] else {
            panic!("the second level hangs from the first");
        };
        assert_eq!(nested.level, 1);
        let Block::List(deepest) = &nested.items[0].blocks[1] else {
            panic!("and the third from the second");
        };
        assert_eq!(deepest.level, 2);
        assert_eq!(report.stats.lists, 1, "one list, three levels");
    }

    #[test]
    fn an_explicit_bullet_wins_over_the_shape_default() {
        let xml = format!(
            r#"<p:txBody {NS} xmlns:p="urn:p"><a:bodyPr/>
                <a:p><a:pPr><a:buAutoNum type="arabicPeriod"/></a:pPr><a:r><a:t>Uno</a:t></a:r></a:p>
                <a:p><a:pPr><a:buAutoNum type="arabicPeriod"/></a:pPr><a:r><a:t>Dos</a:t></a:r></a:p>
                <a:p><a:pPr><a:buNone/></a:pPr><a:r><a:t>Ni viñeta ni número</a:t></a:r></a:p>
            </p:txBody>"#
        );
        let (blocks, _) = read(&xml, true);
        assert_eq!(blocks.len(), 2);
        let Block::List(list) = &blocks[0] else {
            panic!("expected a numbered list");
        };
        assert!(list.ordered);
        assert_eq!(list.items.len(), 2);
        assert_eq!(text_of(&blocks[1]), "Ni viñeta ni número");
    }

    #[test]
    fn an_empty_paragraph_is_content_and_never_a_bullet() {
        // The pptx echo of the Phase 1 empty-paragraph bug.
        let xml = format!(
            r#"<p:txBody {NS} xmlns:p="urn:p"><a:bodyPr/>
                <a:p><a:r><a:t>Primera línea</a:t></a:r></a:p>
                <a:p><a:endParaRPr/></a:p>
                <a:p><a:r><a:t>Tras el hueco</a:t></a:r></a:p>
            </p:txBody>"#
        );
        let (blocks, _) = read(&xml, true);
        assert_eq!(blocks.len(), 3, "the gap is a block of its own: {blocks:?}");
        assert!(matches!(blocks[0], Block::List(_)));
        assert_eq!(text_of(&blocks[1]), "");
        assert!(matches!(blocks[2], Block::List(_)));
    }

    #[test]
    fn run_properties_come_off_the_attributes_drawingml_puts_them_in() {
        let xml = format!(
            r#"<p:txBody {NS} xmlns:p="urn:p"><a:bodyPr/>
                <a:p><a:r><a:rPr b="1" i="0" sz="1800" u="dbl" baseline="30000" cap="all">
                    <a:solidFill><a:srgbClr val="1E5AC8"/></a:solidFill>
                    <a:latin typeface="Calibri"/>
                </a:rPr><a:t>Con formato</a:t></a:r><a:br/><a:r><a:t>Segunda</a:t></a:r></a:p>
            </p:txBody>"#
        );
        let (blocks, _) = read(&xml, false);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("expected a paragraph");
        };
        let Inline::Styled { props, .. } = &p.content[0] else {
            panic!("expected a styled run, got {:?}", p.content[0]);
        };
        assert_eq!(props.direct.bold, Some(true));
        assert_eq!(props.direct.italic, Some(false));
        assert_eq!(props.direct.size, Some(Length::from_pt(18.0)));
        assert_eq!(props.direct.underline, Some(Underline::Double));
        assert_eq!(props.direct.vert_align, Some(VertAlign::Superscript));
        assert_eq!(props.direct.caps, Some(true));
        assert_eq!(props.direct.color.as_deref(), Some("#1e5ac8"));
        assert_eq!(props.direct.name.as_deref(), Some("Calibri"));
        assert!(matches!(p.content[1], Inline::Break(BreakKind::Line)));
    }

    /// A theme with a major font and one scheme colour, enough to resolve
    /// against.
    fn theme() -> Theme {
        let xml = r#"<a:theme xmlns:a="urn:a"><a:themeElements>
              <a:clrScheme name="t"><a:dk1><a:srgbClr val="1F3864"/></a:dk1></a:clrScheme>
              <a:fontScheme name="t">
                <a:majorFont><a:latin typeface="Calibri Light"/></a:majorFont>
                <a:minorFont><a:latin typeface="Calibri"/></a:minorFont>
              </a:fontScheme>
            </a:themeElements></a:theme>"#;
        Theme::read(&Element::parse("theme.xml", xml.as_bytes()).unwrap())
    }

    #[test]
    fn theme_references_resolve_to_what_the_theme_names() {
        // `+mj-lt` says "whatever the theme's major font is" and `dk1` is a
        // slot; storing either verbatim would write a font nobody has
        // installed and a colour no renderer understands.
        let xml = format!(
            r#"<p:txBody {NS} xmlns:p="urn:p"><a:bodyPr/><a:p><a:r>
                <a:rPr><a:solidFill><a:schemeClr val="dk1"/></a:solidFill>
                <a:latin typeface="+mj-lt"/></a:rPr><a:t>Título</a:t>
            </a:r></a:p></p:txBody>"#
        );
        let (blocks, _) = read_with(&xml, false, &theme(), &LevelStyles::default());
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("expected a paragraph");
        };
        let Inline::Styled { props, .. } = &p.content[0] else {
            panic!("expected a styled run, got {:?}", p.content[0]);
        };
        assert_eq!(props.direct.name.as_deref(), Some("Calibri Light"));
        assert_eq!(props.direct.color.as_deref(), Some("#1f3864"));
    }

    #[test]
    fn what_the_run_inherits_is_not_a_delta_and_is_not_stored() {
        // The run states exactly what the cascade already decided, spelled with
        // references rather than values. Resolved, the two are the same thing,
        // and the shape stores nothing — the economy rule, applied to slides.
        let master = r#"<a:lstStyle xmlns:a="urn:a"><a:lvl1pPr><a:defRPr sz="2800">
              <a:solidFill><a:schemeClr val="dk1"/></a:solidFill>
              <a:latin typeface="+mn-lt"/></a:defRPr></a:lvl1pPr></a:lstStyle>"#;
        let mut report = ConversionReport::new();
        let inherited = super::super::cascade::read_levels(
            &Element::parse("m.xml", master.as_bytes()).unwrap(),
            &theme(),
            &mut report,
        );

        let xml = format!(
            r#"<p:txBody {NS} xmlns:p="urn:p"><a:bodyPr/>
                <a:p><a:r><a:rPr sz="2800"><a:solidFill><a:schemeClr val="dk1"/></a:solidFill>
                  <a:latin typeface="+mn-lt"/></a:rPr><a:t>Heredado</a:t></a:r></a:p>
                <a:p><a:r><a:rPr sz="1800"><a:latin typeface="Georgia"/></a:rPr>
                  <a:t>Propio</a:t></a:r></a:p>
            </p:txBody>"#
        );
        let (blocks, _) = read_with(&xml, false, &theme(), &inherited);

        let Block::Paragraph(first) = &blocks[0] else {
            panic!("expected a paragraph");
        };
        assert!(
            matches!(&first.content[0], Inline::Text(t) if t == "Heredado"),
            "nothing to store: {:?}",
            first.content[0]
        );

        // What differs survives, and only that: the colour is still inherited.
        let Block::Paragraph(second) = &blocks[1] else {
            panic!("expected a paragraph");
        };
        let Inline::Styled { props, .. } = &second.content[0] else {
            panic!("expected a styled run, got {:?}", second.content[0]);
        };
        assert_eq!(props.direct.size, Some(Length::from_pt(18.0)));
        assert_eq!(props.direct.name.as_deref(), Some("Georgia"));
        assert_eq!(props.direct.color, None);
    }

    #[test]
    fn a_slide_number_field_keeps_what_it_showed() {
        let xml = format!(
            r#"<p:txBody {NS} xmlns:p="urn:p"><a:bodyPr/><a:p>
                <a:fld id="{{x}}" type="slidenum"><a:t>7</a:t></a:fld>
            </a:p></p:txBody>"#
        );
        let (blocks, _) = read(&xml, false);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("expected a paragraph");
        };
        let Inline::Field {
            kind,
            cached,
            instruction,
        } = &p.content[0]
        else {
            panic!("expected a field, got {:?}", p.content[0]);
        };
        assert_eq!(*kind, FieldKind::Page);
        assert_eq!(cached, "7");
        assert_eq!(instruction, "slidenum", "the writer puts back `slidenum`");
    }

    #[test]
    fn paragraph_properties_survive_the_unit_change() {
        let xml = format!(
            r#"<p:txBody {NS} xmlns:p="urn:p"><a:bodyPr/><a:p>
                <a:pPr algn="ctr" marL="342900" indent="-342900">
                    <a:lnSpc><a:spcPct val="150000"/></a:lnSpc>
                    <a:spcBef><a:spcPts val="600"/></a:spcBef>
                    <a:buNone/>
                </a:pPr><a:r><a:t>Centrado</a:t></a:r>
            </a:p></p:txBody>"#
        );
        let (blocks, _) = read(&xml, true);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("expected a paragraph");
        };
        let direct = &p.format.direct;
        assert_eq!(direct.align, Some(Align::Center));
        assert_eq!(direct.indent_left, Some(Length::from_emu(342_900)));
        assert_eq!(direct.indent_hanging, Some(Length::from_emu(342_900)));
        assert_eq!(direct.line_height, Some(LineHeight::Multiple(1500)));
        assert_eq!(direct.space_before, Some(Length::from_pt(6.0)));
    }
}

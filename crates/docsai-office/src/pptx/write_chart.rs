//! The chart on the way out (Phase 16, increment 16-C).
//!
//! Two paths, and they are the two paths this writer has always had.
//!
//! **Into a preserved part.** `ppt/charts/chartN.xml` is not in
//! [`SkeletonRef::rebuilt_parts`](docsai_model::presentation::SkeletonRef) and
//! 16-A did not move it there: reading a chart as data bought no licence to
//! regenerate it. So the caches are **spliced inside the part that was read** —
//! `c:strCache` under `c:cat`, `c:numCache` under `c:val`, the series name's
//! cache under `c:tx` — and every other byte of the chart is the byte that came
//! in: its `c:spPr`, its axes, its legend, its data labels, its `c:extLst`.
//!
//! The rule that makes that safe is 15-B's and it is enforced here the way
//! [[73-phase-15-carried]] says it must be: **the comparison is `ChartRef`
//! against `ChartRef`**, made by running the 16-A reader
//! ([`super::chart::read_space`]) over the preserved part, never by comparing a
//! rewritten string against the bytes that are there. A chart nobody touched
//! therefore produces no edit at all — not to the chart part, not to its
//! `_rels`, not to `ppt/embeddings/`, not to `[Content_Types].xml` — and no
//! warning either.
//!
//! **Whole, onto a slide that is being generated.** A deck with no skeleton
//! (15-G) has no chart part to splice into, so one is written: a `c:chartSpace`
//! with a plot matching [`ChartRef::kind`], the model's caches, and the axes
//! that plot needs. `c:chartSpace` has a strictly ordered content model and so
//! does every element inside it, which is why the order here is stated rather
//! than discovered — `corpus/schema_gate.py` validates every part this writer
//! creates against the ECMA-376 XSDs.
//!
//! ## What is refused, and why
//!
//! - **A series added or removed.** That is a `c:ser` element appearing or
//!   vanishing, with `c:idx` and `c:order` renumbered across every plot, a
//!   `c:spPr` and a `c:dLbls` invented for it, and every `c:f` in the chart
//!   restated. It is a rewrite of the part, not a splice, and an invented
//!   `c:ser` is what PowerPoint offers to repair. Refused by name.
//! - **The chart's kind.** `barChart` to `lineChart` moves the series into a
//!   different plot element with a different content model and different axes.
//! - **The title.** It is a DrawingML text body (`c:rich`) whose run properties
//!   — size, font, colour — the IR does not hold, so writing the model's plain
//!   text over it would silently restyle the chart.
//! - **The embedded workbook.** See [`workbook_note`].

use std::collections::BTreeMap;
use std::ops::Range;

use docsai_model::presentation::ChartRef;
use docsai_model::report::ConversionReport;
use docsai_model::sheet::CellRef;
use docsai_model::text::RawFragment;

use crate::package::Package;
use crate::xml::Element;

use super::chart::{self, cache_of, points};
use super::write_text::esc_text;

/// The `a:graphicData@uri` a chart frame declares.
pub(super) const CHART_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
/// The relationship type of a chart part, from the slide that shows it.
pub(super) const CHART_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart";
/// What `[Content_Types].xml` has to call a chart part.
pub(super) const CHART_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";

/// The plots this writer can generate, and what each needs around it.
///
/// A pie and a doughnut state **no** `c:axId`, and a plot area holding them
/// declares no axes: writing a `c:catAx` for a pie is not a cosmetic mistake,
/// it is a part the schema rejects.
const PLOTS: [(&str, &str, bool); 5] = [
    // name, the children that come before `c:ser`, whether it takes two axes
    (
        "barChart",
        "<c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>",
        true,
    ),
    ("lineChart", "<c:grouping val=\"standard\"/>", true),
    ("areaChart", "<c:grouping val=\"standard\"/>", true),
    ("pieChart", "", false),
    ("doughnutChart", "", false),
];

/// What splicing one chart produced.
#[derive(Default)]
pub(super) struct Spliced {
    /// The chart part's new bytes. `None` is the common case and the cheap one:
    /// the part already says what the model says.
    pub part: Option<Vec<u8>>,
    /// Clauses for a `Warning::Degraded` naming the chart part. Empty for a
    /// chart nobody edited — a loss that did not happen must not be reported.
    pub kept: Vec<String>,
}

impl Spliced {
    fn refused(why: impl Into<String>) -> Spliced {
        Spliced {
            part: None,
            kept: vec![why.into()],
        }
    }
}

/// Splices the model's data into the preserved chart part.
///
/// The whole of 16-C's first half. `part` is the chart part the slide's
/// relationship resolves to; `fragments` is the deck's raw payload list, so
/// that markup a document carries for *this* part and that differs from it is
/// named rather than dropped in silence.
pub(super) fn rewrite(
    package: &Package,
    part: &str,
    chart: &ChartRef,
    fragments: &BTreeMap<&str, &RawFragment>,
) -> Spliced {
    let Some((source, root)) = package.text(part).ok().and_then(|source| {
        let root = Element::parse(part, source.as_bytes()).ok()?;
        Some((source, root))
    }) else {
        return Spliced::refused(
            "it could not be read back as XML, so the part was copied unchanged and \
             an edit to its data is not in the file",
        );
    };

    let mut kept = Vec::new();
    // Markup the *document* carries for this part. The stub's payload is the
    // chart part verbatim, so it normally equals what is there and nothing is
    // said; a payload that differs is a hand edit to a part outside
    // `rebuilt_parts`, and this writer does not re-inject those.
    if let Some(fragment) = chart.raw.as_ref().and_then(|id| fragments.get(id.as_str())) {
        if fragment.part == part && fragment.content != root.raw(source) {
            kept.push(format!(
                "the preserved markup `{}`, which is this chart's own part and not the \
                 slide's: a part outside the skeleton's rebuilt set is copied, never \
                 rewritten, so the markup in the document is not in the file",
                fragment.id.as_str()
            ));
        }
    }

    // The one honest question: does this part already say what the model says?
    // Asked in the model's units, by the model's own reader.
    let said = chart::read_space(
        &root,
        part,
        &package.relationships(part),
        &Default::default(),
        &mut ConversionReport::new(),
    );
    if said.matches(chart) {
        return Spliced { part: None, kept };
    }

    if said.kind != chart.kind {
        kept.push(format!(
            "its kind, which the document states as `{}` and the part as `{}`: the kind \
             is the plot element the series live in, and moving them into another one \
             is not a splice",
            chart.kind.as_deref().unwrap_or("none"),
            said.kind.as_deref().unwrap_or("none"),
        ));
        return Spliced { part: None, kept };
    }
    let sers = series_elements(&root);
    if sers.len() != chart.series.len() {
        kept.push(format!(
            "its series, of which the part states {} and the document {}: adding or \
             removing a `c:ser` renumbers every `c:idx` and `c:order` in the chart and \
             invents the markup of a series that was never plotted, which is a rewrite \
             of the part rather than a splice",
            sers.len(),
            chart.series.len()
        ));
        return Spliced { part: None, kept };
    }
    if said.title != chart.title {
        kept.push(format!(
            "its title, which the document states as `{}`: a chart title is a DrawingML \
             text body whose size, font and colour the model does not hold, so writing \
             the text over it would restyle the chart",
            chart.title.as_deref().unwrap_or("")
        ));
    }

    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for (index, (ser, wanted)) in sers.iter().zip(&chart.series).enumerate() {
        let number = index + 1;
        let said = &said.series[index];
        if said.name != wanted.name {
            match name_edit(ser, source, wanted.name.as_deref()) {
                Ok(edit) => edits.push(edit),
                Err(why) => kept.push(format!("the name of series {number}, {why}")),
            }
        }
        if said.values != wanted.values || said.number_format != wanted.number_format {
            let Some(cache) = ser.child("val").and_then(cache_of) else {
                kept.push(format!(
                    "the values of series {number}, which the part states through no cache \
                     this writer can splice"
                ));
                continue;
            };
            edits.push((
                cache.span.clone(),
                num_cache(
                    cache,
                    source,
                    &wanted.values,
                    wanted.number_format.as_deref(),
                ),
            ));
            if let Some(edit) = formula_edit(ser.child("val"), wanted.values.len()) {
                match edit {
                    Ok(edit) => edits.push(edit),
                    Err(why) => kept.push(format!("the range of series {number}, {why}")),
                }
            }
        }
    }

    if said.categories != chart.categories {
        if chart.categories.is_empty() {
            kept.push(
                "its category axis, which the document leaves empty: removing the axis \
                 from a preserved chart takes every `c:cat` out of every series and is \
                 not a splice"
                    .into(),
            );
        } else {
            for (index, ser) in sers.iter().enumerate() {
                let number = index + 1;
                let Some(cat) = ser.child("cat") else {
                    continue;
                };
                let Some(cache) = cache_of(cat) else {
                    // A multi-level axis (`c:multiLvlStrRef`) is a tree the
                    // model never read (16-A) and must not be overwritten with
                    // a list it never held.
                    kept.push(format!(
                        "the category axis of series {number}, which the part states as a \
                         tree the model holds one level of"
                    ));
                    continue;
                };
                // Only an axis the model was actually *shown* is rewritten:
                // `ChartRef::categories` is the first one the part states, and
                // a series stating a different one states something the
                // document never carried.
                if labels(cache) != said.categories {
                    kept.push(format!(
                        "the category axis of series {number}, which states labels of its \
                         own that the document's single axis never carried"
                    ));
                    continue;
                }
                edits.push((
                    cache.span.clone(),
                    str_cache(cache, source, &chart.categories),
                ));
                if let Some(edit) = formula_edit(Some(cat), chart.categories.len()) {
                    match edit {
                        Ok(edit) => edits.push(edit),
                        Err(why) => {
                            kept.push(format!("the category range of series {number}, {why}"))
                        }
                    }
                }
            }
        }
    }

    if edits.is_empty() {
        return Spliced { part: None, kept };
    }
    // `splice` drops an overlapping edit **without a word** ([[62-phase-15-package]]),
    // so two ranges that overlap are a defect, not a degradation. Every edit
    // above is a distinct element of a distinct `c:ser` child, and this says so
    // rather than trusting it.
    if let Some(why) = overlapping(&edits) {
        kept.push(format!(
            "its data: {why}, and an overlapping edit is dropped in silence, so the part \
             was copied unchanged"
        ));
        return Spliced { part: None, kept };
    }
    let written = super::write::splice(source, edits);
    if written == source {
        return Spliced { part: None, kept };
    }
    Spliced {
        part: Some(written.into_bytes()),
        kept,
    }
}

/// What to say about the embedded workbook of a chart whose data was spliced.
///
/// **The workbook is preserved, and the divergence is named.** The alternative
/// — regenerating it through `crate::xlsx::write` — was refused for two
/// reasons, and the second is the one that decides it:
///
/// 1. It rebuilds a whole package. Nothing of the source workbook that the
///    workbook IR does not model — its styles, its defined names, its other
///    sheets, a chart or a picture inside it — would survive, and the bytes
///    would differ from the original whether or not anything changed. That is
///    spike P3's measurement one scale down, in a file that is itself an OPC
///    package.
/// 2. The model does not hold a workbook. It holds **one cached column per
///    series**, which is not the sheet those numbers came from: the sheet has a
///    header row, a category column, formulas, and cells no series references.
///    Writing the cache back as a sheet would not restore the provenance, it
///    would *invent* one and destroy the real one.
///
/// The state this leaves is not a corruption and not a novelty: it is exactly
/// `corpus/pptx/charts-stale-workbook.pptx`, the deck 16-A generated on purpose
/// because it is what a real deck looks like after somebody edits the embedded
/// sheet. The reader already describes it correctly on the way back in.
pub(super) fn workbook_note(part: &str, workbook: &str) -> String {
    format!(
        "the workbook `{workbook}` embedded in `{part}`, which still holds the numbers \
         the chart was drawn from before the edit: the cached series in the chart part \
         are what the slide shows and what was rewritten, and the sheet a reader opens \
         under «Edit Data» was preserved byte for byte rather than regenerated from a \
         cache that is not a spreadsheet"
    )
}

/// Every `c:ser` of every plot, in document order — the order 16-A numbers the
/// series in, because it is the order the legend numbers them in.
fn series_elements(root: &Element) -> Vec<&Element> {
    root.path(&["chart", "plotArea"])
        .into_iter()
        .flat_map(|area| area.children().filter(|c| c.name.ends_with("Chart")))
        .flat_map(|plot| plot.children_named("ser"))
        .collect()
}

/// A category cache as [`chart::read_space`] reads it: a gap is an empty label,
/// so that a category's *index* is never shifted by one.
fn labels(cache: &Element) -> Vec<String> {
    points(cache)
        .into_iter()
        .map(Option::unwrap_or_default)
        .collect()
}

/// `prefix:` of an element, read off the part: `c` is a convention, not a rule.
fn prefix(element: &Element) -> String {
    if element.prefix.is_empty() {
        String::new()
    } else {
        format!("{}:", element.prefix)
    }
}

/// The `c:extLst` a cache carries, kept verbatim: it is the one child of a
/// cache this writer does not produce and has no business dropping.
fn ext_list(cache: &Element, source: &str) -> String {
    cache
        .child("extLst")
        .map(|ext| ext.raw(source).to_string())
        .unwrap_or_default()
}

/// A `c:strCache` holding exactly these labels.
///
/// `c:ptCount` is the **length of the column** and `c:pt@idx` are sparse, so an
/// empty label at the end of an axis is written as an absent point rather than
/// as an empty `c:v` — the same shape 16-A reads.
fn str_cache(cache: &Element, source: &str, labels: &[String]) -> String {
    let p = prefix(cache);
    let mut out = format!(
        "<{p}{0}><{p}ptCount val=\"{1}\"/>",
        cache.name,
        labels.len()
    );
    for (index, label) in labels.iter().enumerate() {
        if label.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "<{p}pt idx=\"{index}\"><{p}v>{}</{p}v></{p}pt>",
            esc_text(label)
        ));
    }
    out.push_str(&ext_list(cache, source));
    out.push_str(&format!("</{p}{}>", cache.name));
    out
}

/// A `c:numCache` holding exactly these values.
///
/// A gap is an **absent** `c:pt`, never a `c:v` of `0`: PowerPoint leaves a
/// hole in the line where a zero would draw one, and 16-A went to some trouble
/// to keep the two apart on the way in.
fn num_cache(
    cache: &Element,
    source: &str,
    values: &[Option<f64>],
    format: Option<&str>,
) -> String {
    let p = prefix(cache);
    let mut out = format!("<{p}{}>", cache.name);
    if let Some(format) = format {
        out.push_str(&format!(
            "<{p}formatCode>{}</{p}formatCode>",
            esc_text(format)
        ));
    }
    out.push_str(&format!("<{p}ptCount val=\"{}\"/>", values.len()));
    for (index, value) in values.iter().enumerate() {
        let Some(value) = value else { continue };
        // `f64`'s own `Display` is the shortest text that reads back as the
        // same number, which is the rule DocMark already writes them by.
        out.push_str(&format!(
            "<{p}pt idx=\"{index}\"><{p}v>{value}</{p}v></{p}pt>"
        ));
    }
    out.push_str(&ext_list(cache, source));
    out.push_str(&format!("</{p}{}>", cache.name));
    out
}

/// The edit that makes a series' `c:tx` say the model's name.
///
/// Three shapes, because a chart part has three ways of naming a series: a
/// reference with a cache, a literal `c:v`, and nothing at all. A name the
/// model dropped takes the whole optional `c:tx` with it; a name the model
/// added is written as a **literal** `c:v`, which is what the format is for
/// when there is no cell behind the name — inventing a `c:f` would claim a
/// provenance nobody stated.
fn name_edit(
    ser: &Element,
    source: &str,
    name: Option<&str>,
) -> Result<(Range<usize>, String), String> {
    let p = prefix(ser);
    match (ser.child("tx"), name) {
        (Some(tx), None) => Ok((tx.span.clone(), String::new())),
        (Some(tx), Some(name)) => {
            if let Some(cache) = cache_of(tx) {
                return Ok((
                    cache.span.clone(),
                    str_cache(cache, source, &[name.to_string()]),
                ));
            }
            if let Some(v) = tx.child("v") {
                let p = prefix(v);
                return Ok((v.span.clone(), format!("<{p}v>{}</{p}v>", esc_text(name))));
            }
            Err("which the part states in a way this writer cannot read back".into())
        }
        (None, Some(name)) => {
            // `c:ser` is `c:idx, c:order, c:tx?, …`: the one legal place for a
            // `c:tx` that is not there is right after `c:order`.
            let after = ser
                .child("order")
                .or_else(|| ser.child("idx"))
                .ok_or_else(|| {
                    "which the part has nowhere to state, its series naming neither an \
                     index nor an order"
                        .to_string()
                })?;
            let at = after.span.end;
            Ok((
                at..at,
                format!("<{p}tx><{p}v>{}</{p}v></{p}tx>", esc_text(name)),
            ))
        }
        (None, None) => Err("which neither side states".into()),
    }
}

/// The edit that makes a `c:cat`'s or a `c:val`'s `c:f` cover `count` points.
///
/// **`c:f` is restated rather than left stale.** The formula is the range the
/// cache was computed from, and a cache one point longer than the range it
/// names is a file that says two things about the same series — PowerPoint
/// re-reads the range when a user opens «Edit Data», so a stale one silently
/// truncates the very edit that was just made. Restating it needs no more than
/// what 16-A's reader already understands: a sheet name and an A1 range, with
/// the `$` anchors and the sheet's own spelling kept exactly as they were, and
/// only the end of the range moved along the axis the range already runs on.
///
/// `None` when the reference states no formula (a literal `c:strLit` has none,
/// and there is nothing to keep consistent), `Err` when the range is one this
/// writer will not restate — a rectangle, a 3-D reference, a defined name.
#[allow(clippy::type_complexity)]
fn formula_edit(
    holder: Option<&Element>,
    count: usize,
) -> Option<Result<(Range<usize>, String), String>> {
    let reference = holder?
        .children()
        .find(|c| matches!(c.name.as_str(), "strRef" | "numRef"))?;
    let f = reference.child("f")?;
    let stated = f.deep_text();
    Some(match restate(&stated, count) {
        // The range already covers what the cache now holds: nothing to say and
        // nothing to write.
        Some(restated) if restated == stated => return None,
        Some(restated) => {
            let p = prefix(f);
            Ok((
                f.span.clone(),
                format!("<{p}f>{}</{p}f>", esc_text(&restated)),
            ))
        }
        None => Err(format!(
            "which the part states as `{stated}` and this writer will not restate: the \
             cache now holds {count} point(s) and the range would say otherwise"
        )),
    })
}

/// `Hoja1!$B$2:$B$4` covering `count` points instead of the three it covers.
fn restate(formula: &str, count: usize) -> Option<String> {
    if count == 0 {
        return None;
    }
    let (sheet, range) = formula.rsplit_once('!')?;
    if sheet.trim().is_empty() {
        return None;
    }
    let (first, last) = match range.split_once(':') {
        Some((first, last)) => (first, last),
        None => (range, range),
    };
    let (start, end) = (CellRef::parse_a1(first)?, CellRef::parse_a1(last)?);
    let count = u32::try_from(count).ok()?;
    let wanted = if start.col == end.col {
        CellRef::new(start.col, start.row.checked_add(count - 1)?)
    } else if start.row == end.row {
        CellRef::new(start.col.checked_add(count - 1)?, start.row)
    } else {
        // A rectangle is not a series' range in any chart this reader
        // understands, and guessing which way to grow it is guessing.
        return None;
    };
    let last = anchored(wanted, last);
    Some(if wanted == start {
        format!("{sheet}!{first}")
    } else {
        format!("{sheet}!{first}:{last}")
    })
}

/// A cell written the way the token it replaces was written: `$B$4` stays
/// anchored, `B4` stays relative, and a mixed `$B4` keeps both halves.
fn anchored(cell: CellRef, like: &str) -> String {
    let a1 = cell.a1();
    let split = a1.find(|c: char| c.is_ascii_digit()).unwrap_or(a1.len());
    let (letters, digits) = a1.split_at(split);
    let column_anchor = like.starts_with('$');
    let row_anchor = like[1.min(like.len())..].contains('$');
    format!(
        "{}{letters}{}{digits}",
        if column_anchor { "$" } else { "" },
        if row_anchor { "$" } else { "" }
    )
}

/// The first pair of edits that overlap, or `None` when every one is disjoint.
fn overlapping(edits: &[(Range<usize>, String)]) -> Option<String> {
    let mut sorted: Vec<&Range<usize>> = edits.iter().map(|(range, _)| range).collect();
    sorted.sort_by_key(|range| (range.start, range.end));
    for pair in sorted.windows(2) {
        if pair[1].start < pair[0].end {
            return Some(format!(
                "two of the edits it needs overlap ({}..{} and {}..{})",
                pair[0].start, pair[0].end, pair[1].start, pair[1].end
            ));
        }
    }
    None
}

/// A whole `c:chartSpace` written from the model, for a slide this writer
/// generates.
///
/// Returns the part's XML and, when the kind had to be chosen rather than
/// recovered, the clause a warning carries. The element order is the schema's,
/// stated here rather than discovered in `corpus/schema_gate.py`:
/// `c:chartSpace` is `c:chart, c:spPr?, c:txPr?, c:externalData?, …`; `c:chart`
/// is `c:title?, c:autoTitleDeleted?, …, c:plotArea, c:legend?, c:plotVisOnly?,
/// …`; a plot is its own leading children, then `c:ser*`, then `c:axId`.
pub(super) fn generated(chart: &ChartRef) -> (String, Vec<String>) {
    let mut kept = Vec::new();
    let stated = chart.kind.as_deref().unwrap_or_default();
    let (plot, leading, axes) = match PLOTS.iter().find(|(name, _, _)| *name == stated) {
        Some((name, leading, axes)) => (*name, *leading, *axes),
        None => {
            let (name, leading, axes) = PLOTS[0];
            kept.push(match chart.kind.as_deref() {
                Some(kind) => format!(
                    "its kind, `{kind}`, which this writer cannot generate: the series were \
                     plotted as a `{name}` instead"
                ),
                None => format!(
                    "its kind, which the document does not state: the series were plotted \
                     as a `{name}`"
                ),
            });
            (name, leading, axes)
        }
    };
    // Said for **every** generated chart, because it is true of every one: what
    // the model holds is the categories, the series and the kind of the first
    // plot, and a chart is a great deal more than that. This is not the kind
    // fall-back above — a `barChart` whose kind was recovered exactly still
    // comes out with none of its own design.
    kept.push(format!(
        "its chart's design: what a generated chart carries is the plot, the axes and the \
         model's cached series, so its colours, its legend, its data labels and its \
         gridlines are whatever the consumer draws by default — and a chart that \
         combined more than one kind of plot comes back as a single `{plot}`, because \
         the model records which kind a chart is and not which plot each series was in"
    ));

    let mut series = String::new();
    for (index, one) in chart.series.iter().enumerate() {
        series.push_str(&format!(
            "<c:ser><c:idx val=\"{index}\"/><c:order val=\"{index}\"/>"
        ));
        if let Some(name) = &one.name {
            series.push_str(&format!("<c:tx><c:v>{}</c:v></c:tx>", esc_text(name)));
        }
        if !chart.categories.is_empty() {
            series.push_str("<c:cat><c:strLit>");
            series.push_str(&format!("<c:ptCount val=\"{}\"/>", chart.categories.len()));
            for (at, label) in chart.categories.iter().enumerate() {
                if label.is_empty() {
                    continue;
                }
                series.push_str(&format!(
                    "<c:pt idx=\"{at}\"><c:v>{}</c:v></c:pt>",
                    esc_text(label)
                ));
            }
            series.push_str("</c:strLit></c:cat>");
        }
        series.push_str("<c:val><c:numLit>");
        if let Some(format) = &one.number_format {
            series.push_str(&format!(
                "<c:formatCode>{}</c:formatCode>",
                esc_text(format)
            ));
        }
        series.push_str(&format!("<c:ptCount val=\"{}\"/>", one.values.len()));
        for (at, value) in one.values.iter().enumerate() {
            let Some(value) = value else { continue };
            series.push_str(&format!("<c:pt idx=\"{at}\"><c:v>{value}</c:v></c:pt>"));
        }
        series.push_str("</c:numLit></c:val></c:ser>");
    }

    let title = match &chart.title {
        Some(title) => format!(
            "<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>{}</a:t></a:r></a:p>\
             </c:rich></c:tx><c:overlay val=\"0\"/></c:title><c:autoTitleDeleted val=\"0\"/>",
            esc_text(title)
        ),
        None => "<c:autoTitleDeleted val=\"1\"/>".to_string(),
    };
    let (ids, ax) = if axes {
        (
            "<c:axId val=\"111111111\"/><c:axId val=\"222222222\"/>",
            "<c:catAx><c:axId val=\"111111111\"/>\
             <c:scaling><c:orientation val=\"minMax\"/></c:scaling>\
             <c:delete val=\"0\"/><c:axPos val=\"b\"/><c:crossAx val=\"222222222\"/></c:catAx>\
             <c:valAx><c:axId val=\"222222222\"/>\
             <c:scaling><c:orientation val=\"minMax\"/></c:scaling>\
             <c:delete val=\"0\"/><c:axPos val=\"l\"/><c:crossAx val=\"111111111\"/></c:valAx>",
        )
    } else {
        ("", "")
    };

    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n\
         <c:chartSpace \
         xmlns:c=\"{CHART_URI}\" \
         xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
         <c:chart>{title}<c:plotArea><c:layout/>\
         <c:{plot}>{leading}{series}{ids}</c:{plot}>{ax}</c:plotArea>\
         <c:plotVisOnly val=\"1\"/><c:dispBlanksAs val=\"gap\"/></c:chart></c:chartSpace>"
    );
    (xml, kept)
}

/// The `p:graphicFrame` that shows a generated chart, pointing at the part
/// through the relationship the caller minted.
pub(super) fn frame_xml(name: &str, id: u32, xfrm: &str, rel: &str) -> String {
    format!(
        "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"{}\"/>\
         <p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>{xfrm}\
         <a:graphic><a:graphicData uri=\"{CHART_URI}\">\
         <c:chart xmlns:c=\"{CHART_URI}\" r:id=\"{}\"/>\
         </a:graphicData></a:graphic></p:graphicFrame>",
        super::write::esc_attr(name),
        super::write::esc_attr(rel),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_is_restated_along_the_axis_it_already_runs_on() {
        assert_eq!(
            restate("Hoja1!$B$2:$B$4", 5).as_deref(),
            Some("Hoja1!$B$2:$B$6"),
            "a column grows down, keeping its anchors and its sheet"
        );
        assert_eq!(
            restate("Hoja1!$B$2:$B$4", 1).as_deref(),
            Some("Hoja1!$B$2"),
            "one point is one cell, not a range of one"
        );
        assert_eq!(
            restate("Hoja1!B2:D2", 2).as_deref(),
            Some("Hoja1!B2:C2"),
            "a row shrinks across, and a relative token stays relative"
        );
        assert_eq!(
            restate("'My Sheet'!$B$1", 3).as_deref(),
            Some("'My Sheet'!$B$1:$B$3"),
            "the sheet's own spelling is kept exactly as it was"
        );
        assert_eq!(
            restate("Hoja1!$A$1:$B$4", 5),
            None,
            "a rectangle is refused"
        );
        assert_eq!(restate("SomeName", 2), None);
        assert_eq!(
            restate("Hoja1!$B$2:$B$4", 0),
            None,
            "an empty range is not one"
        );
    }

    #[test]
    fn a_hostile_label_reaches_the_part_as_text_and_not_as_markup() {
        let cache = Element::parse(
            "chart.xml",
            br#"<c:strCache xmlns:c="c"><c:ptCount val="0"/></c:strCache>"#,
        )
        .expect("parses");
        let written = str_cache(&cache, "", &["A & B <x>".to_string(), String::new()]);
        assert!(
            written.contains("<c:v>A &amp; B &lt;x&gt;</c:v>"),
            "got {written}"
        );
        assert!(
            !written.contains("<c:pt idx=\"1\""),
            "an empty label is an absent point, not an empty one: {written}"
        );
        let back = Element::parse("chart.xml", written.as_bytes()).expect("parses back");
        assert_eq!(
            labels(&back),
            vec!["A & B <x>".to_string(), String::new()],
            "and it reads back as what was written"
        );
    }

    #[test]
    fn a_gap_is_an_absent_point_and_never_a_zero() {
        let cache = Element::parse(
            "chart.xml",
            br#"<c:numCache xmlns:c="c"><c:ptCount val="0"/></c:numCache>"#,
        )
        .expect("parses");
        let written = num_cache(&cache, "", &[Some(1.5), None, Some(3.0)], Some("#,##0"));
        assert!(written.contains("<c:formatCode>#,##0</c:formatCode>"));
        assert!(written.contains("<c:ptCount val=\"3\"/>"));
        assert!(written.contains("<c:pt idx=\"0\"><c:v>1.5</c:v></c:pt>"));
        assert!(written.contains("<c:pt idx=\"2\"><c:v>3</c:v></c:pt>"));
        assert!(
            !written.contains("idx=\"1\""),
            "the hole stays a hole: {written}"
        );
    }

    fn ser(xml: &str) -> Element {
        Element::parse("chart.xml", xml.as_bytes()).expect("parses")
    }

    /// A chart part has three ways of naming a series, and a fourth of not
    /// naming it. Each has its own edit, and none of them is a rewritten
    /// `c:ser`.
    #[test]
    fn a_series_is_renamed_the_way_its_own_part_names_it() {
        let cached = ser(r#"<c:ser xmlns:c="c"><c:idx val="0"/><c:order val="0"/>
               <c:tx><c:strRef><c:f>Hoja1!$B$1</c:f><c:strCache><c:ptCount val="1"/>
               <c:pt idx="0"><c:v>Ventas</c:v></c:pt></c:strCache></c:strRef></c:tx></c:ser>"#);
        let (range, text) = name_edit(&cached, "", Some("Costes")).expect("an edit");
        assert!(text.contains("<c:v>Costes</c:v>") && text.starts_with("<c:strCache"));
        assert_eq!(
            range,
            cache_of(cached.child("tx").unwrap()).unwrap().span,
            "the cache is the unit, not the `c:tx` and not the `c:f`"
        );

        let literal = ser(r#"<c:ser xmlns:c="c"><c:idx val="0"/><c:order val="0"/>
               <c:tx><c:v>Ventas</c:v></c:tx></c:ser>"#);
        let (_, text) = name_edit(&literal, "", Some("Costes")).expect("an edit");
        assert_eq!(text, "<c:v>Costes</c:v>");

        // A name the model dropped takes the optional `c:tx` with it.
        let (range, text) = name_edit(&literal, "", None).expect("an edit");
        assert_eq!(range, literal.child("tx").unwrap().span);
        assert!(text.is_empty());

        // A name the model added, on a series that never had one: a **literal**
        // `c:v`, right after `c:order`, because there is no cell behind it and
        // inventing a `c:f` would claim a provenance nobody stated.
        let bare = ser(r#"<c:ser xmlns:c="c"><c:idx val="0"/><c:order val="0"/></c:ser>"#);
        let (range, text) = name_edit(&bare, "", Some("Costes")).expect("an edit");
        assert!(range.is_empty(), "an insertion, not a replacement");
        assert_eq!(range.start, bare.child("order").unwrap().span.end);
        assert_eq!(text, "<c:tx><c:v>Costes</c:v></c:tx>");
    }

    #[test]
    fn overlapping_edits_are_a_defect_this_writer_refuses_to_commit() {
        assert!(overlapping(&[(0..4, String::new()), (4..8, String::new())]).is_none());
        assert!(overlapping(&[(0..5, String::new()), (4..8, String::new())]).is_some());
    }
}

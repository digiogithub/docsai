//! What a chart plots: title, categories, series — and where the numbers came
//! from (Phase 16, increment 16-A).
//!
//! A chart carries its data **twice**. `c:strCache` and `c:numCache` inside
//! `ppt/charts/chartN.xml` hold what the slide displays; `ppt/embeddings/…`
//! holds the workbook a user opens under «Edit Data», reached through the chart
//! part's *own* relationships — one level further in than the frame's `r:id`,
//! which resolves against the slide.
//!
//! **The cache is authoritative for the IR.** It is present in every valid
//! chart, it is what PowerPoint draws without opening anything else, and a
//! chart whose workbook is linked, missing or unreadable still has to read. The
//! workbook is read *as well*, for provenance: its bytes become an asset, and
//! its cells are compared against the caches. A disagreement between the two is
//! a real property of the file — it means someone edited the sheet and never
//! let the chart redraw — so it is **warned, never averaged**.
//!
//! Nothing here rewrites the chart part. `raw` still captures `c:chartSpace`
//! verbatim and the skeleton still preserves `ppt/charts/*` byte for byte:
//! reading a chart as data must not change one byte of a written deck.

use std::io::Cursor;

use docsai_model::assets::{AssetId, AssetStore, MemoryAssetStore};
use docsai_model::presentation::{ChartRef, ChartSeries};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::sheet::{CellRef, CellValue, Workbook};
use docsai_model::Document;

use crate::package::Package;
use crate::xml::Element;

use super::cascade::LevelStyles;
use super::text::{self, TextCtx};
use super::{parse, raw, SlideCtx};

/// The relationship type of an embedded package (`…/relationships/package`),
/// as [`crate::package::Relationships`] keys it: by its last segment.
const PACKAGE_REL: &str = "package";

/// A cache no bigger than this is read. A `c:ptCount` is a number in a file and
/// a file can lie; a hundred thousand points is already three orders of
/// magnitude past what a slide can show.
const MAX_POINTS: usize = 100_000;

/// A workbook range no wider than this is compared against a cache. Beyond it
/// the comparison would cost more than the provenance is worth, and the caches
/// — which are the IR — are unaffected either way.
const MAX_COMPARED_CELLS: usize = 4_096;

/// The chart a graphic frame holds: what kind it is, what it plots, and its
/// markup.
///
/// `None` when the frame's `r:id` leads nowhere — the caller then keeps the
/// frame as a raw stub, which is a chart nobody can read but nobody loses
/// either.
pub(super) fn read(
    data: Option<&Element>,
    ctx: &SlideCtx<'_>,
    assets: &mut dyn AssetStore,
    sink: &mut raw::Sink,
    report: &mut ConversionReport,
) -> Option<ChartRef> {
    let rel_id = data?.child("chart")?.attr_qualified("r:id")?;
    let rel = ctx.rels.get(rel_id).filter(|rel| !rel.external)?;
    let part = rel.target.clone();
    let (root, source) = match (parse(ctx.package, &part), ctx.package.text(&part)) {
        (Ok(root), Ok(source)) => (root, source),
        _ => {
            report.warn(Warning::Degraded {
                what: format!("chart `{part}`"),
                why: "the chart part could not be read; the frame is kept as a stub".into(),
            });
            return None;
        }
    };
    // `c:barChart`, `c:lineChart`… the first plot in the plot area names the
    // chart, and a combo chart is named by its first plot rather than by a
    // guess at which one matters.
    let kind = root.path(&["chart", "plotArea"]).and_then(|area| {
        area.children()
            .find(|child| child.name.ends_with("Chart"))
            .map(|child| child.name.clone())
    });

    let title = read_title(&root, ctx, report);
    let plotted = read_plots(&root, &part, report);
    let stored = read_workbook(&part, ctx, assets, report);

    // Provenance, not data: what the workbook says is compared against the
    // caches, and the caches win.
    if let Some((_, bytes)) = stored.as_ref() {
        compare_with_workbook(&part, bytes, &plotted.refs, report);
    }
    let workbook = stored.map(|(id, _)| id);

    Some(ChartRef {
        kind,
        title,
        workbook,
        categories: plotted.categories,
        series: plotted.series,
        raw: Some(sink.capture(&root, &part, source, report)),
    })
}

/// `c:chart/c:title` as plain text.
///
/// The title is a DrawingML text body — the same `a:p` / `a:r` / `a:t` tree a
/// text box holds — so it is read with the reader the crate already has rather
/// than a second walker. A title that comes from a cell (`c:strRef`) is taken
/// from its cache, on the same rule as the series.
fn read_title(root: &Element, ctx: &SlideCtx<'_>, report: &mut ConversionReport) -> Option<String> {
    let tx = root.path(&["chart", "title", "tx"])?;

    let text = if let Some(rich) = tx.child("rich") {
        let inherited = LevelStyles::default();
        let text_ctx = TextCtx {
            rels: ctx.rels,
            // A chart title is one line, never a bulleted list.
            bulleted: false,
            theme: ctx.cascade.theme_of(ctx.layout),
            inherited: &inherited,
        };
        let blocks = text::read_body(rich, &text_ctx, report);
        blocks
            .iter()
            .filter_map(|block| match block {
                docsai_model::text::Block::Paragraph(p) => Some(p.plain_text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        // `c:strRef` — the title is a cell — or a bare `c:v`.
        cache_of(tx)
            .map(|cache| points(cache).into_iter().flatten().collect::<Vec<_>>())
            .map(|values| values.join(" "))
            .or_else(|| tx.child("v").map(|v| v.deep_text()))?
    };

    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The categories and series of every plot, plus what each was read from.
#[derive(Default)]
struct Plotted {
    categories: Vec<String>,
    series: Vec<ChartSeries>,
    refs: Vec<SourceRef>,
}

/// One `c:f` and the cache beside it: the pair the workbook is checked against.
struct SourceRef {
    /// What this is, for the warning: `series 1 values`.
    what: String,
    /// The formula the reference names, verbatim (`Hoja1!$B$2:$B$4`).
    formula: String,
    /// The cached points, as the file writes them. `None` is a gap.
    cached: Vec<Option<String>>,
    /// True for a `c:numCache`: the comparison is numeric rather than textual.
    numeric: bool,
}

/// Every `c:ser` of every plot, in document order.
///
/// A combo chart has more than one plot element and its series are numbered
/// across all of them, which is also how the legend numbers them. The
/// categories are the chart's, not a series': `c:cat` repeats the same
/// reference on every series, and the first one that carries points is the
/// axis for all of them.
fn read_plots(root: &Element, part: &str, report: &mut ConversionReport) -> Plotted {
    let mut out = Plotted::default();
    let Some(area) = root.path(&["chart", "plotArea"]) else {
        return out;
    };
    let mut unreadable = 0usize;

    for plot in area.children().filter(|c| c.name.ends_with("Chart")) {
        for ser in plot.children_named("ser") {
            let index = out.series.len() + 1;

            let mut series = ChartSeries::default();
            if let Some(tx) = ser.child("tx") {
                series.name = read_name(tx);
                if let Some(source) = source_ref(tx, format!("series {index} name"), false) {
                    out.refs.push(source);
                }
            }

            if let Some(cat) = ser.child("cat") {
                if cat.child("multiLvlStrRef").is_some() {
                    // A two-level category axis ("2024 / Q1") is a tree, and
                    // `ChartRef::categories` is a list. Warned, not guessed at.
                    report.warn(Warning::Degraded {
                        what: format!("chart `{part}` series {index}"),
                        why: "its categories are multi-level (`c:multiLvlStrRef`) and the model \
                              holds one level; the labels are not read"
                            .into(),
                    });
                } else if let Some(cache) = cache_of(cat) {
                    if out.categories.is_empty() {
                        out.categories = points(cache)
                            .into_iter()
                            .map(Option::unwrap_or_default)
                            .collect();
                    }
                    if let Some(source) =
                        source_ref(cat, format!("series {index} categories"), false)
                    {
                        out.refs.push(source);
                    }
                }
            }

            if let Some(val) = ser.child("val") {
                if let Some(cache) = cache_of(val) {
                    series.number_format = cache
                        .child("formatCode")
                        .map(|f| f.deep_text())
                        .filter(|f| !f.is_empty());
                    for point in points(cache) {
                        match point {
                            None => series.values.push(None),
                            Some(text) => match text.trim().parse::<f64>() {
                                Ok(number) if number.is_finite() => {
                                    series.values.push(Some(number))
                                }
                                // `#N/A` is what PowerPoint caches for a cell it
                                // could not evaluate. It is a gap, and it is a
                                // gap this reader does not pretend is a zero.
                                _ => {
                                    unreadable += 1;
                                    series.values.push(None);
                                }
                            },
                        }
                    }
                }
                if let Some(source) = source_ref(val, format!("series {index} values"), true) {
                    out.refs.push(source);
                }
            }

            out.series.push(series);
        }
    }

    if unreadable > 0 {
        report.warn(Warning::Degraded {
            what: format!("chart `{part}`"),
            why: format!(
                "{unreadable} cached value(s) are not numbers (`#N/A` and the like) and are \
                 kept as gaps; the chart markup still has them"
            ),
        });
    }
    out
}

/// The legend name of a series: the cache of its `c:tx`, or a literal `c:v`.
fn read_name(tx: &Element) -> Option<String> {
    let text = match cache_of(tx) {
        Some(cache) => points(cache)
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" "),
        None => tx.child("v")?.deep_text(),
    };
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The cache under a `c:tx`, `c:cat` or `c:val`.
///
/// A reference (`c:strRef`, `c:numRef`) carries its cache as a child; a literal
/// (`c:strLit`, `c:numLit`) *is* the cache and has no formula. Both hold `c:pt`
/// the same way, which is why one function serves them.
fn cache_of(holder: &Element) -> Option<&Element> {
    for child in holder.children() {
        match child.name.as_str() {
            "strRef" | "numRef" => {
                return child.children().find(|c| c.name.ends_with("Cache"));
            }
            "strLit" | "numLit" => return Some(child),
            _ => {}
        }
    }
    None
}

/// The `c:f` beside a cache, with the cache's points, when there is one to
/// compare. A literal has no source to disagree with.
fn source_ref(holder: &Element, what: String, numeric: bool) -> Option<SourceRef> {
    let reference = holder
        .children()
        .find(|c| matches!(c.name.as_str(), "strRef" | "numRef"))?;
    let formula = reference.child("f")?.deep_text();
    let cache = reference.children().find(|c| c.name.ends_with("Cache"))?;
    Some(SourceRef {
        what,
        formula,
        cached: points(cache),
        numeric,
    })
}

/// A cache's points, in axis order, with the gaps kept.
///
/// `c:ptCount` is the **length of the column**, not the number of `c:pt`
/// elements: the indices are sparse, and a series that skips a category is
/// written as two points with a hole between them. A reader that pushed the
/// points it found would shift every value after the hole onto the wrong
/// category.
fn points(cache: &Element) -> Vec<Option<String>> {
    let count = cache
        .child("ptCount")
        .and_then(|c| c.attr_i64("val"))
        .filter(|n| *n >= 0)
        .map(|n| (n as usize).min(MAX_POINTS))
        .unwrap_or(0);
    let mut out = vec![None; count];
    for point in cache.children_named("pt") {
        let Some(index) = point
            .attr_i64("idx")
            .filter(|n| *n >= 0 && (*n as usize) < MAX_POINTS)
            .map(|n| n as usize)
        else {
            continue;
        };
        // A `c:ptCount` that undercounts its own points is malformed, and the
        // points are the data: the column grows rather than the value being
        // dropped.
        if index >= out.len() {
            out.resize(index + 1, None);
        }
        out[index] = Some(point.child("v").map(|v| v.deep_text()).unwrap_or_default());
    }
    out
}

/// Stores the chart's embedded workbook and returns its asset id and its
/// bytes.
///
/// Absent or external is a [`Warning::Degraded`]: the chart's data is complete
/// without it — the caches are the data — and only its *provenance* is missing.
/// A workbook that is *there* and cannot be read or stored is a
/// [`Warning::AssetIssue`], which is what every other reader in this crate says
/// about a part it holds and cannot keep.
fn read_workbook<'a>(
    chart_part: &str,
    ctx: &SlideCtx<'a>,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Option<(AssetId, &'a [u8])> {
    let rels = ctx.package.relationships(chart_part);
    let Some(rel) = rels.of_kind(PACKAGE_REL).next() else {
        report.warn(Warning::Degraded {
            what: format!("chart `{chart_part}`"),
            why: "it embeds no workbook, so the cached series are all there is to say about \
                  where its numbers came from"
                .into(),
        });
        return None;
    };
    if rel.external {
        report.warn(Warning::Degraded {
            what: format!("chart `{chart_part}`"),
            why: format!(
                "its workbook is linked, not embedded (`{}`), and linked content is never \
                 fetched; the cached series are what the chart plots",
                rel.target
            ),
        });
        return None;
    }

    let Some(bytes) = ctx.package.part(&rel.target) else {
        report.warn(Warning::AssetIssue {
            asset: rel.target.clone(),
            why: format!("the workbook `{chart_part}` relates to is not in the package"),
        });
        return None;
    };
    match assets.put(bytes) {
        Ok(id) => Some((id, bytes)),
        Err(error) => {
            report.warn(Warning::AssetIssue {
                asset: rel.target.clone(),
                why: format!("the chart's embedded workbook could not be stored: {error}"),
            });
            None
        }
    }
}

/// Compares the embedded workbook against the caches, and says so when they
/// differ.
///
/// Silence is a claim here: no warning means the two copies agree, so anything
/// this function cannot compare is skipped rather than reported as a
/// difference. What it does compare is every `c:f` the chart states, cell by
/// cell, against the sheet it names.
fn compare_with_workbook(
    chart_part: &str,
    bytes: &[u8],
    refs: &[SourceRef],
    report: &mut ConversionReport,
) {
    let workbook = match Package::open(Cursor::new(bytes))
        .map_err(|e| e.to_string())
        .and_then(|package| {
            // Its own store: a picture anchored in the chart's workbook is not
            // a picture of this deck.
            let mut inner = MemoryAssetStore::new();
            crate::xlsx::read_package(&package, &mut inner).map_err(|e| e.to_string())
        }) {
        // The inner report is about the inner workbook — its macros, its
        // styles. Those are not losses in this deck's IR, and merging them
        // would attribute them to the slide.
        Ok((Document::Workbook(workbook), _)) => workbook,
        Ok(_) => return,
        Err(error) => {
            report.warn(Warning::AssetIssue {
                asset: format!("the workbook embedded in `{chart_part}`"),
                why: format!(
                    "it could not be read, so the chart's cached series could not be checked \
                     against it: {error}"
                ),
            });
            return;
        }
    };

    for source in refs {
        if let Some(difference) = difference(&workbook, source) {
            report.warn(Warning::Degraded {
                what: format!("chart `{chart_part}`"),
                why: format!(
                    "its embedded workbook and the data cached in the chart disagree \
                     ({difference}); the cache is what the slide shows and what the model \
                     holds"
                ),
            });
        }
    }
}

/// The first disagreement between one cached reference and the workbook, or
/// `None` when they agree — or when the comparison cannot be made at all.
fn difference(workbook: &Workbook, source: &SourceRef) -> Option<String> {
    let (sheet_name, cells) = parse_reference(&source.formula)?;
    let what = &source.what;
    let Some(sheet) = workbook
        .sheets
        .iter()
        .find(|sheet| sheet.name == sheet_name)
        .or_else(|| {
            workbook
                .sheets
                .iter()
                .find(|sheet| sheet.name.eq_ignore_ascii_case(&sheet_name))
        })
    else {
        return Some(format!(
            "{what} names sheet `{sheet_name}`, which the workbook does not have"
        ));
    };

    if cells.len() > MAX_COMPARED_CELLS {
        return None;
    }
    if cells.len() != source.cached.len() {
        return Some(format!(
            "{what} covers {} cell(s) and the cache holds {} point(s)",
            cells.len(),
            source.cached.len()
        ));
    }

    for (index, cell) in cells.iter().enumerate() {
        let stored = sheet.cells.get(cell).map(|cell| &cell.value);
        let cached = source.cached[index].as_deref();
        if source.numeric {
            let Some(stored) = numeric(stored) else {
                continue;
            };
            let cached = cached.and_then(|text| text.trim().parse::<f64>().ok());
            if !same_number(stored, cached) {
                return Some(format!(
                    "{what} point {index} is {} in the workbook (`{}`) and {} in the cache",
                    show_number(stored),
                    cell.a1(),
                    show_number(cached),
                ));
            }
        } else {
            let Some(stored) = textual(stored) else {
                continue;
            };
            let cached = cached.unwrap_or_default();
            if stored != cached {
                return Some(format!(
                    "{what} point {index} is `{stored}` in the workbook (`{}`) and `{cached}` \
                     in the cache",
                    cell.a1()
                ));
            }
        }
    }
    None
}

/// A cell as a number, or `None` when it is not one this comparison can judge.
///
/// The outer `Option` is «comparable»; the inner one is «has a value». A date
/// is deliberately not comparable: the xlsx reader turns a date-formatted
/// serial into an ISO string and the chart caches the serial, so comparing them
/// would report a difference that is not there — the false warning rule 3
/// forbids.
fn numeric(value: Option<&CellValue>) -> Option<Option<f64>> {
    match value {
        None | Some(CellValue::Empty) => Some(None),
        Some(CellValue::Number(n)) => Some(Some(*n)),
        Some(CellValue::Text(text)) => text.trim().parse::<f64>().ok().map(Some),
        _ => None,
    }
}

/// A cell as a label, on the same «comparable» convention as [`numeric`].
fn textual(value: Option<&CellValue>) -> Option<String> {
    match value {
        None | Some(CellValue::Empty) => Some(String::new()),
        Some(CellValue::Text(text)) => Some(text.clone()),
        Some(CellValue::Number(n)) => Some(show_number(Some(*n))),
        Some(CellValue::Bool(b)) => Some(b.to_string()),
        _ => None,
    }
}

/// Equality with room for the last bit: the two copies are the *same* number
/// written twice, once by the sheet and once by the cache, and a reader that
/// warned about `0.1 + 0.2` would be crying wolf.
fn same_number(left: Option<f64>, right: Option<f64>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0)
        }
        _ => false,
    }
}

fn show_number(value: Option<f64>) -> String {
    match value {
        Some(number) => format!("{number}"),
        None => "empty".into(),
    }
}

/// `Hoja1!$B$2:$B$4` into the sheet it names and the cells it covers, in the
/// order a chart plots them: down a column, then across.
///
/// A 3-D reference (`Sheet1:Sheet3!A1`), a defined name or a formula this does
/// not recognise returns `None`, and nothing is claimed about it.
fn parse_reference(formula: &str) -> Option<(String, Vec<CellRef>)> {
    let (sheet, range) = formula.rsplit_once('!')?;
    let sheet = sheet.trim();
    let sheet = match sheet.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        // A quoted sheet name escapes its own quote by doubling it.
        Some(quoted) => quoted.replace("''", "'"),
        None => sheet.to_string(),
    };
    if sheet.is_empty() || sheet.contains(':') {
        return None;
    }

    let (start, end) = match range.split_once(':') {
        Some((start, end)) => (CellRef::parse_a1(start)?, CellRef::parse_a1(end)?),
        None => {
            let single = CellRef::parse_a1(range)?;
            (single, single)
        }
    };
    let (top, bottom) = (start.row.min(end.row), start.row.max(end.row));
    let (left, right) = (start.col.min(end.col), start.col.max(end.col));
    let width = (right - left) as usize + 1;
    let height = (bottom - top) as usize + 1;
    if width.saturating_mul(height) > MAX_COMPARED_CELLS {
        // Reported as an over-wide range rather than parsed into a huge vector.
        return Some((sheet, vec![CellRef::new(left, top); MAX_COMPARED_CELLS + 1]));
    }

    // A chart series is a column of cells far more often than a row, and both
    // are one-dimensional; column-major keeps a column in axis order and leaves
    // a row unchanged.
    let mut cells = Vec::with_capacity(width * height);
    for col in left..=right {
        for row in top..=bottom {
            cells.push(CellRef::new(col, row));
        }
    }
    Some((sheet, cells))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(xml: &str) -> Element {
        Element::parse("test.xml", xml.as_bytes()).expect("parses")
    }

    #[test]
    fn a_sparse_cache_keeps_its_gaps_where_the_indices_put_them() {
        let cache = element(
            r#"<c:numCache xmlns:c="c"><c:ptCount val="4"/>
               <c:pt idx="0"><c:v>1</c:v></c:pt>
               <c:pt idx="3"><c:v>4</c:v></c:pt></c:numCache>"#,
        );
        assert_eq!(
            points(&cache),
            vec![Some("1".into()), None, None, Some("4".into())],
            "`c:ptCount` is the length of the column, not the number of points"
        );
    }

    #[test]
    fn a_point_beyond_the_declared_count_grows_the_column_rather_than_vanishing() {
        let cache = element(
            r#"<c:numCache xmlns:c="c"><c:ptCount val="1"/>
               <c:pt idx="2"><c:v>9</c:v></c:pt></c:numCache>"#,
        );
        assert_eq!(points(&cache), vec![None, None, Some("9".into())]);
    }

    #[test]
    fn a_reference_resolves_to_the_cells_it_covers_in_axis_order() {
        let (sheet, cells) = parse_reference("Hoja1!$B$2:$B$4").expect("parses");
        assert_eq!(sheet, "Hoja1");
        assert_eq!(
            cells.iter().map(CellRef::a1).collect::<Vec<_>>(),
            ["B2", "B3", "B4"]
        );

        let (sheet, cells) = parse_reference("'My Sheet'!$B$1").expect("parses");
        assert_eq!(sheet, "My Sheet");
        assert_eq!(cells.iter().map(CellRef::a1).collect::<Vec<_>>(), ["B1"]);

        assert!(
            parse_reference("Sheet1:Sheet3!A1").is_none(),
            "a 3-D reference is not resolved, and nothing is claimed about it"
        );
        assert!(parse_reference("SomeDefinedName").is_none());
    }

    #[test]
    fn a_date_cell_is_not_comparable_rather_than_different() {
        assert_eq!(numeric(Some(&CellValue::Number(3.5))), Some(Some(3.5)));
        assert_eq!(numeric(None), Some(None));
        assert_eq!(
            numeric(Some(&CellValue::DateTime("2026-01-01".into()))),
            None,
            "the sheet has an ISO string where the cache has a serial: not a disagreement"
        );
    }
}

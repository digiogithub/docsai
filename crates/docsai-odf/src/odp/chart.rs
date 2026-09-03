//! An embedded ODF chart sub-document (`draw:object` → `Object 1/content.xml`).
//!
//! The structural difference from OOXML is the whole of this module's reason to
//! exist. A `.pptx` chart caches its numbers in the chart part and keeps a
//! second, authoritative copy in an embedded workbook, which is why
//! `pptx/charts-stale-workbook.pptx` can exist at all. An ODF chart keeps a
//! `table:table` named `local-table` **inside itself**: there is one copy, so
//! there is nothing to reconcile and nothing to warn about. The IR's
//! [`ChartRef::workbook`] is therefore `None` for every `.odp` chart, and that
//! is an accurate statement about the file rather than a loss.
//!
//! What a `chart:series` states is *where* its numbers are, as a cell range
//! into that table (`local-table.$B$2:.$B$4`). The ranges are resolved rather
//! than assumed: reading the second column because the generator happens to put
//! the first series there would be reading the corpus, not the format.

use docsai_model::presentation::{ChartRef, ChartSeries};
use docsai_model::report::ConversionReport;

use crate::xml::Element;

/// `chart:class` → the PresentationML plot element that means the same thing.
///
/// One vocabulary, deliberately. [`ChartRef::kind`] is what DocMark-P writes
/// and what 18-E's cross-conversion matrix compares, and a bar chart that reads
/// as `bar` from one dialect and `barChart` from the other would fail that
/// matrix on a difference that is not one. A class with no counterpart keeps
/// its ODF keyword verbatim rather than being forced into the nearest one.
fn plot_kind(class: &str) -> String {
    let class = class.rsplit(':').next().unwrap_or(class);
    match class {
        "bar" => "barChart",
        "line" => "lineChart",
        "area" => "areaChart",
        "circle" => "pieChart",
        "ring" => "doughnutChart",
        "scatter" => "scatterChart",
        "radar" | "filled-radar" => "radarChart",
        "stock" => "stockChart",
        "bubble" => "bubbleChart",
        "surface" => "surfaceChart",
        other => other,
    }
    .to_string()
}

/// A `local-table` read as a 1-based grid of strings, plus the numeric value of
/// each cell when it has one.
#[derive(Default)]
struct LocalTable {
    /// `rows[row][column]`, both 1-based at the call sites through [`Self::at`].
    rows: Vec<Vec<Cell>>,
}

#[derive(Default, Clone)]
struct Cell {
    text: String,
    value: Option<f64>,
}

impl LocalTable {
    fn read(table: &Element) -> LocalTable {
        let mut rows = Vec::new();
        for child in table.children() {
            match child.name.as_str() {
                "table-header-rows" | "table-rows" => {
                    for row in child.children_named("table-row") {
                        rows.push(read_row(row));
                    }
                }
                "table-row" => rows.push(read_row(child)),
                _ => {}
            }
        }
        LocalTable { rows }
    }

    /// One cell, addressed the way a chart addresses it: 1-based column and row.
    fn at(&self, column: usize, row: usize) -> Option<&Cell> {
        self.rows
            .get(row.checked_sub(1)?)?
            .get(column.checked_sub(1)?)
    }

    /// The category labels: the first column of every row below the header.
    ///
    /// A chart with no `chart:categories` element — which is every chart
    /// LibreOffice writes for a `local-table` — states its categories exactly
    /// here, and nowhere else.
    fn categories(&self, header_rows: usize) -> Vec<String> {
        self.rows
            .iter()
            .skip(header_rows)
            .map(|row| row.first().map(|c| c.text.clone()).unwrap_or_default())
            .collect()
    }
}

fn read_row(row: &Element) -> Vec<Cell> {
    let mut cells = Vec::new();
    for cell in row.children() {
        if cell.name != "table-cell" && cell.name != "covered-table-cell" {
            continue;
        }
        let repeat = cell
            .attr("number-columns-repeated")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(1)
            .min(1024);
        let value = cell
            .attr_qualified("office:value")
            .or_else(|| cell.attr("value"))
            .and_then(|v| v.trim().parse::<f64>().ok());
        let read = Cell {
            text: cell.deep_text().trim().to_string(),
            value,
        };
        for _ in 0..repeat {
            cells.push(read.clone());
        }
    }
    cells
}

/// `local-table.$B$2` → column 2, row 2. The table name before the first `.`
/// is ignored: a chart's ranges always address its own local table, and a
/// range whose second endpoint omits the name (`.$B$4`) is the common spelling.
fn parse_address(address: &str) -> Option<(usize, usize)> {
    let cell = address.rsplit('.').next()?.replace('$', "");
    let split = cell.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = cell.split_at(split);
    if letters.is_empty() {
        return None;
    }
    let mut column = 0usize;
    for byte in letters.bytes() {
        let upper = byte.to_ascii_uppercase();
        if !upper.is_ascii_uppercase() {
            return None;
        }
        column = column
            .checked_mul(26)?
            .checked_add((upper - b'A' + 1) as usize)?;
    }
    Some((column, digits.parse().ok()?))
}

/// Reads one `chart:chart` into the IR.
///
/// `report` is taken so that a chart whose ranges do not resolve says so; a
/// series that reads as an empty column would otherwise look like a chart that
/// plots nothing.
pub(super) fn read(root: &Element, report: &mut ConversionReport) -> Option<ChartRef> {
    let chart = if root.name == "chart" {
        root
    } else {
        root.path(&["body", "chart", "chart"])?
    };

    let kind = chart
        .attr_qualified("chart:class")
        .or_else(|| chart.attr("class"))
        .map(plot_kind);
    let title = chart
        .child("title")
        .map(|t| t.deep_text().trim().to_string())
        .filter(|t| !t.is_empty());

    let local = chart
        .children_named("table")
        .find(|t| t.attr("name") == Some("local-table"))
        .map(LocalTable::read)
        .unwrap_or_default();
    let header_rows = chart
        .children_named("table")
        .find(|t| t.attr("name") == Some("local-table"))
        .map(|t| {
            t.children_named("table-header-rows")
                .flat_map(|h| h.children_named("table-row"))
                .count()
        })
        .unwrap_or(0);

    let categories = local.categories(header_rows);

    let mut series = Vec::new();
    for element in chart
        .child("plot-area")
        .into_iter()
        .flat_map(|area| area.children_named("series"))
    {
        let name = element
            .attr("label-cell-address")
            .and_then(parse_address)
            .and_then(|(column, row)| local.at(column, row))
            .map(|cell| cell.text.clone())
            .filter(|name| !name.is_empty());
        let values = match element.attr("values-cell-range-address") {
            Some(range) => column_values(&local, range),
            None => Vec::new(),
        };
        if values.is_empty() && !categories.is_empty() {
            report.warn(docsai_model::report::Warning::Degraded {
                what: format!("chart series `{}`", name.clone().unwrap_or_default()),
                why: "its values-cell-range-address resolves to no cell of local-table".into(),
            });
        }
        series.push(ChartSeries {
            name,
            values,
            number_format: None,
        });
    }

    Some(ChartRef {
        kind,
        title,
        // An ODF chart has no embedded workbook: the numbers are the
        // `local-table` above, and there is no second copy to point at.
        workbook: None,
        categories,
        series,
        raw: None,
    })
}

/// The values a `chart:values-cell-range-address` selects, as one column.
fn column_values(local: &LocalTable, range: &str) -> Vec<Option<f64>> {
    let (start, end) = match range.split_once(':') {
        Some((a, b)) => (a, b),
        None => (range, range),
    };
    let Some((column, first)) = parse_address(start) else {
        return Vec::new();
    };
    let last = parse_address(end).map(|(_, row)| row).unwrap_or(first);
    (first.min(last)..=first.max(last))
        .map(|row| local.at(column, row).and_then(|cell| cell.value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_parsed_with_or_without_a_table_name() {
        assert_eq!(parse_address("local-table.$B$2"), Some((2, 2)));
        assert_eq!(parse_address(".$AA$11"), Some((27, 11)));
        assert_eq!(parse_address("nonsense"), None);
    }

    #[test]
    fn a_series_reads_the_column_its_range_names() {
        let xml = r#"<chart:chart xmlns:chart="c" xmlns:table="t" xmlns:office="o"
            xmlns:text="x" chart:class="chart:bar">
          <chart:title><text:p>T</text:p></chart:title>
          <chart:plot-area>
            <chart:series chart:values-cell-range-address="local-table.$C$2:.$C$3"
                          chart:label-cell-address="local-table.$C$1"/>
          </chart:plot-area>
          <table:table table:name="local-table">
            <table:table-header-rows><table:table-row>
              <table:table-cell office:value-type="string"><text:p/></table:table-cell>
              <table:table-cell office:value-type="string"><text:p>A</text:p></table:table-cell>
              <table:table-cell office:value-type="string"><text:p>B</text:p></table:table-cell>
            </table:table-row></table:table-header-rows>
            <table:table-rows>
              <table:table-row>
                <table:table-cell office:value-type="string"><text:p>N</text:p></table:table-cell>
                <table:table-cell office:value-type="float" office:value="1"><text:p>1</text:p></table:table-cell>
                <table:table-cell office:value-type="float" office:value="2"><text:p>2</text:p></table:table-cell>
              </table:table-row>
              <table:table-row>
                <table:table-cell office:value-type="string"><text:p>S</text:p></table:table-cell>
                <table:table-cell office:value-type="float" office:value="3"><text:p>3</text:p></table:table-cell>
                <table:table-cell office:value-type="float" office:value="4"><text:p>4</text:p></table:table-cell>
              </table:table-row>
            </table:table-rows>
          </table:table>
        </chart:chart>"#;
        let root = Element::parse("c.xml", xml.as_bytes()).expect("parses");
        let mut report = ConversionReport::new();
        let chart = read(&root, &mut report).expect("a chart");
        assert_eq!(chart.kind.as_deref(), Some("barChart"));
        assert_eq!(chart.categories, vec!["N".to_string(), "S".to_string()]);
        assert_eq!(chart.series.len(), 1);
        assert_eq!(chart.series[0].name.as_deref(), Some("B"));
        assert_eq!(chart.series[0].values, vec![Some(2.0), Some(4.0)]);
        assert!(report.warnings.is_empty());
    }
}

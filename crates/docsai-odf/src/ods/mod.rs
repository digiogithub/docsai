//! The `.ods` reader and writer (Phase 4).

pub(crate) mod write;

use std::collections::BTreeMap;
use std::io::{Read, Seek};

use docsai_model::assets::AssetStore;
use docsai_model::image::ImageRef;
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::sheet::{
    Cell, CellRange, CellRef, CellValue, ColProps, Formula, FormulaDialect, NumFmt, Sheet, Workbook,
};
use docsai_model::text::DocumentMeta;
use docsai_model::units::Length;
use docsai_model::Document;

use crate::draw;
use crate::error::ReadError;
use crate::length::parse_length;
use crate::package::Package;
use crate::styles::{read_automatic_styles, read_named_styles, OdfStyles};
use crate::xml::Element;

const CONTENT: &str = "content.xml";
const STYLES: &str = "styles.xml";
const META: &str = "meta.xml";
const MIME_ODS: &str = "application/vnd.oasis.opendocument.spreadsheet";

/// Reads an `.ods` workbook into the IR.
pub fn read<R: Read + Seek>(
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    let package = Package::open(reader)?;
    read_package(&package, assets)
}

pub(crate) fn read_package(
    package: &Package,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    if !package.has_part(CONTENT) {
        return Err(ReadError::MissingPart(CONTENT.into()));
    }
    if let Some(mime) = package.part("mimetype") {
        let mime = std::str::from_utf8(mime).unwrap_or("").trim();
        if !mime.is_empty() && !mime.contains("opendocument.spreadsheet") {
            return Err(ReadError::WrongShape {
                part: "mimetype".into(),
                expected: MIME_ODS.into(),
            });
        }
    }

    let mut report = ConversionReport::new();
    let mut styles = OdfStyles::default();

    if let Some(styles_root) = package.optional_xml(STYLES)? {
        if let Some(named) = styles_root.child("styles") {
            read_named_styles(named, &mut styles);
        } else {
            read_named_styles(&styles_root, &mut styles);
        }
        if let Some(auto) = styles_root.child("automatic-styles") {
            read_automatic_styles(auto, &mut styles);
        }
    }

    let content_source = package.text(CONTENT)?;
    let content_root = Element::parse(CONTENT, content_source.as_bytes())?;
    if let Some(auto) = content_root.child("automatic-styles") {
        read_automatic_styles(auto, &mut styles);
    }

    let meta = read_meta(package);
    let spreadsheet =
        content_root
            .path(&["body", "spreadsheet"])
            .ok_or_else(|| ReadError::WrongShape {
                part: CONTENT.into(),
                expected: "OpenDocument spreadsheet".into(),
            })?;

    let mut sheets = Vec::new();
    for table in spreadsheet.children_named("table") {
        let sheet = read_sheet(table, package, CONTENT, &styles, assets, &mut report);
        report.stats.sheets = report.stats.sheets.saturating_add(1);
        report.stats.cells = report.stats.cells.saturating_add(sheet.cells.len() as u32);
        sheets.push(sheet);
    }

    let active_sheet = sheets.first().map(|s| s.name.clone());
    report.stats.styles = styles.catalog.styles.len() as u32;

    Ok((
        Document::Workbook(Workbook {
            addressing: Default::default(),
            meta,
            styles: styles.catalog,
            defined_names: Vec::new(),
            sheets,
            active_sheet,
        }),
        report,
    ))
}

fn read_meta(package: &Package) -> DocumentMeta {
    let mut meta = DocumentMeta::default();
    let Ok(Some(root)) = package.optional_xml(META) else {
        return meta;
    };
    let office_meta = root.child("meta").unwrap_or(&root);
    let text = |name: &str| {
        office_meta
            .child(name)
            .map(|e| e.deep_text())
            .filter(|t| !t.is_empty())
    };
    meta.title = text("title");
    meta.author = text("initial-creator").or_else(|| text("creator"));
    meta.last_modified_by = text("creator");
    meta.created = text("creation-date");
    meta.modified = text("date");
    meta.language = text("language");
    meta.subject = text("subject");
    meta.description = text("description");
    meta.application = text("generator");
    let keywords: Vec<String> = office_meta
        .children_named("keyword")
        .map(|e| e.deep_text())
        .filter(|t| !t.is_empty())
        .collect();
    if !keywords.is_empty() {
        meta.keywords = Some(keywords.join(", "));
    }
    for ud in office_meta.children_named("user-defined") {
        if let Some(name) = ud.attr("name") {
            meta.custom.insert(name.to_string(), ud.deep_text());
        }
    }
    meta
}

fn read_sheet(
    table: &Element,
    package: &Package,
    part: &str,
    styles: &OdfStyles,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Sheet {
    let name = table
        .attr("name")
        .or_else(|| table.attr_qualified("table:name"))
        .unwrap_or("Sheet1")
        .to_string();
    let mut sheet = Sheet::new(name);

    // Columns
    let mut col_idx = 0u32;
    for col in table.children_named("table-column") {
        let repeat = col
            .attr("number-columns-repeated")
            .or_else(|| col.attr_qualified("table:number-columns-repeated"))
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1)
            .min(1024);
        let hidden = col
            .attr("visibility")
            .or_else(|| col.attr_qualified("table:visibility"))
            .is_some_and(|v| v == "collapse" || v == "filter");
        let _style_name = col
            .attr("style-name")
            .or_else(|| col.attr_qualified("table:style-name"));
        for _ in 0..repeat {
            if hidden {
                sheet.cols.insert(
                    col_idx,
                    ColProps {
                        width_chars: None,
                        hidden: Some(true),
                    },
                );
            }
            col_idx = col_idx.saturating_add(1);
        }
    }

    let mut row_idx = 0u32;
    let mut merges: Vec<CellRange> = Vec::new();
    // Track covered cells from spans so we skip them.
    let mut covered: BTreeMap<(u32, u32), bool> = BTreeMap::new();

    for row_el in table.children_named("table-row") {
        let row_repeat = row_el
            .attr("number-rows-repeated")
            .or_else(|| row_el.attr_qualified("table:number-rows-repeated"))
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1)
            .min(10_000);

        for _r in 0..row_repeat {
            let mut col = 0u32;
            for child in row_el.children() {
                match child.name.as_str() {
                    "table-cell" => {
                        let col_repeat = child
                            .attr("number-columns-repeated")
                            .or_else(|| child.attr_qualified("table:number-columns-repeated"))
                            .and_then(|s| s.parse::<u32>().ok())
                            .unwrap_or(1)
                            .min(1024);
                        let colspan = child
                            .attr("number-columns-spanned")
                            .or_else(|| child.attr_qualified("table:number-columns-spanned"))
                            .and_then(|s| s.parse::<u32>().ok())
                            .unwrap_or(1)
                            .max(1);
                        let rowspan = child
                            .attr("number-rows-spanned")
                            .or_else(|| child.attr_qualified("table:number-rows-spanned"))
                            .and_then(|s| s.parse::<u32>().ok())
                            .unwrap_or(1)
                            .max(1);

                        let cell = read_cell(child, report);
                        let images = read_cell_images(
                            child,
                            CellRef::new(col, row_idx),
                            package,
                            part,
                            styles,
                            assets,
                            report,
                        );

                        for i in 0..col_repeat {
                            let c = col.saturating_add(i);
                            if covered.contains_key(&(c, row_idx)) {
                                continue;
                            }
                            if !cell_is_blank(&cell) {
                                sheet.cells.insert(CellRef::new(c, row_idx), cell.clone());
                            }
                            if colspan > 1 || rowspan > 1 {
                                let end = CellRef::new(
                                    c.saturating_add(colspan - 1),
                                    row_idx.saturating_add(rowspan - 1),
                                );
                                merges.push(CellRange::new(CellRef::new(c, row_idx), end));
                                for rr in 0..rowspan {
                                    for cc in 0..colspan {
                                        if rr == 0 && cc == 0 {
                                            continue;
                                        }
                                        covered.insert(
                                            (c.saturating_add(cc), row_idx.saturating_add(rr)),
                                            true,
                                        );
                                    }
                                }
                            }
                            for img in &images {
                                let mut image = img.clone();
                                if !image.geometry.anchor.is_sheet() {
                                    image.geometry.anchor = draw::sheet_one_cell(
                                        CellRef::new(c, row_idx),
                                        Length::ZERO,
                                        Length::ZERO,
                                    );
                                }
                                sheet.images.push(image);
                            }
                        }
                        col = col.saturating_add(col_repeat.max(colspan));
                    }
                    "covered-table-cell" => {
                        let col_repeat = child
                            .attr("number-columns-repeated")
                            .or_else(|| child.attr_qualified("table:number-columns-repeated"))
                            .and_then(|s| s.parse::<u32>().ok())
                            .unwrap_or(1)
                            .min(1024);
                        col = col.saturating_add(col_repeat);
                    }
                    _ => {}
                }
            }
            row_idx = row_idx.saturating_add(1);
            if row_idx > 1_000_000 {
                report.warn(Warning::Degraded {
                    what: format!("sheet `{}`", sheet.name),
                    why: "row count capped for safety".into(),
                });
                break;
            }
        }
    }

    // Deduplicate merges
    merges.sort_by_key(|m| (m.start.row, m.start.col, m.end.row, m.end.col));
    merges.dedup();
    sheet.merges = merges;
    sheet
}

fn cell_is_blank(cell: &Cell) -> bool {
    cell.value.is_empty()
        && cell.formula.is_none()
        && cell.num_fmt.is_none()
        && cell.style.is_none()
}

fn read_cell(element: &Element, report: &mut ConversionReport) -> Cell {
    let mut cell = Cell::default();

    if let Some(formula) = element
        .attr("formula")
        .or_else(|| element.attr_qualified("table:formula"))
    {
        let text = strip_of_prefix(formula);
        cell.formula = Some(Formula {
            text,
            dialect: FormulaDialect::OpenFormula,
            shared_over: None,
            array_over: None,
        });
        report.stats.formulas = report.stats.formulas.saturating_add(1);
    }

    let value_type = element
        .attr("value-type")
        .or_else(|| element.attr_qualified("office:value-type"))
        .unwrap_or("");

    cell.value = match value_type {
        "float" | "percentage" | "currency" => {
            let n = element
                .attr("value")
                .or_else(|| element.attr_qualified("office:value"))
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            if value_type == "percentage" {
                cell.num_fmt = Some(NumFmt {
                    code: "0%".into(),
                    id: None,
                });
            } else if value_type == "currency" {
                if let Some(currency) = element
                    .attr("currency")
                    .or_else(|| element.attr_qualified("office:currency"))
                {
                    cell.num_fmt = Some(NumFmt {
                        code: format!("\"{currency}\"#,##0.00"),
                        id: None,
                    });
                }
            }
            CellValue::Number(n)
        }
        "date" => {
            let v = element
                .attr("date-value")
                .or_else(|| element.attr_qualified("office:date-value"))
                .unwrap_or("")
                .to_string();
            if v.is_empty() {
                CellValue::Empty
            } else {
                CellValue::DateTime(v)
            }
        }
        "time" => {
            let v = element
                .attr("time-value")
                .or_else(|| element.attr_qualified("office:time-value"))
                .unwrap_or("")
                .to_string();
            if v.is_empty() {
                CellValue::Empty
            } else {
                CellValue::DateTime(v)
            }
        }
        "boolean" => {
            let v = element
                .attr("boolean-value")
                .or_else(|| element.attr_qualified("office:boolean-value"))
                .unwrap_or("false");
            CellValue::Bool(v == "true")
        }
        "string" | "" => {
            // Prefer office:string-value, else concatenate text:p.
            if let Some(s) = element
                .attr("string-value")
                .or_else(|| element.attr_qualified("office:string-value"))
            {
                if s.is_empty() && cell.formula.is_none() {
                    CellValue::Empty
                } else {
                    CellValue::Text(s.to_string())
                }
            } else {
                let mut parts = Vec::new();
                for p in element.children_named("p") {
                    parts.push(p.deep_text());
                }
                let text = parts.join("\n");
                if text.is_empty() {
                    CellValue::Empty
                } else {
                    CellValue::Text(text)
                }
            }
        }
        other => {
            report.warn(Warning::Degraded {
                what: format!("cell value-type `{other}`"),
                why: "treated as text".into(),
            });
            let text = element.deep_text();
            if text.is_empty() {
                CellValue::Empty
            } else {
                CellValue::Text(text)
            }
        }
    };

    // When a formula has a cached typed value we keep both.
    if cell.formula.is_some() && matches!(cell.value, CellValue::Empty) {
        // display text from paragraphs as text fallback
        let text = element
            .children_named("p")
            .map(|p| p.deep_text())
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            if let Ok(n) = text.replace(',', "").parse::<f64>() {
                cell.value = CellValue::Number(n);
            } else {
                cell.value = CellValue::Text(text);
            }
        }
    }

    cell
}

fn strip_of_prefix(formula: &str) -> String {
    let f = formula.trim();
    if let Some(rest) = f.strip_prefix("of:") {
        rest.trim_start_matches('=').to_string()
    } else if let Some(rest) = f.strip_prefix('=') {
        rest.to_string()
    } else {
        f.to_string()
    }
}

fn read_cell_images(
    cell: &Element,
    cell_ref: CellRef,
    package: &Package,
    part: &str,
    styles: &OdfStyles,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Vec<ImageRef> {
    let mut out = Vec::new();
    for frame in cell.children_named("frame") {
        if let Some(mut img) = draw::read_frame(frame, package, part, styles, assets, report) {
            let x = frame
                .attr("x")
                .or_else(|| frame.attr_qualified("svg:x"))
                .and_then(parse_length)
                .unwrap_or(Length::ZERO);
            let y = frame
                .attr("y")
                .or_else(|| frame.attr_qualified("svg:y"))
                .and_then(parse_length)
                .unwrap_or(Length::ZERO);
            img.geometry.anchor = draw::sheet_one_cell(cell_ref, x, y);
            out.push(img);
        }
    }
    // Also frames nested under text:p
    for p in cell.children_named("p") {
        for frame in p.children_named("frame") {
            if let Some(mut img) = draw::read_frame(frame, package, part, styles, assets, report) {
                img.geometry.anchor = draw::sheet_one_cell(cell_ref, Length::ZERO, Length::ZERO);
                out.push(img);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::MemoryAssetStore;
    use std::io::Cursor;

    fn minimal_ods() -> Vec<u8> {
        let mut package = Package::new();
        package.insert("mimetype", MIME_ODS.as_bytes());
        package.insert(
            "content.xml",
            br#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
 xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"
 xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0">
  <office:body>
    <office:spreadsheet>
      <table:table table:name="Sheet1">
        <table:table-column/>
        <table:table-row>
          <table:table-cell office:value-type="string"><text:p>Hello</text:p></table:table-cell>
          <table:table-cell office:value-type="float" office:value="3.5"/>
          <table:table-cell table:formula="of:=[.A1]" office:value-type="string"><text:p>Hello</text:p></table:table-cell>
        </table:table-row>
      </table:table>
    </office:spreadsheet>
  </office:body>
</office:document-content>"#,
        );
        package.insert(
            "META-INF/manifest.xml",
            br#"<?xml version="1.0"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>"#,
        );
        let mut buf = Cursor::new(Vec::new());
        package.write_to(&mut buf).unwrap();
        buf.into_inner()
    }

    #[test]
    fn reads_minimal_ods_cells_and_formula() {
        let mut assets = MemoryAssetStore::new();
        let (doc, report) = read(Cursor::new(minimal_ods()), &mut assets).unwrap();
        let Document::Workbook(book) = doc else {
            panic!("expected workbook");
        };
        assert_eq!(book.sheets.len(), 1);
        let sheet = &book.sheets[0];
        assert_eq!(
            sheet.cells.get(&CellRef::new(0, 0)).map(|c| &c.value),
            Some(&CellValue::Text("Hello".into()))
        );
        assert_eq!(
            sheet.cells.get(&CellRef::new(1, 0)).map(|c| &c.value),
            Some(&CellValue::Number(3.5))
        );
        let formula = sheet
            .cells
            .get(&CellRef::new(2, 0))
            .and_then(|c| c.formula.as_ref());
        assert!(formula.is_some());
        assert_eq!(formula.unwrap().dialect, FormulaDialect::OpenFormula);
        assert_eq!(formula.unwrap().text, "[.A1]");
        assert!(report.stats.formulas >= 1);
    }
}

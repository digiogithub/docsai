//! The `.xlsx` reader and writer (Phase 3).
//!
//! Reading walks the OPC package with `zip` + `quick-xml` (spike R3). Writing
//! rebuilds SpreadsheetML from the IR the same way. Legacy `.xls` lives in the
//! sibling `xls` module and uses calamine.

mod drawing;
mod styles;
pub(crate) mod write;

use std::collections::BTreeMap;
use std::io::{Read, Seek};

use docsai_model::assets::AssetStore;
use docsai_model::image::RawId;
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::sheet::{
    Cell, CellRange, CellRef, CellValue, ColProps, DefinedName, Formula, FormulaDialect, Pane,
    RowProps, Sheet, Workbook,
};
use docsai_model::text::RawFragment;
use docsai_model::units::Length;
use docsai_model::Document;

use crate::error::ReadError;
use crate::package::{read_meta, Package};
use crate::xml::Element;

use styles::{is_date_format, Styles};

pub(crate) const WORKBOOK_PART: &str = "xl/workbook.xml";

/// Reads a `.xlsx` workbook into the IR.
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
    if !package.has_part(WORKBOOK_PART) {
        return Err(ReadError::MissingPart(WORKBOOK_PART.into()));
    }
    let mut report = ConversionReport::new();

    if package.part_names().any(|n| n.ends_with("vbaProject.bin")) {
        report.warn(Warning::MacrosIgnored {
            part: "xl/vbaProject.bin".into(),
        });
    }

    let meta = read_meta(package);
    let styles = match package.optional_xml("xl/styles.xml")? {
        Some(root) => styles::read_styles(&root),
        None => Styles::default(),
    };

    let shared_strings = read_shared_strings(package)?;
    let workbook_rels = package.relationships(WORKBOOK_PART);
    let workbook_root = {
        let source = package.text(WORKBOOK_PART)?;
        Element::parse(WORKBOOK_PART, source.as_bytes())?
    };

    let mut sheets = Vec::new();
    let sheet_elements: Vec<&Element> = workbook_root
        .child("sheets")
        .map(|s| s.children_named("sheet").collect())
        .unwrap_or_default();

    for sheet_el in sheet_elements {
        let name = sheet_el.attr("name").unwrap_or("Sheet").to_string();
        let hidden = matches!(sheet_el.attr("state"), Some("hidden") | Some("veryHidden"));
        let Some(rid) = sheet_el
            .attr_qualified("r:id")
            .or_else(|| sheet_el.attr("id"))
        else {
            report.warn(Warning::Degraded {
                what: format!("sheet `{name}`"),
                why: "missing relationship id".into(),
            });
            continue;
        };
        let Some(rel) = workbook_rels.get(rid) else {
            report.warn(Warning::Degraded {
                what: format!("sheet `{name}`"),
                why: format!("unresolved relationship `{rid}`"),
            });
            continue;
        };
        let sheet = read_sheet(
            package,
            &rel.target,
            name,
            hidden,
            &shared_strings,
            &styles,
            assets,
            &mut report,
        )?;
        report.stats.sheets = report.stats.sheets.saturating_add(1);
        sheets.push(sheet);
    }

    let defined_names = read_defined_names(&workbook_root);
    let mut active_sheet = sheets.first().map(|s| s.name.clone());

    if let Some(view) = workbook_root
        .child("bookViews")
        .and_then(|v| v.child("workbookView"))
    {
        if let Some(idx) = view.attr_i64("activeTab") {
            if let Some(sheet) = sheets.get(idx as usize) {
                active_sheet = Some(sheet.name.clone());
            }
        }
    }

    let mut book = Workbook {
        addressing: Default::default(),
        meta,
        styles: styles.catalog,
        defined_names,
        sheets,
        active_sheet,
    };
    prune_unused_styles(&mut book);
    report.stats.styles = book.styles.styles.len() as u32;

    Ok((Document::Workbook(book), report))
}

/// Drop catalogue entries that no cell references. Shared corpus stylesheets
/// often declare header fonts that a particular sheet never uses; keeping them
/// would break DocMark round-trips.
fn prune_unused_styles(book: &mut Workbook) {
    use std::collections::BTreeSet;
    let mut used = BTreeSet::new();
    for sheet in &book.sheets {
        for cell in sheet.cells.values() {
            if let Some(id) = &cell.style {
                used.insert(id.clone());
            }
        }
    }
    book.styles.styles.retain(|id, _| used.contains(id));
}

#[allow(clippy::too_many_arguments)]
fn read_sheet(
    package: &Package,
    part: &str,
    name: String,
    hidden: bool,
    shared_strings: &[String],
    styles: &Styles,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Sheet, ReadError> {
    let source = package.text(part)?;
    let root = Element::parse(part, source.as_bytes())?;
    let mut sheet = Sheet::new(name);
    sheet.hidden = hidden;

    if let Some(cols) = root.child("cols") {
        for col in cols.children_named("col") {
            let min = col.attr_i64("min").unwrap_or(1).max(1) as u32;
            let max = col.attr_i64("max").unwrap_or(min as i64).max(min as i64) as u32;
            let width = col.attr("width").and_then(|w| w.parse::<f64>().ok());
            let hidden_col = col
                .attr("hidden")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"));
            let custom = col
                .attr("customWidth")
                .map(|v| v == "1")
                .unwrap_or(width.is_some());
            for index in min..=max {
                let mut props = ColProps::default();
                if custom {
                    props.width_chars = width;
                }
                props.hidden = hidden_col;
                if props.width_chars.is_some() || props.hidden.is_some() {
                    sheet.cols.insert(index - 1, props);
                }
            }
        }
    }

    // Shared formula masters: si → (formula text, ref range).
    let mut shared_masters: BTreeMap<u32, (String, Option<CellRange>)> = BTreeMap::new();

    if let Some(data) = root.child("sheetData") {
        for row_el in data.children_named("row") {
            let row_idx = row_el
                .attr_i64("r")
                .map(|r| (r.max(1) - 1) as u32)
                .unwrap_or(0);
            let height = row_el.attr("ht").and_then(|h| h.parse::<f64>().ok());
            let row_hidden = row_el.attr("hidden").map(|v| v == "1").unwrap_or(false);
            if height.is_some() || row_hidden {
                sheet.rows.insert(
                    row_idx,
                    RowProps {
                        height: height.map(Length::from_pt),
                        hidden: if row_hidden { Some(true) } else { None },
                    },
                );
            }

            for c_el in row_el.children_named("c") {
                let Some(cell_ref) = c_el.attr("r").and_then(CellRef::parse_a1) else {
                    continue;
                };

                let style_idx = c_el.attr_i64("s").unwrap_or(0).max(0) as usize;
                let cell_type = c_el.attr("t").unwrap_or("n");
                let raw_value = c_el.child("v").map(|v| v.text()).unwrap_or_default();
                let inline = c_el
                    .path(&["is"])
                    .map(|is| is.deep_text())
                    .filter(|t| !t.is_empty());

                let mut cell = Cell {
                    value: decode_value(cell_type, &raw_value, inline.as_deref(), shared_strings),
                    ..Default::default()
                };

                if let Some(f_el) = c_el.child("f") {
                    cell.formula = Some(read_formula(f_el, cell_ref, &mut shared_masters));
                    report.stats.formulas = report.stats.formulas.saturating_add(1);
                }

                if let Some(fmt) = styles.num_fmt_for_xf(style_idx) {
                    if !fmt.code.eq_ignore_ascii_case("General") {
                        cell.num_fmt = Some(fmt);
                    }
                }

                // Date detection: numeric serial + date numFmt → DateTime.
                if let CellValue::Number(serial) = cell.value {
                    let date_fmt = cell
                        .num_fmt
                        .as_ref()
                        .map(|f| is_date_format(&f.code))
                        .unwrap_or_else(|| styles.xf_is_date(style_idx));
                    if date_fmt {
                        if let Some(iso) = excel_serial_to_iso(serial) {
                            cell.value = CellValue::DateTime(iso);
                            if cell.num_fmt.is_none() {
                                cell.num_fmt = styles.num_fmt_for_xf(style_idx);
                            }
                        }
                    }
                }

                if let Some(style_id) = styles.style_id_for_xf(style_idx) {
                    cell.style = Some(style_id);
                }

                if !cell.value.is_empty()
                    || cell.formula.is_some()
                    || cell.num_fmt.is_some()
                    || cell.style.is_some()
                {
                    sheet.cells.insert(cell_ref, cell);
                    report.stats.cells = report.stats.cells.saturating_add(1);
                }
            }
        }
    }

    if let Some(merges) = root.child("mergeCells") {
        for m in merges.children_named("mergeCell") {
            if let Some(range) = m.attr("ref").and_then(parse_range) {
                sheet.merges.push(range);
            }
        }
    }

    if let Some(views) = root.child("sheetViews") {
        if let Some(view) = views.child("sheetView") {
            if let Some(pane) = view.child("pane") {
                let top_left = pane
                    .attr("topLeftCell")
                    .and_then(CellRef::parse_a1)
                    .unwrap_or_else(|| CellRef::new(0, 0));
                let frozen = pane
                    .attr("state")
                    .map(|s| s == "frozen" || s == "frozenSplit")
                    .unwrap_or(false);
                sheet.pane = Some(Pane { top_left, frozen });
            }
        }
    }

    let sheet_rels = package.relationships(part);
    if let Some(drawing_el) = root.child("drawing") {
        if let Some(rid) = drawing_el
            .attr_qualified("r:id")
            .or_else(|| drawing_el.attr("id"))
        {
            if let Some(rel) = sheet_rels.get(rid) {
                match drawing::read_drawing_part(package, &rel.target, assets, report) {
                    Ok(images) => {
                        report.stats.images =
                            report.stats.images.saturating_add(images.len() as u32);
                        sheet.images.extend(images);
                    }
                    Err(err) => {
                        report.warn(Warning::Degraded {
                            what: format!("drawing `{}`", rel.target),
                            why: err.to_string(),
                        });
                    }
                }
            }
        }
    }

    for rel in sheet_rels.of_kind("chart") {
        report.warn(Warning::UnsupportedElement {
            kind: "chart".into(),
            location: rel.target.clone(),
            action: "raw-block".into(),
        });
        let content = package
            .part(&rel.target)
            .and_then(|b| std::str::from_utf8(b).ok())
            .unwrap_or("")
            .to_string();
        sheet.raw.push(RawFragment {
            id: RawId::new(format!("chart-{}", sheet.raw.len() + 1)),
            format: "ooxml".into(),
            part: rel.target.clone(),
            content,
        });
        report.raw_blocks_emitted = report.raw_blocks_emitted.saturating_add(1);
    }

    Ok(sheet)
}

fn read_formula(
    f_el: &Element,
    cell: CellRef,
    shared_masters: &mut BTreeMap<u32, (String, Option<CellRange>)>,
) -> Formula {
    let kind = f_el.attr("t").unwrap_or("");
    let text = f_el.text();
    let text = text.strip_prefix('=').unwrap_or(&text).to_string();

    match kind {
        "shared" => {
            let si = f_el.attr_i64("si").unwrap_or(0).max(0) as u32;
            let range = f_el.attr("ref").and_then(parse_range);
            if !text.is_empty() {
                shared_masters.insert(si, (text.clone(), range));
                Formula {
                    text,
                    dialect: FormulaDialect::Ooxml,
                    shared_over: range,
                    array_over: None,
                }
            } else if let Some((master, master_range)) = shared_masters.get(&si) {
                let origin = master_range.map(|r| r.start).unwrap_or(cell);
                let expanded = translate_shared_formula(master, origin, cell);
                Formula {
                    text: expanded,
                    dialect: FormulaDialect::Ooxml,
                    shared_over: *master_range,
                    array_over: None,
                }
            } else {
                Formula {
                    text: String::new(),
                    dialect: FormulaDialect::Ooxml,
                    shared_over: range,
                    array_over: None,
                }
            }
        }
        "array" => {
            let range = f_el.attr("ref").and_then(parse_range);
            Formula {
                text,
                dialect: FormulaDialect::Ooxml,
                shared_over: None,
                array_over: range.or(Some(CellRange::new(cell, cell))),
            }
        }
        _ => Formula {
            text,
            dialect: FormulaDialect::Ooxml,
            shared_over: None,
            array_over: None,
        },
    }
}

/// Translates A1 references in a shared formula master from `origin` to `target`.
pub(crate) fn translate_shared_formula(master: &str, origin: CellRef, target: CellRef) -> String {
    let dcol = target.col as i64 - origin.col as i64;
    let drow = target.row as i64 - origin.row as i64;
    if dcol == 0 && drow == 0 {
        return master.to_string();
    }

    let chars: Vec<char> = master.chars().collect();
    let mut out = String::with_capacity(master.len());
    let mut i = 0;
    while i < chars.len() {
        if let Some((len, rewritten)) = try_rewrite_ref(&chars[i..], dcol, drow) {
            out.push_str(&rewritten);
            i += len;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn try_rewrite_ref(chars: &[char], dcol: i64, drow: i64) -> Option<(usize, String)> {
    let mut i = 0;
    let mut token = String::new();
    if chars.first() == Some(&'$') {
        token.push('$');
        i += 1;
    }
    let letters_start = i;
    while i < chars.len() && chars[i].is_ascii_alphabetic() {
        token.push(chars[i].to_ascii_uppercase());
        i += 1;
    }
    if i == letters_start {
        return None;
    }
    if chars.get(i) == Some(&'$') {
        token.push('$');
        i += 1;
    }
    let digits_start = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        token.push(chars[i]);
        i += 1;
    }
    if i == digits_start {
        return None;
    }
    // Avoid swallowing identifiers like R1C1 leftovers or names ending in digits
    // when the next char continues an identifier.
    if i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
        return None;
    }
    Some((i, rewrite_ref_token(&token, dcol, drow)))
}

fn rewrite_ref_token(token: &str, dcol: i64, drow: i64) -> String {
    let Some(p) = parse_ref_token(token) else {
        return token.to_string();
    };
    let col = if p.abs_col {
        p.col
    } else {
        (p.col as i64 + dcol).max(0) as u32
    };
    let row = if p.abs_row {
        p.row
    } else {
        (p.row as i64 + drow).max(0) as u32
    };
    let mut s = String::new();
    if p.abs_col {
        s.push('$');
    }
    s.push_str(&col_letters(col));
    if p.abs_row {
        s.push('$');
    }
    s.push_str(&(row + 1).to_string());
    s
}

struct ParsedRef {
    col: u32,
    row: u32,
    abs_col: bool,
    abs_row: bool,
}

fn parse_ref_token(token: &str) -> Option<ParsedRef> {
    let bytes = token.as_bytes();
    let mut i = 0;
    let abs_col = if bytes.first() == Some(&b'$') {
        i += 1;
        true
    } else {
        false
    };
    let letters_start = i;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    if i == letters_start {
        return None;
    }
    let letters = std::str::from_utf8(&bytes[letters_start..i]).ok()?;
    let abs_row = if i < bytes.len() && bytes[i] == b'$' {
        i += 1;
        true
    } else {
        false
    };
    let digits_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == digits_start || i != bytes.len() {
        return None;
    }
    let row: u32 = std::str::from_utf8(&bytes[digits_start..i])
        .ok()?
        .parse()
        .ok()?;
    if row == 0 {
        return None;
    }
    let mut col: u64 = 0;
    for b in letters.bytes() {
        let v = b.to_ascii_uppercase() - b'A' + 1;
        col = col * 26 + v as u64;
    }
    if col == 0 {
        return None;
    }
    Some(ParsedRef {
        col: (col - 1) as u32,
        row: row - 1,
        abs_col,
        abs_row,
    })
}

fn col_letters(col: u32) -> String {
    let mut name = String::new();
    let mut c = col as u64 + 1;
    while c > 0 {
        let rem = ((c - 1) % 26) as u8;
        name.insert(0, (b'A' + rem) as char);
        c = (c - 1) / 26;
    }
    name
}

fn decode_value(
    cell_type: &str,
    raw: &str,
    inline: Option<&str>,
    shared_strings: &[String],
) -> CellValue {
    match cell_type {
        "s" => {
            let idx: usize = raw.trim().parse().unwrap_or(0);
            CellValue::Text(shared_strings.get(idx).cloned().unwrap_or_default())
        }
        "inlineStr" | "str" => {
            if let Some(t) = inline {
                CellValue::Text(t.to_string())
            } else if !raw.is_empty() {
                CellValue::Text(raw.to_string())
            } else {
                CellValue::Empty
            }
        }
        "b" => CellValue::Bool(raw == "1" || raw.eq_ignore_ascii_case("true")),
        "e" => {
            if raw.is_empty() {
                CellValue::Error("#VALUE!".into())
            } else {
                CellValue::Error(raw.to_string())
            }
        }
        "d" => {
            if raw.is_empty() {
                CellValue::Empty
            } else {
                CellValue::DateTime(raw.to_string())
            }
        }
        _ => {
            if raw.is_empty() {
                if let Some(t) = inline {
                    return CellValue::Text(t.to_string());
                }
                return CellValue::Empty;
            }
            match raw.parse::<f64>() {
                Ok(n) => CellValue::Number(n),
                Err(_) => CellValue::Text(raw.to_string()),
            }
        }
    }
}

fn read_shared_strings(package: &Package) -> Result<Vec<String>, ReadError> {
    let Some(bytes) = package.part("xl/sharedStrings.xml") else {
        return Ok(Vec::new());
    };
    let root = Element::parse("xl/sharedStrings.xml", bytes)?;
    Ok(root.children_named("si").map(|si| si.deep_text()).collect())
}

fn read_defined_names(workbook: &Element) -> Vec<DefinedName> {
    let Some(names) = workbook.child("definedNames") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for n in names.children_named("definedName") {
        let Some(name) = n.attr("name") else {
            continue;
        };
        let sheet = n.attr_i64("localSheetId").map(|i| i.max(0) as usize);
        out.push(DefinedName {
            name: name.to_string(),
            refers_to: n.text(),
            sheet,
        });
    }
    out
}

fn parse_range(text: &str) -> Option<CellRange> {
    let text = text.replace('$', "");
    if let Some((a, b)) = text.split_once(':') {
        Some(CellRange::new(CellRef::parse_a1(a)?, CellRef::parse_a1(b)?))
    } else {
        let c = CellRef::parse_a1(&text)?;
        Some(CellRange::new(c, c))
    }
}

/// Excel 1900-date system serial → ISO-8601 date or date-time.
pub fn excel_serial_to_iso(serial: f64) -> Option<String> {
    if !serial.is_finite() || serial < 0.0 {
        return None;
    }
    let days = serial.floor() as i64;
    let fraction = serial - days as f64;
    let excel_epoch = CivilDate(1899, 12, 30);
    let date = excel_epoch.checked_add_days(days)?;
    if fraction.abs() < 1e-12 {
        return Some(format!("{:04}-{:02}-{:02}", date.0, date.1, date.2));
    }
    let total_seconds = (fraction * 86400.0).round() as i64;
    let h = total_seconds / 3600;
    let m = (total_seconds % 3600) / 60;
    let s = total_seconds % 60;
    Some(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        date.0, date.1, date.2, h, m, s
    ))
}

/// ISO-8601 date or date-time → Excel serial.
pub fn iso_to_excel_serial(iso: &str) -> Option<f64> {
    let (date_part, time_part) = if let Some((d, t)) = iso.split_once('T') {
        (d, Some(t))
    } else {
        (iso, None)
    };
    let mut parts = date_part.split('-');
    let y: i32 = parts.next()?.parse().ok()?;
    let mo: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let date = CivilDate(y, mo, d);
    let epoch = CivilDate(1899, 12, 30);
    let days = date.days_from_civil() - epoch.days_from_civil();
    let mut serial = days as f64;
    if let Some(t) = time_part {
        let t = t.trim_end_matches('Z');
        let mut tp = t.split(':');
        let h: f64 = tp.next()?.parse().ok()?;
        let m: f64 = tp.next()?.parse().ok()?;
        let s: f64 = tp.next().unwrap_or("0").parse().ok()?;
        serial += (h * 3600.0 + m * 60.0 + s) / 86400.0;
    }
    Some(serial)
}

/// Minimal civil-date helper (Howard Hinnant algorithm) — avoids a chrono dep.
#[derive(Clone, Copy)]
struct CivilDate(i32, u32, u32);

impl CivilDate {
    fn days_from_civil(self) -> i64 {
        let y = if self.1 <= 2 { self.0 - 1 } else { self.0 } as i64;
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = (y - era * 400) as u64;
        let mp = if self.1 > 2 { self.1 - 3 } else { self.1 + 9 } as u64;
        let doy = (153 * mp + 2) / 5 + self.2 as u64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe as i64 - 719468
    }

    fn checked_add_days(self, days: i64) -> Option<CivilDate> {
        civil_from_days(self.days_from_civil() + days)
    }
}

fn civil_from_days(z: i64) -> Option<CivilDate> {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    Some(CivilDate(y as i32, m as u32, d as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::MemoryAssetStore;

    fn read_fixture(name: &str) -> (Document, ConversionReport) {
        let path = format!(
            "{}/../../corpus/xlsx/{name}.xlsx",
            env!("CARGO_MANIFEST_DIR")
        );
        let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let mut assets = MemoryAssetStore::new();
        read(file, &mut assets).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    fn book(doc: &Document) -> &Workbook {
        match doc {
            Document::Workbook(w) => w,
            other => panic!("expected a workbook, got {}", other.shape_name()),
        }
    }

    #[test]
    fn reads_value_types() {
        let (doc, _) = read_fixture("values-types");
        let book = book(&doc);
        assert_eq!(book.meta.title.as_deref(), Some("Tipos de valor"));
        let sheet = &book.sheets[0];
        assert_eq!(sheet.name, "Tipos");
        assert_eq!(
            sheet.cells.get(&CellRef::new(1, 1)).map(|c| &c.value),
            Some(&CellValue::Number(42.0))
        );
        assert_eq!(
            sheet.cells.get(&CellRef::new(1, 3)).map(|c| &c.value),
            Some(&CellValue::Bool(true))
        );
        assert_eq!(
            sheet.cells.get(&CellRef::new(1, 4)).map(|c| &c.value),
            Some(&CellValue::Error("#DIV/0!".into()))
        );
        match sheet.cells.get(&CellRef::new(1, 5)).map(|c| &c.value) {
            Some(CellValue::DateTime(iso)) => assert!(iso.starts_with("202"), "got {iso}"),
            other => panic!("expected date, got {other:?}"),
        }
        assert_eq!(
            sheet.cells.get(&CellRef::new(1, 6)).map(|c| &c.value),
            Some(&CellValue::Text("en linea".into()))
        );
    }

    #[test]
    fn reads_formulas_and_defined_names() {
        let (doc, _) = read_fixture("formulas-basic");
        let book = book(&doc);
        let sheet = &book.sheets[0];
        let b4 = sheet.cells.get(&CellRef::new(1, 3)).expect("B4");
        assert_eq!(
            b4.formula.as_ref().map(|f| f.text.as_str()),
            Some("SUM(B2:B3)")
        );
        assert_eq!(b4.value, CellValue::Number(250.0));
        assert_eq!(book.defined_names.len(), 1);
        assert_eq!(book.defined_names[0].name, "TOTAL_ANUAL");
    }

    #[test]
    fn expands_shared_formulas() {
        let (doc, _) = read_fixture("formulas-shared");
        let sheet = &book(&doc).sheets[0];
        let c1 = sheet.cells.get(&CellRef::new(2, 0)).expect("C1");
        assert_eq!(c1.formula.as_ref().map(|f| f.text.as_str()), Some("A1+B1"));
        assert!(c1.formula.as_ref().unwrap().shared_over.is_some());
        let c2 = sheet.cells.get(&CellRef::new(2, 1)).expect("C2");
        assert_eq!(c2.formula.as_ref().map(|f| f.text.as_str()), Some("A2+B2"));
        let a4 = sheet.cells.get(&CellRef::new(0, 3)).expect("A4");
        assert!(a4.formula.as_ref().unwrap().array_over.is_some());
    }

    #[test]
    fn reads_number_formats_and_merges() {
        let (doc, _) = read_fixture("number-formats");
        let sheet = &book(&doc).sheets[0];
        let b2 = sheet.cells.get(&CellRef::new(1, 1)).expect("B2");
        assert!(b2.num_fmt.as_ref().unwrap().code.contains("EUR"));
        let (doc, _) = read_fixture("merged-cells");
        let sheet = &book(&doc).sheets[0];
        assert_eq!(sheet.merges.len(), 2);
        assert!(sheet.cols.get(&0).and_then(|c| c.width_chars).is_some());
    }

    #[test]
    fn reads_sheet_images_with_three_anchors() {
        let (doc, report) = read_fixture("images-anchored");
        let sheet = &book(&doc).sheets[0];
        assert_eq!(sheet.images.len(), 3);
        assert_eq!(report.stats.images, 3);
        use docsai_model::image::Anchor;
        assert!(matches!(
            sheet.images[0].geometry.anchor,
            Anchor::SheetTwoCell { .. }
        ));
        assert!(matches!(
            sheet.images[1].geometry.anchor,
            Anchor::SheetOneCell { .. }
        ));
        assert!(matches!(
            sheet.images[2].geometry.anchor,
            Anchor::SheetAbsolute { .. }
        ));
    }

    #[test]
    fn serial_date_round_trips() {
        let iso = excel_serial_to_iso(45658.0).unwrap();
        let back = iso_to_excel_serial(&iso).unwrap();
        assert!((back - 45658.0).abs() < 1e-9, "iso={iso} back={back}");
    }

    #[test]
    fn shared_formula_translation_shifts_relative_refs() {
        let origin = CellRef::new(2, 0); // C1
        let target = CellRef::new(2, 1); // C2
        assert_eq!(translate_shared_formula("A1+B1", origin, target), "A2+B2");
        assert_eq!(
            translate_shared_formula("$A1+B$1", origin, target),
            "$A2+B$1"
        );
    }
}

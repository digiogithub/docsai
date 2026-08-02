//! Workbook body parser (DocMark §4).

use std::path::{Path, PathBuf};

use docsai_model::assets::AssetStore;
use docsai_model::image::{
    Anchor, CellAnchor, CropRect, Flip, ImageGeometry, ImageRef, RawId, SimpleBorder,
};
use docsai_model::report::ConversionReport;
use docsai_model::sheet::{
    Cell, CellRange, CellRef, CellValue, ColProps, Formula, FormulaDialect, NumFmt, Pane, Sheet,
    Workbook,
};
use docsai_model::style::StyleId;
use docsai_model::text::RawFragment;
use docsai_model::units::{Length, Point, Size};
use docsai_model::Document;

use crate::attrs::Attrs;
use crate::error::ParseError;
use crate::escape::unescape;
use crate::frontmatter_parse::FrontMatter;

/// True when the document should be parsed as a workbook rather than text.
pub fn looks_like_workbook(fm: &FrontMatter, body: &str) -> bool {
    if matches!(
        fm.source_format,
        docsai_model::Format::Xlsx | docsai_model::Format::Xls | docsai_model::Format::Ods
    ) {
        return true;
    }
    if fm.active_sheet.is_some() || !fm.defined_names.is_empty() {
        return true;
    }
    // Heading with `.sheet` class.
    for line in body.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix('#') {
            if rest.starts_with('#') {
                continue;
            }
            if rest.contains("{.sheet") || rest.contains(".sheet ") || rest.contains(".sheet}") {
                return true;
            }
        }
    }
    false
}

/// Parses the body of a workbook DocMark file.
pub fn parse_workbook(
    body: &str,
    body_line: usize,
    fm: FrontMatter,
    base_dir: Option<&Path>,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Document, ParseError> {
    let mut sheets = Vec::new();
    let mut sections = split_sheets(body, body_line)?;
    if sections.is_empty() {
        // Empty workbook body → one empty sheet.
        sheets.push(Sheet::new("Sheet1"));
    } else {
        for section in sections.drain(..) {
            sheets.push(parse_sheet(section, base_dir, assets, report)?);
        }
    }

    report.stats.sheets = sheets.len() as u32;
    report.stats.cells = sheets.iter().map(|s| s.cells.len() as u32).sum();
    report.stats.formulas = sheets
        .iter()
        .flat_map(|s| s.cells.values())
        .filter(|c| c.formula.is_some())
        .count() as u32;
    report.stats.images = sheets.iter().map(|s| s.images.len() as u32).sum();
    report.stats.styles = fm.styles.styles.len() as u32;

    Ok(Document::Workbook(Workbook {
        meta: fm.meta,
        styles: fm.styles,
        defined_names: fm.defined_names,
        sheets,
        active_sheet: fm.active_sheet,
    }))
}

struct SheetSection {
    heading_line: usize,
    heading: String,
    body: String,
}

fn split_sheets(body: &str, body_line: usize) -> Result<Vec<SheetSection>, ParseError> {
    let lines: Vec<&str> = body.lines().collect();
    let mut sections = Vec::new();
    let mut i = 0usize;
    // Skip leading blanks
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        if !trimmed.starts_with('#') || trimmed.starts_with("##") {
            // Content before first sheet heading is ignored with a soft approach:
            // only H1 starts a sheet.
            i += 1;
            continue;
        }
        let heading_line = body_line + i;
        let heading = trimmed.to_string();
        i += 1;
        let start = i;
        while i < lines.len() {
            let t = lines[i].trim();
            if t.starts_with('#') && !t.starts_with("##") {
                break;
            }
            i += 1;
        }
        let body = lines[start..i].join("\n");
        sections.push(SheetSection {
            heading_line,
            heading,
            body,
        });
    }
    Ok(sections)
}

fn parse_sheet(
    section: SheetSection,
    base_dir: Option<&Path>,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Sheet, ParseError> {
    let (name, attrs) = parse_heading(&section.heading, section.heading_line)?;
    let mut sheet = Sheet::new(name);

    if let Some(frozen) = attrs.get("frozen") {
        if let Some(cell) = CellRef::parse_a1(frozen) {
            sheet.pane = Some(Pane {
                top_left: cell,
                frozen: true,
            });
        }
    }
    if attrs.get("hidden") == Some("true") {
        sheet.hidden = true;
    }

    // Column widths from heading attrs, mapped via cols= if present.
    let col_letters = attrs.get("cols").map(parse_col_span).unwrap_or(None);
    if let (Some((min_col, _)), Some(widths)) = (col_letters, attrs.get("col-widths")) {
        for (offset, part) in widths.split(',').enumerate() {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Ok(w) = part.parse::<f64>() {
                sheet.cols.insert(
                    min_col + offset as u32,
                    ColProps {
                        width_chars: Some(w),
                        hidden: None,
                    },
                );
            }
        }
    }

    let mut rest = section.body.as_str();
    // Optional blank lines
    rest = rest.trim_start_matches('\n');

    // Value table
    if rest.trim_start().starts_with('|') {
        let (table_text, after) = take_table(rest);
        parse_value_table(&mut sheet, table_text, section.heading_line)?;
        rest = after;
    }

    // Containers: cell-meta, sheet-images, raw
    while !rest.trim().is_empty() {
        rest = rest.trim_start_matches('\n').trim_start();
        if rest.is_empty() {
            break;
        }
        if rest.starts_with(":::") {
            let (fattrs, fbody, after) = split_fence(rest, section.heading_line)?;
            rest = after;
            if fattrs.has_class("cell-meta") {
                apply_cell_meta(&mut sheet, &fbody, report)?;
            } else if fattrs.has_class("sheet-images") {
                parse_sheet_images(&mut sheet, &fbody, base_dir, assets, report)?;
            } else if fattrs.has_class("raw") {
                let raw = parse_raw(&fattrs, &fbody);
                sheet.raw.push(raw);
                report.raw_blocks_emitted = report.raw_blocks_emitted.saturating_add(1);
            }
            continue;
        }
        // Unknown trailing content: skip line
        if let Some(pos) = rest.find('\n') {
            rest = &rest[pos + 1..];
        } else {
            break;
        }
    }

    Ok(sheet)
}

fn parse_heading(line: &str, line_no: usize) -> Result<(String, Attrs), ParseError> {
    let rest = line
        .strip_prefix('#')
        .ok_or_else(|| ParseError::unexpected(line_no, "expected sheet heading"))?
        .trim_start();
    // Split trailing attrs `{...}`
    if let Some(idx) = rest.rfind('{') {
        let (name, attrs_src) = rest.split_at(idx);
        let name = unescape(name.trim());
        let attrs = Attrs::parse(attrs_src).unwrap_or_default();
        Ok((name, attrs))
    } else {
        Ok((unescape(rest.trim()), Attrs::new()))
    }
}

fn parse_col_span(text: &str) -> Option<(u32, u32)> {
    let text = text.trim();
    if let Some((a, b)) = text.split_once(':') {
        let start = col_index(a.trim())?;
        let end = col_index(b.trim())?;
        Some((start, end))
    } else {
        let c = col_index(text)?;
        Some((c, c))
    }
}

fn col_index(letters: &str) -> Option<u32> {
    if letters.is_empty() || !letters.bytes().all(|b| b.is_ascii_uppercase()) {
        return None;
    }
    let mut col: u64 = 0;
    for b in letters.bytes() {
        col = col * 26 + (b - b'A' + 1) as u64;
    }
    if col == 0 {
        None
    } else {
        Some((col - 1) as u32)
    }
}

fn take_table(text: &str) -> (&str, &str) {
    let mut end = 0usize;
    for (idx, line) in text.lines().enumerate() {
        let t = line.trim();
        if idx == 0 && !t.contains('|') {
            break;
        }
        if t.is_empty() {
            // end of table on blank line
            break;
        }
        if !t.contains('|') {
            break;
        }
        end += line.len() + 1; // include newline
    }
    if end == 0 {
        return ("", text);
    }
    if end > text.len() {
        end = text.len();
    }
    // trim trailing newline from table slice bookkeeping
    let table = &text[..end.min(text.len())];
    let after = if end < text.len() { &text[end..] } else { "" };
    (table, after)
}

fn parse_value_table(sheet: &mut Sheet, text: &str, line_no: usize) -> Result<(), ParseError> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || !line.contains('|') {
            continue;
        }
        let cells = split_row(line);
        if cells
            .iter()
            .all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' '))
            && cells.iter().any(|c| c.contains('-'))
        {
            continue; // delimiter
        }
        rows.push(cells);
    }
    if rows.is_empty() {
        return Ok(());
    }
    // Header: first cell empty-ish, rest are column letters
    let header = &rows[0];
    if header.is_empty() {
        return Err(ParseError::unexpected(line_no, "empty sheet table header"));
    }
    let mut col_map: Vec<u32> = Vec::new();
    for (i, cell) in header.iter().enumerate() {
        if i == 0 {
            continue;
        }
        let letters = cell.trim();
        if let Some(c) = col_index(letters) {
            col_map.push(c);
        } else {
            // fall back to sequential from 0
            col_map.push((i - 1) as u32);
        }
    }

    for row in rows.iter().skip(1) {
        if row.is_empty() {
            continue;
        }
        let row_label = strip_md_bold(row[0].trim());
        let row_num: u32 = match row_label.parse::<u32>() {
            Ok(n) if n > 0 => n - 1,
            _ => continue,
        };
        for (idx, cell_text) in row.iter().skip(1).enumerate() {
            let Some(&col) = col_map.get(idx) else {
                continue;
            };
            let text = unescape(&cell_text.trim().replace("\\|", "|"));
            if text.is_empty() {
                continue;
            }
            let cref = CellRef::new(col, row_num);
            let mut cell = sheet.cells.remove(&cref).unwrap_or_default();
            cell.value = CellValue::Text(text);
            sheet.cells.insert(cref, cell);
        }
    }
    Ok(())
}

fn strip_md_bold(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix("**").unwrap_or(s);
    let s = s.strip_suffix("**").unwrap_or(s);
    s.to_string()
}

fn split_row(line: &str) -> Vec<String> {
    let line = line.trim().trim_matches('|');
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                cur.push('\\');
                cur.push(n);
            } else {
                cur.push('\\');
            }
            continue;
        }
        if c == '|' {
            cells.push(cur.trim().to_string());
            cur.clear();
            continue;
        }
        cur.push(c);
    }
    cells.push(cur.trim().to_string());
    cells
}

fn apply_cell_meta(
    sheet: &mut Sheet,
    body: &str,
    report: &mut ConversionReport,
) -> Result<(), ParseError> {
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let line = line.strip_prefix('-').unwrap_or(line).trim();
        let Some((range, attrs_src)) = split_meta_line(line) else {
            continue;
        };
        let Some(range) = parse_cell_range(&range) else {
            continue;
        };
        let attrs = parse_meta_attrs(&attrs_src);

        let is_merge = attrs.get("merge") == Some("true");
        if is_merge && !sheet.merges.contains(&range) {
            sheet.merges.push(range);
        }

        // Apply cell attributes to every cell in the range. Merges without other
        // meta only need the range recorded above.
        //
        // When merge=true is combined with style/type/…, only the top-left cell
        // receives those attributes — the other covered cells are absent in the
        // IR (as in OOXML), so re-serializing stays stable.
        let has_cell_attrs = attrs.get("formula").is_some()
            || attrs.get("type").is_some()
            || attrs.get("num-fmt").is_some()
            || attrs.get("style").is_some();
        if !has_cell_attrs {
            continue;
        }

        let targets: Vec<CellRef> = if is_merge {
            vec![range.start]
        } else {
            let mut cells = Vec::new();
            for r in range.start.row..=range.end.row {
                for c in range.start.col..=range.end.col {
                    cells.push(CellRef::new(c, r));
                }
            }
            cells
        };

        for cref in targets {
            let mut cell = sheet.cells.remove(&cref).unwrap_or_default();
            apply_meta_to_cell(&mut cell, &attrs, cref == range.start, report);
            if !cell_is_blank(&cell) {
                sheet.cells.insert(cref, cell);
            }
        }
    }
    Ok(())
}

/// Splits `A1: attrs` / `A1:B2: attrs` / `A1:B2:merge=true`.
fn split_meta_line(line: &str) -> Option<(String, String)> {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    // start cell
    let start = i;
    while i < bytes.len() && (bytes[i].is_ascii_uppercase() || bytes[i] == b'$') {
        i += 1;
    }
    let letters_end = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if letters_end == start || i == letters_end {
        return None;
    }
    // optional :end cell
    if i < bytes.len() && bytes[i] == b':' {
        let after = i + 1;
        let mut j = after;
        while j < bytes.len() && (bytes[j].is_ascii_uppercase() || bytes[j] == b'$') {
            j += 1;
        }
        let end_letters = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if end_letters > after && j > end_letters {
            // confirmed range A1:B2
            i = j;
        }
    }
    let range = line[..i].to_string();
    let rest = line[i..].trim_start();
    let attrs = rest.strip_prefix(':').unwrap_or(rest).trim();
    Some((range, attrs.to_string()))
}

fn parse_cell_range(text: &str) -> Option<CellRange> {
    let text = text.trim();
    if let Some((a, b)) = text.split_once(':') {
        let start = CellRef::parse_a1(a.trim())?;
        let end = CellRef::parse_a1(b.trim())?;
        Some(CellRange::new(start, end))
    } else {
        let c = CellRef::parse_a1(text)?;
        Some(CellRange::new(c, c))
    }
}

fn parse_meta_attrs(text: &str) -> Attrs {
    // Reuse Attrs::parse by wrapping
    let wrapped = format!("{{{text}}}");
    Attrs::parse(&wrapped).unwrap_or_default()
}

fn apply_meta_to_cell(
    cell: &mut Cell,
    attrs: &Attrs,
    is_origin: bool,
    report: &mut ConversionReport,
) {
    if let Some(formula) = attrs.get("formula") {
        // Only origin cell of a shared range keeps the formula text fully;
        // for simplicity every cell in range gets the same formula if listed.
        if is_origin || cell.formula.is_none() {
            let dialect = match attrs.get("formula-dialect") {
                Some("openformula") => FormulaDialect::OpenFormula,
                _ => FormulaDialect::Ooxml,
            };
            let shared_over = attrs.get("shared-over").and_then(parse_cell_range);
            let array_over = attrs.get("array-over").and_then(parse_cell_range);
            cell.formula = Some(Formula {
                text: formula.to_string(),
                dialect,
                shared_over,
                array_over,
            });
            report.stats.formulas = report.stats.formulas.saturating_add(1);
        }
    }
    if let Some(t) = attrs.get("type") {
        cell.value = coerce_value(&cell.value, t);
    }
    if let Some(fmt) = attrs.get("num-fmt") {
        cell.num_fmt = Some(NumFmt {
            code: fmt.to_string(),
            id: None,
        });
    }
    if let Some(style) = attrs.get("style") {
        cell.style = Some(StyleId::new(style));
    }
}

fn coerce_value(current: &CellValue, type_kw: &str) -> CellValue {
    let text = match current {
        CellValue::Text(t) => t.clone(),
        CellValue::Number(n) => trim_num(*n),
        CellValue::Bool(true) => "true".into(),
        CellValue::Bool(false) => "false".into(),
        CellValue::DateTime(s) => s.clone(),
        CellValue::Error(e) => e.clone(),
        CellValue::Empty => String::new(),
    };
    match type_kw {
        "number" => {
            if let Ok(n) = text.parse::<f64>() {
                CellValue::Number(n)
            } else {
                CellValue::Text(text)
            }
        }
        "bool" => match text.to_ascii_lowercase().as_str() {
            "true" | "1" => CellValue::Bool(true),
            "false" | "0" => CellValue::Bool(false),
            _ => CellValue::Text(text),
        },
        "date" => CellValue::DateTime(text),
        "error" => CellValue::Error(text),
        "text" => CellValue::Text(text),
        _ => current.clone(),
    }
}

fn cell_is_blank(cell: &Cell) -> bool {
    cell.value.is_empty()
        && cell.formula.is_none()
        && cell.num_fmt.is_none()
        && cell.style.is_none()
}

fn trim_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        let s = format!("{n:.10}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn parse_sheet_images(
    sheet: &mut Sheet,
    body: &str,
    base_dir: Option<&Path>,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<(), ParseError> {
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with("![") {
            continue;
        }
        if let Some(img) = parse_image_line(line, base_dir, assets)? {
            sheet.images.push(img);
            report.stats.images = report.stats.images.saturating_add(1);
        }
    }
    Ok(())
}

fn resolve_asset_path(
    path: &str,
    base_dir: Option<&Path>,
    assets: &mut dyn AssetStore,
) -> Result<docsai_model::assets::AssetId, ParseError> {
    if let Some(base) = base_dir {
        let full = base.join(path);
        if full.is_file() {
            let bytes = std::fs::read(&full).map_err(|e| ParseError::io(Some(full.clone()), e))?;
            return assets.put(&bytes).map_err(Into::into);
        }
    }
    let p = Path::new(path);
    if p.is_file() {
        let bytes = std::fs::read(p).map_err(|e| ParseError::io(Some(p.to_path_buf()), e))?;
        return assets.put(&bytes).map_err(Into::into);
    }
    // Round-trip path: assets already seeded in the store (MemoryAssetStore).
    let file_name = p.file_name().and_then(|n| n.to_str()).unwrap_or(path);
    for id in assets.ids() {
        if assets
            .info(&id)
            .is_some_and(|info| info.file_name == file_name)
        {
            return Ok(id);
        }
    }
    Ok(docsai_model::assets::AssetId::new(format!(
        "missing-{}",
        path.replace('/', "_")
    )))
}

fn parse_image_line(
    text: &str,
    base_dir: Option<&Path>,
    assets: &mut dyn AssetStore,
) -> Result<Option<ImageRef>, ParseError> {
    // ![alt](path){attrs}
    let text = text.trim();
    let rest = match text.strip_prefix("![") {
        Some(r) => r,
        None => return Ok(None),
    };
    let alt_end = match find_unescaped(rest, ']') {
        Some(i) => i,
        None => return Ok(None),
    };
    let alt = unescape(&rest[..alt_end]);
    let after_alt = &rest[alt_end + 1..];
    let after_alt = after_alt.trim_start();
    let path_body = match after_alt.strip_prefix('(') {
        Some(r) => r,
        None => return Ok(None),
    };
    let path_end = match find_unescaped(path_body, ')') {
        Some(i) => i,
        None => return Ok(None),
    };
    let path = &path_body[..path_end];
    let after_path = path_body[path_end + 1..].trim_start();
    let attrs = if after_path.starts_with('{') {
        Attrs::parse(after_path).unwrap_or_default()
    } else {
        Attrs::new()
    };

    let asset_id = resolve_asset_path(path, base_dir, assets)?;

    let width = attrs
        .get("width")
        .and_then(Length::parse)
        .unwrap_or(Length::ZERO);
    let height = attrs
        .get("height")
        .and_then(Length::parse)
        .unwrap_or(Length::ZERO);
    let mut geometry = ImageGeometry::inline(Size::new(width, height));
    if let Some(ns) = attrs.get("native-size") {
        if let Some((w, h)) = ns.split_once('x') {
            if let (Ok(w), Ok(h)) = (w.parse(), h.parse()) {
                geometry.native_size_px = Some((w, h));
            }
        }
    }
    geometry.anchor = parse_sheet_anchor(&attrs);
    if let Some(rot) = attrs
        .get("rotation")
        .or_else(|| attrs.get("rotate"))
        .and_then(|v| v.parse().ok())
    {
        geometry.rotation_deg = rot;
    }
    if let Some(flip) = attrs.get("flip") {
        geometry.flip = match flip {
            "h" => Flip::H,
            "v" => Flip::V,
            "hv" => Flip::HV,
            _ => Flip::None,
        };
    }
    if let Some(crop) = attrs.get("crop") {
        let parts: Vec<_> = crop.split(',').collect();
        if parts.len() == 4 {
            let parse_pct = |s: &str| s.trim().trim_end_matches('%').parse::<f32>().unwrap_or(0.0);
            geometry.crop = Some(CropRect {
                left: parse_pct(parts[0]),
                top: parse_pct(parts[1]),
                right: parse_pct(parts[2]),
                bottom: parse_pct(parts[3]),
            });
        }
    }
    if let Some(border) = attrs.get("border") {
        let parts: Vec<_> = border.split_whitespace().collect();
        if parts.len() >= 3 {
            geometry.border = Some(SimpleBorder {
                width: Length::parse(parts[0]).unwrap_or(Length::ZERO),
                style: parts[1].to_string(),
                color: parts[2].to_string(),
            });
        }
    }

    let mut image = ImageRef::new(asset_id, geometry);
    image.alt = alt;
    image.title = attrs.get("title").map(str::to_string);
    image.name = attrs.get("name").map(str::to_string);
    image.link = attrs.get("link").map(str::to_string);
    image.external_src = attrs.get("external-src").map(str::to_string);
    Ok(Some(image))
}

fn parse_sheet_anchor(attrs: &Attrs) -> Anchor {
    match attrs.get("anchor").unwrap_or("one-cell") {
        "two-cell" => {
            let from = parse_cell_anchor(attrs.get("from"), attrs.get("from-offset"));
            let to = parse_cell_anchor(attrs.get("to"), attrs.get("to-offset"));
            let move_with_cells = attrs.get("move-with-cells") != Some("false");
            let size_with_cells = attrs.get("size-with-cells") == Some("true");
            Anchor::SheetTwoCell {
                from,
                to,
                move_with_cells,
                size_with_cells,
            }
        }
        "absolute" => Anchor::SheetAbsolute {
            pos: Point::new(
                attrs
                    .get("x")
                    .and_then(Length::parse)
                    .unwrap_or(Length::ZERO),
                attrs
                    .get("y")
                    .and_then(Length::parse)
                    .unwrap_or(Length::ZERO),
            ),
        },
        _ => {
            let from = parse_cell_anchor(attrs.get("from"), attrs.get("from-offset"));
            Anchor::SheetOneCell { from }
        }
    }
}

fn parse_cell_anchor(cell: Option<&str>, offset: Option<&str>) -> CellAnchor {
    let cell = cell
        .and_then(CellRef::parse_a1)
        .unwrap_or(CellRef::new(0, 0));
    let (ox, oy) = match offset {
        Some(o) => {
            let mut parts = o.split(',');
            let x = parts.next().and_then(Length::parse).unwrap_or(Length::ZERO);
            let y = parts.next().and_then(Length::parse).unwrap_or(Length::ZERO);
            (x, y)
        }
        None => (Length::ZERO, Length::ZERO),
    };
    CellAnchor::new(cell, ox, oy)
}

fn find_unescaped(s: &str, target: char) -> Option<usize> {
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == target {
            return Some(i);
        }
    }
    None
}

fn split_fence(text: &str, line: usize) -> Result<(Attrs, String, &str), ParseError> {
    let first_end = text.find('\n').unwrap_or(text.len());
    let header = text[..first_end].trim();
    let attr_src = header.trim_start_matches(':').trim();
    let attrs = Attrs::parse(attr_src).unwrap_or_default();
    let rest = if first_end < text.len() {
        &text[first_end + 1..]
    } else {
        ""
    };
    let mut body_lines: Vec<&str> = Vec::new();
    let mut depth = 1i32;
    let mut consumed = 0usize;
    for l in rest.lines() {
        let t = l.trim();
        let line_len = l.len() + 1;
        if t.starts_with(":::") {
            let inner = t.trim_start_matches(':').trim();
            if inner.is_empty() {
                depth -= 1;
                if depth == 0 {
                    consumed += line_len;
                    break;
                }
                body_lines.push(l);
                consumed += line_len;
                continue;
            } else {
                depth += 1;
            }
        }
        body_lines.push(l);
        consumed += line_len;
    }
    if depth != 0 {
        return Err(ParseError::unexpected(line, "unclosed fence"));
    }
    let body = body_lines.join("\n");
    let after = if consumed <= rest.len() {
        &rest[consumed..]
    } else {
        ""
    };
    Ok((attrs, body, after))
}

fn parse_raw(attrs: &Attrs, body: &str) -> RawFragment {
    RawFragment {
        id: attrs
            .id_ref()
            .map(RawId::new)
            .unwrap_or_else(|| RawId::new("raw")),
        format: attrs.get("format").unwrap_or("ooxml").to_string(),
        part: attrs.get("part").unwrap_or("").to_string(),
        content: body.to_string(),
    }
}

// Silence unused import warning when PathBuf not needed in some builds.
#[allow(dead_code)]
fn _pb() -> PathBuf {
    PathBuf::new()
}

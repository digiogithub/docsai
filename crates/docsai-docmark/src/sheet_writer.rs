//! Workbook → DocMark (spec §4).

use std::collections::BTreeMap;

use docsai_model::assets::AssetStore;
use docsai_model::image::{Anchor, ImageRef};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::sheet::{Cell, CellRange, CellRef, CellValue, FormulaDialect, Sheet, Workbook};

use crate::attrs::Attrs;
use crate::escape::{escape, escape_attr_value, is_bare_value, TextContext};
use crate::ids::IdSource;
use crate::units::{len, number, percent};
use crate::{Fidelity, Options};

/// Serialises a workbook body (front matter excluded).
pub fn write_workbook(
    book: &Workbook,
    assets: &dyn AssetStore,
    options: &Options,
    ids: &mut IdSource,
) -> (String, ConversionReport) {
    let mut out = String::new();
    let mut report = ConversionReport::new();
    report.stats.sheets = book.sheets.len() as u32;
    report.stats.styles = book.styles.styles.len() as u32;

    for (index, sheet) in book.sheets.iter().enumerate() {
        if index > 0 {
            // Separate sheets with a blank line.
            if !out.ends_with("\n\n") {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }
        }
        write_sheet(&mut out, sheet, assets, options, &mut report, ids);
    }

    while out.ends_with("\n\n") {
        out.pop();
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    (out, report)
}

fn write_sheet(
    out: &mut String,
    sheet: &Sheet,
    assets: &dyn AssetStore,
    options: &Options,
    report: &mut ConversionReport,
    ids: &mut IdSource,
) {
    let plain = options.fidelity == Fidelity::Plain;
    let full = options.fidelity == Fidelity::Full;

    // Heading
    if plain {
        out.push_str(&format!(
            "# {}\n\n",
            escape(&sheet.name, TextContext::Block)
        ));
    } else {
        let mut attrs = Attrs::new();
        attrs.class("sheet");
        if let Some(id) = ids.take(sheet) {
            attrs.id(id);
        }
        if let Some(range) = sheet.used_range() {
            attrs.set(
                "cols",
                format!(
                    "{}:{}",
                    col_letter(range.start.col),
                    col_letter(range.end.col)
                ),
            );
        }
        if !sheet.cols.is_empty() {
            if let Some(range) = sheet.used_range() {
                let mut widths = Vec::new();
                for c in range.start.col..=range.end.col {
                    let w = sheet
                        .cols
                        .get(&c)
                        .and_then(|p| p.width_chars)
                        .map(trim_num)
                        .unwrap_or_else(|| "".into());
                    widths.push(w);
                }
                if widths.iter().any(|w| !w.is_empty()) {
                    attrs.set("col-widths", widths.join(","));
                }
            }
        }
        if let Some(pane) = &sheet.pane {
            if pane.frozen {
                attrs.set("frozen", pane.top_left.a1());
            }
        }
        if sheet.hidden {
            attrs.set("hidden", "true");
        }
        let rendered = attrs.render();
        if rendered.is_empty() {
            out.push_str(&format!(
                "# {}\n\n",
                escape(&sheet.name, TextContext::Block)
            ));
        } else {
            out.push_str(&format!(
                "# {} {}\n\n",
                escape(&sheet.name, TextContext::Block),
                rendered
            ));
        }
    }

    // Value table
    if let Some(range) = sheet.used_range() {
        write_table(out, sheet, range);
        report.stats.cells = report.stats.cells.saturating_add(sheet.cells.len() as u32);
    } else if !plain {
        out.push_str("| | |\n|---|---|\n| **1** |  |\n\n");
    }

    // cell-meta
    if full {
        let meta_lines = collect_cell_meta(sheet, report);
        if !meta_lines.is_empty() {
            out.push_str("::: {.cell-meta}\n");
            for line in meta_lines {
                out.push_str(&line);
                out.push('\n');
            }
            out.push_str(":::\n\n");
        }
    }

    // sheet-images
    if !sheet.images.is_empty() && !plain {
        out.push_str("::: {.sheet-images}\n");
        for image in &sheet.images {
            out.push_str(&render_image(image, assets, options, report, ids));
            out.push('\n');
            if options.fidelity == Fidelity::Full {
                // blank line between images for readability like the spec
                out.push('\n');
            }
        }
        // trim trailing extra blank
        while out.ends_with("\n\n\n") {
            out.pop();
        }
        out.push_str(":::\n");
        report.stats.images = report
            .stats
            .images
            .saturating_add(sheet.images.len() as u32);
    }

    for raw in &sheet.raw {
        if full {
            out.push_str(&format!(
                "::: {{.raw format={} id={}}}\n{}\n:::\n\n",
                raw.format,
                raw.id.as_str(),
                raw.content.trim_end()
            ));
            report.raw_blocks_emitted = report.raw_blocks_emitted.saturating_add(1);
        } else {
            report.warn(Warning::RawBlockDropped {
                id: raw.id.as_str().to_string(),
                format: raw.format.clone(),
            });
        }
    }
}

fn write_table(out: &mut String, sheet: &Sheet, range: CellRange) {
    let min_col = range.start.col;
    let max_col = range.end.col;
    let min_row = range.start.row;
    let max_row = range.end.row;
    let cols = (max_col - min_col + 1) as usize;

    // Header row: empty corner + column letters
    let mut header = vec![String::new()];
    for c in min_col..=max_col {
        header.push(col_letter(c));
    }
    let mut grid = vec![header];
    for r in min_row..=max_row {
        let mut row = vec![format!("**{}**", r + 1)];
        for c in min_col..=max_col {
            let text = sheet
                .cells
                .get(&CellRef::new(c, r))
                .map(display_value)
                .unwrap_or_default();
            row.push(escape(&text, TextContext::TableCell));
        }
        grid.push(row);
    }

    let widths = column_widths(&grid, cols + 1);
    out.push_str(&render_row(&grid[0], &widths));
    out.push_str(&render_delimiter(&widths));
    for row in grid.iter().skip(1) {
        out.push_str(&render_row(row, &widths));
    }
    out.push('\n');
}

fn display_value(cell: &Cell) -> String {
    match &cell.value {
        CellValue::Empty => String::new(),
        CellValue::Number(n) => trim_num(*n),
        CellValue::Text(t) => t.clone(),
        CellValue::Bool(true) => "true".into(),
        CellValue::Bool(false) => "false".into(),
        CellValue::DateTime(s) => s.clone(),
        CellValue::Error(e) => e.clone(),
    }
}

fn collect_cell_meta(sheet: &Sheet, report: &mut ConversionReport) -> Vec<String> {
    // Build per-cell attribute maps, then compact identical ranges.
    #[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct MetaKey {
        parts: Vec<(String, String)>,
    }

    let mut by_cell: BTreeMap<CellRef, MetaKey> = BTreeMap::new();

    for (cref, cell) in &sheet.cells {
        let mut parts: Vec<(String, String)> = Vec::new();
        if let Some(f) = &cell.formula {
            parts.push(("formula".into(), f.text.clone()));
            if f.dialect == FormulaDialect::OpenFormula {
                parts.push(("formula-dialect".into(), "openformula".into()));
            }
            if let Some(range) = f.shared_over {
                parts.push(("shared-over".into(), range.a1()));
            }
            if let Some(range) = f.array_over {
                parts.push(("array-over".into(), range.a1()));
            }
            report.stats.formulas = report.stats.formulas.saturating_add(1);
        }
        // type= when not plain text / empty, or when date/bool/error/number with fmt
        match &cell.value {
            CellValue::Number(_) => parts.push(("type".into(), "number".into())),
            CellValue::Bool(_) => parts.push(("type".into(), "bool".into())),
            CellValue::DateTime(_) => parts.push(("type".into(), "date".into())),
            CellValue::Error(_) => parts.push(("type".into(), "error".into())),
            CellValue::Text(_) | CellValue::Empty => {}
        }
        if let Some(fmt) = &cell.num_fmt {
            parts.push(("num-fmt".into(), fmt.code.clone()));
        }
        if let Some(style) = &cell.style {
            parts.push(("style".into(), style.as_str().to_string()));
        }
        if !parts.is_empty() {
            parts.sort();
            by_cell.insert(*cref, MetaKey { parts });
        }
    }

    // Merges
    for m in &sheet.merges {
        let entry = by_cell
            .entry(m.start)
            .or_insert_with(|| MetaKey { parts: Vec::new() });
        entry.parts.push(("merge".into(), "true".into()));
        entry.parts.sort();
        // Represent merge on the range key later
        let _ = m;
    }

    // Compact: group cells with identical meta into rectangular ranges when possible.
    // Simple approach: emit one line per cell, then merge consecutive identical rows in a column span.
    // For determinism and simplicity: emit merges as range lines, then group other meta by value.

    let mut lines = Vec::new();

    // Emit merges first as range: merge=true (and any other meta on top-left already included)
    let mut merge_starts: BTreeMap<CellRef, CellRange> = BTreeMap::new();
    for m in &sheet.merges {
        merge_starts.insert(m.start, *m);
    }

    // Group non-merge-only keys
    let mut groups: BTreeMap<MetaKey, Vec<CellRef>> = BTreeMap::new();
    for (cell, key) in &by_cell {
        groups.entry(key.clone()).or_default().push(*cell);
    }

    for (key, mut cells) in groups {
        cells.sort();
        // Prefer merge ranges when merge=true and the range matches a known merge
        let is_merge = key.parts.iter().any(|(k, v)| k == "merge" && v == "true");
        if is_merge {
            // Emit each contiguous? Use merge_starts if present
            let mut used = std::collections::BTreeSet::new();
            for cell in &cells {
                if used.contains(cell) {
                    continue;
                }
                if let Some(range) = merge_starts.get(cell) {
                    lines.push(format_meta_line(&range.a1(), &key.parts));
                    // mark all cells in range
                    for r in range.start.row..=range.end.row {
                        for c in range.start.col..=range.end.col {
                            used.insert(CellRef::new(c, r));
                        }
                    }
                } else {
                    lines.push(format_meta_line(&cell.a1(), &key.parts));
                    used.insert(*cell);
                }
            }
            continue;
        }

        // Compact identical meta over rectangular runs (row-major greedy).
        let ranges = compact_cells(&cells);
        for range in ranges {
            lines.push(format_meta_line(&range.a1(), &key.parts));
        }
    }

    lines.sort();
    lines
}

fn format_meta_line(range: &str, parts: &[(String, String)]) -> String {
    let mut attrs = String::new();
    for (k, v) in parts {
        attrs.push(' ');
        attrs.push_str(k);
        attrs.push('=');
        if needs_quotes(v) {
            attrs.push('"');
            attrs.push_str(&escape_attr_value(v));
            attrs.push('"');
        } else {
            attrs.push_str(v);
        }
    }
    format!("- {range}:{attrs}")
}

fn needs_quotes(v: &str) -> bool {
    !is_bare_value(v)
}

/// Greedy compaction of sorted cells into ranges (row spans of contiguous cols, then vertical if uniform).
fn compact_cells(cells: &[CellRef]) -> Vec<CellRange> {
    if cells.is_empty() {
        return Vec::new();
    }
    let set: std::collections::BTreeSet<_> = cells.iter().copied().collect();
    let mut remaining = set.clone();
    let mut ranges = Vec::new();

    while let Some(&start) = remaining.iter().next() {
        // Expand horizontally.
        let mut end_col = start.col;
        while remaining.contains(&CellRef::new(end_col + 1, start.row)) {
            end_col += 1;
        }
        // Expand vertically while every column in the strip is present.
        let mut end_row = start.row;
        'vert: loop {
            let next = end_row + 1;
            for c in start.col..=end_col {
                if !remaining.contains(&CellRef::new(c, next)) {
                    break 'vert;
                }
            }
            end_row = next;
        }
        for r in start.row..=end_row {
            for c in start.col..=end_col {
                remaining.remove(&CellRef::new(c, r));
            }
        }
        ranges.push(CellRange::new(start, CellRef::new(end_col, end_row)));
    }
    ranges
}

fn render_image(
    image: &ImageRef,
    assets: &dyn AssetStore,
    options: &Options,
    report: &mut ConversionReport,
    ids: &mut IdSource,
) -> String {
    let info = assets.info(&image.asset);
    let file_name = info
        .map(|i| i.file_name.clone())
        .unwrap_or_else(|| format!("{}.bin", image.asset.as_str()));
    let path = format!("{}/{}", options.assets_dir.trim_end_matches('/'), file_name);
    let alt = escape(&image.alt, TextContext::LinkLabel);
    let mut attrs = Attrs::new();
    if let Some(id) = ids.take(image) {
        attrs.id(id);
    }

    // Always set anchor for sheet images.
    match &image.geometry.anchor {
        Anchor::SheetTwoCell {
            from,
            to,
            move_with_cells,
            size_with_cells,
        } => {
            attrs
                .set("anchor", "two-cell")
                .set("from", from.cell.a1())
                .set(
                    "from-offset",
                    format!("{},{}", len(from.offset_x), len(from.offset_y)),
                )
                .set("to", to.cell.a1())
                .set(
                    "to-offset",
                    format!("{},{}", len(to.offset_x), len(to.offset_y)),
                )
                .set("move-with-cells", move_with_cells.to_string())
                .set("size-with-cells", size_with_cells.to_string());
            // width/height NOT serialized for two-cell (spec §4.1)
        }
        Anchor::SheetOneCell { from } => {
            attrs
                .set("anchor", "one-cell")
                .set("from", from.cell.a1())
                .set(
                    "from-offset",
                    format!("{},{}", len(from.offset_x), len(from.offset_y)),
                )
                .set("width", len(image.geometry.display_size.width))
                .set("height", len(image.geometry.display_size.height));
        }
        Anchor::SheetAbsolute { pos } => {
            attrs
                .set("anchor", "absolute")
                .set("x", len(pos.x))
                .set("y", len(pos.y))
                .set("width", len(image.geometry.display_size.width))
                .set("height", len(image.geometry.display_size.height));
        }
        other => {
            report.warn(Warning::ImageGeometryDegraded {
                what: image.name.clone().unwrap_or_else(|| "image".into()),
                why: format!("unexpected anchor {}", other.keyword()),
            });
            attrs
                .set("anchor", "one-cell")
                .set("from", "A1")
                .set("width", len(image.geometry.display_size.width))
                .set("height", len(image.geometry.display_size.height));
        }
    }

    if image.geometry.rotation_deg != 0.0 {
        attrs.set("rotate", number(image.geometry.rotation_deg));
    }
    if !image.geometry.flip.is_none() {
        attrs.set("flip", image.geometry.flip.as_str());
    }
    if let Some(crop) = &image.geometry.crop {
        attrs.set(
            "crop",
            format!(
                "{},{},{},{}",
                percent(crop.left),
                percent(crop.top),
                percent(crop.right),
                percent(crop.bottom)
            ),
        );
    }
    if let Some((w, h)) = image.geometry.native_size_px {
        attrs.set("native-size", format!("{w}x{h}"));
    }
    attrs.set_opt("name", image.name.clone());
    attrs.set_opt("title", image.title.clone());
    attrs.set_opt("link", image.link.clone());
    attrs.set_opt("external-src", image.external_src.clone());

    format!("![{alt}]({path}){}", attrs.render())
}

fn col_letter(col: u32) -> String {
    CellRef::new(col, 0)
        .a1()
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect()
}

fn trim_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        let s = format!("{n:.10}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn column_widths(grid: &[Vec<String>], columns: usize) -> Vec<usize> {
    const MAX_PADDED_COLUMNS: usize = 120;
    if columns > MAX_PADDED_COLUMNS {
        return vec![0; columns];
    }
    let mut widths = vec![3usize; columns];
    for row in grid {
        for (index, cell) in row.iter().enumerate() {
            if index < columns {
                widths[index] = widths[index].max(cell.chars().count());
            }
        }
    }
    widths
}

fn render_row(row: &[String], widths: &[usize]) -> String {
    let cells: Vec<String> = row
        .iter()
        .zip(widths)
        .map(|(cell, width)| {
            let pad = width.saturating_sub(cell.chars().count());
            format!("{cell}{}", " ".repeat(pad))
        })
        .collect();
    format!("| {} |\n", cells.join(" | "))
}

fn render_delimiter(widths: &[usize]) -> String {
    let cells: Vec<String> = widths.iter().map(|w| "-".repeat((*w).max(3))).collect();
    format!("| {} |\n", cells.join(" | "))
}

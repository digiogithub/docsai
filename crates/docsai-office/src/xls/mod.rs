//! Legacy `.xls` (BIFF8) reading via calamine (Phase 3).
//!
//! Write support is out of scope. Styles and drawings are not available from
//! the BIFF layer with enough fidelity, so the IR carries values/formulas only
//! and the conversion report records the degradation.

use std::io::{Read, Seek};

use calamine::{open_workbook_from_rs, Data, Reader, Xls};
use docsai_model::assets::AssetStore;
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::sheet::{
    Cell, CellRef, CellValue, Formula as IrFormula, FormulaDialect, Sheet, Workbook,
};
use docsai_model::Document;

use crate::error::ReadError;

/// Reads a `.xls` workbook into the IR.
pub fn read<R: Read + Seek>(
    reader: R,
    _assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    let mut report = ConversionReport::new();
    report.warn(Warning::Degraded {
        what: "xls".into(),
        why: "legacy BIFF8 path is values/formulas only; styles and drawings are not mapped".into(),
    });

    let mut workbook: Xls<_> =
        open_workbook_from_rs(reader).map_err(|e| ReadError::WrongShape {
            part: "xls".into(),
            expected: format!("BIFF8 workbook ({e})"),
        })?;

    let mut book = Workbook::default();
    let sheet_names = workbook.sheet_names().to_vec();
    for name in sheet_names {
        let mut sheet = Sheet::new(name.clone());
        let values = match workbook.worksheet_range(&name) {
            Ok(v) => v,
            Err(err) => {
                report.warn(Warning::Degraded {
                    what: format!("sheet `{name}`"),
                    why: err.to_string(),
                });
                continue;
            }
        };
        let formulas = workbook.worksheet_formula(&name).ok();
        absorb_range(&mut sheet, &values, formulas.as_ref(), &mut report);
        report.stats.sheets = report.stats.sheets.saturating_add(1);
        book.sheets.push(sheet);
    }
    book.active_sheet = book.sheets.first().map(|s| s.name.clone());
    Ok((Document::Workbook(book), report))
}

fn absorb_range(
    sheet: &mut Sheet,
    values: &calamine::Range<Data>,
    formulas: Option<&calamine::Range<String>>,
    report: &mut ConversionReport,
) {
    let (height, width) = values.get_size();
    for row in 0..height {
        for col in 0..width {
            let value = values.get((row, col)).cloned().unwrap_or(Data::Empty);
            let formula_text = formulas
                .and_then(|f| f.get((row, col)))
                .cloned()
                .filter(|s| !s.is_empty());
            let cell_value = data_to_value(value);
            if cell_value.is_empty() && formula_text.is_none() {
                continue;
            }
            let mut cell = Cell {
                value: cell_value,
                ..Default::default()
            };
            if let Some(text) = formula_text {
                let text = text.strip_prefix('=').unwrap_or(&text).to_string();
                cell.formula = Some(IrFormula {
                    text,
                    dialect: FormulaDialect::Ooxml,
                    shared_over: None,
                    array_over: None,
                });
                report.stats.formulas = report.stats.formulas.saturating_add(1);
            }
            sheet
                .cells
                .insert(CellRef::new(col as u32, row as u32), cell);
            report.stats.cells = report.stats.cells.saturating_add(1);
        }
    }
}

fn data_to_value(data: Data) -> CellValue {
    match data {
        Data::Empty => CellValue::Empty,
        Data::String(s) => CellValue::Text(s),
        Data::Float(f) => CellValue::Number(f),
        Data::Int(i) => CellValue::Number(i as f64),
        Data::Bool(b) => CellValue::Bool(b),
        Data::Error(e) => CellValue::Error(format!("{e}")),
        Data::DateTime(dt) => {
            let serial = dt.as_f64();
            match crate::xlsx::excel_serial_to_iso(serial) {
                Some(iso) => CellValue::DateTime(iso),
                None => CellValue::Number(serial),
            }
        }
        Data::DateTimeIso(s) => CellValue::DateTime(s),
        Data::DurationIso(s) => CellValue::Text(s),
    }
}

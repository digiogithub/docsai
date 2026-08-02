//! Spreadsheet side of the IR.
//!
//! Populated from Phase 3 onwards; defined now because [`CellRef`] is part of
//! the shared image model and because the IR validator must know the
//! `Workbook`-only invariants from day one.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::image::ImageRef;
use crate::style::{StyleCatalog, StyleId};
use crate::text::{DocumentMeta, RawFragment};
use crate::units::Length;

/// A zero-based `(column, row)` cell coordinate.
///
/// Serialises as its A1 string (`"B2"`) rather than as a pair, so that it can
/// be a JSON map key — [`Sheet::cells`] is keyed by it — and so that
/// `inspect --json` stays readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellRef {
    pub col: u32,
    pub row: u32,
}

impl Serialize for CellRef {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.a1())
    }
}

impl<'de> Deserialize<'de> for CellRef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        CellRef::parse_a1(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid A1 reference `{text}`")))
    }
}

impl std::fmt::Display for CellRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.a1())
    }
}

impl CellRef {
    pub const fn new(col: u32, row: u32) -> Self {
        CellRef { col, row }
    }

    /// The A1-style reference (`B2`), as DocMark writes it.
    ///
    /// ```
    /// # use docsai_model::sheet::CellRef;
    /// assert_eq!(CellRef::new(0, 0).a1(), "A1");
    /// assert_eq!(CellRef::new(26, 9).a1(), "AA10");
    /// ```
    pub fn a1(&self) -> String {
        let mut name = String::new();
        let mut col = self.col as u64 + 1;
        while col > 0 {
            let rem = ((col - 1) % 26) as u8;
            name.insert(0, (b'A' + rem) as char);
            col = (col - 1) / 26;
        }
        name.push_str(&(self.row + 1).to_string());
        name
    }

    /// Parses an A1-style reference. `$` anchors are accepted and ignored.
    pub fn parse_a1(text: &str) -> Option<CellRef> {
        let text = text.replace('$', "");
        let split = text.find(|c: char| c.is_ascii_digit())?;
        let (letters, digits) = text.split_at(split);
        if letters.is_empty() || !letters.bytes().all(|b| b.is_ascii_uppercase()) {
            return None;
        }
        let mut col: u64 = 0;
        for b in letters.bytes() {
            col = col * 26 + (b - b'A' + 1) as u64;
        }
        let row: u32 = digits.parse().ok()?;
        if row == 0 || col == 0 {
            return None;
        }
        Some(CellRef::new((col - 1) as u32, row - 1))
    }
}

/// An inclusive rectangular range of cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CellRange {
    pub start: CellRef,
    pub end: CellRef,
}

impl CellRange {
    pub const fn new(start: CellRef, end: CellRef) -> Self {
        CellRange { start, end }
    }

    /// `A1:C3`, or just `A1` when the range is a single cell.
    pub fn a1(&self) -> String {
        if self.start == self.end {
            self.start.a1()
        } else {
            format!("{}:{}", self.start.a1(), self.end.a1())
        }
    }
}

/// The value stored in a cell.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "type", content = "value")]
pub enum CellValue {
    #[default]
    Empty,
    Number(f64),
    Text(String),
    Bool(bool),
    /// ISO-8601. Serial numbers are converted on read and back on write so a
    /// hand edit of the DocMark cannot corrupt them.
    DateTime(String),
    /// Spreadsheet error literal (`#DIV/0!`…).
    Error(String),
}

impl CellValue {
    pub fn is_empty(&self) -> bool {
        matches!(self, CellValue::Empty)
    }

    /// The DocMark `type=` keyword.
    pub fn type_keyword(&self) -> &'static str {
        match self {
            CellValue::Empty => "empty",
            CellValue::Number(_) => "number",
            CellValue::Text(_) => "text",
            CellValue::Bool(_) => "bool",
            CellValue::DateTime(_) => "date",
            CellValue::Error(_) => "error",
        }
    }
}

/// Which formula language a formula is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FormulaDialect {
    Ooxml,
    OpenFormula,
}

/// A cell formula, kept in its original dialect (risk R5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Formula {
    /// The expression *without* the leading `=`.
    pub text: String,
    pub dialect: FormulaDialect,
    /// Range this formula is shared over, when it was a shared formula.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_over: Option<CellRange>,
    /// Range this formula spills into, when it was an array formula.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub array_over: Option<CellRange>,
}

/// A number format code (`#,##0.00`), plus the source's numeric id when it had one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct NumFmt {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u32>,
}

/// One cell.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Cell {
    pub value: CellValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<Formula>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_fmt: Option<NumFmt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<StyleId>,
}

/// Column layout.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ColProps {
    /// Width in characters, the unit spreadsheets present to the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_chars: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

/// Row layout.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RowProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

/// A frozen/split pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Pane {
    /// Top-left cell of the unfrozen area.
    pub top_left: CellRef,
    pub frozen: bool,
}

/// A named range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DefinedName {
    pub name: String,
    /// The formula text of the reference (`Ventas!$D$10`), source dialect.
    pub refers_to: String,
    /// Sheet index when the name is sheet-scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<usize>,
}

/// A worksheet: a sparse grid plus layout and anchored images.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Sheet {
    pub name: String,
    /// Sparse cell grid; absent keys are empty cells.
    pub cells: BTreeMap<CellRef, Cell>,
    pub merges: Vec<CellRange>,
    pub cols: BTreeMap<u32, ColProps>,
    pub rows: BTreeMap<u32, RowProps>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<Pane>,
    /// Images anchored to this sheet.
    pub images: Vec<ImageRef>,
    /// Sheet-level fragments with no DocMark representation (charts…).
    pub raw: Vec<RawFragment>,
    pub hidden: bool,
}

impl Sheet {
    pub fn new(name: impl Into<String>) -> Self {
        Sheet {
            name: name.into(),
            ..Default::default()
        }
    }

    /// The used range, or `None` for an empty sheet.
    pub fn used_range(&self) -> Option<CellRange> {
        let mut iter = self.cells.keys();
        let first = *iter.next()?;
        let (mut min, mut max) = (first, first);
        for &c in iter {
            min.col = min.col.min(c.col);
            min.row = min.row.min(c.row);
            max.col = max.col.max(c.col);
            max.row = max.row.max(c.row);
        }
        Some(CellRange::new(min, max))
    }
}

/// A spreadsheet document.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Workbook {
    pub meta: DocumentMeta,
    pub styles: StyleCatalog,
    pub defined_names: Vec<DefinedName>,
    pub sheets: Vec<Sheet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_sheet: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a1_notation_round_trips() {
        for (col, row, text) in [
            (0u32, 0u32, "A1"),
            (1, 1, "B2"),
            (25, 0, "Z1"),
            (26, 0, "AA1"),
            (51, 99, "AZ100"),
            (702, 0, "AAA1"),
        ] {
            let r = CellRef::new(col, row);
            assert_eq!(r.a1(), text);
            assert_eq!(CellRef::parse_a1(text), Some(r));
        }
    }

    #[test]
    fn parse_a1_tolerates_absolute_refs_and_rejects_junk() {
        assert_eq!(CellRef::parse_a1("$D$10"), Some(CellRef::new(3, 9)));
        assert_eq!(CellRef::parse_a1("A0"), None);
        assert_eq!(CellRef::parse_a1("1A"), None);
        assert_eq!(CellRef::parse_a1(""), None);
        assert_eq!(
            CellRef::parse_a1("a1"),
            None,
            "lowercase is not A1 notation"
        );
    }

    #[test]
    fn ranges_collapse_when_single_cell() {
        let one = CellRange::new(CellRef::new(0, 0), CellRef::new(0, 0));
        assert_eq!(one.a1(), "A1");
        let many = CellRange::new(CellRef::new(1, 1), CellRef::new(3, 2));
        assert_eq!(many.a1(), "B2:D3");
    }

    #[test]
    fn used_range_covers_every_cell() {
        let mut sheet = Sheet::new("Hoja");
        sheet.cells.insert(CellRef::new(3, 9), Cell::default());
        sheet.cells.insert(CellRef::new(1, 2), Cell::default());
        assert_eq!(
            sheet.used_range(),
            Some(CellRange::new(CellRef::new(1, 2), CellRef::new(3, 9)))
        );
        assert_eq!(Sheet::new("vacia").used_range(), None);
    }
}

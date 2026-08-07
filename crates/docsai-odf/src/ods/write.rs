//! IR → `.ods` writer (Phase 4).

use std::collections::BTreeMap;
use std::io::{Seek, Write};

use docsai_model::assets::AssetStore;
use docsai_model::image::{Anchor, ImageRef};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::sheet::{Cell, CellRef, CellValue, FormulaDialect, Sheet, Workbook};
use docsai_model::text::DocumentMeta;
use docsai_model::Document;

use crate::length::format_cm;
use crate::odt::write::{esc_attr, esc_text};
use crate::package::Package;
use crate::write_error::WriteError;

const MIME: &str = "application/vnd.oasis.opendocument.spreadsheet";
const NS_OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const NS_STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
const NS_TEXT: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
const NS_TABLE: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
const NS_DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
const NS_FO: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
const NS_SVG: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
const NS_XLINK: &str = "http://www.w3.org/1999/xlink";
const NS_DC: &str = "http://purl.org/dc/elements/1.1/";
const NS_META: &str = "urn:oasis:names:tc:opendocument:xmlns:meta:1.0";
const NS_MANIFEST: &str = "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0";

/// Writes a workbook as an `.ods` package.
pub fn write_ods<W: Write + Seek>(
    document: &Document,
    assets: &dyn AssetStore,
    writer: W,
) -> Result<ConversionReport, WriteError> {
    let book = match document {
        Document::Workbook(w) => w,
        other => {
            return Err(WriteError::Invalid(format!(
                "cannot write {} as .ods",
                other.shape_name()
            )));
        }
    };
    let mut report = ConversionReport::new();
    let package = build_package(book, assets, &mut report)?;
    package.write_to(writer)?;
    Ok(report)
}

struct Media {
    pictures: BTreeMap<String, Vec<u8>>,
    seq: u32,
}

impl Media {
    fn store(&mut self, assets: &dyn AssetStore, img: &ImageRef) -> Result<String, WriteError> {
        if let Some(url) = &img.external_src {
            return Ok(url.clone());
        }
        let bytes = assets.get(&img.asset).ok_or_else(|| {
            WriteError::Asset(docsai_model::assets::AssetError::NotFound(
                img.asset.clone(),
            ))
        })?;
        let ext = assets
            .info(&img.asset)
            .map(|i| i.file_name.rsplit('.').next().unwrap_or("png").to_string())
            .unwrap_or_else(|| "png".into());
        self.seq += 1;
        let path = format!("Pictures/image{:04}.{}", self.seq, ext);
        self.pictures.insert(path.clone(), bytes.to_vec());
        Ok(path)
    }
}

fn build_package(
    book: &Workbook,
    assets: &dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Package, WriteError> {
    let mut media = Media {
        pictures: BTreeMap::new(),
        seq: 0,
    };

    let mut tables = String::new();
    for sheet in &book.sheets {
        tables.push_str(&write_sheet(sheet, assets, &mut media, report)?);
        report.stats.sheets = report.stats.sheets.saturating_add(1);
        report.stats.cells = report.stats.cells.saturating_add(sheet.cells.len() as u32);
    }
    if book.sheets.is_empty() {
        tables.push_str(
            r#"  <table:table table:name="Sheet1">
   <table:table-column/>
   <table:table-row><table:table-cell/></table:table-row>
  </table:table>
"#,
        );
    }

    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="{NS_OFFICE}" xmlns:style="{NS_STYLE}" xmlns:text="{NS_TEXT}" xmlns:table="{NS_TABLE}" xmlns:draw="{NS_DRAW}" xmlns:fo="{NS_FO}" xmlns:svg="{NS_SVG}" xmlns:xlink="{NS_XLINK}" office:version="1.3">
 <office:automatic-styles/>
 <office:body>
  <office:spreadsheet>
{tables}  </office:spreadsheet>
 </office:body>
</office:document-content>
"#
    );

    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-styles xmlns:office="{NS_OFFICE}" xmlns:style="{NS_STYLE}" xmlns:fo="{NS_FO}" office:version="1.3">
 <office:styles>
  <style:default-style style:family="table-cell">
   <style:table-cell-properties style:vertical-align="top"/>
   <style:text-properties fo:font-name="Liberation Sans" fo:font-size="10pt"/>
  </style:default-style>
 </office:styles>
 <office:automatic-styles>
  <style:page-layout style:name="pm1">
   <style:page-layout-properties fo:page-width="21cm" fo:page-height="29.7cm" fo:margin="1.5cm"/>
  </style:page-layout>
 </office:automatic-styles>
 <office:master-styles>
  <style:master-page style:name="Default" style:page-layout-name="pm1"/>
 </office:master-styles>
</office:document-styles>
"#
    );

    let mut package = Package::new();
    package.insert("mimetype", MIME.as_bytes());
    package.insert("content.xml", content.into_bytes());
    package.insert("styles.xml", styles.into_bytes());
    package.insert("meta.xml", write_meta(&book.meta).into_bytes());

    for (path, bytes) in &media.pictures {
        package.insert(path, bytes.clone());
    }

    let mut entries = vec![
        ("/".into(), MIME.to_string()),
        ("content.xml".into(), "text/xml".into()),
        ("styles.xml".into(), "text/xml".into()),
        ("meta.xml".into(), "text/xml".into()),
    ];
    for path in media.pictures.keys() {
        let mime = match path.rsplit('.').next().unwrap_or("") {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            _ => "application/octet-stream",
        };
        entries.push((path.clone(), mime.into()));
    }
    package.insert(
        "META-INF/manifest.xml",
        write_manifest(&entries).into_bytes(),
    );
    Ok(package)
}

fn write_sheet(
    sheet: &Sheet,
    assets: &dyn AssetStore,
    media: &mut Media,
    report: &mut ConversionReport,
) -> Result<String, WriteError> {
    let mut out = format!(
        r#"  <table:table table:name="{}">
"#,
        esc_attr(&sheet.name)
    );

    // Determine used range.
    let max_col = sheet
        .cells
        .keys()
        .map(|c| c.col)
        .chain(sheet.merges.iter().map(|m| m.end.col))
        .max()
        .unwrap_or(0);
    let max_row = sheet
        .cells
        .keys()
        .map(|c| c.row)
        .chain(sheet.merges.iter().map(|m| m.end.row))
        .max()
        .unwrap_or(0);

    // Build merge map: top-left → (colspan, rowspan); covered cells set.
    let mut merge_origin: BTreeMap<CellRef, (u32, u32)> = BTreeMap::new();
    let mut covered: BTreeMap<CellRef, bool> = BTreeMap::new();
    for m in &sheet.merges {
        let cs = m.end.col.saturating_sub(m.start.col).saturating_add(1);
        let rs = m.end.row.saturating_sub(m.start.row).saturating_add(1);
        merge_origin.insert(m.start, (cs, rs));
        for r in m.start.row..=m.end.row {
            for c in m.start.col..=m.end.col {
                if r == m.start.row && c == m.start.col {
                    continue;
                }
                covered.insert(CellRef::new(c, r), true);
            }
        }
    }

    // Images keyed by anchor cell.
    let mut images_at: BTreeMap<CellRef, Vec<&ImageRef>> = BTreeMap::new();
    for img in &sheet.images {
        let cell = match &img.geometry.anchor {
            Anchor::SheetOneCell { from } | Anchor::SheetTwoCell { from, .. } => from.cell,
            _ => CellRef::new(0, 0),
        };
        images_at.entry(cell).or_default().push(img);
    }

    let col_count = max_col.saturating_add(1).max(1);
    for c in 0..col_count {
        out.push_str("   <table:table-column");
        if let Some(props) = sheet.cols.get(&c) {
            if props.hidden == Some(true) {
                out.push_str(r#" table:visibility="collapse""#);
            }
        }
        out.push_str("/>\n");
    }

    for r in 0..=max_row {
        out.push_str("   <table:table-row");
        if let Some(props) = sheet.rows.get(&r) {
            if props.hidden == Some(true) {
                out.push_str(r#" table:visibility="collapse""#);
            }
        }
        out.push_str(">\n");
        for c in 0..=max_col {
            let cref = CellRef::new(c, r);
            if covered.contains_key(&cref) {
                out.push_str("    <table:covered-table-cell/>\n");
                continue;
            }
            let cell = sheet.cells.get(&cref);
            let (cs, rs) = merge_origin.get(&cref).copied().unwrap_or((1, 1));
            out.push_str("    <table:table-cell");
            if cs > 1 {
                out.push_str(&format!(r#" table:number-columns-spanned="{cs}""#));
            }
            if rs > 1 {
                out.push_str(&format!(r#" table:number-rows-spanned="{rs}""#));
            }
            if let Some(cell) = cell {
                write_cell_attrs(cell, report, &mut out);
            }
            out.push('>');

            // Display text
            if let Some(cell) = cell {
                let display = cell_display(cell);
                if !display.is_empty() {
                    for line in display.split('\n') {
                        out.push_str(&format!("<text:p>{}</text:p>", esc_text(line)));
                    }
                } else if cell.formula.is_some() {
                    out.push_str("<text:p/>");
                }
            }

            if let Some(imgs) = images_at.get(&cref) {
                for img in imgs {
                    write_image(img, assets, media, report, &mut out)?;
                }
            }

            out.push_str("</table:table-cell>\n");
        }
        out.push_str("   </table:table-row>\n");
    }

    // Sheet-level images without a clear cell still go on A1.
    out.push_str("  </table:table>\n");
    Ok(out)
}

fn write_cell_attrs(cell: &Cell, report: &mut ConversionReport, out: &mut String) {
    if let Some(formula) = &cell.formula {
        let body = if formula.text.starts_with('=') {
            formula.text.clone()
        } else {
            format!("={}", formula.text)
        };
        // Always emit OpenFormula with of: prefix for ODS.
        let of = if matches!(formula.dialect, FormulaDialect::OpenFormula) {
            format!("of:{body}")
        } else {
            // Keep foreign dialect text but still prefix; warn.
            report.warn(Warning::Degraded {
                what: "formula dialect".into(),
                why: format!(
                    "{:?} formula written as OpenFormula wrapper",
                    formula.dialect
                ),
            });
            format!("of:{body}")
        };
        out.push_str(&format!(r#" table:formula="{}""#, esc_attr(&of)));
        report.stats.formulas = report.stats.formulas.saturating_add(1);
    }

    match &cell.value {
        CellValue::Empty => {}
        CellValue::Number(n) => {
            let is_pct = cell.num_fmt.as_ref().is_some_and(|f| f.code.contains('%'));
            if is_pct {
                out.push_str(&format!(
                    r#" office:value-type="percentage" office:value="{n}""#
                ));
            } else {
                out.push_str(&format!(r#" office:value-type="float" office:value="{n}""#));
            }
        }
        CellValue::Text(t) => {
            out.push_str(&format!(
                r#" office:value-type="string" office:string-value="{}""#,
                esc_attr(t)
            ));
        }
        CellValue::Bool(b) => {
            out.push_str(&format!(
                r#" office:value-type="boolean" office:boolean-value="{b}""#
            ));
        }
        CellValue::DateTime(d) => {
            if d.contains('T') || d.len() > 10 {
                out.push_str(&format!(
                    r#" office:value-type="date" office:date-value="{}""#,
                    esc_attr(d)
                ));
            } else if d.starts_with('P') || d.starts_with("PT") {
                out.push_str(&format!(
                    r#" office:value-type="time" office:time-value="{}""#,
                    esc_attr(d)
                ));
            } else {
                out.push_str(&format!(
                    r#" office:value-type="date" office:date-value="{}""#,
                    esc_attr(d)
                ));
            }
        }
        CellValue::Error(e) => {
            out.push_str(&format!(
                r#" office:value-type="string" office:string-value="{}""#,
                esc_attr(e)
            ));
        }
    }
}

fn cell_display(cell: &Cell) -> String {
    match &cell.value {
        CellValue::Empty => String::new(),
        CellValue::Number(n) => format!("{n}"),
        CellValue::Text(t) => t.clone(),
        CellValue::Bool(b) => {
            if *b {
                "TRUE".into()
            } else {
                "FALSE".into()
            }
        }
        CellValue::DateTime(d) => d.clone(),
        CellValue::Error(e) => e.clone(),
    }
}

fn write_image(
    img: &ImageRef,
    assets: &dyn AssetStore,
    media: &mut Media,
    report: &mut ConversionReport,
    out: &mut String,
) -> Result<(), WriteError> {
    if let Some(url) = &img.external_src {
        report.warn(Warning::ExternalImageNotFetched { url: url.clone() });
    }
    let href = media.store(assets, img)?;
    let w = format_cm(img.geometry.display_size.width);
    let h = format_cm(img.geometry.display_size.height);
    let (x, y) = match &img.geometry.anchor {
        Anchor::SheetOneCell { from } => (from.offset_x, from.offset_y),
        Anchor::SheetTwoCell { from, .. } => (from.offset_x, from.offset_y),
        Anchor::SheetAbsolute { pos } => (pos.x, pos.y),
        _ => (
            docsai_model::units::Length::ZERO,
            docsai_model::units::Length::ZERO,
        ),
    };
    out.push_str(&format!(
        r#"<draw:frame draw:z-index="0" svg:width="{}" svg:height="{}" svg:x="{}" svg:y="{}"><draw:image xlink:href="{}" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/></draw:frame>"#,
        esc_attr(&w),
        esc_attr(&h),
        esc_attr(&format_cm(x)),
        esc_attr(&format_cm(y)),
        esc_attr(&href),
    ));
    report.stats.images = report.stats.images.saturating_add(1);
    Ok(())
}

fn write_meta(meta: &DocumentMeta) -> String {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-meta xmlns:office="{NS_OFFICE}" xmlns:dc="{NS_DC}" xmlns:meta="{NS_META}" office:version="1.3">
 <office:meta>
"#
    );
    if let Some(v) = &meta.title {
        s.push_str(&format!("  <dc:title>{}</dc:title>\n", esc_text(v)));
    }
    if let Some(v) = &meta.author {
        s.push_str(&format!(
            "  <meta:initial-creator>{}</meta:initial-creator>\n",
            esc_text(v)
        ));
    }
    if let Some(v) = &meta.last_modified_by {
        s.push_str(&format!("  <dc:creator>{}</dc:creator>\n", esc_text(v)));
    }
    if let Some(v) = &meta.language {
        s.push_str(&format!("  <dc:language>{}</dc:language>\n", esc_text(v)));
    }
    if let Some(v) = &meta.created {
        s.push_str(&format!(
            "  <meta:creation-date>{}</meta:creation-date>\n",
            esc_text(v)
        ));
    }
    if let Some(v) = &meta.modified {
        s.push_str(&format!("  <dc:date>{}</dc:date>\n", esc_text(v)));
    }
    if let Some(v) = &meta.application {
        s.push_str(&format!(
            "  <meta:generator>{}</meta:generator>\n",
            esc_text(v)
        ));
    }
    for (k, v) in &meta.custom {
        s.push_str(&format!(
            r#"  <meta:user-defined meta:name="{}">{}</meta:user-defined>
"#,
            esc_attr(k),
            esc_text(v)
        ));
    }
    s.push_str(" </office:meta>\n</office:document-meta>\n");
    s
}

fn write_manifest(entries: &[(String, String)]) -> String {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="{NS_MANIFEST}" manifest:version="1.3">
"#
    );
    for (path, mime) in entries {
        if path == "/" {
            s.push_str(&format!(
                r#" <manifest:file-entry manifest:full-path="/" manifest:version="1.3" manifest:media-type="{}"/>
"#,
                esc_attr(mime)
            ));
        } else {
            s.push_str(&format!(
                r#" <manifest:file-entry manifest:full-path="{}" manifest:media-type="{}"/>
"#,
                esc_attr(path),
                esc_attr(mime)
            ));
        }
    }
    s.push_str("</manifest:manifest>\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::MemoryAssetStore;
    use std::io::Cursor;

    #[test]
    fn writes_and_rereads_values() {
        let mut sheet = Sheet::new("Data");
        sheet.cells.insert(
            CellRef::new(0, 0),
            Cell {
                value: CellValue::Text("A".into()),
                ..Default::default()
            },
        );
        sheet.cells.insert(
            CellRef::new(1, 0),
            Cell {
                value: CellValue::Number(42.0),
                formula: Some(docsai_model::sheet::Formula {
                    text: "21*2".into(),
                    dialect: FormulaDialect::OpenFormula,
                    shared_over: None,
                    array_over: None,
                }),
                ..Default::default()
            },
        );
        let doc = Document::Workbook(Workbook {
            sheets: vec![sheet],
            ..Default::default()
        });
        let assets = MemoryAssetStore::new();
        let mut buf = Cursor::new(Vec::new());
        write_ods(&doc, &assets, &mut buf).unwrap();
        let mut assets2 = MemoryAssetStore::new();
        let (back, _) = crate::ods::read(Cursor::new(buf.into_inner()), &mut assets2).unwrap();
        let Document::Workbook(book) = back else {
            panic!("expected workbook");
        };
        assert_eq!(
            book.sheets[0]
                .cells
                .get(&CellRef::new(0, 0))
                .map(|c| &c.value),
            Some(&CellValue::Text("A".into()))
        );
        let c = book.sheets[0].cells.get(&CellRef::new(1, 0)).unwrap();
        assert_eq!(c.value, CellValue::Number(42.0));
        assert_eq!(c.formula.as_ref().map(|f| f.text.as_str()), Some("21*2"));
    }
}

//! IR → `.xlsx` writer (Phase 3, spike R3).

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Seek, Write};

use docsai_model::assets::AssetStore;
use docsai_model::image::Anchor;
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::sheet::{Cell, CellRef, CellValue, Formula, FormulaDialect, Sheet, Workbook};
use docsai_model::text::DocumentMeta;
use docsai_model::Document;

use crate::package::Package;
use crate::write_error::WriteError;

use super::{iso_to_excel_serial, WORKBOOK_PART};

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const NS_PKG_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const NS_CORE: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
const NS_DC: &str = "http://purl.org/dc/elements/1.1/";
const NS_DCTERMS: &str = "http://purl.org/dc/terms/";
const NS_XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
const NS_EP: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";
const NS_XDR: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const REL_PKG: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// Writes a workbook as a `.xlsx` package.
/// One media part: (package path, content-type, bytes).
type MediaFile = (String, String, Vec<u8>);
/// Drawing XML plus its media files.
type DrawingPart = (String, Vec<MediaFile>);

pub fn write_xlsx<W: Write + Seek>(
    document: &Document,
    assets: &dyn AssetStore,
    writer: W,
) -> Result<ConversionReport, WriteError> {
    let book = match document {
        Document::Workbook(w) => w,
        other => {
            return Err(WriteError::Invalid(format!(
                "cannot write {} as .xlsx",
                other.shape_name()
            )));
        }
    };
    let mut report = ConversionReport::new();
    let package = build_package(book, assets, &mut report)?;
    package.write_to(writer)?;
    Ok(report)
}

fn build_package(
    book: &Workbook,
    assets: &dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Package, WriteError> {
    let mut package = Package::new();
    let mut content_types = ContentTypes::new();
    content_types.default(
        "rels",
        "application/vnd.openxmlformats-package.relationships+xml",
    );
    content_types.default("xml", "application/xml");

    // Shared strings + style tables collected across sheets.
    let mut sst = SharedStrings::default();
    let mut styles = StyleTable::new(&book.styles);

    let mut sheet_parts: Vec<(String, String, bool)> = Vec::new(); // name, xml, has_drawing
    let mut drawing_parts: Vec<(String, DrawingPart)> = Vec::new();
    // drawing part name, xml, media list (filename, content-type, bytes)

    for (index, sheet) in book.sheets.iter().enumerate() {
        let sheet_no = index + 1;
        let (sheet_xml, drawing) = write_sheet(sheet, &mut sst, &mut styles, assets, report)?;
        let part = format!("xl/worksheets/sheet{sheet_no}.xml");
        package.insert(&part, sheet_xml.into_bytes());
        content_types.override_part(
            &format!("/{part}"),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml",
        );

        let has_drawing = drawing.is_some();
        if let Some((draw_xml, media)) = drawing {
            let dpart = format!("xl/drawings/drawing{sheet_no}.xml");
            // sheet rels
            let rels = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{NS_PKG_REL}">
  <Relationship Id="rIdDraw" Type="{REL}/drawing" Target="../drawings/drawing{sheet_no}.xml"/>
</Relationships>"#
            );
            package.insert(
                format!("xl/worksheets/_rels/sheet{sheet_no}.xml.rels"),
                rels.into_bytes(),
            );
            drawing_parts.push((dpart, (draw_xml, media)));
        }
        sheet_parts.push((sheet.name.clone(), part, has_drawing));
        report.stats.sheets = report.stats.sheets.saturating_add(1);
        report.stats.cells = report.stats.cells.saturating_add(sheet.cells.len() as u32);
    }

    // Media + drawing parts
    for (dpart, (draw_xml, media)) in &drawing_parts {
        package.insert(dpart, draw_xml.as_bytes());
        content_types.override_part(
            &format!("/{dpart}"),
            "application/vnd.openxmlformats-officedocument.drawing+xml",
        );
        let mut rel_xml = String::new();
        for (i, (fname, ctype, bytes)) in media.iter().enumerate() {
            let rid = format!("rIdMedia{}", i + 1);
            let media_part = format!("xl/media/{fname}");
            package.insert(&media_part, bytes.clone());
            let ext = fname.rsplit('.').next().unwrap_or("bin");
            content_types.default(ext, ctype);
            rel_xml.push_str(&format!(
                r#"<Relationship Id="{rid}" Type="{REL}/image" Target="../media/{fname}"/>"#
            ));
            report.stats.images = report.stats.images.saturating_add(1);
        }
        let file = dpart.rsplit('/').next().unwrap();
        package.insert(
            format!("xl/drawings/_rels/{file}.rels"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{NS_PKG_REL}">{rel_xml}</Relationships>"#
            )
            .into_bytes(),
        );
    }

    // shared strings
    if !sst.list.is_empty() {
        package.insert("xl/sharedStrings.xml", sst.to_xml().into_bytes());
        content_types.override_part(
            "/xl/sharedStrings.xml",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml",
        );
    }

    package.insert("xl/styles.xml", styles.to_xml().into_bytes());
    content_types.override_part(
        "/xl/styles.xml",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml",
    );

    package.insert(WORKBOOK_PART, workbook_xml(book, &sheet_parts).into_bytes());
    content_types.override_part(
        "/xl/workbook.xml",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
    );

    package.insert(
        "xl/_rels/workbook.xml.rels",
        workbook_rels_xml(&sheet_parts, !sst.list.is_empty()).into_bytes(),
    );

    package.insert("docProps/core.xml", core_xml(&book.meta).into_bytes());
    content_types.override_part(
        "/docProps/core.xml",
        "application/vnd.openxmlformats-package.core-properties+xml",
    );
    package.insert("docProps/app.xml", app_xml(book).into_bytes());
    content_types.override_part(
        "/docProps/app.xml",
        "application/vnd.openxmlformats-officedocument.extended-properties+xml",
    );

    package.insert(
        "_rels/.rels",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{NS_PKG_REL}">
  <Relationship Id="rId1" Type="{REL}/officeDocument" Target="xl/workbook.xml"/>
  <Relationship Id="rId2" Type="{REL_PKG}/metadata/core-properties" Target="docProps/core.xml"/>
  <Relationship Id="rId3" Type="{REL}/extended-properties" Target="docProps/app.xml"/>
</Relationships>"#
        )
        .into_bytes(),
    );

    package.insert("[Content_Types].xml", content_types.to_xml().into_bytes());
    Ok(package)
}

fn write_sheet(
    sheet: &Sheet,
    sst: &mut SharedStrings,
    styles: &mut StyleTable,
    assets: &dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<(String, Option<DrawingPart>), WriteError> {
    let mut out = String::new();
    let mut shared_si: BTreeMap<String, u32> = BTreeMap::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    out.push_str(&format!(
        r#"<worksheet xmlns="{NS_MAIN}" xmlns:r="{NS_R}">"#
    ));
    if let Some(range) = sheet.used_range() {
        out.push_str(&format!(r#"<dimension ref="{}"/>"#, esc_attr(&range.a1())));
    }
    out.push_str(r#"<sheetViews><sheetView workbookViewId="0""#);
    if let Some(pane) = &sheet.pane {
        out.push('>');
        out.push_str(&format!(
            r#"<pane topLeftCell="{}" state="{}" ySplit="{}" xSplit="0" activePane="bottomRight"/><selection pane="bottomRight"/></sheetView>"#,
            esc_attr(&pane.top_left.a1()),
            if pane.frozen { "frozen" } else { "split" },
            pane.top_left.row,
        ));
    } else {
        out.push_str("/>");
    }
    out.push_str("</sheetViews>");

    if !sheet.cols.is_empty() {
        out.push_str("<cols>");
        for (idx, props) in &sheet.cols {
            let min = idx + 1;
            out.push_str(&format!(r#"<col min="{min}" max="{min}""#));
            if let Some(w) = props.width_chars {
                out.push_str(&format!(r#" width="{w}" customWidth="1""#));
            }
            if props.hidden == Some(true) {
                out.push_str(r#" hidden="1""#);
            }
            out.push_str("/>");
        }
        out.push_str("</cols>");
    }

    // Group cells by row.
    let mut by_row: BTreeMap<u32, Vec<(&CellRef, &Cell)>> = BTreeMap::new();
    for (r, c) in &sheet.cells {
        by_row.entry(r.row).or_default().push((r, c));
    }

    out.push_str("<sheetData>");
    for (row_idx, cells) in by_row {
        out.push_str(&format!(r#"<row r="{}""#, row_idx + 1));
        if let Some(props) = sheet.rows.get(&row_idx) {
            if let Some(h) = props.height {
                out.push_str(&format!(r#" ht="{}" customHeight="1""#, h.pt()));
            }
            if props.hidden == Some(true) {
                out.push_str(r#" hidden="1""#);
            }
        }
        out.push('>');
        let mut cells = cells;
        cells.sort_by_key(|(r, _)| r.col);
        for (cref, cell) in cells {
            write_cell(&mut out, *cref, cell, sst, styles, &mut shared_si, report);
        }
        out.push_str("</row>");
    }
    out.push_str("</sheetData>");

    if !sheet.merges.is_empty() {
        out.push_str(&format!(r#"<mergeCells count="{}">"#, sheet.merges.len()));
        for m in &sheet.merges {
            out.push_str(&format!(r#"<mergeCell ref="{}"/>"#, esc_attr(&m.a1())));
        }
        out.push_str("</mergeCells>");
    }

    let drawing = if sheet.images.is_empty() {
        None
    } else {
        let (xml, media) = write_drawing(&sheet.images, assets, report)?;
        out.push_str(r#"<drawing r:id="rIdDraw"/>"#);
        Some((xml, media))
    };

    // Sheet-level raw fragments we cannot place are warned.
    for raw in &sheet.raw {
        report.warn(Warning::RawBlockDropped {
            id: raw.id.as_str().to_string(),
            format: raw.format.clone(),
        });
    }

    out.push_str("</worksheet>");
    Ok((out, drawing))
}

fn write_cell(
    out: &mut String,
    cref: CellRef,
    cell: &Cell,
    sst: &mut SharedStrings,
    styles: &mut StyleTable,
    shared_si: &mut BTreeMap<String, u32>,
    report: &mut ConversionReport,
) {
    let style = styles.style_index(cell);
    out.push_str(&format!(r#"<c r="{}""#, cref.a1()));
    if style > 0 {
        out.push_str(&format!(r#" s="{style}""#));
    }

    // Formula cells keep cached value when present.
    if let Some(formula) = &cell.formula {
        if formula.dialect != FormulaDialect::Ooxml {
            report.warn(Warning::Degraded {
                what: format!("formula at {}", cref.a1()),
                why: format!("dialect {:?} written as OOXML", formula.dialect),
            });
        }
        match &cell.value {
            CellValue::Text(t) => {
                out.push_str(r#" t="str">"#);
                write_formula_el(out, formula, cref, shared_si);
                out.push_str(&format!("<v>{}</v>", esc_text(t)));
            }
            CellValue::Bool(b) => {
                out.push_str(r#" t="b">"#);
                write_formula_el(out, formula, cref, shared_si);
                out.push_str(&format!("<v>{}</v>", if *b { "1" } else { "0" }));
            }
            CellValue::Error(e) => {
                out.push_str(r#" t="e">"#);
                write_formula_el(out, formula, cref, shared_si);
                out.push_str(&format!("<v>{}</v>", esc_text(e)));
            }
            CellValue::DateTime(iso) => {
                out.push('>');
                write_formula_el(out, formula, cref, shared_si);
                let serial = iso_to_excel_serial(iso).unwrap_or(0.0);
                out.push_str(&format!("<v>{serial}</v>"));
            }
            CellValue::Number(n) => {
                out.push('>');
                write_formula_el(out, formula, cref, shared_si);
                out.push_str(&format!("<v>{n}</v>"));
            }
            CellValue::Empty => {
                out.push('>');
                write_formula_el(out, formula, cref, shared_si);
            }
        }
        out.push_str("</c>");
        report.stats.formulas = report.stats.formulas.saturating_add(1);
        return;
    }

    match &cell.value {
        CellValue::Empty => {
            out.push_str("/>");
            return;
        }
        CellValue::Number(n) => {
            out.push('>');
            out.push_str(&format!("<v>{n}</v>"));
        }
        CellValue::Bool(b) => {
            out.push_str(r#" t="b">"#);
            out.push_str(&format!("<v>{}</v>", if *b { "1" } else { "0" }));
        }
        CellValue::Error(e) => {
            out.push_str(r#" t="e">"#);
            out.push_str(&format!("<v>{}</v>", esc_text(e)));
        }
        CellValue::DateTime(iso) => {
            out.push('>');
            let serial = iso_to_excel_serial(iso).unwrap_or(0.0);
            out.push_str(&format!("<v>{serial}</v>"));
        }
        CellValue::Text(t) => {
            let idx = sst.intern(t);
            out.push_str(r#" t="s">"#);
            out.push_str(&format!("<v>{idx}</v>"));
        }
    }
    out.push_str("</c>");
}

fn write_formula_el(
    out: &mut String,
    formula: &Formula,
    cell: CellRef,
    shared_si: &mut BTreeMap<String, u32>,
) {
    if let Some(range) = formula.array_over {
        out.push_str(&format!(
            r#"<f t="array" ref="{}">{}</f>"#,
            esc_attr(&range.a1()),
            esc_text(&formula.text)
        ));
    } else if let Some(range) = formula.shared_over {
        let key = range.a1();
        let next = shared_si.len() as u32;
        let si = *shared_si.entry(key).or_insert(next);
        if cell == range.start {
            out.push_str(&format!(
                r#"<f t="shared" ref="{}" si="{}">{}</f>"#,
                esc_attr(&range.a1()),
                si,
                esc_text(&formula.text)
            ));
        } else {
            out.push_str(&format!(r#"<f t="shared" si="{si}"/>"#));
        }
    } else {
        out.push_str(&format!("<f>{}</f>", esc_text(&formula.text)));
    }
}

fn write_drawing(
    images: &[docsai_model::image::ImageRef],
    assets: &dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<DrawingPart, WriteError> {
    let mut media: Vec<(String, String, Vec<u8>)> = Vec::new();
    let mut asset_rid: BTreeMap<String, usize> = BTreeMap::new();
    let mut xml = String::new();
    xml.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    xml.push_str(&format!(
        r#"<xdr:wsDr xmlns:xdr="{NS_XDR}" xmlns:a="{NS_A}" xmlns:r="{NS_R}">"#
    ));

    for (index, image) in images.iter().enumerate() {
        let rid_index = if let Some(&i) = asset_rid.get(image.asset.as_str()) {
            i
        } else {
            let Some(bytes) = assets.get(&image.asset) else {
                report.warn(Warning::AssetIssue {
                    asset: image.asset.as_str().to_string(),
                    why: "missing from asset store".into(),
                });
                continue;
            };
            let info = assets.info(&image.asset);
            let ext = info
                .map(|i| i.file_name.rsplit('.').next().unwrap_or("bin"))
                .unwrap_or("bin");
            let ctype = info
                .map(|i| i.content_type.clone())
                .unwrap_or_else(|| "application/octet-stream".into());
            let fname = format!("image{}.{}", media.len() + 1, ext);
            media.push((fname, ctype, bytes.to_vec()));
            let i = media.len();
            asset_rid.insert(image.asset.as_str().to_string(), i);
            i
        };
        let rid = format!("rIdMedia{rid_index}");
        let name = image
            .name
            .clone()
            .unwrap_or_else(|| format!("Image{}", index + 1));
        let alt = esc_attr(&image.alt);
        let name_e = esc_attr(&name);
        let cx = image.geometry.display_size.width.emu();
        let cy = image.geometry.display_size.height.emu();
        let pic = format!(
            r#"<xdr:pic><xdr:nvPicPr><xdr:cNvPr id="{id}" name="{name_e}" descr="{alt}"/><xdr:cNvPicPr/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill><xdr:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic>"#,
            id = index + 1,
        );

        match &image.geometry.anchor {
            Anchor::SheetTwoCell {
                from,
                to,
                move_with_cells,
                size_with_cells,
            } => {
                let edit_as = match (*move_with_cells, *size_with_cells) {
                    (true, true) => "twoCell",
                    (true, false) => "oneCell",
                    _ => "absolute",
                };
                xml.push_str(&format!(r#"<xdr:twoCellAnchor editAs="{edit_as}">"#));
                xml.push_str(&cell_anchor_xml("from", from));
                xml.push_str(&cell_anchor_xml("to", to));
                xml.push_str(&pic);
                xml.push_str("<xdr:clientData/></xdr:twoCellAnchor>");
            }
            Anchor::SheetOneCell { from } => {
                xml.push_str("<xdr:oneCellAnchor>");
                xml.push_str(&cell_anchor_xml("from", from));
                xml.push_str(&format!(r#"<xdr:ext cx="{cx}" cy="{cy}"/>"#));
                xml.push_str(&pic);
                xml.push_str("<xdr:clientData/></xdr:oneCellAnchor>");
            }
            Anchor::SheetAbsolute { pos } => {
                xml.push_str("<xdr:absoluteAnchor>");
                xml.push_str(&format!(
                    r#"<xdr:pos x="{}" y="{}"/><xdr:ext cx="{cx}" cy="{cy}"/>"#,
                    pos.x.emu(),
                    pos.y.emu()
                ));
                xml.push_str(&pic);
                xml.push_str("<xdr:clientData/></xdr:absoluteAnchor>");
            }
            other => {
                report.warn(Warning::ImageGeometryDegraded {
                    what: name,
                    why: format!(
                        "non-sheet anchor {:?} degraded to one-cell A1",
                        other.keyword()
                    ),
                });
                xml.push_str("<xdr:oneCellAnchor>");
                xml.push_str(
                    r#"<xdr:from><xdr:col>0</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>0</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>"#,
                );
                xml.push_str(&format!(r#"<xdr:ext cx="{cx}" cy="{cy}"/>"#));
                xml.push_str(&pic);
                xml.push_str("<xdr:clientData/></xdr:oneCellAnchor>");
            }
        }
    }
    xml.push_str("</xdr:wsDr>");
    Ok((xml, media))
}

fn cell_anchor_xml(tag: &str, a: &docsai_model::image::CellAnchor) -> String {
    format!(
        r#"<xdr:{tag}><xdr:col>{}</xdr:col><xdr:colOff>{}</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>{}</xdr:rowOff></xdr:{tag}>"#,
        a.cell.col,
        a.offset_x.emu(),
        a.cell.row,
        a.offset_y.emu()
    )
}

fn workbook_xml(book: &Workbook, sheets: &[(String, String, bool)]) -> String {
    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    out.push_str(&format!(r#"<workbook xmlns="{NS_MAIN}" xmlns:r="{NS_R}">"#));
    if let Some(active) = &book.active_sheet {
        if let Some(idx) = sheets.iter().position(|(n, _, _)| n == active) {
            out.push_str(&format!(
                r#"<bookViews><workbookView activeTab="{idx}"/></bookViews>"#
            ));
        }
    }
    out.push_str("<sheets>");
    for (i, (name, _, _)) in sheets.iter().enumerate() {
        let id = i + 1;
        out.push_str(&format!(
            r#"<sheet name="{}" sheetId="{id}" r:id="rIdSheet{id}"/>"#,
            esc_attr(name)
        ));
    }
    out.push_str("</sheets>");
    if !book.defined_names.is_empty() {
        out.push_str("<definedNames>");
        for n in &book.defined_names {
            out.push_str(&format!(r#"<definedName name="{}""#, esc_attr(&n.name)));
            if let Some(sheet) = n.sheet {
                out.push_str(&format!(r#" localSheetId="{sheet}""#));
            }
            out.push('>');
            out.push_str(&esc_text(&n.refers_to));
            out.push_str("</definedName>");
        }
        out.push_str("</definedNames>");
    }
    out.push_str("</workbook>");
    out
}

fn workbook_rels_xml(sheets: &[(String, String, bool)], has_sst: bool) -> String {
    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    out.push_str(&format!(r#"<Relationships xmlns="{NS_PKG_REL}">"#));
    for (i, (_, part, _)) in sheets.iter().enumerate() {
        let id = i + 1;
        let target = part.strip_prefix("xl/").unwrap_or(part);
        out.push_str(&format!(
            r#"<Relationship Id="rIdSheet{id}" Type="{REL}/worksheet" Target="{target}"/>"#
        ));
    }
    out.push_str(&format!(
        r#"<Relationship Id="rIdStyles" Type="{REL}/styles" Target="styles.xml"/>"#
    ));
    if has_sst {
        out.push_str(&format!(
            r#"<Relationship Id="rIdSst" Type="{REL}/sharedStrings" Target="sharedStrings.xml"/>"#
        ));
    }
    out.push_str("</Relationships>");
    out
}

fn core_xml(meta: &DocumentMeta) -> String {
    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    out.push_str(&format!(
        r#"<cp:coreProperties xmlns:cp="{NS_CORE}" xmlns:dc="{NS_DC}" xmlns:dcterms="{NS_DCTERMS}" xmlns:xsi="{NS_XSI}">"#
    ));
    if let Some(t) = &meta.title {
        out.push_str(&format!("<dc:title>{}</dc:title>", esc_text(t)));
    }
    if let Some(a) = &meta.author {
        out.push_str(&format!("<dc:creator>{}</dc:creator>", esc_text(a)));
    }
    if let Some(v) = &meta.last_modified_by {
        out.push_str(&format!(
            "<cp:lastModifiedBy>{}</cp:lastModifiedBy>",
            esc_text(v)
        ));
    }
    if let Some(v) = &meta.created {
        out.push_str(&format!(
            r#"<dcterms:created xsi:type="dcterms:W3CDTF">{}</dcterms:created>"#,
            esc_text(v)
        ));
    }
    if let Some(v) = &meta.modified {
        out.push_str(&format!(
            r#"<dcterms:modified xsi:type="dcterms:W3CDTF">{}</dcterms:modified>"#,
            esc_text(v)
        ));
    }
    if let Some(v) = &meta.language {
        out.push_str(&format!("<dc:language>{}</dc:language>", esc_text(v)));
    }
    out.push_str("</cp:coreProperties>");
    out
}

fn app_xml(book: &Workbook) -> String {
    let sheets = book.sheets.len();
    let app = book
        .meta
        .application
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("docsai");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="{NS_EP}">
  <Application>{}</Application>
  <HeadingPairs/><TitlesOfParts/>
  <Documents>{sheets}</Documents>
</Properties>"#,
        esc_text(app)
    )
}

// --- helpers ----------------------------------------------------------------

#[derive(Default)]
struct SharedStrings {
    list: Vec<String>,
    index: BTreeMap<String, usize>,
}

impl SharedStrings {
    fn intern(&mut self, s: &str) -> usize {
        if let Some(&i) = self.index.get(s) {
            return i;
        }
        let i = self.list.len();
        self.list.push(s.to_string());
        self.index.insert(s.to_string(), i);
        i
    }

    fn to_xml(&self) -> String {
        let mut out = String::new();
        out.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
        out.push_str(&format!(
            r#"<sst xmlns="{NS_MAIN}" count="{c}" uniqueCount="{c}">"#,
            c = self.list.len()
        ));
        for s in &self.list {
            out.push_str(&format!("<si><t>{}</t></si>", esc_text(s)));
        }
        out.push_str("</sst>");
        out
    }
}

struct StyleTable {
    /// format code → numFmtId
    num_fmts: BTreeMap<String, u32>,
    next_custom: u32,
    /// style key → xf index
    xf_index: BTreeMap<String, u32>,
    /// rendered cellXfs (index 0 is General)
    xfs: Vec<String>,
    /// fonts[0] is default Calibri 11
    fonts: Vec<String>,
    /// StyleId string → font index
    font_for_style: BTreeMap<String, u32>,
}

impl StyleTable {
    fn new(catalog: &docsai_model::style::StyleCatalog) -> Self {
        let mut table = StyleTable {
            num_fmts: BTreeMap::new(),
            next_custom: 164,
            xf_index: BTreeMap::new(),
            xfs: vec![r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>"#.into()],
            fonts: vec![r#"<font><sz val="11"/><name val="Calibri"/></font>"#.into()],
            font_for_style: BTreeMap::new(),
        };
        for style in catalog.styles.values() {
            let font_xml = font_to_xml(&style.font);
            let idx = table.fonts.len() as u32;
            table.fonts.push(font_xml);
            table
                .font_for_style
                .insert(style.id.as_str().to_string(), idx);
        }
        table
    }

    fn style_index(&mut self, cell: &Cell) -> u32 {
        let code = cell.num_fmt.as_ref().map(|f| f.code.as_str()).unwrap_or("");
        let style_key = cell
            .style
            .as_ref()
            .map(|s| s.as_str().to_string())
            .unwrap_or_default();
        if code.is_empty() && style_key.is_empty() {
            return 0;
        }
        let key = format!("{code}|{style_key}");
        if let Some(&idx) = self.xf_index.get(&key) {
            return idx;
        }
        let num_id = if code.is_empty() {
            0
        } else if let Some(id) = cell.num_fmt.as_ref().and_then(|f| f.id) {
            self.num_fmts.entry(code.to_string()).or_insert(id);
            *self.num_fmts.get(code).unwrap()
        } else if let Some(&id) = self.num_fmts.get(code) {
            id
        } else {
            // Built-in-ish: try common date format id 14 for short dates
            let id = builtin_num_fmt_id(code).unwrap_or_else(|| {
                let id = self.next_custom;
                self.next_custom += 1;
                id
            });
            self.num_fmts.entry(code.to_string()).or_insert(id);
            id
        };
        let font_id = cell
            .style
            .as_ref()
            .and_then(|s| self.font_for_style.get(s.as_str()).copied())
            .unwrap_or(0);
        let mut attrs =
            format!(r#"numFmtId="{num_id}" fontId="{font_id}" fillId="0" borderId="0" xfId="0""#);
        if !code.is_empty() {
            attrs.push_str(r#" applyNumberFormat="1""#);
        }
        if font_id > 0 {
            attrs.push_str(r#" applyFont="1""#);
        }
        let idx = self.xfs.len() as u32;
        self.xfs.push(format!("<xf {attrs}/>"));
        self.xf_index.insert(key, idx);
        idx
    }

    fn to_xml(&self) -> String {
        let mut out = String::new();
        out.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
        out.push_str(&format!(r#"<styleSheet xmlns="{NS_MAIN}">"#));
        if !self.num_fmts.is_empty() {
            let customs: Vec<_> = self.num_fmts.iter().filter(|(_, id)| **id >= 164).collect();
            if !customs.is_empty() {
                out.push_str(&format!(r#"<numFmts count="{}">"#, customs.len()));
                let mut customs = customs;
                customs.sort_by_key(|(_, id)| *id);
                for (code, id) in customs {
                    out.push_str(&format!(
                        r#"<numFmt numFmtId="{id}" formatCode="{}"/>"#,
                        esc_attr(code)
                    ));
                }
                out.push_str("</numFmts>");
            }
        }
        out.push_str(&format!(r#"<fonts count="{}">"#, self.fonts.len()));
        for font in &self.fonts {
            out.push_str(font);
        }
        out.push_str("</fonts>");
        out.push_str(
            r#"<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>"#,
        );
        out.push_str(
            r#"<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>"#,
        );
        out.push_str(
            r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#,
        );
        out.push_str(&format!(r#"<cellXfs count="{}">"#, self.xfs.len()));
        for xf in &self.xfs {
            out.push_str(xf);
        }
        out.push_str("</cellXfs>");
        out.push_str(
            r#"<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>"#,
        );
        out.push_str("</styleSheet>");
        out
    }
}

fn builtin_num_fmt_id(code: &str) -> Option<u32> {
    match code {
        "General" => Some(0),
        "0" => Some(1),
        "0.00" => Some(2),
        "#,##0" => Some(3),
        "#,##0.00" => Some(4),
        "0%" => Some(9),
        "0.00%" => Some(10),
        "mm-dd-yy" | "dd/mm/yyyy" | "yyyy-mm-dd" => None, // custom keep
        _ => None,
    }
}

fn font_to_xml(font: &docsai_model::style::FontProps) -> String {
    let mut out = String::from("<font>");
    if let Some(sz) = font.size {
        out.push_str(&format!(r#"<sz val="{}"/>"#, sz.pt()));
    } else {
        out.push_str(r#"<sz val="11"/>"#);
    }
    if let Some(name) = &font.name {
        out.push_str(&format!(r#"<name val="{}"/>"#, esc_attr(name)));
    } else {
        out.push_str(r#"<name val="Calibri"/>"#);
    }
    if let Some(color) = &font.color {
        let rgb = color.trim_start_matches('#');
        out.push_str(&format!(r#"<color rgb="FF{}"/>"#, esc_attr(rgb)));
    }
    if font.bold == Some(true) {
        out.push_str("<b/>");
    }
    if font.italic == Some(true) {
        out.push_str("<i/>");
    }
    if font.strike == Some(true) {
        out.push_str("<strike/>");
    }
    if let Some(u) = font.underline {
        if u != docsai_model::style::Underline::None {
            out.push_str("<u/>");
        }
    }
    out.push_str("</font>");
    out
}

struct ContentTypes {
    defaults: BTreeMap<String, String>,
    overrides: Vec<(String, String)>,
    seen_defaults: BTreeSet<String>,
}

impl ContentTypes {
    fn new() -> Self {
        Self {
            defaults: BTreeMap::new(),
            overrides: Vec::new(),
            seen_defaults: BTreeSet::new(),
        }
    }
    fn default(&mut self, ext: &str, ctype: &str) {
        if self.seen_defaults.insert(ext.to_string()) {
            self.defaults.insert(ext.to_string(), ctype.to_string());
        }
    }
    fn override_part(&mut self, part: &str, ctype: &str) {
        self.overrides.push((part.to_string(), ctype.to_string()));
    }
    fn to_xml(&self) -> String {
        let mut out = String::new();
        out.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
        out.push_str(&format!(r#"<Types xmlns="{NS_CT}">"#));
        for (ext, ctype) in &self.defaults {
            out.push_str(&format!(
                r#"<Default Extension="{ext}" ContentType="{ctype}"/>"#
            ));
        }
        for (part, ctype) in &self.overrides {
            out.push_str(&format!(
                r#"<Override PartName="{part}" ContentType="{ctype}"/>"#
            ));
        }
        out.push_str("</Types>");
        out
    }
}

fn esc_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn esc_attr(s: &str) -> String {
    esc_text(s).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::MemoryAssetStore;
    use std::io::Cursor;

    #[test]
    fn round_trips_values_through_xlsx_bytes() {
        let path = format!(
            "{}/../../corpus/xlsx/values-types.xlsx",
            env!("CARGO_MANIFEST_DIR")
        );
        let file = std::fs::File::open(&path).unwrap();
        let mut assets = MemoryAssetStore::new();
        let (doc, _) = crate::xlsx::read(file, &mut assets).unwrap();
        let mut buf = Cursor::new(Vec::new());
        write_xlsx(&doc, &assets, &mut buf).unwrap();
        buf.set_position(0);
        let mut assets2 = MemoryAssetStore::new();
        let (doc2, _) = crate::xlsx::read(buf, &mut assets2).unwrap();
        let b1 = match doc {
            Document::Workbook(w) => w,
            _ => panic!(),
        };
        let b2 = match doc2 {
            Document::Workbook(w) => w,
            _ => panic!(),
        };
        assert_eq!(b1.sheets[0].cells.len(), b2.sheets[0].cells.len());
        assert_eq!(
            b1.sheets[0]
                .cells
                .get(&CellRef::new(1, 1))
                .map(|c| &c.value),
            b2.sheets[0]
                .cells
                .get(&CellRef::new(1, 1))
                .map(|c| &c.value)
        );
    }
}

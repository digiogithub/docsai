//! Pictures and tables on a slide: `p:pic` and the `p:graphicFrame` that holds
//! an `a:tbl` (increment 13-E).
//!
//! Neither gets a model of its own. A slide picture is the same normalised
//! [`ImageRef`] a `.docx` paragraph carries, resolved through the same
//! [`AssetStore`] and the same `a:blip` code, so one bitmap used on four slides
//! is stored once. A slide table is the IR [`Table`], the one the writers and
//! DocMark already know how to render — a table is a table wherever it is.
//!
//! What a `p:graphicFrame` holds that is *not* a table — a chart, SmartArt, an
//! OLE object — is reported by the uri that names it and left to the increment
//! that models it. A frame skipped without a word is a slide that quietly loses
//! its chart.

use docsai_model::assets::AssetStore;
use docsai_model::image::{ImageGeometry, ImageRef};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::style::StyleId;
use docsai_model::text::{Table, TableCell, TableRow};
use docsai_model::units::{Length, Size};

use crate::drawingml;
use crate::error::ReadError;
use crate::package::{Package, Relationships};
use crate::xml::Element;

use super::cascade::{LevelStyles, Theme};
use super::text;

/// The `a:graphicData@uri` of a DrawingML table.
pub(super) const TABLE_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/table";

/// Reads a `p:pic` into the normalised image model.
///
/// The picture's *placement* is not stored here: it is the shape's own
/// geometry, read from `p:spPr/a:xfrm` like every other shape's, and writing
/// the position twice is writing it twice to disagree with itself later. What
/// the [`ImageGeometry`] carries is what belongs to the bitmap — the size it is
/// drawn at, the crop, and the pixels it actually has.
pub(super) fn read_picture(
    pic: &Element,
    package: &Package,
    rels: &Relationships,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Option<ImageRef>, ReadError> {
    let blip = pic.path(&["blipFill", "blip"]);
    let Some(mut image) = drawingml::resolve_blip(blip, package, rels, assets, report)? else {
        return Ok(None);
    };

    let ext = pic.path(&["spPr", "xfrm", "ext"]);
    let mut geometry = ImageGeometry::inline(Size::new(
        Length::from_emu(ext.and_then(|e| e.attr_i64("cx")).unwrap_or(0)),
        Length::from_emu(ext.and_then(|e| e.attr_i64("cy")).unwrap_or(0)),
    ));
    drawingml::read_crop(pic.path(&["blipFill", "srcRect"]), &mut geometry);
    if let Some(info) = assets.info(&image.asset) {
        geometry.native_size_px = info.native_size_px;
    }
    image.geometry = geometry;

    let nv = pic.path(&["nvPicPr", "cNvPr"]);
    image.alt = nv
        .and_then(|nv| nv.attr("descr"))
        .unwrap_or_default()
        .to_string();
    image.name = nv
        .and_then(|nv| nv.attr("name"))
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    image.link = nv
        .and_then(|nv| nv.child("hlinkClick"))
        .and_then(|link| link.attr_qualified("r:id"))
        .and_then(|id| rels.get(id))
        .map(|rel| rel.target.clone());

    // A picture can fill a placeholder slot. The image model has nowhere to put
    // that identity, so the slot is named rather than dropped in silence.
    if let Some(ph) = pic.path(&["nvPicPr", "nvPr", "ph"]) {
        report.warn(Warning::Degraded {
            what: image.name.clone().unwrap_or_else(|| "picture".into()),
            why: format!(
                "it fills the `{}` placeholder slot, which a picture cannot carry",
                ph.attr("type").unwrap_or("body")
            ),
        });
    }

    // Effects have no DocMark model, exactly as in a `.docx` (architecture §3.1).
    if pic
        .child("spPr")
        .is_some_and(|pr| pr.child("effectLst").is_some() || pr.child("scene3d").is_some())
    {
        report.warn(Warning::ImageGeometryDegraded {
            what: image.name.clone().unwrap_or_else(|| "picture".into()),
            why: "DrawingML effects (shadow/bevel/3-D) have no DocMark model".into(),
        });
    }

    Ok(Some(image))
}

/// Reads an `a:tbl` into the IR table.
///
/// DrawingML states its merges the other way round from WordprocessingML: the
/// origin cell carries `gridSpan`/`rowSpan` and every cell it swallows is
/// written out with `hMerge`/`vMerge`. That is nearly the IR's own shape, and
/// the difference matters in one place. A cell swallowed *horizontally* is not
/// in the IR grid at all — a `colspan` of 2 already occupies both columns, and
/// keeping the `hMerge` cell as well would make the row one column wider than
/// the table has. A cell swallowed *vertically* stays, marked `covered`, which
/// is exactly what a `.docx` `vMerge` continuation becomes.
pub(super) fn read_table(
    tbl: &Element,
    theme: &Theme,
    rels: &Relationships,
    report: &mut ConversionReport,
) -> Table {
    let properties = tbl.child("tblPr");
    let mut table = Table {
        // The GUID of a table style in `ppt/tableStyles.xml`: a reference, kept
        // as one. Resolving it into per-cell formatting would flatten a deck's
        // styling into its content.
        style: properties
            .and_then(|pr| pr.child("tableStyleId"))
            .map(|id| StyleId::new(id.text().trim().to_string()))
            .filter(|id| !id.as_str().is_empty()),
        col_widths: tbl
            .child("tblGrid")
            .map(|grid| {
                grid.children_named("gridCol")
                    .map(|col| Length::from_emu(col.attr_i64("w").unwrap_or(0)))
                    .collect()
            })
            .unwrap_or_default(),
        ..Default::default()
    };

    // `firstRow` is a property of the table, not of the row: it says the first
    // row is a header, which is what the IR records on both.
    let header_row = properties.is_some_and(|pr| pr.attr("firstRow").is_some_and(is_true));

    for (index, tr) in tbl.children_named("tr").enumerate() {
        let mut row = TableRow {
            is_header: header_row && index == 0,
            ..Default::default()
        };
        for tc in tr.children_named("tc") {
            if tc.attr("hMerge").is_some_and(is_true) {
                continue;
            }
            let covered = tc.attr("vMerge").is_some_and(is_true);
            let cell_properties = tc.child("tcPr");
            row.cells.push(TableCell {
                // A covered cell holds no content of its own; PowerPoint keeps
                // an empty `a:txBody` there, and reading it would duplicate the
                // spanning cell's text into the grid.
                blocks: match (covered, tc.child("txBody")) {
                    (false, Some(body)) => text::read_body(
                        body,
                        &text::TextCtx {
                            rels,
                            // A table cell has no placeholder to bullet it and
                            // no cascade above it: the table style is a
                            // reference, and it is kept as one.
                            bulleted: false,
                            theme,
                            inherited: &LevelStyles::default(),
                        },
                        report,
                    ),
                    _ => Vec::new(),
                },
                colspan: span(tc.attr_i64("gridSpan")),
                rowspan: span(tc.attr_i64("rowSpan")),
                covered,
                width: None,
                background: cell_properties
                    .and_then(|pr| pr.child("solidFill"))
                    .and_then(|fill| text::solid_colour(fill, theme, report)),
            });
        }
        table.rows.push(row);
    }

    table.header_row = header_row;
    table
}

/// A span attribute. Absent means one; a nonsensical one is clamped rather than
/// trusted, because it indexes a grid.
fn span(value: Option<i64>) -> u16 {
    value.unwrap_or(1).clamp(1, 1024) as u16
}

fn is_true(value: &str) -> bool {
    matches!(value, "1" | "true" | "on")
}

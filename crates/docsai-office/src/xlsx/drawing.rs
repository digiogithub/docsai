//! SpreadsheetDrawingML (`xl/drawings/drawing*.xml`) → sheet image anchors.

use docsai_model::assets::AssetStore;
use docsai_model::image::{Anchor, CellAnchor, ImageGeometry, ImageRef};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::sheet::CellRef;
use docsai_model::units::{Length, Point, Size};

use crate::error::ReadError;
use crate::package::Package;
use crate::xml::Element;

/// Reads one drawing part into sheet-anchored images.
pub fn read_drawing_part(
    package: &Package,
    part: &str,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Vec<ImageRef>, ReadError> {
    let bytes = package
        .part(part)
        .ok_or_else(|| ReadError::MissingPart(part.to_string()))?;
    let root = Element::parse(part, bytes)?;
    let rels = package.relationships(part);
    let mut images = Vec::new();

    for child in root.children() {
        let anchor_kind = child.name.as_str();
        if !matches!(
            anchor_kind,
            "twoCellAnchor" | "oneCellAnchor" | "absoluteAnchor"
        ) {
            if matches!(anchor_kind, "graphicFrame" | "cxnSp" | "sp") {
                report.warn(Warning::UnsupportedElement {
                    kind: anchor_kind.into(),
                    location: part.into(),
                    action: "skipped".into(),
                });
            }
            continue;
        }

        let Some(pic) = find_pic(child) else {
            report.warn(Warning::UnsupportedElement {
                kind: anchor_kind.into(),
                location: part.into(),
                action: "skipped".into(),
            });
            continue;
        };

        let display_size = extent_size(child, pic);
        let mut geometry = ImageGeometry::inline(display_size);
        geometry.anchor = match anchor_kind {
            "twoCellAnchor" => {
                let from = read_cell_anchor(child.child("from"));
                let to = read_cell_anchor(child.child("to"));
                let edit_as = child.attr("editAs").unwrap_or("twoCell");
                let (move_with_cells, size_with_cells) = match edit_as {
                    "absolute" => (false, false),
                    "oneCell" => (true, false),
                    _ => (true, true), // twoCell
                };
                Anchor::SheetTwoCell {
                    from,
                    to,
                    move_with_cells,
                    size_with_cells,
                }
            }
            "oneCellAnchor" => Anchor::SheetOneCell {
                from: read_cell_anchor(child.child("from")),
            },
            "absoluteAnchor" => {
                let pos = child
                    .child("pos")
                    .map(|p| {
                        Point::new(
                            Length::from_emu(p.attr_i64("x").unwrap_or(0)),
                            Length::from_emu(p.attr_i64("y").unwrap_or(0)),
                        )
                    })
                    .unwrap_or_default();
                Anchor::SheetAbsolute { pos }
            }
            _ => unreachable!(),
        };

        read_transform(pic.child("spPr"), &mut geometry);

        let cnv = pic
            .path(&["nvPicPr", "cNvPr"])
            .or_else(|| child.path(&["pic", "nvPicPr", "cNvPr"]));
        let alt = cnv
            .and_then(|e| e.attr("descr"))
            .unwrap_or_default()
            .to_string();
        let name = cnv
            .and_then(|e| e.attr("name"))
            .filter(|n| !n.is_empty())
            .map(str::to_string);

        let blip = pic.path(&["blipFill", "blip"]);
        let embed = blip.and_then(|b| b.attr_qualified("r:embed").or_else(|| b.attr("embed")));
        let link = blip.and_then(|b| b.attr_qualified("r:link").or_else(|| b.attr("link")));

        let mut image = if let Some(rid) = embed {
            let Some(rel) = rels.get(rid) else {
                report.warn(Warning::AssetIssue {
                    asset: rid.into(),
                    why: format!("missing relationship in {part}"),
                });
                continue;
            };
            let Some(bytes) = package.part(&rel.target) else {
                report.warn(Warning::AssetIssue {
                    asset: rel.target.clone(),
                    why: "part missing".into(),
                });
                continue;
            };
            let id = match assets.put(bytes) {
                Ok(id) => id,
                Err(err) => {
                    report.warn(Warning::AssetIssue {
                        asset: rel.target.clone(),
                        why: err.to_string(),
                    });
                    continue;
                }
            };
            ImageRef::new(id, geometry)
        } else if let Some(rid) = link {
            let url = rels
                .get(rid)
                .map(|r| r.target.clone())
                .unwrap_or_else(|| rid.to_string());
            report.warn(Warning::ExternalImageNotFetched { url: url.clone() });
            // Placeholder empty asset is not ideal; skip with warning.
            continue;
        } else {
            continue;
        };

        image.alt = alt;
        image.name = name;
        images.push(image);
    }

    Ok(images)
}

fn find_pic(anchor: &Element) -> Option<&Element> {
    if let Some(pic) = anchor.child("pic") {
        return Some(pic);
    }
    // Sometimes nested under graphicFrame etc.
    for child in anchor.children() {
        if child.name == "pic" {
            return Some(child);
        }
        if let Some(pic) = child.child("pic") {
            return Some(pic);
        }
    }
    None
}

fn extent_size(anchor: &Element, pic: &Element) -> Size {
    if let Some(ext) = anchor.child("ext") {
        return Size::new(
            Length::from_emu(ext.attr_i64("cx").unwrap_or(0)),
            Length::from_emu(ext.attr_i64("cy").unwrap_or(0)),
        );
    }
    if let Some(ext) = pic.path(&["spPr", "xfrm", "ext"]) {
        return Size::new(
            Length::from_emu(ext.attr_i64("cx").unwrap_or(0)),
            Length::from_emu(ext.attr_i64("cy").unwrap_or(0)),
        );
    }
    Size::default()
}

fn read_cell_anchor(el: Option<&Element>) -> CellAnchor {
    let Some(el) = el else {
        return CellAnchor::new(CellRef::new(0, 0), Length::ZERO, Length::ZERO);
    };
    let col = el
        .child("col")
        .map(|c| c.text().parse().unwrap_or(0))
        .unwrap_or(0);
    let row = el
        .child("row")
        .map(|c| c.text().parse().unwrap_or(0))
        .unwrap_or(0);
    let col_off = el
        .child("colOff")
        .map(|c| Length::from_emu(c.text().parse().unwrap_or(0)))
        .unwrap_or(Length::ZERO);
    let row_off = el
        .child("rowOff")
        .map(|c| Length::from_emu(c.text().parse().unwrap_or(0)))
        .unwrap_or(Length::ZERO);
    CellAnchor::new(CellRef::new(col as u32, row as u32), col_off, row_off)
}

fn read_transform(sp_pr: Option<&Element>, geometry: &mut ImageGeometry) {
    let Some(xfrm) = sp_pr.and_then(|e| e.child("xfrm")) else {
        return;
    };
    if let Some(rot) = xfrm.attr_i64("rot") {
        geometry.rotation_deg = rot as f32 / 60_000.0;
    }
    let flip_h = xfrm.attr("flipH").map(|v| v == "1").unwrap_or(false);
    let flip_v = xfrm.attr("flipV").map(|v| v == "1").unwrap_or(false);
    geometry.flip = docsai_model::image::Flip::from_flags(flip_h, flip_v);
}

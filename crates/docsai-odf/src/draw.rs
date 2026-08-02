//! ODF `draw:frame` / `draw:image` → normalised [`ImageRef`].

use docsai_model::assets::AssetStore;
use docsai_model::image::{
    AlignKeyword, Anchor, AxisPos, CropRect, Flip, HVPos, ImageGeometry, ImageRef, RelBase,
    WrapMode, WrapSide,
};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::sheet::CellRef;
use docsai_model::units::{Length, Point, Size};

use crate::length::parse_length;
use crate::package::Package;
use crate::styles::{GraphicStyle, OdfStyles};
use crate::xml::Element;

/// Reads a `draw:frame` that contains a `draw:image`.
pub fn read_frame(
    frame: &Element,
    package: &Package,
    base_part: &str,
    styles: &OdfStyles,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Option<ImageRef> {
    let image = frame.child("image")?;
    let href = image
        .attr_qualified("xlink:href")
        .or_else(|| image.attr("href"))?;

    let width = frame
        .attr("width")
        .or_else(|| frame.attr_qualified("svg:width"))
        .and_then(parse_length)
        .unwrap_or(Length::ZERO);
    let height = frame
        .attr("height")
        .or_else(|| frame.attr_qualified("svg:height"))
        .and_then(parse_length)
        .unwrap_or(Length::ZERO);
    let display_size = Size::new(width, height);

    let graphic = styles.graphic(
        frame
            .attr("style-name")
            .or_else(|| frame.attr_qualified("draw:style-name")),
    );
    let mut geometry = ImageGeometry::inline(display_size);
    apply_graphic_style(&mut geometry, graphic, frame, report, base_part);

    // Spreadsheet cell anchors when present.
    if let Some(end_cell) = frame
        .attr("end-cell-address")
        .or_else(|| frame.attr_qualified("table:end-cell-address"))
    {
        // ODF anchors drawings with `table:end-cell-address` + end-x/end-y and
        // the frame sits inside a cell (one-cell) or spans (two-cell via end).
        if let Some(anchor) = sheet_anchor_from_frame(frame, end_cell) {
            geometry.anchor = anchor;
        }
    }

    let alt = frame
        .attr("name")
        .or_else(|| frame.attr_qualified("draw:name"))
        .unwrap_or("")
        .to_string();
    // Prefer svg:title / svg:desc children when present.
    let title = frame
        .child("title")
        .map(|e| e.deep_text())
        .filter(|t| !t.is_empty());
    let desc = frame
        .child("desc")
        .map(|e| e.deep_text())
        .filter(|t| !t.is_empty());

    // Linked (external) images are never fetched.
    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("file:") {
        report.warn(Warning::ExternalImageNotFetched {
            url: href.to_string(),
        });
        // Minimal PNG signature bytes so the store can mint an id; content is
        // not a real image and is only a stand-in for the external reference.
        let asset = assets
            .put(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
            .ok()?;
        let mut image_ref = ImageRef::new(asset, geometry);
        image_ref.external_src = Some(href.to_string());
        image_ref.alt = desc.unwrap_or(alt);
        image_ref.title = title;
        return Some(image_ref);
    }

    let part_name = package.resolve_href(base_part, href)?;
    let bytes = package.part(&part_name)?;
    let asset = match assets.put(bytes) {
        Ok(id) => id,
        Err(e) => {
            report.warn(Warning::Degraded {
                what: format!("image `{href}`"),
                why: e.to_string(),
            });
            return None;
        }
    };

    report.stats.images += 1;
    let mut image_ref = ImageRef::new(asset, geometry);
    image_ref.alt = desc.unwrap_or(alt.clone());
    image_ref.title = title;
    image_ref.name = if alt.is_empty() { None } else { Some(alt) };
    // Hyperlink on the frame via draw:a parent is handled by the caller.
    Some(image_ref)
}

fn apply_graphic_style(
    geometry: &mut ImageGeometry,
    graphic: Option<&GraphicStyle>,
    frame: &Element,
    report: &mut ConversionReport,
    part: &str,
) {
    let x = frame
        .attr("x")
        .or_else(|| frame.attr_qualified("svg:x"))
        .and_then(parse_length);
    let y = frame
        .attr("y")
        .or_else(|| frame.attr_qualified("svg:y"))
        .and_then(parse_length);

    let anchor_type = frame
        .attr("anchor-type")
        .or_else(|| frame.attr_qualified("text:anchor-type"))
        .unwrap_or("paragraph");

    match anchor_type {
        "as-char" => {
            geometry.anchor = Anchor::Inline;
        }
        "char" | "paragraph" | "page" | "frame" => {
            let (rel_h, rel_v) = match anchor_type {
                "page" => (RelBase::Page, RelBase::Page),
                "char" => (RelBase::Character, RelBase::Line),
                _ => (RelBase::Paragraph, RelBase::Paragraph),
            };
            let (rel_h, rel_v) = if let Some(g) = graphic {
                (
                    map_rel(g.horizontal_rel.as_deref()).unwrap_or(rel_h),
                    map_rel(g.vertical_rel.as_deref()).unwrap_or(rel_v),
                )
            } else {
                (rel_h, rel_v)
            };

            let h = if let Some(g) = graphic {
                map_align_or_offset(g.horizontal_pos.as_deref(), x, true)
            } else {
                AxisPos::Offset(x.unwrap_or(Length::ZERO))
            };
            let v = if let Some(g) = graphic {
                map_align_or_offset(g.vertical_pos.as_deref(), y, false)
            } else {
                AxisPos::Offset(y.unwrap_or(Length::ZERO))
            };

            let (wrap, wrap_side) = map_wrap(graphic);
            let behind = graphic
                .and_then(|g| g.run_through.as_deref())
                .is_some_and(|r| r == "background");

            geometry.anchor = Anchor::Floating {
                relative_to_h: rel_h,
                relative_to_v: rel_v,
                position: HVPos { h, v },
                wrap,
                wrap_side,
                behind_text: behind,
            };
        }
        other => {
            report.warn(Warning::ImageGeometryDegraded {
                what: format!("anchor-type `{other}`"),
                why: format!("unknown ODF anchor at {part}; treated as inline"),
            });
            geometry.anchor = Anchor::Inline;
        }
    }

    if let Some(g) = graphic {
        if let Some(angle) = g.rotation_angle {
            geometry.rotation_deg = angle;
        }
        if let Some(mirror) = g.mirror.as_deref() {
            let h = mirror.contains("horizontal");
            let v = mirror.contains("vertical");
            geometry.flip = Flip::from_flags(h, v);
        }
        if let Some(clip) = g.clip.as_deref() {
            if let Some(crop) = parse_fo_clip(clip, geometry.display_size) {
                geometry.crop = Some(crop);
            }
        }
    }

    if let Some(z) = frame
        .attr("z-index")
        .or_else(|| frame.attr_qualified("draw:z-index"))
        .and_then(|s| s.parse().ok())
    {
        geometry.z_index = Some(z);
    }
}

fn map_rel(value: Option<&str>) -> Option<RelBase> {
    match value? {
        "page" => Some(RelBase::Page),
        "page-content" | "paragraph-content" | "frame-content" => Some(RelBase::Margin),
        "paragraph" => Some(RelBase::Paragraph),
        "char" => Some(RelBase::Character),
        "line" => Some(RelBase::Line),
        "frame" => Some(RelBase::Column),
        _ => None,
    }
}

fn map_align_or_offset(pos: Option<&str>, offset: Option<Length>, horizontal: bool) -> AxisPos {
    match pos {
        Some("left") if horizontal => AxisPos::Align(AlignKeyword::Left),
        Some("right") if horizontal => AxisPos::Align(AlignKeyword::Right),
        Some("center") | Some("middle") if horizontal => AxisPos::Align(AlignKeyword::Center),
        Some("top") if !horizontal => AxisPos::Align(AlignKeyword::Top),
        Some("bottom") if !horizontal => AxisPos::Align(AlignKeyword::Bottom),
        Some("middle") | Some("center") if !horizontal => AxisPos::Align(AlignKeyword::Middle),
        Some("from-left") | Some("from-inside") | Some("from-outside") | Some("below") | None => {
            AxisPos::Offset(offset.unwrap_or(Length::ZERO))
        }
        _ => AxisPos::Offset(offset.unwrap_or(Length::ZERO)),
    }
}

fn map_wrap(graphic: Option<&GraphicStyle>) -> (WrapMode, WrapSide) {
    let wrap = graphic.and_then(|g| g.wrap.as_deref()).unwrap_or("none");
    let mode = match wrap {
        "left" | "right" | "parallel" | "dynamic" => WrapMode::Square,
        "biggest" => WrapMode::Square,
        "run-through" => WrapMode::Through,
        "none" => WrapMode::TopBottom,
        _ => WrapMode::Square,
    };
    let side = match wrap {
        "left" => WrapSide::Left,
        "right" => WrapSide::Right,
        "biggest" => WrapSide::Largest,
        _ => WrapSide::Both,
    };
    (mode, side)
}

/// `fo:clip` form: `rect(top, right, bottom, left)` in absolute lengths.
fn parse_fo_clip(clip: &str, display: Size) -> Option<CropRect> {
    let inner = clip.trim().strip_prefix("rect(")?.strip_suffix(')')?.trim();
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() != 4 {
        return None;
    }
    let top = parse_length(parts[0])?;
    let right = parse_length(parts[1])?;
    let bottom = parse_length(parts[2])?;
    let left = parse_length(parts[3])?;
    let w = display.width.emu().max(1) as f32;
    let h = display.height.emu().max(1) as f32;
    Some(CropRect {
        top: (top.emu() as f32 / h) * 100.0,
        right: (right.emu() as f32 / w) * 100.0,
        bottom: (bottom.emu() as f32 / h) * 100.0,
        left: (left.emu() as f32 / w) * 100.0,
    })
}

fn sheet_anchor_from_frame(frame: &Element, end_cell: &str) -> Option<Anchor> {
    // end-cell-address looks like "Sheet1.C5" or just "C5".
    let end = parse_sheet_cell(Some(end_cell))?;
    let end_x = frame
        .attr("end-x")
        .or_else(|| frame.attr_qualified("table:end-x"))
        .and_then(parse_length)
        .unwrap_or(Length::ZERO);
    let end_y = frame
        .attr("end-y")
        .or_else(|| frame.attr_qualified("table:end-y"))
        .and_then(parse_length)
        .unwrap_or(Length::ZERO);

    // When the frame lives in a cell, ODF does not always repeat the start
    // cell on the frame; the ODS reader supplies it. For a standalone frame
    // with only an end cell we treat it as one-cell at that address.
    let _ = (end_x, end_y);
    Some(Anchor::SheetOneCell {
        from: docsai_model::image::CellAnchor::new(end, Length::ZERO, Length::ZERO),
    })
}

fn parse_sheet_cell(address: Option<&str>) -> Option<CellRef> {
    let address = address?;
    let cell = address.rsplit('.').next()?;
    CellRef::parse_a1(cell)
}

/// Builds a sheet one-cell anchor for a frame nested under a known cell.
pub fn sheet_one_cell(cell: CellRef, col_off: Length, row_off: Length) -> Anchor {
    Anchor::SheetOneCell {
        from: docsai_model::image::CellAnchor::new(cell, col_off, row_off),
    }
}

/// Absolute sheet position from svg:x / svg:y.
#[allow(dead_code)]
pub fn sheet_absolute(x: Length, y: Length) -> Anchor {
    Anchor::SheetAbsolute {
        pos: Point { x, y },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fo_clip_into_percentages() {
        let size = Size::new(Length::from_cm(10.0), Length::from_cm(5.0));
        let crop = parse_fo_clip("rect(0.5cm, 1cm, 0.5cm, 1cm)", size).unwrap();
        assert!((crop.top - 10.0).abs() < 0.1);
        assert!((crop.left - 10.0).abs() < 0.1);
        assert!((crop.right - 10.0).abs() < 0.1);
        assert!((crop.bottom - 10.0).abs() < 0.1);
    }
}

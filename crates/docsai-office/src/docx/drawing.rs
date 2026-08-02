//! DrawingML (`w:drawing`) and legacy VML (`w:pict`) → the normalised image
//! model of architecture §3.1.
//!
//! This is where the spike (docs/spikes/R1) concluded the existing crates fall
//! short, so it is deliberately exhaustive: wrap mode and side, `behindDoc`,
//! crop, rotation, flips, alt text, title, object name, the hyperlink on the
//! image and linked-but-not-embedded sources are all mapped, and anything left
//! over becomes a typed warning rather than a silent loss.

use docsai_model::assets::AssetStore;
use docsai_model::image::{
    AlignKeyword, Anchor, AxisPos, CropRect, Flip, HVPos, ImageGeometry, ImageRef, RelBase,
    SimpleBorder, WrapMode, WrapSide,
};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::units::{Length, Size};

use super::format::hex_color;
use crate::error::ReadError;
use crate::package::{Package, Relationships};
use crate::xml::Element;

/// Sixty-thousandths of a degree per degree, the DrawingML rotation unit.
const ROT_UNITS_PER_DEGREE: f32 = 60_000.0;
/// Thousandths of a percent, the `a:srcRect` unit.
const CROP_UNITS_PER_PERCENT: f32 = 1_000.0;

/// Context a drawing needs to resolve its media and report problems.
pub struct DrawingContext<'a> {
    pub package: &'a Package,
    pub rels: &'a Relationships,
    /// Part the drawing lives in, for warning locations.
    pub part: &'a str,
}

/// Reads a `w:drawing` element.
///
/// Returns `Ok(None)` when the drawing holds something that is not a picture
/// (a chart, SmartArt, a text box); the caller preserves those as raw-blocks.
pub fn read_drawing(
    drawing: &Element,
    ctx: &DrawingContext<'_>,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Option<ImageRef>, ReadError> {
    let (frame, anchored) = match (drawing.child("inline"), drawing.child("anchor")) {
        (Some(inline), _) => (inline, false),
        (None, Some(anchor)) => (anchor, true),
        (None, None) => return Ok(None),
    };

    let Some(graphic_data) = frame.path(&["graphic", "graphicData"]) else {
        return Ok(None);
    };
    let Some(pic) = graphic_data.child("pic") else {
        // Charts, diagrams and shapes live here too and have no image model.
        report.warn(Warning::UnsupportedElement {
            kind: graphic_data
                .attr("uri")
                .unwrap_or("a:graphicData")
                .rsplit('/')
                .next()
                .unwrap_or("graphicData")
                .to_string(),
            location: ctx.part.to_string(),
            action: "raw-block".into(),
        });
        return Ok(None);
    };

    let display_size = frame
        .child("extent")
        .map(|e| {
            Size::new(
                Length::from_emu(e.attr_i64("cx").unwrap_or(0)),
                Length::from_emu(e.attr_i64("cy").unwrap_or(0)),
            )
        })
        .unwrap_or_default();

    let mut geometry = ImageGeometry::inline(display_size);
    read_shape_properties(pic.child("spPr"), &mut geometry, report, ctx.part);
    read_crop(pic.path(&["blipFill", "srcRect"]), &mut geometry);
    if anchored {
        geometry.anchor = read_anchor(frame);
        // `relativeHeight` is the z-order; it lives on the geometry, not on
        // the anchor, because sheet-anchored images stack the same way.
        geometry.z_index = frame.attr_i64("relativeHeight").map(|v| v as i32);
    }

    let doc_pr = frame.child("docPr");
    let alt = doc_pr
        .and_then(|e| e.attr("descr"))
        .or_else(|| {
            pic.path(&["nvPicPr", "cNvPr"])
                .and_then(|e| e.attr("descr"))
        })
        .unwrap_or_default()
        .to_string();

    let blip = pic.path(&["blipFill", "blip"]);
    let mut image = match resolve_media(blip, ctx, assets, report)? {
        Some(mut image) => {
            image.geometry = geometry;
            image.alt = alt;
            image
        }
        None => return Ok(None),
    };

    image.title = doc_pr.and_then(|e| e.attr("title")).map(str::to_string);
    image.name = doc_pr
        .and_then(|e| e.attr("name"))
        .or_else(|| pic.path(&["nvPicPr", "cNvPr"]).and_then(|e| e.attr("name")))
        .filter(|n| !n.is_empty())
        .map(str::to_string);
    image.link = doc_pr
        .and_then(|e| e.child("hlinkClick"))
        .and_then(|e| e.attr_qualified("r:id"))
        .and_then(|id| ctx.rels.get(id))
        .map(|rel| rel.target.clone());

    // Native dimensions come from the stored asset's own header.
    if let Some(info) = assets.info(&image.asset) {
        image.geometry.native_size_px = info.native_size_px;
    }

    // DrawingML effects have no DocMark representation; flag them so the
    // caller can attach a raw-block instead of losing them silently.
    if pic
        .child("spPr")
        .is_some_and(|spr| spr.child("effectLst").is_some() || spr.child("scene3d").is_some())
    {
        report.warn(Warning::ImageGeometryDegraded {
            what: image.name.clone().unwrap_or_else(|| "picture".into()),
            why: "DrawingML effects (shadow/bevel/3-D) have no DocMark model".into(),
        });
    }

    Ok(Some(image))
}

fn read_shape_properties(
    sppr: Option<&Element>,
    geometry: &mut ImageGeometry,
    report: &mut ConversionReport,
    part: &str,
) {
    let Some(sppr) = sppr else { return };

    if let Some(xfrm) = sppr.child("xfrm") {
        if let Some(rot) = xfrm.attr_i64("rot") {
            geometry.rotation_deg = rot as f32 / ROT_UNITS_PER_DEGREE;
        }
        geometry.flip = Flip::from_flags(
            xfrm.attr("flipH").is_some_and(is_true),
            xfrm.attr("flipV").is_some_and(is_true),
        );
    }

    if let Some(ln) = sppr.child("ln") {
        let color = ln
            .path(&["solidFill", "srgbClr"])
            .and_then(|c| c.attr("val"))
            .and_then(hex_color)
            .unwrap_or_else(|| "#000000".into());
        geometry.border = Some(SimpleBorder {
            width: Length::from_emu(ln.attr_i64("w").unwrap_or(9525)),
            style: ln
                .child("prstDash")
                .and_then(|d| d.attr("val"))
                .map(dash_style)
                .unwrap_or("solid")
                .to_string(),
            color,
        });
        if ln.child("gradFill").is_some() || ln.child("pattFill").is_some() {
            report.warn(Warning::ImageGeometryDegraded {
                what: format!("image border in {part}"),
                why: "only single-colour borders are modelled; the fill was simplified".into(),
            });
        }
    }

    // A non-rectangular preset geometry changes the visible shape.
    if let Some(prst) = sppr.child("prstGeom").and_then(|g| g.attr("prst")) {
        if prst != "rect" {
            report.warn(Warning::ImageGeometryDegraded {
                what: format!("image in {part}"),
                why: format!("preset shape `{prst}` is rendered as a rectangle"),
            });
        }
    }
}

fn read_crop(src_rect: Option<&Element>, geometry: &mut ImageGeometry) {
    let Some(rect) = src_rect else { return };
    let side = |name: &str| rect.attr_i64(name).unwrap_or(0) as f32 / CROP_UNITS_PER_PERCENT;
    let crop = CropRect {
        left: side("l"),
        top: side("t"),
        right: side("r"),
        bottom: side("b"),
    };
    if !crop.is_empty() {
        geometry.crop = Some(crop);
    }
}

fn read_anchor(anchor: &Element) -> Anchor {
    let axis = |name: &str| -> (RelBase, AxisPos) {
        let Some(position) = anchor.child(name) else {
            return (RelBase::Paragraph, AxisPos::Offset(Length::ZERO));
        };
        let base = rel_base(position.attr("relativeFrom").unwrap_or("column"));
        let value = match position.child("align").map(|a| a.text()) {
            Some(keyword) => AxisPos::Align(align_keyword(keyword.trim())),
            None => AxisPos::Offset(Length::from_emu(
                position
                    .child("posOffset")
                    .and_then(|o| o.text().trim().parse::<i64>().ok())
                    .unwrap_or(0),
            )),
        };
        (base, value)
    };

    let (relative_to_h, h) = axis("positionH");
    let (relative_to_v, v) = axis("positionV");
    let (wrap, wrap_side) = read_wrap(anchor);

    Anchor::Floating {
        relative_to_h,
        relative_to_v,
        position: HVPos { h, v },
        wrap,
        wrap_side,
        behind_text: anchor.attr("behindDoc").is_some_and(is_true),
    }
}

fn read_wrap(anchor: &Element) -> (WrapMode, WrapSide) {
    for child in anchor.children() {
        let mode = match child.name.as_str() {
            "wrapSquare" => WrapMode::Square,
            "wrapTight" => WrapMode::Tight,
            "wrapThrough" => WrapMode::Through,
            "wrapTopAndBottom" => WrapMode::TopBottom,
            "wrapNone" => WrapMode::None,
            _ => continue,
        };
        let side = match child.attr("wrapText") {
            Some("left") => WrapSide::Left,
            Some("right") => WrapSide::Right,
            Some("largest") => WrapSide::Largest,
            _ => WrapSide::Both,
        };
        return (mode, side);
    }
    (WrapMode::None, WrapSide::Both)
}

/// Resolves `a:blip` to a stored asset, honouring embedded and linked images.
fn resolve_media(
    blip: Option<&Element>,
    ctx: &DrawingContext<'_>,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Option<ImageRef>, ReadError> {
    let Some(blip) = blip else { return Ok(None) };

    if let Some(embed) = blip.attr_qualified("r:embed") {
        let Some(rel) = ctx.rels.get(embed) else {
            report.warn(Warning::AssetIssue {
                asset: embed.to_string(),
                why: "relationship not found".into(),
            });
            return Ok(None);
        };
        let Some(bytes) = ctx.package.part(&rel.target) else {
            report.warn(Warning::AssetIssue {
                asset: rel.target.clone(),
                why: "media part missing from the package".into(),
            });
            return Ok(None);
        };
        let id = assets.put(bytes)?;
        report.stats.images += 1;
        let mut image = ImageRef::new(id, ImageGeometry::inline(Size::default()));
        warn_if_unrenderable(&image, assets, report);
        image.alt.clear();
        return Ok(Some(image));
    }

    if let Some(link) = blip.attr_qualified("r:link") {
        // Linked images are never fetched: doing so would turn a document into
        // an outbound network request (architecture §3.2).
        let url = ctx
            .rels
            .get(link)
            .map(|r| r.target.clone())
            .unwrap_or_else(|| link.to_string());
        report.warn(Warning::ExternalImageNotFetched { url: url.clone() });
        let mut image = ImageRef::new(
            docsai_model::AssetId::new(String::new()),
            ImageGeometry::inline(Size::default()),
        );
        image.external_src = Some(url);
        return Ok(Some(image));
    }

    Ok(None)
}

fn warn_if_unrenderable(image: &ImageRef, assets: &dyn AssetStore, report: &mut ConversionReport) {
    let Some(info) = assets.info(&image.asset) else {
        return;
    };
    if matches!(info.content_type.as_str(), "image/x-emf" | "image/x-wmf") {
        report.warn(Warning::Degraded {
            what: info.file_name.clone(),
            why: "WMF/EMF is preserved byte for byte but Markdown viewers cannot render it".into(),
        });
    } else if info.content_type == "application/octet-stream" {
        report.warn(Warning::AssetIssue {
            asset: info.file_name.clone(),
            why: "unrecognised media type; stored verbatim".into(),
        });
    }
}

/// Reads a legacy VML picture (`w:pict`), typical of documents that once were
/// `.doc`. Only what the normalised model covers is mapped; the rest is
/// reported as degraded (analysis §1.4).
pub fn read_vml_picture(
    pict: &Element,
    ctx: &DrawingContext<'_>,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Option<ImageRef>, ReadError> {
    let Some(shape) = pict.child("shape").or_else(|| pict.child("rect")) else {
        return Ok(None);
    };
    let Some(data) = shape.child("imagedata") else {
        return Ok(None);
    };
    let Some(rel_id) = data
        .attr_qualified("r:id")
        .or_else(|| data.attr_qualified("o:relid"))
    else {
        return Ok(None);
    };
    let Some(rel) = ctx.rels.get(rel_id) else {
        report.warn(Warning::AssetIssue {
            asset: rel_id.to_string(),
            why: "VML relationship not found".into(),
        });
        return Ok(None);
    };
    let Some(bytes) = ctx.package.part(&rel.target) else {
        report.warn(Warning::AssetIssue {
            asset: rel.target.clone(),
            why: "media part missing from the package".into(),
        });
        return Ok(None);
    };

    let style = VmlStyle::parse(shape.attr("style").unwrap_or_default());
    let mut geometry = ImageGeometry::inline(Size::new(
        style.length("width").unwrap_or_default(),
        style.length("height").unwrap_or_default(),
    ));
    geometry.rotation_deg = style.number("rotation").unwrap_or(0.0);
    geometry.z_index = style.number("z-index").map(|z| z as i32);

    let floating = style.get("position").is_some_and(|p| p == "absolute");
    if floating {
        let (wrap, wrap_side) = match shape.child("wrap") {
            Some(w) => (
                match w.attr("type") {
                    Some("square") => WrapMode::Square,
                    Some("tight") => WrapMode::Tight,
                    Some("through") => WrapMode::Through,
                    Some("topAndBottom") => WrapMode::TopBottom,
                    _ => WrapMode::None,
                },
                match w.attr("side") {
                    Some("left") => WrapSide::Left,
                    Some("right") => WrapSide::Right,
                    Some("largest") => WrapSide::Largest,
                    _ => WrapSide::Both,
                },
            ),
            None => (WrapMode::None, WrapSide::Both),
        };
        geometry.anchor = Anchor::Floating {
            relative_to_h: rel_base(
                style
                    .get("mso-position-horizontal-relative")
                    .unwrap_or("column"),
            ),
            relative_to_v: rel_base(
                style
                    .get("mso-position-vertical-relative")
                    .unwrap_or("paragraph"),
            ),
            position: HVPos {
                h: AxisPos::Offset(style.length("margin-left").unwrap_or_default()),
                v: AxisPos::Offset(style.length("margin-top").unwrap_or_default()),
            },
            wrap,
            wrap_side,
            behind_text: geometry.z_index.is_some_and(|z| z < 0),
        };
    }

    let id = assets.put(bytes)?;
    report.stats.images += 1;
    let mut image = ImageRef::new(id, geometry);
    image.alt = shape.attr("alt").unwrap_or_default().to_string();
    image.title = data
        .attr_qualified("o:title")
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    image.name = shape.attr("id").map(str::to_string);
    if let Some(info) = assets.info(&image.asset) {
        image.geometry.native_size_px = info.native_size_px;
    }
    warn_if_unrenderable(&image, assets, report);

    report.warn(Warning::ImageGeometryDegraded {
        what: image.name.clone().unwrap_or_else(|| "VML shape".into()),
        why: "read from legacy VML; only the normalised attributes are mapped".into(),
    });
    Ok(Some(image))
}

/// The `style` attribute of a VML shape, which is a CSS-like `k:v;` list.
struct VmlStyle {
    entries: Vec<(String, String)>,
}

impl VmlStyle {
    fn parse(text: &str) -> VmlStyle {
        let entries = text
            .split(';')
            .filter_map(|pair| pair.split_once(':'))
            .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            .collect();
        VmlStyle { entries }
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn number(&self, key: &str) -> Option<f32> {
        self.get(key)?
            .trim_end_matches(|c: char| c.is_alphabetic())
            .parse()
            .ok()
    }

    /// A CSS length. VML writes points by default; `in`, `cm`, `mm` and `px`
    /// also appear in documents converted from `.doc`.
    fn length(&self, key: &str) -> Option<Length> {
        let raw = self.get(key)?.trim();
        let split = raw.find(|c: char| c.is_alphabetic()).unwrap_or(raw.len());
        let (number, unit) = raw.split_at(split);
        let value: f64 = number.trim().parse().ok()?;
        Some(match unit.trim() {
            "in" => Length::from_inch(value),
            "cm" => Length::from_cm(value),
            "mm" => Length::from_mm(value),
            "px" => Length::from_px(value),
            "pc" => Length::from_pt(value * 12.0),
            _ => Length::from_pt(value),
        })
    }
}

fn rel_base(value: &str) -> RelBase {
    match value {
        "page" => RelBase::Page,
        "margin" | "leftMargin" | "rightMargin" | "topMargin" | "bottomMargin" | "insideMargin"
        | "outsideMargin" => RelBase::Margin,
        "paragraph" => RelBase::Paragraph,
        "character" => RelBase::Character,
        "line" => RelBase::Line,
        _ => RelBase::Column,
    }
}

fn align_keyword(value: &str) -> AlignKeyword {
    match value {
        "center" => AlignKeyword::Center,
        "right" => AlignKeyword::Right,
        "inside" => AlignKeyword::Inside,
        "outside" => AlignKeyword::Outside,
        "top" => AlignKeyword::Top,
        "bottom" => AlignKeyword::Bottom,
        "middle" => AlignKeyword::Middle,
        _ => AlignKeyword::Left,
    }
}

fn dash_style(value: &str) -> &'static str {
    match value {
        "solid" => "solid",
        "dot" | "sysDot" => "dotted",
        _ => "dashed",
    }
}

fn is_true(value: &str) -> bool {
    matches!(value, "1" | "true" | "on")
}

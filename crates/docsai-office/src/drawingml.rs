//! The parts of DrawingML that are the same wherever they appear.
//!
//! `a:blip` and `a:srcRect` are written identically in a `w:drawing`, in a
//! `xdr:pic` and in a `p:pic`: the picture is a relationship id, the crop is
//! four thousandths-of-a-percent insets. They live here rather than in one
//! reader so that a slide and a paragraph resolve their media through the same
//! code — the same asset hash, the same warnings, the same refusal to fetch a
//! linked image.

use docsai_model::assets::AssetStore;
use docsai_model::image::{CropRect, ImageGeometry, ImageRef};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::units::Size;

use crate::error::ReadError;
use crate::package::{Package, Relationships};
use crate::xml::Element;

/// Thousandths of a percent, the `a:srcRect` unit.
const CROP_UNITS_PER_PERCENT: f32 = 1_000.0;

/// Resolves `a:blip` to a stored asset, honouring embedded and linked images.
///
/// The returned [`ImageRef`] carries only the asset: geometry, alt text and
/// name belong to the element around the blip, which differs per format.
pub(crate) fn resolve_blip(
    blip: Option<&Element>,
    package: &Package,
    rels: &Relationships,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Option<ImageRef>, ReadError> {
    let Some(blip) = blip else { return Ok(None) };

    if let Some(embed) = blip.attr_qualified("r:embed") {
        let Some(rel) = rels.get(embed) else {
            report.warn(Warning::AssetIssue {
                asset: embed.to_string(),
                why: "relationship not found".into(),
            });
            return Ok(None);
        };
        let Some(bytes) = package.part(&rel.target) else {
            report.warn(Warning::AssetIssue {
                asset: rel.target.clone(),
                why: "media part missing from the package".into(),
            });
            return Ok(None);
        };
        let id = assets.put(bytes)?;
        report.stats.images += 1;
        let image = ImageRef::new(id, ImageGeometry::inline(Size::default()));
        warn_if_unrenderable(&image, assets, report);
        return Ok(Some(image));
    }

    if let Some(link) = blip.attr_qualified("r:link") {
        // Linked images are never fetched: doing so would turn a document into
        // an outbound network request (architecture §3.2).
        let url = rels
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

/// A picture that is stored faithfully but that no Markdown viewer will draw is
/// not a loss of data, and is a loss of meaning. Both are said out loud.
pub(crate) fn warn_if_unrenderable(
    image: &ImageRef,
    assets: &dyn AssetStore,
    report: &mut ConversionReport,
) {
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

/// `a:srcRect`: the crop, as insets from each side.
pub(crate) fn read_crop(src_rect: Option<&Element>, geometry: &mut ImageGeometry) {
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

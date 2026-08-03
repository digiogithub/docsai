//! Normalised image model (architecture §3.1).
//!
//! Every source format has its own graphics model — DrawingML, VML, Escher,
//! SpreadsheetDrawingML, ODF `draw:frame`. Readers translate *into* this model
//! rather than around it: whatever cannot be mapped degrades with a typed
//! warning, and effects with no representation move to a raw-block through
//! [`ImageRef::effects_raw`].

use serde::{Deserialize, Serialize};

use crate::addressing::NodeId;
use crate::assets::AssetId;
use crate::sheet::CellRef;
use crate::units::{Length, Point, Size};

/// Identifier of a raw fragment kept aside for round-trip fidelity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RawId(pub String);

impl RawId {
    pub fn new(id: impl Into<String>) -> Self {
        RawId(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A picture placed in a document, with everything needed to put it back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ImageRef {
    /// Stable address of the picture (spec §11.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<NodeId>,
    /// Key in the [`AssetStore`](crate::assets::AssetStore): the content hash,
    /// so N appearances of one bitmap share a single stored file.
    pub asset: AssetId,
    pub geometry: ImageGeometry,
    /// Alternative text. Travels in the Markdown `![…]` slot, never as an
    /// attribute, so plain viewers show it (spec §3.5).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Internal object name (`wp:docPr @name`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Hyperlink applied to the image itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// Set when the image was *linked* rather than embedded. The referenced
    /// content is never fetched (architecture §3.2, security).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_src: Option<String>,
    /// Effects with no DocMark representation, kept as an associated raw-block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effects_raw: Option<RawId>,
}

impl ImageRef {
    pub fn new(asset: AssetId, geometry: ImageGeometry) -> Self {
        ImageRef {
            id: None,
            asset,
            geometry,
            alt: String::new(),
            title: None,
            name: None,
            link: None,
            external_src: None,
            effects_raw: None,
        }
    }
}

/// Placement and transform of an image.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ImageGeometry {
    /// Size as displayed, which need not match the bitmap's native size.
    pub display_size: Size,
    /// Native pixel dimensions of the bitmap, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_size_px: Option<(u32, u32)>,
    /// Declared resolution, when it differs from 96 dpi.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpi: Option<u32>,
    pub anchor: Anchor,
    /// Clockwise rotation in degrees.
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub rotation_deg: f32,
    #[serde(default, skip_serializing_if = "Flip::is_none")]
    pub flip: Flip,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<CropRect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<SimpleBorder>,
    /// Stacking order among floating objects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z_index: Option<i32>,
}

fn is_zero_f32(v: &f32) -> bool {
    *v == 0.0
}

impl ImageGeometry {
    /// An inline image of the given displayed size.
    pub fn inline(display_size: Size) -> Self {
        ImageGeometry {
            display_size,
            native_size_px: None,
            dpi: None,
            anchor: Anchor::Inline,
            rotation_deg: 0.0,
            flip: Flip::None,
            crop: None,
            border: None,
            z_index: None,
        }
    }

    /// True when the anchor only makes sense inside a spreadsheet.
    pub fn is_sheet_anchored(&self) -> bool {
        self.anchor.is_sheet()
    }
}

/// How an image is attached to its surroundings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum Anchor {
    /// Flows with the text.
    Inline,
    /// Positioned relative to a page element, with text wrapping around it.
    Floating {
        relative_to_h: RelBase,
        relative_to_v: RelBase,
        position: HVPos,
        wrap: WrapMode,
        wrap_side: WrapSide,
        behind_text: bool,
    },
    /// Spreadsheet only: stretches and moves with the grid.
    SheetTwoCell {
        from: CellAnchor,
        to: CellAnchor,
        move_with_cells: bool,
        size_with_cells: bool,
    },
    /// Spreadsheet only: anchored to one cell, fixed size.
    SheetOneCell { from: CellAnchor },
    /// Spreadsheet only: absolute position on the sheet.
    SheetAbsolute { pos: Point },
}

impl Anchor {
    /// True for the three spreadsheet-only anchors.
    pub fn is_sheet(&self) -> bool {
        matches!(
            self,
            Anchor::SheetTwoCell { .. }
                | Anchor::SheetOneCell { .. }
                | Anchor::SheetAbsolute { .. }
        )
    }

    /// The DocMark `anchor=` value for this anchor (spec §3.5 / §4.1).
    pub fn keyword(&self) -> &'static str {
        match self {
            Anchor::Inline => "inline",
            Anchor::Floating { behind_text, .. } => {
                if *behind_text {
                    "behind"
                } else {
                    "floating"
                }
            }
            Anchor::SheetTwoCell { .. } => "two-cell",
            Anchor::SheetOneCell { .. } => "one-cell",
            Anchor::SheetAbsolute { .. } => "absolute",
        }
    }
}

/// What a floating position is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelBase {
    Page,
    Margin,
    Paragraph,
    Character,
    Line,
    Column,
}

impl RelBase {
    pub fn as_str(self) -> &'static str {
        match self {
            RelBase::Page => "page",
            RelBase::Margin => "margin",
            RelBase::Paragraph => "paragraph",
            RelBase::Character => "character",
            RelBase::Line => "line",
            RelBase::Column => "column",
        }
    }
}

/// A floating position: either an offset or a symbolic alignment, per axis.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct HVPos {
    pub h: AxisPos,
    pub v: AxisPos,
}

/// Position along one axis.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AxisPos {
    /// Offset from the relative base.
    Offset(Length),
    /// Symbolic alignment (`left`, `center`, `top`…) as the source expressed it.
    Align(AlignKeyword),
}

/// Symbolic alignment keywords shared by both axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AlignKeyword {
    Left,
    Center,
    Right,
    Inside,
    Outside,
    Top,
    Middle,
    Bottom,
}

impl AlignKeyword {
    pub fn as_str(self) -> &'static str {
        match self {
            AlignKeyword::Left => "left",
            AlignKeyword::Center => "center",
            AlignKeyword::Right => "right",
            AlignKeyword::Inside => "inside",
            AlignKeyword::Outside => "outside",
            AlignKeyword::Top => "top",
            AlignKeyword::Middle => "middle",
            AlignKeyword::Bottom => "bottom",
        }
    }
}

/// How text flows around a floating image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WrapMode {
    Square,
    Tight,
    Through,
    TopBottom,
    None,
}

impl WrapMode {
    pub fn as_str(self) -> &'static str {
        match self {
            WrapMode::Square => "square",
            WrapMode::Tight => "tight",
            WrapMode::Through => "through",
            WrapMode::TopBottom => "top-bottom",
            WrapMode::None => "none",
        }
    }
}

/// Which sides of a floating image text may flow on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WrapSide {
    Both,
    Left,
    Right,
    Largest,
}

impl WrapSide {
    pub fn as_str(self) -> &'static str {
        match self {
            WrapSide::Both => "both",
            WrapSide::Left => "left",
            WrapSide::Right => "right",
            WrapSide::Largest => "largest",
        }
    }
}

/// Mirroring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Flip {
    #[default]
    None,
    H,
    V,
    HV,
}

impl Flip {
    pub fn is_none(&self) -> bool {
        *self == Flip::None
    }

    pub fn from_flags(h: bool, v: bool) -> Flip {
        match (h, v) {
            (false, false) => Flip::None,
            (true, false) => Flip::H,
            (false, true) => Flip::V,
            (true, true) => Flip::HV,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Flip::None => "none",
            Flip::H => "h",
            Flip::V => "v",
            Flip::HV => "hv",
        }
    }
}

/// Crop, as a percentage of the original taken off each side.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CropRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl CropRect {
    /// True when nothing is actually cropped.
    pub fn is_empty(&self) -> bool {
        self.left == 0.0 && self.top == 0.0 && self.right == 0.0 && self.bottom == 0.0
    }
}

/// A single-line border around an image. Anything richer goes to a raw-block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SimpleBorder {
    pub width: Length,
    /// `solid`, `dashed`, `dotted`… as named by the source.
    pub style: String,
    /// `#RRGGBB`.
    pub color: String,
}

/// A cell-relative anchor point: a cell plus an offset inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CellAnchor {
    pub cell: CellRef,
    pub offset_x: Length,
    pub offset_y: Length,
}

impl CellAnchor {
    pub fn new(cell: CellRef, offset_x: Length, offset_y: Length) -> Self {
        CellAnchor {
            cell,
            offset_x,
            offset_y,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_keywords_match_the_spec() {
        assert_eq!(Anchor::Inline.keyword(), "inline");
        let floating = Anchor::Floating {
            relative_to_h: RelBase::Margin,
            relative_to_v: RelBase::Paragraph,
            position: HVPos {
                h: AxisPos::Offset(Length::from_cm(1.2)),
                v: AxisPos::Align(AlignKeyword::Top),
            },
            wrap: WrapMode::Square,
            wrap_side: WrapSide::Right,
            behind_text: false,
        };
        assert_eq!(floating.keyword(), "floating");
        assert!(!floating.is_sheet());
    }

    #[test]
    fn behind_text_changes_the_keyword() {
        let behind = Anchor::Floating {
            relative_to_h: RelBase::Page,
            relative_to_v: RelBase::Page,
            position: HVPos {
                h: AxisPos::Offset(Length::ZERO),
                v: AxisPos::Offset(Length::ZERO),
            },
            wrap: WrapMode::None,
            wrap_side: WrapSide::Both,
            behind_text: true,
        };
        assert_eq!(behind.keyword(), "behind");
    }

    #[test]
    fn sheet_anchors_are_flagged() {
        let a = Anchor::SheetOneCell {
            from: CellAnchor::new(CellRef::new(1, 1), Length::ZERO, Length::ZERO),
        };
        assert!(a.is_sheet());
        assert_eq!(a.keyword(), "one-cell");
    }

    #[test]
    fn flip_flags_combine() {
        assert_eq!(Flip::from_flags(true, true), Flip::HV);
        assert_eq!(Flip::from_flags(false, false), Flip::None);
        assert!(Flip::None.is_none());
    }
}

//! Presentation side of the IR (`pptx`, later `ppt` and `odp`).
//!
//! The third document root, and it is a root rather than a flavour of
//! [`TextDocument`](crate::text::TextDocument) for the reason
//! `technical-analysis-presentations.md` §1 gives: a slide is a **canvas of
//! absolutely positioned shapes** with no semantic reading order, not a flow of
//! blocks.
//!
//! Three rules the types encode, each of them a decision taken in Phase 12:
//!
//! * **The cascade is reference + delta.** A [`Placeholder`] keeps its identity
//!   (`ph_type` + `idx`) and its slide's [`Slide::layout`]; the properties it
//!   inherits from layout, master and theme are *not* copied into it. A
//!   placeholder that changes nothing carries an empty [`ShapeProps`].
//! * **Reading order is a policy, and the original order is data.** Shapes are
//!   stored in reading order (placeholders by type, then the rest by top-left),
//!   and every shape remembers its [`Shape::z_index`] — its index in the source
//!   `p:spTree` — so the round trip can put them back.
//! * **A layout names its title and its body.** DocMark-P writes the title as
//!   the slide heading and the primary body as plain blocks under it
//!   (spike P2), which is only resolvable because [`Layout`] says which
//!   placeholder is which.
//!
//! Nothing here does I/O: the package skeleton and the media are
//! [`AssetId`](crate::assets::AssetId)s, exactly as images already were.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::addressing::{Addressing, NodeId};
use crate::assets::AssetId;
use crate::image::{Flip, ImageRef, RawId};
use crate::style::{FontProps, ParaProps, StyleCatalog};
use crate::text::{Block, DocumentMeta, RawFragment, Table};
use crate::units::{Point, Size};

/// A presentation.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Presentation {
    pub meta: DocumentMeta,
    /// The monotonic id counter serialised as `next-id`.
    pub addressing: Addressing,
    pub styles: StyleCatalog,
    pub layouts: LayoutCatalog,
    /// `p:sldSz`: the canvas every slide's geometry is relative to.
    pub slide_size: Size,
    /// Slides in presentation order — `p:sldIdLst`, never file-name order.
    pub slides: Vec<Slide>,
    /// The original package, kept opaquely so the writer re-injects slides into
    /// it instead of regenerating masters, theme and `tableStyles.xml`
    /// (spike P3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skeleton: Option<SkeletonRef>,
    /// The markup behind every [`RawId`] a slide or a shape refers to.
    ///
    /// Deck-level, because the ids are: a stub says `raw=r7` and the payload is
    /// found here, exactly as a `Sheet` keeps the fragments its charts point at.
    /// The Phase 11 sidecar is where these bytes go when the deck is
    /// serialised; keeping them in one list is what lets a serialiser write the
    /// sidecar without walking every shape.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub raw: Vec<RawFragment>,
}

impl Presentation {
    /// Every block of every slide, in reading order, notes included.
    ///
    /// The traversal a consumer wants when it only cares about text — `tokens`,
    /// `search` — and it is deliberately flat: the shape a block came from is
    /// reached through [`Presentation::slides`].
    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.slides.iter().flat_map(|slide| slide.blocks())
    }
}

/// A reference to the preserved package (spike P3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SkeletonRef {
    /// The whole original package, content-hashed by the asset store.
    pub asset: AssetId,
    /// Parts the writer rebuilds from the IR instead of copying. Everything
    /// else is written back byte for byte — «preserve everything, rebuild as
    /// the exception».
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rebuilt_parts: Vec<String>,
}

/// Identifier of a slide layout, as written in the DocMark front matter.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayoutId(pub String);

impl LayoutId {
    pub fn new(id: impl Into<String>) -> Self {
        LayoutId(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for LayoutId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Identifier of a slide master.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MasterId(pub String);

impl MasterId {
    pub fn new(id: impl Into<String>) -> Self {
        MasterId(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for MasterId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Layouts and masters, the targets every placeholder reference resolves
/// against. The parallel of [`StyleCatalog`] for the placeholder cascade.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct LayoutCatalog {
    /// Ordered for determinism, as every catalogue in the IR is.
    pub layouts: BTreeMap<LayoutId, Layout>,
    pub masters: BTreeMap<MasterId, Master>,
}

impl LayoutCatalog {
    pub fn is_empty(&self) -> bool {
        self.layouts.is_empty() && self.masters.is_empty()
    }

    pub fn layout(&self, id: &LayoutId) -> Option<&Layout> {
        self.layouts.get(id)
    }

    /// The master a layout hangs from, when both are known.
    pub fn master_of(&self, id: &LayoutId) -> Option<&Master> {
        let master = self.layouts.get(id)?.master.as_ref()?;
        self.masters.get(master)
    }
}

/// A slide layout: a named set of placeholder positions inherited from a
/// master.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Layout {
    /// `p:cSld@name` — "Title and Content".
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master: Option<MasterId>,
    /// The layout's own placeholders, in `p:spTree` order.
    pub placeholders: Vec<LayoutPlaceholder>,
}

impl Layout {
    /// The placeholder DocMark-P writes as the slide heading.
    ///
    /// Title and centred title are the same thing to a reader, and a layout
    /// never has both.
    pub fn title(&self) -> Option<&LayoutPlaceholder> {
        self.placeholders.iter().find(|p| p.ph_type.is_title())
    }

    /// The placeholder DocMark-P writes as plain blocks under the heading: the
    /// first body-like one in layout order (spike P2 §3.2).
    pub fn body(&self) -> Option<&LayoutPlaceholder> {
        self.placeholders.iter().find(|p| p.ph_type.is_body())
    }
}

/// A slide master.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Master {
    pub name: String,
    /// Theme part name, kept as a reference: the theme itself lives in the
    /// preserved skeleton, not in the IR.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    pub placeholders: Vec<LayoutPlaceholder>,
}

/// A placeholder as a layout or master declares it: an identity, a position and
/// the properties slides inherit from it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct LayoutPlaceholder {
    pub ph_type: PhType,
    /// `p:ph@idx`. Absent on title placeholders, which are identified by type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idx: Option<u32>,
    pub geometry: ShapeGeometry,
    #[serde(skip_serializing_if = "ShapeProps::is_empty")]
    pub props: ShapeProps,
}

/// `p:ph@type`. The identity half of the cascade: what a shape *is* on the
/// slide, independent of what it looks like.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PhType {
    Title,
    CenterTitle,
    Subtitle,
    /// `body`, and the default when `p:ph` carries no type at all.
    #[default]
    Body,
    Object,
    Chart,
    Table,
    ClipArt,
    Diagram,
    Media,
    Picture,
    SlideNumber,
    DateTime,
    Footer,
    Header,
    /// A type this version does not model, kept verbatim so the writer can put
    /// it back.
    Other(String),
}

impl PhType {
    /// The PresentationML keyword.
    pub fn as_str(&self) -> &str {
        match self {
            PhType::Title => "title",
            PhType::CenterTitle => "ctrTitle",
            PhType::Subtitle => "subTitle",
            PhType::Body => "body",
            PhType::Object => "obj",
            PhType::Chart => "chart",
            PhType::Table => "tbl",
            PhType::ClipArt => "clipArt",
            PhType::Diagram => "dgm",
            PhType::Media => "media",
            PhType::Picture => "pic",
            PhType::SlideNumber => "sldNum",
            PhType::DateTime => "dt",
            PhType::Footer => "ftr",
            PhType::Header => "hdr",
            PhType::Other(name) => name,
        }
    }

    /// Parses a `p:ph@type` value. An absent attribute means [`PhType::Body`],
    /// which is what PresentationML says and not a guess.
    pub fn parse(keyword: &str) -> PhType {
        match keyword.trim() {
            "title" => PhType::Title,
            "ctrTitle" => PhType::CenterTitle,
            "subTitle" => PhType::Subtitle,
            "" | "body" => PhType::Body,
            "obj" => PhType::Object,
            "chart" => PhType::Chart,
            "tbl" => PhType::Table,
            "clipArt" => PhType::ClipArt,
            "dgm" => PhType::Diagram,
            "media" => PhType::Media,
            "pic" => PhType::Picture,
            "sldNum" => PhType::SlideNumber,
            "dt" => PhType::DateTime,
            "ftr" => PhType::Footer,
            "hdr" => PhType::Header,
            other => PhType::Other(other.to_string()),
        }
    }

    /// True for the placeholder DocMark-P writes as the slide heading.
    pub fn is_title(&self) -> bool {
        matches!(self, PhType::Title | PhType::CenterTitle)
    }

    /// True for the placeholders that hold slide content in reading order.
    pub fn is_body(&self) -> bool {
        matches!(
            self,
            PhType::Body | PhType::Subtitle | PhType::Object | PhType::Table | PhType::Chart
        )
    }

    /// True for the furniture a deck repeats on every slide. It is inherited
    /// from the layout and carries no authored content, so the lossy fidelity
    /// levels drop it.
    pub fn is_furniture(&self) -> bool {
        matches!(
            self,
            PhType::SlideNumber | PhType::DateTime | PhType::Footer | PhType::Header
        )
    }
}

impl std::fmt::Display for PhType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A slide.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Slide {
    /// Stable address of the slide (spec §11.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<NodeId>,
    /// The layout this slide's placeholders resolve against. `None` only for a
    /// deck built from scratch, before a layout has been chosen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<LayoutId>,
    /// `p:cSld@name`, when the deck names its slides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Shapes in **reading order**; the source order is each shape's
    /// [`Shape::z_index`].
    pub shapes: Vec<Shape>,
    /// Speaker notes. `None` when the deck has no notes slide for this slide;
    /// an empty `Vec` when it has one and it is empty, which are different
    /// things to the writer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<Block>>,
    /// `p:sld@show="0"`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// The section (`p14:sectionLst`) this slide belongs to, by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// Slide-level subtrees with no IR representation: `p:transition`,
    /// `p:timing`. Stored in the Phase 11 sidecar and referenced from here.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub raw: Vec<RawId>,
}

impl Slide {
    /// The slide's title text, when it has a title placeholder holding text.
    pub fn title(&self) -> Option<String> {
        self.shapes.iter().find_map(|shape| match &shape.kind {
            ShapeKind::Placeholder(ph) if ph.ph_type.is_title() => {
                let text = blocks_text(&ph.body);
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            }
            _ => None,
        })
    }

    /// Every block the slide holds, in reading order, notes last.
    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        fn shape_blocks<'a>(shapes: &'a [Shape], out: &mut Vec<&'a Block>) {
            for shape in shapes {
                match &shape.kind {
                    ShapeKind::Placeholder(ph) => out.extend(ph.body.iter()),
                    ShapeKind::TextBox { body } => out.extend(body.iter()),
                    ShapeKind::Group(children) => shape_blocks(children, out),
                    ShapeKind::Picture(_)
                    | ShapeKind::Table(_)
                    | ShapeKind::Chart(_)
                    | ShapeKind::Raw(_) => {}
                }
            }
        }
        let mut blocks = Vec::new();
        shape_blocks(&self.shapes, &mut blocks);
        if let Some(notes) = &self.notes {
            blocks.extend(notes.iter());
        }
        blocks.into_iter()
    }
}

/// A shape on a slide: what every one of them has, plus what it is.
///
/// Architecture §9.1 sketched `Shape` as a bare enum. It is a struct here
/// because identity, geometry and the source z-order belong to *every* shape —
/// a group and a picture are equally addressable and equally reversible — and
/// repeating those four fields in seven variants is how they drift apart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Shape {
    /// Stable address of the shape (spec §11.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<NodeId>,
    /// `p:cNvPr@name` — "Arrow 1". What a human sees in the selection pane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Index of the shape in the source `p:spTree`, which is z-order. Kept so
    /// the reading-order policy is reversible (analysis §5.3).
    pub z_index: u32,
    #[serde(default)]
    pub geometry: ShapeGeometry,
    pub kind: ShapeKind,
}

impl Shape {
    /// A shape of `kind` at `z_index`, with no geometry of its own.
    pub fn new(z_index: u32, kind: ShapeKind) -> Self {
        Shape {
            id: None,
            name: None,
            z_index,
            geometry: ShapeGeometry::default(),
            kind,
        }
    }

    /// The placeholder identity of this shape, when it has one.
    pub fn placeholder(&self) -> Option<&Placeholder> {
        match &self.kind {
            ShapeKind::Placeholder(ph) => Some(ph),
            _ => None,
        }
    }
}

/// What a shape actually is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "shape", content = "data")]
pub enum ShapeKind {
    /// A shape filling a slot the layout declares.
    Placeholder(Placeholder),
    /// A free text box: content with a position and nothing inherited.
    TextBox {
        body: Vec<Block>,
    },
    Picture(ImageRef),
    Table(Table),
    Chart(ChartRef),
    Group(Vec<Shape>),
    /// Something with no Markdown representation, kept as a visible stub over a
    /// raw payload (analysis §2). The stub is the point: an agent must know the
    /// object is there and must not hand-edit it.
    Raw(RawShape),
}

/// A placeholder: identity and delta, never the resolved cascade.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Placeholder {
    pub ph_type: PhType,
    /// `p:ph@idx`, which is what actually matches a slide placeholder to its
    /// layout one; the type alone is ambiguous when a layout has two bodies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idx: Option<u32>,
    /// An empty body is content, not absence: the box holds its layout
    /// position and PowerPoint prompts in it.
    pub body: Vec<Block>,
    /// Only what this shape changes over its layout placeholder.
    #[serde(skip_serializing_if = "ShapeProps::is_empty")]
    pub delta: ShapeProps,
}

/// A chart. Phase 13 records that one is there and where its data lives;
/// Phase 16 is what fills in the series.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct ChartRef {
    /// `barChart`, `lineChart`, … as the chart part names it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The embedded `.xlsx` holding the series — a real workbook, and we
    /// already own a reader for it (on-ramp).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workbook: Option<AssetId>,
    /// The chart XML, in the sidecar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<RawId>,
}

/// A shape the IR does not model, and why.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct RawShape {
    pub kind: RawShapeKind,
    /// The source markup, in the Phase 11 sidecar. `None` at the fidelity
    /// levels that do not carry raw payloads — the stub survives, the bytes do
    /// not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<RawId>,
    /// The text the shape shows, when it shows any. A stub that swallows an
    /// arrow's label is a silent loss.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
}

/// The kinds of unmodelled shape, as the stub names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RawShapeKind {
    /// A preset or custom geometry with no Markdown equivalent.
    #[default]
    Shape,
    Connector,
    SmartArt,
    Ole,
    Media,
    /// `mc:AlternateContent` and anything else the reader could not classify.
    Other,
}

impl RawShapeKind {
    /// The class DocMark-P writes on the stub: `::: {.shape …}`.
    pub fn as_str(self) -> &'static str {
        match self {
            RawShapeKind::Shape => "shape",
            RawShapeKind::Connector => "connector",
            RawShapeKind::SmartArt => "smartart",
            RawShapeKind::Ole => "ole",
            RawShapeKind::Media => "media",
            RawShapeKind::Other => "object",
        }
    }
}

impl std::fmt::Display for RawShapeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a shape sits on the canvas: `a:xfrm` plus the preset geometry name.
///
/// Absent position and size mean *inherited* — a placeholder that sits exactly
/// where its layout puts it stores nothing, which is the same
/// reference-plus-delta rule the styles follow.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct ShapeGeometry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos: Option<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    /// `a:prstGeom@prst` — `rect`, `roundRect`, `rightArrow`. The name DocMark-P
    /// writes as `geom=` on a shape, and the only thing that says a box is an
    /// arrow: without it a shape's outline is lost the moment the IR is written
    /// back from anything but the skeleton. `None` for a custom geometry, whose
    /// path list travels as a raw fragment instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// `a:xfrm@rot`, in degrees. The IR keeps degrees; the reader converts from
    /// sixty-thousandths.
    #[serde(skip_serializing_if = "is_zero_f32")]
    pub rotation_deg: f32,
    #[serde(skip_serializing_if = "Flip::is_none")]
    pub flip: Flip,
}

fn is_zero_f32(value: &f32) -> bool {
    *value == 0.0
}

impl ShapeGeometry {
    /// Geometry at a position with a size, unrotated.
    pub fn at(pos: Point, size: Size) -> Self {
        ShapeGeometry {
            pos: Some(pos),
            size: Some(size),
            preset: None,
            rotation_deg: 0.0,
            flip: Flip::default(),
        }
    }

    /// True when the shape inherits everything and stores nothing.
    pub fn is_inherited(&self) -> bool {
        self.pos.is_none() && self.size.is_none() && self.rotation_deg == 0.0 && self.flip.is_none()
    }
}

/// Shape-level formatting, as a delta over the resolved cascade.
///
/// Font and paragraph properties reuse the text model unchanged: a run inside a
/// placeholder is the same run as a run inside a paragraph, and inventing a
/// parallel set of properties for PresentationML is how two models drift.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct ShapeProps {
    #[serde(skip_serializing_if = "FontProps::is_empty")]
    pub font: FontProps,
    #[serde(skip_serializing_if = "ParaProps::is_empty")]
    pub para: ParaProps,
    /// Solid fill, as `#rrggbb` or a theme colour name kept verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    /// `a:normAutofit@fontScale`, in percent. Dropped at the lossy levels with
    /// [`Warning::AutofitStale`](crate::report::Warning), because an agent that
    /// adds a bullet makes it a lie (analysis §5.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_scale: Option<f32>,
}

impl ShapeProps {
    /// True when the shape changes nothing over what it inherits — the case
    /// the cascade must produce for an untouched placeholder.
    pub fn is_empty(&self) -> bool {
        self.font.is_empty()
            && self.para.is_empty()
            && self.fill.is_none()
            && self.font_scale.is_none()
    }
}

/// The concatenated text of a block tree, used for slide titles.
fn blocks_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        let text = match block {
            Block::Paragraph(p) => p.plain_text(),
            Block::Heading(h) => h.paragraph.plain_text(),
            Block::List(list) => list
                .items
                .iter()
                .map(|item| blocks_text(&item.blocks))
                .collect::<Vec<_>>()
                .join(" "),
            Block::Table(_) | Block::Image(_) | Block::TextBox(_) | Block::Raw(_) => String::new(),
        };
        if text.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&text);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Paragraph;
    use crate::units::Length;

    fn placeholder(ph_type: PhType, text: &str) -> Shape {
        Shape::new(
            0,
            ShapeKind::Placeholder(Placeholder {
                ph_type,
                body: vec![Block::Paragraph(Paragraph::text(text))],
                ..Default::default()
            }),
        )
    }

    #[test]
    fn a_layout_names_its_title_and_its_body() {
        // Spike P2 §3.2: DocMark-P writes the title as a heading and the body
        // as plain blocks, which is only resolvable from the catalogue.
        let layout = Layout {
            name: "Title and Content".into(),
            master: Some(MasterId::new("M1")),
            placeholders: vec![
                LayoutPlaceholder {
                    ph_type: PhType::Title,
                    ..Default::default()
                },
                LayoutPlaceholder {
                    ph_type: PhType::Body,
                    idx: Some(1),
                    ..Default::default()
                },
                LayoutPlaceholder {
                    ph_type: PhType::Body,
                    idx: Some(2),
                    ..Default::default()
                },
            ],
        };
        assert_eq!(
            layout.title().map(|p| p.ph_type.clone()),
            Some(PhType::Title)
        );
        assert_eq!(
            layout.body().and_then(|p| p.idx),
            Some(1),
            "the primary body is the first one in layout order"
        );
    }

    #[test]
    fn placeholder_types_round_trip_through_their_keyword() {
        for keyword in [
            "title", "ctrTitle", "subTitle", "body", "obj", "chart", "tbl", "clipArt", "dgm",
            "media", "pic", "sldNum", "dt", "ftr", "hdr",
        ] {
            assert_eq!(PhType::parse(keyword).as_str(), keyword);
        }
        // An absent type is `body`, per PresentationML.
        assert_eq!(PhType::parse(""), PhType::Body);
        // And an unknown one survives verbatim rather than becoming `body`.
        assert_eq!(PhType::parse("sldImg").as_str(), "sldImg");
    }

    #[test]
    fn an_untouched_placeholder_carries_no_delta_and_no_geometry() {
        // The trap the on-ramp names: the cascade must emit nothing when
        // nothing changed, or every round trip grows the file.
        let shape = placeholder(PhType::Title, "Resultados");
        assert!(shape.geometry.is_inherited());
        assert!(shape.placeholder().unwrap().delta.is_empty());
    }

    #[test]
    fn slide_title_comes_from_the_title_placeholder_only() {
        let slide = Slide {
            shapes: vec![
                placeholder(PhType::Body, "Crecimiento del 12 %"),
                placeholder(PhType::CenterTitle, "Resultados"),
            ],
            ..Default::default()
        };
        assert_eq!(slide.title().as_deref(), Some("Resultados"));

        let untitled = Slide {
            shapes: vec![placeholder(PhType::Body, "solo cuerpo")],
            ..Default::default()
        };
        assert_eq!(untitled.title(), None);
    }

    #[test]
    fn an_empty_title_placeholder_is_not_a_title() {
        // An empty placeholder is real content — it holds a layout position —
        // but it is not a title a consumer can show.
        let slide = Slide {
            shapes: vec![Shape::new(
                0,
                ShapeKind::Placeholder(Placeholder {
                    ph_type: PhType::Title,
                    ..Default::default()
                }),
            )],
            ..Default::default()
        };
        assert_eq!(slide.title(), None);
    }

    #[test]
    fn slide_blocks_reach_groups_and_notes() {
        let slide = Slide {
            shapes: vec![
                placeholder(PhType::Title, "Riesgos"),
                Shape::new(
                    1,
                    ShapeKind::Group(vec![Shape::new(
                        0,
                        ShapeKind::TextBox {
                            body: vec![Block::Paragraph(Paragraph::text("dentro del grupo"))],
                        },
                    )]),
                ),
            ],
            notes: Some(vec![Block::Paragraph(Paragraph::text("hablar despacio"))]),
            ..Default::default()
        };
        let texts: Vec<String> = slide
            .blocks()
            .map(|b| match b {
                Block::Paragraph(p) => p.plain_text(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(texts, ["Riesgos", "dentro del grupo", "hablar despacio"]);
    }

    #[test]
    fn geometry_reuses_the_existing_length_model() {
        let geometry = ShapeGeometry::at(
            Point::new(Length::from_emu(88_000), Length::from_emu(5_200_000)),
            Size::new(Length::from_cm(2.5), Length::from_emu(1_000_000)),
        );
        assert!(!geometry.is_inherited());
        assert_eq!(geometry.pos.unwrap().x.emu(), 88_000);
        assert_eq!(geometry.size.unwrap().width.emu(), 900_000);
    }
}

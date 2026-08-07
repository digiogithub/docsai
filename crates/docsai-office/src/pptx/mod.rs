//! The `.pptx` reader (Phase 13).
//!
//! A sibling of `docx/` and `xlsx/`, on the same `zip` + `quick-xml` foundation
//! spike P1 chose over `ooxmlsdk`. This module is the **package layer**: it
//! answers which parts exist, what they are, and in what order the slides come.
//! Filling those slides with shapes is the increment after this one.
//!
//! Two rules from spike P3 are enforced here rather than assumed:
//!
//! * **A part is what its content type says it is**, never what its name
//!   suggests. `ppt/slides/slide1.xml` is a convention; the deck a converter
//!   produced may put its slides anywhere, and the reader still has to find
//!   them.
//! * **Order comes from `p:sldIdLst`**, never from the part names. `slide3.xml`
//!   being the first slide of the deck is legal and common after a reorder in
//!   PowerPoint.

use std::collections::BTreeMap;
use std::io::{Read, Seek};

use docsai_model::assets::AssetStore;
use docsai_model::presentation::{
    Layout, LayoutCatalog, LayoutId, LayoutPlaceholder, Master, MasterId, PhType, Presentation,
    ShapeGeometry, Slide,
};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::units::{Length, Point, Size};
use docsai_model::Document;

use crate::error::ReadError;
use crate::package::{read_meta, ContentTypes, Package};
use crate::xml::Element;

/// Where PowerPoint puts the presentation part. Used to *detect* a deck and as
/// the last resort when the content types are unreadable; the reader itself
/// resolves the part through the package relationships.
pub(crate) const PRESENTATION_PART: &str = "ppt/presentation.xml";

const PML: &str = "application/vnd.openxmlformats-officedocument.presentationml";
const CT_PRESENTATION: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
const CT_PRESENTATION_MACRO: &str =
    "application/vnd.ms-powerpoint.presentation.macroEnabled.main+xml";

/// Default slide size when `p:sldSz` is missing: 4:3 at 10 × 7.5 in.
const DEFAULT_SLIDE_SIZE: (i64, i64) = (9_144_000, 6_858_000);

/// Reads a `.pptx` presentation into the IR.
pub fn read<R: Read + Seek>(
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    let package = Package::open(reader)?;
    read_package(&package, assets)
}

pub(crate) fn read_package(
    package: &Package,
    // Media and the preserved skeleton land in the store in later increments;
    // the package layer stores nothing.
    _assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    let mut report = ConversionReport::new();
    let types = ContentTypes::read(package);
    let main = main_part(package, &types)?;

    let root = parse(package, &main)?;
    let rels = package.relationships(&main);

    let slide_size = match root.child("sldSz") {
        Some(size) => Size::new(
            Length::from_emu(size.attr_i64("cx").unwrap_or(DEFAULT_SLIDE_SIZE.0)),
            Length::from_emu(size.attr_i64("cy").unwrap_or(DEFAULT_SLIDE_SIZE.1)),
        ),
        None => Size::new(
            Length::from_emu(DEFAULT_SLIDE_SIZE.0),
            Length::from_emu(DEFAULT_SLIDE_SIZE.1),
        ),
    };

    let mut layouts = LayoutCatalog::default();
    let mut wanted_layouts: Vec<String> = Vec::new();

    for master_part in targets(&root, "sldMasterIdLst", "sldMasterId", &rels) {
        if !types.is(&master_part, &format!("{PML}.slideMaster+xml")) {
            report.warn(Warning::Degraded {
                what: format!("slide master `{master_part}`"),
                why: "the package declares it as something else".into(),
            });
            continue;
        }
        let (id, master, own_layouts) = read_master(package, &master_part, &mut report)?;
        wanted_layouts.extend(own_layouts);
        layouts.masters.insert(id, master);
    }

    let sections = read_sections(&root);
    let mut slides = Vec::new();

    for entry in slide_entries(&root) {
        let Some(rel) = entry.rel_id.as_deref().and_then(|id| rels.get(id)) else {
            report.warn(Warning::Degraded {
                what: format!("slide {}", entry.id),
                why: "unresolved relationship".into(),
            });
            continue;
        };
        if rel.external {
            report.warn(Warning::Degraded {
                what: format!("slide {}", entry.id),
                why: "the slide part lives outside the package".into(),
            });
            continue;
        }
        if !types.is(&rel.target, &format!("{PML}.slide+xml")) {
            report.warn(Warning::Degraded {
                what: format!("slide {}", entry.id),
                why: format!("`{}` is not declared as a slide", rel.target),
            });
            continue;
        }
        let slide = read_slide(package, &rel.target, &entry, &sections, &mut report)?;
        if let Some(layout) = &slide.layout {
            wanted_layouts.push(layout.as_str().to_string());
        }
        report.stats.slides = report.stats.slides.saturating_add(1);
        slides.push(slide);
    }

    wanted_layouts.sort();
    wanted_layouts.dedup();
    for layout_part in wanted_layouts {
        if layouts
            .layouts
            .contains_key(&LayoutId::new(layout_part.clone()))
        {
            continue;
        }
        if !types.is(&layout_part, &format!("{PML}.slideLayout+xml")) {
            report.warn(Warning::Degraded {
                what: format!("slide layout `{layout_part}`"),
                why: "the package declares it as something else".into(),
            });
            continue;
        }
        let (id, layout) = read_layout(package, &layout_part, &mut report)?;
        layouts.layouts.insert(id, layout);
    }

    let presentation = Presentation {
        meta: read_meta(package),
        addressing: Default::default(),
        styles: Default::default(),
        layouts,
        slide_size,
        slides,
        skeleton: None,
    };
    Ok((Document::Presentation(presentation), report))
}

/// Locates the presentation part.
///
/// The package relationship of type `officeDocument` is the authority; the
/// content types are the cross-check, and the conventional name is only used
/// when a package carries neither — a deck that opens in PowerPoint and would
/// otherwise be refused over a missing `_rels/.rels`.
fn main_part(package: &Package, types: &ContentTypes) -> Result<String, ReadError> {
    let root_rels = package.relationships("_rels/.rels");
    let by_rel = root_rels
        .of_kind("officeDocument")
        .find(|rel| !rel.external && package.has_part(&rel.target))
        .map(|rel| rel.target.clone())
        .filter(|part| is_presentation_part(types, part));
    if let Some(part) = by_rel {
        return Ok(part);
    }
    let by_type = types
        .parts_of(CT_PRESENTATION)
        .chain(types.parts_of(CT_PRESENTATION_MACRO))
        .find(|part| package.has_part(part))
        .map(str::to_string);
    if let Some(part) = by_type {
        return Ok(part);
    }
    if package.has_part(PRESENTATION_PART) {
        return Ok(PRESENTATION_PART.to_string());
    }
    Err(ReadError::MissingPart(PRESENTATION_PART.into()))
}

fn is_presentation_part(types: &ContentTypes, part: &str) -> bool {
    match types.of(part) {
        Some(kind) => kind == CT_PRESENTATION || kind == CT_PRESENTATION_MACRO,
        // No declaration: the relationship already said this is the main part.
        None => true,
    }
}

/// One `p:sldId`: the deck-wide slide id, and the relationship to its part.
struct SlideEntry {
    /// `p:sldId@id`, which is what `p14:sectionLst` refers to.
    id: i64,
    rel_id: Option<String>,
}

fn slide_entries(root: &Element) -> Vec<SlideEntry> {
    root.child("sldIdLst")
        .into_iter()
        .flat_map(|list| list.children_named("sldId"))
        .map(|entry| SlideEntry {
            id: entry.attr_i64("id").unwrap_or_default(),
            rel_id: rel_id(entry).map(str::to_string),
        })
        .collect()
}

/// The relationship id of an element.
///
/// It has to be matched on the **qualified** name: `p:sldId` carries both
/// `r:id` (the relationship) and `id` (a numeric slide id), so a lookup by
/// local name returns the wrong one — silently, and with a number that parses.
fn rel_id(element: &Element) -> Option<&str> {
    element
        .attrs
        .iter()
        .find(|(name, _)| name.ends_with(":id"))
        .map(|(_, value)| value.as_str())
}

/// Part names reached through `<list><item r:id=…/></list>`, in document order.
fn targets(
    root: &Element,
    list: &str,
    item: &str,
    rels: &crate::package::Relationships,
) -> Vec<String> {
    root.child(list)
        .into_iter()
        .flat_map(|list| list.children_named(item))
        .filter_map(|entry| rels.get(rel_id(entry)?))
        .filter(|rel| !rel.external)
        .map(|rel| rel.target.clone())
        .collect()
}

/// `p14:sectionLst`: slide id to section name.
///
/// Sections live in an extension, so every step of the path is optional and a
/// deck without them is the normal case, not a degraded one.
fn read_sections(root: &Element) -> BTreeMap<i64, String> {
    let mut sections = BTreeMap::new();
    let Some(ext_list) = root.child("extLst") else {
        return sections;
    };
    for section in ext_list
        .children_named("ext")
        .filter_map(|ext| ext.child("sectionLst"))
        .flat_map(|list| list.children_named("section"))
    {
        let Some(name) = section.attr("name").filter(|n| !n.is_empty()) else {
            continue;
        };
        for entry in section
            .child("sldIdLst")
            .into_iter()
            .flat_map(|list| list.children_named("sldId"))
        {
            if let Some(id) = entry.attr_i64("id") {
                sections.insert(id, name.to_string());
            }
        }
    }
    sections
}

fn read_slide(
    package: &Package,
    part: &str,
    entry: &SlideEntry,
    sections: &BTreeMap<i64, String>,
    report: &mut ConversionReport,
) -> Result<Slide, ReadError> {
    let root = parse(package, part)?;
    let rels = package.relationships(part);

    let layout = rels
        .of_kind("slideLayout")
        .find(|rel| !rel.external)
        .map(|rel| LayoutId::new(rel.target.clone()));
    if layout.is_none() {
        report.warn(Warning::Degraded {
            what: format!("slide `{part}`"),
            why: "no slide layout: its placeholders cannot inherit".into(),
        });
    }

    Ok(Slide {
        id: None,
        layout,
        name: csld_name(&root),
        // Shapes, notes and raw payloads are the increments after this one.
        shapes: Vec::new(),
        notes: None,
        hidden: root.attr("show") == Some("0"),
        section: sections.get(&entry.id).cloned(),
        raw: Vec::new(),
    })
}

/// Reads a slide master, returning it with the layout parts it declares.
fn read_master(
    package: &Package,
    part: &str,
    report: &mut ConversionReport,
) -> Result<(MasterId, Master, Vec<String>), ReadError> {
    let root = parse(package, part)?;
    let rels = package.relationships(part);
    let layouts = targets(&root, "sldLayoutIdLst", "sldLayoutId", &rels);
    if layouts.is_empty() {
        report.warn(Warning::Degraded {
            what: format!("slide master `{part}`"),
            why: "declares no layouts".into(),
        });
    }
    let master = Master {
        name: csld_name(&root).unwrap_or_else(|| part_stem(part)),
        theme: rels
            .of_kind("theme")
            .find(|rel| !rel.external)
            .map(|rel| rel.target.clone()),
        placeholders: read_placeholders(&root),
    };
    Ok((MasterId::new(part.to_string()), master, layouts))
}

fn read_layout(
    package: &Package,
    part: &str,
    report: &mut ConversionReport,
) -> Result<(LayoutId, Layout), ReadError> {
    let root = parse(package, part)?;
    let rels = package.relationships(part);
    let master = rels
        .of_kind("slideMaster")
        .find(|rel| !rel.external)
        .map(|rel| MasterId::new(rel.target.clone()));
    if master.is_none() {
        report.warn(Warning::Degraded {
            what: format!("slide layout `{part}`"),
            why: "no slide master: the cascade stops here".into(),
        });
    }
    let layout = Layout {
        name: csld_name(&root).unwrap_or_else(|| part_stem(part)),
        master,
        placeholders: read_placeholders(&root),
    };
    Ok((LayoutId::new(part.to_string()), layout))
}

/// The placeholders a layout or master declares, in `p:spTree` order.
fn read_placeholders(root: &Element) -> Vec<LayoutPlaceholder> {
    let Some(tree) = root.path(&["cSld", "spTree"]) else {
        return Vec::new();
    };
    tree.children_named("sp")
        .filter_map(|shape| {
            let ph = shape.path(&["nvSpPr", "nvPr", "ph"])?;
            Some(LayoutPlaceholder {
                ph_type: PhType::parse(ph.attr("type").unwrap_or_default()),
                idx: ph.attr_i64("idx").and_then(|n| u32::try_from(n).ok()),
                geometry: read_geometry(shape.path(&["spPr", "xfrm"])),
                // The delta over the cascade is increment 13-D.
                props: Default::default(),
            })
        })
        .collect()
}

/// `a:xfrm`. An absent transform is *inherited*, not zero, so it stays empty.
fn read_geometry(xfrm: Option<&Element>) -> ShapeGeometry {
    let Some(xfrm) = xfrm else {
        return ShapeGeometry::default();
    };
    let pos = xfrm.child("off").and_then(|off| {
        Some(Point::new(
            Length::from_emu(off.attr_i64("x")?),
            Length::from_emu(off.attr_i64("y")?),
        ))
    });
    let size = xfrm.child("ext").and_then(|ext| {
        Some(Size::new(
            Length::from_emu(ext.attr_i64("cx")?),
            Length::from_emu(ext.attr_i64("cy")?),
        ))
    });
    ShapeGeometry {
        pos,
        size,
        // DrawingML angles are sixtieth-thousandths of a degree.
        rotation_deg: xfrm.attr_i64("rot").unwrap_or(0) as f32 / 60_000.0,
        flip: docsai_model::image::Flip::from_flags(
            xfrm.attr("flipH").is_some_and(|v| v != "0"),
            xfrm.attr("flipV").is_some_and(|v| v != "0"),
        ),
    }
}

/// `p:cSld@name`, when the part names itself.
fn csld_name(root: &Element) -> Option<String> {
    root.child("cSld")
        .and_then(|c| c.attr("name"))
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// `ppt/slideLayouts/slideLayout1.xml` → `slideLayout1`. The fallback name for
/// a part that does not name itself, which the hand-built corpus does not.
fn part_stem(part: &str) -> String {
    let file = part.rsplit('/').next().unwrap_or(part);
    file.rsplit_once('.').map_or(file, |(stem, _)| stem).into()
}

fn parse(package: &Package, part: &str) -> Result<Element, ReadError> {
    let source = package.text(part)?;
    Element::parse(part, source.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::MemoryAssetStore;

    fn read_fixture(name: &str) -> (Presentation, ConversionReport) {
        let path = format!(
            "{}/../../corpus/pptx/{name}.pptx",
            env!("CARGO_MANIFEST_DIR")
        );
        let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let mut assets = MemoryAssetStore::new();
        let (document, report) = read(file, &mut assets).unwrap_or_else(|e| panic!("{name}: {e}"));
        match document {
            Document::Presentation(deck) => (deck, report),
            other => panic!("expected a presentation, got {}", other.shape_name()),
        }
    }

    #[test]
    fn a_deck_reads_its_slides_its_size_and_its_layouts() {
        let (deck, report) = read_fixture("basic-slides");
        assert_eq!(deck.slides.len(), 2);
        assert_eq!(report.stats.slides, 2);
        assert_eq!(deck.slide_size.width.emu(), 9_144_000);
        assert_eq!(deck.slide_size.height.emu(), 6_858_000);
        assert_eq!(deck.meta.title.as_deref(), Some("Presentación básica"));
        assert_eq!(deck.layouts.layouts.len(), 1);
        assert_eq!(deck.layouts.masters.len(), 1);
        assert!(
            report.warnings.is_empty(),
            "a well-formed deck warns about nothing: {:?}",
            report.warnings
        );
    }

    #[test]
    fn slide_order_comes_from_the_id_list_not_the_file_names() {
        // `slide-order.pptx` numbers its parts against the presentation order on
        // purpose: a reader that sorts part names gets this fixture backwards.
        let (deck, _) = read_fixture("slide-order");
        let parts = order_of("slide-order");
        assert_eq!(
            parts,
            [
                "ppt/slides/slide2.xml",
                "ppt/slides/slide3.xml",
                "ppt/slides/slide1.xml"
            ],
            "the deck's `p:sldIdLst` is 2, 3, 1 — file order would be 1, 2, 3"
        );
        assert_eq!(deck.slides.len(), 3);
    }

    /// The slide parts, in the order the reader visits them.
    fn order_of(name: &str) -> Vec<String> {
        let path = format!(
            "{}/../../corpus/pptx/{name}.pptx",
            env!("CARGO_MANIFEST_DIR")
        );
        let file = std::fs::File::open(&path).unwrap();
        let package = Package::open(file).unwrap();
        let types = ContentTypes::read(&package);
        let main = main_part(&package, &types).unwrap();
        let root = parse(&package, &main).unwrap();
        let rels = package.relationships(&main);
        slide_entries(&root)
            .into_iter()
            .filter_map(|entry| Some(rels.get(entry.rel_id.as_deref()?)?.target.clone()))
            .collect()
    }

    #[test]
    fn every_slide_names_the_layout_it_inherits_from() {
        let (deck, _) = read_fixture("placeholders-cascade");
        let layout_id = LayoutId::new("ppt/slideLayouts/slideLayout1.xml");
        for slide in &deck.slides {
            assert_eq!(slide.layout.as_ref(), Some(&layout_id));
        }
        let layout = deck.layouts.layout(&layout_id).expect("catalogued");
        assert_eq!(layout.name, "slideLayout1");
        assert!(layout.title().is_some(), "the layout declares a title");
        assert_eq!(layout.body().and_then(|p| p.idx), Some(1));
        let master = deck.layouts.master_of(&layout_id).expect("reachable");
        assert_eq!(
            master.theme.as_deref(),
            Some("ppt/theme/theme1.xml"),
            "the theme is a reference; the part itself stays in the package"
        );
    }

    #[test]
    fn a_master_placeholder_keeps_the_geometry_its_layout_omits() {
        let (deck, _) = read_fixture("basic-slides");
        let master = deck.layouts.masters.values().next().expect("one master");
        let title = master
            .placeholders
            .iter()
            .find(|p| p.ph_type.is_title())
            .expect("the master positions the title");
        assert_eq!(title.geometry.pos.unwrap().x.emu(), 838_200);
        assert_eq!(title.geometry.size.unwrap().width.emu(), 7_467_600);

        let layout = deck.layouts.layouts.values().next().expect("one layout");
        let layout_title = layout
            .placeholders
            .iter()
            .find(|p| p.ph_type.is_title())
            .expect("the layout redeclares it");
        assert!(
            layout_title.geometry.is_inherited(),
            "a layout placeholder with no `a:xfrm` inherits, it does not sit at zero"
        );
    }

    #[test]
    fn slides_start_empty_in_this_increment() {
        // 13-B is the package layer. The assertion is here so the increment that
        // fills the shapes has to come back and change it deliberately.
        let (deck, _) = read_fixture("basic-slides");
        assert!(deck.slides.iter().all(|s| s.shapes.is_empty()));
    }

    #[test]
    fn every_deck_in_the_corpus_gets_through_the_package_layer() {
        // The package layer is format-wide: a deck with SmartArt, a chart or an
        // image has the same masters, layouts and slide list as a plain one, and
        // none of them may fail before the shapes are even read.
        let dir = format!("{}/../../corpus/pptx", env!("CARGO_MANIFEST_DIR"));
        let mut decks: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                name.strip_suffix(".pptx").map(str::to_string)
            })
            .collect();
        decks.sort();
        assert!(decks.len() >= 12, "the Phase 12 corpus has twelve decks");
        for name in decks {
            let (deck, report) = read_fixture(&name);
            assert!(!deck.slides.is_empty(), "{name}: no slides");
            assert!(
                !deck.layouts.is_empty(),
                "{name}: no layout catalogue to inherit from"
            );
            assert!(
                report.warnings.is_empty(),
                "{name} degrades at the package layer: {:?}",
                report.warnings
            );
        }
    }

    #[test]
    fn sections_map_slides_by_their_deck_id() {
        // No corpus fixture carries `p14:sectionLst` yet, and the mapping is a
        // pure function of the presentation part, so it is tested as one.
        let xml = r#"<p:presentation xmlns:p="urn:p" xmlns:p14="urn:p14">
            <p:sldIdLst><p:sldId id="256" r:id="rId2"/><p:sldId id="257" r:id="rId3"/></p:sldIdLst>
            <p:extLst><p:ext uri="{521415D9-36F7-43E2-AB2F-B90AF26B5E84}">
              <p14:sectionLst>
                <p14:section name="Intro" id="{1}">
                  <p14:sldIdLst><p14:sldId id="256"/></p14:sldIdLst>
                </p14:section>
                <p14:section name="Cifras" id="{2}">
                  <p14:sldIdLst><p14:sldId id="257"/></p14:sldIdLst>
                </p14:section>
              </p14:sectionLst>
            </p:ext></p:extLst>
        </p:presentation>"#;
        let root = Element::parse("p.xml", xml.as_bytes()).unwrap();
        let sections = read_sections(&root);
        assert_eq!(sections.get(&256).map(String::as_str), Some("Intro"));
        assert_eq!(sections.get(&257).map(String::as_str), Some("Cifras"));

        let entries = slide_entries(&root);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, 256);
        assert_eq!(
            entries[0].rel_id.as_deref(),
            Some("rId2"),
            "`r:id` and `id` are different attributes on the same element"
        );
    }

    #[test]
    fn a_package_without_a_presentation_part_is_an_error_not_a_panic() {
        let path = format!(
            "{}/../../corpus/docx/basic-text.docx",
            env!("CARGO_MANIFEST_DIR")
        );
        let file = std::fs::File::open(&path).unwrap();
        let mut assets = MemoryAssetStore::new();
        assert!(matches!(
            read(file, &mut assets),
            Err(ReadError::MissingPart(_))
        ));
    }

    #[test]
    fn part_stems_name_the_parts_that_do_not_name_themselves() {
        assert_eq!(
            part_stem("ppt/slideLayouts/slideLayout1.xml"),
            "slideLayout1"
        );
        assert_eq!(part_stem("noslash"), "noslash");
    }
}

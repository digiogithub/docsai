//! `word/numbering.xml` → [`ListCatalog`].
//!
//! OOXML keeps two levels of indirection: a paragraph names a `numId`, that
//! `w:num` points at an `abstractNumId`, and the abstract definition holds the
//! levels. The IR flattens the indirection into one definition per `numId`,
//! named `L<numId>`, which is what the DocMark front matter shows.

use docsai_model::list::{ListCatalog, ListDef, ListId, ListLevel, NumFormat};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::units::Length;
use std::collections::BTreeMap;

use crate::xml::Element;

/// The list definitions of a document, plus the `numId` → definition mapping.
#[derive(Debug, Default)]
pub struct Numbering {
    pub catalog: ListCatalog,
    /// `numId` as written in `w:numPr`, mapped to its catalogue entry.
    ids: BTreeMap<i64, ListId>,
}

impl Numbering {
    /// The catalogue id for a `w:numId` value.
    pub fn list_id(&self, num_id: i64) -> Option<&ListId> {
        self.ids.get(&num_id)
    }

    /// Whether a `(numId, ilvl)` pair renders as an ordered list.
    pub fn is_ordered(&self, num_id: i64, ilvl: usize) -> bool {
        self.list_id(num_id)
            .and_then(|id| self.catalog.get(id))
            .is_none_or(|def| def.is_ordered_at(ilvl))
    }
}

/// Reads `numbering.xml`.
pub fn read_numbering(root: &Element, report: &mut ConversionReport) -> Numbering {
    let mut abstracts: BTreeMap<i64, ListDef> = BTreeMap::new();
    for element in root.children_named("abstractNum") {
        let Some(id) = element.attr_i64("abstractNumId") else {
            continue;
        };
        abstracts.insert(id, read_definition(element));
    }

    let mut numbering = Numbering::default();
    for element in root.children_named("num") {
        let Some(num_id) = element.attr_i64("numId") else {
            continue;
        };
        let abstract_id = element
            .child("abstractNumId")
            .and_then(|e| e.attr_i64("val"));
        let Some(def) = abstract_id.and_then(|id| abstracts.get(&id)).cloned() else {
            continue;
        };
        if element.child("lvlOverride").is_some() {
            report.warn(Warning::Degraded {
                what: format!("list {num_id}"),
                why: "level overrides (w:lvlOverride) are not modelled in v1".into(),
            });
        }
        let list_id = ListId::new(format!("L{num_id}"));
        numbering.catalog.insert(list_id.clone(), def);
        numbering.ids.insert(num_id, list_id);
    }
    numbering
}

fn read_definition(element: &Element) -> ListDef {
    let mut levels: Vec<(usize, ListLevel)> = Vec::new();
    for lvl in element.children_named("lvl") {
        let index = lvl.attr_i64("ilvl").unwrap_or(0).max(0) as usize;
        let format = lvl
            .child("numFmt")
            .and_then(|e| e.attr("val"))
            .map(NumFormat::from_ooxml)
            .unwrap_or(NumFormat::Decimal);
        let mut level = ListLevel::new(
            format,
            lvl.child("lvlText")
                .and_then(|e| e.attr("val"))
                .unwrap_or(""),
        );
        level.start = lvl
            .child("start")
            .and_then(|e| e.attr_i64("val"))
            .map(|v| v as i32);
        if let Some(ind) = lvl.path(&["pPr", "ind"]) {
            level.indent = ind
                .attr_i64("left")
                .or_else(|| ind.attr_i64("start"))
                .map(Length::from_twips);
            level.hanging = ind.attr_i64("hanging").map(Length::from_twips);
        }
        levels.push((index, level));
    }

    // Levels may be declared out of order or with gaps; fill so that indexing
    // by `ilvl` is always safe.
    let depth = levels.iter().map(|(i, _)| *i + 1).max().unwrap_or(0);
    let mut ordered = vec![ListLevel::new(NumFormat::Decimal, ""); depth];
    for (index, level) in levels {
        ordered[index] = level;
    }
    ListDef { levels: ordered }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NUMBERING: &str = r#"<w:numbering xmlns:w="urn:w">
      <w:abstractNum w:abstractNumId="0">
        <w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/>
          <w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl>
        <w:lvl w:ilvl="1"><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2)"/></w:lvl>
      </w:abstractNum>
      <w:abstractNum w:abstractNumId="1">
        <w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/></w:lvl>
      </w:abstractNum>
      <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
      <w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
    </w:numbering>"#;

    fn numbering() -> (Numbering, ConversionReport) {
        let mut report = ConversionReport::new();
        let root = Element::parse("numbering.xml", NUMBERING.as_bytes()).unwrap();
        (read_numbering(&root, &mut report), report)
    }

    #[test]
    fn maps_num_ids_through_to_their_levels() {
        let (n, _) = numbering();
        let id = n.list_id(1).unwrap().clone();
        assert_eq!(id, ListId::new("L1"));
        let def = n.catalog.get(&id).unwrap();
        assert_eq!(def.levels.len(), 2);
        assert_eq!(def.levels[0].format, NumFormat::Decimal);
        assert_eq!(def.levels[0].text, "%1.");
        assert_eq!(def.levels[0].indent, Some(Length::from_twips(720)));
        assert_eq!(def.levels[0].hanging, Some(Length::from_twips(360)));
        assert_eq!(def.levels[1].format, NumFormat::LowerLetter);
    }

    #[test]
    fn bullets_and_numbers_are_distinguished_per_level() {
        let (n, _) = numbering();
        assert!(n.is_ordered(1, 0));
        assert!(!n.is_ordered(2, 0));
        assert!(n.is_ordered(99, 0), "unknown lists default to ordered");
    }

    #[test]
    fn gaps_in_ilvl_do_not_shift_the_levels() {
        let xml = r#"<w:numbering xmlns:w="urn:w">
          <w:abstractNum w:abstractNumId="0">
            <w:lvl w:ilvl="2"><w:numFmt w:val="bullet"/><w:lvlText w:val="-"/></w:lvl>
          </w:abstractNum>
          <w:num w:numId="7"><w:abstractNumId w:val="0"/></w:num>
        </w:numbering>"#;
        let mut report = ConversionReport::new();
        let n = read_numbering(
            &Element::parse("n.xml", xml.as_bytes()).unwrap(),
            &mut report,
        );
        let def = n.catalog.get(n.list_id(7).unwrap()).unwrap();
        assert_eq!(def.levels.len(), 3);
        assert_eq!(def.levels[2].format, NumFormat::Bullet);
    }

    #[test]
    fn level_overrides_are_reported_not_silently_dropped() {
        let xml = r#"<w:numbering xmlns:w="urn:w">
          <w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum>
          <w:num w:numId="1"><w:abstractNumId w:val="0"/>
            <w:lvlOverride w:ilvl="0"><w:startOverride w:val="5"/></w:lvlOverride></w:num>
        </w:numbering>"#;
        let mut report = ConversionReport::new();
        read_numbering(
            &Element::parse("n.xml", xml.as_bytes()).unwrap(),
            &mut report,
        );
        assert!(matches!(report.warnings[0], Warning::Degraded { .. }));
    }

    #[test]
    fn a_num_pointing_nowhere_is_skipped_not_fatal() {
        let xml = r#"<w:numbering xmlns:w="urn:w"><w:num w:numId="1"><w:abstractNumId w:val="9"/></w:num></w:numbering>"#;
        let mut report = ConversionReport::new();
        let n = read_numbering(
            &Element::parse("n.xml", xml.as_bytes()).unwrap(),
            &mut report,
        );
        assert!(n.list_id(1).is_none());
    }
}

//! The placeholder cascade: shape → layout → master → theme.
//!
//! PresentationML states a placeholder's formatting in four places at once. The
//! master's `p:txStyles` carries a style per *kind* of placeholder and per
//! outline level; the master's and the layout's own placeholders refine it in
//! their `a:lstStyle`; the slide's shape refines that; and none of them spell
//! out a colour or a font — they name `a:schemeClr val="tx1"` and
//! `+mj-lt`, which are references into the theme.
//!
//! Two rules shape this module, and both come from the IR:
//!
//! * **Resolve, then subtract.** A reference is resolved against the theme
//!   *before* anything is compared, because `#000000` and `tx1` are the same
//!   colour and only one of them is a fact. What a slide shape stores is then
//!   the part that survives the subtraction — its delta, never the resolved
//!   cascade. A placeholder that changes nothing stores nothing.
//! * **The reference stays.** Nothing here flattens: the slide keeps its
//!   [`LayoutId`], and the resolved values live on the layout and master
//!   placeholders, which is where the writer looks them up again.
//!
//! What is resolved is colour, font and size — the three the cascade actually
//! decides — through the shared [`FontProps`], so every property `a:defRPr`
//! states travels with them.

use std::collections::BTreeMap;

use docsai_model::presentation::{LayoutId, PhType};
use docsai_model::report::ConversionReport;
use docsai_model::style::FontProps;

use crate::xml::Element;

/// The outline levels DrawingML addresses: `a:lvl1pPr` … `a:lvl9pPr`.
const LEVELS: usize = 9;

/// Run properties per outline level, which is how every list style in
/// PresentationML is written: one `a:defRPr` per level, nine levels deep.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct LevelStyles {
    levels: Vec<FontProps>,
}

impl LevelStyles {
    /// The style at an outline level. Beyond the nine DrawingML defines — and
    /// for a style that states nothing at all — the answer is "inherits
    /// everything", not an error.
    pub(super) fn at(&self, level: u8) -> FontProps {
        self.levels.get(level as usize).cloned().unwrap_or_default()
    }

    /// `self` layered on top of `base`, level by level: the merge step of the
    /// cascade, delegated to the same [`FontProps::over`] the docx styles use.
    pub(super) fn over(&self, base: &LevelStyles) -> LevelStyles {
        LevelStyles {
            levels: (0..LEVELS)
                .map(|level| {
                    self.levels
                        .get(level)
                        .cloned()
                        .unwrap_or_default()
                        .over(&base.levels.get(level).cloned().unwrap_or_default())
                })
                .collect(),
        }
    }

    fn is_empty(&self) -> bool {
        self.levels.iter().all(FontProps::is_empty)
    }
}

/// Reads the `a:lvl1pPr` … `a:lvl9pPr` of a list style into resolved run
/// properties. Anything the element does not state stays absent, so an empty
/// `a:lstStyle` — which is what most slides carry — costs nothing.
pub(super) fn read_levels(
    container: &Element,
    theme: &Theme,
    report: &mut ConversionReport,
) -> LevelStyles {
    let mut levels = vec![FontProps::default(); LEVELS];
    for (index, slot) in levels.iter_mut().enumerate() {
        let Some(properties) = container.child(&format!("lvl{}pPr", index + 1)) else {
            continue;
        };
        if let Some(def) = properties.child("defRPr") {
            *slot = super::text::font_props(def, theme, report);
        }
    }
    LevelStyles { levels }
}

/// A theme's colour and font schemes, plus the `p:clrMap` of the master that
/// points at it.
///
/// The map matters: a master names its colours by *slot* (`tx1`, `bg1`) and the
/// theme defines them by *scheme name* (`dk1`, `lt1`). They coincide in the
/// default mapping and stop coinciding the moment a deck inverts a master,
/// which is exactly when reading the map instead of assuming it is what keeps
/// the colours right.
#[derive(Debug, Clone, Default)]
pub(super) struct Theme {
    /// Scheme name (`dk1`, `accent1`) to `#rrggbb`.
    colours: BTreeMap<String, String>,
    major_latin: Option<String>,
    minor_latin: Option<String>,
    /// `p:clrMap`: slot name to scheme name.
    map: BTreeMap<String, String>,
}

impl Theme {
    /// Reads an `a:theme` part.
    pub(super) fn read(root: &Element) -> Theme {
        let mut theme = Theme::default();
        let Some(elements) = root.child("themeElements") else {
            return theme;
        };
        if let Some(scheme) = elements.child("clrScheme") {
            for entry in scheme.children() {
                if let Some(colour) = scheme_colour(entry) {
                    theme.colours.insert(entry.name.clone(), colour);
                }
            }
        }
        if let Some(fonts) = elements.child("fontScheme") {
            theme.major_latin = latin_of(fonts.child("majorFont"));
            theme.minor_latin = latin_of(fonts.child("minorFont"));
        }
        theme
    }

    /// Adds the `p:clrMap` of the master this theme is reached from.
    pub(super) fn with_map(mut self, master: &Element) -> Theme {
        if let Some(map) = master.child("clrMap") {
            for (name, value) in &map.attrs {
                self.map.insert(name.clone(), value.clone());
            }
        }
        self
    }

    /// True when there is no theme to resolve against — a deck whose master
    /// declares no theme relationship.
    pub(super) fn is_empty(&self) -> bool {
        self.colours.is_empty() && self.major_latin.is_none() && self.minor_latin.is_none()
    }

    /// Resolves an `a:schemeClr@val` to `#rrggbb`.
    pub(super) fn colour(&self, val: &str) -> Option<String> {
        let name = self.map.get(val).map(String::as_str).unwrap_or(val);
        self.colours.get(name).cloned()
    }

    /// Resolves an `a:latin@typeface`. `+mj-lt` is the major (heading) font and
    /// `+mn-lt` the minor (body) one; anything without the `+` is already a
    /// font name and travels unchanged.
    pub(super) fn font(&self, typeface: &str) -> Option<String> {
        let Some(reference) = typeface.strip_prefix('+') else {
            return (!typeface.is_empty()).then(|| typeface.to_string());
        };
        // `+mj-lt`, `+mj-ea`, `+mj-cs`: the script differs, the slot does not,
        // and this reader models one font name per run.
        match reference.split('-').next() {
            Some("mj") => self.major_latin.clone(),
            Some("mn") => self.minor_latin.clone(),
            _ => None,
        }
    }
}

/// One entry of `a:clrScheme`, which states its colour in one of two ways.
fn scheme_colour(entry: &Element) -> Option<String> {
    if let Some(srgb) = entry.child("srgbClr").and_then(|c| c.attr("val")) {
        return hex(srgb);
    }
    // `a:sysClr` is a system colour with the value it last resolved to
    // alongside it — the only number in the file, and the one PowerPoint itself
    // falls back to.
    entry
        .child("sysClr")
        .and_then(|c| c.attr("lastClr"))
        .and_then(hex)
}

fn hex(value: &str) -> Option<String> {
    (value.len() == 6 && value.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| format!("#{}", value.to_ascii_lowercase()))
}

fn latin_of(slot: Option<&Element>) -> Option<String> {
    slot?
        .child("latin")?
        .attr("typeface")
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// A placeholder as a layout or a master declares it, for cascade purposes.
#[derive(Debug, Clone)]
struct PlaceholderStyles {
    ph_type: PhType,
    idx: Option<u32>,
    levels: LevelStyles,
}

/// Whether two placeholder declarations are the same slot.
///
/// `idx` is the identity that matters — a layout with two bodies is ambiguous
/// by type alone — with one exception PresentationML makes itself: a title
/// carries no `idx` and is matched by type.
fn same_slot(a_type: &PhType, a_idx: Option<u32>, b_type: &PhType, b_idx: Option<u32>) -> bool {
    if a_type.is_title() && b_type.is_title() {
        return true;
    }
    match (a_idx, b_idx) {
        (Some(a), Some(b)) => a == b,
        (None, None) => a_type == b_type,
        _ => false,
    }
}

#[derive(Debug, Clone, Default)]
struct MasterStyles {
    theme: Theme,
    /// `p:txStyles`, one per kind of placeholder.
    title: LevelStyles,
    body: LevelStyles,
    other: LevelStyles,
    placeholders: Vec<PlaceholderStyles>,
}

impl MasterStyles {
    /// The `p:txStyles` entry a placeholder of this type inherits from.
    fn kind(&self, ph_type: &PhType) -> &LevelStyles {
        if ph_type.is_title() {
            &self.title
        } else if ph_type.is_body() {
            &self.body
        } else {
            &self.other
        }
    }

    /// What a slot inherits from this master: its text style for the kind, with
    /// the master placeholder's own list style on top.
    fn inherited(&self, ph_type: &PhType, idx: Option<u32>) -> LevelStyles {
        let base = self.kind(ph_type);
        match self
            .placeholders
            .iter()
            .find(|p| same_slot(&p.ph_type, p.idx, ph_type, idx))
        {
            Some(placeholder) => placeholder.levels.over(base),
            None => base.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct LayoutStyles {
    master: Option<String>,
    placeholders: Vec<PlaceholderStyles>,
}

/// Every reference a slide's text resolves against, gathered while the masters
/// and layouts are read.
///
/// Keyed by part name, because that is what [`LayoutId`] and `MasterId` already
/// are: the one identifier a package guarantees is unique.
#[derive(Debug, Default)]
pub(super) struct Cascade {
    masters: BTreeMap<String, MasterStyles>,
    layouts: BTreeMap<String, LayoutStyles>,
    /// `p:defaultTextStyle`: what text outside any placeholder starts from.
    default_text: LevelStyles,
    /// The theme of the first master, used for the parts of a deck that no
    /// master owns — `p:defaultTextStyle` and a slide whose layout is missing.
    fallback_theme: Theme,
}

impl Cascade {
    /// Records a master: its theme, its `p:txStyles` and its placeholders.
    pub(super) fn add_master(
        &mut self,
        part: &str,
        root: &Element,
        theme: Theme,
        report: &mut ConversionReport,
    ) {
        let mut styles = MasterStyles {
            theme,
            ..Default::default()
        };
        if let Some(tx) = root.child("txStyles") {
            for (name, slot) in [
                ("titleStyle", &mut styles.title),
                ("bodyStyle", &mut styles.body),
                ("otherStyle", &mut styles.other),
            ] {
                if let Some(style) = tx.child(name) {
                    *slot = read_levels(style, &styles.theme, report);
                }
            }
        }
        styles.placeholders = placeholder_styles(root, &styles.theme, report);
        if self.fallback_theme.is_empty() {
            self.fallback_theme = styles.theme.clone();
        }
        self.masters.insert(part.to_string(), styles);
    }

    /// Records a layout and the master it hangs from.
    pub(super) fn add_layout(
        &mut self,
        part: &str,
        root: &Element,
        master: Option<&str>,
        report: &mut ConversionReport,
    ) {
        let theme = self.theme_of_master(master).clone();
        let placeholders = placeholder_styles(root, &theme, report);
        self.layouts.insert(
            part.to_string(),
            LayoutStyles {
                master: master.map(str::to_string),
                placeholders,
            },
        );
    }

    /// Reads `p:defaultTextStyle` from the presentation part. Called after the
    /// masters, because it is resolved against the deck's theme like everything
    /// else.
    pub(super) fn add_default_text(&mut self, root: &Element, report: &mut ConversionReport) {
        if let Some(style) = root.child("defaultTextStyle") {
            let theme = self.fallback_theme.clone();
            self.default_text = read_levels(style, &theme, report);
        }
    }

    fn theme_of_master(&self, master: Option<&str>) -> &Theme {
        master
            .and_then(|part| self.masters.get(part))
            .map(|styles| &styles.theme)
            .unwrap_or(&self.fallback_theme)
    }

    /// The theme a slide's text resolves against, reached through its layout.
    pub(super) fn theme_of(&self, layout: Option<&LayoutId>) -> &Theme {
        let master = layout
            .and_then(|id| self.layouts.get(id.as_str()))
            .and_then(|layout| layout.master.as_deref());
        self.theme_of_master(master)
    }

    /// What a slide placeholder inherits: the master's style for its kind, the
    /// master placeholder's list style, then the layout placeholder's.
    pub(super) fn inherited(
        &self,
        layout: Option<&LayoutId>,
        ph_type: &PhType,
        idx: Option<u32>,
    ) -> LevelStyles {
        let layout = layout.and_then(|id| self.layouts.get(id.as_str()));
        let master = layout
            .and_then(|layout| layout.master.as_deref())
            .and_then(|part| self.masters.get(part));

        let base = match master {
            Some(master) => master.inherited(ph_type, idx),
            None => LevelStyles::default(),
        };
        match layout.and_then(|layout| {
            layout
                .placeholders
                .iter()
                .find(|p| same_slot(&p.ph_type, p.idx, ph_type, idx))
        }) {
            Some(placeholder) => placeholder.levels.over(&base),
            None => base,
        }
    }

    /// What text outside a placeholder inherits: the deck's default text style,
    /// over the master's `p:otherStyle`.
    pub(super) fn inherited_text_box(&self, layout: Option<&LayoutId>) -> LevelStyles {
        let other = layout
            .and_then(|id| self.layouts.get(id.as_str()))
            .and_then(|layout| layout.master.as_deref())
            .and_then(|part| self.masters.get(part))
            .map(|master| master.other.clone())
            .unwrap_or_default();
        self.default_text.over(&other)
    }

    /// What a master's own placeholder resolves to — the reference the IR
    /// stores on it, not a delta.
    pub(super) fn master_placeholder(
        &self,
        master: &str,
        ph_type: &PhType,
        idx: Option<u32>,
    ) -> LevelStyles {
        match self.masters.get(master) {
            Some(styles) => styles.inherited(ph_type, idx),
            None => LevelStyles::default(),
        }
    }
}

/// The list styles of every placeholder a layout or master declares.
fn placeholder_styles(
    root: &Element,
    theme: &Theme,
    report: &mut ConversionReport,
) -> Vec<PlaceholderStyles> {
    let Some(tree) = root.path(&["cSld", "spTree"]) else {
        return Vec::new();
    };
    tree.children_named("sp")
        .filter_map(|shape| {
            let ph = shape.path(&["nvSpPr", "nvPr", "ph"])?;
            let levels = match shape.path(&["txBody", "lstStyle"]) {
                Some(style) => read_levels(style, theme, report),
                None => LevelStyles::default(),
            };
            Some(PlaceholderStyles {
                ph_type: PhType::parse(ph.attr("type").unwrap_or_default()),
                idx: ph.attr_i64("idx").and_then(|n| u32::try_from(n).ok()),
                levels,
            })
        })
        .collect()
}

/// The resolved level-1 properties, as the IR stores them on a layout or master
/// placeholder. Empty when the cascade decides nothing, which keeps a bare
/// package bare.
pub(super) fn reference_font(levels: &LevelStyles) -> FontProps {
    if levels.is_empty() {
        return FontProps::default();
    }
    levels.at(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(xml: &str) -> Element {
        Element::parse("test.xml", xml.as_bytes()).expect("well-formed")
    }

    const THEME: &str = r#"<a:theme xmlns:a="x"><a:themeElements>
        <a:clrScheme name="t">
          <a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
          <a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
          <a:accent1><a:srgbClr val="1E5AC8"/></a:accent1>
        </a:clrScheme>
        <a:fontScheme name="t">
          <a:majorFont><a:latin typeface="Calibri Light"/></a:majorFont>
          <a:minorFont><a:latin typeface="Calibri"/></a:minorFont>
        </a:fontScheme>
      </a:themeElements></a:theme>"#;

    #[test]
    fn a_theme_resolves_its_colours_through_the_masters_map() {
        let master =
            element(r#"<p:sldMaster xmlns:p="x"><p:clrMap bg1="dk1" tx1="lt1"/></p:sldMaster>"#);
        let theme = Theme::read(&element(THEME)).with_map(&master);

        // `tx1` is mapped to `lt1`, which the theme defines as white. Reading
        // the map is the whole point: an inverted master says the opposite of
        // what the slot name suggests.
        assert_eq!(theme.colour("tx1").as_deref(), Some("#ffffff"));
        assert_eq!(theme.colour("bg1").as_deref(), Some("#000000"));
        // An unmapped slot is a scheme name already.
        assert_eq!(theme.colour("accent1").as_deref(), Some("#1e5ac8"));
        assert_eq!(theme.colour("accent6"), None);
    }

    #[test]
    fn theme_font_references_resolve_and_real_names_travel_unchanged() {
        let theme = Theme::read(&element(THEME));
        assert_eq!(theme.font("+mj-lt").as_deref(), Some("Calibri Light"));
        assert_eq!(theme.font("+mn-ea").as_deref(), Some("Calibri"));
        assert_eq!(theme.font("Georgia").as_deref(), Some("Georgia"));
        assert_eq!(theme.font(""), None);
    }

    #[test]
    fn a_layout_placeholder_layers_over_the_masters_text_style() {
        let mut report = ConversionReport::new();
        let mut cascade = Cascade::default();
        let theme = Theme::read(&element(THEME));
        cascade.add_master(
            "m.xml",
            &element(
                r#"<p:sldMaster xmlns:p="x" xmlns:a="y"><p:txStyles><p:bodyStyle>
                     <a:lvl1pPr><a:defRPr sz="2800"><a:solidFill><a:schemeClr val="dk1"/></a:solidFill>
                       <a:latin typeface="+mn-lt"/></a:defRPr></a:lvl1pPr>
                     <a:lvl2pPr><a:defRPr sz="2400"/></a:lvl2pPr>
                   </p:bodyStyle></p:txStyles></p:sldMaster>"#,
            ),
            theme,
            &mut report,
        );
        cascade.add_layout(
            "l.xml",
            &element(
                r#"<p:sldLayout xmlns:p="x" xmlns:a="y"><p:cSld><p:spTree><p:sp>
                     <p:nvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr>
                     <p:txBody><a:lstStyle><a:lvl1pPr><a:defRPr b="1"/></a:lvl1pPr></a:lstStyle></p:txBody>
                   </p:sp></p:spTree></p:cSld></p:sldLayout>"#,
            ),
            Some("m.xml"),
            &mut report,
        );

        let id = LayoutId::new("l.xml");
        let inherited = cascade.inherited(Some(&id), &PhType::Body, Some(1));
        let level1 = inherited.at(0);
        // The size and the resolved theme references come from the master, the
        // bold from the layout: neither overwrites the other.
        assert_eq!(
            level1.size,
            Some(docsai_model::units::Length::from_pt(28.0))
        );
        assert_eq!(level1.name.as_deref(), Some("Calibri"));
        assert_eq!(level1.color.as_deref(), Some("#000000"));
        assert_eq!(level1.bold, Some(true));
        // Deeper levels keep their own size and inherit nothing they were not given.
        assert_eq!(
            inherited.at(1).size,
            Some(docsai_model::units::Length::from_pt(24.0))
        );
        assert_eq!(inherited.at(1).bold, None);
    }

    #[test]
    fn a_slot_with_no_layout_inherits_nothing_rather_than_guessing() {
        let cascade = Cascade::default();
        assert!(cascade.inherited(None, &PhType::Title, None).is_empty());
        assert!(cascade
            .inherited(Some(&LayoutId::new("absent.xml")), &PhType::Body, Some(1))
            .is_empty());
    }
}

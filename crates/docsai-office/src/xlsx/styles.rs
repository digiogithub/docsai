//! `xl/styles.xml` → number formats and a thin style catalogue.

use docsai_model::sheet::NumFmt;
use docsai_model::style::{FontProps, Style, StyleCatalog, StyleId, StyleType, Underline};
use docsai_model::units::Length;

use crate::xml::Element;

/// Parsed stylesheet indexes.
#[derive(Debug, Default)]
pub struct Styles {
    /// numFmtId → format code
    pub num_fmts: std::collections::BTreeMap<u32, String>,
    /// cellXfs entries
    pub cell_xfs: Vec<Xf>,
    /// IR catalogue of named/synthetic styles referenced by cells.
    pub catalog: StyleCatalog,
    /// cellXf index → optional StyleId in the catalogue
    pub xf_styles: Vec<Option<StyleId>>,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct Xf {
    pub num_fmt_id: u32,
    pub font_id: u32,
    pub fill_id: u32,
    pub border_id: u32,
    pub apply_number_format: bool,
    pub apply_font: bool,
    pub apply_fill: bool,
    pub apply_border: bool,
}

fn font_style_id(font: &FontProps) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    font.name.hash(&mut h);
    font.size.map(|l| l.emu()).hash(&mut h);
    font.color.hash(&mut h);
    font.bold.hash(&mut h);
    font.italic.hash(&mut h);
    font.strike.hash(&mut h);
    format!("F{:x}", h.finish())
}

pub fn read_styles(root: &Element) -> Styles {
    let mut styles = Styles::default();

    // Built-in formats (ECMA-376 §18.8.30 subset we care about).
    for (id, code) in BUILTIN_NUM_FMTS {
        styles.num_fmts.insert(*id, (*code).to_string());
    }

    if let Some(num_fmts) = root.child("numFmts") {
        for n in num_fmts.children_named("numFmt") {
            let Some(id) = n.attr_i64("numFmtId").map(|i| i as u32) else {
                continue;
            };
            let code = n.attr("formatCode").unwrap_or("General").to_string();
            styles.num_fmts.insert(id, code);
        }
    }

    let fonts: Vec<FontProps> = root
        .child("fonts")
        .map(|f| f.children_named("font").map(read_font).collect())
        .unwrap_or_default();

    if let Some(xfs) = root.child("cellXfs") {
        for xf_el in xfs.children_named("xf") {
            let xf = Xf {
                num_fmt_id: xf_el.attr_i64("numFmtId").unwrap_or(0).max(0) as u32,
                font_id: xf_el.attr_i64("fontId").unwrap_or(0).max(0) as u32,
                fill_id: xf_el.attr_i64("fillId").unwrap_or(0).max(0) as u32,
                border_id: xf_el.attr_i64("borderId").unwrap_or(0).max(0) as u32,
                apply_number_format: flag(xf_el, "applyNumberFormat"),
                apply_font: flag(xf_el, "applyFont"),
                apply_fill: flag(xf_el, "applyFill"),
                apply_border: flag(xf_el, "applyBorder"),
            };
            styles.cell_xfs.push(xf);
        }
    }

    // Build synthetic style ids for non-default xfs that carry font diffs.
    for (index, xf) in styles.cell_xfs.iter().enumerate() {
        if index == 0 {
            styles.xf_styles.push(None);
            continue;
        }
        let mut interesting = false;
        let mut font = FontProps::default();
        if xf.apply_font || xf.font_id > 0 {
            if let Some(f) = fonts.get(xf.font_id as usize) {
                font = f.clone();
                interesting = !font.is_empty();
            }
        }
        // Header-like fill+bold from the corpus becomes a named style.
        if interesting {
            // Stable id from font fingerprint so Office→DocMark→Office→DocMark
            // keeps the same style= reference rather than CellXf{index}.
            let id = StyleId::new(font_style_id(&font));
            if !styles.catalog.styles.contains_key(&id) {
                styles.catalog.insert(Style {
                    id: id.clone(),
                    name: id.as_str().to_string(),
                    style_type: StyleType::Character,
                    based_on: None,
                    next: None,
                    font,
                    paragraph: Default::default(),
                    is_default: false,
                });
            }
            styles.xf_styles.push(Some(id));
        } else {
            styles.xf_styles.push(None);
        }
    }

    styles
}

impl Styles {
    pub fn num_fmt_for_xf(&self, xf_index: usize) -> Option<NumFmt> {
        let xf = self.cell_xfs.get(xf_index)?;
        let code = self
            .num_fmts
            .get(&xf.num_fmt_id)
            .cloned()
            .unwrap_or_else(|| "General".into());
        if xf.num_fmt_id == 0 && !xf.apply_number_format {
            return None;
        }
        Some(NumFmt {
            code,
            id: Some(xf.num_fmt_id),
        })
    }

    pub fn xf_is_date(&self, xf_index: usize) -> bool {
        self.num_fmt_for_xf(xf_index)
            .map(|f| is_date_format(&f.code))
            .unwrap_or(false)
    }

    pub fn style_id_for_xf(&self, xf_index: usize) -> Option<StyleId> {
        self.xf_styles.get(xf_index).cloned().flatten()
    }
}

fn flag(el: &Element, name: &str) -> bool {
    match el.attr(name) {
        Some("1") | Some("true") | Some("True") => true,
        Some("0") | Some("false") | Some("False") => false,
        // OOXML: presence of apply* without value still means the child overrides.
        None => false,
        Some(_) => true,
    }
}

fn read_font(font: &Element) -> FontProps {
    let mut props = FontProps::default();
    if font.child("b").is_some() {
        props.bold = Some(true);
    }
    if font.child("i").is_some() {
        props.italic = Some(true);
    }
    if font.child("strike").is_some() {
        props.strike = Some(true);
    }
    if let Some(sz) = font.child("sz").and_then(|e| e.attr("val")) {
        if let Ok(pt) = sz.parse::<f64>() {
            props.size = Some(Length::from_pt(pt));
        }
    }
    if let Some(name) = font.child("name").and_then(|e| e.attr("val")) {
        props.name = Some(name.to_string());
    }
    if let Some(color) = font.child("color") {
        if let Some(rgb) = color.attr("rgb") {
            props.color = Some(normalise_rgb(rgb));
        }
    }
    if font.child("u").is_some() {
        props.underline = Some(Underline::Single);
    }
    props
}

fn normalise_rgb(rgb: &str) -> String {
    let rgb = rgb.trim();
    if rgb.len() == 8 {
        // AARRGGBB → #RRGGBB
        format!("#{}", &rgb[2..])
    } else if rgb.len() == 6 {
        format!("#{rgb}")
    } else if let Some(stripped) = rgb.strip_prefix('#') {
        format!("#{stripped}")
    } else {
        format!("#{rgb}")
    }
}

/// True when a format code is date/time oriented.
pub fn is_date_format(code: &str) -> bool {
    let lower = code.to_ascii_lowercase();
    // Strip locale/colour brackets for detection.
    let stripped: String = lower.chars().filter(|c| *c != '"' && *c != '\\').collect();
    if stripped.contains("red")
        && !stripped.contains('y')
        && !stripped.contains('d')
        && !stripped.contains('h')
    {
        // currency formats can mention colors
    }
    let has_date_token = stripped.contains('y')
        || stripped.contains("mm")
        || stripped.contains("dd")
        || stripped.contains('d') && stripped.contains('m')
        || stripped.contains('h') && stripped.contains('m')
        || stripped.contains("am/pm")
        || stripped.contains("a/p");
    // Exclude pure number formats that use `m` as months wrongly: require y or d or time.
    if !has_date_token {
        return false;
    }
    // `#`/`0` heavy numeric formats with no y/d/h are not dates.
    if stripped.contains('#')
        && !stripped.contains('y')
        && !stripped.contains('d')
        && !stripped.contains('h')
    {
        return false;
    }
    true
}

/// ECMA-376 built-in numFmt ids used for date detection and round-trip.
const BUILTIN_NUM_FMTS: &[(u32, &str)] = &[
    (0, "General"),
    (1, "0"),
    (2, "0.00"),
    (3, "#,##0"),
    (4, "#,##0.00"),
    (9, "0%"),
    (10, "0.00%"),
    (11, "0.00E+00"),
    (14, "mm-dd-yy"),
    (15, "d-mmm-yy"),
    (16, "d-mmm"),
    (17, "mmm-yy"),
    (18, "h:mm AM/PM"),
    (19, "h:mm:ss AM/PM"),
    (20, "h:mm"),
    (21, "h:mm:ss"),
    (22, "m/d/yy h:mm"),
    (37, "#,##0 ;(#,##0)"),
    (38, "#,##0 ;[Red](#,##0)"),
    (39, "#,##0.00;(#,##0.00)"),
    (40, "#,##0.00;[Red](#,##0.00)"),
    (45, "mm:ss"),
    (46, "[h]:mm:ss"),
    (47, "mmss.0"),
    (48, "##0.0E+0"),
    (49, "@"),
];

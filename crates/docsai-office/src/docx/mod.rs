//! The `.docx` reader (Fase 1).
//!
//! Built directly on `zip` + `quick-xml` after the R1 spike
//! (`docs/spikes/R1-estrategia-docx.md`): one pass over `document.xml` that
//! produces the IR and preserves everything it does not recognise.

mod body;
mod drawing;
mod format;
mod numbering;
mod styles;

use std::collections::BTreeMap;
use std::io::{Read, Seek};

use docsai_model::assets::AssetStore;
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::text::{
    Block, DocumentMeta, HeaderFooter, HeaderScope, Margins, Orientation, PageGeometry, Section,
    TextDocument,
};
use docsai_model::units::{Length, Size};
use docsai_model::Document;

use crate::error::ReadError;
use crate::package::{Package, Relationships};
use crate::xml::Element;

use body::{read_blocks, read_blocks_of, Ctx, State};

pub(crate) const DOCUMENT_PART: &str = "word/document.xml";

/// Reads a `.docx` into the IR.
///
/// Media are stored in `assets`; everything that could not be represented
/// faithfully is reported in the returned [`ConversionReport`].
pub fn read<R: Read + Seek>(
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    let package = Package::open(reader)?;
    read_package(&package, assets)
}

pub(crate) fn read_package(
    package: &Package,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    if !package.has_part(DOCUMENT_PART) {
        return Err(ReadError::MissingPart(DOCUMENT_PART.into()));
    }
    let mut report = ConversionReport::new();

    if package.part_names().any(|n| n.ends_with("vbaProject.bin")) {
        // Macros are ignored always and by design (AGENTS.md §7, plan Fase 8).
        report.warn(Warning::MacrosIgnored {
            part: "word/vbaProject.bin".into(),
        });
    }

    let meta = read_meta(package);
    let styles = match package.optional_xml("word/styles.xml")? {
        Some(root) => styles::read_styles(&root),
        None => Default::default(),
    };
    report.stats.styles = styles.styles.len() as u32;

    let numbering = match package.optional_xml("word/numbering.xml")? {
        Some(root) => numbering::read_numbering(&root, &mut report),
        None => Default::default(),
    };

    let document_rels = package.relationships(DOCUMENT_PART);
    let mut state = State {
        assets,
        report: &mut report,
        raw_seq: 0,
        revisions: 0,
    };

    // Footnotes first: the body needs their blocks to inline them, and they
    // themselves cannot contain footnote references.
    let footnotes = read_footnotes(package, &styles, &numbering, &document_rels, &mut state)?;

    let source = package.text(DOCUMENT_PART)?;
    let root = Element::parse(DOCUMENT_PART, source.as_bytes())?;
    let doc_body = root.child("body").ok_or_else(|| ReadError::WrongShape {
        part: DOCUMENT_PART.into(),
        expected: "WordprocessingML document".into(),
    })?;

    let ctx = Ctx {
        package,
        rels: &document_rels,
        part: DOCUMENT_PART,
        source,
        styles: &styles,
        numbering: &numbering,
        footnotes: &footnotes,
    };

    let sections = read_sections(doc_body, &ctx, &mut state, package, &styles, &numbering)?;
    let revisions = state.revisions;
    if revisions > 0 {
        report.warn(Warning::RevisionsAccepted { count: revisions });
    }

    let document = Document::Text(TextDocument {
        meta,
        styles,
        list_defs: numbering.catalog,
        sections,
    });
    Ok((document, report))
}

// --------------------------------------------------------------------------
// Sections
// --------------------------------------------------------------------------

/// Splits the body at every `w:sectPr` and reads each section.
///
/// A section ends at the paragraph whose `pPr` carries a `sectPr`; the body's
/// own trailing `sectPr` closes the last one.
fn read_sections(
    doc_body: &Element,
    ctx: &Ctx<'_>,
    state: &mut State<'_>,
    package: &Package,
    styles: &docsai_model::StyleCatalog,
    numbering: &numbering::Numbering,
) -> Result<Vec<Section>, ReadError> {
    let mut sections = Vec::new();
    let mut pending: Vec<&Element> = Vec::new();

    for child in doc_body.children() {
        if child.name == "sectPr" {
            continue; // the trailing one, handled below
        }
        pending.push(child);
        if let Some(sect_pr) = child.path(&["pPr", "sectPr"]) {
            let blocks = read_blocks_of(pending.drain(..), ctx, state);
            sections.push(build_section(
                sect_pr, blocks, ctx, state, package, styles, numbering,
            )?);
        }
    }

    let trailing = doc_body.child("sectPr");
    let blocks = read_blocks_of(pending, ctx, state);
    if !blocks.is_empty() || sections.is_empty() {
        let section = match trailing {
            Some(sect_pr) => {
                build_section(sect_pr, blocks, ctx, state, package, styles, numbering)?
            }
            None => Section {
                blocks,
                ..Default::default()
            },
        };
        sections.push(section);
    }
    Ok(sections)
}

fn build_section(
    sect_pr: &Element,
    blocks: Vec<Block>,
    ctx: &Ctx<'_>,
    state: &mut State<'_>,
    package: &Package,
    styles: &docsai_model::StyleCatalog,
    numbering: &numbering::Numbering,
) -> Result<Section, ReadError> {
    let mut section = Section {
        page: read_page_geometry(sect_pr),
        blocks,
        ..Default::default()
    };

    for (element, scope) in sect_pr
        .children_named("headerReference")
        .map(|e| (e, "header"))
        .chain(
            sect_pr
                .children_named("footerReference")
                .map(|e| (e, "footer")),
        )
    {
        let Some(rel_id) = element.attr_qualified("r:id") else {
            continue;
        };
        let Some(rel) = ctx.rels.get(rel_id) else {
            continue;
        };
        let Some(source) = package.part(&rel.target) else {
            continue;
        };
        let Ok(source) = std::str::from_utf8(source) else {
            continue;
        };
        let root = Element::parse(&rel.target, source.as_bytes())?;
        let part_rels = package.relationships(&rel.target);
        let inner = Ctx {
            package,
            rels: &part_rels,
            part: &rel.target,
            source,
            styles,
            numbering,
            footnotes: ctx.footnotes,
        };
        let header = HeaderFooter {
            scope: match element.attr("type") {
                Some("first") => HeaderScope::First,
                Some("even") => HeaderScope::Even,
                _ => HeaderScope::Default,
            },
            blocks: read_blocks(&root, &inner, state),
        };
        if scope == "header" {
            section.headers.push(header);
        } else {
            section.footers.push(header);
        }
    }

    section.headers.sort_by_key(|h| h.scope);
    section.footers.sort_by_key(|h| h.scope);
    Ok(section)
}

fn read_page_geometry(sect_pr: &Element) -> PageGeometry {
    let mut page = PageGeometry {
        columns: 1,
        ..Default::default()
    };
    if let Some(sz) = sect_pr.child("pgSz") {
        page.size = Size::new(
            Length::from_twips(sz.attr_i64("w").unwrap_or(0)),
            Length::from_twips(sz.attr_i64("h").unwrap_or(0)),
        );
        page.orientation = match sz.attr("orient") {
            Some("landscape") => Orientation::Landscape,
            // Word omits `orient` and simply swaps the dimensions.
            _ if page.size.width > page.size.height => Orientation::Landscape,
            _ => Orientation::Portrait,
        };
    }
    if let Some(mar) = sect_pr.child("pgMar") {
        let twips = |name: &str| Length::from_twips(mar.attr_i64(name).unwrap_or(0));
        page.margins = Margins {
            top: twips("top"),
            right: twips("right"),
            bottom: twips("bottom"),
            left: twips("left"),
            header: twips("header"),
            footer: twips("footer"),
        };
    }
    if let Some(cols) = sect_pr.child("cols").and_then(|c| c.attr_i64("num")) {
        page.columns = cols.clamp(1, 64) as u16;
    }
    page.title_page = sect_pr.child("titlePg").is_some_and(|e| e.ooxml_flag());
    page
}

// --------------------------------------------------------------------------
// Footnotes and document properties
// --------------------------------------------------------------------------

fn read_footnotes(
    package: &Package,
    styles: &docsai_model::StyleCatalog,
    numbering: &numbering::Numbering,
    document_rels: &Relationships,
    state: &mut State<'_>,
) -> Result<BTreeMap<i64, Vec<Block>>, ReadError> {
    let part = match document_rels.first_of_kind("footnotes") {
        Some(rel) => rel.target.clone(),
        None => "word/footnotes.xml".to_string(),
    };
    let Some(bytes) = package.part(&part) else {
        return Ok(BTreeMap::new());
    };
    let Ok(source) = std::str::from_utf8(bytes) else {
        return Ok(BTreeMap::new());
    };
    let root = Element::parse(&part, source.as_bytes())?;
    let rels = package.relationships(&part);
    let empty = BTreeMap::new();
    let ctx = Ctx {
        package,
        rels: &rels,
        part: &part,
        source,
        styles,
        numbering,
        footnotes: &empty,
    };

    let mut out = BTreeMap::new();
    for note in root.children_named("footnote") {
        // The separator pseudo-notes are layout, not content.
        if matches!(
            note.attr("type"),
            Some("separator") | Some("continuationSeparator")
        ) {
            continue;
        }
        let Some(id) = note.attr_i64("id") else {
            continue;
        };
        out.insert(id, read_blocks(note, &ctx, state));
    }
    Ok(out)
}

fn read_meta(package: &Package) -> DocumentMeta {
    let mut meta = DocumentMeta::default();

    if let Ok(Some(core)) = package.optional_xml("docProps/core.xml") {
        let text = |name: &str| core.child(name).map(|e| e.text()).filter(|t| !t.is_empty());
        meta.title = text("title");
        meta.author = text("creator");
        meta.last_modified_by = text("lastModifiedBy");
        meta.created = text("created");
        meta.modified = text("modified");
        meta.language = text("language");
        meta.subject = text("subject");
        meta.keywords = text("keywords");
        meta.description = text("description");
    }

    if let Ok(Some(app)) = package.optional_xml("docProps/app.xml") {
        meta.application = app
            .child("Application")
            .map(|e| e.text())
            .filter(|t| !t.is_empty());
    }

    if let Ok(Some(custom)) = package.optional_xml("docProps/custom.xml") {
        for property in custom.children_named("property") {
            let Some(name) = property.attr("name") else {
                continue;
            };
            let value = property.children().map(|v| v.text()).collect::<String>();
            meta.custom.insert(name.to_string(), value);
        }
    }

    meta
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::MemoryAssetStore;

    fn read_fixture(name: &str) -> (Document, ConversionReport) {
        let path = format!(
            "{}/../../corpus/docx/{name}.docx",
            env!("CARGO_MANIFEST_DIR")
        );
        let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let mut assets = MemoryAssetStore::new();
        read(file, &mut assets).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    fn text_doc(doc: &Document) -> &TextDocument {
        match doc {
            Document::Text(d) => d,
            Document::Workbook(_) => panic!("expected a text document"),
        }
    }

    #[test]
    fn reads_page_geometry_and_document_properties() {
        let (doc, _) = read_fixture("basic-text");
        let doc = text_doc(&doc);
        assert_eq!(doc.meta.title.as_deref(), Some("Texto basico"));
        assert_eq!(doc.meta.author.as_deref(), Some("docsai corpus"));
        assert_eq!(doc.meta.language.as_deref(), Some("es-ES"));
        let page = doc.sections[0].page;
        assert_eq!(page.paper_name(), Some("A4"));
        assert_eq!(page.orientation, Orientation::Portrait);
        assert_eq!(page.margins.top, Length::from_twips(1417));
    }

    #[test]
    fn reads_custom_properties() {
        let (doc, _) = read_fixture("custom-styles");
        let doc = text_doc(&doc);
        assert_eq!(
            doc.meta.custom.get("Departamento").map(String::as_str),
            Some("Ventas")
        );
        assert_eq!(
            doc.meta.custom.get("Revision").map(String::as_str),
            Some("3")
        );
    }

    #[test]
    fn resolves_headers_footers_and_section_settings() {
        let (doc, _) = read_fixture("headers-footers");
        let doc = text_doc(&doc);
        let section = &doc.sections[0];
        assert_eq!(section.page.columns, 2);
        assert!(section.page.title_page);
        assert_eq!(section.headers.len(), 2, "default and first-page headers");
        assert_eq!(section.headers[0].scope, HeaderScope::Default);
        assert_eq!(section.headers[1].scope, HeaderScope::First);
        assert_eq!(section.footers.len(), 1);

        let footer_text: String = section.footers[0]
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(p.plain_text()),
                _ => None,
            })
            .collect();
        assert!(footer_text.contains("Pagina 1 de 3"), "got {footer_text}");
    }

    #[test]
    fn inlines_footnote_content_at_the_reference() {
        use docsai_model::text::Inline;
        let (doc, report) = read_fixture("footnotes");
        let doc = text_doc(&doc);
        // Footnote references carry a character style, so the inline sits
        // inside a `Styled` wrapper: collect recursively.
        fn collect<'a>(inlines: &'a [Inline], out: &mut Vec<&'a Vec<Block>>) {
            for inline in inlines {
                match inline {
                    Inline::Footnote(blocks) => out.push(blocks),
                    Inline::Styled { content, .. } | Inline::Link { content, .. } => {
                        collect(content, out)
                    }
                    _ => {}
                }
            }
        }
        let mut notes = Vec::new();
        for block in doc.blocks() {
            if let Block::Paragraph(p) = block {
                collect(&p.content, &mut notes);
            }
        }
        assert_eq!(notes.len(), 2);
        assert_eq!(report.stats.footnotes, 2);
        let first = match &notes[0][0] {
            Block::Paragraph(p) => p.plain_text(),
            other => panic!("{other:?}"),
        };
        assert!(first.contains("Primera nota al pie"), "got {first}");
    }

    #[test]
    fn a_missing_document_part_is_an_error() {
        let package = Package::default();
        let mut assets = MemoryAssetStore::new();
        assert!(matches!(
            read_package(&package, &mut assets),
            Err(ReadError::MissingPart(_))
        ));
    }
}

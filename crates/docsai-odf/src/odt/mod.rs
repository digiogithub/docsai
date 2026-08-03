//! The `.odt` reader and writer (Phase 4).

mod body;
pub(crate) mod write;

use std::io::{Read, Seek};

use docsai_model::assets::AssetStore;
use docsai_model::report::ConversionReport;
use docsai_model::text::{
    DocumentMeta, HeaderFooter, HeaderScope, Margins, Orientation, PageGeometry, Section,
    TextDocument,
};
use docsai_model::units::{Length, Size};
use docsai_model::Document;

use crate::error::ReadError;
use crate::length::parse_length;
use crate::package::Package;
use crate::styles::{read_automatic_styles, read_named_styles, OdfStyles};
use crate::xml::Element;

use body::{read_blocks, Ctx, State};

const CONTENT_PART: &str = "content.xml";
const STYLES_PART: &str = "styles.xml";
const META_PART: &str = "meta.xml";
pub(crate) const MIME: &str = "application/vnd.oasis.opendocument.text";
const MIME_ODT: &str = MIME;

/// Reads an `.odt` document into the IR.
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
    if !package.has_part(CONTENT_PART) {
        return Err(ReadError::MissingPart(CONTENT_PART.into()));
    }
    if let Some(mime) = package.part("mimetype") {
        let mime = std::str::from_utf8(mime).unwrap_or("").trim();
        if !mime.is_empty() && !mime.contains("opendocument.text") {
            return Err(ReadError::WrongShape {
                part: "mimetype".into(),
                expected: MIME_ODT.into(),
            });
        }
    }

    let mut report = ConversionReport::new();
    let mut styles = OdfStyles::default();

    if let Some(styles_root) = package.optional_xml(STYLES_PART)? {
        // Named styles live under office:styles; automatic under office:automatic-styles.
        if let Some(named) = styles_root.child("styles") {
            read_named_styles(named, &mut styles);
        } else {
            read_named_styles(&styles_root, &mut styles);
        }
        if let Some(auto) = styles_root.child("automatic-styles") {
            read_automatic_styles(auto, &mut styles);
        }
        // List styles may also sit directly under office:styles.
        read_named_styles(&styles_root, &mut styles);
    }

    let content_source = package.text(CONTENT_PART)?;
    let content_root = Element::parse(CONTENT_PART, content_source.as_bytes())?;
    if let Some(auto) = content_root.child("automatic-styles") {
        read_automatic_styles(auto, &mut styles);
    }

    let meta = read_meta(package);
    let page = read_page_geometry(package);
    let (headers, footers) = read_headers_footers(package, &styles, assets, &mut report)?;

    let text_root = content_root
        .path(&["body", "text"])
        .ok_or_else(|| ReadError::WrongShape {
            part: CONTENT_PART.into(),
            expected: "OpenDocument text".into(),
        })?;

    let mut state = State {
        assets,
        report: &mut report,
        raw_seq: 0,
    };
    let ctx = Ctx {
        package,
        part: CONTENT_PART,
        source: content_source,
        styles: &styles,
    };
    let blocks = read_blocks(text_root, &ctx, &mut state);

    report.stats.styles = styles.catalog.styles.len() as u32;

    let document = Document::Text(TextDocument {
        addressing: Default::default(),
        meta,
        styles: styles.catalog,
        list_defs: styles.lists,
        sections: vec![Section {
            id: None,
            page,
            headers,
            footers,
            blocks,
        }],
    });
    Ok((document, report))
}

fn read_meta(package: &Package) -> DocumentMeta {
    let mut meta = DocumentMeta::default();
    let Ok(Some(root)) = package.optional_xml(META_PART) else {
        return meta;
    };
    let office_meta = root.child("meta").unwrap_or(&root);
    let text = |name: &str| {
        office_meta
            .child(name)
            .map(|e| e.text())
            .filter(|t| !t.is_empty())
    };
    meta.title = text("title");
    meta.author = text("initial-creator").or_else(|| text("creator"));
    meta.last_modified_by = text("creator");
    meta.created = text("creation-date");
    meta.modified = text("date");
    meta.language = text("language");
    meta.subject = text("subject");
    meta.description = text("description");
    meta.application = text("generator");
    // Keywords may appear multiple times.
    let keywords: Vec<String> = office_meta
        .children_named("keyword")
        .map(|e| e.text())
        .filter(|t| !t.is_empty())
        .collect();
    if !keywords.is_empty() {
        meta.keywords = Some(keywords.join(", "));
    }
    for ud in office_meta.children_named("user-defined") {
        if let Some(name) = ud.attr("name") {
            meta.custom.insert(name.to_string(), ud.text());
        }
    }
    meta
}

fn read_page_geometry(package: &Package) -> PageGeometry {
    let mut page = PageGeometry {
        size: Size::new(Length::from_cm(21.0), Length::from_cm(29.7)),
        margins: Margins {
            top: Length::from_cm(2.0),
            bottom: Length::from_cm(2.0),
            left: Length::from_cm(2.0),
            right: Length::from_cm(2.0),
            header: Length::ZERO,
            footer: Length::ZERO,
        },
        orientation: Orientation::Portrait,
        columns: 1,
        title_page: false,
    };
    let Ok(Some(styles_root)) = package.optional_xml(STYLES_PART) else {
        return page;
    };
    let layout = styles_root
        .path(&["automatic-styles"])
        .into_iter()
        .flat_map(|a| a.children_named("page-layout"))
        .next()
        .and_then(|pl| pl.child("page-layout-properties"));
    if let Some(props) = layout {
        if let (Some(w), Some(h)) = (
            props.attr("page-width").and_then(parse_length),
            props.attr("page-height").and_then(parse_length),
        ) {
            page.size = Size::new(w, h);
            page.orientation = if w > h {
                Orientation::Landscape
            } else {
                Orientation::Portrait
            };
        }
        let margin = |name: &str| props.attr(name).and_then(parse_length);
        if let Some(v) = margin("margin-top") {
            page.margins.top = v;
        }
        if let Some(v) = margin("margin-bottom") {
            page.margins.bottom = v;
        }
        if let Some(v) = margin("margin-left") {
            page.margins.left = v;
        }
        if let Some(v) = margin("margin-right") {
            page.margins.right = v;
        }
    }
    page
}

fn read_headers_footers(
    package: &Package,
    styles: &OdfStyles,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<(Vec<HeaderFooter>, Vec<HeaderFooter>), ReadError> {
    let mut headers = Vec::new();
    let mut footers = Vec::new();
    let Ok(Some(styles_root)) = package.optional_xml(STYLES_PART) else {
        return Ok((headers, footers));
    };
    let Some(master_styles) = styles_root.child("master-styles") else {
        return Ok((headers, footers));
    };
    let Some(master) = master_styles.child("master-page") else {
        return Ok((headers, footers));
    };

    let source = package.text(STYLES_PART)?;
    let mut state = State {
        assets,
        report,
        raw_seq: 10_000, // keep ids away from body raws
    };
    let ctx = Ctx {
        package,
        part: STYLES_PART,
        source,
        styles,
    };

    if let Some(header) = master.child("header") {
        headers.push(HeaderFooter {
            scope: HeaderScope::Default,
            blocks: read_blocks(header, &ctx, &mut state),
        });
    }
    if let Some(header) = master.child("header-first") {
        headers.push(HeaderFooter {
            scope: HeaderScope::First,
            blocks: read_blocks(header, &ctx, &mut state),
        });
    }
    if let Some(header) = master.child("header-left") {
        headers.push(HeaderFooter {
            scope: HeaderScope::Even,
            blocks: read_blocks(header, &ctx, &mut state),
        });
    }
    if let Some(footer) = master.child("footer") {
        footers.push(HeaderFooter {
            scope: HeaderScope::Default,
            blocks: read_blocks(footer, &ctx, &mut state),
        });
    }
    if let Some(footer) = master.child("footer-first") {
        footers.push(HeaderFooter {
            scope: HeaderScope::First,
            blocks: read_blocks(footer, &ctx, &mut state),
        });
    }
    if let Some(footer) = master.child("footer-left") {
        footers.push(HeaderFooter {
            scope: HeaderScope::Even,
            blocks: read_blocks(footer, &ctx, &mut state),
        });
    }
    headers.sort_by_key(|h| h.scope);
    footers.sort_by_key(|h| h.scope);
    Ok((headers, footers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::text::Block;
    use docsai_model::MemoryAssetStore;

    fn read_fixture(name: &str) -> (Document, ConversionReport) {
        let path = format!("{}/../../corpus/odt/{name}.odt", env!("CARGO_MANIFEST_DIR"));
        let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let mut assets = MemoryAssetStore::new();
        read(file, &mut assets).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    #[test]
    fn reads_basic_text_meta_and_paragraphs() {
        let (doc, _) = read_fixture("basic-text");
        let Document::Text(text) = doc else {
            panic!("expected text");
        };
        assert_eq!(text.meta.title.as_deref(), Some("Texto basico"));
        assert_eq!(text.meta.author.as_deref(), Some("docsai corpus"));
        assert!(text.sections[0].blocks.len() >= 4);
        let first = match &text.sections[0].blocks[0] {
            Block::Paragraph(p) => p.plain_text(),
            other => panic!("{other:?}"),
        };
        assert!(first.contains("Primer parrafo"), "{first}");
    }

    #[test]
    fn deautomatizes_direct_formatting() {
        use docsai_model::text::Inline;
        let (doc, _) = read_fixture("basic-styles");
        let Document::Text(text) = doc else {
            panic!("expected text");
        };
        let has_bold_delta = text.blocks().any(|b| match b {
            Block::Paragraph(p) => p.content.iter().any(|i| match i {
                Inline::Styled { props, .. } => props.direct.bold == Some(true),
                _ => false,
            }),
            _ => false,
        });
        assert!(has_bold_delta, "expected a bold automatic-style delta");
    }
}

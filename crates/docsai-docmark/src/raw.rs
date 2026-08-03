//! Raw-block sidecars (spec §7, plan v2 Phase 11).
//!
//! Under [`RawPolicy::Sidecar`] the body keeps the stub and the bytes move to a
//! file of their own. Naming lives here, in one place, because the serializer
//! writes the reference and the caller writes the file: if the two disagreed,
//! a document would point at a sidecar nobody wrote.

use docsai_model::text::{Block, Inline, RawFragment, TextDocument};
use docsai_model::Document;

use crate::{Options, RawPolicy};

/// One raw-block's bytes and where the DocMark says they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSidecar {
    /// Path relative to the `.dmk.md` file, exactly as written in `src=`.
    pub path: String,
    /// The original markup, byte for byte as it was read.
    pub content: String,
}

/// Where a fragment's bytes go, relative to the DocMark file.
///
/// The id is sanitised the way asset names are: a raw id comes from the source
/// package, and a package is not a trustworthy source of file names.
pub(crate) fn sidecar_path(assets_dir: &str, raw: &RawFragment) -> String {
    let dir = assets_dir.trim_end_matches('/');
    format!("{dir}/_raw/{}.xml", sanitize(raw.id.as_str()))
}

fn sanitize(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    if trimmed.is_empty() {
        "raw".to_string()
    } else {
        trimmed.to_string()
    }
}

/// The sidecar files a serialisation of `doc` with these `options` refers to.
///
/// A pure function of the document, so a caller can write the files before,
/// after or without serialising, and always get the same names the body uses.
/// Empty unless the run both emits raw-blocks (`full`) and puts them aside.
pub fn raw_sidecars(doc: &Document, options: &Options) -> Vec<RawSidecar> {
    if options.fidelity != crate::Fidelity::Full || options.raw != RawPolicy::Sidecar {
        return Vec::new();
    }
    let Document::Text(text) = doc else {
        // Workbooks have no raw-blocks: a cell is either a value or a formula.
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_text(text, options, &mut out);
    out
}

fn collect_text(text: &TextDocument, options: &Options, out: &mut Vec<RawSidecar>) {
    for section in &text.sections {
        for header in &section.headers {
            collect_blocks(&header.blocks, options, out);
        }
        for footer in &section.footers {
            collect_blocks(&footer.blocks, options, out);
        }
        collect_blocks(&section.blocks, options, out);
    }
}

fn collect_blocks(blocks: &[Block], options: &Options, out: &mut Vec<RawSidecar>) {
    for block in blocks {
        match block {
            Block::Raw(raw) => push(raw, options, out),
            Block::Paragraph(paragraph) => collect_inlines(&paragraph.content, options, out),
            Block::Heading(heading) => collect_inlines(&heading.paragraph.content, options, out),
            Block::List(list) => {
                for item in &list.items {
                    collect_blocks(&item.blocks, options, out);
                }
            }
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        collect_blocks(&cell.blocks, options, out);
                    }
                }
            }
            Block::TextBox(text_box) => collect_blocks(&text_box.blocks, options, out),
            Block::Image(_) => {}
        }
    }
}

fn collect_inlines(inlines: &[Inline], options: &Options, out: &mut Vec<RawSidecar>) {
    for inline in inlines {
        match inline {
            Inline::Raw(raw) => push(raw, options, out),
            Inline::Styled { content, .. } | Inline::Link { content, .. } => {
                collect_inlines(content, options, out)
            }
            Inline::Footnote(footnote) => collect_blocks(&footnote.blocks, options, out),
            _ => {}
        }
    }
}

fn push(raw: &RawFragment, options: &Options, out: &mut Vec<RawSidecar>) {
    let path = sidecar_path(&options.assets_dir, raw);
    // Two fragments sharing an id would share a file; the first one wins and
    // the writer points both at it, which is what the source package meant.
    if out.iter().any(|s| s.path == path) {
        return;
    }
    out.push(RawSidecar {
        path,
        content: raw.content.clone(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::image::RawId;
    use docsai_model::text::{Paragraph, Section};

    fn fragment(id: &str, content: &str) -> RawFragment {
        RawFragment {
            id: RawId::new(id),
            format: "ooxml".into(),
            part: "word/document.xml".into(),
            content: content.into(),
        }
    }

    fn document(blocks: Vec<Block>) -> Document {
        Document::Text(TextDocument {
            sections: vec![Section {
                blocks,
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    #[test]
    fn a_sidecar_is_named_after_its_fragment() {
        assert_eq!(
            sidecar_path("assets", &fragment("r7", "<w:sdt/>")),
            "assets/_raw/r7.xml"
        );
    }

    #[test]
    fn an_id_from_the_package_cannot_escape_the_directory() {
        assert_eq!(
            sidecar_path("assets", &fragment("../../etc/passwd", "<x/>")),
            "assets/_raw/etc-passwd.xml"
        );
    }

    #[test]
    fn raw_fragments_are_collected_from_inside_paragraphs_too() {
        let doc = document(vec![
            Block::Raw(fragment("r1", "<a/>")),
            Block::Paragraph(Paragraph {
                content: vec![
                    Inline::Text("x".into()),
                    Inline::Styled {
                        content: vec![Inline::Raw(fragment("r2", "<b/>"))],
                        props: Default::default(),
                    },
                ],
                ..Default::default()
            }),
        ]);
        let sidecars = raw_sidecars(&doc, &Options::default());
        assert_eq!(
            sidecars,
            vec![
                RawSidecar {
                    path: "assets/_raw/r1.xml".into(),
                    content: "<a/>".into()
                },
                RawSidecar {
                    path: "assets/_raw/r2.xml".into(),
                    content: "<b/>".into()
                },
            ]
        );
    }

    #[test]
    fn nothing_is_put_aside_when_nothing_is_emitted() {
        let doc = document(vec![Block::Raw(fragment("r1", "<a/>"))]);
        let inline = Options {
            raw: RawPolicy::Inline,
            ..Options::default()
        };
        assert!(raw_sidecars(&doc, &inline).is_empty());
        let lossy = Options {
            fidelity: crate::Fidelity::Standard,
            ..Options::default()
        };
        assert!(raw_sidecars(&doc, &lossy).is_empty());
    }
}

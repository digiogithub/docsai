//! Delta emission against the inheritance chain (plan v2 Phase 11, C).
//!
//! The economy rule of spec §3.1 says a value the chain already implies is not
//! written. "The chain" is the whole OOXML cascade — document defaults, then
//! the paragraph's style, then the run's style, then direct formatting — and
//! the interesting cases are the ones the serializer used to miss: a run
//! repeating what its *paragraph's* style said, a style repeating what its
//! parent said, and a class that names what the syntax already names.
//!
//! Every rule here has to be reversible. Dropping a value is only legitimate
//! when re-reading the document yields the same one, which is why the heading
//! case is tested through a full parse.

use docsai_docmark::{parse, serialize, Options};
use docsai_model::style::{
    Align, FontProps, ParaProps, Style, StyleCatalog, StyleId, StyleType, VertAlign,
};
use docsai_model::text::{Block, Heading, Paragraph, RunProps, Section, TextDocument};
use docsai_model::units::Length;
use docsai_model::{Document, MemoryAssetStore};

fn red_18pt() -> FontProps {
    FontProps {
        color: Some("#C00000".into()),
        size: Some(Length::from_pt(18.0)),
        ..FontProps::default()
    }
}

/// A catalogue with a heading style, a default paragraph style, and a
/// two-level chain that repeats what it inherits.
fn catalog() -> StyleCatalog {
    let mut catalog = StyleCatalog::default();

    let mut normal = Style::new("Normal", StyleType::Paragraph);
    normal.is_default = true;
    catalog.insert(normal);

    let mut heading = Style::new("Heading1", StyleType::Paragraph);
    heading.based_on = Some(StyleId::new("Normal"));
    heading.paragraph.outline_level = Some(0);
    catalog.insert(heading);

    let mut parent = Style::new("Resaltado", StyleType::Paragraph);
    parent.based_on = Some(StyleId::new("Normal"));
    parent.font = red_18pt();
    parent.paragraph.align = Some(Align::Center);
    catalog.insert(parent);

    let mut child = Style::new("ResaltadoHijo", StyleType::Paragraph);
    child.based_on = Some(StyleId::new("Resaltado"));
    // Everything but `bold` is what the parent already says.
    child.font = FontProps {
        bold: Some(true),
        ..red_18pt()
    };
    child.paragraph.align = Some(Align::Center);
    catalog.insert(child);

    catalog
}

fn document(blocks: Vec<Block>) -> Document {
    Document::Text(TextDocument {
        styles: catalog(),
        sections: vec![Section {
            blocks,
            ..Default::default()
        }],
        ..Default::default()
    })
}

fn styled(style: &str, props: FontProps, text: &str) -> Paragraph {
    let mut paragraph = Paragraph::default();
    paragraph.format.style = Some(StyleId::new(style));
    paragraph.content = vec![docsai_model::text::Inline::Styled {
        content: vec![docsai_model::text::Inline::Text(text.into())],
        props: RunProps {
            style: None,
            direct: props,
        },
    }];
    paragraph
}

fn markdown(doc: &Document) -> String {
    let assets = MemoryAssetStore::new();
    serialize(doc, &assets, &Options::default()).0
}

#[test]
fn a_run_says_nothing_its_paragraph_style_already_said() {
    let doc = document(vec![Block::Paragraph(styled(
        "Resaltado",
        red_18pt(),
        "Todo esto lo dice el estilo.",
    ))]);
    let out = markdown(&doc);
    assert!(
        !out.contains("color=") && !out.contains("size="),
        "the run repeats its paragraph's style and should carry no span:\n{out}"
    );
    assert!(
        out.contains("Todo esto lo dice el estilo. {.Resaltado}"),
        "{out}"
    );
}

#[test]
fn a_real_difference_still_survives() {
    let doc = document(vec![Block::Paragraph(styled(
        "Resaltado",
        FontProps {
            vert_align: Some(VertAlign::Superscript),
            ..red_18pt()
        },
        "Distinto.",
    ))]);
    let out = markdown(&doc);
    assert!(
        out.contains(".sup"),
        "dropping what is implied must not drop what is not:\n{out}"
    );
}

#[test]
fn a_style_says_nothing_its_parent_already_said() {
    let out = markdown(&document(vec![Block::Paragraph(Paragraph::text("x"))]));
    let child: String = out
        .split("  ResaltadoHijo:\n")
        .nth(1)
        .expect("the child style is in the catalogue")
        .lines()
        .take_while(|line| line.starts_with("    "))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        child.contains("bold: true"),
        "its own delta stays:\n{child}"
    );
    assert!(
        !child.contains("#C00000") && !child.contains("18pt") && !child.contains("align"),
        "what it inherits from `Resaltado` should not be repeated:\n{child}"
    );
}

#[test]
fn the_default_paragraph_style_is_not_named() {
    let mut paragraph = Paragraph::text("Sin nada que decir.");
    paragraph.format.style = Some(StyleId::new("Normal"));
    let out = markdown(&document(vec![Block::Paragraph(paragraph)]));
    assert!(
        !out.contains("{.Normal}"),
        "the default applies wherever nothing else does:\n{out}"
    );
}

#[test]
fn a_heading_does_not_name_the_style_its_level_names() {
    let mut paragraph = Paragraph::text("Titulo");
    paragraph.format.style = Some(StyleId::new("Heading1"));
    let doc = document(vec![Block::Heading(Heading {
        id: None,
        level: 1,
        paragraph,
    })]);
    let out = markdown(&doc);
    assert!(
        !out.contains(".Heading1"),
        "the `#` already says it:\n{out}"
    );

    // …and reading it back has to give the style anyway, or the drop was a loss.
    let mut assets = MemoryAssetStore::new();
    let (back, _) = parse(&out, &mut assets).expect("parses");
    let Document::Text(text) = &back else {
        panic!("a text document")
    };
    let Some(Block::Heading(heading)) = text.sections[0].blocks.first() else {
        panic!("a heading")
    };
    assert_eq!(
        heading.paragraph.format.style,
        Some(StyleId::new("Heading1")),
        "the parser has to re-derive what the serializer left out"
    );
    assert_eq!(markdown(&back), out, "and the second pass is the first one");
}

#[test]
fn an_ambiguous_level_keeps_its_class() {
    // Two styles claiming outline level 0: the level no longer names one of
    // them, so neither may be dropped.
    let mut catalog = catalog();
    let mut other = Style::new("TituloAlternativo", StyleType::Paragraph);
    other.paragraph = ParaProps {
        outline_level: Some(0),
        ..ParaProps::default()
    };
    catalog.insert(other);

    let mut paragraph = Paragraph::text("Titulo");
    paragraph.format.style = Some(StyleId::new("Heading1"));
    let doc = Document::Text(TextDocument {
        styles: catalog,
        sections: vec![Section {
            blocks: vec![Block::Heading(Heading {
                id: None,
                level: 1,
                paragraph,
            })],
            ..Default::default()
        }],
        ..Default::default()
    });
    let out = markdown(&doc);
    assert!(
        out.contains(".Heading1"),
        "an ambiguous chain implies nothing:\n{out}"
    );
}

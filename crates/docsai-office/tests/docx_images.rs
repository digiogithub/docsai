//! The image half of the Phase 1 acceptance criteria: floating, transformed and
//! duplicated pictures must reach the IR with their geometry intact.

use docsai_model::assets::AssetStore;
use docsai_model::image::{
    AlignKeyword, Anchor, AxisPos, Flip, ImageRef, RelBase, WrapMode, WrapSide,
};
use docsai_model::text::{Block, Inline, TextDocument};
use docsai_model::units::Length;
use docsai_model::{ConversionReport, Document, MemoryAssetStore};

fn read(name: &str) -> (TextDocument, ConversionReport, MemoryAssetStore) {
    let path = format!(
        "{}/../../corpus/docx/{name}.docx",
        env!("CARGO_MANIFEST_DIR")
    );
    let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut assets = MemoryAssetStore::new();
    let (document, report) = docsai_office::read_docx(file, &mut assets).expect("reads");
    match document {
        Document::Text(doc) => (doc, report, assets),
        Document::Workbook(_) => panic!("expected a text document"),
    }
}

/// Every image in the document, in order.
fn images(doc: &TextDocument) -> Vec<&ImageRef> {
    fn from_inlines<'a>(inlines: &'a [Inline], out: &mut Vec<&'a ImageRef>) {
        for inline in inlines {
            match inline {
                Inline::Image(image) => out.push(image),
                Inline::Styled { content, .. } | Inline::Link { content, .. } => {
                    from_inlines(content, out)
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for block in doc.blocks() {
        match block {
            Block::Paragraph(p) => from_inlines(&p.content, &mut out),
            Block::Image(image) => out.push(image),
            _ => {}
        }
    }
    out
}

#[test]
fn inline_images_carry_size_alt_title_and_name() {
    let (doc, report, assets) = read("images-inline");
    let images = images(&doc);
    assert_eq!(images.len(), 3, "png, gif and emf");
    assert_eq!(report.stats.images, 3);

    let first = images[0];
    assert_eq!(first.geometry.anchor, Anchor::Inline);
    assert_eq!(first.geometry.display_size.width, Length::from_px(120.0));
    assert_eq!(first.geometry.display_size.height, Length::from_px(90.0));
    assert_eq!(first.alt, "Diagrama de ventas");
    assert_eq!(first.title.as_deref(), Some("Figura 1"));
    assert_eq!(first.name.as_deref(), Some("Diagrama"));
    assert_eq!(
        first.geometry.native_size_px,
        Some((120, 90)),
        "native size is read from the bitmap header, not from the document"
    );

    let info = assets.info(&images[1].asset).expect("gif stored");
    assert_eq!(info.content_type, "image/gif");
    assert!(info.file_name.ends_with(".gif"));
}

#[test]
fn emf_is_preserved_verbatim_with_a_warning() {
    let (doc, report, assets) = read("images-inline");
    let emf = images(&doc)
        .into_iter()
        .find(|i| {
            assets
                .info(&i.asset)
                .is_some_and(|info| info.content_type == "image/x-emf")
        })
        .expect("the EMF was stored");
    assert_eq!(emf.geometry.display_size.width, Length::from_cm(4.0));
    assert_eq!(emf.geometry.native_size_px, None, "vector, no pixel size");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.message().contains("cannot render")),
        "the user must be told EMF will not display: {:?}",
        report.warnings
    );
}

#[test]
fn floating_images_keep_position_wrap_and_z_order() {
    let (doc, _, _) = read("images-floating");
    let images = images(&doc);
    assert_eq!(images.len(), 3);

    // Anchored to the margin with explicit offsets and square wrap on the right.
    match &images[0].geometry.anchor {
        Anchor::Floating {
            relative_to_h,
            relative_to_v,
            position,
            wrap,
            wrap_side,
            behind_text,
        } => {
            assert_eq!(*relative_to_h, RelBase::Margin);
            assert_eq!(*relative_to_v, RelBase::Paragraph);
            assert_eq!(position.h, AxisPos::Offset(Length::from_cm(1.2)));
            assert_eq!(position.v, AxisPos::Offset(Length::from_cm(0.5)));
            assert_eq!(*wrap, WrapMode::Square);
            assert_eq!(*wrap_side, WrapSide::Right);
            assert!(!behind_text);
        }
        other => panic!("expected a floating anchor, got {other:?}"),
    }
    assert_eq!(images[0].geometry.z_index, Some(2));

    // Anchored to the page with symbolic alignment and top/bottom wrap.
    match &images[1].geometry.anchor {
        Anchor::Floating {
            relative_to_h,
            position,
            wrap,
            ..
        } => {
            assert_eq!(*relative_to_h, RelBase::Page);
            assert_eq!(position.h, AxisPos::Align(AlignKeyword::Center));
            assert_eq!(position.v, AxisPos::Align(AlignKeyword::Top));
            assert_eq!(*wrap, WrapMode::TopBottom);
        }
        other => panic!("expected a floating anchor, got {other:?}"),
    }

    // Behind the text: the watermark.
    assert_eq!(images[2].geometry.anchor.keyword(), "behind");
    assert_eq!(images[2].alt, "Marca de agua");
}

#[test]
fn transforms_survive_rotation_flip_crop_and_border() {
    let (doc, _, _) = read("images-transformed");
    let images = images(&doc);
    assert_eq!(images.len(), 3);

    let rotated = &images[0];
    assert!(
        (rotated.geometry.rotation_deg - 45.0).abs() < 0.01,
        "got {}",
        rotated.geometry.rotation_deg
    );

    let cropped = &images[1];
    let crop = cropped.geometry.crop.expect("srcRect became a crop");
    assert!((crop.left - 10.0).abs() < 0.01);
    assert!((crop.top - 5.0).abs() < 0.01);
    assert!((crop.right - 20.0).abs() < 0.01);
    assert_eq!(crop.bottom, 0.0);
    let border = cropped
        .geometry
        .border
        .as_ref()
        .expect("a:ln became a border");
    assert_eq!(border.width, Length::from_pt(1.0));
    assert_eq!(border.color, "#000000");
    assert_eq!(border.style, "solid");

    let flipped = &images[2];
    assert_eq!(flipped.geometry.flip, Flip::HV);
    assert_eq!(flipped.geometry.display_size.width, Length::from_cm(2.0));
    assert_eq!(
        flipped.geometry.native_size_px,
        Some((64, 64)),
        "scaled down from its native size"
    );
}

#[test]
fn one_bitmap_used_three_times_is_stored_once() {
    let (doc, report, assets) = read("images-duplicated");
    let images = images(&doc);
    assert_eq!(images.len(), 3);
    assert_eq!(report.stats.images, 3);

    let ids: std::collections::BTreeSet<_> = images.iter().map(|i| &i.asset).collect();
    assert_eq!(ids.len(), 1, "three appearances, one asset");
    assert_eq!(assets.len(), 1, "and one stored file");

    // …each keeping its own geometry.
    assert_eq!(
        images[0].geometry.display_size.width,
        Length::from_px(120.0)
    );
    assert_eq!(images[1].geometry.display_size.width, Length::from_px(60.0));
    assert!(matches!(images[2].geometry.anchor, Anchor::Floating { .. }));
}

#[test]
fn legacy_vml_pictures_are_read_into_the_same_model() {
    let (doc, report, _) = read("images-vml");
    let images = images(&doc);
    assert_eq!(images.len(), 1, "the VML shape became an image");

    let image = images[0];
    assert_eq!(image.alt, "Imagen VML heredada");
    assert_eq!(image.title.as_deref(), Some("Heredada"));
    assert_eq!(image.geometry.display_size.width, Length::from_pt(90.0));
    assert_eq!(image.geometry.display_size.height, Length::from_pt(67.5));
    match &image.geometry.anchor {
        Anchor::Floating {
            relative_to_h,
            position,
            wrap,
            wrap_side,
            ..
        } => {
            assert_eq!(*relative_to_h, RelBase::Margin);
            assert_eq!(position.h, AxisPos::Offset(Length::from_pt(10.0)));
            assert_eq!(*wrap, WrapMode::Square);
            assert_eq!(*wrap_side, WrapSide::Right);
        }
        other => panic!("expected a floating anchor, got {other:?}"),
    }
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.message().contains("legacy VML")),
        "reading VML is a degradation and must say so"
    );
}

#[test]
fn every_document_of_the_corpus_satisfies_the_ir_invariants() {
    let dir = format!("{}/../../corpus/docx", env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).expect("corpus present") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("docx") {
            continue;
        }
        let file = std::fs::File::open(&path).expect("open");
        let mut assets = MemoryAssetStore::new();
        let (document, _) = docsai_office::read_docx(file, &mut assets)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        if let Err(errors) = docsai_model::validate::validate(&document) {
            panic!("{}: {errors:?}", path.display());
        }
        checked += 1;
    }
    assert!(checked >= 14, "only {checked} documents checked");
}

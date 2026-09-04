//! Legacy binary PowerPoint (`.ppt`) — read-only (Phase 19).
//!
//! A sibling of [`crate::doc`] and [`crate::xls`], because that is where the
//! OLE2 binary formats live. Two routes reach a `.ppt`, exactly as they reach a
//! `.doc`: `docsai-convert` pre-converts it to `.pptx` with LibreOffice
//! (increment 19-B), and — when `--use-loffice never` forbids that, or no
//! LibreOffice is installed — this module reads it natively and **degraded**
//! (increment 19-D).
//!
//! What the native route recovers: the slides, in the deck's own order; the
//! text of every shape that has any, with its outline levels; and the pictures
//! of the `Pictures` stream, placed by the shape that shows them. What it does
//! not: character and paragraph formatting, layouts, masters, themes, tables,
//! charts, notes, and any shape that is neither text nor a picture. That list
//! is invariant, so the [`Warning::Degraded`] at the top of [`read`] states it
//! for every file and is never false — and nothing else here warns about a
//! thing that was in fact recovered (AGENTS.md §7 rule 3, both halves).
//!
//! Bounds checking is structural rather than sprinkled: [`window::Window`] is
//! the only way an offset becomes bytes, it has no clamping accessor at all,
//! and `#![deny(clippy::indexing_slicing)]` below makes that mechanical for
//! every submodule. `corpus/ppt/truncated.ppt` is refused at
//! [`persist::read_persist_directory`], on the offset the `Current User` stream
//! declares, with no scan fallback.
//!
//! Writing `.ppt` is out of scope for the whole of Phase 19, and there is no
//! `write` here to grow into one.

#![deny(clippy::indexing_slicing)]

mod images;
mod persist;
mod records;
mod text;
mod window;

use std::io::{Read, Seek};

use docsai_model::assets::AssetStore;
use docsai_model::image::{ImageGeometry, ImageRef};
use docsai_model::presentation::{
    PhType, Placeholder, Presentation, Shape, ShapeGeometry, ShapeKind, Slide,
};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::units::{Length, Point, Size};
use docsai_model::Document;

use crate::ReadError;

use persist::{CURRENT_USER, PICTURES, POWERPOINT_DOCUMENT};
use records::{Record, RecordIter};
use window::Window;

// [MS-PPT] record types this reader names.
const RT_DOCUMENT: u16 = 0x03E8;
const RT_DOCUMENT_ATOM: u16 = 0x03E9;
const RT_SLIDE: u16 = 0x03EE;
const RT_SLIDE_PERSIST_ATOM: u16 = 0x03F3;
const RT_DRAWING_GROUP: u16 = 0x040B;
const RT_DRAWING: u16 = 0x040C;
const RT_SLIDE_LIST_WITH_TEXT: u16 = 0x0FF0;
const RT_PLACEHOLDER_ATOM: u16 = 0x0BC3;
/// `SlideListWithTextContainer` — the slide list, in presentation order
/// ([MS-PPT] 2.4.14.3). Instance 1 is the masters and 2 the notes.
const SLIDE_LIST_INSTANCE: u16 = 0x0000;

// [MS-ODRAW] record types.
const OFFICE_ART_DG_CONTAINER: u16 = 0xF002;
const OFFICE_ART_SPGR_CONTAINER: u16 = 0xF003;
const OFFICE_ART_SP_CONTAINER: u16 = 0xF004;
const OFFICE_ART_FOPT: u16 = 0xF00B;
const OFFICE_ART_CLIENT_TEXTBOX: u16 = 0xF00D;
const OFFICE_ART_CLIENT_ANCHOR: u16 = 0xF010;
const OFFICE_ART_CLIENT_DATA: u16 = 0xF011;
/// `pib` — the one-based index of this shape's BLIP ([MS-ODRAW] 2.3.7.2).
const PROPERTY_PIB: u16 = 0x0104;

/// A master unit is 1/576 inch and an EMU is 1/914 400 inch, so the ratio is
/// 1 587.5 — not an integer, which is why the multiplication comes first.
const EMU_PER_INCH: i64 = 914_400;
const MASTER_UNITS_PER_INCH: i64 = 576;

/// How deep the shape tree may nest before the file is hostile rather than
/// merely grouped.
const MAX_SHAPE_DEPTH: u8 = 16;

/// Reads a legacy `.ppt` natively, degraded.
///
/// Always emits one [`Warning::Degraded`]: the native route is *always* less
/// than the LibreOffice one, so the warning is never false.
pub(crate) fn read<R: Read + Seek>(
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    let mut report = ConversionReport::new();
    report.warn(Warning::Degraded {
        what: "ppt".into(),
        why: "the native MS-PPT path recovers slides, shape text with its outline levels, \
              and the pictures of the Pictures stream; character and paragraph formatting, \
              layouts, masters, themes, tables, charts, speaker notes and shapes that are \
              neither text nor picture are not read. Install LibreOffice and use \
              --use-loffice auto|require for full fidelity via pptx"
            .into(),
    });

    let mut comp = crate::doc::open_cfb(reader)?;
    if !comp.is_stream(POWERPOINT_DOCUMENT) {
        return Err(ReadError::WrongShape {
            part: POWERPOINT_DOCUMENT.into(),
            expected: "a PowerPoint 97–2003 .ppt (PowerPoint Document stream)".into(),
        });
    }
    if !comp.is_stream(CURRENT_USER) {
        return Err(ReadError::MissingPart(CURRENT_USER.into()));
    }

    // The header token is read before any content, because that is the whole
    // point of it ([MS-PPT] 2.3.2).
    let current_user = crate::doc::read_stream(&mut comp, CURRENT_USER)?;
    let current = persist::read_current_user(&current_user)?;
    if current.is_encrypted() {
        // Increment 19-E owns the refusal, and it must sit *in front of* the
        // hand-off so the LibreOffice route refuses the same file — which this
        // reader cannot do from here. What it can do is refuse to read an
        // encrypted deck as if it were plain **silently**.
        report.warn(Warning::UnsupportedElement {
            kind: "encrypted .ppt (CurrentUserAtom.headerToken = 0xF3D1C4DF)".into(),
            location: CURRENT_USER.into(),
            action: "read anyway; whatever comes out is not this document's content, and \
                     refusing it is increment 19-E"
                .into(),
        });
    } else if current.header_token != persist::TOKEN_PLAIN {
        report.warn(Warning::UnsupportedElement {
            kind: format!(
                "unknown CurrentUserAtom.headerToken {:#010X}",
                current.header_token
            ),
            location: CURRENT_USER.into(),
            action: "read anyway".into(),
        });
    }

    let bytes = crate::doc::read_stream(&mut comp, POWERPOINT_DOCUMENT)?;
    let stream = Window::stream(POWERPOINT_DOCUMENT, &bytes);
    let directory = persist::read_persist_directory(&stream, &current)?;

    let document = container(
        &stream,
        directory
            .offset(directory.document_id())
            .ok_or_else(|| ReadError::WrongShape {
                part: POWERPOINT_DOCUMENT.into(),
                expected: format!(
                    "a persist directory holding the DocumentContainer id {}",
                    directory.document_id()
                ),
            })?,
        RT_DOCUMENT,
    )?;

    let pictures = if comp.is_stream(PICTURES) {
        let raw = crate::doc::read_stream(&mut comp, PICTURES)?;
        images::read_pictures(&raw)?
    } else {
        images::Pictures::default()
    };
    let blip_store = match records::find(document.body, RT_DRAWING_GROUP, None)? {
        Some(group) => images::blip_store(group.body)?,
        None => Vec::new(),
    };
    let mut deck = Presentation {
        meta: crate::doc::read_summary_meta(&mut comp),
        slide_size: slide_size(&document)?,
        ..Default::default()
    };

    let order = slide_order(&document)?;
    let mut used = Vec::new();
    for persist_id in &order {
        let offset = directory
            .offset(*persist_id)
            .ok_or_else(|| ReadError::WrongShape {
                part: POWERPOINT_DOCUMENT.into(),
                expected: format!("a persist directory entry for slide id {persist_id}"),
            })?;
        used.push(offset);
        let slide = container(&stream, offset, RT_SLIDE)?;
        deck.slides.push(read_slide(
            &slide,
            &blip_store,
            &pictures,
            assets,
            &mut report,
        )?);
    }
    report.stats.slides = report.stats.slides.saturating_add(deck.slides.len() as u32);

    // A slide the deck's own list does not name is a slide this reader did not
    // convert, and saying so is cheaper than a user noticing later.
    let orphans = orphan_slides(&stream, &directory, &used)?;
    if orphans > 0 {
        report.warn(Warning::Degraded {
            what: "ppt-slides".into(),
            why: format!(
                "{orphans} slide container(s) in the file are not listed in the deck's \
                 SlideListWithTextContainer, so their position is unknown and they are not read"
            ),
        });
    }

    warn_about_unplaced_pictures(&pictures, &blip_store, &mut report);

    Ok((Document::Presentation(deck), report))
}

/// The container at `offset`, checked to be the type the caller expects.
fn container<'a>(
    stream: &Window<'a>,
    offset: usize,
    rec_type: u16,
) -> Result<Record<'a>, ReadError> {
    let header = records::header_at(stream, offset)?;
    if header.rec_type != rec_type {
        return Err(ReadError::WrongShape {
            part: format!("{POWERPOINT_DOCUMENT}@{offset}"),
            expected: format!(
                "record type {rec_type:#06X}, found {:#06X}",
                header.rec_type
            ),
        });
    }
    Ok(Record {
        header,
        body: stream.sub(
            stream.advance(offset, records::HEADER_LEN)?,
            header.len as usize,
        )?,
        at: offset,
    })
}

/// `DocumentAtom.slideSize`, in master units.
fn slide_size(document: &Record<'_>) -> Result<Size, ReadError> {
    let Some(atom) = records::find(document.body, RT_DOCUMENT_ATOM, None)? else {
        return Ok(Size::default());
    };
    Ok(Size::new(
        master_units(atom.body.i32_at(0)?.into()),
        master_units(atom.body.i32_at(4)?.into()),
    ))
}

fn master_units(value: i64) -> Length {
    Length::from_emu(value.saturating_mul(EMU_PER_INCH) / MASTER_UNITS_PER_INCH)
}

/// The persist ids of the slides, in presentation order.
fn slide_order(document: &Record<'_>) -> Result<Vec<u32>, ReadError> {
    let Some(list) = records::find(
        document.body,
        RT_SLIDE_LIST_WITH_TEXT,
        Some(SLIDE_LIST_INSTANCE),
    )?
    else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for record in list.children() {
        let record = record?;
        if record.rec_type() == RT_SLIDE_PERSIST_ATOM {
            out.push(record.body.u32_at(0)?);
        }
    }
    Ok(out)
}

/// How many persist entries hold a `SlideContainer` the slide list did not name.
fn orphan_slides(
    stream: &Window<'_>,
    directory: &persist::PersistDirectory,
    used: &[usize],
) -> Result<usize, ReadError> {
    let mut count = 0;
    for (_, offset) in directory.entries() {
        if used.contains(&offset) {
            continue;
        }
        // Every directory offset was header-checked when the directory was
        // built, so this cannot fail on a file that got this far.
        if records::header_at(stream, offset)?.rec_type == RT_SLIDE {
            count += 1;
        }
    }
    Ok(count)
}

/// Reads one `SlideContainer` into a [`Slide`].
fn read_slide(
    slide: &Record<'_>,
    blip_store: &[usize],
    pictures: &images::Pictures,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Slide, ReadError> {
    let mut shapes = Vec::new();
    if let Some(drawing) = records::find(slide.body, RT_DRAWING, None)? {
        if let Some(dg) = records::find(drawing.body, OFFICE_ART_DG_CONTAINER, None)? {
            let mut z_index = 0u32;
            collect_shapes(
                dg.body,
                MAX_SHAPE_DEPTH,
                &mut z_index,
                blip_store,
                pictures,
                assets,
                report,
                &mut shapes,
            )?;
        }
    }
    crate::pptx::order::sort(&mut shapes);
    Ok(Slide {
        shapes,
        ..Default::default()
    })
}

/// Walks the shape tree, emitting the shapes that carry something the IR holds.
#[allow(clippy::too_many_arguments)]
fn collect_shapes(
    window: Window<'_>,
    depth: u8,
    z_index: &mut u32,
    blip_store: &[usize],
    pictures: &images::Pictures,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
    out: &mut Vec<Shape>,
) -> Result<(), ReadError> {
    if depth == 0 {
        return Ok(());
    }
    for record in RecordIter::new(window) {
        let record = record?;
        match record.rec_type() {
            OFFICE_ART_SPGR_CONTAINER => collect_shapes(
                record.body,
                depth.saturating_sub(1),
                z_index,
                blip_store,
                pictures,
                assets,
                report,
                out,
            )?,
            OFFICE_ART_SP_CONTAINER => {
                let index = *z_index;
                if let Some(shape) =
                    read_shape(&record, index, blip_store, pictures, assets, report)?
                {
                    out.push(shape);
                }
                *z_index = z_index.saturating_add(1);
            }
            _ => {}
        }
    }
    Ok(())
}

/// One `OfficeArtSpContainer`: text, or a picture, or nothing the IR holds.
fn read_shape(
    sp: &Record<'_>,
    z_index: u32,
    blip_store: &[usize],
    pictures: &images::Pictures,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Option<Shape>, ReadError> {
    let geometry = match records::find(sp.body, OFFICE_ART_CLIENT_ANCHOR, None)? {
        Some(anchor) if anchor.body.len() >= 8 => {
            // [MS-ODRAW] 2.2.16 for PowerPoint: top, left, right, bottom, in
            // master units.
            let top = i64::from(anchor.body.i16_at(0)?);
            let left = i64::from(anchor.body.i16_at(2)?);
            let right = i64::from(anchor.body.i16_at(4)?);
            let bottom = i64::from(anchor.body.i16_at(6)?);
            ShapeGeometry::at(
                Point::new(master_units(left), master_units(top)),
                Size::new(
                    master_units(right.saturating_sub(left)),
                    master_units(bottom.saturating_sub(top)),
                ),
            )
        }
        _ => ShapeGeometry::default(),
    };

    if let Some(textbox) = records::find(sp.body, OFFICE_ART_CLIENT_TEXTBOX, None)? {
        if let Some(shape_text) = text::read_client_textbox(&textbox)? {
            report.stats.paragraphs = report
                .stats
                .paragraphs
                .saturating_add(shape_text.paragraphs);
            let has_placeholder = records::find(sp.body, OFFICE_ART_CLIENT_DATA, None)?
                .map(|data| records::find(data.body, RT_PLACEHOLDER_ATOM, None))
                .transpose()?
                .flatten()
                .is_some();
            let kind = match placeholder_type(shape_text.text_type, has_placeholder) {
                Some(ph_type) => ShapeKind::Placeholder(Placeholder {
                    ph_type,
                    idx: None,
                    body: shape_text.blocks,
                    delta: Default::default(),
                }),
                None => ShapeKind::TextBox {
                    body: shape_text.blocks,
                },
            };
            return Ok(Some(Shape {
                id: None,
                name: None,
                z_index,
                geometry,
                kind,
            }));
        }
    }

    if let Some(index) = picture_index(sp)? {
        // `pib` is one-based into the drawing group's BLIP store.
        let Some(offset) = index
            .checked_sub(1)
            .and_then(|i| blip_store.get(i as usize))
            .copied()
        else {
            report.warn(Warning::AssetIssue {
                asset: format!("ppt-blip#{index}"),
                why: format!(
                    "the shape's pib names BLIP {index} but the drawing group's store holds {}",
                    blip_store.len()
                ),
            });
            return Ok(None);
        };
        let Some(payload) = images::by_offset(pictures).get(&offset).copied() else {
            report.warn(Warning::AssetIssue {
                asset: format!("ppt-blip@{offset}"),
                why: "the BLIP store points at an offset the Pictures stream did not decode \
                      into an image"
                    .into(),
            });
            return Ok(None);
        };
        let asset = assets.put(payload)?;
        report.stats.images = report.stats.images.saturating_add(1);
        let mut image = ImageRef::new(
            asset,
            ImageGeometry::inline(geometry.size.unwrap_or_default()),
        );
        if let Some(info) = assets.info(&image.asset) {
            image.geometry.native_size_px = info.native_size_px;
        }
        return Ok(Some(Shape {
            id: None,
            name: None,
            z_index,
            geometry,
            kind: ShapeKind::Picture(image),
        }));
    }

    Ok(None)
}

/// The `pib` property of a shape's `OfficeArtFOPT`, when it has one.
fn picture_index(sp: &Record<'_>) -> Result<Option<u32>, ReadError> {
    let Some(fopt) = records::find(sp.body, OFFICE_ART_FOPT, None)? else {
        return Ok(None);
    };
    // `recInstance` counts the fixed-size entries; the complex payloads that
    // may follow them are not read here.
    let mut off = 0usize;
    for _ in 0..fopt.instance() {
        let pid = fopt.body.u16_at(off)?;
        let value = fopt.body.u32_at(fopt.body.advance(off, 2)?)?;
        if pid & 0x3FFF == PROPERTY_PIB {
            return Ok(Some(value));
        }
        off = fopt.body.advance(off, 6)?;
    }
    Ok(None)
}

/// Which placeholder slot a text shape fills, if any.
///
/// `TextHeaderAtom.textType` is what the binary format states; a
/// `PlaceholderAtom` beside it says the shape really is a placeholder rather
/// than a free box that happens to be typed. `Tx_TYPE_OTHER` with no
/// `PlaceholderAtom` is a text box, which is exactly what LibreOffice's own
/// export of these decks produces on the other side.
fn placeholder_type(text_type: u32, has_placeholder: bool) -> Option<PhType> {
    match text_type {
        text::text_type::TITLE => Some(PhType::Title),
        text::text_type::CENTER_TITLE => Some(PhType::CenterTitle),
        text::text_type::BODY
        | text::text_type::CENTER_BODY
        | text::text_type::HALF_BODY
        | text::text_type::QUARTER_BODY => Some(PhType::Body),
        text::text_type::NOTES => Some(PhType::Body),
        text::text_type::OTHER if has_placeholder => Some(PhType::Body),
        _ => None,
    }
}

/// A BLIP no shape points at came back without a position, and that one really
/// is geometry lost — unlike every picture that a shape places.
fn warn_about_unplaced_pictures(
    pictures: &images::Pictures,
    blip_store: &[usize],
    report: &mut ConversionReport,
) {
    for blip in &pictures.blips {
        if !blip_store.contains(&blip.at) {
            report.warn(Warning::ImageGeometryDegraded {
                what: format!("ppt-blip@{}", blip.at),
                why: "the Pictures stream holds this BLIP but no shape in the deck refers to \
                      it, so it has no position and no size and is not placed on a slide"
                    .into(),
            });
        }
    }
    for undecoded in &pictures.undecoded {
        report.warn(Warning::AssetIssue {
            asset: format!("ppt-blip@{}", undecoded.at),
            why: format!(
                "the BLIP is a {} the native path does not decode into a file",
                undecoded.kind
            ),
        });
    }
}

#[cfg(test)]
// The module-wide `deny(indexing_slicing)` is about the reader, not about
// its tests: a panicking index in a test is a failing test, which is the
// point. Assertions below index freely.
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;
    use docsai_model::MemoryAssetStore;
    use std::io::Cursor;

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/ppt")
            .join(name)
    }

    fn read_fixture(name: &str) -> Result<(Presentation, ConversionReport), ReadError> {
        let mut assets = MemoryAssetStore::new();
        let bytes = std::fs::read(fixture(name)).unwrap();
        let (document, report) = read(Cursor::new(bytes), &mut assets)?;
        match document {
            Document::Presentation(deck) => Ok((deck, report)),
            other => panic!("not a presentation: {other:?}"),
        }
    }

    #[test]
    fn the_route_is_not_a_zip_and_says_so_rather_than_panicking() {
        let mut assets = MemoryAssetStore::new();
        assert!(read(Cursor::new(b"not a compound file".to_vec()), &mut assets).is_err());
    }

    #[test]
    fn basic_slides_comes_back_whole() {
        let (deck, report) = read_fixture("basic-slides.ppt").unwrap();
        assert_eq!(deck.slides.len(), 2);
        assert_eq!(
            deck.slides[0].title().as_deref(),
            Some("Informe trimestral")
        );
        assert_eq!(deck.slides[1].title().as_deref(), Some("Siguientes pasos"));
        assert_eq!(deck.slide_size.width.emu(), 9_144_000);
        assert_eq!(deck.slide_size.height.emu(), 6_858_000);
        assert_eq!(report.stats.slides, 2);
    }

    #[test]
    fn the_only_warning_on_an_ordinary_deck_is_the_route_itself() {
        // §7 rule 3's second half: nothing warns about a loss that did not
        // happen. `basic-slides.ppt` has no pictures, no orphan slides and no
        // encryption, so the invariant Degraded is the whole report.
        let (_, report) = read_fixture("basic-slides.ppt").unwrap();
        assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
        assert!(matches!(
            report.warnings.first(),
            Some(Warning::Degraded { what, .. }) if what == "ppt"
        ));
    }

    #[test]
    fn mixed_media_keeps_its_bullet_levels_and_places_its_picture() {
        let (deck, report) = read_fixture("mixed-media.ppt").unwrap();
        assert_eq!(deck.slides.len(), 3);
        let titles: Vec<Option<String>> = deck.slides.iter().map(|s| s.title()).collect();
        assert_eq!(
            titles,
            vec![
                Some("Legado en binario".to_string()),
                Some("Segunda diapositiva".to_string()),
                Some("Con imagen".to_string()),
            ]
        );
        assert_eq!(report.stats.images, 1);

        // The picture is a shape with a position, not a loose asset.
        let picture = deck.slides[2]
            .shapes
            .iter()
            .find(|s| matches!(s.kind, ShapeKind::Picture(_)))
            .expect("a picture shape");
        assert!(picture.geometry.pos.is_some());
        assert!(picture.geometry.size.is_some());

        // …so nothing claims its geometry was degraded.
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| matches!(w, Warning::ImageGeometryDegraded { .. })),
            "{:?}",
            report.warnings
        );
    }

    #[test]
    fn the_truncated_fixture_is_refused_and_the_refusal_says_where_and_why() {
        let error = read_fixture("truncated.ppt").unwrap_err();
        match &error {
            ReadError::Truncated {
                part,
                at,
                need,
                have,
            } => {
                assert_eq!(part, POWERPOINT_DOCUMENT);
                // `CurrentUserAtom.offsetToCurrentEdit` says 10 946; the stream
                // is 5 632 bytes, so the UserEditAtom header is 5 314 bytes
                // past the end.
                assert_eq!(*at, 10_946);
                assert_eq!(*need, records::HEADER_LEN);
                assert_eq!(*have, 5_632);
            }
            other => panic!("{other:?}"),
        }
        let message = error.to_string();
        assert!(message.contains("10946"), "{message}");
        assert!(message.contains("5632"), "{message}");
    }

    #[test]
    fn a_naive_reclen_walk_of_the_truncated_stream_is_refused_too() {
        // The fixture carries two traps and the reader above only trips the
        // first. This is the second, asserted directly: the header at 5 300
        // declares a body ending at 7 164 in a 5 632-byte stream, so the walk
        // refuses from the declared length alone.
        let bytes = std::fs::read(fixture("truncated.ppt")).unwrap();
        let mut comp = crate::doc::open_cfb(Cursor::new(bytes)).unwrap();
        let stream = crate::doc::read_stream(&mut comp, POWERPOINT_DOCUMENT).unwrap();
        assert_eq!(stream.len(), 5_632);
        let window = Window::stream(POWERPOINT_DOCUMENT, &stream);
        let error = RecordIter::new(window)
            .find_map(|record| record.err())
            .expect("the walk must not run off the end quietly");
        match error {
            ReadError::Truncated { at, need, have, .. } => {
                assert_eq!(at, 5_308, "just past the header at 5 300");
                assert_eq!(at + need, 7_164, "where the declared body ends");
                assert_eq!(have, 5_632);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_encrypted_fixture_is_read_but_never_silently() {
        // Increment 19-E owns the refusal, and it belongs in front of both
        // routes rather than here. What 19-D owes is that the file is not read
        // *as if it were plain*.
        let (_, report) = read_fixture("encrypted.ppt").unwrap();
        let flagged = report.warnings.iter().any(
            |w| matches!(w, Warning::UnsupportedElement { kind, .. } if kind.contains("encrypted")),
        );
        assert!(flagged, "{:?}", report.warnings);
    }
}

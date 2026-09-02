//! The `.pptx` writer (Phase 15, increments 15-A to 15-F).
//!
//! The reader's counterpart, and the place where spike P3's rule is enforced
//! rather than restated:
//!
//! > **Preserve everything; rebuilding is the exception.**
//!
//! A deck read by [`super::read_pptx`] carries its whole original package in
//! the [`AssetStore`], opaque and byte for byte ([`super::skeleton`]). This
//! writer puts that package back and substitutes only the parts
//! [`SkeletonRef::rebuilt_parts`] licenses it to regenerate. Everything
//! else is copied: the theme, the masters, `tableStyles.xml`, the embedded
//! workbook of a chart, the four `dgm:` parts of a diagram, and the parts
//! nobody has looked at yet.
//!
//! **The archive is part of the fidelity.** A [`Package`](crate::package::Package)
//! is a `BTreeMap`, so a package rebuilt through it comes out alphabetised,
//! recompressed and re-timestamped even when every byte of every part is
//! identical. That is a different file, and a different file is a file
//! PowerPoint gets to have an opinion about. So the re-emission here works on
//! the ZIP itself: entry by entry, in the original order, copying the *stored*
//! bytes without a decompress/recompress cycle, keeping each entry's own
//! compression method, CRC and timestamp.
//!
//! Since 15-B the **text bodies** of a slide are rebuilt from the IR and spliced
//! back into the part they came from, which is spike P3's `rewrite` mode taken
//! one level further in: a placeholder's `a:p` sequence comes from the model,
//! and its `p:spPr`, its `a:bodyPr`, the shapes around it and the rest of the
//! part are the bytes that were read. A deck nobody edited therefore comes out
//! byte for byte as it went in — the rewriter reproduces what was already
//! there — while a retitled slide differs in exactly the run that was retyped.
//!
//! Since 15-C the same splice reaches **what a text body is not**: a picture's
//! image, alt text, drawn size and crop; the text of a table's cells; and the
//! hyperlinks a run carries. Those three need something the part alone cannot
//! give — a relationship id, a media part, a content type — so this module also
//! writes the *neighbouring* parts, and only the ones an edit actually moved:
//! a slide's `_rels` gains the relationships that were not in it, spliced in
//! before its closing tag rather than regenerated, and `[Content_Types].xml`
//! gains a `Default` for an extension the deck had never carried. Media bytes
//! go in **as they came out of the store**: the asset is the file that was
//! read, and nothing here re-encodes a bitmap.
//!
//! Since 15-D a **preserved object** — SmartArt, OLE, a group, a custom
//! geometry, an animation timeline — is not a loss at all: the splice leaves
//! its markup where the reader found it, so it goes out as the original XML at
//! its original z-order, and a payload the model carries that differs from the
//! package's is written back over it. A fragment in another dialect is dropped
//! with `Warning::RawBlockDropped` rather than emitted as nonsense.
//!
//! Since 15-E the writer reaches the two parts that are the *deck's* rather
//! than a slide's. A **notes page** is rewritten like any other text body,
//! through the slide's own relationships and never by number; and when the
//! model's slides are not the package's — one removed, or the order changed —
//! `p:sldIdLst` and `p14:sectionLst` are restated, the removed slide's parts
//! leave the archive with it, and `ppt/presentation.xml` is otherwise copied.
//! Which package slide a model slide *is* has to be recovered from the content
//! (see [`align`]): nothing in this chain carries an identity for a slide.
//!
//! Since 15-F the one thing this writer deliberately does **not** preserve is
//! the autofit scale of a text body whose text changed. `a:normAutofit@fontScale`
//! is a number PowerPoint computed for the text that was in the box when a
//! human last edited it; left over text that is no longer there it is a lie no
//! reader can detect, so it is dropped and `Warning::AutofitStale` names the
//! shape (analysis §5.4). «Changed» is the writer's own notion of change and
//! not a second one: the body's new bytes differ from the bytes the part
//! already said. A shape nobody edited keeps its scale and is not reported.
//!
//! What the writer cannot yet put back it **keeps and reports**: a chart's
//! data (Phase 16), a field (whose GUID the IR does not record), a table whose
//! grid changed, a slide *added* or a notes page the deck does not have (both
//! 15-G, which writes a part rather than into one). Each one is a `Degraded`
//! warning naming what was copied and the increment that lifts it, because a
//! caller who edited the model and got the original back has to be able to
//! learn that from the report rather than from the file (`AGENTS.md` §7 rule
//! 3) — and, just as much, a loss that did *not* happen must not be reported
//! as one.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Seek, Write};
use std::ops::Range;

use docsai_model::assets::AssetStore;
use docsai_model::image::{ImageGeometry, ImageRef};
use docsai_model::presentation::{
    PhType, Placeholder, Presentation, RawShape, ShapeKind, SkeletonRef, Slide,
};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::text::{RawFragment, Table};
use docsai_model::units::{Length, Size};
use docsai_model::Document;

use crate::package::{rels_part_name, Package, Relationships};
use crate::write_error::WriteError;
use crate::xml::Element;

use super::write_gen;
use super::write_text::{self, Links};

/// The relationship type of an embedded image, as the `_rels` part spells it.
const IMAGE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
/// The relationship type of a hyperlink.
const HYPERLINK_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
/// The relationship type of a slide, as `ppt/_rels/presentation.xml.rels` states it.
const SLIDE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
/// The relationship type of a slide layout, from the slide that uses it.
const LAYOUT_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
/// The relationship type of a notes page, from the slide it belongs to.
const NOTES_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide";
/// The relationship type of the notes master a notes page hangs from.
const NOTES_MASTER_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster";

/// The package a deck with no preserved one is written on (15-G).
///
/// Generated by `corpus/generate.py` (`office_default_template`) from the XML
/// in that file, under this repository's own licence: there is no binary of
/// unknown provenance in this tree, and `corpus/generate.py --check` fails if
/// the file drifts from the source that states it.
const TEMPLATE: &[u8] = include_bytes!("../../assets/default-template.pptx");

/// Writes a presentation as a `.pptx` package.
pub fn write_pptx<W: Write + Seek>(
    document: &Document,
    assets: &dyn AssetStore,
    writer: W,
) -> Result<ConversionReport, WriteError> {
    let Document::Presentation(deck) = document else {
        return Err(WriteError::Invalid(format!(
            "cannot write {} as .pptx",
            document.shape_name()
        )));
    };
    if deck.skeleton.is_none() && deck.slides.is_empty() {
        // The template on its own is not a deck: a package whose `p:sldIdLst`
        // is empty is not one PowerPoint opens, and writing one would answer a
        // caller's mistake with a file that looks like a success.
        return Err(WriteError::Invalid(
            "this deck carries no preserved package and no slides either, and a \
             package with no slides in it is not a deck"
                .into(),
        ));
    }
    let mut report = ConversionReport::new();
    let skeleton = skeleton_bytes(deck, assets, &mut report)?;
    // A slide the package does not have is written from the model rather than
    // spliced into a part (15-G). Seeding produces the package the rest of the
    // writer then works on, so a generated slide reaches the archive through
    // exactly the same door a preserved one does.
    let seeded = match seed(skeleton, deck, assets, &mut report) {
        Ok(seeded) => seeded,
        Err(reason) => {
            if deck.skeleton.is_none() {
                // There is no package to fall back on: writing the template
                // unchanged would be a file with no slides in it at all.
                return Err(WriteError::Invalid(format!(
                    "this deck carries no preserved package and one could not be \
                     generated from the embedded template: {reason}"
                )));
            }
            report.warn(Warning::Degraded {
                what: "the slides the model added".into(),
                why: format!("{reason}, so the preserved package was copied unchanged"),
            });
            None
        }
    };
    let (skeleton, generated) = match &seeded {
        Some(Seeded { package, generated }) => (package.as_slice(), generated.clone()),
        None => (skeleton, BTreeSet::new()),
    };
    let rebuild = rebuild_slides(deck, skeleton, assets, &generated, &mut report);

    reemit(skeleton, &rebuild, writer)?;
    Ok(report)
}

/// The bytes of the preserved package, or of the embedded template.
///
/// A deck read from a `.pptx` carries its own package and everything this
/// writer does is a splice into it. A deck typed as attribute-free Markdown
/// carries none, and since 15-G it gets the template compiled into this binary:
/// a package with a master, four layouts, a notes master and a theme, and **no
/// slides at all**, because a slide is what the writer generates.
///
/// Using it is not a silent substitution. A deck that came through DocMark
/// without its `skeleton:` file beside it looks exactly like a deck typed by
/// hand from here, so the warning states what the written deck's design now is
/// rather than guessing which of the two happened.
fn skeleton_bytes<'a>(
    deck: &Presentation,
    assets: &'a dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<&'a [u8], WriteError> {
    let Some(SkeletonRef { asset, .. }) = deck.skeleton.as_ref() else {
        report.warn(Warning::Degraded {
            what: "the deck's design".into(),
            why: "this deck carries no preserved package, so it was written on the \
                  embedded default template: its theme, its master and its layouts \
                  are docsai's, and each slide was given a layout chosen from the \
                  shape of its content"
                .into(),
        });
        return Ok(TEMPLATE);
    };
    assets.get(asset).ok_or_else(|| {
        WriteError::Invalid(format!(
            "the deck names its preserved package as `{}` and the asset store \
             does not hold it",
            asset.as_str()
        ))
    })
}

/// The package with the slides the model added in it, and which parts they are.
struct Seeded {
    package: Vec<u8>,
    /// The slide parts written from the model. The rewriting pass leaves them
    /// alone: they already say exactly what the model says, and running the
    /// splice over them would report a preserved package that never existed.
    generated: BTreeSet<String>,
}

/// Writes the slide parts the package does not have, and the parts that have to
/// agree with them.
///
/// This is the increment 15-E refused by name: a slide the model **added** has
/// no part to splice into. It is also, and by the same code, the whole of a
/// deck typed as attribute-free Markdown, which arrives here on the embedded
/// template — a package whose `p:sldIdLst` is empty, so that every one of its
/// slides is a slide the package does not have.
///
/// Nothing is regenerated: the parts written here are **new**, and every part
/// the package already carries is spliced (`p:sldIdLst`, the deck's
/// relationships, `[Content_Types].xml`) or left exactly as it was. A deck
/// whose model matches its package produces `Ok(None)` and never reaches a
/// single line of this, which is what keeps the byte-identity of 15-A intact.
fn seed(
    skeleton: &[u8],
    deck: &Presentation,
    assets: &dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Option<Seeded>, String> {
    let Ok(package) = Package::open(Cursor::new(skeleton)) else {
        // Unreadable: the rewriting pass says so, with the part it failed on.
        return Ok(None);
    };
    let Some(parts) = deck_parts(&package) else {
        return Ok(None);
    };

    // Which model slides the package already states, by [`align`]'s own first
    // two passes — so that the pairing this decides and the one `align` decides
    // afterwards, over the seeded package, are the same pairing.
    let claimed = claims(&parts.entries, &deck.slides);
    let free: Vec<usize> = (0..parts.entries.len())
        .filter(|j| !claimed.contains(&Some(*j)))
        .collect();
    let pending: Vec<usize> = (0..deck.slides.len())
        .filter(|i| claimed[*i].is_none())
        .collect();
    let mut map = claimed;
    for (&slot, &entry) in pending.iter().zip(&free) {
        map[slot] = Some(entry);
    }
    let added: Vec<usize> = pending.get(free.len()..).unwrap_or_default().to_vec();
    // A slide the package **has** can still need a part it does not: a note
    // typed onto a slide that never had a notes page. It is the same work as an
    // added slide's — a part of its own, hanging from the notes master — so it
    // is done here rather than refused, which is what 15-E pointed at 15-G for.
    let noted: Vec<usize> = map
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| {
            let entry = (*slot)?;
            let wanted = deck.slides[index].notes.is_some();
            let has =
                super::notes::part(&package.relationships(&parts.entries[entry].part)).is_some();
            (wanted && !has).then_some(index)
        })
        .collect();
    if added.is_empty() && noted.is_empty() {
        // Every model slide has a part, and every note has a page: an unedited
        // deck, an edited one, a reorder, a removal. All of them are the
        // splice's, not this.
        return Ok(None);
    }

    let layouts = write_gen::layouts_of(&package);
    if layouts.is_empty() && !added.is_empty() {
        return Err(
            "the package declares no slide layout for a new slide to be \
                    written against"
                .into(),
        );
    }
    let canvas = canvas_of(&parts.root);
    let notes_master = package
        .relationships(&parts.main)
        .of_kind("notesMaster")
        .find(|rel| !rel.external)
        .map(|rel| rel.target.clone());

    let mut media = Media::new(&package, assets);
    let mut main_rels = PartRels::new(&package, &parts.main);
    let mut taken: BTreeSet<String> = package.part_names().map(str::to_string).collect();
    let mut written: Vec<(String, Vec<u8>)> = Vec::new();
    let mut overrides: BTreeMap<String, String> = BTreeMap::new();
    let mut generated = BTreeSet::new();
    let mut entries: BTreeMap<usize, String> = BTreeMap::new();
    let next_id = parts
        .entries
        .iter()
        .map(|entry| entry.id)
        .max()
        .unwrap_or(255)
        + 1;
    let prefix = list_prefix(&parts.root);

    for (offset, &index) in added.iter().enumerate() {
        let next_id = next_id + offset as i64;
        let slide = &deck.slides[index];
        let label = slide_label(index, slide);
        let choice = write_gen::choose(&layouts, slide)
            .ok_or_else(|| format!("{label} has no layout in the package to be written against"))?;
        if let Some(why) = choice.unmatched {
            report.warn(Warning::Degraded {
                what: label.clone(),
                why,
            });
        }
        let layout = &layouts[choice.layout];

        let part = free_part(&taken, "ppt/slides", "slide");
        taken.insert(part.clone());
        // A notes page is a part of its own hanging from a notes master, and a
        // deck with no notes master has nowhere to hang one from.
        let notes_part = match (&slide.notes, &notes_master) {
            (Some(_), Some(_)) => {
                let name = free_part(&taken, "ppt/notesSlides", "notesSlide");
                taken.insert(name.clone());
                Some(name)
            }
            (Some(_), None) => {
                report.warn(Warning::Degraded {
                    what: label.clone(),
                    why: "its speaker notes, which need a notes page hanging from a notes \
                          master the package does not have"
                        .into(),
                });
                None
            }
            (None, _) => None,
        };

        let mut kept = Vec::new();
        let (xml, rels) = {
            let mut new = NewParts {
                rels: PartRels::new(&package, &part),
                media: &mut media,
            };
            new.rels.relate(LAYOUT_REL, &layout.part, false);
            if let Some(notes) = &notes_part {
                new.rels.relate(NOTES_REL, notes, false);
            }
            let xml = write_gen::slide_xml(slide, layout, canvas, &mut new, &mut kept)
                .map_err(|reason| format!("{label} holds {reason}"))?;
            (xml, new.rels.rewritten())
        };
        for reason in kept {
            report.warn(Warning::Degraded {
                what: label.clone(),
                why: reason,
            });
        }
        generated.insert(part.clone());
        overrides.insert(
            part.clone(),
            write_gen::content_type_of(&part).unwrap_or_default(),
        );
        written.push((part.clone(), xml.into_bytes()));
        if let Some((name, xml)) = rels {
            written.push((name, xml.into_bytes()));
        }

        if let (Some(notes), Some(master), Some(blocks)) =
            (&notes_part, &notes_master, &slide.notes)
        {
            let mut new = NewParts {
                rels: PartRels::new(&package, notes),
                media: &mut media,
            };
            new.rels.relate(NOTES_MASTER_REL, master, false);
            new.rels.relate(SLIDE_REL, &part, false);
            let xml = write_gen::notes_xml(blocks, &mut new)
                .map_err(|reason| format!("the speaker notes of {label} hold {reason}"))?;
            overrides.insert(
                notes.clone(),
                write_gen::content_type_of(notes).unwrap_or_default(),
            );
            written.push((notes.clone(), xml.into_bytes()));
            if let Some((name, xml)) = new.rels.rewritten() {
                written.push((name, xml.into_bytes()));
            }
        }

        let id = main_rels.relate(SLIDE_REL, &part, false);
        entries.insert(
            index,
            format!(
                "<{prefix}sldId id=\"{next_id}\" r:id=\"{}\"/>",
                esc_attr(&id)
            ),
        );
    }

    // The notes pages of slides that already have a part. The slide's own
    // `_rels` gains one relationship and keeps every byte it had; the page is
    // then written like any other, by the rewriting pass, through the door the
    // reader goes through.
    for &index in &noted {
        let slide = &deck.slides[index];
        let label = slide_label(index, slide);
        let part = parts.entries[map[index].expect("a slide with a part")]
            .part
            .clone();
        let (Some(master), Some(blocks)) = (&notes_master, &slide.notes) else {
            report.warn(Warning::Degraded {
                what: label,
                why: "its speaker notes, which need a notes page hanging from a notes \
                      master the package does not have"
                    .into(),
            });
            continue;
        };
        let notes = free_part(&taken, "ppt/notesSlides", "notesSlide");
        taken.insert(notes.clone());
        let mut new = NewParts {
            rels: PartRels::new(&package, &notes),
            media: &mut media,
        };
        new.rels.relate(NOTES_MASTER_REL, master, false);
        new.rels.relate(SLIDE_REL, &part, false);
        let xml = write_gen::notes_xml(blocks, &mut new)
            .map_err(|reason| format!("the speaker notes of {label} hold {reason}"))?;
        written.push((notes.clone(), xml.into_bytes()));
        if let Some((name, xml)) = new.rels.rewritten() {
            written.push((name, xml.into_bytes()));
        }
        overrides.insert(
            notes.clone(),
            write_gen::content_type_of(&notes).unwrap_or_default(),
        );
        // The slide points at its page, spliced into the relationships it had.
        let mut slide_rels = PartRels::new(&package, &part);
        slide_rels.relate(NOTES_REL, &notes, false);
        if let Some((name, xml)) = slide_rels.rewritten() {
            written.push((name, xml.into_bytes()));
        }
    }

    // `p:sldIdLst` in the model's order: the entries the deck already had, byte
    // for byte, and one new entry per slide that was written.
    if !added.is_empty() {
        let list = parts
            .root
            .child("sldIdLst")
            .ok_or_else(|| format!("`{}` states no slide list to add a slide to", parts.main))?;
        let mut wanted = String::new();
        for (index, slot) in map.iter().enumerate() {
            match slot {
                Some(entry) => wanted.push_str(&parts.source[parts.entries[*entry].span.clone()]),
                None => wanted.push_str(entries.get(&index).ok_or("a slide with no entry")?),
            }
        }
        let stated: Vec<&Element> = list.children_named("sldId").collect();
        let edit = match (stated.first(), stated.last()) {
            (Some(first), Some(last)) => (first.span.start..last.span.end, wanted),
            // An empty `p:sldIdLst` — which is what the embedded template carries —
            // is replaced whole, because there is no child span to write between.
            _ => (
                list.span.clone(),
                format!("<{prefix}sldIdLst>{wanted}</{prefix}sldIdLst>"),
            ),
        };
        written.push((
            parts.main.clone(),
            splice(&parts.source, vec![edit]).into_bytes(),
        ));
        if let Some((name, xml)) = main_rels.rewritten() {
            written.push((name, xml.into_bytes()));
        }
    }

    let mut rebuild = Rebuild::default();
    for (name, bytes) in written {
        rebuild.put(&package, name, bytes);
    }
    for (name, bytes) in std::mem::take(&mut media.added) {
        rebuild.additions.insert(name, bytes);
    }
    if let Some((name, xml)) =
        content_types(&package, &media.extensions, &overrides, &BTreeSet::new())
    {
        rebuild.put(&package, name, xml.into_bytes());
    }

    let mut out = Cursor::new(Vec::new());
    reemit(skeleton, &rebuild, &mut out).map_err(|error| {
        format!("the package with the new slides could not be written ({error})")
    })?;
    Ok(Some(Seeded {
        package: out.into_inner(),
        generated,
    }))
}

/// The relationships and media parts a generated slide needs, as the generator
/// asks for them.
struct NewParts<'a, 'm> {
    rels: PartRels,
    media: &'a mut Media<'m>,
}

impl Links for NewParts<'_, '_> {
    fn hyperlink(&mut self, target: &str) -> Option<String> {
        self.rels.hyperlink(target)
    }
}

impl write_gen::Parts for NewParts<'_, '_> {
    fn image(&mut self, image: &ImageRef) -> Result<String, String> {
        let part = self.media.part_for(image)?;
        Ok(self.rels.relate(IMAGE_REL, &part, false))
    }
}

/// A part name of the shape `dir/stemN.xml` the package does not already use.
fn free_part(taken: &BTreeSet<String>, dir: &str, stem: &str) -> String {
    let mut n = 1;
    loop {
        let candidate = format!("{dir}/{stem}{n}.xml");
        if !taken.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// `p:sldSz`, which is the canvas a generated shape is placed on.
fn canvas_of(root: &Element) -> Size {
    let (width, height) = root
        .child("sldSz")
        .and_then(|size| Some((size.attr_i64("cx")?, size.attr_i64("cy")?)))
        .filter(|(cx, cy)| *cx > 0 && *cy > 0)
        .unwrap_or(super::DEFAULT_SLIDE_SIZE);
    Size::new(Length::from_emu(width), Length::from_emu(height))
}

/// The namespace prefix the deck writes its own elements with, read off the
/// part rather than assumed: `p` is a convention, not a rule.
fn list_prefix(root: &Element) -> String {
    let element = root.child("sldIdLst").unwrap_or(root);
    if element.prefix.is_empty() {
        String::new()
    } else {
        format!("{}:", element.prefix)
    }
}

/// The slide parts this deck's model can regenerate, as new part bytes.
///
/// The unit of rewriting is a **text body**, not a slide and not a package. The
/// part is otherwise the one that was read: its shape tree keeps its `p:spPr`,
/// its `a:bodyPr`, its transition and its timing, and a picture, a table or a
/// stub keeps its markup byte for byte. What changes is the `a:p` sequence
/// inside a placeholder or a text box, which is the content DocMark can hold
/// and therefore the content that can have been edited.
///
/// That is spike P3's `rewrite` mode rather than its `naive` one, applied one
/// level further in: preserve everything, and rebuild only what the IR models.
///
/// Two parts that are not a slide come out of here too, both of them 15-E: a
/// slide's **notes page**, rewritten the same way and reached the same way the
/// reader reached it; and the **deck's own part**, edited — not rebuilt — in
/// the two places that state which slides the deck has and what order they are
/// in. The order of the work is deliberate: a slide list that cannot be
/// restated stops the slides from being rewritten at all, because the model's
/// third slide written into a part the file still calls the fourth is the one
/// corruption a warning cannot repair.
fn rebuild_slides(
    deck: &Presentation,
    skeleton: &[u8],
    assets: &dyn AssetStore,
    generated: &BTreeSet<String>,
    report: &mut ConversionReport,
) -> Rebuild {
    let mut rebuild = Rebuild::default();
    let package = match Package::open(Cursor::new(skeleton)) {
        Ok(package) => package,
        Err(error) => {
            report.warn(Warning::Degraded {
                what: "every slide of the written deck".into(),
                why: format!(
                    "the preserved package could not be read back ({error}), so it \
                     was copied unchanged and no edit made to the model is in it"
                ),
            });
            return rebuild;
        }
    };

    let parts = match deck_parts(&package) {
        Some(parts) => parts,
        None => {
            report.warn(Warning::Degraded {
                what: "every slide of the written deck".into(),
                why: "the preserved package does not name its slides in a way this \
                      writer can follow, so it was copied unchanged"
                    .into(),
            });
            return rebuild;
        }
    };

    // Which package slide each model slide *is*. Neither the deck nor DocMark
    // carries an identity for a slide, so it is recovered from the content —
    // see [`align`], which is also where a slide the model added is refused.
    let Alignment { map, guessed } = match align(&parts.entries, &deck.slides) {
        Ok(alignment) => alignment,
        Err(reason) => {
            report.warn(Warning::Degraded {
                what: "every slide of the written deck".into(),
                why: format!("{reason}, so the package was copied unchanged"),
            });
            return rebuild;
        }
    };
    // Two or more slides that were edited *and* moved leave `align` nothing to
    // match on, and the parts they get are the ones left over in order. The
    // deck is still written, and written in the model's order — refusing an
    // ordinary two-slide edit would cost more than the risk — but the pairing
    // is a guess and the person who made the edit is the only one who can tell
    // whether it is the right one, so it is named rather than assumed.
    if !guessed.is_empty() {
        let paired: Vec<String> = guessed
            .iter()
            .map(|&i| {
                format!(
                    "{} into `{}`",
                    slide_label(i, &deck.slides[i]),
                    parts.entries[map[i]].part
                )
            })
            .collect();
        report.warn(Warning::Degraded {
            what: format!("the part alignment of {} slides", guessed.len()),
            why: format!(
                "each of them was edited and none of them is where the package still \
                 says it is, so which package part a slide *is* could not be recovered \
                 from what it says; they were paired with the parts left over, in order \
                 — {} — and wherever that order is not the one the edit meant, the text \
                 still lands but the slide takes over another one's layout, its \
                 relationships and its notes page",
                paired.join(", ")
            ),
        });
    }

    // The deck's own part comes first: a slide list that cannot be restated is
    // a deck whose slides must not be rewritten either, because writing the
    // model's third slide into a part the file still calls the fourth is the
    // corruption this ordering exists to prevent.
    let restated = match restate_deck(&package, &parts, deck, &map) {
        Ok(restated) => restated,
        Err(reason) => {
            report.warn(Warning::Degraded {
                what: "the deck's slide list".into(),
                why: format!("{reason}, so the package was copied unchanged"),
            });
            return rebuild;
        }
    };
    for reason in &restated.kept {
        report.warn(Warning::Degraded {
            what: "the deck's slide list".into(),
            why: format!("{reason}: the preserved package's own answer stands"),
        });
    }
    for (name, bytes) in restated.parts {
        rebuild.put(&package, name, bytes);
    }
    rebuild.removals = restated.removed;

    let mut media = Media::new(&package, assets);
    // The payloads are one deck-wide list and the stubs point into it by id
    // ([[43-phase-13-raw]] decision 3), so the writer indexes it once.
    let fragments: BTreeMap<&str, &RawFragment> = deck
        .raw
        .iter()
        .map(|fragment| (fragment.id.as_str(), fragment))
        .collect();

    for (index, (&entry, slide)) in map.iter().zip(&deck.slides).enumerate() {
        let number = index + 1;
        let part = &parts.entries[entry].part;
        // A part [`seed`] wrote from this very model needs no rewriting: it
        // already says what the model says, and splicing into it would report
        // a preserved package that never existed.
        if generated.contains(part) {
            continue;
        }
        let Some(Parsed { source, root }) = &parts.entries[entry].parsed else {
            report.warn(Warning::Degraded {
                what: format!("slide {number}"),
                why: format!("`{part}` is not readable as XML, so it was copied unchanged"),
            });
            continue;
        };
        let source = source.as_str();
        // The notes page is a part of its own, reached through the slide's own
        // relationships — the same door the reader went through.
        match rewrite_notes(&package, part, slide, &mut rebuild) {
            Ok(stale) => {
                for (what, scale) in stale {
                    report.warn(Warning::AutofitStale { what, scale });
                }
            }
            Err(reason) => report.warn(Warning::Degraded {
                what: format!("slide {number}"),
                why: format!(
                    "{reason}: it was copied from the preserved package, so an edit \
                     to it in the model is not in the file"
                ),
            }),
        }
        let mut rels = PartRels::new(&package, part);
        let mut outside = Outside {
            part,
            package: &package,
            rels: &mut rels,
            media: &mut media,
            fragments: &fragments,
        };
        match rewrite_slide(root, source, slide, &mut outside) {
            Ok(Rewritten {
                xml,
                kept,
                dropped,
                stale,
            }) => {
                for (id, format) in dropped {
                    report.warn(Warning::RawBlockDropped { id, format });
                }
                for (what, scale) in stale {
                    report.warn(Warning::AutofitStale { what, scale });
                }
                for reason in kept {
                    report.warn(Warning::Degraded {
                        what: format!("slide {number}"),
                        why: format!("{reason}: it was copied from the preserved package, so an edit to it in the model is not in the file"),
                    });
                }
                if xml != source {
                    rebuild.replacements.insert(part.clone(), xml.into_bytes());
                }
                // The relationship part moves only when the slide gained a
                // target it did not have; an unedited slide reuses the ids it
                // was read with and leaves its `_rels` untouched.
                if let Some((name, xml)) = rels.rewritten() {
                    rebuild.put(&package, name, xml.into_bytes());
                }
            }
            Err(reason) => report.warn(Warning::Degraded {
                what: format!("slide {number}"),
                why: format!("{reason}, so the whole slide was copied unchanged"),
            }),
        }
    }

    for (name, bytes) in std::mem::take(&mut media.added) {
        rebuild.additions.insert(name, bytes);
    }
    if let Some((name, xml)) = content_types(
        &package,
        &media.extensions,
        &BTreeMap::new(),
        &rebuild.removals,
    ) {
        rebuild.put(&package, name, xml.into_bytes());
    }
    rebuild
}

/// What goes into the written archive that is not a raw copy.
///
/// Two maps rather than one, because the ZIP treats them differently: a
/// replacement keeps its member's position, compression and timestamp, and an
/// addition is a member the original package never had.
#[derive(Default)]
struct Rebuild {
    replacements: BTreeMap<String, Vec<u8>>,
    additions: BTreeMap<String, Vec<u8>>,
    /// Members the written archive does not carry at all: the parts a removed
    /// slide takes with it (15-E).
    removals: BTreeSet<String>,
}

impl Rebuild {
    /// Files a part as whichever of the two it is.
    fn put(&mut self, package: &Package, name: String, bytes: Vec<u8>) {
        if package.has_part(&name) {
            self.replacements.insert(name, bytes);
        } else {
            self.additions.insert(name, bytes);
        }
    }
}

/// The deck's own part, and the `p:sldId` entries that name its slides.
///
/// `ppt/presentation.xml` is **not** in
/// [`SkeletonRef::rebuilt_parts`](docsai_model::presentation::SkeletonRef) and
/// deliberately stays out of it: the IR does not hold that part's content, only
/// two facts stated inside it — the order of the slides and which section each
/// one is in. So it is not rebuilt, it is *edited*, in those two places and
/// nowhere else, and every other byte of it — `p:sldSz`, the master and notes-
/// master lists, the presentation-wide text styles, the custom shows — is the
/// bytes that were read.
struct DeckParts {
    /// The part's own name, `ppt/presentation.xml` by convention.
    main: String,
    /// Its text, which the two edits of this increment are spliced into.
    source: String,
    root: Element,
    /// One entry per `p:sldId` that resolves to a slide part of this package,
    /// in `p:sldIdLst` order — the reader's own rule, so the *n*th entry here
    /// is the *n*th slide the reader produced whatever the parts are named.
    entries: Vec<SlideEntry>,
    /// `p:sldId` children that resolve to nothing. A deck that carries one is
    /// already degraded (the reader warned about it), and its slide list
    /// cannot be restated without silently dropping the entry, so restating is
    /// refused rather than done wrong.
    unresolved: usize,
}

/// One `p:sldId`: what it points at, and where it is written.
struct SlideEntry {
    /// `p:sldId@id`, the deck-wide id `p14:sectionLst` refers to.
    id: i64,
    /// `p:sldId@r:id`, the relationship the part is named through.
    rel: String,
    part: String,
    /// The element's own bytes inside `ppt/presentation.xml`, so a list that
    /// was reordered is written from the entries that were read rather than
    /// from entries this writer made up.
    span: Range<usize>,
    /// The slide part itself, when it reads and parses.
    parsed: Option<Parsed>,
}

/// A part and the tree parsed out of it, kept together because every splice
/// needs both: the tree for the spans, the text for the bytes between them.
struct Parsed {
    source: String,
    root: Element,
}

fn deck_parts(package: &Package) -> Option<DeckParts> {
    let types = crate::package::ContentTypes::read(package);
    let main = super::main_part(package, &types).ok()?;
    let source = package.text(&main).ok()?.to_string();
    let root = Element::parse(&main, source.as_bytes()).ok()?;
    let rels = package.relationships(&main);

    let mut entries = Vec::new();
    let mut unresolved = 0;
    for entry in root
        .child("sldIdLst")
        .into_iter()
        .flat_map(|list| list.children_named("sldId"))
    {
        let resolved = super::rel_id(entry)
            .and_then(|id| rels.get(id).map(|rel| (id, rel)))
            .filter(|(_, rel)| !rel.external);
        let Some((rel_id, rel)) = resolved else {
            unresolved += 1;
            continue;
        };
        let parsed = package.text(&rel.target).ok().and_then(|text| {
            Element::parse(&rel.target, text.as_bytes())
                .ok()
                .map(|root| Parsed {
                    source: text.to_string(),
                    root,
                })
        });
        entries.push(SlideEntry {
            id: entry.attr_i64("id").unwrap_or_default(),
            rel: rel_id.to_string(),
            part: rel.target.clone(),
            span: entry.span.clone(),
            parsed,
        });
    }
    Some(DeckParts {
        main,
        source,
        root,
        entries,
        unresolved,
    })
}

/// What a slide is recognised by when a model and a package have to be matched.
///
/// A slide has no identity anywhere in this chain: `p:sldId@id` is the deck's
/// bookkeeping and the IR does not keep it, DocMark's `{#n7}` is assigned on
/// serialisation and the package has never heard of it. So the only witness
/// that the model's second slide is the package's fifth is what the slide
/// *says*, and this is the part of it both sides can state: the slide's name,
/// its title and how many shapes it has.
#[derive(PartialEq, Eq)]
struct Signature {
    name: Option<String>,
    title: Option<String>,
    shapes: usize,
}

fn model_signature(slide: &Slide) -> Signature {
    Signature {
        name: slide.name.clone(),
        title: slide.title().map(|title| collapse(&title)),
        shapes: slide.shapes.len(),
    }
}

fn part_signature(root: &Element) -> Option<Signature> {
    let tree = root.path(&["cSld", "spTree"])?;
    let title = tree
        .children_named("sp")
        .find(|sp| {
            sp.path(&["nvSpPr", "nvPr", "ph"])
                .is_some_and(|ph| PhType::parse(ph.attr("type").unwrap_or_default()).is_title())
        })
        .and_then(|sp| sp.child("txBody"))
        .map(|body| {
            let paragraphs: Vec<String> = body
                .children_named("p")
                .map(|p| p.deep_text())
                .filter(|text| !text.is_empty())
                .collect();
            collapse(&paragraphs.join(" "))
        })
        .filter(|title| !title.is_empty());
    Some(Signature {
        name: super::csld_name(root),
        title,
        shapes: tree
            .children()
            .filter(|child| !matches!(child.name.as_str(), "nvGrpSpPr" | "grpSpPr"))
            .count(),
    })
}

/// Runs of whitespace as one space, because the two sides count them
/// differently: a line break is a node in the IR and a `a:br` element with no
/// text in the part.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Which package slide part each model slide is written into.
///
/// Three passes, weakest claim last:
///
/// 1. a slide that is where it was and says what it said keeps its own part —
///    which is the whole corpus, and the reason an unedited deck is untouched;
/// 2. a slide the package still states verbatim *somewhere else* has moved, and
///    takes its part with it: that is what makes a reorder a reorder rather
///    than every slide from the second one on being rewritten in place;
/// 3. what is left is a slide that was edited, paired in order with the parts
///    that are left. A deck where the same edit removed one slide and retyped
///    another still lands right, because the untouched slides anchor it.
///
/// Pass 3 is forced when **one** slot reaches it — an edited slide has a
/// different signature by definition, which is why it fell through, and with a
/// single free part the pairing is the only one there is. With two or more it
/// is a guess: `[A, B]` in the package against `[B', A']` in the model, both
/// slides edited *and* reordered, matches nothing in passes 1 and 2 and pairs
/// `B'` with `A`'s part. No text is lost and the deck comes out in the model's
/// order, but each slide inherits the other's layout, its `_rels` and its notes
/// page — so those slots come back in [`Alignment::guessed`] and the caller
/// names them (rule 3: never degrade fidelity silently).
///
/// A part no model slide claims was **removed**. A model slide with no part
/// left to claim is a slide that was *added*, and since 15-G [`seed`] has
/// already written a part for it before this runs — so reaching here with more
/// slides than parts means the seeding could not be done, and the package is
/// copied rather than written half right.
fn align(entries: &[SlideEntry], slides: &[Slide]) -> Result<Alignment, String> {
    if slides.len() > entries.len() {
        return Err(format!(
            "the model holds {} slide(s) and the package {}; the parts for the \
             slides the model added could not be written",
            slides.len(),
            entries.len()
        ));
    }
    if slides.is_empty() && !entries.is_empty() {
        return Err("the model holds no slides at all, and a deck whose \
                    `p:sldIdLst` is empty is not a deck PowerPoint opens"
            .to_string());
    }

    let mut map = claims(entries, slides);
    // The order-fill, and the slots it filled: with more than one of them the
    // pairing below is the writer's guess and not a match it can point at.
    let pending: Vec<usize> = (0..slides.len()).filter(|&i| map[i].is_none()).collect();
    let free: Vec<usize> = (0..entries.len())
        .filter(|j| !map.contains(&Some(*j)))
        .collect();
    let mut free = free.into_iter();
    for &i in &pending {
        map[i] = free.next();
    }
    let map = map
        .into_iter()
        .collect::<Option<Vec<usize>>>()
        .ok_or_else(|| "the model's slides cannot be matched to the package's".to_string())?;
    let guessed = if pending.len() > 1 {
        pending
    } else {
        Vec::new()
    };
    Ok(Alignment { map, guessed })
}

/// The first two passes of [`align`]: the model slides whose part the package
/// still states, and nothing weaker than that.
///
/// Split out because [`seed`] needs the same answer before the package has
/// parts for every model slide — the slots this leaves empty are the slides
/// that were edited *and* the slides that were added, and it is the count of
/// free parts that tells the two apart.
fn claims(entries: &[SlideEntry], slides: &[Slide]) -> Vec<Option<usize>> {
    let stated: Vec<Option<Signature>> = entries
        .iter()
        .map(|entry| entry.parsed.as_ref().and_then(|p| part_signature(&p.root)))
        .collect();
    let wanted: Vec<Signature> = slides.iter().map(model_signature).collect();

    let mut taken = vec![false; entries.len()];
    let mut map: Vec<Option<usize>> = vec![None; slides.len()];
    for (i, want) in wanted.iter().enumerate() {
        if stated.get(i).and_then(Option::as_ref) == Some(want) {
            map[i] = Some(i);
            taken[i] = true;
        }
    }
    for (i, want) in wanted.iter().enumerate() {
        if map[i].is_some() {
            continue;
        }
        if let Some(j) = (0..entries.len()).find(|&j| !taken[j] && stated[j].as_ref() == Some(want))
        {
            map[i] = Some(j);
            taken[j] = true;
        }
    }
    map
}

/// Which package slide part each model slide is written into, and how sure the
/// writer is of it.
struct Alignment {
    /// One entry index into [`DeckParts::entries`] per model slide.
    map: Vec<usize>,
    /// The model slides whose part was decided by [`align`]'s third pass with
    /// more than one slot still to fill — an order-fill, not a match. Empty for
    /// every deck the first two passes settle, which is every deck the corpus
    /// has, and empty too when a single slot reaches the third pass, where the
    /// pairing is forced.
    guessed: Vec<usize>,
}

/// A model slide in words a reader of a warning can act on: its number in the
/// written deck and what it says, which is what the person editing it typed.
fn slide_label(index: usize, slide: &Slide) -> String {
    let said = slide
        .title()
        .map(|title| collapse(&title))
        .filter(|title| !title.is_empty())
        .or_else(|| slide.name.clone())
        .unwrap_or_else(|| "untitled".to_string());
    format!("slide {} (\"{said}\")", index + 1)
}

/// What restating the deck's own parts produced.
///
/// Every one of these is an edit or a removal, never a regeneration: the
/// `p:sldId` entries that stay are the bytes that were read, in the model's
/// order, and a `p14:section` keeps its name and its GUID and changes only
/// which slides it lists.
#[derive(Debug, Default)]
struct DeckRestated {
    /// Parts to write with new bytes.
    parts: Vec<(String, Vec<u8>)>,
    /// Parts the archive does not carry any more.
    removed: BTreeSet<String>,
    /// What could not be restated, in words a warning can carry.
    kept: Vec<String>,
}

fn restate_deck(
    package: &Package,
    parts: &DeckParts,
    deck: &Presentation,
    map: &[usize],
) -> Result<DeckRestated, String> {
    let mut restated = DeckRestated::default();
    let removed: Vec<usize> = (0..parts.entries.len())
        .filter(|j| !map.contains(j))
        .collect();

    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    if let Some(edit) = slide_list_edit(parts, map) {
        edits.push(edit);
    }
    edits.extend(section_edits(parts, deck, map, &mut restated.kept));
    if !edits.is_empty() && parts.unresolved > 0 {
        return Err(format!(
            "the deck names {} slide(s) this writer cannot resolve to a part, and \
             restating `p:sldIdLst` would drop them",
            parts.unresolved
        ));
    }

    if !removed.is_empty() {
        // A custom show names its slides through the same relationships, and
        // this writer does not model one: removing a slide would leave the show
        // pointing at nothing, which is a repair dialog rather than a deck.
        if parts.root.child("custShowLst").is_some() {
            return Err("the deck has custom shows, which name the slides through \
                        relationships this writer does not restate"
                .into());
        }
        for &j in &removed {
            let entry = &parts.entries[j];
            let mut gone = vec![entry.part.clone()];
            let slide_rels = package.relationships(&entry.part);
            if let Some(notes) = super::notes::part(&slide_rels) {
                gone.push(notes.to_string());
            }
            for name in std::mem::take(&mut gone) {
                if let Some(rels) = rels_part_name(&name) {
                    gone.push(rels);
                }
                gone.push(name);
            }
            restated
                .removed
                .extend(gone.into_iter().filter(|name| package.has_part(name)));
        }
        // Nothing that stays may point at something that goes. A hyperlink
        // from another slide, a thumbnail, a part nobody has looked at yet: a
        // dangling relationship is exactly what P3 measured LibreOffice
        // accepting and PowerPoint refusing.
        for name in package.part_names() {
            if name == parts.main || name.ends_with(".rels") || restated.removed.contains(name) {
                continue;
            }
            for (_, rel) in package.relationships(name).iter() {
                if !rel.external && restated.removed.contains(&rel.target) {
                    return Err(format!(
                        "`{name}` points at `{}`, which a removed slide takes with it",
                        rel.target
                    ));
                }
            }
        }
        restated
            .parts
            .push(deck_rels_without(package, parts, &removed)?);
    }

    if !edits.is_empty() {
        restated.parts.push((
            parts.main.clone(),
            splice(&parts.source, edits).into_bytes(),
        ));
    }
    Ok(restated)
}

/// `p:sldIdLst` restated in the model's order, or `None` when it already says
/// what the model says — which is the case that keeps an unedited deck's
/// `ppt/presentation.xml` byte for byte the part that was read.
fn slide_list_edit(parts: &DeckParts, map: &[usize]) -> Option<(Range<usize>, String)> {
    let list = parts.root.child("sldIdLst")?;
    let children: Vec<&Element> = list.children_named("sldId").collect();
    let range = children.first()?.span.start..children.last()?.span.end;
    let wanted: String = map
        .iter()
        .map(|&j| &parts.source[parts.entries[j].span.clone()])
        .collect();
    (wanted != parts.source[range.clone()]).then_some((range, wanted))
}

/// `p14:sectionLst` restated: each section keeps its name and its id and lists
/// the slides the model puts in it, in the model's order.
///
/// Sections live in an extension and a deck without them is the normal case,
/// so every step of the path is optional and an absent `p14:sectionLst` is no
/// edit at all rather than one that writes an empty one.
fn section_edits(
    parts: &DeckParts,
    deck: &Presentation,
    map: &[usize],
    kept: &mut Vec<String>,
) -> Vec<(Range<usize>, String)> {
    let mut edits = Vec::new();
    let sections: Vec<&Element> = parts
        .root
        .child("extLst")
        .into_iter()
        .flat_map(|list| list.children_named("ext"))
        .filter_map(|ext| ext.child("sectionLst"))
        .flat_map(|list| list.children_named("section"))
        .collect();
    if sections.is_empty() {
        return edits;
    }

    let named: BTreeSet<&str> = sections.iter().filter_map(|s| s.attr("name")).collect();
    // What the deck currently says, by the slide id `p14:sectionLst` refers to.
    // A model that states no section for a slide is not a model asking for it
    // to be taken out of every section: it is a model that does not state it,
    // and the package's own answer stands — the rule the whole writer follows.
    let mut current: BTreeMap<i64, &str> = BTreeMap::new();
    for section in &sections {
        let Some(name) = section.attr("name") else {
            continue;
        };
        for id in section
            .child("sldIdLst")
            .into_iter()
            .flat_map(|list| list.children_named("sldId"))
            .filter_map(|entry| entry.attr_i64("id"))
        {
            current.insert(id, name);
        }
    }

    // Where each slide goes, and the ones that cannot go where they are asked.
    let mut wanted_section: Vec<Option<&str>> = Vec::with_capacity(map.len());
    let mut unnamed: Vec<String> = Vec::new();
    for (index, &j) in map.iter().enumerate() {
        let current = current.get(&parts.entries[j].id).copied();
        match deck.slides[index].section.as_deref() {
            Some(name) if named.contains(name) => wanted_section.push(Some(name)),
            Some(name) => {
                unnamed.push(format!("slide {} in `{name}`", index + 1));
                wanted_section.push(current);
            }
            None => wanted_section.push(current),
        }
    }
    if !unnamed.is_empty() {
        kept.push(format!(
            "the deck has no section to put {}, and creating one needs a \
             `p14:section` GUID nothing recorded",
            unnamed.join(", ")
        ));
    }

    for section in sections {
        let Some(name) = section.attr("name") else {
            continue;
        };
        let Some(list) = section.child("sldIdLst") else {
            kept.push(format!(
                "the deck's section `{name}` states no slide list to restate"
            ));
            continue;
        };
        let known: Vec<(i64, &Element)> = list
            .children_named("sldId")
            .filter_map(|entry| Some((entry.attr_i64("id")?, entry)))
            .collect();
        // The namespace the deck itself writes these in, taken off the
        // element rather than assumed: `p14` is a convention, not a rule.
        let ns = if list.prefix.is_empty() {
            String::new()
        } else {
            format!("{}:", list.prefix)
        };
        let mut wanted = String::new();
        for (i, &j) in map.iter().enumerate() {
            if wanted_section[i] != Some(name) {
                continue;
            }
            let entry = &parts.entries[j];
            match known.iter().find(|(id, _)| *id == entry.id) {
                // The entry the deck already had, byte for byte.
                Some((_, element)) => wanted.push_str(&parts.source[element.span.clone()]),
                None => wanted.push_str(&format!("<{ns}sldId id=\"{}\"/>", entry.id)),
            }
        }
        let restated = if wanted.is_empty() {
            format!("<{ns}sldIdLst/>")
        } else {
            format!("<{ns}sldIdLst>{wanted}</{ns}sldIdLst>")
        };
        if restated != parts.source[list.span.clone()] {
            edits.push((list.span.clone(), restated));
        }
    }
    edits
}

/// `ppt/_rels/presentation.xml.rels` without the relationships of the slides
/// that were removed. Spliced, so the relationships that stay keep their ids,
/// their order and their bytes.
fn deck_rels_without(
    package: &Package,
    parts: &DeckParts,
    removed: &[usize],
) -> Result<(String, Vec<u8>), String> {
    let name = rels_part_name(&parts.main)
        .ok_or_else(|| format!("`{}` has no relationship part to restate", parts.main))?;
    let source = package
        .text(&name)
        .map_err(|_| format!("`{name}` is not readable as XML"))?;
    let root =
        Element::parse(&name, source.as_bytes()).map_err(|_| format!("`{name}` did not parse"))?;
    let gone: BTreeSet<&str> = removed
        .iter()
        .map(|&j| parts.entries[j].rel.as_str())
        .collect();
    let edits: Vec<(Range<usize>, String)> = root
        .children_named("Relationship")
        .filter(|rel| gone.contains(rel.attr("Id").unwrap_or_default()))
        .map(|rel| (rel.span.clone(), String::new()))
        .collect();
    Ok((name, splice(source, edits).into_bytes()))
}

/// One slide part rewritten, and what inside it was left as it was.
struct Rewritten {
    xml: String,
    kept: Vec<String>,
    /// Fragments the model carried in a dialect this package cannot hold:
    /// their id and the format they were written in.
    dropped: Vec<(String, String)>,
    /// Autofit scales dropped because the text under them changed (15-F):
    /// the clause naming the shape, and the scale that was in the file.
    stale: Vec<(String, u32)>,
}

/// Rewrites the text bodies of one slide, leaving every other byte alone.
/// Everything a slide's rewrite reaches outside the slide itself: the package
/// it was read from, the three neighbours an edit can move, and the payloads
/// the stubs point into.
struct Outside<'a, 'm> {
    /// Name of the part being rewritten, which is what says whether a
    /// fragment's markup belongs to it.
    part: &'a str,
    package: &'a Package,
    rels: &'a mut PartRels,
    media: &'a mut Media<'m>,
    fragments: &'a BTreeMap<&'a str, &'a RawFragment>,
}

fn rewrite_slide(
    root: &Element,
    source: &str,
    slide: &Slide,
    outside: &mut Outside<'_, '_>,
) -> Result<Rewritten, String> {
    let tree = root
        .path(&["cSld", "spTree"])
        .ok_or_else(|| "the slide has no shape tree".to_string())?;
    // The z-order the reader counted: `p:nvGrpSpPr` and `p:grpSpPr` are the
    // tree's own identity and transform, not shapes on it.
    let children: Vec<&Element> = tree
        .children()
        .filter(|child| !matches!(child.name.as_str(), "nvGrpSpPr" | "grpSpPr"))
        .collect();
    if children.len() != slide.shapes.len() {
        return Err(format!(
            "the model holds {} shape(s) and the slide {}; adding or removing a shape \
             is not this increment's",
            slide.shapes.len(),
            children.len()
        ));
    }

    let lang = editing_language(tree);
    let part = outside.part;
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut kept: Vec<String> = Vec::new();
    let mut dropped: Vec<(String, String)> = Vec::new();
    let mut stale: Vec<(String, u32)> = Vec::new();
    // What is not a shape: `p:transition`, `p:timing` and anything else the
    // reader took off `p:sld` itself, put back at the level it came from.
    for id in &slide.raw {
        // `p:cSld` is not a candidate whatever a fragment calls itself: an edit
        // spanning it would swallow every text edit inside the shape tree, and
        // `splice` drops an overlapping edit without a word.
        let candidates = root.children().filter(|child| child.name != "cSld");
        match reinject(candidates, source, id.as_str(), outside, &mut edits) {
            Reinjected::Standing => {}
            Reinjected::Dropped(format) => dropped.push((id.as_str().to_string(), format)),
            Reinjected::Kept(reason) => kept.push(reason),
        }
    }
    // Which tree child each model shape is. The model's shapes are in reading
    // order — that is what the reader produced and what DocMark writes — so the
    // package's own reading order is the map, and `Shape::z_index` is not
    // consulted: a deck that travelled through DocMark carries the position it
    // was written at there, not the z-order the tree has (`order::of_tree`).
    let reading = super::order::of_tree(&children);
    for (position, shape) in slide.shapes.iter().enumerate() {
        let Some(element) = reading.get(position).and_then(|at| children.get(*at)) else {
            return Err(format!(
                "shape {position} is outside the slide's shape tree"
            ));
        };
        let (body, bulleted) = match &shape.kind {
            ShapeKind::Placeholder(Placeholder { body, ph_type, .. }) => (body, ph_type.is_body()),
            ShapeKind::TextBox { body } => (body, false),
            // A picture is not a text body and never was: what the model can
            // have changed is the bitmap itself, its alt text, the size it is
            // drawn at and its crop, each written only where it differs from
            // what the part already says.
            ShapeKind::Picture(image) => {
                if let Err(reason) = rewrite_picture(
                    element,
                    source,
                    image,
                    outside.package,
                    outside.rels,
                    outside.media,
                    &mut edits,
                ) {
                    kept.push(reason);
                }
                continue;
            }
            ShapeKind::Table(table) => {
                if let Err(reason) =
                    rewrite_table(element, table, lang.as_deref(), outside.rels, &mut edits)
                {
                    kept.push(reason);
                }
                continue;
            }
            // A preserved object goes back **as the bytes that came out**:
            // SmartArt, OLE, a group, a custom geometry, an animation the IR
            // never modelled. The splice already leaves the element alone, so
            // the work here is to say so honestly and to put the fragment back
            // when the model carries one that differs from the package's.
            ShapeKind::Raw(RawShape { raw, .. }) => {
                let Some(id) = raw else {
                    // No payload at this fidelity level: the stub is all the
                    // model has, and the object standing in the package is the
                    // original XML at its original z-order. Nothing is lost.
                    continue;
                };
                match reinject(
                    std::iter::once(*element),
                    source,
                    id.as_str(),
                    outside,
                    &mut edits,
                ) {
                    Reinjected::Standing => {}
                    Reinjected::Dropped(format) => {
                        dropped.push((id.as_str().to_string(), format));
                    }
                    Reinjected::Kept(reason) => kept.push(reason),
                }
                continue;
            }
            other => {
                kept.push(format!("its {}", shape_noun(other)));
                continue;
            }
        };
        // A `p:sp` the reader stubbed — a custom geometry, a group — reaches
        // here as a raw shape and is filtered out above; anything else that is
        // not a `p:sp` never carried a modelled text body either.
        if element.name != "sp" {
            kept.push(format!("its `{}`", qualified(element)));
            continue;
        }
        let Some(tx_body) = element.child("txBody") else {
            kept.push("a text shape with no text body".into());
            continue;
        };
        match write_text::write_body(body, bulleted, lang.as_deref(), outside.rels) {
            Ok(paragraphs) => {
                let range = paragraph_range(tx_body);
                let written = if paragraphs.is_empty() && !source[range.clone()].trim().is_empty() {
                    // `CT_TextBody` needs a paragraph; emptying the box means
                    // writing the empty one rather than none at all.
                    "<a:p/>".to_string()
                } else {
                    paragraphs
                };
                // 15-F: *this* is what «the shape's text changed» means, and
                // the writer already knew it — 15-B's rule is that nothing is
                // written unless it differs from what the part already says, so
                // a body whose new bytes are the old bytes did not change and
                // its scale is not stale.
                if source[range.clone()] != written {
                    if let Some(scale) = drop_stale_autofit(tx_body, source, &mut edits) {
                        // Named the way the *part* names it when the model has
                        // no name of its own: a deck that came back through
                        // DocMark carries none, and «shape 1» is not something
                        // a caller can go and look at.
                        let named = shape
                            .name
                            .clone()
                            .or_else(|| {
                                element
                                    .path(&["nvSpPr", "cNvPr"])
                                    .and_then(|nv| nv.attr("name"))
                                    .filter(|name| !name.is_empty())
                                    .map(str::to_string)
                            })
                            .unwrap_or_else(|| shape_name(shape));
                        stale.push((
                            format!(
                                "the text of `{named}` on `{part}` changed, so the writer dropped"
                            ),
                            scale,
                        ));
                    }
                }
                edits.push((range, written));
            }
            Err(reason) => kept.push(format!("{reason} in `{}`", shape_name(shape))),
        }
    }

    Ok(Rewritten {
        xml: splice(source, edits),
        kept,
        dropped,
        stale,
    })
}

/// Drops the autofit scale of a text body whose text has just changed, and
/// says which scale it was.
///
/// Analysis §5.4: `a:normAutofit@fontScale` is a number PowerPoint computed for
/// the text that was in the box the last time a human edited it, and it does
/// not recompute it when a file changes underneath. A scale left over text that
/// is no longer there is therefore **a lie no reader can detect** — the box
/// renders at 62.5 % for content sized for 100 %, and nothing in the package
/// says why. Measuring the text to compute the right scale needs font metrics
/// this binary does not carry (post-2.0 backlog), so the honest answer is to
/// drop the number and let PowerPoint recompute it, saying so in a warning.
///
/// `lnSpcReduction` goes with it: it is the same computation's other half, and
/// keeping it would leave the paragraphs squeezed for text that has gone. A
/// `lnSpcReduction` with **no** `fontScale` is left standing — the shape states
/// no scale, so there is nothing this increment is licensed to call stale, and
/// leaving something is not a loss.
///
/// The edit is on `a:bodyPr`, which precedes the `a:p` children the caller is
/// replacing, so the two never overlap — which matters, because [`splice`]
/// drops an overlapping edit without a word.
fn drop_stale_autofit(
    tx_body: &Element,
    source: &str,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Option<u32> {
    let fit = tx_body.path(&["bodyPr", "normAutofit"])?;
    let scale = fit
        .attr_i64("fontScale")
        .and_then(|scale| u32::try_from(scale).ok())?;
    set_attr(source, fit, "fontScale", None, edits);
    set_attr(source, fit, "lnSpcReduction", None, edits);
    Some(scale)
}

/// Puts a slide's speaker notes back into the notes page that holds them.
///
/// The part is reached through the **slide's own relationships**, never by
/// number: `notesSlide7.xml` belonging to the seventh slide is a convention
/// PowerPoint happens to follow and a converter is free to break. The reader
/// goes through the same door ([`super::notes::part`]), so the page this writes
/// is the page whose text the IR actually holds — the condition
/// [`SkeletonRef::rebuilt_parts`] states, checked at the door rather than
/// against a list, because a deck that came back through DocMark carries no
/// list at all.
///
/// The unit of rewriting is the same as everywhere else: the `a:p` sequence of
/// one text body. The thumbnail of the slide, the header, the footer, the date
/// and the page number around it are the notes master's furniture and the
/// bytes that were read.
fn rewrite_notes(
    package: &Package,
    slide_part: &str,
    slide: &Slide,
    rebuild: &mut Rebuild,
) -> Result<Vec<(String, u32)>, String> {
    let slide_rels = package.relationships(slide_part);
    let notes_part = super::notes::part(&slide_rels).map(str::to_string);
    let (blocks, notes_part) = match (&slide.notes, notes_part) {
        (None, None) => return Ok(Vec::new()),
        (None, Some(part)) => {
            return Err(format!(
                "its speaker notes, which the model does not hold and `{part}` still \
                 states: deleting a notes page takes the deck's own parts with it"
            ))
        }
        (Some(_), None) => {
            // Since 15-G a notes page is *written* for a slide that has none —
            // in [`seed`], before this runs. Reaching here means the package
            // has no notes master for one to hang from, which is the one case
            // the writer cannot answer.
            return Err(
                "its speaker notes, which the deck has no notes page to state and no \
                 notes master to hang a new one from"
                    .into(),
            );
        }
        (Some(blocks), Some(part)) => (blocks, part),
    };

    let source = package.text(&notes_part).map_err(|_| {
        format!("its speaker notes, whose part `{notes_part}` is not readable as XML")
    })?;
    let root = Element::parse(&notes_part, source.as_bytes())
        .map_err(|_| format!("its speaker notes, whose part `{notes_part}` did not parse"))?;
    let tree = root
        .path(&["cSld", "spTree"])
        .ok_or_else(|| format!("its speaker notes, whose part `{notes_part}` has no shape tree"))?;

    // The reader's rule, unchanged: a shape with no `p:ph` at all, or with a
    // body-like one, is the note; everything else on the page is furniture.
    let bodies: Vec<&Element> = tree
        .children_named("sp")
        .filter(|sp| {
            let ph = sp.path(&["nvSpPr", "nvPr", "ph"]);
            let is_note = ph
                .map(|ph| PhType::parse(ph.attr("type").unwrap_or_default()).is_body())
                .unwrap_or(true);
            is_note && sp.child("txBody").is_some()
        })
        .collect();
    if bodies.is_empty() && blocks.is_empty() {
        // A notes page with nothing to write the notes into, and no notes to
        // write. Nothing was lost, so nothing is reported.
        return Ok(Vec::new());
    }
    // The reader concatenates the text of every note-like shape on the page,
    // so a page with two of them cannot be written back: the model no longer
    // records where one note ended and the next began.
    let [body] = bodies[..] else {
        return Err(format!(
            "its speaker notes, which `{notes_part}` states over {} text shape(s) and \
             the model holds as one note",
            bodies.len()
        ));
    };
    let tx_body = body.child("txBody").expect("the filter required one");

    let mut rels = PartRels::new(package, &notes_part);
    let paragraphs =
        write_text::write_body(blocks, false, editing_language(tree).as_deref(), &mut rels)
            .map_err(|reason| format!("its speaker notes, which hold {reason}"))?;
    let range = paragraph_range(tx_body);
    let written = if paragraphs.is_empty() && !source[range.clone()].trim().is_empty() {
        // `CT_TextBody` needs a paragraph; emptying the page means writing the
        // empty one rather than none at all.
        "<a:p/>".to_string()
    } else {
        paragraphs
    };
    // A notes page is a text body like any other, and a scale computed for a
    // note that has been retyped is as stale as one on a slide (15-F).
    let mut edits = Vec::new();
    let mut stale = Vec::new();
    if source[range.clone()] != written {
        if let Some(scale) = drop_stale_autofit(tx_body, source, &mut edits) {
            stale.push((
                format!("the note in `{notes_part}` changed, so the writer dropped"),
                scale,
            ));
        }
    }
    edits.push((range, written));
    let xml = splice(source, edits);
    if xml != source {
        rebuild
            .replacements
            .insert(notes_part.clone(), xml.into_bytes());
    }
    // A hyperlink typed into a note needs a relationship in the notes page's
    // own `_rels`, exactly as one typed into a slide does.
    if let Some((name, xml)) = rels.rewritten() {
        rebuild.put(package, name, xml.into_bytes());
    }
    Ok(stale)
}

/// What became of one raw fragment on the way out.
enum Reinjected {
    /// The original markup is in the written part — either because the
    /// preserved package already holds it byte for byte, or because the
    /// fragment the model carried was spliced back over it.
    Standing,
    /// The fragment is in a dialect this package cannot hold, so it was not
    /// emitted: `Warning::RawBlockDropped`, never nonsense in the file.
    Dropped(String),
    /// The fragment could not be placed, and the package's own bytes stand.
    Kept(String),
}

/// Puts one raw fragment back where it came from.
///
/// The preserved package is the first answer: a fragment nobody edited is
/// already in the part, at its original z-order, byte for byte — so the
/// comparison is with the source, and an identical fragment produces no edit
/// and no warning. What this adds is the case the skeleton cannot cover: a
/// payload that differs from the package's bytes, which is written back over
/// the element it belongs to.
///
/// `candidates` are the elements the fragment could be: one shape for a shape
/// fragment, the children of `p:sld` for a slide-level one, where the match is
/// by element name because the id is not in the file.
fn reinject<'a>(
    candidates: impl Iterator<Item = &'a Element>,
    source: &str,
    id: &str,
    outside: &Outside<'_, '_>,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Reinjected {
    let Some(fragment) = outside.fragments.get(id) else {
        // The stub points at a payload this document does not carry — a deck
        // read back from DocMark, whose sidecar the deck parser does not load.
        // The object is in the package all the same, which is what the caller
        // needs; inventing markup for it would be worse than saying nothing.
        return Reinjected::Standing;
    };
    if fragment.format != "ooxml" {
        return Reinjected::Dropped(fragment.format.clone());
    }
    if fragment.part != outside.part {
        // A chart's fragment is the chart part, not this slide's markup, and
        // that part is outside `rebuilt_parts`: it is copied, not rewritten.
        return Reinjected::Kept(format!(
            "the preserved markup `{id}`, which belongs to `{}`",
            fragment.part
        ));
    }
    // A fragment whose own root element cannot be read matches **nothing**:
    // falling back to the first candidate would put a slide-level payload over
    // `p:spTree`, which is a corrupt slide rather than a degraded one.
    let Some(name) = fragment_name(&fragment.content) else {
        return Reinjected::Kept(format!(
            "the preserved markup `{id}`, which does not open with an element this writer can name"
        ));
    };
    for element in candidates {
        if name != qualified(element) {
            continue;
        }
        if element.raw(source) == fragment.content {
            return Reinjected::Standing;
        }
        edits.push((element.span.clone(), fragment.content.clone()));
        return Reinjected::Standing;
    }
    Reinjected::Kept(format!(
        "the preserved markup `{id}`, which no element of the part answers to"
    ))
}

/// The qualified name of a fragment's own root element, read off its first tag.
///
/// The fragment is a slice of a part, so its first `<` opens the element it
/// preserves; `None` for markup this cannot read rather than a guess that would
/// match the wrong element.
fn fragment_name(content: &str) -> Option<&str> {
    let rest = content.strip_prefix('<')?;
    let end = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/')?;
    let name = &rest[..end];
    (!name.is_empty() && !name.starts_with('?') && !name.starts_with('!')).then_some(name)
}

/// The editing language the slide's runs already state, if they agree.
///
/// `a:rPr@lang` is spell-check metadata with no IR field, and it is on every
/// run PowerPoint writes. Reading it off the slide and putting it back is what
/// keeps a rewritten body the same file rather than the same text: the
/// alternative is dropping an attribute from every run in the deck.
fn editing_language(tree: &Element) -> Option<String> {
    fn walk(element: &Element, out: &mut Vec<String>) {
        if element.name == "rPr" || element.name == "endParaRPr" {
            if let Some(lang) = element.attr("lang") {
                out.push(lang.to_string());
            }
        }
        for child in element.children() {
            walk(child, out);
        }
    }
    let mut found = Vec::new();
    walk(tree, &mut found);
    let first = found.first()?.clone();
    // Disagreement is a slide with runs in two languages, and picking one would
    // retag the other. Better to write none than to write the wrong one.
    found.iter().all(|lang| *lang == first).then_some(first)
}

/// The byte range of a text body's `a:p` children.
///
/// Empty and pointing just after `a:lstStyle` when there are none, so an
/// insertion lands where a paragraph belongs rather than before the properties
/// that have to precede it.
fn paragraph_range(tx_body: &Element) -> std::ops::Range<usize> {
    let paragraphs: Vec<&Element> = tx_body.children_named("p").collect();
    match (paragraphs.first(), paragraphs.last()) {
        (Some(first), Some(last)) => first.span.start..last.span.end,
        _ => {
            let after = tx_body
                .children()
                .map(|child| child.span.end)
                .max()
                .unwrap_or(tx_body.span.end);
            after..after
        }
    }
}

/// Applies non-overlapping edits to the source, left to right.
fn splice(source: &str, mut edits: Vec<(std::ops::Range<usize>, String)>) -> String {
    edits.sort_by_key(|(range, _)| range.start);
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0;
    for (range, replacement) in edits {
        if range.start < cursor {
            continue;
        }
        out.push_str(&source[cursor..range.start]);
        out.push_str(&replacement);
        cursor = range.end;
    }
    out.push_str(&source[cursor..]);
    out
}

fn qualified(element: &Element) -> String {
    if element.prefix.is_empty() {
        element.name.clone()
    } else {
        format!("{}:{}", element.prefix, element.name)
    }
}

fn shape_name(shape: &docsai_model::presentation::Shape) -> String {
    shape
        .name
        .clone()
        .unwrap_or_else(|| format!("shape {}", shape.z_index))
}

/// What a shape is, for a warning that has to name it.
fn shape_noun(kind: &ShapeKind) -> &'static str {
    match kind {
        ShapeKind::Picture(_) => "picture",
        ShapeKind::Table(_) => "table",
        ShapeKind::Chart(_) => "chart",
        ShapeKind::Raw(_) => "preserved object",
        ShapeKind::Group(_) => "group",
        _ => "shape this writer does not rebuild",
    }
}

/// Rewrites what a picture states about itself, attribute by attribute.
///
/// Nothing here is written unless it differs from what the part already says,
/// which is what keeps an unedited deck byte for byte: a picture nobody touched
/// produces no edit at all, and a picture whose alt text was retyped produces
/// exactly one.
///
/// The bitmap is compared by **bytes**, not by name: the asset store keys on
/// the content hash, so an image that came out of this very package is the same
/// file and keeps the relationship it already had. A different one becomes a
/// new `ppt/media/` part, a new relationship, and — if the deck had never
/// carried that extension — a new content type.
fn rewrite_picture(
    pic: &Element,
    source: &str,
    image: &ImageRef,
    package: &Package,
    rels: &mut PartRels,
    media: &mut Media<'_>,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Result<(), String> {
    if pic.name != "pic" {
        return Err(format!(
            "its picture, which the part states as `{}`",
            qualified(pic)
        ));
    }
    let Some(blip) = pic.path(&["blipFill", "blip"]) else {
        return Err("its picture, which has no `a:blip` to point an image at".into());
    };

    // A linked picture states `r:link` and holds no bytes in the package; the
    // model keeps the URL and this writer has nothing to re-inject.
    if image.external_src.is_none() {
        let embed = blip
            .attrs
            .iter()
            .find(|(name, _)| local_of(name) == "embed")
            .map(|(name, value)| (name.clone(), value.clone()));
        let current = embed
            .as_ref()
            .and_then(|(_, id)| rels.target_of(id))
            .and_then(|target| package.part(&target).map(<[u8]>::to_vec));
        let assets = media.assets;
        let Some(wanted) = assets.get(&image.asset) else {
            return Err(format!(
                "its picture, whose bytes the asset store does not hold under `{}`",
                image.asset.as_str()
            ));
        };
        if current.as_deref() != Some(wanted) {
            let part = media.part_for(image)?;
            let id = rels.relate(IMAGE_REL, &part, false);
            match &embed {
                Some((name, value)) if *value != id => {
                    set_attr(source, blip, name, Some(&id), edits);
                }
                Some(_) => {}
                None => set_attr(source, blip, "r:embed", Some(&id), edits),
            }
        }
    }

    if let Some(nv) = pic.path(&["nvPicPr", "cNvPr"]) {
        if nv.attr("descr").unwrap_or_default() != image.alt {
            let alt = image.alt.as_str();
            set_attr(source, nv, "descr", (!alt.is_empty()).then_some(alt), edits);
        }
        // `name` is required on `CT_NonVisualDrawingProps`, so a model that
        // does not carry one leaves the part's alone rather than emptying it.
        if let Some(name) = &image.name {
            if nv.attr("name") != Some(name.as_str()) {
                set_attr(source, nv, "name", Some(name), edits);
            }
        }
    }

    // The crop is the bitmap's own, `a:srcRect` beside the blip; the reader's
    // own function decides what the part currently says, so the comparison is
    // made in the units the model uses rather than in the ones the file does.
    let src_rect = pic.path(&["blipFill", "srcRect"]);
    let mut probe = ImageGeometry::inline(Size::new(Length::from_emu(0), Length::from_emu(0)));
    crate::drawingml::read_crop(src_rect, &mut probe);
    if probe.crop != image.geometry.crop {
        match (src_rect, &image.geometry.crop) {
            (Some(rect), Some(crop)) => edits.push((rect.span.clone(), src_rect_xml(crop))),
            (Some(rect), None) => edits.push((rect.span.clone(), String::new())),
            (None, Some(crop)) => {
                edits.push((blip.span.end..blip.span.end, src_rect_xml(crop)));
            }
            (None, None) => {}
        }
    }

    // Last, because it is the one that can end the function: a picture with no
    // `a:ext` inherits its size, and writing one would place a picture the deck
    // had left to its layout.
    let size = image.geometry.display_size;
    match pic.path(&["spPr", "xfrm", "ext"]) {
        Some(ext) => {
            for (name, emu) in [("cx", size.width.emu()), ("cy", size.height.emu())] {
                if ext.attr_i64(name) != Some(emu) {
                    set_attr(source, ext, name, Some(&emu.to_string()), edits);
                }
            }
        }
        None if size.width.emu() != 0 || size.height.emu() != 0 => {
            return Err("its picture's size, which the part does not state at all".into());
        }
        None => {}
    }
    Ok(())
}

/// `a:srcRect` in the thousandths of a percent the format counts in.
fn src_rect_xml(crop: &docsai_model::image::CropRect) -> String {
    let mut out = String::from("<a:srcRect");
    for (name, value) in [
        ("l", crop.left),
        ("t", crop.top),
        ("r", crop.right),
        ("b", crop.bottom),
    ] {
        let units = (value * 1_000.0).round() as i64;
        if units != 0 {
            out.push_str(&format!(" {name}=\"{units}\""));
        }
    }
    out.push_str("/>");
    out
}

/// Rewrites the text of a table's cells, and nothing else about the table.
///
/// The same rule as a shape, one container further in: the grid, the merges,
/// the row heights, the cell properties and the table style are the bytes that
/// were read, and what comes from the model is the `a:p` sequence of each
/// cell's body. A grid the model changed is a table that has to be regenerated
/// rather than spliced, and regenerating an `a:tbl` from the IR belongs with
/// the deck that has no skeleton to preserve.
fn rewrite_table(
    frame: &Element,
    table: &Table,
    lang: Option<&str>,
    rels: &mut PartRels,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Result<(), String> {
    let Some(tbl) = frame.path(&["graphic", "graphicData", "tbl"]) else {
        return Err("its table, which the part does not state as an `a:tbl`".into());
    };
    let rows: Vec<&Element> = tbl.children_named("tr").collect();
    if rows.len() != table.rows.len() {
        return Err(format!(
            "its table, whose grid the model moved from {} row(s) to {} — an `a:tbl` \
             is written from the model only onto a slide this writer generates, \
             where there is no grid, no row height and no cell property to lose",
            rows.len(),
            table.rows.len()
        ));
    }

    for (tr, row) in rows.iter().zip(&table.rows) {
        // A cell swallowed horizontally is not in the IR grid at all: a
        // `gridSpan` of 2 already occupies both columns, so the reader dropped
        // it and the pairing has to drop it here too.
        let cells: Vec<&Element> = tr
            .children_named("tc")
            .filter(|tc| !is_true(tc.attr("hMerge")))
            .collect();
        if cells.len() != row.cells.len() {
            return Err(format!(
                "its table, whose row the model moved from {} cell(s) to {} — an \
                 `a:tbl` is written from the model only onto a slide this writer \
                 generates, where there is no grid to lose",
                cells.len(),
                row.cells.len()
            ));
        }
        for (tc, cell) in cells.iter().zip(&row.cells) {
            // A cell covered by the one above holds no content of its own, and
            // the reader read none: writing the empty body it has in the model
            // would empty the `a:txBody` PowerPoint keeps there.
            if cell.covered {
                continue;
            }
            let Some(body) = tc.child("txBody") else {
                continue;
            };
            let paragraphs = write_text::write_body(&cell.blocks, false, lang, rels)
                .map_err(|reason| format!("its table, which holds {reason}"))?;
            let range = paragraph_range(body);
            if paragraphs.is_empty() {
                edits.push((range, "<a:p/>".to_string()));
            } else {
                edits.push((range, paragraphs));
            }
        }
    }

    let grid = tbl
        .child("tblGrid")
        .map(|grid| grid.children_named("gridCol").count())
        .unwrap_or(0);
    let style = tbl
        .path(&["tblPr", "tableStyleId"])
        .map(|id| id.text().trim().to_string());
    if grid != table.col_widths.len()
        || style.as_deref() != table.style.as_ref().map(|id| id.as_str())
    {
        return Err("its table's column grid and style reference".into());
    }
    Ok(())
}

/// An OOXML boolean attribute, where absent means false.
fn is_true(value: Option<&str>) -> bool {
    matches!(value, Some("1") | Some("true") | Some("on"))
}

/// The relationships of one part: what it already has, and what an edit added.
///
/// The reuse comes first and matters most. A picture that did not change, a
/// link that still points where it pointed, a slide reference that was already
/// there — all resolve to the id the deck was written with, so the `_rels` part
/// is not touched at all and the slide keeps the ids it was read with. Only a
/// genuinely new target mints an id, and even then the part is **spliced**: the
/// relationships the deck had keep their bytes, their order and their ids.
struct PartRels {
    /// The `_rels` part's own name.
    part: String,
    /// The directory a relative target is written against.
    base: String,
    existing: Relationships,
    /// The part as it stands, when the package has one.
    source: Option<String>,
    /// Id, type, package-absolute target, external.
    added: Vec<(String, &'static str, String, bool)>,
    used: BTreeSet<String>,
}

impl PartRels {
    fn new(package: &Package, owner: &str) -> Self {
        let existing = package.relationships(owner);
        let part = rels_part_name(owner).unwrap_or_default();
        let source = package.text(&part).ok().map(str::to_string);
        let used = existing.iter().map(|(id, _)| id.to_string()).collect();
        PartRels {
            part,
            base: parent_dir(owner),
            existing,
            source,
            added: Vec::new(),
            used,
        }
    }

    /// The part a relationship id points at, when it points inside the package.
    fn target_of(&self, id: &str) -> Option<String> {
        self.existing
            .get(id)
            .filter(|rel| !rel.external)
            .map(|rel| rel.target.clone())
    }

    /// The id already pointing at a target, whatever kind of relationship it is.
    fn id_of_target(&self, target: &str) -> Option<String> {
        self.existing
            .iter()
            .find(|(_, rel)| rel.target == target)
            .map(|(id, _)| id.to_string())
            .or_else(|| {
                self.added
                    .iter()
                    .find(|(_, _, added, _)| added == target)
                    .map(|(id, _, _, _)| id.clone())
            })
    }

    /// The id for a target, reusing the relationship the part already has.
    fn relate(&mut self, kind: &'static str, target: &str, external: bool) -> String {
        if let Some(id) = self.id_of_target(target) {
            return id;
        }
        let id = self.fresh_id();
        self.added
            .push((id.clone(), kind, target.to_string(), external));
        id
    }

    fn fresh_id(&mut self) -> String {
        let mut n = self.used.len() + 1;
        loop {
            let id = format!("rId{n}");
            if self.used.insert(id.clone()) {
                return id;
            }
            n += 1;
        }
    }

    /// The relationship part with the new relationships in it, or `None` when
    /// nothing was added — an unedited slide must not move its `_rels` at all.
    fn rewritten(&self) -> Option<(String, String)> {
        if self.added.is_empty() || self.part.is_empty() {
            return None;
        }
        let mut entries = String::new();
        for (id, kind, target, external) in &self.added {
            let (target, mode) = if *external {
                (target.clone(), " TargetMode=\"External\"")
            } else {
                (relative_target(&self.base, target), "")
            };
            entries.push_str(&format!(
                "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"{mode}/>",
                esc_attr(id),
                esc_attr(kind),
                esc_attr(&target)
            ));
        }
        match &self.source {
            Some(source) => {
                let at = source.rfind("</Relationships>")?;
                Some((
                    self.part.clone(),
                    format!("{}{entries}{}", &source[..at], &source[at..]),
                ))
            }
            None => Some((
                self.part.clone(),
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
                     <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
                     {entries}</Relationships>"
                ),
            )),
        }
    }
}

impl Links for PartRels {
    fn hyperlink(&mut self, target: &str) -> Option<String> {
        if let Some(id) = self.id_of_target(target) {
            return Some(id);
        }
        // An internal target — another slide of this deck — is not a hyperlink
        // relationship at all: PowerPoint writes a `slide`-typed one and marks
        // the `a:hlinkClick` with `action="ppaction://hlinksldjump"`, and the
        // IR records neither the action nor which slide the target names. 15-E
        // gives the writer the slide-to-part map and still does not settle
        // this, because the missing half is in the model. A link the deck did
        // not already have is refused rather than pointed at nothing.
        is_external(target).then(|| self.relate(HYPERLINK_REL, target, true))
    }
}

/// A target a hyperlink relationship can carry as it stands.
fn is_external(target: &str) -> bool {
    target.contains("://") || target.starts_with("mailto:") || target.starts_with("tel:")
}

/// Media parts the written package needs and the original did not have.
struct Media<'a> {
    assets: &'a dyn AssetStore,
    /// Asset id to the part it was given, so one bitmap on four slides is one
    /// part — the store already deduplicated it, and so does this.
    parts: BTreeMap<String, String>,
    added: BTreeMap<String, Vec<u8>>,
    /// Every part name the package uses, without its extension, so a new one
    /// collides with nothing — not even with the `image1.png` a new
    /// `image1.gif` would sit beside and be mistaken for.
    taken: BTreeSet<String>,
    /// Extensions the new parts need, with the content type each declares.
    extensions: BTreeMap<String, String>,
}

impl<'a> Media<'a> {
    fn new(package: &Package, assets: &'a dyn AssetStore) -> Self {
        Media {
            assets,
            parts: BTreeMap::new(),
            added: BTreeMap::new(),
            taken: package.part_names().map(stem).collect(),
            extensions: BTreeMap::new(),
        }
    }

    /// The package part holding this image's bytes, minting one if needed.
    ///
    /// The bytes go in as the store holds them. They are the bytes that came
    /// out of the source document — the store keys on their hash — so nothing
    /// here decodes, scales or re-encodes a bitmap; the ZIP deflates the member
    /// like any other, which is lossless and reversible.
    fn part_for(&mut self, image: &ImageRef) -> Result<String, String> {
        let key = image.asset.as_str().to_string();
        if let Some(part) = self.parts.get(&key) {
            return Ok(part.clone());
        }
        let Some(bytes) = self.assets.get(&image.asset) else {
            return Err(format!(
                "its picture, whose bytes the asset store does not hold under `{key}`"
            ));
        };
        let info = self.assets.info(&image.asset);
        // The store sniffs the extension from the content rather than trusting
        // the source document's naming, which is exactly what a content type
        // has to agree with.
        let extension = info
            .and_then(|info| info.file_name.rsplit('.').next())
            .unwrap_or("bin")
            .to_ascii_lowercase();
        let content_type = info
            .map(|info| info.content_type.clone())
            .unwrap_or_else(|| "application/octet-stream".into());

        let mut n = 1;
        let part = loop {
            let candidate = format!("ppt/media/image{n}.{extension}");
            if !self.taken.contains(&stem(&candidate)) {
                break candidate;
            }
            n += 1;
        };
        self.taken.insert(stem(&part));
        self.parts.insert(key, part.clone());
        self.added.insert(part.clone(), bytes.to_vec());
        self.extensions.insert(extension, content_type);
        Ok(part)
    }
}

/// `[Content_Types].xml` with a `Default` for every extension the new media
/// needs and the package does not declare, or `None` when it declares them all.
///
/// A part with no content type is the one thing in an OPC package PowerPoint
/// refuses outright, and it is spliced rather than rebuilt for the same reason
/// everything else here is: the overrides a deck carries are its own.
fn content_types(
    package: &Package,
    extensions: &BTreeMap<String, String>,
    added: &BTreeMap<String, String>,
    removed: &BTreeSet<String>,
) -> Option<(String, String)> {
    const PART: &str = "[Content_Types].xml";
    if extensions.is_empty() && added.is_empty() && removed.is_empty() {
        return None;
    }
    let source = package.text(PART).ok()?;
    let root = Element::parse(PART, source.as_bytes()).ok()?;
    let declared: BTreeSet<String> = root
        .children_named("Default")
        .filter_map(|default| default.attr("Extension"))
        .map(str::to_ascii_lowercase)
        .collect();

    // A part that is gone takes its `Override` with it: an override naming a
    // part the package does not carry is exactly the kind of inconsistency an
    // OPC consumer is entitled to refuse.
    let mut edits: Vec<(Range<usize>, String)> = root
        .children_named("Override")
        .filter(|over| {
            over.attr("PartName")
                .and_then(|name| name.strip_prefix('/'))
                .is_some_and(|name| removed.contains(name))
        })
        .map(|over| (over.span.clone(), String::new()))
        .collect();

    let mut entries = String::new();
    for (extension, content_type) in extensions {
        if declared.contains(extension) {
            continue;
        }
        entries.push_str(&format!(
            "<Default Extension=\"{}\" ContentType=\"{}\"/>",
            esc_attr(extension),
            esc_attr(content_type)
        ));
    }
    // A part the package never had needs an `Override` naming it: a part with
    // no content type is the one thing an OPC consumer refuses outright.
    let stated: BTreeSet<&str> = root
        .children_named("Override")
        .filter_map(|over| over.attr("PartName"))
        .collect();
    for (part, content_type) in added {
        if content_type.is_empty() || stated.contains(format!("/{part}").as_str()) {
            continue;
        }
        entries.push_str(&format!(
            "<Override PartName=\"/{}\" ContentType=\"{}\"/>",
            esc_attr(part),
            esc_attr(content_type)
        ));
    }
    if !entries.is_empty() {
        let at = source.rfind("</Types>")?;
        edits.push((at..at, entries));
    }
    if edits.is_empty() {
        return None;
    }
    Some((PART.to_string(), splice(source, edits)))
}

/// Where an attribute lives inside an element's start tag.
enum Slot {
    /// The whole attribute, and the byte range of its value inside it.
    Value(Range<usize>, Range<usize>),
    /// Where a new attribute goes, just before the tag closes.
    Absent(usize),
}

/// Scans an element's start tag in the source it was parsed from.
///
/// The parser keeps an element's span but not its attributes', and this writer
/// needs the second: changing one attribute has to leave the order, the
/// spacing and the quoting of every other one exactly as it was.
fn attr_slot(source: &str, element: &Element, name: &str) -> Slot {
    let bytes = source.as_bytes();
    let end = element.span.end.min(bytes.len());
    let mut i = element.span.start + 1;
    while i < end && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' && bytes[i] != b'/' {
        i += 1;
    }
    loop {
        let gap = i;
        while i < end && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= end || bytes[i] == b'>' || bytes[i] == b'/' {
            return Slot::Absent(i.min(end));
        }
        let name_start = i;
        while i < end && bytes[i] != b'=' && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
            i += 1;
        }
        let found = &source[name_start..i];
        while i < end && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= end || bytes[i] != b'=' {
            return Slot::Absent(i.min(end));
        }
        i += 1;
        while i < end && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= end || (bytes[i] != b'"' && bytes[i] != b'\'') {
            return Slot::Absent(i.min(end));
        }
        let quote = bytes[i];
        i += 1;
        let value_start = i;
        while i < end && bytes[i] != quote {
            i += 1;
        }
        let value = value_start..i;
        i = (i + 1).min(end);
        let hit = if name.contains(':') {
            found == name
        } else {
            local_of(found) == name
        };
        if hit {
            return Slot::Value(gap..i, value);
        }
    }
}

/// Sets, replaces or removes one attribute of a start tag, as an edit.
fn set_attr(
    source: &str,
    element: &Element,
    name: &str,
    value: Option<&str>,
    edits: &mut Vec<(Range<usize>, String)>,
) {
    match (attr_slot(source, element, name), value) {
        (Slot::Value(_, range), Some(value)) => edits.push((range, esc_attr(value))),
        // Removing it takes its name, its quotes and the space before it.
        (Slot::Value(whole, _), None) => edits.push((whole, String::new())),
        (Slot::Absent(at), Some(value)) => {
            edits.push((at..at, format!(" {name}=\"{}\"", esc_attr(value))))
        }
        (Slot::Absent(_), None) => {}
    }
}

/// A package-absolute part name as a relationship writes it: relative to the
/// directory of the part that owns the relationship.
fn relative_target(base: &str, target: &str) -> String {
    let base: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    let target: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    let mut out = String::new();
    for _ in common..base.len() {
        out.push_str("../");
    }
    out.push_str(&target[common..].join("/"));
    out
}

/// A part name without its extension.
fn stem(part: &str) -> String {
    match part.rsplit_once('.') {
        Some((stem, extension)) if !extension.contains('/') => stem.to_string(),
        _ => part.to_string(),
    }
}

fn parent_dir(part: &str) -> String {
    match part.rfind('/') {
        Some(i) => part[..i].to_string(),
        None => String::new(),
    }
}

fn local_of(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

pub(super) fn esc_attr(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Copies an OPC package entry by entry, preserving what a rebuilt one loses.
///
/// Member order, compression method, CRC and timestamp all come from the source
/// entry, and the compressed bytes are copied as they stand: no entry is
/// inflated on the way through, so no entry can come out re-deflated by a
/// different encoder into different bytes.
///
/// One field the `zip` writer will not leave alone is the **external file
/// attributes** of each central-directory entry: it builds them from the source
/// entry's Unix mode but ORs `S_IFREG` in (`FileOptions::normalize`), so a deck
/// written by Python's `zipfile` with mode `0o600` would come back as
/// `0o100600`. Nothing in OPC reads that field, but the deck's *identity* does:
/// a preserved package is addressed by the content hash of its bytes
/// (`skeleton: assets/_skeleton/deck-<hash>.pptx`), so a permission bit that
/// moves renames the file and makes the round trip look like an edit. The
/// attributes are therefore put back as they were, entry by entry, once the
/// archive is built — see [`restore_external_attributes`].
fn reemit<W: Write + Seek>(
    package: &[u8],
    rebuild: &Rebuild,
    mut writer: W,
) -> Result<(), WriteError> {
    let mut source = zip::ZipArchive::new(Cursor::new(package))?;
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..source.len() {
        let entry = source.by_index_raw(index)?;
        if entry.is_dir() {
            // A directory member carries no content and no relationship; the
            // reader dropped it too.
            continue;
        }
        // A slide the model no longer holds takes its part, its relationships
        // and its notes page out of the archive with it. Leaving them behind
        // would leave the deleted slide's text in a file that says it is gone.
        if rebuild.removals.contains(entry.name()) {
            continue;
        }
        match rebuild.replacements.get(entry.name()) {
            // A rewritten part cannot be copied raw — its bytes are new — but
            // it keeps its neighbours' treatment: same position in the archive,
            // same compression method, same timestamp.
            Some(bytes) => {
                let options = entry.options();
                let name = entry.name().to_string();
                drop(entry);
                out.start_file(name, options)?;
                out.write_all(bytes)?;
            }
            None => out.raw_copy_file(entry)?,
        }
    }
    // A part the package never had — a picture swapped for one the deck did not
    // carry — goes on the end. OPC names its parts in `[Content_Types].xml` and
    // in the relationship graph, never by position, and a fixed timestamp keeps
    // two runs of the writer producing the same file.
    for (name, bytes) in &rebuild.additions {
        out.start_file(name.as_str(), added_options())?;
        out.write_all(bytes)?;
    }
    let mut bytes = out.finish()?.into_inner();
    restore_external_attributes(package, &mut bytes);
    writer.write_all(&bytes)?;
    Ok(())
}

/// Puts each entry's external file attributes back to what the source package
/// said, so a package copied through [`reemit`] is byte for byte the one that
/// was read.
///
/// The `zip` writer offers no way to state those four bytes verbatim — it ORs
/// `S_IFREG` into every mode it writes — and they live only in the central
/// directory, never in a local header, so the correction is a fixed-width
/// overwrite at a known offset of each record rather than a re-encode. A member
/// the source package did not have keeps what the writer chose for it.
///
/// A directory it cannot parse — a zip64 archive, an archive with a comment
/// whose bytes look like a header — is left exactly as the writer produced it:
/// this is a cosmetic correction, and a wrong guess at an offset would not be.
fn restore_external_attributes(source: &[u8], written: &mut [u8]) {
    let Some(before) = central_directory(source) else {
        return;
    };
    let Some(after) = central_directory(written) else {
        return;
    };
    let attributes: BTreeMap<&str, [u8; 4]> = before
        .iter()
        .map(|entry| (entry.name.as_str(), entry.external_attributes))
        .collect();
    for entry in after {
        if let Some(original) = attributes.get(entry.name.as_str()) {
            written[entry.attributes_at..entry.attributes_at + 4].copy_from_slice(original);
        }
    }
}

/// One central-directory record: the member's name, its external attributes and
/// where in the archive those four bytes are.
struct DirectoryEntry {
    name: String,
    external_attributes: [u8; 4],
    attributes_at: usize,
}

/// The central directory of a ZIP archive, read by walking it from the
/// end-of-central-directory record rather than by scanning for signatures — a
/// deflated part can contain any four bytes, including `PK\x01\x02`.
///
/// `None` for anything this simple reader does not fully understand: a
/// zip64 archive, a truncated one, a record that does not start where the
/// previous one ended.
fn central_directory(archive: &[u8]) -> Option<Vec<DirectoryEntry>> {
    const EOCD: [u8; 4] = [b'P', b'K', 5, 6];
    const RECORD: [u8; 4] = [b'P', b'K', 1, 2];
    // The end record is 22 bytes plus a comment of at most 64 KiB.
    let start = archive.len().checked_sub(22)?;
    let floor = start.saturating_sub(u16::MAX as usize);
    let eocd = (floor..=start).rev().find(|&i| archive[i..i + 4] == EOCD)?;
    let count = u16::from_le_bytes(archive[eocd + 10..eocd + 12].try_into().ok()?) as usize;
    let offset = u32::from_le_bytes(archive[eocd + 16..eocd + 20].try_into().ok()?) as usize;
    if offset == u32::MAX as usize || count == u16::MAX as usize {
        return None; // zip64: the real values are in a record this does not read
    }
    let mut entries = Vec::with_capacity(count);
    let mut at = offset;
    for _ in 0..count {
        if archive.len() < at + 46 || archive[at..at + 4] != RECORD {
            return None;
        }
        let name_len = u16::from_le_bytes(archive[at + 28..at + 30].try_into().ok()?) as usize;
        let extra_len = u16::from_le_bytes(archive[at + 30..at + 32].try_into().ok()?) as usize;
        let comment_len = u16::from_le_bytes(archive[at + 32..at + 34].try_into().ok()?) as usize;
        let name_at = at + 46;
        let name = std::str::from_utf8(archive.get(name_at..name_at + name_len)?)
            .ok()?
            .to_string();
        entries.push(DirectoryEntry {
            name,
            external_attributes: archive[at + 38..at + 42].try_into().ok()?,
            attributes_at: at + 38,
        });
        at = name_at + name_len + extra_len + comment_len;
    }
    Some(entries)
}

/// How a member the original package did not have is stored.
fn added_options() -> zip::write::SimpleFileOptions {
    zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(
            zip::DateTime::from_date_and_time(2020, 1, 1, 0, 0, 0)
                .unwrap_or_else(|_| zip::DateTime::default_for_write()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::presentation::Shape;
    use docsai_model::MemoryAssetStore;

    /// Every deck of the corpus, so a trait nobody thought about — a macro
    /// part, an embedded workbook, a diagram's four parts — is in the sample.
    fn corpus() -> Vec<std::path::PathBuf> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/pptx");
        let mut decks: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|p| {
                matches!(
                    p.extension().and_then(|e| e.to_str()),
                    Some("pptx") | Some("pptm")
                )
            })
            .collect();
        decks.sort();
        assert!(decks.len() >= 17, "the pptx corpus lost fixtures");
        decks
    }

    fn deck(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/pptx")
            .join(name)
    }

    fn presentation(document: &Document) -> &Presentation {
        match document {
            Document::Presentation(deck) => deck,
            _ => panic!("not a deck"),
        }
    }

    fn presentation_mut(document: &mut Document) -> &mut Presentation {
        match document {
            Document::Presentation(deck) => deck,
            _ => panic!("not a deck"),
        }
    }

    /// The written bytes read back the way a consumer would.
    fn read_bytes(package: &[u8]) -> (Presentation, ConversionReport) {
        let mut assets = MemoryAssetStore::new();
        let (document, report) =
            super::super::read(Cursor::new(package.to_vec()), &mut assets).expect("reads it back");
        match document {
            Document::Presentation(deck) => (deck, report),
            _ => panic!("it came back as something that is not a deck"),
        }
    }

    /// The first paragraph of the first shape that has one.
    fn first_paragraph(slide: &mut Slide) -> Option<&mut docsai_model::text::Paragraph> {
        slide.shapes.iter_mut().find_map(|shape| {
            let body = match &mut shape.kind {
                ShapeKind::Placeholder(ph) => &mut ph.body,
                ShapeKind::TextBox { body } => body,
                _ => return None,
            };
            body.iter_mut().find_map(|block| match block {
                docsai_model::text::Block::Paragraph(paragraph) => Some(paragraph),
                _ => None,
            })
        })
    }

    fn set_first_text(document: &mut Document, text: &str) {
        let deck = presentation_mut(document);
        let paragraph = first_paragraph(&mut deck.slides[0]).expect("a paragraph to retype");
        paragraph.content = vec![docsai_model::text::Inline::Text(text.into())];
    }

    /// The first picture of the first slide.
    fn picture_mut(document: &mut Document) -> &mut docsai_model::image::ImageRef {
        presentation_mut(document).slides[0]
            .shapes
            .iter_mut()
            .find_map(|shape| match &mut shape.kind {
                ShapeKind::Picture(image) => Some(image),
                _ => None,
            })
            .expect("the fixture has a picture")
    }

    /// The first table of the first slide.
    fn table_mut(document: &mut Document) -> &mut Table {
        presentation_mut(document).slides[0]
            .shapes
            .iter_mut()
            .find_map(|shape| match &mut shape.kind {
                ShapeKind::Table(table) => Some(table),
                _ => None,
            })
            .expect("the fixture has a table")
    }

    fn count_items(slide: &Slide) -> usize {
        fn count(blocks: &[docsai_model::text::Block]) -> usize {
            blocks
                .iter()
                .map(|block| match block {
                    docsai_model::text::Block::List(list) => {
                        list.items.len()
                            + list
                                .items
                                .iter()
                                .map(|item| count(&item.blocks))
                                .sum::<usize>()
                    }
                    _ => 0,
                })
                .sum()
        }
        slide
            .shapes
            .iter()
            .map(|shape| match &shape.kind {
                ShapeKind::Placeholder(ph) => count(&ph.body),
                ShapeKind::TextBox { body } => count(body),
                _ => 0,
            })
            .sum()
    }

    /// One part of a package, decompressed.
    fn part_of(package: &[u8], part: &str) -> Option<Vec<u8>> {
        use std::io::Read;
        let mut zip = zip::ZipArchive::new(Cursor::new(package)).ok()?;
        let mut bytes = Vec::new();
        zip.by_name(part).ok()?.read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }

    /// Every `ppt/slides/*.xml` of a package, with its bytes.
    fn slide_parts_of(package: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut zip = zip::ZipArchive::new(Cursor::new(package)).expect("a readable package");
        let names: Vec<String> = (0..zip.len())
            .filter_map(|i| {
                let entry = zip.by_index_raw(i).ok()?;
                let name = entry.name().to_string();
                (name.starts_with("ppt/slides/") && name.ends_with(".xml")).then_some(name)
            })
            .collect();
        names
            .into_iter()
            .map(|name| {
                let bytes = part_of(package, &name).expect("the part is readable");
                (name, bytes)
            })
            .collect()
    }

    fn read_deck(path: &std::path::Path) -> (Document, MemoryAssetStore) {
        let mut assets = MemoryAssetStore::new();
        let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let (document, _) = super::super::read(file, &mut assets)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        (document, assets)
    }

    fn write_deck(document: &Document, assets: &dyn AssetStore) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        write_pptx(document, assets, &mut out).expect("writes the deck");
        out.into_inner()
    }

    /// Every member, in order, with what the archive says about it.
    fn members(package: &[u8]) -> Vec<(String, zip::CompressionMethod, u32, u64)> {
        let mut zip = zip::ZipArchive::new(Cursor::new(package)).expect("a readable package");
        (0..zip.len())
            .filter_map(|i| {
                let entry = zip.by_index_raw(i).expect("a readable member");
                (!entry.is_dir()).then(|| {
                    (
                        entry.name().to_string(),
                        entry.compression(),
                        entry.crc32(),
                        entry.size(),
                    )
                })
            })
            .collect()
    }

    /// 15-H: a deck whose z-order and reading order disagree is written back
    /// into the *elements the shapes came from*, not into the tree positions
    /// their reading order happens to have.
    ///
    /// This is what a model that travelled through DocMark looks like: the
    /// parser numbers the shapes by the position they were written at, because
    /// DocMark-P writes them in reading order and says nothing about z-order.
    /// Before `order::of_tree` the writer took that number for a tree index and
    /// put every shape's text into a different shape's box — silently, and only
    /// on a deck whose two orders differ, which is why an idempotence run over
    /// the corpus is what found it.
    #[test]
    fn a_deck_renumbered_by_the_docmark_parser_still_lands_in_the_right_shapes() {
        let path = deck("reading-order.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (mut document, assets) = read_deck(&path);
        let source: Vec<u32> = presentation(&document).slides[0]
            .shapes
            .iter()
            .map(|shape| shape.z_index)
            .collect();
        assert_ne!(
            source,
            (0..source.len() as u32).collect::<Vec<_>>(),
            "the fixture is meant to have a reading order that is not its z-order"
        );
        for (position, shape) in presentation_mut(&mut document).slides[0]
            .shapes
            .iter_mut()
            .enumerate()
        {
            shape.z_index = position as u32;
        }
        assert!(
            write_deck(&document, &assets) == original,
            "the renumbered deck did not come back as the deck that was read"
        );
    }

    /// The claim of the increment, over the whole corpus: what goes out is what
    /// came in — not «the same parts», the same archive.
    #[test]
    fn a_written_deck_is_the_package_that_was_read() {
        for path in corpus() {
            let original = std::fs::read(&path).expect("reads the deck");
            let (document, assets) = read_deck(&path);
            let written = write_deck(&document, &assets);

            let name = path.file_name().unwrap().to_string_lossy();
            assert_eq!(
                members(&written),
                members(&original),
                "{name}: the archive is not the one that was read"
            );
            // Byte for byte, not merely member for member: the deck's own
            // identity is the content hash of these bytes, so a field that
            // drifts renames the preserved package and makes a round trip look
            // like an edit (15-H). The external attributes `zip` insists on
            // rewriting are put back by `reemit`.
            assert!(
                written == original,
                "{name}: the package is not the one that was read"
            );
        }
    }

    /// The parts themselves, decompressed, over a deck that carries the traits
    /// a rebuilt package silently loses: a chart's embedded workbook, a
    /// diagram's `dgm:` parts.
    #[test]
    fn every_part_survives_byte_for_byte() {
        for name in ["charts-embedded", "smartart-fallback", "macro-enabled"] {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/pptx");
            let path = if name == "macro-enabled" {
                dir.join(format!("{name}.pptm"))
            } else {
                dir.join(format!("{name}.pptx"))
            };
            let original = std::fs::read(&path).expect("reads the deck");
            let (document, assets) = read_deck(&path);
            let written = write_deck(&document, &assets);

            let mut before = zip::ZipArchive::new(Cursor::new(&original)).expect("original");
            let mut after = zip::ZipArchive::new(Cursor::new(&written)).expect("written");
            assert_eq!(before.len(), after.len(), "{name}: the member count moved");
            for i in 0..before.len() {
                use std::io::Read;
                let mut a = Vec::new();
                let mut b = Vec::new();
                let part = {
                    let mut entry = before.by_index(i).expect("a member");
                    entry.read_to_end(&mut a).expect("reads it");
                    entry.name().to_string()
                };
                after
                    .by_name(&part)
                    .unwrap_or_else(|e| panic!("{name}: `{part}` is not in the written deck: {e}"))
                    .read_to_end(&mut b)
                    .expect("reads it back");
                assert!(a == b, "{name}: `{part}` came out different");
            }
        }
    }

    /// The rewriter is a no-op on a deck nobody edited.
    ///
    /// The strongest thing 15-B can claim, and the one that keeps a conversion
    /// from being a diff: rewriting a text body from the model produces the
    /// bytes that were already in the part, so a slide nobody edited is not
    /// merely equivalent — it is the same XML. The two tests above cover the
    /// rest of the package; this one is about the parts the writer *does* touch.
    #[test]
    fn an_unedited_slide_is_rewritten_into_the_same_bytes() {
        for path in corpus() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let original = std::fs::read(&path).expect("reads the deck");
            let (document, assets) = read_deck(&path);
            let written = write_deck(&document, &assets);
            for (part, bytes) in slide_parts_of(&original) {
                assert!(
                    part_of(&written, &part) == Some(bytes),
                    "{name}: `{part}` came out different from an untouched model"
                );
            }
        }
    }

    /// The claim of 15-B: an edit to the model is in the file.
    #[test]
    fn a_retitled_slide_reaches_the_written_deck() {
        let path = deck("basic-slides.pptx");
        let (mut document, assets) = read_deck(&path);
        let before = presentation(&document).clone();
        set_first_text(&mut document, "Título reescrito");

        let written = write_deck(&document, &assets);
        let (after, _) = read_bytes(&written);

        assert_eq!(
            after.slides[0].title().as_deref(),
            Some("Título reescrito"),
            "the new title is not in the written deck"
        );
        assert_ne!(before.slides[0].title(), after.slides[0].title());
        // The slide nobody touched is the slide that was read, shape for shape.
        assert_eq!(
            after.slides[1..],
            before.slides[1..],
            "a slide nobody edited changed"
        );
    }

    /// A bullet added to a body placeholder comes back as a bullet, at the
    /// level it was given — the bullet the master supplies is inherited, so
    /// what is written is a paragraph with no bullet element at all.
    #[test]
    fn an_added_bullet_comes_back_as_a_bullet() {
        let path = deck("bullets-levels.pptx");
        let (mut document, assets) = read_deck(&path);
        let before = count_items(&presentation(&document).slides[0]);

        {
            let deck = presentation_mut(&mut document);
            let list = deck.slides[0]
                .shapes
                .iter_mut()
                .find_map(|shape| match &mut shape.kind {
                    ShapeKind::Placeholder(ph) if ph.ph_type.is_body() => {
                        ph.body.iter_mut().find_map(|block| match block {
                            docsai_model::text::Block::List(list) => Some(list),
                            _ => None,
                        })
                    }
                    _ => None,
                })
                .expect("the fixture has a bulleted body");
            list.items.push(docsai_model::text::ListItem {
                blocks: vec![docsai_model::text::Block::Paragraph(
                    docsai_model::text::Paragraph::text("Punto añadido"),
                )],
            });
        }

        let written = write_deck(&document, &assets);
        let (after, _) = read_bytes(&written);
        assert_eq!(count_items(&after.slides[0]), before + 1);
        let text = format!("{:?}", after.slides[0]);
        assert!(text.contains("Punto añadido"), "the bullet is not there");
    }

    /// A run's formatting is a delta over the cascade on the way in and a
    /// direct property on the way out, and the two have to agree: what is read
    /// back is what was set, not what the layout would have supplied.
    #[test]
    fn run_formatting_survives_the_round_trip() {
        use docsai_model::style::FontProps;
        let path = deck("basic-slides.pptx");
        let (mut document, assets) = read_deck(&path);
        {
            let deck = presentation_mut(&mut document);
            let paragraph = first_paragraph(&mut deck.slides[0]).expect("a paragraph");
            paragraph.content = vec![docsai_model::text::Inline::Styled {
                content: vec![docsai_model::text::Inline::Text("Con formato".into())],
                props: docsai_model::text::RunProps::direct(FontProps {
                    bold: Some(true),
                    italic: Some(true),
                    size: Some(docsai_model::units::Length::from_pt(28.0)),
                    color: Some("#c00000".into()),
                    ..FontProps::default()
                }),
            }];
        }

        let written = write_deck(&document, &assets);
        let (after, _) = read_bytes(&written);
        let paragraph = format!("{:?}", after.slides[0]);
        assert!(paragraph.contains("Con formato"));
        assert!(paragraph.contains("bold: Some(true)"), "{paragraph}");
        assert!(paragraph.contains("italic: Some(true)"), "{paragraph}");
        assert!(paragraph.contains("#c00000"), "{paragraph}");
    }

    /// 15-C: the hyperlink reaches the file, and so does the relationship.
    ///
    /// The link is not an element of its own in DrawingML — it is an `r:id` on
    /// the run — so «the link was written» means two parts agreeing: the slide
    /// names an id, and the slide's `_rels` says what that id points at. The
    /// relationships the deck already had keep their bytes: the part is
    /// spliced, not regenerated.
    #[test]
    fn a_link_reaches_the_written_deck_with_its_relationship() {
        let path = deck("basic-slides.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (mut document, assets) = read_deck(&path);
        {
            let deck = presentation_mut(&mut document);
            let paragraph = first_paragraph(&mut deck.slides[0]).expect("a paragraph");
            paragraph.content = vec![docsai_model::text::Inline::Link {
                target: "https://example.org".into(),
                content: vec![docsai_model::text::Inline::Text("un enlace".into())],
                props: Default::default(),
            }];
        }

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            !report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("example.org"))
            }),
            "the link is reported as lost and it was written: {:?}",
            report.warnings
        );
        let (after, _) = read_bytes(&written);
        let text = format!("{:?}", after.slides[0]);
        assert!(
            text.contains("un enlace") && text.contains("https://example.org"),
            "the link did not come back: {text}"
        );

        let part = "ppt/slides/_rels/slide1.xml.rels";
        let before = String::from_utf8(part_of(&original, part).expect("the rels part")).unwrap();
        let now = String::from_utf8(part_of(&written, part).expect("the rels part")).unwrap();
        assert!(
            now.contains("TargetMode=\"External\"") && now.contains("https://example.org"),
            "the relationship is not in the part: {now}"
        );
        let head = before.trim_end().trim_end_matches("</Relationships>");
        assert!(
            now.starts_with(head),
            "the relationships the deck already had were rewritten: {now}"
        );
    }

    /// A target no relationship can state as it stands is still refused.
    ///
    /// An internal link points at a *part*, through a `slide`-typed
    /// relationship and an `action` attribute the IR does not record. 15-E
    /// knows which part every slide is and still refuses it, because the
    /// missing half is in the model. The wrong answer is an `r:id` pointing at
    /// nothing, which is precisely the dangling relationship spike P3 measured
    /// LibreOffice accepting in silence.
    #[test]
    fn a_link_with_no_relationship_to_make_is_still_refused() {
        let path = deck("basic-slides.pptx");
        let (mut document, assets) = read_deck(&path);
        {
            let deck = presentation_mut(&mut document);
            let paragraph = first_paragraph(&mut deck.slides[0]).expect("a paragraph");
            paragraph.content = vec![docsai_model::text::Inline::Link {
                target: "otra-presentacion.pptx".into(),
                content: vec![docsai_model::text::Inline::Text("un enlace".into())],
                props: Default::default(),
            }];
        }

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        assert!(
            report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("otra-presentacion"))
            }),
            "the link was dropped without a word: {:?}",
            report.warnings
        );
    }

    /// 15-C, the claim that decides the increment: the bytes that came out of
    /// the store are the bytes that go into the package.
    ///
    /// A new extension brings a new `Default` with it, because a part with no
    /// content type is the one thing an OPC consumer refuses outright — and the
    /// media part is compared byte for byte, so nothing here re-encoded a
    /// bitmap on the way through.
    #[test]
    fn a_swapped_image_goes_in_as_its_own_bytes_and_is_declared() {
        // A 1×1 transparent GIF: a real file, and an extension this deck has
        // never carried.
        const TINY_GIF: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff\
                                  !\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\
                                  \x00\x00\x02\x02D\x01\x00;";
        let path = deck("images-anchored.pptx");
        let (mut document, mut assets) = read_deck(&path);
        let swapped = assets.put(TINY_GIF).expect("stores the image");
        picture_mut(&mut document).asset = swapped;

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        let part = "ppt/media/image2.gif";
        assert_eq!(
            part_of(&written, part).as_deref(),
            Some(TINY_GIF),
            "the image is not in the package as the store holds it"
        );
        let types = String::from_utf8(part_of(&written, "[Content_Types].xml").expect("types"))
            .expect("utf-8");
        assert!(
            types.contains("Extension=\"gif\"") && types.contains("image/gif"),
            "the new extension is not declared: {types}"
        );
        // The old image is still there: nothing in this writer deletes a part.
        assert!(part_of(&written, "ppt/media/image1.png").is_some());

        let (after, _) = read_bytes(&written);
        let ShapeKind::Picture(image) = &after.slides[0].shapes[1].kind else {
            panic!("the picture did not survive the swap");
        };
        let mut store = MemoryAssetStore::new();
        let expected = store.put(TINY_GIF).expect("stores it");
        assert_eq!(image.asset, expected, "the deck points at the other image");
    }

    /// The alt text is what a reader of the Markdown edits, and it is an
    /// attribute of the picture rather than anything textual.
    #[test]
    fn a_retyped_alt_text_reaches_the_picture() {
        let path = deck("images-anchored.pptx");
        let (mut document, assets) = read_deck(&path);
        picture_mut(&mut document).alt = "Otro texto alternativo".into();

        let written = write_deck(&document, &assets);
        let (after, _) = read_bytes(&written);
        let ShapeKind::Picture(image) = &after.slides[0].shapes[1].kind else {
            panic!("the picture is gone");
        };
        assert_eq!(image.alt, "Otro texto alternativo");
        // One attribute moved, and nothing else about the picture did.
        assert_eq!(
            image.geometry.display_size,
            docsai_model::units::Size::new(Length::from_emu(1_143_000), Length::from_emu(857_250))
        );
    }

    /// A picture nobody edited is not a loss, and must not be reported as one.
    #[test]
    fn a_picture_nobody_edited_is_not_reported() {
        let path = deck("images-anchored.pptx");
        let (mut document, assets) = read_deck(&path);
        set_first_text(&mut document, "Otro título");

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            report.warnings.is_empty(),
            "an untouched picture was reported: {:?}",
            report.warnings
        );
        let (after, _) = read_bytes(&written);
        assert_eq!(after.slides[0].title().as_deref(), Some("Otro título"));
        assert!(
            after.slides[0]
                .shapes
                .iter()
                .any(|shape| matches!(shape.kind, ShapeKind::Picture(_))),
            "the picture did not survive the rewrite"
        );
    }

    /// A table's cells are text bodies like any other, and the grid around them
    /// is the bytes that were read.
    #[test]
    fn a_retyped_table_cell_reaches_the_written_deck() {
        let path = deck("tables-simple.pptx");
        let (mut document, assets) = read_deck(&path);
        {
            let table = table_mut(&mut document);
            table.rows[1].cells[0].blocks = vec![docsai_model::text::Block::Paragraph(
                docsai_model::text::Paragraph::text("Suroeste"),
            )];
        }

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        let (mut after, _) = read_bytes(&written);
        let ShapeKind::Table(table) = &mut after.slides[0].shapes[1].kind else {
            panic!("the table is gone");
        };
        let text = format!("{:?}", table.rows[1].cells[0]);
        assert!(
            text.contains("Suroeste"),
            "the cell was not rewritten: {text}"
        );
        // The row nobody touched came back as it was.
        let other = format!("{:?}", table.rows[1].cells[1]);
        assert!(other.contains("1 200"), "{other}");
    }

    /// A grid the model moved is a table this writer cannot splice, so it keeps
    /// the one that was read, and says the grid is what could not be written.
    #[test]
    fn a_table_whose_grid_moved_is_kept_and_named() {
        let path = deck("tables-simple.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (mut document, assets) = read_deck(&path);
        table_mut(&mut document).rows.pop();

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("whose grid the model moved"))
            }),
            "the report does not name what was kept: {:?}",
            report.warnings
        );
        for (part, bytes) in slide_parts_of(&original) {
            assert_eq!(
                part_of(&written, &part),
                Some(bytes),
                "`{part}` was rewritten around the table anyway"
            );
        }
    }

    /// Retypes the first paragraph of one shape of the first slide, by index in
    /// the reader's order — the title is 0 and the body 1 in `autofit-stale`.
    fn retype(document: &mut Document, shape: usize, text: &str) {
        let deck = presentation_mut(document);
        let body = match &mut deck.slides[0].shapes[shape].kind {
            ShapeKind::Placeholder(ph) => &mut ph.body,
            ShapeKind::TextBox { body } => body,
            other => panic!("shape {shape} is a {}", shape_noun(other)),
        };
        let paragraph = body
            .iter_mut()
            .find_map(|block| match block {
                docsai_model::text::Block::Paragraph(paragraph) => Some(paragraph),
                docsai_model::text::Block::List(list) => list.items.first_mut().and_then(|item| {
                    item.blocks.iter_mut().find_map(|block| match block {
                        docsai_model::text::Block::Paragraph(paragraph) => Some(paragraph),
                        _ => None,
                    })
                }),
                _ => None,
            })
            .expect("a paragraph to retype");
        paragraph.content = vec![docsai_model::text::Inline::Text(text.into())];
    }

    fn stale_warnings(report: &ConversionReport) -> Vec<(String, u32)> {
        report
            .warnings
            .iter()
            .filter_map(|warning| match warning {
                Warning::AutofitStale { what, scale } => Some((what.clone(), *scale)),
                _ => None,
            })
            .collect()
    }

    /// The claim of 15-F: a scale computed for text that is no longer there is
    /// a lie no reader can detect, so the text that changed takes it with it.
    #[test]
    fn a_changed_body_drops_the_scale_that_was_computed_for_its_old_text() {
        let path = deck("autofit-stale.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (mut document, assets) = read_deck(&path);
        retype(&mut document, 1, "Una línea que nadie midió");

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert_eq!(
            stale_warnings(&report),
            vec![(
                "the text of `Content Placeholder 2` on `ppt/slides/slide1.xml` \
                 changed, so the writer dropped"
                    .to_string(),
                62_500
            )],
            "{:?}",
            report.warnings
        );
        let part =
            String::from_utf8(part_of(&written, "ppt/slides/slide1.xml").expect("the slide"))
                .expect("utf-8");
        assert!(
            part.contains("<a:normAutofit/>"),
            "the scale and its line-space reduction are still in the part: {part}"
        );
        assert!(!part.contains("fontScale") && !part.contains("lnSpcReduction"));
        assert!(part.contains("Una línea que nadie midió"), "{part}");
        // Nothing else moved: the title's shape is the bytes that were read,
        // `a:bodyPr` and all, and so is everything up to the body's own text.
        let before = String::from_utf8(
            part_of(&original, "ppt/slides/slide1.xml").expect("the slide it was read from"),
        )
        .expect("utf-8");
        let title = before
            .split_once("<p:sp><p:nvSpPr><p:cNvPr id=\"2\"")
            .expect("the title shape")
            .1;
        let title = &title[..title.find("</p:sp>").expect("its end")];
        assert!(part.contains(title), "the title shape was rewritten too");
        // And the model that comes back no longer states a scale at all.
        let (after, _) = read_bytes(&written);
        assert_eq!(
            after.slides[0].shapes[1]
                .placeholder()
                .expect("the body")
                .delta
                .font_scale,
            None
        );
    }

    /// The interaction that decides the increment: the attribute sits on
    /// `a:bodyPr`, inside the very `p:txBody` whose paragraphs are being
    /// replaced, and [`splice`] drops an overlapping edit **without a word**.
    /// So the two edits have to be disjoint, and the proof is that both of them
    /// are in the file rather than one of them.
    #[test]
    fn dropping_the_scale_does_not_overlap_the_paragraphs_it_was_computed_for() {
        let path = deck("autofit-stale.pptx");
        let (mut document, assets) = read_deck(&path);
        retype(&mut document, 1, "Sólo una línea");

        let written = write_deck(&document, &assets);
        let (after, report) = read_bytes(&written);
        let body = after.slides[0].shapes[1].placeholder().expect("the body");

        assert_eq!(body.delta.font_scale, None, "the scale survived the splice");
        assert!(
            format!("{:?}", body.body).contains("Sólo una línea"),
            "the paragraph edit was swallowed by the attribute edit: {:?}",
            body.body
        );
        // Reading the written deck back must not report a scale either: there
        // is none left to be stale.
        assert!(stale_warnings(&report).is_empty(), "{:?}", report.warnings);
    }

    /// A shape whose text changed and that carries no scale: no edit to drop,
    /// and nothing to report. The corpus is almost entirely this case.
    #[test]
    fn a_changed_body_with_no_scale_produces_no_edit_and_no_warning() {
        let path = deck("basic-slides.pptx");
        let (mut document, assets) = read_deck(&path);
        set_first_text(&mut document, "Título reescrito");

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(stale_warnings(&report).is_empty(), "{:?}", report.warnings);
        let part =
            String::from_utf8(part_of(&written, "ppt/slides/slide1.xml").expect("the slide"))
                .expect("utf-8");
        assert!(
            !part.contains("normAutofit"),
            "the writer invented an autofit element: {part}"
        );
    }

    /// The false-warning trap, and the one §7 rule 3 calls the same failure as
    /// a missing warning: the *other* shape's text changed, so this one's scale
    /// is exactly as valid as it was. It stays, and nothing is reported about
    /// it — the writing side, that is; reading the file back still says the
    /// scale is a number a human computed, which it is.
    #[test]
    fn a_scale_whose_own_text_did_not_change_is_left_alone() {
        let path = deck("autofit-stale.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (mut document, assets) = read_deck(&path);
        retype(&mut document, 0, "Otro título");

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(stale_warnings(&report).is_empty(), "{:?}", report.warnings);
        let part =
            String::from_utf8(part_of(&written, "ppt/slides/slide1.xml").expect("the slide"))
                .expect("utf-8");
        assert!(
            part.contains(r#"<a:normAutofit fontScale="62500" lnSpcReduction="20000"/>"#),
            "a scale nobody invalidated was dropped: {part}"
        );
        // The whole body shape is still the bytes that were read.
        let before = String::from_utf8(
            part_of(&original, "ppt/slides/slide1.xml").expect("the slide it was read from"),
        )
        .expect("utf-8");
        let body = before
            .split_once("<p:sp><p:nvSpPr><p:cNvPr id=\"3\"")
            .expect("the body shape")
            .1;
        let body = &body[..body.find("</p:sp>").expect("its end")];
        assert!(part.contains(body), "the body shape was rewritten anyway");
    }

    /// A [`DeckParts`] built from a presentation part written by hand.
    ///
    /// No corpus deck carries `p14:sectionLst` — PowerPoint writes it and the
    /// generated corpus does not — and the restatement is a pure function of
    /// the presentation part, so it is tested as one, exactly as the reader
    /// tests `read_sections`.
    fn deck_parts_from(xml: &str) -> DeckParts {
        let root = Element::parse("ppt/presentation.xml", xml.as_bytes()).expect("parses");
        let entries = root
            .child("sldIdLst")
            .into_iter()
            .flat_map(|list| list.children_named("sldId"))
            .enumerate()
            .map(|(i, entry)| SlideEntry {
                id: entry.attr_i64("id").unwrap_or_default(),
                rel: super::super::rel_id(entry).unwrap_or_default().to_string(),
                part: format!("ppt/slides/slide{}.xml", i + 1),
                span: entry.span.clone(),
                parsed: None,
            })
            .collect();
        DeckParts {
            main: "ppt/presentation.xml".into(),
            source: xml.to_string(),
            root,
            entries,
            unresolved: 0,
        }
    }

    fn slide_in(section: Option<&str>) -> Slide {
        Slide {
            section: section.map(str::to_string),
            ..Default::default()
        }
    }

    const SECTIONED: &str = r#"<p:presentation xmlns:p="urn:p" xmlns:p14="urn:p14" xmlns:r="urn:r"><p:sldIdLst><p:sldId id="256" r:id="rId2"/><p:sldId id="257" r:id="rId3"/><p:sldId id="258" r:id="rId4"/></p:sldIdLst><p:extLst><p:ext uri="{521415D9}"><p14:sectionLst><p14:section name="Intro" id="{1}"><p14:sldIdLst><p14:sldId id="256"/></p14:sldIdLst></p14:section><p14:section name="Cifras" id="{2}"><p14:sldIdLst><p14:sldId id="257"/><p14:sldId id="258"/></p14:sldIdLst></p14:section></p14:sectionLst></p:ext></p:extLst></p:presentation>"#;

    /// A deck whose sections still say what the model says is not edited at
    /// all — the property that keeps `ppt/presentation.xml` byte for byte.
    #[test]
    fn sections_that_did_not_move_are_not_rewritten() {
        let parts = deck_parts_from(SECTIONED);
        let deck = Presentation {
            slides: vec![
                slide_in(Some("Intro")),
                slide_in(Some("Cifras")),
                slide_in(Some("Cifras")),
            ],
            ..Default::default()
        };
        let mut kept = Vec::new();
        let edits = section_edits(&parts, &deck, &[0, 1, 2], &mut kept);
        assert!(
            edits.is_empty(),
            "an unmoved section was rewritten: {edits:?}"
        );
        assert!(kept.is_empty(), "{kept:?}");
    }

    /// A slide that changed section, and one that was removed: each section
    /// lists the slides the model puts in it, by the deck ids they already had.
    #[test]
    fn sections_follow_the_slides_that_moved() {
        let parts = deck_parts_from(SECTIONED);
        // Slide 258 moves to «Intro», 257 is gone, and the two that are left
        // are written in the model's order.
        let deck = Presentation {
            slides: vec![slide_in(Some("Intro")), slide_in(Some("Intro"))],
            ..Default::default()
        };
        let mut kept = Vec::new();
        let edits = section_edits(&parts, &deck, &[0, 2], &mut kept);
        assert!(kept.is_empty(), "{kept:?}");
        let restated = splice(&parts.source, edits);
        assert!(
            restated.contains(
                r#"<p14:section name="Intro" id="{1}"><p14:sldIdLst><p14:sldId id="256"/><p14:sldId id="258"/></p14:sldIdLst>"#
            ),
            "the section that gained a slide: {restated}"
        );
        assert!(
            restated.contains(r#"<p14:section name="Cifras" id="{2}"><p14:sldIdLst/>"#),
            "the section that lost every slide: {restated}"
        );
        // The section keeps its name and its GUID: nothing here invents one.
        assert!(restated.contains(r#"id="{2}""#));
    }

    /// A section the deck does not have is not created: the `p14:section` needs
    /// a GUID nothing recorded, and inventing identity is not this writer's.
    ///
    /// The slide stays where the package puts it, and so does the slide the
    /// model states no section for at all — a model that does not state
    /// something is not a model asking for it to be removed, which is the rule
    /// the rest of this writer follows and the reason an unedited deck with
    /// sections produces no edit and no warning.
    #[test]
    fn a_section_the_deck_does_not_have_is_named_rather_than_created() {
        let parts = deck_parts_from(SECTIONED);
        let deck = Presentation {
            slides: vec![
                slide_in(Some("Intro")),
                slide_in(Some("Anexos")),
                slide_in(None),
            ],
            ..Default::default()
        };
        let mut kept = Vec::new();
        let edits = section_edits(&parts, &deck, &[0, 1, 2], &mut kept);
        let said = kept.join(" ");
        assert!(said.contains("`Anexos`"), "{said}");
        assert!(
            !said.contains("slide 3"),
            "a slide the model says nothing about was reported as a loss: {said}"
        );
        assert!(
            edits.is_empty(),
            "the slides stayed where the deck puts them, so nothing is rewritten: {edits:?}"
        );
    }

    /// A package built by hand, for the two refusals that no corpus deck can
    /// state: a custom show, and a part pointing at a slide that would go.
    fn package_with(parts: &[(&str, &str)]) -> Package {
        let mut package = Package::new();
        for (name, body) in parts {
            package.insert(*name, body.as_bytes().to_vec());
        }
        package
    }

    const DECK_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide2.xml"/></Relationships>"#;

    /// A slide another part points at is not removed.
    ///
    /// A relationship left pointing at a part that is gone is the dangling
    /// relationship spike P3 §2.4 measured LibreOffice accepting in silence,
    /// and it is a repair dialog in PowerPoint — the failure mode this phase
    /// is measured on. Refusing the removal loses a deleted slide; performing
    /// it can lose the deck.
    #[test]
    fn a_slide_another_part_points_at_is_not_removed() {
        let link = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slide2.xml"/></Relationships>"#;
        let package = package_with(&[
            ("ppt/_rels/presentation.xml.rels", DECK_RELS),
            ("ppt/slides/slide1.xml", "<p:sld/>"),
            ("ppt/slides/slide2.xml", "<p:sld/>"),
            ("ppt/slides/_rels/slide1.xml.rels", link),
        ]);
        let parts = deck_parts_from(SECTIONED);
        let deck = Presentation {
            slides: vec![slide_in(None)],
            ..Default::default()
        };
        let error = restate_deck(&package, &parts, &deck, &[0]).expect_err("refuses");
        assert!(error.contains("ppt/slides/slide2.xml"), "{error}");
        assert!(error.contains("slide1.xml"), "{error}");
    }

    /// A deck with custom shows keeps every slide it has.
    #[test]
    fn a_deck_with_custom_shows_does_not_lose_a_slide_behind_their_back() {
        let package = package_with(&[
            ("ppt/_rels/presentation.xml.rels", DECK_RELS),
            ("ppt/slides/slide1.xml", "<p:sld/>"),
            ("ppt/slides/slide2.xml", "<p:sld/>"),
        ]);
        let xml = SECTIONED.replace("<p:extLst>", "<p:custShowLst/><p:extLst>");
        let parts = deck_parts_from(&xml);
        let deck = Presentation {
            slides: vec![slide_in(None)],
            ..Default::default()
        };
        let error = restate_deck(&package, &parts, &deck, &[0]).expect_err("refuses");
        assert!(error.contains("custom shows"), "{error}");
    }

    /// Every `ppt/notesSlides/*.xml` of a package, with its bytes.
    fn notes_parts_of(package: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut zip = zip::ZipArchive::new(Cursor::new(package)).expect("a readable package");
        let names: Vec<String> = (0..zip.len())
            .filter_map(|i| {
                let entry = zip.by_index_raw(i).ok()?;
                let name = entry.name().to_string();
                (name.starts_with("ppt/notesSlides/") && name.ends_with(".xml")).then_some(name)
            })
            .collect();
        names
            .into_iter()
            .map(|name| {
                let bytes = part_of(package, &name).expect("a readable part");
                (name, bytes)
            })
            .collect()
    }

    fn note_text(slide: &Slide) -> String {
        slide
            .notes
            .iter()
            .flatten()
            .map(|block| match block {
                docsai_model::text::Block::Paragraph(p) => p.plain_text(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn set_notes(slide: &mut Slide, text: &str) {
        slide.notes = Some(vec![docsai_model::text::Block::Paragraph(
            docsai_model::text::Paragraph {
                content: vec![docsai_model::text::Inline::Text(text.into())],
                ..Default::default()
            },
        )]);
    }

    /// 15-E, and the same claim 15-B made for a slide: rewriting a notes page
    /// from the model produces the bytes that were already in the part.
    ///
    /// It is the notes counterpart of
    /// `an_unedited_slide_is_rewritten_into_the_same_bytes`, and the reason the
    /// corpus check above still passes now that a second kind of part is
    /// regenerated.
    #[test]
    fn an_unedited_notes_page_is_rewritten_into_the_same_bytes() {
        let mut seen = 0;
        for path in corpus() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let original = std::fs::read(&path).expect("reads the deck");
            let (document, assets) = read_deck(&path);
            let written = write_deck(&document, &assets);
            for (part, bytes) in notes_parts_of(&original) {
                seen += 1;
                assert!(
                    part_of(&written, &part) == Some(bytes),
                    "{name}: `{part}` came out different from an untouched model"
                );
            }
        }
        assert!(seen >= 2, "the corpus lost its decks with speaker notes");
    }

    /// The claim of 15-E: an edited note is in the file.
    #[test]
    fn an_edited_note_reaches_the_written_deck() {
        let path = deck("notes-speaker.pptx");
        let (mut document, assets) = read_deck(&path);
        let before = presentation(&document).clone();
        assert!(
            before.slides.iter().any(|slide| slide.notes.is_some()),
            "the fixture stopped carrying speaker notes"
        );
        set_notes(
            &mut presentation_mut(&mut document).slides[0],
            "Nota reescrita",
        );

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();
        let (after, _) = read_bytes(&written);

        assert_eq!(note_text(&after.slides[0]), "Nota reescrita");
        assert_ne!(note_text(&before.slides[0]), note_text(&after.slides[0]));
        // The note nobody touched is the note that was read, and so is
        // everything else on the slide.
        assert_eq!(
            note_text(&after.slides[1]),
            note_text(&before.slides[1]),
            "a notes page nobody edited changed"
        );
        assert_eq!(after.slides[1], before.slides[1]);
        assert!(
            !report
                .warnings
                .iter()
                .any(|warning| matches!(warning, Warning::Degraded { .. })),
            "a note this writer wrote was reported as a loss: {:?}",
            report.warnings
        );
    }

    /// The notes page is reached through the slide, not by its number.
    ///
    /// `notes-crossed.pptx` exists for the reader's sake: its first slide's
    /// note is in `notesSlide2.xml`. A writer that trusted the numbering would
    /// put the edit under the other slide — a silent, plausible-looking
    /// corruption, and the reason both sides go through `notes::part`.
    #[test]
    fn an_edited_note_lands_in_the_page_the_slide_points_at() {
        let path = deck("notes-crossed.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let first = String::from_utf8(
            part_of(&original, "ppt/notesSlides/notesSlide1.xml").expect("the first page"),
        )
        .expect("utf-8");
        assert!(
            first.contains("segunda"),
            "the fixture stopped crossing its notes pages"
        );

        let (mut document, assets) = read_deck(&path);
        set_notes(
            &mut presentation_mut(&mut document).slides[0],
            "Nota cruzada",
        );
        let written = write_deck(&document, &assets);

        let landed = String::from_utf8(
            part_of(&written, "ppt/notesSlides/notesSlide2.xml").expect("the second page"),
        )
        .expect("utf-8");
        assert!(
            landed.contains("Nota cruzada"),
            "the edit did not land in the page the slide points at: {landed}"
        );
        assert_eq!(
            part_of(&written, "ppt/notesSlides/notesSlide1.xml"),
            Some(first.into_bytes()),
            "the page the numbering would have chosen was rewritten"
        );
    }

    /// A note typed onto a slide the deck has no notes page for.
    ///
    /// The page is a part of its own, and it hangs from a notes master: writing
    /// one is the same work as writing a deck that has no skeleton at all, so
    /// it is refused by the name of the increment that does it — and loudly,
    /// because the note is text a caller typed and it is not in the file.
    #[test]
    fn a_note_the_deck_has_no_page_for_is_kept_and_named() {
        let path = deck("basic-slides.pptx");
        let (mut document, assets) = read_deck(&path);
        assert!(
            presentation(&document)
                .slides
                .iter()
                .all(|slide| slide.notes.is_none()),
            "the fixture grew speaker notes"
        );
        set_notes(&mut presentation_mut(&mut document).slides[0], "Una nota");

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            notes_parts_of(&written).is_empty(),
            "a notes page was invented"
        );
        assert!(
            report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("speaker notes") && why.contains("notes master"))
            }),
            "the note was dropped without a word: {:?}",
            report.warnings
        );
    }

    /// Notes the model no longer holds are still in the file, and said so.
    ///
    /// Deleting a notes page is the deck's own bookkeeping again — the part,
    /// its content type and the slide's relationship to it — and this
    /// increment removes those only for a slide that went with them. A caller
    /// who deleted the notes and got them back has to learn it from the report.
    #[test]
    fn dropping_the_notes_of_a_slide_is_kept_and_named() {
        let path = deck("notes-speaker.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (mut document, assets) = read_deck(&path);
        presentation_mut(&mut document).slides[0].notes = None;

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        for (part, bytes) in notes_parts_of(&original) {
            assert_eq!(
                part_of(&written, &part),
                Some(bytes),
                "`{part}` was rewritten anyway"
            );
        }
        assert!(
            report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("speaker notes"))
            }),
            "the deleted notes page was copied in silence: {:?}",
            report.warnings
        );
    }

    /// 15-E: a slide the model dropped leaves the deck, and takes its own
    /// parts with it.
    ///
    /// Leaving the part behind and only unlisting it would be the smaller
    /// edit, and it would leave the deleted slide's text in a file that says
    /// the slide is gone. So the part, its relationships, its notes page and
    /// its content type all go, and the two parts that name it — the deck's
    /// `p:sldIdLst` and its `_rels` — are spliced rather than regenerated.
    #[test]
    fn removing_a_slide_takes_its_parts_out_of_the_deck() {
        let path = deck("basic-slides.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (mut document, assets) = read_deck(&path);
        let gone = presentation_mut(&mut document)
            .slides
            .pop()
            .expect("a slide to remove");
        let kept = presentation(&document).slides[0].clone();

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            part_of(&written, "ppt/slides/slide2.xml").is_none(),
            "the removed slide's part is still in the package"
        );
        assert!(
            part_of(&written, "ppt/slides/_rels/slide2.xml.rels").is_none(),
            "the removed slide's relationships are still in the package"
        );
        // The slide that stays is byte for byte the part that was read.
        assert_eq!(
            part_of(&written, "ppt/slides/slide1.xml"),
            part_of(&original, "ppt/slides/slide1.xml"),
            "the slide nobody touched was rewritten"
        );
        let types = String::from_utf8(part_of(&written, "[Content_Types].xml").expect("types"))
            .expect("utf-8");
        assert!(
            !types.contains("/ppt/slides/slide2.xml"),
            "the content types still declare a part that is gone: {types}"
        );
        let rels = String::from_utf8(
            part_of(&written, "ppt/_rels/presentation.xml.rels").expect("the deck's rels"),
        )
        .expect("utf-8");
        assert!(
            !rels.contains("slides/slide2.xml"),
            "the deck still relates the slide that is gone: {rels}"
        );

        let (after, _) = read_bytes(&written);
        assert_eq!(
            after.slides.len(),
            1,
            "the written deck still has two slides"
        );
        assert_eq!(after.slides[0].title(), kept.title());
        assert_ne!(kept.title(), gone.title(), "the fixture titles its slides");
        assert!(
            !report
                .warnings
                .iter()
                .any(|warning| matches!(warning, Warning::Degraded { .. })),
            "a removal this writer performed was reported as a loss: {:?}",
            report.warnings
        );
    }

    /// A slide that moved takes its part with it, and the part does not move.
    ///
    /// The claim that makes a reorder a reorder: only `p:sldIdLst` changes, and
    /// every slide part comes out byte for byte the one that was read. The
    /// alternative — rewriting slide 1 with slide 2's text — would keep the
    /// deck's *text* right and lose everything on the slides the IR does not
    /// model, silently.
    #[test]
    fn reordering_the_slides_moves_the_list_and_not_the_parts() {
        let path = deck("basic-slides.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (mut document, assets) = read_deck(&path);
        let before = presentation(&document).clone();
        presentation_mut(&mut document).slides.swap(0, 1);

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        for (part, bytes) in slide_parts_of(&original) {
            assert_eq!(
                part_of(&written, &part),
                Some(bytes),
                "`{part}` was rewritten instead of moved"
            );
        }
        assert_ne!(
            part_of(&written, "ppt/presentation.xml"),
            part_of(&original, "ppt/presentation.xml"),
            "the slide list did not move"
        );
        let (after, _) = read_bytes(&written);
        assert_eq!(after.slides[0].title(), before.slides[1].title());
        assert_eq!(after.slides[1].title(), before.slides[0].title());
        assert!(
            !report
                .warnings
                .iter()
                .any(|warning| matches!(warning, Warning::Degraded { .. })),
            "a reorder this writer performed was reported as a loss: {:?}",
            report.warnings
        );
    }

    /// Retypes the first paragraph of one slide, which for these fixtures is
    /// its title — the part of the signature `align` matches on.
    fn retype_slide(document: &mut Document, index: usize, text: &str) {
        let deck = presentation_mut(document);
        let paragraph = first_paragraph(&mut deck.slides[index]).expect("a paragraph to retype");
        paragraph.content = vec![docsai_model::text::Inline::Text(text.into())];
    }

    /// The warnings `align`'s third pass raises when it had to guess.
    fn alignment_warnings(report: &ConversionReport) -> Vec<&str> {
        report
            .warnings
            .iter()
            .filter_map(|warning| match warning {
                Warning::Degraded { what, why } if what.contains("part alignment") => {
                    Some(why.as_str())
                }
                _ => None,
            })
            .collect()
    }

    /// Two slides edited *and* moved: the pairing is an order-fill, and it says
    /// so, naming both slides and the part each was written into.
    ///
    /// `[A, B]` in the package against `[B', A']` in the model matches nothing
    /// in the first two passes — a retyped title is a different signature by
    /// definition, and neither slide is where the package still says it is — so
    /// the third pass pairs `B'` with `A`'s part. The deck is still written, and
    /// in the model's order; what the caller cannot see without being told is
    /// that each slide may have taken over the other's layout, `_rels` and
    /// notes page.
    #[test]
    fn two_slides_edited_and_reordered_name_the_pairing_as_a_guess() {
        let path = deck("basic-slides.pptx");
        let (mut document, assets) = read_deck(&path);
        presentation_mut(&mut document).slides.swap(0, 1);
        retype_slide(&mut document, 0, "Segunda, retitulada");
        retype_slide(&mut document, 1, "Primera, retitulada");

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");

        let guessed = alignment_warnings(&report);
        assert_eq!(guessed.len(), 1, "{:?}", report.warnings);
        let why = guessed[0];
        for named in [
            "slide 1 (\"Segunda, retitulada\")",
            "slide 2 (\"Primera, retitulada\")",
            "ppt/slides/slide1.xml",
            "ppt/slides/slide2.xml",
        ] {
            assert!(
                why.contains(named),
                "the warning does not name {named}: {why}"
            );
        }
    }

    /// One slide edited and nothing moved: the third pass has a single free
    /// part, the pairing is forced, and there is nothing to warn about.
    #[test]
    fn one_edited_slide_is_a_forced_pairing_and_says_nothing() {
        let path = deck("basic-slides.pptx");
        let (mut document, assets) = read_deck(&path);
        retype_slide(&mut document, 0, "Sólo esta cambia");

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            alignment_warnings(&report).is_empty(),
            "a forced pairing was reported as a guess: {:?}",
            report.warnings
        );
        let (after, _) = read_bytes(&written);
        assert_eq!(
            after.slides[0].title().as_deref(),
            Some("Sólo esta cambia"),
            "the edit did not land on the slide it was made on"
        );
    }

    /// A deck nobody touched is still the deck that was read, and says nothing.
    #[test]
    fn an_unedited_deck_reports_no_alignment_guess() {
        let path = deck("basic-slides.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (document, assets) = read_deck(&path);

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert_eq!(
            members(&written),
            members(&original),
            "the package is not the one that was read"
        );
        for (part, bytes) in slide_parts_of(&original) {
            assert_eq!(part_of(&written, &part), Some(bytes), "`{part}` moved");
        }
        assert!(
            alignment_warnings(&report).is_empty(),
            "an untouched deck was reported as a guess: {:?}",
            report.warnings
        );
    }

    /// A reorder with no edit is matched by the second pass — every slide is
    /// still stated verbatim somewhere in the package — so the third pass never
    /// runs and nothing is guessed.
    #[test]
    fn a_pure_reorder_reports_no_alignment_guess() {
        let path = deck("basic-slides.pptx");
        let (mut document, assets) = read_deck(&path);
        presentation_mut(&mut document).slides.swap(0, 1);

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");

        assert!(
            alignment_warnings(&report).is_empty(),
            "a reorder the second pass matched was reported as a guess: {:?}",
            report.warnings
        );
    }

    /// A slide the model *added* has no part to be spliced into, and inventing
    /// one is 15-G. The package is copied whole rather than written with the
    /// edits of slide 3 under slide 4's number.
    /// A deck the way an agent types one: no skeleton, no attributes, no
    /// layout named anywhere — a title and some bullets.
    fn typed_slide(title: &str, bullets: &[&str]) -> Slide {
        use docsai_model::text::{Block, Inline, List, ListItem, Paragraph};
        let mut shapes = Vec::new();
        if !title.is_empty() {
            shapes.push(Shape::new(
                0,
                ShapeKind::Placeholder(Placeholder {
                    ph_type: PhType::Title,
                    body: vec![Block::Paragraph(Paragraph::new(vec![Inline::Text(
                        title.into(),
                    )]))],
                    ..Default::default()
                }),
            ));
        }
        if !bullets.is_empty() {
            let items = bullets
                .iter()
                .map(|text| ListItem {
                    blocks: vec![Block::Paragraph(Paragraph::new(vec![Inline::Text(
                        (*text).into(),
                    )]))],
                })
                .collect();
            shapes.push(Shape::new(
                shapes.len() as u32,
                ShapeKind::Placeholder(Placeholder {
                    ph_type: PhType::Body,
                    body: vec![Block::List(List {
                        id: None,
                        def: None,
                        ordered: false,
                        level: 0,
                        items,
                    })],
                    ..Default::default()
                }),
            ));
        }
        Slide {
            shapes,
            ..Default::default()
        }
    }

    fn typed_deck(slides: Vec<Slide>) -> Document {
        Document::Presentation(Presentation {
            slides,
            ..Default::default()
        })
    }

    /// The relationships of one part of a written package, as text.
    fn rels_of(package: &[u8], part: &str) -> String {
        let name = rels_part_name(part).expect("a part with relationships");
        String::from_utf8(part_of(package, &name).unwrap_or_else(|| panic!("{name} is missing")))
            .expect("the relationship part is text")
    }

    fn text_of(package: &[u8], part: &str) -> String {
        String::from_utf8(part_of(package, part).unwrap_or_else(|| panic!("{part} is missing")))
            .expect("the part is text")
    }

    /// The acceptance criterion of 15-G: a deck with nothing to preserve is
    /// written on the embedded template, and the layout comes from the shape of
    /// the content.
    #[test]
    fn a_deck_with_no_skeleton_is_written_on_the_embedded_template() {
        let document = typed_deck(vec![typed_slide(
            "Resultados",
            &["Ingresos al alza", "Costes estables"],
        )]);
        let assets = MemoryAssetStore::new();
        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        let names: Vec<String> = members(&written).into_iter().map(|(n, ..)| n).collect();
        for part in [
            "[Content_Types].xml",
            "_rels/.rels",
            "ppt/presentation.xml",
            "ppt/_rels/presentation.xml.rels",
            "ppt/slideMasters/slideMaster1.xml",
            "ppt/theme/theme1.xml",
            "ppt/slides/slide1.xml",
            "ppt/slides/_rels/slide1.xml.rels",
        ] {
            assert!(
                names.contains(&part.to_string()),
                "`{part}` is not in {names:?}"
            );
        }

        // Rule 1: a title with bullets under it is «Title and Content», and the
        // slide states the layout's own placeholders rather than the model's.
        let slide = text_of(&written, "ppt/slides/slide1.xml");
        assert!(slide.contains("<p:ph type=\"title\"/>"), "{slide}");
        assert!(slide.contains("<p:ph type=\"body\" idx=\"1\"/>"), "{slide}");
        assert!(slide.contains("<a:t>Ingresos al alza</a:t>"), "{slide}");
        let rels = rels_of(&written, "ppt/slides/slide1.xml");
        assert!(
            rels.contains("slideLayout2.xml"),
            "the slide is not on «Title and Content»: {rels}"
        );
        let layout = text_of(&written, "ppt/slideLayouts/slideLayout2.xml");
        assert!(layout.contains("name=\"Title and Content\""), "{layout}");

        // The three parts that have to agree about the slide existing at all.
        let types = text_of(&written, "[Content_Types].xml");
        assert!(
            types.contains(
                "<Override PartName=\"/ppt/slides/slide1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
            ),
            "{types}"
        );
        let deck_rels = text_of(&written, "ppt/_rels/presentation.xml.rels");
        let id = deck_rels
            .split("Target=\"slides/slide1.xml\"")
            .next()
            .and_then(|before| before.rsplit("Id=\"").next())
            .and_then(|id| id.split('"').next())
            .expect("the deck relates the slide")
            .to_string();
        let presentation = text_of(&written, "ppt/presentation.xml");
        assert!(
            presentation.contains(&format!("<p:sldId id=\"256\" r:id=\"{id}\"/>")),
            "the slide list does not name the slide's relationship: {presentation}"
        );

        // And it reads back as the deck that was written.
        let (deck, _) = read_bytes(&written);
        assert_eq!(deck.slides.len(), 1);
        assert_eq!(deck.slides[0].title().as_deref(), Some("Resultados"));
        assert_eq!(count_items(&deck.slides[0]), 2);
        assert!(
            report.warnings.iter().any(|warning| matches!(
                warning,
                Warning::Degraded { what, why }
                    if what.contains("design") && why.contains("embedded default template")
            )),
            "the report does not say the deck was written on the template: {:?}",
            report.warnings
        );
    }

    /// Rule 2: a title and nothing else is a title slide.
    #[test]
    fn a_title_only_slide_is_written_on_the_title_layout() {
        let document = typed_deck(vec![typed_slide("Portada", &[])]);
        let assets = MemoryAssetStore::new();
        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        let rels = rels_of(&written, "ppt/slides/slide1.xml");
        assert!(
            rels.contains("slideLayout1.xml"),
            "the slide is not on «Title Slide»: {rels}"
        );
        let layout = text_of(&written, "ppt/slideLayouts/slideLayout1.xml");
        assert!(layout.contains("name=\"Title Slide\""), "{layout}");
        // The layout's title is a *centred* one, and the slide has to say so or
        // the box is unmatched and the text lands at the master's position.
        let slide = text_of(&written, "ppt/slides/slide1.xml");
        assert!(slide.contains("<p:ph type=\"ctrTitle\"/>"), "{slide}");
        assert!(
            !report
                .warnings
                .iter()
                .any(|warning| matches!(warning, Warning::Degraded { why, .. } if why.contains("matches neither"))),
            "a slide the rules answer for was reported as unmatched: {:?}",
            report.warnings
        );
    }

    /// Content that fits no rule is still written, and the report says which
    /// layout it went on rather than leaving the caller to find out.
    #[test]
    fn content_that_matches_no_rule_is_written_and_named() {
        let mut slide = typed_slide("Datos", &[]);
        slide.shapes.push(Shape::new(
            1,
            ShapeKind::Table(docsai_model::text::Table {
                col_widths: vec![Length::from_emu(2_000_000), Length::from_emu(2_000_000)],
                rows: vec![docsai_model::text::TableRow {
                    cells: vec![cell_with("Norte"), docsai_model::text::TableCell::default()],
                    ..Default::default()
                }],
                header_row: true,
                ..Default::default()
            }),
        ));
        let document = typed_deck(vec![slide]);
        let assets = MemoryAssetStore::new();
        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        // The table is an `a:tbl` written from the model — the writer
        // [[64-phase-15-media]] pointed here for — and it is a whole grid.
        let slide = text_of(&written, "ppt/slides/slide1.xml");
        assert!(slide.contains("<a:tbl>"), "{slide}");
        assert!(slide.contains("<a:gridCol w=\"2000000\"/>"), "{slide}");
        assert!(slide.contains("<a:t>Norte</a:t>"), "{slide}");
        assert!(
            report.warnings.iter().any(|warning| matches!(
                warning,
                Warning::Degraded { what, why }
                    if what.contains("Datos") && why.contains("matches neither")
                        && why.contains("Title and Content")
            )),
            "the report does not name the slide whose content fits no rule: {:?}",
            report.warnings
        );

        let (deck, _) = read_bytes(&written);
        let table = deck.slides[0]
            .shapes
            .iter()
            .find_map(|shape| match &shape.kind {
                ShapeKind::Table(table) => Some(table),
                _ => None,
            });
        assert!(table.is_some(), "the table did not survive the write");
    }

    fn cell_with(text: &str) -> docsai_model::text::TableCell {
        use docsai_model::text::{Block, Inline, Paragraph};
        docsai_model::text::TableCell {
            blocks: vec![Block::Paragraph(Paragraph::new(vec![Inline::Text(
                text.into(),
            )]))],
            ..Default::default()
        }
    }

    /// 15-E refused a slide the model added by name, because there was no part
    /// to splice it into. There is one now, and the slides that were already
    /// there do not move.
    #[test]
    fn adding_a_slide_writes_the_part_the_package_did_not_have() {
        let path = deck("notes-speaker.pptx");
        let (mut document, assets) = read_deck(&path);
        let before = write_deck(&document, &assets);
        let mut extra = typed_slide("Tercera", &["Añadida como texto"]);
        extra.notes = Some(vec![docsai_model::text::Block::Paragraph(
            docsai_model::text::Paragraph::new(vec![docsai_model::text::Inline::Text(
                "Notas de la nueva".into(),
            )]),
        )]);
        presentation_mut(&mut document).slides.push(extra);

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            !report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("15-G"))
            }),
            "the writer still refuses the added slide: {:?}",
            report.warnings
        );
        // The two slides that were already there are the bytes that were read.
        for part in ["ppt/slides/slide1.xml", "ppt/slides/slide2.xml"] {
            assert_eq!(
                part_of(&written, part),
                part_of(&before, part),
                "`{part}` moved when a slide was added beside it"
            );
        }
        let slide = text_of(&written, "ppt/slides/slide3.xml");
        assert!(slide.contains("<a:t>Tercera</a:t>"), "{slide}");
        let presentation = text_of(&written, "ppt/presentation.xml");
        assert_eq!(
            presentation.matches("<p:sldId ").count(),
            3,
            "the deck's slide list does not hold three slides: {presentation}"
        );
        // A notes page the deck does not have is created too, hanging from the
        // notes master the package already carries.
        let notes = text_of(&written, "ppt/notesSlides/notesSlide3.xml");
        assert!(notes.contains("<a:t>Notas de la nueva</a:t>"), "{notes}");
        assert!(
            rels_of(&written, "ppt/slides/slide3.xml").contains("notesSlide3.xml"),
            "the slide does not point at its notes page"
        );

        let (deck, _) = read_bytes(&written);
        assert_eq!(deck.slides.len(), 3);
        assert_eq!(deck.slides[2].title().as_deref(), Some("Tercera"));
        assert_eq!(
            deck.slides[0].title().as_deref(),
            Some("Resultados"),
            "the slides that were there did not stay where they were"
        );
    }

    /// A notes page needs a notes master to hang from, and a deck without one
    /// keeps its slide and is told what it lost.
    #[test]
    fn a_note_on_an_added_slide_with_no_notes_master_is_named() {
        let path = deck("basic-slides.pptx");
        let (mut document, assets) = read_deck(&path);
        let mut extra = typed_slide("Tercera", &["Con notas"]);
        extra.notes = Some(vec![docsai_model::text::Block::Paragraph(
            docsai_model::text::Paragraph::new(vec![docsai_model::text::Inline::Text(
                "Notas sin sitio".into(),
            )]),
        )]);
        presentation_mut(&mut document).slides.push(extra);

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        let slide = text_of(&written, "ppt/slides/slide3.xml");
        assert!(
            slide.contains("<a:t>Tercera</a:t>"),
            "the slide was lost too"
        );
        assert!(
            report.warnings.iter().any(|warning| matches!(
                warning,
                Warning::Degraded { what, why }
                    if what.contains("Tercera") && why.contains("notes master")
            )),
            "the report does not name the notes that could not be written: {:?}",
            report.warnings
        );
    }

    /// A note typed onto a slide that never had a notes page: the page is
    /// written, the slide's own relationships gain one line and keep every
    /// other byte, and the page the *other* slide already had is untouched.
    ///
    /// No corpus deck has a notes master and a slide without a page — the
    /// generator gives every noted deck a page per slide — so the fixture is
    /// made here by taking the `notesSlide` relationship off the second slide,
    /// which is exactly the deck a caller who added a note by hand has.
    #[test]
    fn a_note_on_a_slide_with_no_page_gets_one_written() {
        let path = deck("notes-speaker.pptx");
        let bytes = std::fs::read(&path).expect("reads the deck");
        let mut package = Package::open(Cursor::new(bytes)).expect("the fixture is a package");
        let rels = "ppt/slides/_rels/slide2.xml.rels";
        let stripped = package
            .text(rels)
            .expect("the slide has relationships")
            .lines()
            .map(|line| {
                let mut kept = String::new();
                for piece in line.split_inclusive("/>") {
                    if !piece.contains("notesSlide") {
                        kept.push_str(piece);
                    }
                }
                kept
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !stripped.contains("notesSlide"),
            "the fixture kept the relationship"
        );
        package.insert(rels, stripped.into_bytes());
        let mut fixture = Cursor::new(Vec::new());
        package.write_to(&mut fixture).expect("writes the fixture");
        let fixture = fixture.into_inner();

        let mut assets = MemoryAssetStore::new();
        let (mut document, _) =
            super::super::read(Cursor::new(fixture), &mut assets).expect("reads the fixture");
        assert!(
            presentation(&document).slides[1].notes.is_none(),
            "the second slide still has a notes page"
        );
        set_notes(&mut presentation_mut(&mut document).slides[1], "Nota nueva");

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        let page = notes_parts_of(&written)
            .into_iter()
            .find(|(name, bytes)| {
                name != "ppt/notesSlides/notesSlide1.xml"
                    && String::from_utf8_lossy(bytes).contains("Nota nueva")
            })
            .map(|(name, _)| name);
        assert!(
            page.is_some(),
            "no notes page was written for the second slide"
        );
        let page = page.expect("the page");
        let slide_rels = rels_of(&written, "ppt/slides/slide2.xml");
        assert!(
            slide_rels.contains("slideLayout1.xml") && slide_rels.contains("notesSlide"),
            "the slide's relationships lost what they had: {slide_rels}"
        );
        let types = text_of(&written, "[Content_Types].xml");
        assert!(
            types.contains(&format!("PartName=\"/{page}\"")),
            "the new page has no content type: {types}"
        );
        assert!(
            !report.warnings.iter().any(|warning| matches!(
                warning,
                Warning::Degraded { why, .. } if why.contains("speaker notes")
            )),
            "a page that was written was reported as a loss: {:?}",
            report.warnings
        );
        // The note the deck already had is where it was.
        let first = text_of(&written, "ppt/notesSlides/notesSlide1.xml");
        assert!(
            first.contains("interanual"),
            "the other page moved: {first}"
        );
    }

    /// A shape whose markup only its own package holds — a chart, a diagram,
    /// an embedded object — cannot be generated at all. The slide is still
    /// written, its label stands where the object was, and the report says so:
    /// a hole in a slide nobody was told about is the failure of rule 3.
    #[test]
    fn a_shape_that_cannot_be_generated_is_named_and_stands_in() {
        let mut slide = typed_slide("Ventas", &[]);
        slide.shapes.push(Shape::new(
            1,
            ShapeKind::Chart(docsai_model::presentation::ChartRef {
                kind: Some("bar".into()),
                title: Some("Ventas por región".into()),
                ..Default::default()
            }),
        ));
        let document = typed_deck(vec![slide]);
        let assets = MemoryAssetStore::new();
        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        let slide = text_of(&written, "ppt/slides/slide1.xml");
        assert!(slide.contains("<a:t>Ventas por región</a:t>"), "{slide}");
        assert!(
            !slide.contains("graphicFrame"),
            "a chart frame was invented for a chart part that does not exist: {slide}"
        );
        assert!(
            report.warnings.iter().any(|warning| matches!(
                warning,
                Warning::Degraded { what, why }
                    if what.contains("Ventas") && why.contains("chart")
            )),
            "the report does not name the shape that could not be generated: {:?}",
            report.warnings
        );
    }

    /// A slide inserted in the middle lands there, and the slides around it
    /// keep their own parts — the pairing of [`align`] and the one [`seed`]
    /// makes have to be the same pairing.
    #[test]
    fn a_slide_added_in_the_middle_lands_in_the_middle() {
        let path = deck("basic-slides.pptx");
        let (mut document, assets) = read_deck(&path);
        let extra = typed_slide("Intercalada", &["En medio"]);
        presentation_mut(&mut document).slides.insert(1, extra);

        let mut out = Cursor::new(Vec::new());
        write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        let (deck, _) = read_bytes(&written);
        let titles: Vec<Option<String>> = deck.slides.iter().map(|s| s.title()).collect();
        assert_eq!(
            titles,
            vec![
                Some("Informe trimestral".to_string()),
                Some("Intercalada".to_string()),
                Some("Siguientes pasos".to_string()),
            ],
            "the added slide is not where the model put it"
        );
    }

    /// A deck with no skeleton and no slides has nothing to write, and the
    /// template on its own is not a deck: it is refused rather than emitted.
    #[test]
    fn a_deck_with_no_skeleton_and_no_slides_is_refused() {
        let document = typed_deck(Vec::new());
        let assets = MemoryAssetStore::new();
        let mut out = Cursor::new(Vec::new());
        let error = write_pptx(&document, &assets, &mut out).expect_err("refuses");
        assert!(
            error.to_string().contains("no slides"),
            "the refusal does not say what is wrong: {error}"
        );
    }

    /// The other shapes of a document are not decks.
    #[test]
    fn a_text_document_is_not_written_as_a_deck() {
        let document = Document::Text(docsai_model::TextDocument::default());
        let assets = MemoryAssetStore::new();
        let mut out = Cursor::new(Vec::new());
        let error = write_pptx(&document, &assets, &mut out).expect_err("refuses");
        assert!(error.to_string().contains(".pptx"), "{error}");
    }

    /// 15-D: a preserved object is not a loss, and must not be reported as one.
    ///
    /// The group, the custom geometry, the transition and the timing of
    /// `raw-preserved.pptx` are markup the IR only stubs. The splice never
    /// touched them, so they leave the writer as the bytes that came in, at
    /// the z-order they came in at — which is what 15-D promises, and what a
    /// `Degraded` warning would have been lying about.
    #[test]
    fn a_preserved_object_goes_out_as_the_bytes_that_came_in() {
        let path = deck("raw-preserved.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (document, assets) = read_deck(&path);
        let deck_ir = presentation(&document);
        assert!(
            !deck_ir.raw.is_empty() && !deck_ir.slides[0].raw.is_empty(),
            "the fixture stopped carrying raw fragments"
        );

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            !report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("preserved"))
            }),
            "a preserved object was reported as copied: {:?}",
            report.warnings
        );
        let part = "ppt/slides/slide1.xml";
        assert_eq!(
            part_of(&original, part),
            part_of(&written, part),
            "the slide holding the preserved objects moved"
        );
    }

    /// A payload the model changed is written back over the shape it belongs to.
    ///
    /// This is the case the preserved package cannot cover on its own: the
    /// bytes in the skeleton are the old ones, and the fragment is what the
    /// caller edited. The match is by element name, because the fragment is a
    /// slice of the part and carries no id of its own.
    #[test]
    fn an_edited_fragment_is_written_back_over_its_shape() {
        let path = deck("raw-preserved.pptx");
        let (mut document, assets) = read_deck(&path);
        {
            let deck_ir = presentation_mut(&mut document);
            let fragment = deck_ir
                .raw
                .iter_mut()
                .find(|fragment| fragment.content.contains("custGeom"))
                .expect("the custom geometry fragment");
            fragment.content = fragment
                .content
                .replace("Triángulo libre", "Triángulo reescrito");
        }

        let written = write_deck(&document, &assets);
        let slide =
            String::from_utf8(part_of(&written, "ppt/slides/slide1.xml").expect("the slide"))
                .expect("utf-8");
        assert!(
            slide.contains("Triángulo reescrito") && !slide.contains("Triángulo libre"),
            "the edited fragment is not in the part: {slide}"
        );
        // The neighbouring shapes were spliced around, not regenerated.
        assert!(slide.contains("Caja agrupada 2") && slide.contains("<a:close/>"));
    }

    /// A slide-level fragment goes back at the level it came from.
    ///
    /// `p:transition` and `p:timing` are children of `p:sld`, not shapes on the
    /// tree, and the reader captures them onto `Slide::raw`
    /// ([[43-phase-13-raw]] decision 4). Writing them back at shape level would
    /// put an animation inside `p:spTree`.
    #[test]
    fn an_edited_slide_level_fragment_reaches_the_deck() {
        let path = deck("raw-preserved.pptx");
        let (mut document, assets) = read_deck(&path);
        {
            let deck_ir = presentation_mut(&mut document);
            let fragment = deck_ir
                .raw
                .iter_mut()
                .find(|fragment| fragment.content.starts_with("<p:transition"))
                .expect("the transition fragment");
            fragment.content = fragment.content.replace("spd=\"slow\"", "spd=\"fast\"");
        }

        let written = write_deck(&document, &assets);
        let slide =
            String::from_utf8(part_of(&written, "ppt/slides/slide1.xml").expect("the slide"))
                .expect("utf-8");
        assert!(
            slide.contains("<p:transition spd=\"fast\"") && !slide.contains("spd=\"slow\""),
            "the slide-level fragment did not move: {slide}"
        );
        // The edit landed on `p:transition` and on nothing else: its siblings
        // are the bytes that were read.
        let original = std::fs::read(&path).expect("reads the deck");
        let before =
            String::from_utf8(part_of(&original, "ppt/slides/slide1.xml").expect("the slide"))
                .expect("utf-8");
        assert_eq!(
            slide.replace("spd=\"fast\"", "spd=\"slow\""),
            before,
            "something other than the transition moved"
        );
    }

    /// A fragment this writer cannot name does not land on the first element
    /// that happens to be there.
    ///
    /// The candidates for a slide-level fragment are every child of `p:sld`,
    /// and the first of them is `p:cSld` — the whole shape tree. Matching it
    /// by default would not degrade the slide, it would corrupt it.
    #[test]
    fn a_fragment_that_opens_with_nothing_is_kept_and_named() {
        let path = deck("raw-preserved.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (mut document, assets) = read_deck(&path);
        {
            let deck_ir = presentation_mut(&mut document);
            let fragment = deck_ir
                .raw
                .iter_mut()
                .find(|fragment| fragment.content.starts_with("<p:transition"))
                .expect("the transition fragment");
            fragment.content = "<!-- lo que quedó del fragmento -->".into();
        }

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("does not open with an element"))
            }),
            "the fragment that could not be placed is not named: {:?}",
            report.warnings
        );
        assert_eq!(
            part_of(&original, "ppt/slides/slide1.xml"),
            part_of(&written, "ppt/slides/slide1.xml"),
            "an unplaceable fragment moved the slide"
        );
    }

    /// A fragment in another dialect is dropped, never emitted as nonsense.
    ///
    /// `odf` markup inside a `p:spTree` is not a degraded deck, it is a broken
    /// one. The package's own bytes stand and `Warning::RawBlockDropped` names
    /// the fragment and the dialect it was in.
    #[test]
    fn a_fragment_in_another_dialect_is_dropped_and_named() {
        let path = deck("raw-preserved.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (mut document, assets) = read_deck(&path);
        let id = {
            let deck_ir = presentation_mut(&mut document);
            let fragment = deck_ir
                .raw
                .iter_mut()
                .find(|fragment| fragment.content.contains("custGeom"))
                .expect("the custom geometry fragment");
            fragment.format = "odf".into();
            fragment.content = "<draw:custom-shape/>".into();
            fragment.id.as_str().to_string()
        };

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            report.warnings.iter().any(|warning| {
                matches!(warning, Warning::RawBlockDropped { id: reported, format }
                    if *reported == id && format == "odf")
            }),
            "the dropped fragment is not in the report: {:?}",
            report.warnings
        );
        let slide =
            String::from_utf8(part_of(&written, "ppt/slides/slide1.xml").expect("the slide"))
                .expect("utf-8");
        assert!(
            !slide.contains("draw:custom-shape"),
            "foreign markup reached the package: {slide}"
        );
        assert_eq!(
            part_of(&original, "ppt/slides/slide1.xml"),
            part_of(&written, "ppt/slides/slide1.xml"),
            "the slide moved for a fragment that was not written"
        );
    }

    /// A fragment whose markup belongs to another part is kept and named.
    ///
    /// `rebuilt_parts` is the licence, and a part outside it is copied whatever
    /// the IR seems to know about it — a chart's XML is the case that exists in
    /// the corpus, and it is Phase 16's, not this writer's.
    #[test]
    fn a_fragment_from_another_part_is_kept_and_named() {
        let path = deck("raw-preserved.pptx");
        let (mut document, assets) = read_deck(&path);
        {
            let deck_ir = presentation_mut(&mut document);
            let fragment = deck_ir
                .raw
                .iter_mut()
                .find(|fragment| fragment.content.contains("custGeom"))
                .expect("the custom geometry fragment");
            fragment.part = "ppt/charts/chart1.xml".into();
            fragment.content = fragment.content.replace("Triángulo libre", "Otro sitio");
        }

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("ppt/charts/chart1.xml"))
            }),
            "the part that was not rewritten is not named: {:?}",
            report.warnings
        );
        let slide =
            String::from_utf8(part_of(&written, "ppt/slides/slide1.xml").expect("the slide"))
                .expect("utf-8");
        assert!(
            !slide.contains("Otro sitio"),
            "the fragment was written into the wrong part: {slide}"
        );
    }
}

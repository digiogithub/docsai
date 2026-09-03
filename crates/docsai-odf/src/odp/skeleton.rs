//! The preserved skeleton for an ODF presentation (Phase 18, increment 18-D).
//!
//! The rule is spike P3's, unchanged by the dialect:
//!
//! > **Preserve everything; rebuilding is the exception.**
//!
//! So the skeleton is the *whole original package*, byte for byte, stored
//! opaquely in the [`AssetStore`] — `styles.xml` with its masters and page
//! layouts, `meta.xml`, `settings.xml`, `META-INF/manifest.xml`, `Pictures/`,
//! the chart sub-documents, a `Basic/` project the reader deliberately never
//! executed, and the ZIP's own member order and compression, which no
//! decompressed [`Package`](crate::package::Package) can reproduce. The last of
//! those matters more here than it does for OPC: ODF requires `mimetype` to be
//! the **first** member and **stored uncompressed**, and a package rebuilt
//! through a `BTreeMap` loses both facts along with every timestamp.
//!
//! ## Why the list is one name long
//!
//! [`SkeletonRef::rebuilt_parts`] is the writer's licence to regenerate a part
//! rather than copy it, and for a `.odp` it holds exactly `content.xml`.
//! [`kb/98`](../../../../kb/98-phase-18-odp-reader.md) predicted the shape and
//! warned that the pressure would be to widen it:
//!
//! * **`content.xml` is the one part whose content becomes the IR.** Every
//!   `draw:page`, every frame, every paragraph of every text box is read out of
//!   it, so the model can restate it.
//! * **`styles.xml` is read and is deliberately *not* on the list.** The masters,
//!   the page layouts and the page size reach the IR as *references and deltas* —
//!   `Layout`'s placeholders carry no properties and `Master::theme` is `None`
//!   for every `.odp`, on purpose — so the model could not restate the part if
//!   it tried. The writer copies it verbatim, always: a page transition declared
//!   in it (which `RawFragment::part` names honestly, and which 18-B warned
//!   18-D about) therefore survives a round trip that dropped it from DocMark.
//! * **`meta.xml` reaches `DocumentMeta`, and is still not on the list**, for the
//!   same reason `ppt/presentation.xml` is absent from the pptx one: the IR holds
//!   facts stated in it, not its content.
//!
//! A pptx skeleton names one part per slide plus one per notes page — seventeen
//! names for `corpus/pptx/forty-slides.pptx`. A `.odp` names one, whatever the
//! deck, because ODF puts every page in a single part. Shorter is the point, not
//! an omission.
//!
//! ## What the writer does with the list, which is nothing
//!
//! The same thing the pptx writer does, and for the same reason its own module
//! writes down: **the list does not survive a round trip through DocMark.** A
//! deck parsed back from Markdown resolves its `skeleton:` reference and carries
//! an empty `rebuilt_parts`, so a writer that consulted the list would refuse
//! every edit a person actually made. [`super::write`] therefore goes through
//! the *same door* the reader went through — `content.xml`, and only ever the
//! byte ranges of the frames the model states — which is what makes the two
//! sides agree about which part the IR holds without a list to compare.
//!
//! The list is the reader's honest statement of what it turned into IR, and it
//! is what an `inspect` of a deck, or a later increment reasoning about the
//! package, has to go on. `META-INF/manifest.xml` is not on it and is still
//! *spliced* when an edited body brings a picture the package did not carry: a
//! `<manifest:file-entry>` added before the closing tag is an edit inside a
//! part, not a regeneration of one — exactly what the pptx writer does to a
//! slide's `_rels` and to `[Content_Types].xml`, neither of which is on its
//! list either.

use docsai_model::assets::AssetStore;
use docsai_model::presentation::SkeletonRef;
use docsai_model::report::{ConversionReport, Warning};

/// The one part whose content this reader turns into IR.
pub(crate) const REBUILT_PARTS: &[&str] = &["content.xml"];

/// Stores the original package and names the parts the IR reproduces.
///
/// Returns `None` only when the package could not be stored, and says so: a
/// deck with no skeleton is a deck no ODF writer can produce, because this
/// writer splices and never generates.
pub(crate) fn capture(
    original: &[u8],
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Option<SkeletonRef> {
    match assets.put(original) {
        Ok(asset) => Some(SkeletonRef {
            asset,
            rebuilt_parts: REBUILT_PARTS.iter().map(|s| (*s).to_string()).collect(),
        }),
        Err(error) => {
            report.warn(Warning::Degraded {
                what: "the preserved package skeleton".into(),
                why: format!(
                    "the original package could not be stored ({error}): the deck can be \
                     read but not written back, because the `.odp` writer splices into the \
                     package it was read from and never regenerates one"
                ),
            });
            None
        }
    }
}

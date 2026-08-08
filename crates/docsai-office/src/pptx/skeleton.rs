//! The preserved skeleton (Phase 13, increment 13-H).
//!
//! Spike P3 measured what happens to a deck whose non-slide parts are
//! regenerated instead of kept: LibreOffice
//! opens the result without a word, and a chart loses its values while SmartArt
//! loses five parts with no visible change at all. The rule it produced is
//! stronger than "preserve the parts we do not model":
//!
//! > **Preserve everything; rebuilding is the exception.**
//!
//! So the skeleton is the *whole original package*, byte for byte, stored
//! opaquely in the [`AssetStore`] — theme, masters, `tableStyles.xml`,
//! `docProps`, the embedded workbook of a chart, the four `dgm:` parts of a
//! diagram, and the ZIP's own member order and compression, which no
//! decompressed `Package` can reproduce. What the IR *does* model is listed in
//! [`SkeletonRef::rebuilt_parts`], and that list is the writer's licence to
//! regenerate a part rather than copy it.
//!
//! It is content-hashed like every other asset, so a deck read twice is stored
//! once, and the id survives serialisation while the bytes stay out of the IR.

use docsai_model::assets::AssetStore;
use docsai_model::presentation::SkeletonRef;
use docsai_model::report::{ConversionReport, Warning};

/// Stores the original package and names the parts the IR reproduces.
///
/// `rebuilt` is the list of parts whose content this reader turned into IR — the
/// slides it read and their notes pages, and nothing else. A part the reader
/// skipped (an orphan slide no `p:sldIdLst` entry points at, a layout, the
/// theme) is deliberately absent: the IR does not hold it, so a writer that
/// regenerated it would be inventing content.
///
/// Returns `None` only when the package could not be stored, and says so: a
/// deck with no skeleton is a deck a writer has to rebuild from the IR alone,
/// which is exactly the loss spike P3 measured.
pub(super) fn capture(
    original: &[u8],
    rebuilt: &[String],
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Option<SkeletonRef> {
    match assets.put(original) {
        Ok(asset) => {
            let mut rebuilt_parts = rebuilt.to_vec();
            rebuilt_parts.sort();
            rebuilt_parts.dedup();
            Some(SkeletonRef {
                asset,
                rebuilt_parts,
            })
        }
        Err(error) => {
            report.warn(Warning::Degraded {
                what: "the preserved package skeleton".into(),
                why: format!(
                    "the original package could not be stored ({error}): \
                     theme, masters and every unmodelled part would have to be \
                     regenerated from the IR"
                ),
            });
            None
        }
    }
}

//! Structured conversion reports.
//!
//! Rule 3 of `AGENTS.md`: information is never lost silently. Every
//! degradation a reader or writer performs shows up here, typed, and travels
//! to the CLI (stderr / `--json`) and to the MCP response.

use serde::{Deserialize, Serialize};

/// How much a warning should worry the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// Nothing was lost; the user may still want to know.
    Info,
    /// Something was simplified but is still recognisable.
    Minor,
    /// Content was dropped or degraded in a way that shows.
    Severe,
}

/// A typed conversion warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "warning")]
pub enum Warning {
    /// An element with no IR representation was met.
    UnsupportedElement {
        kind: String,
        location: String,
        /// What was done about it (`raw-block`, `skipped`…).
        action: String,
    },
    /// Something was mapped, but imperfectly.
    Degraded { what: String, why: String },
    /// Image geometry could not be mapped in full (arquitectura §3.1).
    ImageGeometryDegraded { what: String, why: String },
    /// A media part could not be stored or read.
    AssetIssue { asset: String, why: String },
    /// A linked (not embedded) image was found; remote content is never
    /// fetched, so only the URL survives.
    ExternalImageNotFetched { url: String },
    /// Macros are always ignored (`.docm`/`.xlsm` read as their macro-free
    /// equivalents).
    MacrosIgnored { part: String },
    /// Tracked changes were resolved to their accepted state.
    RevisionsAccepted { count: u32 },
    /// A raw-block could not be re-injected because the target dialect differs.
    RawBlockDropped { id: String, format: String },
}

impl Warning {
    pub fn severity(&self) -> Severity {
        match self {
            Warning::UnsupportedElement { action, .. } if action == "raw-block" => Severity::Minor,
            Warning::UnsupportedElement { .. } => Severity::Severe,
            Warning::Degraded { .. } | Warning::ImageGeometryDegraded { .. } => Severity::Minor,
            Warning::AssetIssue { .. } | Warning::RawBlockDropped { .. } => Severity::Severe,
            Warning::ExternalImageNotFetched { .. } => Severity::Minor,
            Warning::MacrosIgnored { .. } | Warning::RevisionsAccepted { .. } => Severity::Info,
        }
    }

    /// One-line human rendering, for stderr.
    pub fn message(&self) -> String {
        match self {
            Warning::UnsupportedElement {
                kind,
                location,
                action,
            } => format!("unsupported element <{kind}> at {location}: {action}"),
            Warning::Degraded { what, why } => format!("degraded {what}: {why}"),
            Warning::ImageGeometryDegraded { what, why } => {
                format!("image geometry degraded ({what}): {why}")
            }
            Warning::AssetIssue { asset, why } => format!("asset {asset}: {why}"),
            Warning::ExternalImageNotFetched { url } => {
                format!("linked image not fetched (by design): {url}")
            }
            Warning::MacrosIgnored { part } => format!("macros ignored in {part}"),
            Warning::RevisionsAccepted { count } => {
                format!("{count} tracked change(s) taken as accepted")
            }
            Warning::RawBlockDropped { id, format } => {
                format!("raw-block {id} dropped: source dialect is {format}")
            }
        }
    }
}

/// Counters describing what a conversion processed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct ConversionStats {
    pub paragraphs: u32,
    pub headings: u32,
    pub lists: u32,
    pub tables: u32,
    pub images: u32,
    pub footnotes: u32,
    pub sheets: u32,
    pub cells: u32,
    pub formulas: u32,
    pub styles: u32,
}

/// The outcome of a conversion, beyond the document itself.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct ConversionReport {
    pub warnings: Vec<Warning>,
    pub raw_blocks_emitted: u32,
    pub stats: ConversionStats,
}

impl ConversionReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn warn(&mut self, warning: Warning) {
        self.warnings.push(warning);
    }

    /// Records an unsupported element that was preserved as a raw-block.
    pub fn raw_block(&mut self, kind: impl Into<String>, location: impl Into<String>) {
        self.raw_blocks_emitted += 1;
        self.warn(Warning::UnsupportedElement {
            kind: kind.into(),
            location: location.into(),
            action: "raw-block".into(),
        });
    }

    /// The worst severity present, if any.
    pub fn max_severity(&self) -> Option<Severity> {
        self.warnings.iter().map(|w| w.severity()).max()
    }

    /// True when at least one warning is [`Severity::Severe`]; drives the
    /// CLI's exit code 1 and `--strict`.
    pub fn has_severe(&self) -> bool {
        self.max_severity() == Some(Severity::Severe)
    }

    /// Folds another report into this one.
    pub fn merge(&mut self, other: ConversionReport) {
        self.warnings.extend(other.warnings);
        self.raw_blocks_emitted += other.raw_blocks_emitted;
        let s = other.stats;
        self.stats.paragraphs += s.paragraphs;
        self.stats.headings += s.headings;
        self.stats.lists += s.lists;
        self.stats.tables += s.tables;
        self.stats.images += s.images;
        self.stats.footnotes += s.footnotes;
        self.stats.sheets += s.sheets;
        self.stats.cells += s.cells;
        self.stats.formulas += s.formulas;
        self.stats.styles += s.styles;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_blocks_are_counted_and_reported() {
        let mut report = ConversionReport::new();
        report.raw_block("w:sdt", "word/document.xml:12");
        assert_eq!(report.raw_blocks_emitted, 1);
        assert_eq!(report.warnings.len(), 1);
        assert_eq!(report.max_severity(), Some(Severity::Minor));
        assert!(!report.has_severe());
    }

    #[test]
    fn dropping_an_element_is_severe_preserving_it_is_not() {
        let dropped = Warning::UnsupportedElement {
            kind: "w:object".into(),
            location: "x".into(),
            action: "skipped".into(),
        };
        let kept = Warning::UnsupportedElement {
            kind: "w:object".into(),
            location: "x".into(),
            action: "raw-block".into(),
        };
        assert_eq!(dropped.severity(), Severity::Severe);
        assert_eq!(kept.severity(), Severity::Minor);
    }

    #[test]
    fn merge_accumulates_everything() {
        let mut a = ConversionReport::new();
        a.stats.paragraphs = 2;
        a.raw_block("x", "y");
        let mut b = ConversionReport::new();
        b.stats.paragraphs = 3;
        b.stats.images = 1;
        b.warn(Warning::MacrosIgnored {
            part: "word/vbaProject.bin".into(),
        });
        a.merge(b);
        assert_eq!(a.stats.paragraphs, 5);
        assert_eq!(a.stats.images, 1);
        assert_eq!(a.warnings.len(), 2);
        assert_eq!(a.raw_blocks_emitted, 1);
    }

    #[test]
    fn messages_are_one_line() {
        let w = Warning::ImageGeometryDegraded {
            what: "VML shape".into(),
            why: "no z-order in source".into(),
        };
        assert!(!w.message().contains('\n'));
    }
}

//! OpenDocument readers and writers (`.odt`, `.ods`).
//!
//! Scheduled for **Phase 4** of `docs/development-plan.md`. The crate exists
//! from Phase 0 so that the workspace layout and the dependency rules of
//! `AGENTS.md` §3 are enforced by the compiler from the start: this crate may
//! depend on `docsai-model` and on nothing else in the workspace.

#![forbid(unsafe_code)]

use docsai_model::Format;

/// Formats this crate will handle.
pub const FORMATS: &[Format] = &[Format::Odt, Format::Ods];

#[cfg(test)]
mod tests {
    #[test]
    fn formats_are_the_odf_pair() {
        assert_eq!(super::FORMATS.len(), 2);
    }
}

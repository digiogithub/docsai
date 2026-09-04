//! Phase 19-B and 19-D: the two routes into a legacy `.ppt`.
//!
//! The phase's first acceptance criterion is «with LibreOffice, fidelity
//! equivalent to the pptx path», and `corpus/ppt/` was built to make that
//! measurable rather than assertable: `basic-slides.ppt` and `mixed-media.ppt`
//! were produced *from* `.pptx` originals that are still in the tree, so both
//! ends of the comparison exist. These tests convert both ends through the same
//! reader and compare what came out.
//!
//! The comparison is of **content**, not of bytes: LibreOffice's export to the
//! binary format and its import back are what stand between the two files, and
//! what they change is recorded in `kb/102-phase-19-loffice.md` rather than
//! asserted away.
//!
//! 19-D's criterion — «without LibreOffice, full text with correct slide
//! boundaries, no panics» — is measured against the *same* two `.pptx`
//! originals, with `--use-loffice never`, so «full» and «correct» are a
//! comparison rather than an opinion. Those tests need no LibreOffice and are
//! never skipped.

use std::path::{Path, PathBuf};

use docsai_convert::{ConvertOptions, UseLoffice, SUPPORT};
use docsai_model::Format;

fn corpus(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(relative)
}

/// The LibreOffice-dependent tests are skipped where LibreOffice is not
/// installed, exactly as `render_gate.py` skips the renderer. A skipped test
/// says so; it never passes by pretending.
fn soffice() -> Option<PathBuf> {
    docsai_convert::loffice::find_soffice()
}

/// Converts a corpus file, writing every sidecar asset into `scratch`.
///
/// `assets_dir` is not decoration. `docsai convert` defaults media to
/// `assets/` **next to the input**, so a test that leaves it unset writes into
/// `corpus/` — which is a golden tree, checked byte for byte by
/// `corpus/generate.py --check`. The first run dirties it and the second fails
/// on what the first left behind. Every conversion here is given a temporary
/// directory instead, and the test depends on nothing about where the default
/// would have put them.
fn convert(
    path: &Path,
    policy: UseLoffice,
    scratch: &Path,
) -> Result<docsai_convert::Outcome, String> {
    docsai_convert::convert_file(
        path,
        None,
        &ConvertOptions {
            use_loffice: policy,
            assets_dir: Some(scratch.to_path_buf()),
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())
}

/// A temporary directory that lives for the duration of one test.
fn scratch() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("docsai-ppt-19b-")
        .tempdir()
        .expect("a temporary directory for this test's assets")
}

/// The visible text of a DocMark body, one entry per non-empty line, with the
/// inline attribute syntax removed.
///
/// This is what «equivalent» is measured on: the same slides, in the same
/// order, carrying the same words at the same list depth. The *attributes* are
/// deliberately dropped, because they are precisely where the two ends differ —
/// see the KB entry.
fn visible_lines(markdown: &str) -> Vec<String> {
    let body = markdown
        .split_once("\n---\n")
        .map(|(_, rest)| rest)
        .unwrap_or(markdown);
    body.lines()
        .map(strip_inline_syntax)
        .filter(|line| !line.is_empty() && line != ":::")
        .collect()
}

fn strip_inline_syntax(line: &str) -> String {
    // Attribute blocks first: `{...}` never nests in DocMark inline syntax.
    let mut out = String::with_capacity(line.len());
    let mut depth = 0usize;
    for ch in line.chars() {
        match ch {
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    // Then the span brackets that carried them.
    out.retain(|ch| ch != '[' && ch != ']');
    out.trim_end().to_string()
}

#[test]
fn support_matrix_lists_ppt_read_and_never_write() {
    let row = SUPPORT
        .iter()
        .find(|s| s.format == Format::Ppt)
        .expect("ppt is in the support matrix");
    assert!(row.read, "a .ppt is readable through LibreOffice");
    assert!(
        !row.write,
        "writing .ppt is out of scope for the whole of Phase 19"
    );
}

#[test]
fn a_ppt_is_detected_by_its_powerpoint_document_stream() {
    for name in ["basic-slides.ppt", "mixed-media.ppt", "encrypted.ppt"] {
        let mut file = std::fs::File::open(corpus(&format!("ppt/{name}"))).unwrap();
        let (format, score) = docsai_office::detect(&mut file, Some(name));
        assert_eq!(format, Format::Ppt, "{name}");
        assert_eq!(score, docsai_office::DetectScore::Certain, "{name}");
    }
}

#[test]
fn never_reads_the_deck_natively_instead_of_refusing_it() {
    // Until 19-D this returned `ReadError::NoNativeRoute`. It reads now, and
    // the three `--use-loffice` values keep their `.doc` meanings.
    let scratch = scratch();
    let outcome = convert(
        &corpus("ppt/basic-slides.ppt"),
        UseLoffice::Never,
        scratch.path(),
    )
    .expect("the native degraded reader is 19-D and is in this build");
    assert_eq!(outcome.source_format, Format::Ppt);
    assert_eq!(
        visible_lines(&outcome.markdown)
            .into_iter()
            .filter(|l| l.starts_with("## "))
            .collect::<Vec<_>>(),
        vec!["## Informe trimestral", "## Siguientes pasos"],
    );
}

#[test]
fn the_native_route_reads_the_same_text_and_boundaries_as_the_pptx_original() {
    // 19-D's acceptance criterion, measured. No LibreOffice anywhere in this
    // test: the legacy deck is read by our own atom walk and compared against
    // the `.pptx` it was made from, read by the Phase 13 reader.
    let scratch = scratch();
    for (legacy, original) in [
        ("ppt/basic-slides.ppt", "pptx/basic-slides.pptx"),
        ("ppt/mixed-media.ppt", "ppt/mixed-media.source.pptx"),
    ] {
        let native = convert(&corpus(legacy), UseLoffice::Never, scratch.path()).expect(legacy);
        let direct = convert(&corpus(original), UseLoffice::Never, scratch.path()).expect(original);
        assert_eq!(native.source_format, Format::Ppt, "{legacy}");

        let text = |markdown: &str| {
            visible_lines(markdown)
                .into_iter()
                .filter(|l| l.starts_with("## ") || l.trim_start().starts_with("- "))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            text(&native.markdown),
            text(&direct.markdown),
            "every slide title and every bullet, at its own indent level ({legacy})"
        );
    }
}

#[test]
fn the_native_route_recovers_the_picture_with_its_place_on_the_slide() {
    let scratch = scratch();
    let native = convert(
        &corpus("ppt/mixed-media.ppt"),
        UseLoffice::Never,
        scratch.path(),
    )
    .unwrap();
    let direct = convert(
        &corpus("ppt/mixed-media.source.pptx"),
        UseLoffice::Never,
        scratch.path(),
    )
    .unwrap();
    let asset_of = |markdown: &str| {
        markdown
            .split("assets/")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .map(str::to_string)
    };
    // The asset name is a content hash: the same name is the same bytes.
    assert_eq!(asset_of(&native.markdown), asset_of(&direct.markdown));
    // And it is placed, which is why nothing warns that it is not.
    assert!(
        native.markdown.contains("pos=\"160px,240px\""),
        "{}",
        native.markdown
    );
    assert!(
        !native
            .report
            .warnings
            .iter()
            .any(|w| matches!(w, docsai_model::Warning::ImageGeometryDegraded { .. })),
        "{:?}",
        native.report.warnings
    );
}

#[test]
fn the_native_route_warns_once_and_only_about_what_it_really_lost() {
    let scratch = scratch();
    for name in ["ppt/basic-slides.ppt", "ppt/mixed-media.ppt"] {
        let outcome = convert(&corpus(name), UseLoffice::Never, scratch.path()).unwrap();
        assert_eq!(
            outcome.report.warnings.len(),
            1,
            "{name}: {:?}",
            outcome.report.warnings
        );
        match outcome.report.warnings.first() {
            Some(docsai_model::Warning::Degraded { what, why }) => {
                assert_eq!(what, "ppt");
                assert!(why.contains("--use-loffice"), "{why}");
            }
            other => panic!("{name}: {other:?}"),
        }
    }
}

#[test]
fn the_native_route_refuses_the_truncated_deck_from_its_own_bounds_check() {
    // 19-B's refusal of this file was LibreOffice's. This one is ours, and it
    // names the offset the `Current User` stream declared and the length the
    // stream really has.
    let scratch = scratch();
    let err = convert(
        &corpus("ppt/truncated.ppt"),
        UseLoffice::Never,
        scratch.path(),
    )
    .expect_err("half a deck is not a deck");
    assert!(err.contains("truncated"), "{err}");
    assert!(err.contains("10946"), "{err}");
    assert!(err.contains("5632"), "{err}");
    assert!(!err.contains("LibreOffice"), "the refusal is ours: {err}");
}

#[test]
fn the_native_route_never_reads_an_encrypted_deck_silently() {
    // Criterion 3 is 19-E's, and the check belongs in front of both routes.
    // What 19-D owes is that the file is not read *as if it were plain*.
    let scratch = scratch();
    let outcome = convert(
        &corpus("ppt/encrypted.ppt"),
        UseLoffice::Never,
        scratch.path(),
    )
    .unwrap();
    assert!(outcome.report.has_severe(), "{:?}", outcome.report.warnings);
    assert!(
        outcome.report.warnings.iter().any(|w| matches!(
            w,
            docsai_model::Warning::UnsupportedElement { kind, .. } if kind.contains("encrypted")
        )),
        "{:?}",
        outcome.report.warnings
    );
}

#[test]
fn the_libreoffice_route_reads_the_same_slides_as_the_pptx_original() {
    let Some(_) = soffice() else {
        eprintln!("skipped: LibreOffice not installed");
        return;
    };
    let scratch = scratch();
    for (legacy, original) in [
        ("ppt/basic-slides.ppt", "pptx/basic-slides.pptx"),
        ("ppt/mixed-media.ppt", "ppt/mixed-media.source.pptx"),
    ] {
        let via_lo = convert(&corpus(legacy), UseLoffice::Require, scratch.path()).expect(legacy);
        let direct = convert(&corpus(original), UseLoffice::Never, scratch.path()).expect(original);

        assert_eq!(via_lo.source_format, Format::Ppt, "{legacy}");
        assert_eq!(direct.source_format, Format::Pptx, "{original}");

        let legacy_slides = visible_lines(&via_lo.markdown)
            .into_iter()
            .filter(|l| l.starts_with("## "))
            .collect::<Vec<_>>();
        let original_slides = visible_lines(&direct.markdown)
            .into_iter()
            .filter(|l| l.starts_with("## "))
            .collect::<Vec<_>>();
        assert_eq!(
            legacy_slides, original_slides,
            "slide count, order and titles must survive the trip out to .ppt and back ({legacy})"
        );

        let legacy_bullets = visible_lines(&via_lo.markdown)
            .into_iter()
            .filter(|l| l.trim_start().starts_with("- "))
            .collect::<Vec<_>>();
        let original_bullets = visible_lines(&direct.markdown)
            .into_iter()
            .filter(|l| l.trim_start().starts_with("- "))
            .collect::<Vec<_>>();
        assert_eq!(
            legacy_bullets, original_bullets,
            "every bullet, at its own indent level, must survive ({legacy})"
        );
    }
}

#[test]
fn the_route_says_it_took_the_route_and_says_nothing_it_did_not_do() {
    let Some(_) = soffice() else {
        eprintln!("skipped: LibreOffice not installed");
        return;
    };
    let scratch = scratch();
    let outcome = convert(
        &corpus("ppt/basic-slides.ppt"),
        UseLoffice::Require,
        scratch.path(),
    )
    .unwrap();
    let degraded: Vec<_> = outcome
        .report
        .warnings
        .iter()
        .filter_map(|w| match w {
            docsai_model::Warning::Degraded { what, why } => Some((what.clone(), why.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        degraded.len(),
        1,
        "exactly one degradation warning, and it is the route itself: {:?}",
        outcome.report.warnings
    );
    assert_eq!(degraded[0].0, "ppt-via-libreoffice");
    assert!(degraded[0].1.contains("pptx"), "{}", degraded[0].1);
}

#[test]
fn a_picture_survives_the_legacy_round_trip_byte_for_byte() {
    let Some(_) = soffice() else {
        eprintln!("skipped: LibreOffice not installed");
        return;
    };
    let scratch = scratch();
    let via_lo = convert(
        &corpus("ppt/mixed-media.ppt"),
        UseLoffice::Require,
        scratch.path(),
    )
    .unwrap();
    let direct = convert(
        &corpus("ppt/mixed-media.source.pptx"),
        UseLoffice::Never,
        scratch.path(),
    )
    .unwrap();
    // The asset name is a content hash, so the same name is the same bytes: the
    // picture reaches the IR unchanged, and the geometry around it is what the
    // KB entry records as differing.
    let asset_of = |markdown: &str| {
        markdown
            .split("assets/")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .map(str::to_string)
    };
    assert_eq!(
        asset_of(&via_lo.markdown),
        asset_of(&direct.markdown),
        "the picture's bytes must be the bytes of the original"
    );
}

#[test]
fn libreoffice_refuses_the_truncated_deck_rather_than_inventing_one() {
    let Some(_) = soffice() else {
        eprintln!("skipped: LibreOffice not installed");
        return;
    };
    // 19-A cut the `PowerPoint Document` stream in half. LibreOffice declines
    // to load it, which is the honest outcome — but the refusal is *its*, not
    // ours, and 19-D owes the same answer without it.
    let scratch = scratch();
    let err = convert(
        &corpus("ppt/truncated.ppt"),
        UseLoffice::Require,
        scratch.path(),
    )
    .expect_err("a half a deck is not a deck");
    assert!(err.contains("LibreOffice"), "{err}");
}

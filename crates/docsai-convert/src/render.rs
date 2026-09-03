//! Slide thumbnails: a raster of a whole slide, drawn by headless LibreOffice
//! (plan v2, Phase 16, task 4).
//!
//! # What a thumbnail is here, and what it is not
//!
//! In this project a **thumbnail is a raster of a whole page** — for a deck, of
//! a whole slide, chrome and background and every shape on it, as a consumer
//! would see it. It is produced by an external renderer and it is not part of
//! the document: nothing in the package contains it, and asking for one twice
//! can give two different files if the renderer or its fonts changed.
//!
//! That is a different thing from the *downscaled copies of the pictures a
//! document embeds*, which the MCP server serves under
//! `include_images=downscaled` (`docsai-mcp::images`). Those come out of the
//! package, are reproducible from it, and say nothing about how a slide looks.
//! The two used to share the word «thumbnails», which is why the rung was
//! renamed: see `kb/81-phase-16-thumbnails.md`.
//!
//! # How the image is made
//!
//! The same two steps the render gate uses (`corpus/render_gate.py`,
//! [[71-phase-15-render]] decision 1), and for the same reason: LibreOffice
//! asked for a PNG directly exports **one page**, so a deck rendered that way
//! would be its first slide and thirty-nine silent lies. PDF is the only
//! LibreOffice export that carries the whole deck.
//!
//! ```text
//! soffice --headless --convert-to pdf   →   pdftoppm -png -r <dpi>
//! ```
//!
//! No image dependency is added to the workspace by this: `pdftoppm` writes
//! the PNG, and the only thing read out of it here is its `IHDR`, which is
//! eight bytes at a fixed offset.
//!
//! # Everything happens in a temporary directory
//!
//! One `TempDir` per call holds the isolated LibreOffice user profile
//! (`-env:UserInstallation=file://…`, so concurrent runs and the operator's own
//! profile never meet), the PDF and the rasters. Nothing is written outside it;
//! the caller decides whether the bytes reach a file at all.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use docsai_model::{Document, MemoryAssetStore, NodeId};
use serde::Serialize;

use crate::loffice;
use crate::pipeline::ConvertOptions;
use crate::ConvertError;

/// Environment variable naming the `pdftoppm` binary.
pub const ENV_PDFTOPPM: &str = "DOCSAI_PDFTOPPM";
/// Environment variable naming the directory LibreOffice's own shared objects
/// live in, for installs whose loader cannot find them by itself.
pub const ENV_LIBRARY_PATH: &str = "DOCSAI_LIBREOFFICE_LIBPATH";

/// Default resolution: a 10 × 7.5 inch slide becomes 960 × 720.
pub const DEFAULT_DPI: u32 = 96;
/// Resolutions accepted; outside this a thumbnail is either unreadable or no
/// longer a thumbnail.
pub const MIN_DPI: u32 = 24;
pub const MAX_DPI: u32 = 300;
/// Default wall-clock ceiling for the whole render.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

/// How to draw the thumbnails.
#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    pub dpi: u32,
    pub timeout: Duration,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            dpi: DEFAULT_DPI,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

impl RenderOptions {
    /// Refuses a resolution outside [`MIN_DPI`]..=[`MAX_DPI`] rather than
    /// clamping it: a caller who asked for 5 000 dpi asked for something else.
    pub fn with_dpi(mut self, dpi: u32) -> Result<Self, ConvertError> {
        if !(MIN_DPI..=MAX_DPI).contains(&dpi) {
            return Err(ConvertError::Invalid(format!(
                "dpi {dpi} is outside {MIN_DPI}..={MAX_DPI}"
            )));
        }
        self.dpi = dpi;
        Ok(self)
    }
}

/// One rendered slide.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct SlideThumbnail {
    /// 1-based position in the deck's own slide order — the primary address,
    /// because it is the only one the renderer can be held to.
    pub slide_number: usize,
    /// The slide's stable node id, when the deck's slide count and the
    /// renderer's page count agree so the two can be paired honestly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<NodeId>,
    pub file_name: String,
    pub content_type: &'static str,
    pub width: u32,
    pub height: u32,
    #[serde(skip)]
    pub data: Vec<u8>,
}

/// Everything one call produced.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct SlideThumbnails {
    pub dpi: u32,
    /// The renderer's own version string, so a difference between two runs can
    /// be attributed rather than guessed at.
    pub renderer: String,
    pub slides: Vec<SlideThumbnail>,
    /// Facts the caller has to know and that are not failures — chiefly a page
    /// count that does not match the deck's slide count, which is what makes
    /// the node ids unattributable.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Why this machine cannot draw a thumbnail.
///
/// A sentence, not a code: every caller does the same thing with it — say so
/// and carry on — and the sentence is the part that saves the next person an
/// afternoon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererUnavailable {
    pub reason: String,
}

impl std::fmt::Display for RendererUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

/// A renderer this machine can actually run.
///
/// Holding one is the proof: it is only ever built by [`probe_renderer`], which
/// builds it after both tools have run and said what they are.
#[derive(Debug, Clone)]
pub struct Renderer {
    soffice: PathBuf,
    pdftoppm: PathBuf,
    /// Prepended to `LD_LIBRARY_PATH` for the child, when plain invocation
    /// could not load LibreOffice's own libraries and this could.
    library_path: Option<PathBuf>,
    version: String,
}

impl Renderer {
    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn soffice(&self) -> &Path {
        &self.soffice
    }

    /// The directory added to the child's library search path, if any. `None`
    /// on a machine whose loader finds LibreOffice's libraries unaided.
    pub fn library_path(&self) -> Option<&Path> {
        self.library_path.as_deref()
    }
}

/// Whether this machine can render, and — when it cannot — why.
///
/// **The answer is a measurement, never an inference.** Both tools are *run*,
/// and LibreOffice has to identify itself in its own output before it counts as
/// present. That is not pedantry: an incomplete or mis-linked LibreOffice sits
/// on `PATH`, is executable, exits **0**, and prints a dynamic-loader error
/// instead of a version ([[71-phase-15-render]], «The loader fault»). A probe
/// that stopped at `is_executable` would report that machine as able to render
/// and every skip on it as absent — both of them false.
///
/// When the plain invocation fails, one more thing is tried before giving up:
/// the installation's **own** `program` directory — the parent of the resolved
/// `soffice` — on the child's `LD_LIBRARY_PATH`. That is the documented
/// workaround for a LibreOffice whose `RUNPATH $ORIGIN` is not honoured and
/// whose launcher exports the variable only on the BSDs. It is derived from
/// wherever the binary turned out to be, so no path from any one machine is
/// written down anywhere; `DOCSAI_LIBREOFFICE_LIBPATH` overrides the derivation
/// for an install that keeps its libraries somewhere else.
pub fn probe_renderer() -> Result<Renderer, RendererUnavailable> {
    let soffice = loffice::find_soffice().ok_or_else(|| RendererUnavailable {
        reason: "LibreOffice was not found: install it, or set DOCSAI_LIBREOFFICE to the \
                 soffice binary"
            .into(),
    })?;

    let (version, library_path) =
        resolve_soffice(&soffice).map_err(|plain| RendererUnavailable {
            reason: format!(
                "`{}` is installed but does not run: {plain}",
                soffice.display()
            ),
        })?;

    let pdftoppm = find_pdftoppm().ok_or_else(|| RendererUnavailable {
        reason: "pdftoppm was not found: install poppler-utils, or set DOCSAI_PDFTOPPM to the \
                 binary. LibreOffice alone exports one page of a deck, so the rasteriser is not \
                 optional"
            .into(),
    })?;

    Ok(Renderer {
        soffice,
        pdftoppm,
        library_path,
        version,
    })
}

/// Makes `soffice` run, or says why it does not.
///
/// `Ok((version, library_path))`: the version line it printed, and the
/// directory that had to be put on the loader's path to get it, if any.
///
/// Shared with the legacy-`.doc` fallback in [`crate::loffice`], which suffers
/// from exactly the same fault for exactly the same reason: before this, a
/// `--use-loffice require` on a machine like the one in [[71-phase-15-render]]
/// failed with a dynamic-loader error even though LibreOffice was installed,
/// complete and perfectly able to do the conversion.
pub(crate) fn resolve_soffice(soffice: &Path) -> Result<(String, Option<PathBuf>), String> {
    match soffice_version(soffice, None) {
        Ok(version) => Ok((version, None)),
        Err(plain) => {
            let Some(dir) = library_fallback(soffice) else {
                return Err(plain);
            };
            match soffice_version(soffice, Some(&dir)) {
                Ok(version) => Ok((version, Some(dir))),
                Err(_) => Err(plain),
            }
        }
    }
}

fn library_fallback(soffice: &Path) -> Option<PathBuf> {
    if let Ok(value) = std::env::var(ENV_LIBRARY_PATH) {
        if !value.trim().is_empty() {
            return Some(PathBuf::from(value));
        }
    }
    // The installation's own program directory: `/usr/bin/soffice` is a symlink
    // into it, so resolving the link is what finds it.
    std::fs::canonicalize(soffice)
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
}

fn find_pdftoppm() -> Option<PathBuf> {
    if let Ok(value) = std::env::var(ENV_PDFTOPPM) {
        if !value.trim().is_empty() {
            let path = PathBuf::from(value);
            return loffice::is_executable(&path).then_some(path);
        }
    }
    loffice::search_path("pdftoppm")
}

/// Runs `soffice --version` and insists on being told it is LibreOffice.
fn soffice_version(soffice: &Path, library_path: Option<&Path>) -> Result<String, String> {
    let mut command = Command::new(soffice);
    command.arg("--headless").arg("--version");
    apply_library_path(&mut command, library_path);
    let output = command
        .output()
        .map_err(|e| format!("could not be spawned: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(line) = stdout.lines().find(|line| line.contains("LibreOffice")) {
        return Ok(line.trim().to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr
        .lines()
        .chain(stdout.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("it printed no version");
    Err(detail.to_string())
}

pub(crate) fn apply_library_path(command: &mut Command, library_path: Option<&Path>) {
    let Some(dir) = library_path else { return };
    // `LD_LIBRARY_PATH` is the loader variable on the platforms where this
    // fault occurs. macOS bundles carry their libraries with them and Windows
    // resolves from the executable's directory, so neither needs it.
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let existing = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
        let value = if existing.is_empty() {
            dir.display().to_string()
        } else {
            format!("{}:{existing}", dir.display())
        };
        command.env("LD_LIBRARY_PATH", value);
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        let _ = (command, dir);
    }
}

/// Draws every slide of `input`, in the deck's own order.
///
/// `slide_ids` pairs the rasters with the IR: it is the deck's slide ids, in
/// order, and it is used **only** when its length matches the number of pages
/// the renderer produced. A deck can hide a slide, and a hidden slide is a page
/// the exporter may drop; pairing by position after that would attach one
/// slide's id to another slide's picture, which is a lie of exactly the kind
/// §7 rule 3 forbids. When the counts disagree the ids are dropped and a note
/// says so.
pub fn render_slides(
    renderer: &Renderer,
    input: &Path,
    slide_ids: &[Option<NodeId>],
    options: &RenderOptions,
) -> Result<SlideThumbnails, ConvertError> {
    let scratch = tempfile::Builder::new()
        .prefix("docsai-render-")
        .tempdir()
        .map_err(|source| ConvertError::Io {
            path: "temporary directory".into(),
            source,
        })?;
    let work = scratch.path();
    let profile = work.join("lo-profile");
    std::fs::create_dir_all(&profile).map_err(|source| ConvertError::Io {
        path: profile.display().to_string(),
        source,
    })?;

    let pdf = export_pdf(renderer, input, work, &profile, options)?;
    let pages = rasterise(renderer, &pdf, work, options)?;

    let mut notes = Vec::new();
    let paired = if slide_ids.is_empty() {
        false
    } else if slide_ids.len() == pages.len() {
        true
    } else {
        notes.push(format!(
            "the deck has {} slide(s) and the renderer produced {} page(s); the thumbnails are \
             numbered by page and carry no node ids, because pairing them by position would \
             attach one slide's id to another slide's picture",
            slide_ids.len(),
            pages.len()
        ));
        false
    };

    let width = pages.len().to_string().len().max(2);
    let mut slides = Vec::with_capacity(pages.len());
    for (index, page) in pages.iter().enumerate() {
        let data = std::fs::read(page).map_err(|source| ConvertError::Io {
            path: page.display().to_string(),
            source,
        })?;
        let (w, h) = png_dimensions(&data).ok_or_else(|| ConvertError::Renderer {
            message: format!(
                "{} is not a PNG the rasteriser should have written",
                page.display()
            ),
        })?;
        slides.push(SlideThumbnail {
            slide_number: index + 1,
            node_id: paired.then(|| slide_ids[index].clone()).flatten(),
            file_name: format!("slide-{:0width$}.png", index + 1, width = width),
            content_type: "image/png",
            width: w,
            height: h,
            data,
        });
    }

    Ok(SlideThumbnails {
        dpi: options.dpi,
        renderer: renderer.version.clone(),
        slides,
        notes,
    })
}

/// The deck's slide ids, in the deck's own order.
///
/// One extra read of the document, and it buys exactly one thing: the join key
/// between a picture and the node an agent can address. `outline_document`
/// hands an agent `n1`, `n3`, `n5`; without this the thumbnails would come back
/// numbered `1`, `2`, `3` and the agent would have to *assume* the mapping.
/// Assuming it is right almost always and wrong exactly where it matters — a
/// hidden slide, a deck whose slide list was restated — so it is computed.
///
/// The empty vector is returned, not an error, for a document that is not a
/// presentation: [`render_document_slides`] is where that is refused, once.
pub fn slide_ids(
    input: &Path,
    options: &ConvertOptions,
) -> Result<Vec<Option<NodeId>>, ConvertError> {
    let mut assets = MemoryAssetStore::new();
    let (mut document, _, _) = crate::read_path_with_options(input, &mut assets, options)?;
    // Ids are handed out by the addressing pass, not by the reader, and the
    // serializer is where the conversion path normally runs it. Nothing is
    // being serialized here, so it is run explicitly — with the caller's own
    // policy, so the ids on a thumbnail are the ids `outline_document` would
    // give for the same options and not a second numbering of the same deck.
    docsai_model::addressing::assign_ids(&mut document, options.id_policy());
    let Document::Presentation(deck) = document else {
        return Ok(Vec::new());
    };
    Ok(deck.slides.iter().map(|slide| slide.id.clone()).collect())
}

/// Draws every slide of a presentation, with each raster carrying the node id
/// of the slide it is a picture of.
///
/// Refuses anything that is not a presentation. A thumbnail here is *a slide*;
/// a page of a report is a different unit with a different address, and
/// answering one question with the other would be worse than answering
/// neither.
pub fn render_document_slides(
    renderer: &Renderer,
    input: &Path,
    options: &ConvertOptions,
    render: &RenderOptions,
) -> Result<SlideThumbnails, ConvertError> {
    let ids = slide_ids(input, options)?;
    if ids.is_empty() {
        return Err(ConvertError::Renderer {
            message: format!(
                "`{}` is not a presentation: a slide thumbnail is a raster of a slide, and this \
                 document has none",
                input.display()
            ),
        });
    }
    render_slides(renderer, input, &ids, render)
}

fn export_pdf(
    renderer: &Renderer,
    input: &Path,
    work: &Path,
    profile: &Path,
    options: &RenderOptions,
) -> Result<PathBuf, ConvertError> {
    let out_dir = work.join("pdf");
    let mut command = Command::new(&renderer.soffice);
    command
        .arg("--headless")
        .arg("--nologo")
        .arg("--nolockcheck")
        .arg("--norestore")
        .arg("--nodefault")
        .arg(format!(
            "-env:UserInstallation={}",
            loffice::path_to_file_uri(profile)
        ))
        .arg("--convert-to")
        .arg("pdf")
        .arg("--outdir")
        .arg(&out_dir)
        .arg(input);
    apply_library_path(&mut command, renderer.library_path.as_deref());
    let output = run(command, options.timeout, "LibreOffice")?;

    // `soffice` names the file after the input stem, and exits 0 whether or not
    // it wrote anything — so the file is the check, not the status.
    let produced = std::fs::read_dir(&out_dir)
        .ok()
        .and_then(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .find(|p| p.extension().and_then(|e| e.to_str()) == Some("pdf"))
        })
        .ok_or_else(|| ConvertError::Renderer {
            message: format!(
                "LibreOffice produced no PDF for `{}`{}",
                input.display(),
                last_line(&output)
            ),
        })?;
    Ok(produced)
}

fn rasterise(
    renderer: &Renderer,
    pdf: &Path,
    work: &Path,
    options: &RenderOptions,
) -> Result<Vec<PathBuf>, ConvertError> {
    let out_dir = work.join("png");
    std::fs::create_dir_all(&out_dir).map_err(|source| ConvertError::Io {
        path: out_dir.display().to_string(),
        source,
    })?;
    let mut command = Command::new(&renderer.pdftoppm);
    command
        .arg("-png")
        .arg("-r")
        .arg(options.dpi.to_string())
        .arg(pdf)
        .arg(out_dir.join("page"));
    let output = run(command, options.timeout, "pdftoppm")?;

    // `pdftoppm` zero-pads its suffix to the page count, so the names sort in
    // page order — but only within one run. Sorting by the number itself is
    // what makes that independent of the padding.
    let mut pages: Vec<(u32, PathBuf)> = std::fs::read_dir(&out_dir)
        .map_err(|source| ConvertError::Io {
            path: out_dir.display().to_string(),
            source,
        })?
        .flatten()
        .map(|entry| entry.path())
        .filter_map(|path| {
            let stem = path.file_stem()?.to_str()?;
            let number = stem.strip_prefix("page-")?.parse::<u32>().ok()?;
            Some((number, path))
        })
        .collect();
    pages.sort_by_key(|(number, _)| *number);
    if pages.is_empty() {
        return Err(ConvertError::Renderer {
            message: format!("pdftoppm rasterised no page{}", last_line(&output)),
        });
    }
    Ok(pages.into_iter().map(|(_, path)| path).collect())
}

/// Runs a child under a wall-clock ceiling, with its output captured.
///
/// The output goes to pipes that are only read after the child is gone, so the
/// child is spawned with them redirected to `null` when nothing is waiting —
/// here they are captured, and the ceiling is enforced by polling rather than
/// by blocking on a read that a hung child would never end.
fn run(mut command: Command, timeout: Duration, tool: &str) -> Result<String, ConvertError> {
    let temp = tempfile::Builder::new()
        .prefix("docsai-render-log-")
        .tempfile()
        .map_err(|source| ConvertError::Io {
            path: "temporary file".into(),
            source,
        })?;
    let log = temp.path().to_path_buf();
    let out = temp.reopen().map_err(|source| ConvertError::Io {
        path: log.display().to_string(),
        source,
    })?;
    let err = temp.reopen().map_err(|source| ConvertError::Io {
        path: log.display().to_string(),
        source,
    })?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|source| ConvertError::Renderer {
            message: format!("could not spawn {tool}: {source}"),
        })?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(source) => {
                return Err(ConvertError::Renderer {
                    message: format!("could not wait for {tool}: {source}"),
                })
            }
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ConvertError::Renderer {
                message: format!("{tool} did not finish within {}s", timeout.as_secs()),
            });
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Ok(std::fs::read_to_string(&log).unwrap_or_default())
}

fn last_line(output: &str) -> String {
    match output.lines().map(str::trim).rfind(|line| !line.is_empty()) {
        Some(line) => format!(": {line}"),
        None => String::new(),
    }
}

/// Width and height out of a PNG's `IHDR`.
///
/// Eight bytes at a fixed offset behind a fixed signature. Reading them is not
/// decoding the image and does not want a decoder.
fn png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if data.len() < 24 || data[..8] != SIGNATURE || &data[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(data[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(data[20..24].try_into().ok()?);
    (width > 0 && height > 0).then_some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpi_outside_the_range_is_refused_rather_than_clamped() {
        assert!(RenderOptions::default().with_dpi(10).is_err());
        assert!(RenderOptions::default().with_dpi(4000).is_err());
        assert_eq!(RenderOptions::default().with_dpi(150).unwrap().dpi, 150);
    }

    #[test]
    fn a_pngs_header_is_read_without_decoding_it() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&960u32.to_be_bytes());
        png.extend_from_slice(&720u32.to_be_bytes());
        assert_eq!(png_dimensions(&png), Some((960, 720)));
        assert_eq!(png_dimensions(b"not a png at all, truly not"), None);
    }

    #[test]
    fn a_missing_binary_is_a_sentence_a_person_can_act_on() {
        // Not the real probe: what is asserted is that the unavailable value
        // carries prose, because that is all any caller does with it.
        let why = RendererUnavailable {
            reason: "LibreOffice was not found".into(),
        };
        assert!(why.to_string().contains("LibreOffice"));
    }

    /// The probe is a measurement, so this test measures rather than asserts.
    ///
    /// It cannot require a renderer — most machines running the suite have
    /// none — but it can require that the answer is *consistent*: a machine the
    /// probe calls able must render the corpus deck it is given, and a machine
    /// it calls unable must say why in a sentence. Both halves are real
    /// coverage, and which one runs is decided by the machine, not by the test.
    #[test]
    fn the_probe_and_the_render_agree_with_each_other() {
        let deck = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/pptx/basic-slides.pptx")
            .canonicalize()
            .expect("the corpus deck");
        match probe_renderer() {
            Ok(renderer) => {
                assert!(renderer.version().contains("LibreOffice"));
                let out = render_slides(
                    &renderer,
                    &deck,
                    &[Some(NodeId("n1".into())), Some(NodeId("n3".into()))],
                    &RenderOptions::default(),
                )
                .expect("a machine that probes able must render");
                assert_eq!(out.slides.len(), 2, "basic-slides.pptx has two slides");
                assert!(out.notes.is_empty(), "{:?}", out.notes);
                for (index, slide) in out.slides.iter().enumerate() {
                    assert_eq!(slide.slide_number, index + 1);
                    assert!(slide.width > 0 && slide.height > 0);
                    assert!(slide.data.len() > 100, "an empty raster is not a render");
                    assert!(slide.node_id.is_some(), "the counts matched");
                }
            }
            Err(why) => {
                assert!(
                    !why.reason.trim().is_empty(),
                    "an unavailable renderer owes the caller a reason"
                );
            }
        }
    }
}

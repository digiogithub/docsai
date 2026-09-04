//! Optional LibreOffice headless fallback for legacy `.doc` (Phase 5).
//!
//! When `soffice` / `libreoffice` is available, `.doc` is converted to `.docx`
//! in a temporary directory and re-entered through the Phase 1 docx pipeline.
//! Detection never fails the process: missing LO simply leaves the native
//! degraded path in charge (unless the user passed `--use-loffice require`).

use std::path::{Path, PathBuf};
use std::process::Command;

use docsai_model::Format;

use crate::ConvertError;

/// Wall-clock ceiling on one headless conversion.
///
/// The same 300 s the slide renderer allows itself: a legacy binary file is
/// attacker-shaped input handed to a large C++ program, and a child with no
/// ceiling is a hang, not an error.
const CONVERT_TIMEOUT: std::time::Duration = crate::render::DEFAULT_TIMEOUT;

/// How aggressively to use LibreOffice for formats that benefit from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UseLoffice {
    /// Use LibreOffice when found on the system; otherwise native degraded.
    #[default]
    Auto,
    /// Never invoke LibreOffice; always use the native path.
    Never,
    /// Require LibreOffice; error if it cannot be found or fails.
    Require,
}

impl UseLoffice {
    /// Parses `auto`, `never`, or `require` (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(UseLoffice::Auto),
            "never" => Some(UseLoffice::Never),
            "require" => Some(UseLoffice::Require),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            UseLoffice::Auto => "auto",
            UseLoffice::Never => "never",
            UseLoffice::Require => "require",
        }
    }
}

/// Locates a LibreOffice program executable, if any.
///
/// Checks `DOCSAI_LIBREOFFICE` / `LIBREOFFICE_PATH`, then `PATH`, then common
/// install locations on Linux, macOS, and Windows.
pub fn find_soffice() -> Option<PathBuf> {
    // Explicit overrides win. If the variable is set but unusable, do not
    // silently fall through to PATH — that would make CI/tests non-deterministic
    // and surprise operators who pointed at a specific binary.
    for key in ["DOCSAI_LIBREOFFICE", "LIBREOFFICE_PATH"] {
        if let Ok(val) = std::env::var(key) {
            if val.trim().is_empty() {
                continue;
            }
            let p = PathBuf::from(&val);
            return if is_executable(&p) { Some(p) } else { None };
        }
    }
    for name in ["soffice", "libreoffice"] {
        if let Some(p) = search_path(name) {
            return Some(p);
        }
    }
    standard_locations()
        .into_iter()
        .find(|candidate| is_executable(candidate))
}

fn standard_locations() -> Vec<PathBuf> {
    let mut out = vec![
        // Linux
        PathBuf::from("/usr/bin/soffice"),
        PathBuf::from("/usr/bin/libreoffice"),
        PathBuf::from("/usr/lib/libreoffice/program/soffice"),
        PathBuf::from("/snap/bin/libreoffice"),
        // macOS
        PathBuf::from("/Applications/LibreOffice.app/Contents/MacOS/soffice"),
        // Windows (typical)
        PathBuf::from(r"C:\Program Files\LibreOffice\program\soffice.exe"),
        PathBuf::from(r"C:\Program Files (x86)\LibreOffice\program\soffice.exe"),
    ];
    if let Ok(program_files) = std::env::var("PROGRAMFILES") {
        out.push(
            PathBuf::from(program_files)
                .join("LibreOffice")
                .join("program")
                .join("soffice.exe"),
        );
    }
    if let Ok(program_files) = std::env::var("PROGRAMFILES(X86)") {
        out.push(
            PathBuf::from(program_files)
                .join("LibreOffice")
                .join("program")
                .join("soffice.exe"),
        );
    }
    out
}

pub(crate) fn search_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
        // Windows: try .exe
        let exe = dir.join(format!("{name}.exe"));
        if is_executable(&exe) {
            return Some(exe);
        }
    }
    None
}

pub(crate) fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            return meta.permissions().mode() & 0o111 != 0;
        }
        false
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Converts `input` to `.docx` via LibreOffice headless into a temp directory.
///
/// Returns the path to the produced `.docx` inside `out_dir`.
pub fn convert_to_docx(
    soffice: &Path,
    input: &Path,
    out_dir: &Path,
) -> Result<PathBuf, ConvertError> {
    convert_with_loffice(soffice, input, out_dir, "docx")
}

/// Converts `input` to `.pptx` via LibreOffice headless into a temp directory.
///
/// Phase 19-B. The deck that comes out is read by the Phase 13 `.pptx` reader
/// with no accommodation of any kind, which is what makes the phase's first
/// acceptance criterion — «with LibreOffice, fidelity equivalent to the pptx
/// path» — a measurement rather than a claim.
pub fn convert_to_pptx(
    soffice: &Path,
    input: &Path,
    out_dir: &Path,
) -> Result<PathBuf, ConvertError> {
    convert_with_loffice(soffice, input, out_dir, "pptx")
}

/// Runs one headless `--convert-to` in a sandbox and returns what it produced.
///
/// The sandbox is the one [`crate::render`] established in Phase 16-G and is
/// not a second discipline: an isolated `UserInstallation` profile inside
/// `out_dir` (so concurrent runs never share a profile and nothing is written
/// outside the caller's temporary directory), the library path derived at run
/// time from the resolved `soffice` — never hard-coded — and a wall-clock
/// ceiling enforced by polling, so a child that hangs on a hostile file is
/// killed instead of blocking the process for ever.
fn convert_with_loffice(
    soffice: &Path,
    input: &Path,
    out_dir: &Path,
    extension: &str,
) -> Result<PathBuf, ConvertError> {
    std::fs::create_dir_all(out_dir).map_err(|source| ConvertError::Io {
        path: out_dir.display().to_string(),
        source,
    })?;

    // User profile isolated per invocation so concurrent runs do not clash.
    let profile = out_dir.join("lo-profile");
    let _ = std::fs::create_dir_all(&profile);
    let profile_uri = path_to_file_uri(&profile);

    // An installed LibreOffice whose loader cannot find its own libraries runs
    // and fails; the same resolution the slide renderer uses (Phase 16-G) makes
    // it run here too, and costs one `--version` when it was never needed.
    let library_path = match crate::render::resolve_soffice(soffice) {
        Ok((_, library_path)) => library_path,
        Err(why) => {
            return Err(ConvertError::Loffice {
                message: format!("`{}` does not run: {why}", soffice.display()),
            })
        }
    };

    let mut command = Command::new(soffice);
    command
        .arg("--headless")
        .arg("--nologo")
        .arg("--nolockcheck")
        .arg("--norestore")
        .arg("--nodefault")
        .arg(format!("-env:UserInstallation={profile_uri}"))
        .arg("--convert-to")
        .arg(extension)
        .arg("--outdir")
        .arg(out_dir)
        .arg(input);
    crate::render::apply_library_path(&mut command, library_path.as_deref());
    let (status, log) = crate::render::run_with_status(command, CONVERT_TIMEOUT, "LibreOffice")
        .map_err(|error| ConvertError::Loffice {
            message: error.to_string(),
        })?;

    if !status.success() {
        return Err(ConvertError::Loffice {
            message: format!(
                "LibreOffice conversion to {extension} failed (exit {:?}): {}",
                status.code(),
                log.trim()
            ),
        });
    }

    // soffice names the output after the input stem.
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("converted");
    let candidate = out_dir.join(format!("{stem}.{extension}"));
    if candidate.is_file() {
        return Ok(candidate);
    }
    // Some LO builds may alter the name; pick any match in out_dir (not nested).
    if let Ok(entries) = std::fs::read_dir(out_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some(extension) {
                return Ok(path);
            }
        }
    }
    Err(ConvertError::Loffice {
        message: format!(
            "LibreOffice reported success but no .{extension} appeared in {}{}",
            out_dir.display(),
            if log.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", log.trim())
            }
        ),
    })
}

pub(crate) fn path_to_file_uri(path: &Path) -> String {
    // LibreOffice expects a file:// URI for UserInstallation.
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let mut s = abs.display().to_string();
    // Windows paths: C:\foo → file:///C:/foo
    if cfg!(windows) {
        s = s.replace('\\', "/");
        if let Some(rest) = s.strip_prefix("//?") {
            s = rest.to_string();
        }
        format!("file:///{s}")
    } else {
        format!("file://{s}")
    }
}

/// Whether this source format may benefit from a LibreOffice pre-conversion.
pub fn benefits_from_loffice(format: Format) -> bool {
    loffice_route(format).is_some()
}

/// The modern format a legacy one is pre-converted into, and its extension.
///
/// `.doc` → `.docx` since Phase 5, `.ppt` → `.pptx` since Phase 19-B. The pair
/// is what keeps one policy — `--use-loffice auto|never|require` — governing
/// both, instead of each legacy format inventing its own.
pub fn loffice_route(format: Format) -> Option<(Format, &'static str)> {
    match format {
        Format::Doc => Some((Format::Docx, "docx")),
        Format::Ppt => Some((Format::Pptx, "pptx")),
        _ => None,
    }
}

/// Resolves whether to attempt LO given policy and availability.
pub fn should_use(policy: UseLoffice, available: bool) -> Result<bool, ConvertError> {
    match policy {
        UseLoffice::Never => Ok(false),
        UseLoffice::Auto => Ok(available),
        UseLoffice::Require => {
            if available {
                Ok(true)
            } else {
                Err(ConvertError::Loffice {
                    message: "LibreOffice is required (--use-loffice require) but was not found. \
                         Install LibreOffice or set DOCSAI_LIBREOFFICE to the soffice binary"
                        .into(),
                })
            }
        }
    }
}

/// Tiny helper so tests can assert the policy parser without spawning LO.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_policy() {
        assert_eq!(UseLoffice::parse("AUTO"), Some(UseLoffice::Auto));
        assert_eq!(UseLoffice::parse("never"), Some(UseLoffice::Never));
        assert_eq!(UseLoffice::parse("require"), Some(UseLoffice::Require));
        assert_eq!(UseLoffice::parse("maybe"), None);
    }

    #[test]
    fn require_without_binary_errors() {
        assert!(should_use(UseLoffice::Require, false).is_err());
        assert!(!should_use(UseLoffice::Auto, false).unwrap());
        assert!(should_use(UseLoffice::Auto, true).unwrap());
        assert!(!should_use(UseLoffice::Never, true).unwrap());
    }

    #[test]
    fn the_legacy_formats_route_through_their_modern_twin() {
        assert_eq!(
            loffice_route(Format::Doc),
            Some((Format::Docx, "docx")),
            "Phase 5"
        );
        assert_eq!(
            loffice_route(Format::Ppt),
            Some((Format::Pptx, "pptx")),
            "Phase 19-B"
        );
        assert_eq!(
            loffice_route(Format::Xls),
            None,
            "`.xls` reads natively through calamine and wants no pre-conversion"
        );
        assert!(benefits_from_loffice(Format::Doc));
        assert!(benefits_from_loffice(Format::Ppt));
        assert!(!benefits_from_loffice(Format::Pptx));
    }

    #[test]
    fn file_uri_is_absolute_shaped() {
        let uri = path_to_file_uri(Path::new("/tmp/lo"));
        assert!(uri.starts_with("file://"), "{uri}");
    }
}

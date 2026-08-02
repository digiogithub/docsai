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

fn search_path(name: &str) -> Option<PathBuf> {
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

fn is_executable(path: &Path) -> bool {
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
    std::fs::create_dir_all(out_dir).map_err(|source| ConvertError::Io {
        path: out_dir.display().to_string(),
        source,
    })?;

    // User profile isolated per invocation so concurrent runs do not clash.
    let profile = out_dir.join("lo-profile");
    let _ = std::fs::create_dir_all(&profile);
    let profile_uri = path_to_file_uri(&profile);

    let status = Command::new(soffice)
        .arg("--headless")
        .arg("--nologo")
        .arg("--nolockcheck")
        .arg("--norestore")
        .arg("--nodefault")
        .arg(format!("-env:UserInstallation={profile_uri}"))
        .arg("--convert-to")
        .arg("docx")
        .arg("--outdir")
        .arg(out_dir)
        .arg(input)
        .output()
        .map_err(|source| ConvertError::Loffice {
            message: format!("failed to spawn `{}`: {source}", soffice.display()),
        })?;

    if !status.status.success() {
        let stderr = String::from_utf8_lossy(&status.stderr);
        let stdout = String::from_utf8_lossy(&status.stdout);
        return Err(ConvertError::Loffice {
            message: format!(
                "LibreOffice conversion failed (exit {:?}): {stderr}{stdout}",
                status.status.code()
            ),
        });
    }

    // soffice names the output after the input stem.
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("converted");
    let candidate = out_dir.join(format!("{stem}.docx"));
    if candidate.is_file() {
        return Ok(candidate);
    }
    // Some LO builds may alter the name; pick any .docx in out_dir (except nested).
    if let Ok(entries) = std::fs::read_dir(out_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("docx") {
                return Ok(path);
            }
        }
    }
    Err(ConvertError::Loffice {
        message: format!(
            "LibreOffice reported success but no .docx appeared in {}",
            out_dir.display()
        ),
    })
}

fn path_to_file_uri(path: &Path) -> String {
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
    matches!(format, Format::Doc)
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
    fn file_uri_is_absolute_shaped() {
        let uri = path_to_file_uri(Path::new("/tmp/lo"));
        assert!(uri.starts_with("file://"), "{uri}");
    }
}

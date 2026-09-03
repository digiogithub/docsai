//! `apply_edits`: a closed set of operations, applied transactionally.
//!
//! This module is 17-C (the transaction) and 17-D (`dry_run` and the report).
//! The operation set it implements is normative and lives in
//! [`docs/edit-operations.md`](../../../../docs/edit-operations.md); the door
//! that exposes it — a CLI flag and an MCP tool — is 17-E's, and finds here a
//! function to call and no logic to reimplement.
//!
//! # All-or-nothing
//!
//! §4 fixes the contract: «Either every operation applied or none did.» It is
//! implemented in the only way that makes it a property rather than an
//! intention:
//!
//! 1. The document is read **into memory**. Nothing on disk is touched.
//! 2. The whole operation list is **validated** before any of it is applied
//!    (§4's early pass: a request with a typo in it costs nothing).
//! 3. The operations are applied to the in-memory document, in order. A refusal
//!    at operation *k* returns and the in-memory document is dropped — there is
//!    no rollback to get wrong, because the only thing that was mutated was a
//!    value the failure path discards.
//! 4. The edited document is written to a **temporary file in the destination's
//!    own directory**.
//! 5. The temporary file **replaces** the original by rename.
//!
//! Steps 4 and 5 are where the honesty is owed, and the next section pays it.
//!
//! # Where `rename` gives atomicity, and where it does not
//!
//! The temporary file is created in the destination's own directory, never in
//! `/tmp`, for one reason: a rename across filesystems is not a rename. It is a
//! copy followed by an unlink, and a copy can be interrupted halfway, which is
//! precisely the failure the whole construction exists to prevent. Same
//! directory means same filesystem means the kernel's own rename.
//!
//! What the platforms actually promise:
//!
//! - **POSIX (Linux, macOS, the BSDs), local filesystem.** `rename(2)` is
//!   atomic *with respect to other processes*: a reader either sees the old file
//!   or the new one, never a partial one, and never a missing one. This is the
//!   guarantee this module relies on and it is a real guarantee.
//! - **Durability is not atomicity.** After a power loss, a filesystem may
//!   replay the rename without the data behind it. The temporary file is
//!   therefore flushed and `fsync`ed before the rename, and the containing
//!   directory is `fsync`ed after it, which is what makes the *name* durable as
//!   well as the bytes. A directory `fsync` is not available on every platform
//!   and is a no-op on some; where it fails, the edit is not failed for it —
//!   the file is complete and correctly named, and refusing an otherwise
//!   finished edit because the directory entry may need a fsck would be a false
//!   failure. The limit is stated rather than papered over: **on a crash, this
//!   module guarantees you never read a half-written document; it does not
//!   guarantee that a completed edit survives a power cut on a filesystem that
//!   reorders the directory entry.**
//! - **Windows.** `std::fs::rename` maps to `MoveFileEx` with
//!   `MOVEFILE_REPLACE_EXISTING`. No observer sees the destination missing, but
//!   three things differ from POSIX and none of them is fixable here. It
//!   **fails** if the destination is open by another process without
//!   `FILE_SHARE_DELETE` — a deck open in PowerPoint is the ordinary case, and
//!   the edit is refused with the operating system's own error rather than
//!   retried. The replacement's ACLs, alternate data streams and compression
//!   attributes are the *temporary file's*, not the original's, because the
//!   original's directory entry is discarded. And crash-atomicity on FAT and
//!   ReFS is weaker than NTFS's.
//! - **Network filesystems.** On NFS, atomicity is the server's property and
//!   not the client's: a client that retries a rename after a lost reply can
//!   see `ENOENT` for an operation that in fact succeeded, and `fsync` reaching
//!   stable storage depends on the export options. On SMB/CIFS, a replacing
//!   rename is not guaranteed atomic at all. **This module does not claim
//!   atomicity on a network filesystem** and cannot detect that it is on one
//!   cheaply enough to refuse.
//!
//! One more consequence of writing beside the destination: a hard crash between
//! step 4 and step 5 can leave a `.docsai-edit-*.tmp` file in the directory.
//! Every other failure removes it — the handle owns the file and its `Drop`
//! unlinks it — but a killed process runs no destructor. The name begins with a
//! dot and says what it is, which is the most that can be done for it.
//!
//! # `dry_run` runs everything except step 5
//!
//! 17-D's requirement is that a dry run answers with «the same report the real
//! call would return». That is only true if the dry run does everything the
//! real one does apart from replacing the file — including **writing the
//! document out**, because a writer refusal and a writer's warnings (§8.1) are
//! not knowable any other way. So `dry_run` writes the temporary file, throws
//! it away, and returns a report that is equal, field for field, to the one the
//! real call returns. Nothing in the report can only be known after the rename,
//! so nothing in the report differs, and
//! `crates/docsai-convert/tests/edit_transaction.rs` asserts the equality
//! rather than describing it.
//!
//! The price is stated: a dry run needs write permission on the destination's
//! directory. That is a feature — a dry run that passed and a real call that
//! then failed on a read-only directory would be a dry run that lied.

// See the note in `ops.rs`: a refusal is a large, rare value, and boxing it
// would put an allocation on the refusal path to save moving a struct that only
// exists when a request is already being rejected.
#![allow(clippy::result_large_err)]

pub mod fragment;
mod ops;
mod report;
mod request;

use std::io::Write;
use std::path::{Path, PathBuf};

use docsai_model::assets::AssetStore;
use docsai_model::{Document, Format, MemoryAssetStore};

use crate::assets::DirAssetStore;
use crate::pipeline::ConvertOptions;
use crate::ConvertError;

pub use report::{ChangedNode, CreatedNode, EditReport};
pub use request::{DocClass, EditRequest, Operation, Refusal, Refused, Scalar, OPS_VERSION};

/// Everything that can stop an edit.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    /// An operation was refused (§8). The document was not touched.
    #[error("{0}")]
    Refused(#[from] Refused),
    /// The document could not be read, or could not be written back.
    #[error("{0}")]
    Convert(#[from] ConvertError),
    /// The transaction itself failed: the temporary file, the rename.
    #[error("the edit could not be written to `{path}`: {source}")]
    Transaction {
        path: String,
        #[source]
        source: std::io::Error,
    },
    /// A fault injected by a test reached its point (17-C's evidence).
    ///
    /// Carries the temporary file's path where there is one, so a test can
    /// assert the thing this module promises: that it sits in the
    /// destination's **own** directory.
    #[error("injected fault at {at:?}")]
    InjectedFault {
        at: FaultPoint,
        temporary: Option<PathBuf>,
    },
}

/// Where a test may inject a failure, to prove the rollback rather than argue
/// it.
///
/// This is deliberately part of the public surface and deliberately marked
/// hidden. 17-C's brief is «proven by injecting one, not by argument», and a
/// failure in the *write* half cannot be provoked by any request: every refusal
/// a caller can trigger happens before a byte is written. A knob that only the
/// test suite sets is the honest way to reach the branch, and hiding it from the
/// documentation is the honest way to say it is not a feature.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultPoint {
    /// After the operations applied, before anything is serialised.
    AfterApply,
    /// After the temporary file holds the whole edited document, before the
    /// rename. This is the interesting one: the document is complete, on disk,
    /// under a different name.
    BeforeRename,
}

/// How an edit is run.
#[derive(Debug, Clone, Default)]
pub struct EditOptions {
    /// Read and write settings. `fidelity` and `ids` are checked against §3:
    /// version 1 edits at `full` and `agent` only.
    pub convert: ConvertOptions,
    #[doc(hidden)]
    pub fault: Option<FaultPoint>,
}

/// What an edit did.
#[derive(Debug)]
pub struct EditOutcome {
    /// The change report. Identical for a dry run and for the real call.
    pub report: EditReport,
    /// The file that was edited — or that would have been.
    pub path: PathBuf,
    pub format: Format,
    /// True when nothing replaced the file.
    ///
    /// Outside the report on purpose: a caller knows what it asked for, and a
    /// field that differed between a dry run and the real call would make the
    /// equality 17-D promises impossible to state.
    pub dry_run: bool,
}

/// Applies `request` to the document at `input`, in place.
///
/// The whole transaction, and the only entry point 17-E needs.
pub fn edit_path(
    input: &Path,
    request: &EditRequest,
    options: &EditOptions,
) -> Result<EditOutcome, EditError> {
    if let Some(requested) = request.ops {
        if requested != OPS_VERSION {
            return Err(EditError::Refused(Refused {
                index: 0,
                op: "<request>".into(),
                refusal: Refusal::UnsupportedOpsVersion {
                    requested,
                    speaks: OPS_VERSION,
                },
            }));
        }
    }
    refuse_unaddressable(&options.convert)?;

    let input = input.to_path_buf();
    let directory = input
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let assets_dir = options
        .convert
        .assets_dir
        .clone()
        .unwrap_or_else(|| directory.join("assets"));

    // A package keeps its media inside itself, so nothing may be written beside
    // it; a DocMark document keeps its media in `assets/`, which is where a
    // conversion would put them and where the document already points.
    let format = detect(&input)?;
    let mut memory = MemoryAssetStore::new();
    let mut files = DirAssetStore::new(assets_dir.clone());
    let docmark = format == Format::DocMark;

    let (mut document, source_format, mut report) = {
        let store: &mut dyn AssetStore = if docmark { &mut files } else { &mut memory };
        crate::pipeline::read_path_with_options(&input, store, &options.convert)?
    };

    // A reader does not put ids in the IR — the DocMark *writer* hands them out
    // as it goes, which is where a caller's `n12` came from. An edit addresses
    // by that name, so the same allocation has to happen here, in the same
    // document order, before any target is resolved. It is materialisation and
    // not invention: `assign_ids` is the walk `docsai_model::addressing`
    // already guarantees agrees with the serializer's, and it changes nothing a
    // package writer puts on disk (Phase 16 settled that a node id is not a
    // bookmark).
    docsai_model::addressing::assign_ids(&mut document, docsai_model::addressing::IdPolicy::Assign);

    let (edit_report, warnings) = {
        let store: &mut dyn AssetStore = if docmark { &mut files } else { &mut memory };
        apply_edits_in_memory(
            &mut document,
            store,
            request,
            Some(directory.clone()),
            Some(format),
        )?
    };
    report.warnings.extend(warnings);

    if options.fault == Some(FaultPoint::AfterApply) {
        return Err(EditError::InjectedFault {
            at: FaultPoint::AfterApply,
            temporary: None,
        });
    }

    docsai_model::validate::validate(&document).map_err(|errors| {
        ConvertError::Invalid(
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;

    // ---- the write half -------------------------------------------------
    let temp = tempfile::Builder::new()
        .prefix(".docsai-edit-")
        .suffix(".tmp")
        .tempfile_in(&directory)
        .map_err(|source| EditError::Transaction {
            path: directory.display().to_string(),
            source,
        })?;

    let mut write_report = if docmark {
        let mut options = options.convert.clone();
        options.target = Some(Format::DocMark);
        let outcome = crate::pipeline::write_document(
            document,
            source_format,
            Format::DocMark,
            Some(temp.path()),
            &options,
            &assets_dir,
            &mut files,
            docsai_model::ConversionReport::new(),
        )?;
        outcome.report
    } else {
        let file = std::fs::File::create(temp.path()).map_err(|source| EditError::Transaction {
            path: temp.path().display().to_string(),
            source,
        })?;
        match format {
            Format::Odt | Format::Ods => {
                docsai_odf::write(format, &document, &memory, file).map_err(ConvertError::from)?
            }
            other => {
                docsai_office::write(other, &document, &memory, file).map_err(ConvertError::from)?
            }
        }
    };
    report.merge(std::mem::take(&mut write_report));

    let mut edit_report = edit_report;
    edit_report.warnings = report.warnings.clone();

    // The document is complete on disk under a name nothing points at. Every
    // failure from here on unlinks it, because `temp` owns it.
    sync(temp.path())?;
    if options.fault == Some(FaultPoint::BeforeRename) {
        return Err(EditError::InjectedFault {
            at: FaultPoint::BeforeRename,
            temporary: Some(temp.path().to_path_buf()),
        });
    }

    if request.dry_run {
        // `temp` is dropped here and takes the file with it.
        return Ok(EditOutcome {
            report: edit_report,
            path: input,
            format,
            dry_run: true,
        });
    }

    // The replacement inherits the temporary file's permissions, so the
    // original's are copied onto it first; otherwise a group-readable document
    // would come back private, which is a change to the document nobody asked
    // for.
    if let Ok(metadata) = std::fs::metadata(&input) {
        let _ = std::fs::set_permissions(temp.path(), metadata.permissions());
    }
    temp.persist(&input)
        .map_err(|error| EditError::Transaction {
            path: input.display().to_string(),
            source: error.error,
        })?;
    sync_dir(&directory);

    Ok(EditOutcome {
        report: edit_report,
        path: input,
        format,
        dry_run: false,
    })
}

/// Applies `request` to a document already in memory.
///
/// The half with no I/O in it, so that a caller with a document and no file —
/// and every test of the operation set — reaches the operations without going
/// through a filesystem.
pub fn apply_edits_in_memory(
    document: &mut Document,
    store: &mut dyn AssetStore,
    request: &EditRequest,
    base: Option<PathBuf>,
    format: Option<Format>,
) -> Result<(EditReport, Vec<docsai_model::Warning>), EditError> {
    let mut session = ops::Session::new(document, store, base, format);
    session.validate(&request.operations)?;
    for (index, operation) in request.operations.iter().enumerate() {
        session.apply(index, operation)?;
    }
    let warnings = session.report.warnings.clone();
    let created = session.created().to_vec();
    let deleted = session.deleted().to_vec();
    let changed = session.changed().to_vec();
    Ok((
        report::build(document, &created, &deleted, &changed, &warnings),
        warnings,
    ))
}

/// §3's id policy: an edit is only meaningful on a document that carries ids.
fn refuse_unaddressable(options: &ConvertOptions) -> Result<(), EditError> {
    let refuse = |reason: String| {
        Err(EditError::Refused(Refused {
            index: 0,
            op: "<request>".into(),
            refusal: Refusal::Unaddressable { reason },
        }))
    };
    if !options.fidelity.addresses() {
        return refuse(format!(
            "`{}` carries no node ids, and the response names what it changed by id; \
             version 1 edits at `full` and `agent`",
            options.fidelity.as_str()
        ));
    }
    if !options.id_policy().emits() {
        return refuse(format!(
            "`--ids {}` would write a document with no ids to address next time",
            options.id_policy().as_str()
        ));
    }
    Ok(())
}

fn detect(input: &Path) -> Result<Format, ConvertError> {
    let hint = input.file_name().and_then(|n| n.to_str());
    let mut probe = std::fs::File::open(input).map_err(|source| ConvertError::Io {
        path: input.display().to_string(),
        source,
    })?;
    let (format, score) = docsai_office::detect(&mut probe, hint);
    if score == docsai_office::DetectScore::No {
        return Err(ConvertError::UnknownFormat(input.display().to_string()));
    }
    if !crate::can_write(format) {
        return Err(ConvertError::Unsupported {
            from: format,
            to: format,
        });
    }
    Ok(format)
}

/// Flushes the temporary file to stable storage before it is given the
/// destination's name.
fn sync(path: &Path) -> Result<(), EditError> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|source| EditError::Transaction {
            path: path.display().to_string(),
            source,
        })?;
    let mut file = file;
    file.flush().map_err(|source| EditError::Transaction {
        path: path.display().to_string(),
        source,
    })?;
    file.sync_all().map_err(|source| EditError::Transaction {
        path: path.display().to_string(),
        source,
    })?;
    Ok(())
}

/// Makes the *name* durable, not just the bytes.
///
/// Best effort by design: a directory `fsync` is meaningless on some platforms
/// and refused on others, and a completed, correctly named file is not worth
/// failing over a hint the filesystem declined to take.
fn sync_dir(directory: &Path) {
    if let Ok(dir) = std::fs::File::open(directory) {
        let _ = dir.sync_all();
    }
}

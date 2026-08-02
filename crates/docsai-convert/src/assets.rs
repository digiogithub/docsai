//! [`AssetStore`] implementations (architecture §3.2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use docsai_model::assets::{describe, AssetError, AssetId, AssetInfo, AssetStore};

/// An [`AssetStore`] that materialises every asset into a directory.
///
/// Files are written as they are stored, under the deterministic
/// `img-<hash8>.<ext>` name, so two conversions of the same document produce
/// the same directory and `git diff` on `assets/` stays empty.
#[derive(Debug)]
pub struct DirAssetStore {
    dir: PathBuf,
    entries: BTreeMap<AssetId, (AssetInfo, Vec<u8>)>,
    written: Vec<PathBuf>,
}

impl DirAssetStore {
    /// Creates the directory lazily: it is only made when the first asset
    /// arrives, so documents without media leave no empty folder behind.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        DirAssetStore {
            dir: dir.into(),
            entries: BTreeMap::new(),
            written: Vec::new(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Paths written so far, in storage order.
    pub fn written(&self) -> &[PathBuf] {
        &self.written
    }
}

impl AssetStore for DirAssetStore {
    fn put(&mut self, bytes: &[u8]) -> Result<AssetId, AssetError> {
        if bytes.is_empty() {
            return Err(AssetError::Empty);
        }
        let id = AssetId::new(docsai_model::assets::content_hash(bytes));
        if self.entries.contains_key(&id) {
            return Ok(id); // deduplicated: nothing more to write
        }
        let info = describe(&id, bytes);
        std::fs::create_dir_all(&self.dir).map_err(|e| AssetError::Io(e.to_string()))?;
        let path = self.dir.join(&info.file_name);
        std::fs::write(&path, bytes).map_err(|e| AssetError::Io(e.to_string()))?;
        self.written.push(path);
        self.entries.insert(id.clone(), (info, bytes.to_vec()));
        Ok(id)
    }

    fn get(&self, id: &AssetId) -> Option<&[u8]> {
        self.entries.get(id).map(|(_, bytes)| bytes.as_slice())
    }

    fn info(&self, id: &AssetId) -> Option<&AssetInfo> {
        self.entries.get(id).map(|(info, _)| info)
    }

    fn ids(&self) -> Vec<AssetId> {
        self.entries.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("docsai-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn writes_files_with_deterministic_names_and_deduplicates() {
        let dir = temp_dir("assets");
        let mut store = DirAssetStore::new(&dir);
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x78\x00\x00\x00\x5a\x08\x02\x00\x00\x00";

        let first = store.put(png).unwrap();
        let second = store.put(png).unwrap();
        assert_eq!(first, second);
        assert_eq!(store.written().len(), 1, "written once");

        let info = store.info(&first).unwrap();
        assert!(info.file_name.starts_with("img-"));
        assert!(info.file_name.ends_with(".png"));
        assert_eq!(std::fs::read(dir.join(&info.file_name)).unwrap(), png);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_media_leaves_no_directory_behind() {
        let dir = temp_dir("empty");
        let store = DirAssetStore::new(&dir);
        assert!(store.is_empty());
        assert!(!dir.exists(), "the directory is only created on demand");
    }
}

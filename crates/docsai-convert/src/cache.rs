//! The read cache (Phase 16-H).
//!
//! An agent that works on one document calls several tools on it in a row —
//! `outline_document`, then `search_document`, then `read_selection` — and
//! every one of them re-reads the same bytes today. This cache holds what a
//! read produced so the second and third calls do not parse again.
//!
//! It is a **pure optimisation**, and the whole module is written around one
//! rule: a hit must be indistinguishable from a miss. That is what decides the
//! design:
//!
//! - The key covers everything a read can depend on ([`CacheKey`]), and the
//!   options fingerprint is built by destructuring [`ConvertOptions`]
//!   exhaustively, so a future option that changes a read cannot be forgotten
//!   — the code stops compiling instead.
//! - The value carries the [`ConversionReport`] with the document. A cached
//!   read that dropped its warnings would degrade silently, which is the one
//!   thing AGENTS.md §7 rule 3 forbids outright.
//! - The value also carries every byte string the read put into the asset
//!   store, in order. Asset ids are content hashes, so replaying those puts
//!   into the caller's store rebuilds exactly the state the read left behind —
//!   including the files a [`DirAssetStore`](crate::DirAssetStore) writes.
//! - When any of that cannot be guaranteed the cache **refuses**: it does not
//!   serve, and it does not store. A slow answer is a bug report; a wrong one
//!   is a corrupted document.
//!
//! The cache is a value the caller owns. The MCP server keeps one for the life
//! of the process, which is where the repeated calls come from; the CLI is
//! one-shot and keeps none, so its behaviour and its memory are unchanged.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use docsai_model::assets::{AssetError, AssetId, AssetInfo, AssetStore};
use docsai_model::{ConversionReport, Document, Format};

/// Default payload the cache will hold, in bytes.
///
/// 64 MiB: the MCP server's own default input ceiling is 50 MiB
/// (`DOCSAI_MCP_MAX_INPUT_BYTES`), so one maximal document still fits and the
/// cache is never dead weight for the largest thing it is allowed to see,
/// while the total stays small next to the footprint of a process that parses
/// Office packages for a living.
pub const DEFAULT_CAPACITY_BYTES: usize = 64 * 1024 * 1024;

/// Default number of entries the cache will hold.
///
/// 16: an agent session works on a handful of documents at a time, the lookup
/// is a linear scan over this list, and each entry costs a `Document` tree
/// whatever its input weighed.
pub const DEFAULT_MAX_ENTRIES: usize = 16;

/// What a cached read is filed under.
///
/// Every field is here because it can change the answer:
///
/// - `source` — the origin, not just the bytes. A DocMark file resolves its
///   sidecars against its own directory (`parse_with_base`), so the same bytes
///   in two directories are two different documents. For a path this is the
///   canonical path; for in-memory bytes it is the filename hint under a
///   `bytes:` prefix, since that hint is all a byte read has.
/// - `content_hash` / `content_len` — the bytes themselves, via the FNV-1a
///   already in `docsai-model`. The length is kept separately as well: it
///   costs eight bytes and it makes a hash collision need a length collision
///   too.
/// - `options` — the fingerprint of the whole [`ConvertOptions`]. Today only
///   `use_loffice` and the format hint reach a reader, but keying on all of it
///   is deliberate: a conservative key can only cost a hit, and a key that is
///   too narrow serves a wrong document.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CacheKey {
    pub source: String,
    pub content_hash: String,
    pub content_len: usize,
    pub options: String,
}

/// What one read produced, held whole.
#[derive(Debug)]
pub(crate) struct CachedRead {
    pub document: Document,
    pub format: Format,
    /// The warnings the read emitted. Cached with the document because a
    /// degradation that is reported once and then silently dropped on every
    /// later call is worse than no cache at all.
    pub report: ConversionReport,
    /// Every distinct byte string the read stored, in first-put order.
    pub assets: Vec<Vec<u8>>,
    /// What this entry is charged against the capacity.
    pub weight: usize,
}

/// A snapshot of the cache's counters.
///
/// `misses` is the number of reads the cache watched actually happen: it is
/// the parse counter the tests assert on, and it is exact rather than timed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub refusals: u64,
    pub evictions: u64,
    pub entries: usize,
    pub bytes: usize,
}

#[derive(Debug, Default)]
struct Inner {
    /// Most recently used first. A linear scan is the right structure at
    /// [`DEFAULT_MAX_ENTRIES`]; a hash map plus an intrusive list would be
    /// more code and no faster at sixteen entries.
    entries: Vec<(CacheKey, Arc<CachedRead>)>,
    bytes: usize,
    hits: u64,
    misses: u64,
    refusals: u64,
    evictions: u64,
}

/// A bounded LRU of parsed documents.
///
/// Cloneable through an [`Arc`]; every clone shares one cache.
pub struct ConvertCache {
    capacity_bytes: usize,
    max_entries: usize,
    inner: Mutex<Inner>,
}

impl Default for ConvertCache {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ConvertCache {
    /// Deliberately shallow: [`ConvertOptions`] derives `Debug` and is printed
    /// in errors and traces, and a cache that dumped whole document trees into
    /// a log line would be unusable.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stats = self.stats();
        f.debug_struct("ConvertCache")
            .field("capacity_bytes", &self.capacity_bytes)
            .field("max_entries", &self.max_entries)
            .field("entries", &stats.entries)
            .field("bytes", &stats.bytes)
            .finish()
    }
}

impl ConvertCache {
    /// A cache with the default bounds.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY_BYTES, DEFAULT_MAX_ENTRIES)
    }

    /// A cache bounded by both a payload size and an entry count.
    pub fn with_capacity(capacity_bytes: usize, max_entries: usize) -> Self {
        ConvertCache {
            capacity_bytes,
            max_entries,
            inner: Mutex::new(Inner::default()),
        }
    }

    /// A shared cache ready to be handed to
    /// [`ConvertOptions::cache`](crate::ConvertOptions::cache).
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    pub fn capacity_bytes(&self) -> usize {
        self.capacity_bytes
    }

    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// Takes the lock, recovering from poisoning.
    ///
    /// A poisoned lock is treated as usable rather than as a permanent
    /// failure. Nothing in the critical section can panic — it is pushes,
    /// pops and integer arithmetic over plain data — so there is no torn
    /// invariant to inherit, and turning one unrelated panic elsewhere in the
    /// process into a cache that never works again would trade a bug for a
    /// permanent slowdown.
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Current counters.
    pub fn stats(&self) -> CacheStats {
        let inner = self.lock();
        CacheStats {
            hits: inner.hits,
            misses: inner.misses,
            refusals: inner.refusals,
            evictions: inner.evictions,
            entries: inner.entries.len(),
            bytes: inner.bytes,
        }
    }

    /// Empties the cache. Counters are kept: they describe the process, not
    /// the contents.
    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.entries.clear();
        inner.bytes = 0;
    }

    /// Looks `key` up, promoting the entry to most-recently-used on a hit.
    pub(crate) fn lookup(&self, key: &CacheKey) -> Option<Arc<CachedRead>> {
        let mut inner = self.lock();
        let position = inner.entries.iter().position(|(k, _)| k == key)?;
        let entry = inner.entries.remove(position);
        let value = Arc::clone(&entry.1);
        inner.entries.insert(0, entry);
        inner.hits += 1;
        Some(value)
    }

    /// Records that a real read happened.
    pub(crate) fn record_miss(&self) {
        self.lock().misses += 1;
    }

    /// Records that the cache declined to take part in a read.
    pub(crate) fn record_refusal(&self) {
        self.lock().refusals += 1;
    }

    /// Stores a read, evicting least-recently-used entries until it fits.
    ///
    /// An entry heavier than the whole capacity is not stored: it would evict
    /// everything to hold one document that the next insert throws away.
    pub(crate) fn insert(&self, key: CacheKey, value: Arc<CachedRead>) {
        if value.weight > self.capacity_bytes || self.max_entries == 0 {
            return;
        }
        let mut inner = self.lock();
        if let Some(position) = inner.entries.iter().position(|(k, _)| k == &key) {
            let (_, old) = inner.entries.remove(position);
            inner.bytes -= old.weight;
        }
        inner.bytes += value.weight;
        inner.entries.insert(0, (key, value));
        while inner.bytes > self.capacity_bytes || inner.entries.len() > self.max_entries {
            let Some((_, evicted)) = inner.entries.pop() else {
                break;
            };
            inner.bytes -= evicted.weight;
            inner.evictions += 1;
        }
    }
}

/// An [`AssetStore`] that writes through to another one and remembers what it
/// was asked to store.
///
/// This is how a cached read reproduces the store state a real read leaves
/// behind. Ids are content hashes, so replaying the recorded byte strings in
/// order into a fresh store of any implementation yields the same ids, the
/// same manifest and — for a directory store — the same files in the same
/// order. Only the first `put` of a given id is recorded, because a repeat put
/// is already a no-op in every store.
pub(crate) struct RecordingStore<'a> {
    inner: &'a mut dyn AssetStore,
    seen: Vec<AssetId>,
    puts: Vec<Vec<u8>>,
    bytes: usize,
}

impl<'a> RecordingStore<'a> {
    pub(crate) fn new(inner: &'a mut dyn AssetStore) -> Self {
        RecordingStore {
            inner,
            seen: Vec::new(),
            puts: Vec::new(),
            bytes: 0,
        }
    }

    /// The recorded puts and the bytes they account for.
    pub(crate) fn into_recording(self) -> (Vec<Vec<u8>>, usize) {
        (self.puts, self.bytes)
    }
}

impl AssetStore for RecordingStore<'_> {
    fn put(&mut self, bytes: &[u8]) -> Result<AssetId, AssetError> {
        let id = self.inner.put(bytes)?;
        if !self.seen.contains(&id) {
            self.seen.push(id.clone());
            self.puts.push(bytes.to_vec());
            self.bytes += bytes.len();
        }
        Ok(id)
    }

    fn get(&self, id: &AssetId) -> Option<&[u8]> {
        self.inner.get(id)
    }

    fn info(&self, id: &AssetId) -> Option<&AssetInfo> {
        self.inner.info(id)
    }

    fn ids(&self) -> Vec<AssetId> {
        self.inner.ids()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::MemoryAssetStore;

    fn key(name: &str) -> CacheKey {
        CacheKey {
            source: name.into(),
            content_hash: "0000000000000000".into(),
            content_len: 1,
            options: "()".into(),
        }
    }

    fn entry(weight: usize) -> Arc<CachedRead> {
        Arc::new(CachedRead {
            document: Document::Text(Default::default()),
            format: Format::Docx,
            report: ConversionReport::new(),
            assets: Vec::new(),
            weight,
        })
    }

    #[test]
    fn a_miss_then_a_hit() {
        let cache = ConvertCache::new();
        assert!(cache.lookup(&key("a")).is_none());
        cache.insert(key("a"), entry(10));
        assert!(cache.lookup(&key("a")).is_some());
        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.bytes, 10);
    }

    #[test]
    fn eviction_respects_the_byte_bound() {
        let cache = ConvertCache::with_capacity(100, 16);
        cache.insert(key("a"), entry(60));
        cache.insert(key("b"), entry(60));
        assert!(cache.lookup(&key("a")).is_none(), "a is the oldest");
        assert!(cache.lookup(&key("b")).is_some());
        assert_eq!(cache.stats().bytes, 60);
        assert_eq!(cache.stats().evictions, 1);
    }

    #[test]
    fn eviction_respects_the_entry_bound() {
        let cache = ConvertCache::with_capacity(usize::MAX, 2);
        cache.insert(key("a"), entry(1));
        cache.insert(key("b"), entry(1));
        cache.insert(key("c"), entry(1));
        assert_eq!(cache.stats().entries, 2);
        assert!(cache.lookup(&key("a")).is_none());
    }

    #[test]
    fn a_hit_is_promoted_over_an_older_neighbour() {
        let cache = ConvertCache::with_capacity(usize::MAX, 2);
        cache.insert(key("a"), entry(1));
        cache.insert(key("b"), entry(1));
        assert!(cache.lookup(&key("a")).is_some());
        cache.insert(key("c"), entry(1));
        assert!(
            cache.lookup(&key("a")).is_some(),
            "a was used more recently"
        );
        assert!(cache.lookup(&key("b")).is_none());
    }

    #[test]
    fn an_entry_larger_than_the_capacity_is_not_stored() {
        let cache = ConvertCache::with_capacity(10, 16);
        cache.insert(key("a"), entry(11));
        assert_eq!(cache.stats().entries, 0);
    }

    #[test]
    fn re_inserting_a_key_does_not_double_count_its_weight() {
        let cache = ConvertCache::with_capacity(1000, 16);
        cache.insert(key("a"), entry(10));
        cache.insert(key("a"), entry(10));
        assert_eq!(cache.stats().entries, 1);
        assert_eq!(cache.stats().bytes, 10);
    }

    #[test]
    fn clearing_keeps_the_counters() {
        let cache = ConvertCache::new();
        cache.insert(key("a"), entry(10));
        let _ = cache.lookup(&key("a"));
        cache.clear();
        assert_eq!(cache.stats().entries, 0);
        assert_eq!(cache.stats().bytes, 0);
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn the_recording_store_writes_through_and_deduplicates() {
        let mut inner = MemoryAssetStore::new();
        let (puts, bytes) = {
            let mut recorder = RecordingStore::new(&mut inner);
            let first = recorder.put(b"one").unwrap();
            let again = recorder.put(b"one").unwrap();
            recorder.put(b"two").unwrap();
            assert_eq!(first, again);
            recorder.into_recording()
        };
        assert_eq!(puts, vec![b"one".to_vec(), b"two".to_vec()]);
        assert_eq!(bytes, 6);
        assert_eq!(inner.len(), 2);
    }

    #[test]
    fn replaying_a_recording_reproduces_the_ids() {
        let mut first = MemoryAssetStore::new();
        let puts = {
            let mut recorder = RecordingStore::new(&mut first);
            recorder.put(b"alpha").unwrap();
            recorder.put(b"beta").unwrap();
            recorder.into_recording().0
        };
        let mut second = MemoryAssetStore::new();
        for bytes in &puts {
            second.put(bytes).unwrap();
        }
        assert_eq!(first.ids(), second.ids());
        assert_eq!(first.manifest(), second.manifest());
    }

    #[test]
    fn the_debug_form_does_not_print_the_documents() {
        let cache = ConvertCache::new();
        cache.insert(key("a"), entry(10));
        let rendered = format!("{cache:?}");
        assert!(rendered.contains("entries: 1"), "{rendered}");
        assert!(!rendered.contains("Document"), "{rendered}");
    }
}

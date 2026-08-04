//! The attribute-set dictionary (spec §3.7).
//!
//! A pattern of key/value pairs that repeats often enough is written once in
//! the front matter under a generated class name, and every node that carried
//! it carries the class instead. It is a compression of the *body*, not a
//! change of meaning: the parser expands the class back into the pairs before
//! anything interprets them, so the IR is the same either way.
//!
//! The dictionary is built in a first serialisation pass that counts patterns
//! without changing a byte of output; only when that pass finds something worth
//! naming is the body rendered a second time. A document with nothing repeated
//! pays one pass, which is what almost every document is.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};

/// How many times a pattern must appear before it is worth naming.
///
/// The arithmetic, in tokens: a pattern costing `p` used `k` times costs `k·p`
/// inline, and `p + e + k·c` interned, where `e` is the front-matter entry's
/// own overhead (the name, the colon, the quotes, the newline: ~5) and `c` is
/// the reference `.g1` (~3). At `k = 2` the dictionary loses for every pattern
/// short enough to be common; at `k = 3` it starts paying from `p ≈ 7`.
pub const MIN_USES: usize = 3;

/// How long a pattern must be, rendered, before it is worth naming. Below this
/// the reference plus the entry costs more than the repetition it replaces.
pub const MIN_LEN: usize = 12;

/// The prefix of a generated name. `g` for group; the number is the order in
/// which the patterns were first seen, so the names are a function of the
/// document and not of a hash.
const PREFIX: &str = "g";

/// Class names the body already gives a meaning to. A generated name that
/// collided with one of these would change what a node *is*, not just how it
/// is written, so the generator skips them (as it skips every style id).
pub const RESERVED_CLASSES: &[&str] = &[
    "break",
    "caps",
    "cell",
    "empty",
    "field",
    "footer",
    "header",
    "raw",
    "row",
    "section",
    "sheet",
    "small-caps",
    "sub",
    "sup",
    "table",
    "textbox",
    "underline",
];

#[derive(Debug, Default)]
pub struct AttrDict {
    mode: Mode,
}

#[derive(Debug, Default)]
enum Mode {
    /// Interning is off: rendering an attribute block is the identity.
    #[default]
    Off,
    /// First pass: every rendered pattern is counted, output unchanged.
    Collect(RefCell<Sightings>),
    /// Second pass: a pattern that earned a name renders as that class.
    Apply(Applied),
}

#[derive(Debug, Default)]
struct Sightings {
    counts: HashMap<String, usize>,
    /// First-occurrence order, which is what the names follow.
    order: Vec<String>,
}

#[derive(Debug, Default)]
struct Applied {
    /// pattern → generated class name.
    names: HashMap<String, String>,
    /// (name, pattern) in generation order, which is how the front matter
    /// writes them.
    entries: Vec<(String, String)>,
}

impl AttrDict {
    /// A dictionary that does nothing, for the levels and documents that do not
    /// use one.
    pub fn off() -> Self {
        AttrDict { mode: Mode::Off }
    }

    /// A dictionary in its counting pass.
    pub fn collecting() -> Self {
        AttrDict {
            mode: Mode::Collect(RefCell::new(Sightings::default())),
        }
    }

    /// True while the dictionary is counting rather than substituting. The
    /// writer renders the same bytes either way; this is what tells the caller
    /// a second pass is still to come.
    pub fn is_collecting(&self) -> bool {
        matches!(self.mode, Mode::Collect(_))
    }

    /// Records one appearance of `pattern`. Ignored outside the counting pass.
    pub fn observe(&self, pattern: &str) {
        if let Mode::Collect(sightings) = &self.mode {
            let mut sightings = sightings.borrow_mut();
            let count = sightings.counts.entry(pattern.to_string()).or_insert(0);
            *count += 1;
            if *count == 1 {
                sightings.order.push(pattern.to_string());
            }
        }
    }

    /// The class that stands for `pattern`, if it earned one.
    pub fn name_for(&self, pattern: &str) -> Option<&str> {
        match &self.mode {
            Mode::Apply(applied) => applied.names.get(pattern).map(String::as_str),
            _ => None,
        }
    }

    /// What the front matter has to declare, in generation order.
    pub fn entries(&self) -> &[(String, String)] {
        match &self.mode {
            Mode::Apply(applied) => &applied.entries,
            _ => &[],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries().is_empty()
    }

    /// Turns the counting pass into the substituting one.
    ///
    /// `taken` is every class name the document already uses for something
    /// else — style ids, list names, the markers of `RESERVED_CLASSES` — and a
    /// generated name never lands on one of them.
    pub fn build(self, taken: &BTreeSet<String>) -> AttrDict {
        let sightings = match self.mode {
            Mode::Collect(sightings) => sightings.into_inner(),
            other => return AttrDict { mode: other },
        };
        let mut applied = Applied::default();
        let mut next = 1u32;
        for pattern in sightings.order {
            let uses = sightings.counts.get(&pattern).copied().unwrap_or(0);
            if uses < MIN_USES || pattern.len() < MIN_LEN {
                continue;
            }
            let name = loop {
                let name = format!("{PREFIX}{next}");
                next += 1;
                if !taken.contains(&name) {
                    break name;
                }
            };
            applied.names.insert(pattern.clone(), name.clone());
            applied.entries.push((name, pattern));
        }
        AttrDict {
            mode: Mode::Apply(applied),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taken() -> BTreeSet<String> {
        BTreeSet::new()
    }

    #[test]
    fn a_pattern_seen_twice_is_not_worth_a_name() {
        let dict = AttrDict::collecting();
        dict.observe("color=C00000 size=14pt");
        dict.observe("color=C00000 size=14pt");
        assert!(dict.build(&taken()).is_empty());
    }

    #[test]
    fn a_pattern_seen_three_times_earns_one() {
        let dict = AttrDict::collecting();
        for _ in 0..3 {
            dict.observe("color=C00000 size=14pt");
        }
        let dict = dict.build(&taken());
        assert_eq!(dict.name_for("color=C00000 size=14pt"), Some("g1"));
        assert_eq!(
            dict.entries(),
            [("g1".to_string(), "color=C00000 size=14pt".to_string())]
        );
    }

    #[test]
    fn a_short_pattern_never_earns_one() {
        let dict = AttrDict::collecting();
        for _ in 0..50 {
            dict.observe("gap=1");
        }
        assert!(dict.build(&taken()).is_empty());
    }

    #[test]
    fn names_follow_first_occurrence_and_avoid_what_is_taken() {
        let dict = AttrDict::collecting();
        for _ in 0..3 {
            dict.observe("indent-left=1.25cm");
        }
        for _ in 0..3 {
            dict.observe("color=C00000 size=14pt");
        }
        let mut taken = BTreeSet::new();
        taken.insert("g1".to_string());
        let dict = dict.build(&taken);
        assert_eq!(dict.name_for("indent-left=1.25cm"), Some("g2"));
        assert_eq!(dict.name_for("color=C00000 size=14pt"), Some("g3"));
    }
}

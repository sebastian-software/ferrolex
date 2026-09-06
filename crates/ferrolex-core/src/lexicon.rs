use std::borrow::Cow;
use std::fmt;
use std::sync::{Arc, OnceLock};

use unicode_normalization::{is_nfc, is_nfkc, UnicodeNormalization};

use crate::{CandidateIndex, CandidateSource};

/// An immutable collection that can recognize words.
///
/// Dictionaries are safe to share across threads. Lookup only observes the
/// dictionary and never mutates it.
pub trait Dictionary: Send + Sync {
    /// Returns whether this dictionary recognizes `word`.
    fn contains(&self, word: &str) -> bool;

    /// Returns this dictionary's suggestion source, when it provides one.
    ///
    /// Checking-only dictionaries keep the default `None`. This optional
    /// boundary lets [`crate::Checker`] compose dictionaries for lookup while
    /// delegating suggestions only to constituents that support them.
    fn as_candidate_source(&self) -> Option<&dyn CandidateSource> {
        None
    }
}

/// The lookup transformation applied to both dictionary entries and queries.
///
/// Normalization never performs case folding. That behavior must remain a
/// separate policy because language-specific casing can carry meaning.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum Normalization {
    /// Compare each UTF-8 string exactly as supplied.
    #[default]
    Exact,
    /// Canonically normalize Unicode text according to NFC.
    Nfc,
    /// Compatibility-normalize Unicode text according to NFKC.
    Nfkc,
}

impl Normalization {
    /// Returns the normalized lookup representation of `word`.
    ///
    /// Exact input and already-normalized NFC/NFKC input are borrowed. Other
    /// normalization requests allocate the transformed UTF-8 representation.
    #[must_use]
    pub fn normalize(self, word: &str) -> Cow<'_, str> {
        match self {
            Self::Exact => Cow::Borrowed(word),
            Self::Nfc if is_nfc(word) => Cow::Borrowed(word),
            Self::Nfc => Cow::Owned(word.nfc().collect()),
            Self::Nfkc if is_nfkc(word) => Cow::Borrowed(word),
            Self::Nfkc => Cow::Owned(word.nfkc().collect()),
        }
    }
}

/// Returns whether `dictionary` recognizes `word` exactly or after an NFC
/// fallback.
///
/// The exact lookup remains the fast path. NFC is attempted only after an
/// exact miss so callers that need canonical-equivalence behavior can share
/// one explicit policy without changing the [`Dictionary`] trait contract.
#[must_use]
pub fn contains_normalized(dictionary: &dyn Dictionary, word: &str) -> bool {
    if dictionary.contains(word) {
        return true;
    }
    match Normalization::Nfc.normalize(word) {
        Cow::Borrowed(_) => false,
        Cow::Owned(normalized) => dictionary.contains(&normalized),
    }
}

/// A structured error encountered while building or updating a plain-word-list
/// dictionary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WordListError {
    /// An entry was empty after line-oriented input whitespace was removed.
    EmptyEntry {
        /// One-based position of the invalid entry.
        position: usize,
    },
    /// An entry contains syntax that cannot survive a plain-word-list round-trip.
    InvalidEntry {
        /// One-based position of the invalid entry.
        position: usize,
    },
    /// The concatenated dictionary data exceeds the compact offset limit.
    DictionaryTooLarge,
}

impl fmt::Display for WordListError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyEntry { position } => {
                write!(formatter, "dictionary entry {position} is empty")
            }
            Self::InvalidEntry { position } => write!(
                formatter,
                "dictionary entry {position} cannot be represented by plain-word-list syntax"
            ),
            Self::DictionaryTooLarge => write!(
                formatter,
                "dictionary data exceeds the 4 GiB plain-word-list limit"
            ),
        }
    }
}

impl std::error::Error for WordListError {}

/// An immutable, sorted plain-word-list dictionary.
///
/// Entries are deduplicated at construction time. The sorted contiguous
/// representation keeps exact lookup deterministic and allocation-free. The
/// word bytes live in one arena; `offsets` stores one 32-bit start offset per
/// word, with each end offset derived from the next entry.
#[derive(Clone, Debug)]
pub struct WordList {
    arena: String,
    offsets: Vec<u32>,
    normalization: Normalization,
    candidate_index: Arc<OnceLock<CandidateIndex>>,
}

impl PartialEq for WordList {
    fn eq(&self, other: &Self) -> bool {
        self.arena == other.arena
            && self.offsets == other.offsets
            && self.normalization == other.normalization
    }
}

impl Eq for WordList {}

impl WordList {
    /// Builds an exactly matched dictionary from non-empty UTF-8 entries.
    ///
    /// # Errors
    ///
    /// Returns [`WordListError::EmptyEntry`] when an input entry is empty, or
    /// [`WordListError::DictionaryTooLarge`] when the arena cannot be indexed
    /// by compact 32-bit offsets.
    pub fn new<I, S>(words: I) -> Result<Self, WordListError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::with_normalization(Normalization::Exact, words)
    }

    /// Builds a dictionary using an explicit lookup normalization policy.
    ///
    /// # Errors
    ///
    /// Returns [`WordListError::EmptyEntry`] when an input entry is empty, or
    /// [`WordListError::DictionaryTooLarge`] when the arena cannot be indexed
    /// by compact 32-bit offsets.
    pub fn with_normalization<I, S>(
        normalization: Normalization,
        words: I,
    ) -> Result<Self, WordListError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::from_entries(normalization, words)
    }

    /// Builds a dictionary from UTF-8 plain-word-list text.
    ///
    /// Each non-empty line supplies one word. Leading and trailing whitespace
    /// is ignored, and a line is a comment only when its first non-whitespace
    /// character is `#`. Internal whitespace and inline `#` remain part of the
    /// entry. This deliberately small syntax is independent of Hunspell
    /// dictionary files.
    ///
    /// # Panics
    ///
    /// Panics when the resulting arena exceeds the 4 GiB compact-offset limit.
    #[must_use]
    pub fn from_text(normalization: Normalization, text: &str) -> Self {
        let entries = text.lines().enumerate().filter_map(|(line_number, line)| {
            let line = if line_number == 0 {
                line.strip_prefix('\u{feff}').unwrap_or(line)
            } else {
                line
            };
            let word = line.trim();

            (!word.is_empty() && !word.starts_with('#')).then_some(word)
        });
        Self::from_entries(normalization, entries)
            .expect("plain word-list exceeds the 4 GiB offset-table limit")
    }

    /// Returns the number of unique words in this dictionary.
    #[must_use]
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    /// Returns whether this dictionary has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    /// Returns the normalization policy used by this dictionary.
    #[must_use]
    pub fn normalization(&self) -> Normalization {
        self.normalization
    }

    /// Returns an iterator over entries in deterministic lexical order.
    #[must_use]
    pub fn words(&self) -> impl ExactSizeIterator<Item = &str> + DoubleEndedIterator + '_ {
        self.offsets
            .iter()
            .enumerate()
            .map(|(index, _)| self.word_at(index))
    }

    /// Returns the lazily-built spelling-candidate index.
    #[doc(hidden)]
    #[must_use]
    pub fn candidate_index(&self, maximum_word_scalars: usize) -> &CandidateIndex {
        self.candidate_index
            .get_or_init(|| CandidateIndex::new(self.words(), maximum_word_scalars))
    }

    fn from_entries<I, S>(normalization: Normalization, words: I) -> Result<Self, WordListError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut arena = String::new();
        let mut ranges = Vec::new();

        for (index, word) in words.into_iter().enumerate() {
            let word = normalization.normalize(word.as_ref());
            if word.is_empty() {
                return Err(WordListError::EmptyEntry {
                    position: index + 1,
                });
            }

            let start =
                u32::try_from(arena.len()).map_err(|_| WordListError::DictionaryTooLarge)?;
            arena.push_str(word.as_ref());
            let end = u32::try_from(arena.len()).map_err(|_| WordListError::DictionaryTooLarge)?;
            ranges.push((start, end));
        }

        ranges.sort_unstable_by(|left, right| {
            word_slice(&arena, *left).cmp(word_slice(&arena, *right))
        });
        ranges.dedup_by(|left, right| word_slice(&arena, *left) == word_slice(&arena, *right));

        let mut sorted_arena = String::with_capacity(arena.len());
        let mut offsets = Vec::with_capacity(ranges.len());
        for range in ranges {
            let word = word_slice(&arena, range);
            offsets.push(
                u32::try_from(sorted_arena.len())
                    .expect("sorted word-list arena cannot exceed the source arena"),
            );
            sorted_arena.push_str(word);
        }

        Ok(Self {
            arena: sorted_arena,
            offsets,
            normalization,
            candidate_index: Arc::new(OnceLock::new()),
        })
    }

    fn word_at(&self, index: usize) -> &str {
        let start = self.offsets[index];
        let end = self.offsets.get(index + 1).copied().unwrap_or_else(|| {
            u32::try_from(self.arena.len())
                .expect("word-list arena cannot exceed the offset-table limit")
        });
        word_slice(&self.arena, (start, end))
    }
}

fn word_slice(arena: &str, (start, end): (u32, u32)) -> &str {
    let start = usize::try_from(start).expect("word offset fits in the process address space");
    let end = usize::try_from(end).expect("word offset fits in the process address space");
    arena
        .get(start..end)
        .expect("word-list offsets must describe valid UTF-8 boundaries")
}

impl Dictionary for WordList {
    fn contains(&self, word: &str) -> bool {
        let word = self.normalization.normalize(word);
        let mut left = 0;
        let mut right = self.len();

        while left < right {
            let middle = left + (right - left) / 2;
            match self.word_at(middle).cmp(word.as_ref()) {
                std::cmp::Ordering::Less => left = middle + 1,
                std::cmp::Ordering::Greater => right = middle,
                std::cmp::Ordering::Equal => return true,
            }
        }

        false
    }

    fn as_candidate_source(&self) -> Option<&dyn CandidateSource> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{Dictionary, Normalization, WordList, WordListError};

    proptest! {
        #[test]
        fn normalization_is_idempotent(word in any::<String>()) {
            for normalization in [Normalization::Exact, Normalization::Nfc, Normalization::Nfkc] {
                let once = normalization.normalize(&word);
                prop_assert_eq!(normalization.normalize(once.as_ref()).into_owned(), once.into_owned());
            }
        }
    }

    #[test]
    fn recognizes_exact_utf8_entries() {
        let dictionary =
            WordList::new(["Straße", "東京", "🦀"]).expect("all test entries are non-empty");

        assert!(dictionary.contains("Straße"));
        assert!(dictionary.contains("東京"));
        assert!(dictionary.contains("🦀"));
        assert!(!dictionary.contains("Strasse"));
        assert!(!dictionary.contains("straße"));
    }

    #[test]
    fn rejects_empty_entries_with_their_position() {
        let error = WordList::new(["valid", "", "later"])
            .expect_err("an empty dictionary entry must be rejected");

        assert_eq!(error, WordListError::EmptyEntry { position: 2 });
    }

    #[test]
    fn sorts_and_deduplicates_entries() {
        let dictionary =
            WordList::new(["zebra", "apple", "zebra"]).expect("all test entries are non-empty");

        assert_eq!(dictionary.len(), 2);
        assert_eq!(dictionary.words().collect::<Vec<_>>(), ["apple", "zebra"]);
    }

    #[test]
    fn stores_sorted_words_in_one_arena_with_compact_offsets() {
        let dictionary =
            WordList::new(["zebra", "apple", "zebra"]).expect("all test entries are non-empty");

        assert_eq!(dictionary.arena, "applezebra");
        assert_eq!(dictionary.offsets, [0, 5]);
        assert_eq!(
            dictionary.words().rev().collect::<Vec<_>>(),
            ["zebra", "apple"]
        );
    }

    #[test]
    fn records_the_explicit_normalization_policy() {
        let dictionary = WordList::with_normalization(Normalization::Exact, ["word"])
            .expect("all test entries are non-empty");

        assert_eq!(dictionary.normalization(), Normalization::Exact);
    }

    #[test]
    fn nfc_recognizes_canonically_equivalent_unicode() {
        let dictionary = WordList::with_normalization(Normalization::Nfc, ["café"])
            .expect("all test entries are non-empty");

        assert!(dictionary.contains("cafe\u{301}"));
        assert_eq!(dictionary.words().collect::<Vec<_>>(), ["café"]);
    }

    #[test]
    fn contains_normalized_uses_an_nfc_fallback_after_an_exact_miss() {
        let dictionary = WordList::new(["café"]).expect("test entry is non-empty");

        assert!(super::contains_normalized(&dictionary, "cafe\u{301}"));
        assert!(!super::contains_normalized(&dictionary, "coffee"));
    }

    #[test]
    fn nfkc_recognizes_compatibility_equivalent_unicode() {
        let dictionary = WordList::with_normalization(Normalization::Nfkc, ["H"])
            .expect("all test entries are non-empty");

        assert!(dictionary.contains("ℌ"));
        assert!(!dictionary.contains("h"));
    }

    #[test]
    fn parses_a_plain_word_list_with_comments_and_crlf() {
        let dictionary = WordList::from_text(
            Normalization::Exact,
            "\u{feff}# German and Japanese\r\n Straße \r\n\r\n# Comment\n東京\n",
        );

        assert_eq!(dictionary.words().collect::<Vec<_>>(), ["Straße", "東京"]);
    }

    #[test]
    fn preserves_inline_hashes_and_internal_whitespace_in_plain_word_lists() {
        let dictionary = WordList::from_text(
            Normalization::Exact,
            "word # data\n two words \n# comment\n",
        );

        assert!(dictionary.contains("word # data"));
        assert!(dictionary.contains("two words"));
        assert!(!dictionary.contains("word"));
    }
}

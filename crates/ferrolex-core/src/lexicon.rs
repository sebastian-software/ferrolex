use std::fmt;

/// An immutable collection that can recognize words.
///
/// Dictionaries are safe to share across threads. Lookup only observes the
/// dictionary and never mutates it.
pub trait Dictionary: Send + Sync {
    /// Returns whether this dictionary recognizes `word`.
    fn contains(&self, word: &str) -> bool;
}

/// The lookup transformation applied to both dictionary entries and queries.
///
/// Only exact matching is implemented initially. The enum makes the
/// normalization contract explicit and leaves room for separately specified
/// behavior such as Unicode normalization or case folding.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum Normalization {
    /// Compare each UTF-8 string exactly as supplied.
    #[default]
    Exact,
}

/// A structured error encountered while building a [`WordList`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WordListError {
    /// An entry was empty after line-oriented input whitespace was removed.
    EmptyEntry {
        /// One-based position of the invalid entry.
        position: usize,
    },
}

impl fmt::Display for WordListError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyEntry { position } => {
                write!(formatter, "dictionary entry {position} is empty")
            }
        }
    }
}

impl std::error::Error for WordListError {}

/// An immutable, sorted plain-word-list dictionary.
///
/// Entries are deduplicated at construction time. The sorted contiguous
/// representation keeps exact lookup deterministic and allocation-free.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WordList {
    words: Vec<Box<str>>,
    normalization: Normalization,
}

impl WordList {
    /// Builds an exactly matched dictionary from non-empty UTF-8 entries.
    ///
    /// # Errors
    ///
    /// Returns [`WordListError::EmptyEntry`] when an input entry is empty.
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
    /// Returns [`WordListError::EmptyEntry`] when an input entry is empty.
    pub fn with_normalization<I, S>(
        normalization: Normalization,
        words: I,
    ) -> Result<Self, WordListError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut entries = Vec::new();

        for (index, word) in words.into_iter().enumerate() {
            let word = word.as_ref();
            if word.is_empty() {
                return Err(WordListError::EmptyEntry {
                    position: index + 1,
                });
            }
            entries.push(Box::<str>::from(word));
        }

        Ok(Self::from_entries(normalization, entries))
    }

    /// Builds a dictionary from UTF-8 plain-word-list text.
    ///
    /// Each non-empty, non-comment line supplies one word. Leading and
    /// trailing whitespace is ignored, and comments begin with `#` after that
    /// whitespace is removed. This deliberately small syntax is independent
    /// of Hunspell dictionary files.
    #[must_use]
    pub fn from_text(normalization: Normalization, text: &str) -> Self {
        let entries = text
            .lines()
            .enumerate()
            .filter_map(|(line_number, line)| {
                let line = if line_number == 0 {
                    line.strip_prefix('\u{feff}').unwrap_or(line)
                } else {
                    line
                };
                let word = line.trim();

                (!word.is_empty() && !word.starts_with('#')).then_some(word)
            })
            .map(Box::<str>::from)
            .collect();

        Self::from_entries(normalization, entries)
    }

    /// Returns the number of unique words in this dictionary.
    #[must_use]
    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// Returns whether this dictionary has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Returns the normalization policy used by this dictionary.
    #[must_use]
    pub fn normalization(&self) -> Normalization {
        self.normalization
    }

    /// Returns an iterator over entries in deterministic lexical order.
    pub fn words(&self) -> impl ExactSizeIterator<Item = &str> + DoubleEndedIterator + '_ {
        self.words.iter().map(Box::as_ref)
    }

    fn from_entries(normalization: Normalization, mut entries: Vec<Box<str>>) -> Self {
        entries.sort_unstable();
        entries.dedup();

        Self {
            words: entries,
            normalization,
        }
    }
}

impl Dictionary for WordList {
    fn contains(&self, word: &str) -> bool {
        match self.normalization {
            Normalization::Exact => self
                .words
                .binary_search_by(|candidate| candidate.as_ref().cmp(word))
                .is_ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Dictionary, Normalization, WordList, WordListError};

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
    fn records_the_explicit_normalization_policy() {
        let dictionary = WordList::with_normalization(Normalization::Exact, ["word"])
            .expect("all test entries are non-empty");

        assert_eq!(dictionary.normalization(), Normalization::Exact);
    }

    #[test]
    fn parses_a_plain_word_list_with_comments_and_crlf() {
        let dictionary = WordList::from_text(
            Normalization::Exact,
            "\u{feff}# German and Japanese\r\n Straße \r\n\r\n# Comment\n東京\n",
        );

        assert_eq!(dictionary.words().collect::<Vec<_>>(), ["Straße", "東京"]);
    }
}

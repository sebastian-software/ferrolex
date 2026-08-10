use std::collections::BTreeSet;
use std::sync::{LockResult, PoisonError, RwLock};

use crate::{Dictionary, Normalization, WordListError};

/// A small, mutable dictionary layer for user- or project-specific words.
///
/// Base dictionaries remain immutable. This overlay intentionally owns its
/// synchronization so that a shared [`std::sync::Arc`] can take effect in
/// concurrent checkers immediately without a global lock.
#[derive(Debug)]
pub struct UserDictionary {
    normalization: Normalization,
    words: RwLock<BTreeSet<Box<str>>>,
}

impl UserDictionary {
    /// Creates an empty user dictionary with an explicit normalization policy.
    #[must_use]
    pub fn new(normalization: Normalization) -> Self {
        Self {
            normalization,
            words: RwLock::new(BTreeSet::new()),
        }
    }

    /// Loads an overlay from the project's UTF-8 plain-word-list syntax.
    ///
    /// Blank lines and lines beginning with `#` are ignored, matching
    /// [`crate::WordList::from_text`]. The returned overlay is immediately
    /// available for concurrent updates and lookups.
    #[must_use]
    pub fn from_text(normalization: Normalization, text: &str) -> Self {
        let words = crate::WordList::from_text(normalization, text)
            .words()
            .map(Box::<str>::from)
            .collect();
        Self {
            normalization,
            words: RwLock::new(words),
        }
    }

    /// Returns a deterministic snapshot of the current overlay entries.
    #[must_use]
    pub fn snapshot(&self) -> Vec<String> {
        recover_lock(self.words.read())
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    /// Serializes the current overlay as a deterministic UTF-8 word list.
    ///
    /// The output ends with a newline when non-empty, which makes it suitable
    /// for atomic replacement by a caller-owned persistence layer.
    #[must_use]
    pub fn to_text(&self) -> String {
        let words = recover_lock(self.words.read());
        let mut text = String::new();
        for word in words.iter() {
            text.push_str(word);
            text.push('\n');
        }
        text
    }

    /// Adds `word` to this overlay.
    ///
    /// Returns whether the word was newly added.
    ///
    /// # Errors
    ///
    /// Returns [`WordListError::EmptyEntry`] when `word` is empty after
    /// normalization.
    pub fn insert(&self, word: &str) -> Result<bool, WordListError> {
        let word = self.normalization.normalize(word);
        if word.is_empty() {
            return Err(WordListError::EmptyEntry { position: 1 });
        }

        Ok(recover_lock(self.words.write()).insert(Box::<str>::from(word.as_ref())))
    }

    /// Removes `word` from this overlay.
    ///
    /// Returns whether the word existed.
    ///
    /// # Errors
    ///
    /// Returns [`WordListError::EmptyEntry`] when `word` is empty after
    /// normalization.
    pub fn remove(&self, word: &str) -> Result<bool, WordListError> {
        let word = self.normalization.normalize(word);
        if word.is_empty() {
            return Err(WordListError::EmptyEntry { position: 1 });
        }

        Ok(recover_lock(self.words.write()).remove(word.as_ref()))
    }

    /// Returns the number of words in this overlay.
    #[must_use]
    pub fn len(&self) -> usize {
        recover_lock(self.words.read()).len()
    }

    /// Returns whether this overlay has no words.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        recover_lock(self.words.read()).is_empty()
    }
}

impl Dictionary for UserDictionary {
    fn contains(&self, word: &str) -> bool {
        let word = self.normalization.normalize(word);
        recover_lock(self.words.read()).contains(word.as_ref())
    }
}

fn recover_lock<T>(result: LockResult<T>) -> T {
    // A panic may poison a lock, but Rust has already restored the BTreeSet's
    // invariants during unwinding. The Dictionary trait cannot surface an
    // operational error, so a later lookup continues from that valid state.
    result.unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use crate::{Dictionary, Normalization, UserDictionary, WordListError};

    #[test]
    fn changes_are_immediately_visible_to_lookup() {
        let dictionary = UserDictionary::new(Normalization::Nfc);

        assert!(dictionary.insert("café").expect("word is non-empty"));
        assert!(dictionary.contains("cafe\u{301}"));
        assert!(!dictionary.insert("café").expect("word is non-empty"));
        assert!(dictionary.remove("cafe\u{301}").expect("word is non-empty"));
        assert!(!dictionary.contains("café"));
    }

    #[test]
    fn rejects_empty_words() {
        let dictionary = UserDictionary::new(Normalization::Exact);

        assert_eq!(
            dictionary.insert("").expect_err("empty words are invalid"),
            WordListError::EmptyEntry { position: 1 }
        );
    }

    #[test]
    fn supports_concurrent_updates_and_lookups() {
        let dictionary = Arc::new(UserDictionary::new(Normalization::Exact));

        thread::scope(|scope| {
            let writer = Arc::clone(&dictionary);
            scope.spawn(move || {
                writer.insert("shared").expect("word is non-empty");
            });
        });

        assert!(dictionary.contains("shared"));
    }

    #[test]
    fn round_trips_a_deterministic_persistent_word_list() {
        let dictionary = UserDictionary::from_text(
            Normalization::Nfc,
            "\u{feff}# project vocabulary\n cafe\u{301}\n\nAlpha\n",
        );

        assert_eq!(dictionary.snapshot(), ["Alpha", "café"]);
        assert_eq!(dictionary.to_text(), "Alpha\ncafé\n");
        assert!(dictionary.contains("cafe\u{301}"));
    }
}

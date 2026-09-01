//! Plain-text tokenization and spell-checking for ferrolex.
//!
//! This crate is deliberately independent from source-code tokenization. It
//! extracts natural-language words and delegates recognition to a core
//! [`Dictionary`].
//!
//! ```
//! use ferrolex_core::WordList;
//! use ferrolex_text::check_text;
//!
//! let dictionary = WordList::new(["known"])?;
//! assert_eq!(check_text(&dictionary, "known typo").count(), 1);
//! # Ok::<(), ferrolex_core::WordListError>(())
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::borrow::Cow;
use std::ops::Range;

use ferrolex_core::{Dictionary, Normalization};
use unicode_normalization::char::canonical_combining_class;

/// A misspelled natural-language token and its byte range in the input text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Misspelling<'text> {
    word: &'text str,
    range: Range<usize>,
}

impl<'text> Misspelling<'text> {
    /// Returns the original UTF-8 token from the checked text.
    #[must_use]
    pub fn word(&self) -> &'text str {
        self.word
    }

    /// Returns the token's byte range within the checked text.
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }
}

/// An iterator over misspelled tokens in a plain-text input.
pub struct Misspellings<'dictionary, 'text> {
    dictionary: &'dictionary dyn Dictionary,
    tokens: WordTokens<'text>,
}

impl<'text> Iterator for Misspellings<'_, 'text> {
    type Item = Misspelling<'text>;

    fn next(&mut self) -> Option<Self::Item> {
        self.tokens.find_map(|(range, word)| {
            (!contains_normalized(self.dictionary, word)).then_some(Misspelling { word, range })
        })
    }
}

/// Checks natural-language words in `text` against `dictionary`.
///
/// Tokens comprise Unicode alphabetic characters and may contain a straight
/// or curly apostrophe between letters. All returned ranges are valid UTF-8
/// byte boundaries in `text`.
#[must_use]
pub fn check_text<'dictionary, 'text>(
    dictionary: &'dictionary impl Dictionary,
    text: &'text str,
) -> Misspellings<'dictionary, 'text> {
    Misspellings {
        dictionary,
        tokens: WordTokens::new(text),
    }
}

struct WordTokens<'text> {
    text: &'text str,
    next_byte: usize,
}

impl<'text> WordTokens<'text> {
    const fn new(text: &'text str) -> Self {
        Self { text, next_byte: 0 }
    }
}

impl<'text> Iterator for WordTokens<'text> {
    type Item = (Range<usize>, &'text str);

    fn next(&mut self) -> Option<Self::Item> {
        let remaining = &self.text[self.next_byte..];
        let (start_offset, first_character) = remaining
            .char_indices()
            .find(|(_, character)| character.is_alphabetic())?;
        let start = self.next_byte + start_offset;
        let mut end = start + first_character.len_utf8();
        let tail_start = end;
        let mut characters = self.text[tail_start..].char_indices().peekable();

        while let Some((offset, character)) = characters.next() {
            let character_end = tail_start + offset + character.len_utf8();
            if is_word_character(character) {
                end = character_end;
                continue;
            }

            let next_is_letter = characters
                .peek()
                .is_some_and(|(_, next_character)| next_character.is_alphabetic());
            if matches!(character, '\'' | '’') && next_is_letter {
                end = character_end;
                continue;
            }

            self.next_byte = end;
            return Some((start..end, &self.text[start..end]));
        }

        self.next_byte = self.text.len();
        Some((start..end, &self.text[start..end]))
    }
}

fn contains_normalized(dictionary: &dyn Dictionary, token: &str) -> bool {
    if dictionary.contains(token) {
        return true;
    }
    match Normalization::Nfc.normalize(token) {
        Cow::Borrowed(_) => false,
        Cow::Owned(normalized) => dictionary.contains(&normalized),
    }
}

fn is_word_character(character: char) -> bool {
    character.is_alphabetic() || canonical_combining_class(character) != 0
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ferrolex_core::{Dictionary, Normalization, UserDictionary, WordList};

    use super::check_text;

    struct CountingDictionary {
        accepted: &'static str,
        lookups: AtomicUsize,
    }

    impl CountingDictionary {
        const fn new(accepted: &'static str) -> Self {
            Self {
                accepted,
                lookups: AtomicUsize::new(0),
            }
        }
    }

    impl Dictionary for CountingDictionary {
        fn contains(&self, word: &str) -> bool {
            self.lookups.fetch_add(1, Ordering::Relaxed);
            word == self.accepted
        }
    }

    #[test]
    fn reports_unknown_unicode_words_with_utf8_ranges() {
        let dictionary = WordList::new(["Café", "Straße"]).expect("test entries are valid");
        let text = "Café, Strasse!";

        let misspellings = check_text(&dictionary, text).collect::<Vec<_>>();

        assert_eq!(misspellings.len(), 1);
        assert_eq!(misspellings[0].word(), "Strasse");
        assert_eq!(misspellings[0].range(), 7..14);
    }

    #[test]
    fn keeps_apostrophes_inside_words() {
        let dictionary = WordList::new(["don't", "l’esprit"]).expect("test entries are valid");

        assert!(check_text(&dictionary, "don't l’esprit").next().is_none());
    }

    #[test]
    fn recognizes_nfd_words_without_changing_the_reported_source_text() {
        let dictionary = WordList::new(["café"]).expect("test word is valid");
        let text = "cafe\u{301} typo";
        let misspellings = check_text(&dictionary, text).collect::<Vec<_>>();

        assert_eq!(misspellings.len(), 1);
        assert_eq!(misspellings[0].word(), "typo");
        assert_eq!(&text[misspellings[0].range()], "typo");
    }

    #[test]
    fn avoids_duplicate_nfc_lookups_without_hiding_live_updates() {
        let misses = CountingDictionary::new("");
        assert_eq!(check_text(&misses, "typo").count(), 1);
        assert_eq!(misses.lookups.load(Ordering::Relaxed), 1);

        let normalized = CountingDictionary::new("café");
        assert_eq!(check_text(&normalized, "cafe\u{301}").count(), 0);
        assert_eq!(normalized.lookups.load(Ordering::Relaxed), 2);

        let overlay = UserDictionary::new(Normalization::Exact);
        let mut misspellings = check_text(&overlay, "typo typo");
        assert_eq!(
            misspellings.next().map(|finding| finding.word()),
            Some("typo")
        );
        overlay.insert("typo").expect("test word is valid");
        assert!(misspellings.next().is_none());
    }

    #[test]
    fn ignores_numbers_and_punctuation() {
        let dictionary = WordList::new(["version"]).expect("test entries are valid");

        assert!(check_text(&dictionary, "version 1.81!!!").next().is_none());
    }
}

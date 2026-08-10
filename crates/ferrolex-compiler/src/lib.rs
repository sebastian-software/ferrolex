//! A deterministic, bounds-checked compiled dictionary format.
//!
//! The format is designed to be suitable for a memory-mapped backing store:
//! all fields are little-endian integers, sections are offset-addressed and
//! eight-byte aligned, and lookup performs no allocation.  This initial
//! version only represents exact words. Metadata and morphology are separate
//! future format features rather than implicit, unstable payloads.

#![forbid(unsafe_code)]

use std::fmt;

use ferrolex_core::Dictionary;

const MAGIC: [u8; 8] = *b"FLEXDIC\0";
const FORMAT_VERSION: u16 = 1;
const HEADER_SIZE: usize = 64;
const HEADER_SIZE_U16: u16 = 64;
const INDEX_ENTRY_SIZE: usize = 16;
const CHECKSUM_OFFSET: usize = 16;
const CHECKSUM_END: usize = CHECKSUM_OFFSET + 8;

const VERSION_OFFSET: usize = 8;
const HEADER_SIZE_OFFSET: usize = 10;
const FLAGS_OFFSET: usize = 12;
const WORD_COUNT_OFFSET: usize = 24;
const INDEX_OFFSET: usize = 32;
const DATA_OFFSET: usize = 40;
const DATA_LEN_OFFSET: usize = 48;
const FILE_LEN_OFFSET: usize = 56;

/// Largest compiled artifact accepted by the version-1 runtime.
///
/// Command-line callers should check a file's metadata before reading it; the
/// in-memory loader repeats the limit for embedded callers.
pub const MAX_COMPILED_ARTIFACT_BYTES: usize = 128 * 1024 * 1024;

/// Reports a failure while compiling a textual word collection.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CompileError {
    /// A supplied entry was empty.
    EmptyWord {
        /// One-based position in the supplied input.
        position: usize,
    },
    /// The input cannot be represented by version 1 of the format.
    DictionaryTooLarge,
}

impl fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyWord { position } => write!(formatter, "word {position} is empty"),
            Self::DictionaryTooLarge => write!(formatter, "dictionary is too large to compile"),
        }
    }
}

impl std::error::Error for CompileError {}

/// Reports an invalid fixed header or failed fast integrity check.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LoadError {
    /// The supplied artifact exceeds the runtime's fixed allocation limit.
    ArtifactTooLarge {
        /// Actual backing byte length.
        actual: usize,
    },
    /// The input does not contain the fixed header.
    TruncatedHeader {
        /// Actual byte length of the input.
        actual: usize,
    },
    /// The fixed magic marker does not identify a ferrolex dictionary.
    InvalidMagic,
    /// The file uses a format version this library does not support.
    UnsupportedVersion {
        /// Version declared in the input.
        found: u16,
    },
    /// The declared header size is not the fixed size of this version.
    InvalidHeaderSize {
        /// Header size declared in the input.
        found: u16,
    },
    /// Reserved header feature bits are not supported by this reader.
    UnsupportedFeatures {
        /// Feature bit set declared in the input.
        found: u32,
    },
    /// The header's file length does not match the backing bytes.
    FileLengthMismatch {
        /// Length declared in the input header.
        declared: u64,
        /// Actual backing byte length.
        actual: usize,
    },
    /// A section offset or length cannot be safely used for lookup.
    InvalidLayout {
        /// Layout invariant that failed.
        reason: LayoutError,
    },
    /// The fixed checksum does not match the backing bytes.
    ChecksumMismatch {
        /// Checksum declared in the header.
        declared: u64,
        /// Checksum calculated from the backing bytes.
        actual: u64,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ArtifactTooLarge { actual } => write!(
                formatter,
                "compiled dictionary is {actual} bytes and exceeds the {} MiB runtime limit",
                MAX_COMPILED_ARTIFACT_BYTES / (1024 * 1024)
            ),
            Self::TruncatedHeader { actual } => {
                write!(
                    formatter,
                    "compiled dictionary header is truncated ({actual} bytes)"
                )
            }
            Self::InvalidMagic => write!(formatter, "compiled dictionary magic is invalid"),
            Self::UnsupportedVersion { found } => {
                write!(
                    formatter,
                    "compiled dictionary version {found} is unsupported"
                )
            }
            Self::InvalidHeaderSize { found } => {
                write!(
                    formatter,
                    "compiled dictionary header size {found} is invalid"
                )
            }
            Self::UnsupportedFeatures { found } => {
                write!(
                    formatter,
                    "compiled dictionary uses unsupported feature bits {found:#x}"
                )
            }
            Self::FileLengthMismatch { declared, actual } => write!(
                formatter,
                "compiled dictionary declares {declared} bytes but has {actual} bytes"
            ),
            Self::InvalidLayout { reason } => {
                write!(formatter, "compiled dictionary layout is invalid: {reason}")
            }
            Self::ChecksumMismatch { declared, actual } => write!(
                formatter,
                "compiled dictionary checksum {declared:#x} does not match {actual:#x}"
            ),
        }
    }
}

impl std::error::Error for LoadError {}

/// Describes a failed cheap layout invariant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LayoutError {
    /// A section is not eight-byte aligned.
    UnalignedSection,
    /// The index cannot contain the declared number of fixed-size entries.
    IndexOutsideFile,
    /// The data section is not wholly inside the backing bytes.
    DataOutsideFile,
    /// A section offset does not fit in the process address space.
    OffsetDoesNotFit,
}

impl fmt::Display for LayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnalignedSection => "a section is not 8-byte aligned",
            Self::IndexOutsideFile => "the index is outside the file",
            Self::DataOutsideFile => "the data section is outside the file",
            Self::OffsetDoesNotFit => "an offset does not fit this platform",
        })
    }
}

/// Reports corruption found by complete structural validation.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ValidationError {
    /// A fixed-size index entry could not be read from the declared index.
    TruncatedIndex {
        /// Zero-based index entry number.
        entry: usize,
    },
    /// An index entry points outside the declared data section.
    WordOutsideData {
        /// Zero-based index entry number.
        entry: usize,
    },
    /// An indexed word is empty.
    EmptyWord {
        /// Zero-based index entry number.
        entry: usize,
    },
    /// An indexed word does not contain UTF-8.
    InvalidUtf8 {
        /// Zero-based index entry number.
        entry: usize,
    },
    /// Indexed words are not strictly sorted in UTF-8 byte order.
    UnsortedWords {
        /// Zero-based index entry number that is not greater than its predecessor.
        entry: usize,
    },
    /// The index has bytes after its declared entries before the data section.
    UnexpectedIndexPadding,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedIndex { entry } => write!(formatter, "index entry {entry} is truncated"),
            Self::WordOutsideData { entry } => write!(formatter, "word {entry} lies outside data"),
            Self::EmptyWord { entry } => write!(formatter, "word {entry} is empty"),
            Self::InvalidUtf8 { entry } => write!(formatter, "word {entry} is not valid UTF-8"),
            Self::UnsortedWords { entry } => {
                write!(formatter, "word {entry} is not strictly sorted")
            }
            Self::UnexpectedIndexPadding => {
                write!(formatter, "the index contains unexpected bytes before data")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Compiles exact non-empty UTF-8 words into format version 1.
///
/// Entries are sorted by UTF-8 byte order and deduplicated, so input order and
/// duplicate entries never affect the resulting bytes.
///
/// # Errors
///
/// Returns [`CompileError::EmptyWord`] for empty input entries or
/// [`CompileError::DictionaryTooLarge`] if the output cannot be represented.
pub fn compile_words<I, S>(words: I) -> Result<Vec<u8>, CompileError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut sorted_words = Vec::<Box<str>>::new();
    for (index, word) in words.into_iter().enumerate() {
        let word = word.as_ref();
        if word.is_empty() {
            return Err(CompileError::EmptyWord {
                position: index + 1,
            });
        }
        sorted_words.push(Box::from(word));
    }
    sorted_words.sort_unstable();
    sorted_words.dedup();

    let word_count =
        u64::try_from(sorted_words.len()).map_err(|_| CompileError::DictionaryTooLarge)?;
    let index_len = sorted_words
        .len()
        .checked_mul(INDEX_ENTRY_SIZE)
        .ok_or(CompileError::DictionaryTooLarge)?;
    let data_len = sorted_words.iter().try_fold(0_usize, |total, word| {
        total
            .checked_add(word.len())
            .ok_or(CompileError::DictionaryTooLarge)
    })?;
    let index_offset = HEADER_SIZE;
    let data_offset = align_to_eight(
        index_offset
            .checked_add(index_len)
            .ok_or(CompileError::DictionaryTooLarge)?,
    )
    .ok_or(CompileError::DictionaryTooLarge)?;
    let file_len = data_offset
        .checked_add(data_len)
        .ok_or(CompileError::DictionaryTooLarge)?;

    let mut bytes = vec![0_u8; file_len];
    bytes[..MAGIC.len()].copy_from_slice(&MAGIC);
    put_u16(&mut bytes, VERSION_OFFSET, FORMAT_VERSION);
    put_u16(&mut bytes, HEADER_SIZE_OFFSET, HEADER_SIZE_U16);
    put_u32(&mut bytes, FLAGS_OFFSET, 0);
    put_u64(&mut bytes, WORD_COUNT_OFFSET, word_count);
    put_u64(
        &mut bytes,
        INDEX_OFFSET,
        u64::try_from(index_offset).map_err(|_| CompileError::DictionaryTooLarge)?,
    );
    put_u64(
        &mut bytes,
        DATA_OFFSET,
        u64::try_from(data_offset).map_err(|_| CompileError::DictionaryTooLarge)?,
    );
    put_u64(
        &mut bytes,
        DATA_LEN_OFFSET,
        u64::try_from(data_len).map_err(|_| CompileError::DictionaryTooLarge)?,
    );
    put_u64(
        &mut bytes,
        FILE_LEN_OFFSET,
        u64::try_from(file_len).map_err(|_| CompileError::DictionaryTooLarge)?,
    );

    let mut data_cursor = 0_usize;
    for (entry, word) in sorted_words.iter().enumerate() {
        let start = data_cursor;
        data_cursor = data_cursor
            .checked_add(word.len())
            .ok_or(CompileError::DictionaryTooLarge)?;
        let index_entry = index_offset
            .checked_add(
                entry
                    .checked_mul(INDEX_ENTRY_SIZE)
                    .ok_or(CompileError::DictionaryTooLarge)?,
            )
            .ok_or(CompileError::DictionaryTooLarge)?;
        put_u64(
            &mut bytes,
            index_entry,
            u64::try_from(start).map_err(|_| CompileError::DictionaryTooLarge)?,
        );
        put_u64(
            &mut bytes,
            index_entry + 8,
            u64::try_from(data_cursor).map_err(|_| CompileError::DictionaryTooLarge)?,
        );
        bytes[data_offset + start..data_offset + data_cursor].copy_from_slice(word.as_bytes());
    }

    let calculated_checksum = checksum(&bytes);
    put_u64(&mut bytes, CHECKSUM_OFFSET, calculated_checksum);
    Ok(bytes)
}

/// A compiled exact-word dictionary backed by its serialized bytes.
///
/// The backing byte vector is intentionally retained rather than decoded into
/// pointer-heavy runtime objects. A caller that later supplies a memory map can
/// use the same layout and bounds-checking rules.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledDictionary {
    bytes: Vec<u8>,
    word_count: usize,
    index_offset: usize,
    data_offset: usize,
    data_len: usize,
}

impl CompiledDictionary {
    /// Loads a format-version-1 dictionary after a fixed-header and checksum check.
    ///
    /// This does not scan every index entry. Call [`Self::validate`] when a
    /// complete structural validation is required.
    ///
    /// # Errors
    ///
    /// Returns a [`LoadError`] when the header, cheap layout checks, or checksum
    /// cannot be trusted.
    pub fn load(bytes: Vec<u8>) -> Result<Self, LoadError> {
        if bytes.len() > MAX_COMPILED_ARTIFACT_BYTES {
            return Err(LoadError::ArtifactTooLarge {
                actual: bytes.len(),
            });
        }
        validate_header(&bytes)?;

        let word_count = read_u64(&bytes, WORD_COUNT_OFFSET).ok_or(LoadError::TruncatedHeader {
            actual: bytes.len(),
        })?;
        let index_offset = read_u64(&bytes, INDEX_OFFSET).ok_or(LoadError::TruncatedHeader {
            actual: bytes.len(),
        })?;
        let data_offset = read_u64(&bytes, DATA_OFFSET).ok_or(LoadError::TruncatedHeader {
            actual: bytes.len(),
        })?;
        let data_len = read_u64(&bytes, DATA_LEN_OFFSET).ok_or(LoadError::TruncatedHeader {
            actual: bytes.len(),
        })?;
        let word_count = usize::try_from(word_count).map_err(|_| LoadError::InvalidLayout {
            reason: LayoutError::OffsetDoesNotFit,
        })?;
        let index_offset = usize::try_from(index_offset).map_err(|_| LoadError::InvalidLayout {
            reason: LayoutError::OffsetDoesNotFit,
        })?;
        let data_offset = usize::try_from(data_offset).map_err(|_| LoadError::InvalidLayout {
            reason: LayoutError::OffsetDoesNotFit,
        })?;
        let data_len = usize::try_from(data_len).map_err(|_| LoadError::InvalidLayout {
            reason: LayoutError::OffsetDoesNotFit,
        })?;
        if index_offset % 8 != 0 || data_offset % 8 != 0 {
            return Err(LoadError::InvalidLayout {
                reason: LayoutError::UnalignedSection,
            });
        }
        let index_len =
            word_count
                .checked_mul(INDEX_ENTRY_SIZE)
                .ok_or(LoadError::InvalidLayout {
                    reason: LayoutError::IndexOutsideFile,
                })?;
        if index_offset < HEADER_SIZE
            || index_offset
                .checked_add(index_len)
                .filter(|end| *end <= bytes.len())
                .is_none()
        {
            return Err(LoadError::InvalidLayout {
                reason: LayoutError::IndexOutsideFile,
            });
        }
        if data_offset
            .checked_add(data_len)
            .filter(|end| *end <= bytes.len())
            .is_none()
        {
            return Err(LoadError::InvalidLayout {
                reason: LayoutError::DataOutsideFile,
            });
        }

        Ok(Self {
            bytes,
            word_count,
            index_offset,
            data_offset,
            data_len,
        })
    }

    /// Validates every word offset, UTF-8 payload, and sort-order invariant.
    ///
    /// # Errors
    ///
    /// Returns a [`ValidationError`] with the malformed entry where applicable.
    pub fn validate(&self) -> Result<(), ValidationError> {
        let expected_index_end = self
            .index_offset
            .checked_add(self.word_count.saturating_mul(INDEX_ENTRY_SIZE));
        if expected_index_end != Some(self.data_offset) {
            return Err(ValidationError::UnexpectedIndexPadding);
        }

        let mut previous = None::<&[u8]>;
        for entry in 0..self.word_count {
            let word = self.validated_word_bytes(entry)?;
            if word.is_empty() {
                return Err(ValidationError::EmptyWord { entry });
            }
            if std::str::from_utf8(word).is_err() {
                return Err(ValidationError::InvalidUtf8 { entry });
            }
            if previous.is_some_and(|previous| previous >= word) {
                return Err(ValidationError::UnsortedWords { entry });
            }
            previous = Some(word);
        }
        Ok(())
    }

    /// Returns the number of compiled unique words.
    #[must_use]
    pub fn len(&self) -> usize {
        self.word_count
    }

    /// Returns whether no words are present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.word_count == 0
    }

    /// Returns the complete format bytes without copying them.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    fn word_bytes(&self, entry: usize) -> Option<&[u8]> {
        let (start, end) = self.word_offsets(entry)?;
        if start > end || end > self.data_len {
            return None;
        }
        let data_start = self.data_offset.checked_add(start)?;
        let data_end = self.data_offset.checked_add(end)?;
        self.bytes.get(data_start..data_end)
    }

    fn validated_word_bytes(&self, entry: usize) -> Result<&[u8], ValidationError> {
        let (start, end) = self
            .word_offsets(entry)
            .ok_or(ValidationError::TruncatedIndex { entry })?;
        if start > end || end > self.data_len {
            return Err(ValidationError::WordOutsideData { entry });
        }
        let data_start = self
            .data_offset
            .checked_add(start)
            .ok_or(ValidationError::WordOutsideData { entry })?;
        let data_end = self
            .data_offset
            .checked_add(end)
            .ok_or(ValidationError::WordOutsideData { entry })?;
        self.bytes
            .get(data_start..data_end)
            .ok_or(ValidationError::WordOutsideData { entry })
    }

    fn word_offsets(&self, entry: usize) -> Option<(usize, usize)> {
        let index_entry = self
            .index_offset
            .checked_add(entry.checked_mul(INDEX_ENTRY_SIZE)?)?;
        let start = usize::try_from(read_u64(&self.bytes, index_entry)?).ok()?;
        let end = usize::try_from(read_u64(&self.bytes, index_entry.checked_add(8)?)?).ok()?;
        Some((start, end))
    }
}

fn validate_header(bytes: &[u8]) -> Result<(), LoadError> {
    if bytes.len() < HEADER_SIZE {
        return Err(truncated_header(bytes));
    }
    if bytes[..MAGIC.len()] != MAGIC {
        return Err(LoadError::InvalidMagic);
    }

    let version = required_u16(bytes, VERSION_OFFSET)?;
    if version != FORMAT_VERSION {
        return Err(LoadError::UnsupportedVersion { found: version });
    }
    let header_size = required_u16(bytes, HEADER_SIZE_OFFSET)?;
    if header_size != HEADER_SIZE_U16 {
        return Err(LoadError::InvalidHeaderSize { found: header_size });
    }
    let flags = required_u32(bytes, FLAGS_OFFSET)?;
    if flags != 0 {
        return Err(LoadError::UnsupportedFeatures { found: flags });
    }

    let declared_len = required_u64(bytes, FILE_LEN_OFFSET)?;
    if usize::try_from(declared_len).ok() != Some(bytes.len()) {
        return Err(LoadError::FileLengthMismatch {
            declared: declared_len,
            actual: bytes.len(),
        });
    }
    let declared_checksum = required_u64(bytes, CHECKSUM_OFFSET)?;
    let actual_checksum = checksum(bytes);
    if declared_checksum != actual_checksum {
        return Err(LoadError::ChecksumMismatch {
            declared: declared_checksum,
            actual: actual_checksum,
        });
    }
    Ok(())
}

fn truncated_header(bytes: &[u8]) -> LoadError {
    LoadError::TruncatedHeader {
        actual: bytes.len(),
    }
}

fn required_u16(bytes: &[u8], offset: usize) -> Result<u16, LoadError> {
    read_u16(bytes, offset).ok_or_else(|| truncated_header(bytes))
}

fn required_u32(bytes: &[u8], offset: usize) -> Result<u32, LoadError> {
    read_u32(bytes, offset).ok_or_else(|| truncated_header(bytes))
}

fn required_u64(bytes: &[u8], offset: usize) -> Result<u64, LoadError> {
    read_u64(bytes, offset).ok_or_else(|| truncated_header(bytes))
}

impl Dictionary for CompiledDictionary {
    fn contains(&self, word: &str) -> bool {
        let mut left = 0_usize;
        let mut right = self.word_count;
        let query = word.as_bytes();

        while left < right {
            let middle = left + (right - left) / 2;
            let Some(candidate) = self.word_bytes(middle) else {
                return false;
            };
            match candidate.cmp(query) {
                std::cmp::Ordering::Less => left = middle + 1,
                std::cmp::Ordering::Equal => return true,
                std::cmp::Ordering::Greater => right = middle,
            }
        }
        false
    }
}

fn align_to_eight(value: usize) -> Option<usize> {
    value.checked_add(7).map(|value| value & !7)
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    bytes
        .get(offset..offset.checked_add(2)?)?
        .try_into()
        .ok()
        .map(u16::from_le_bytes)
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    bytes
        .get(offset..offset.checked_add(4)?)?
        .try_into()
        .ok()
        .map(u32::from_le_bytes)
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    bytes
        .get(offset..offset.checked_add(8)?)?
        .try_into()
        .ok()
        .map(u64::from_le_bytes)
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

/// Computes the format's non-cryptographic FNV-1a integrity checksum.
///
/// The checksum field itself is logically zero while calculating the result.
/// It detects accidental corruption cheaply; it is not a signature or security
/// boundary.
fn checksum(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    bytes
        .iter()
        .enumerate()
        .fold(OFFSET_BASIS, |hash, (index, byte)| {
            let byte = if (CHECKSUM_OFFSET..CHECKSUM_END).contains(&index) {
                0
            } else {
                *byte
            };
            (hash ^ u64::from(byte)).wrapping_mul(PRIME)
        })
}

#[cfg(test)]
mod tests {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    use super::{
        checksum, compile_words, put_u64, CompileError, CompiledDictionary, LoadError,
        ValidationError, CHECKSUM_OFFSET, DATA_OFFSET, INDEX_OFFSET,
    };
    use ferrolex_core::Dictionary;

    #[test]
    fn compilation_is_byte_identical_across_input_order_and_duplicates() {
        let first =
            compile_words(["zebra", "東京", "apple", "zebra"]).expect("valid words compile");
        let second = compile_words(["東京", "apple", "zebra"]).expect("valid words compile");

        assert_eq!(first, second);
    }

    #[test]
    fn compiled_dictionary_uses_allocation_free_exact_utf8_lookup() {
        let bytes = compile_words(["Straße", "東京", "🦀"]).expect("valid words compile");
        let dictionary = CompiledDictionary::load(bytes).expect("output loads");

        assert!(dictionary.contains("Straße"));
        assert!(dictionary.contains("東京"));
        assert!(dictionary.contains("🦀"));
        assert!(!dictionary.contains("Strasse"));
        assert!(!dictionary.contains("straße"));
        assert_eq!(dictionary.len(), 3);
        dictionary
            .validate()
            .expect("compiler output is structurally valid");
    }

    #[test]
    fn compiler_rejects_empty_words() {
        assert_eq!(
            compile_words(["word", "", "later"]),
            Err(CompileError::EmptyWord { position: 2 })
        );
    }

    #[test]
    fn loader_rejects_payload_corruption_by_checksum() {
        let mut bytes = compile_words(["word"]).expect("valid words compile");
        let data_offset =
            usize::try_from(read_header_u64(&bytes, DATA_OFFSET)).expect("test platform");
        bytes[data_offset] ^= 1;

        assert!(matches!(
            CompiledDictionary::load(bytes),
            Err(LoadError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn loader_rejects_truncated_files() {
        let mut bytes = compile_words(["word"]).expect("valid words compile");
        bytes.pop();

        assert!(matches!(
            CompiledDictionary::load(bytes),
            Err(LoadError::FileLengthMismatch { .. })
        ));
    }

    #[test]
    fn validation_reports_invalid_utf8_after_fast_loading() {
        let mut bytes = compile_words(["word"]).expect("valid words compile");
        let data_offset =
            usize::try_from(read_header_u64(&bytes, DATA_OFFSET)).expect("test platform");
        bytes[data_offset] = 0xff;
        refresh_checksum(&mut bytes);
        let dictionary = CompiledDictionary::load(bytes).expect("header and checksum remain valid");

        assert_eq!(
            dictionary.validate(),
            Err(ValidationError::InvalidUtf8 { entry: 0 })
        );
        assert!(!dictionary.contains("word"));
    }

    #[test]
    fn validation_reports_unsorted_index_entries() {
        let mut bytes = compile_words(["alpha", "beta"]).expect("valid words compile");
        let index_offset =
            usize::try_from(read_header_u64(&bytes, INDEX_OFFSET)).expect("test platform");
        let first = bytes[index_offset..index_offset + 16].to_vec();
        let second = bytes[index_offset + 16..index_offset + 32].to_vec();
        bytes[index_offset..index_offset + 16].copy_from_slice(&second);
        bytes[index_offset + 16..index_offset + 32].copy_from_slice(&first);
        refresh_checksum(&mut bytes);
        let dictionary =
            CompiledDictionary::load(bytes).expect("fast loading permits deferred validation");

        assert_eq!(
            dictionary.validate(),
            Err(ValidationError::UnsortedWords { entry: 1 })
        );
    }

    #[test]
    fn sections_are_eight_byte_aligned_and_header_is_little_endian() {
        let bytes = compile_words(["abc"]).expect("valid words compile");
        let index_offset = read_header_u64(&bytes, INDEX_OFFSET);
        let data_offset = read_header_u64(&bytes, DATA_OFFSET);

        assert_eq!(index_offset % 8, 0);
        assert_eq!(data_offset % 8, 0);
        assert_eq!(&bytes[8..10], &[1, 0]);
    }

    #[test]
    fn deterministic_adversarial_loader_corpus_never_panics() {
        let template = compile_words(["alpha", "東京", "🦀"])
            .expect("valid words compile into a test template");

        for length in 0..=template.len() {
            assert_loader_handles(&template[..length], format_args!("truncation at {length}"));
        }
        for offset in 0..template.len() {
            let mut mutated = template.clone();
            mutated[offset] ^= 0xa5;
            refresh_checksum(&mut mutated);
            assert_loader_handles(&mutated, format_args!("single-byte mutation at {offset}"));
        }

        let mut state = 0x6d0f_27bd_4c51_aa93_u64;
        for case in 0..512 {
            let length = usize::try_from(next_random(&mut state) % 513)
                .expect("small deterministic corpus length fits usize");
            let mut bytes = vec![0_u8; length];
            for byte in &mut bytes {
                *byte = next_random(&mut state).to_le_bytes()[0];
            }
            assert_loader_handles(&bytes, format_args!("seeded byte case {case}"));
        }
    }

    fn assert_loader_handles(bytes: &[u8], label: std::fmt::Arguments<'_>) {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            if let Ok(dictionary) = CompiledDictionary::load(bytes.to_vec()) {
                let _ = dictionary.validate();
                for query in ["", "alpha", "東京", "🦀", "not-present"] {
                    let _ = dictionary.contains(query);
                }
            }
        }));
        assert!(outcome.is_ok(), "compiled loader panicked for {label}");
    }

    fn next_random(state: &mut u64) -> u64 {
        *state ^= *state << 7;
        *state ^= *state >> 9;
        *state ^= *state << 8;
        *state
    }

    fn read_header_u64(bytes: &[u8], offset: usize) -> u64 {
        u64::from_le_bytes(
            bytes[offset..offset + 8]
                .try_into()
                .expect("header field exists"),
        )
    }

    fn refresh_checksum(bytes: &mut [u8]) {
        let calculated_checksum = checksum(bytes);
        put_u64(bytes, CHECKSUM_OFFSET, calculated_checksum);
    }
}

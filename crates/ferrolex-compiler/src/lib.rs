//! A deterministic, bounds-checked compiled dictionary format.
//!
//! The format is designed to be suitable for a memory-mapped backing store:
//! all fields are little-endian integers, sections are offset-addressed and
//! eight-byte aligned, and lookup performs no allocation.  This initial
//! version only represents exact words. Metadata and morphology are separate
//! future format features rather than implicit, unstable payloads.
//!
//! ```
//! use ferrolex_compiler::{compile_words, CompiledDictionary};
//! use ferrolex_core::Dictionary;
//!
//! let bytes = compile_words(["ferrolex"])?;
//! let dictionary = CompiledDictionary::load(bytes)?;
//! assert!(dictionary.contains("ferrolex"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod ir;

use std::fmt;
use std::sync::{Arc, OnceLock};

use ferrolex_core::{CandidateIndex, Dictionary};
use ferrolex_suggest::CandidateSource;

pub use ir::{
    AffixKindIr, AffixRuleIr, BreakPatternIr, CaseLanguageIr, CompoundConfigIr, CompoundPatternIr,
    CompoundSyllableLimitIr, ConditionAtomIr, ConditionIr, DictionaryIr, FlagIr, FlagModeIr,
    InputConversionIr, LexemeIr, ReplacementRuleIr, SpecialFlagsIr,
};

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
const FEATURE_FREQUENCIES: u32 = 1;

/// Self-describing metadata available without decoding dictionary words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompiledArtifactMetadata {
    format_version: u16,
    feature_bits: u32,
    word_count: u64,
}

impl CompiledArtifactMetadata {
    /// Serialized layout version.
    #[must_use]
    pub const fn format_version(self) -> u16 {
        self.format_version
    }

    /// Feature bits declared by the artifact.
    #[must_use]
    pub const fn feature_bits(self) -> u32 {
        self.feature_bits
    }

    /// Number of unique exact words.
    #[must_use]
    pub const fn word_count(self) -> u64 {
        self.word_count
    }
}

/// Largest compiled artifact accepted by the version-1 runtime.
///
/// Command-line callers should check a file's metadata before reading it; the
/// in-memory loader repeats the limit for embedded callers.
pub const MAX_COMPILED_ARTIFACT_BYTES: usize = 128 * 1024 * 1024;

/// Inspects a `FLEXDIC` header after its integrity checks.
///
/// This validates the fixed header and checksum without decoding every entry.
/// Use [`CompiledDictionary::validate`] when the complete structural scan is
/// required.
///
/// # Errors
///
/// Returns [`LoadError`] when the artifact cannot be recognized safely.
pub fn inspect_compiled_artifact(bytes: &[u8]) -> Result<CompiledArtifactMetadata, LoadError> {
    if bytes.len() > MAX_COMPILED_ARTIFACT_BYTES {
        return Err(LoadError::ArtifactTooLarge {
            actual: bytes.len(),
        });
    }
    validate_header(bytes)?;
    Ok(CompiledArtifactMetadata {
        format_version: required_u16(bytes, VERSION_OFFSET)?,
        feature_bits: required_u32(bytes, FLAGS_OFFSET)?,
        word_count: required_u64(bytes, WORD_COUNT_OFFSET)?,
    })
}

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

/// A malformed line in the tab-separated frequency word-list format.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrequencyListError {
    /// A non-comment row was not exactly `word<TAB>unsigned-frequency`.
    InvalidEntry {
        /// One-based line number of the malformed row.
        line: usize,
    },
    /// The frequency field was not an unsigned decimal integer.
    InvalidFrequency {
        /// One-based line number of the malformed row.
        line: usize,
    },
    /// The parsed entries cannot fit into the native artifact.
    Compile(CompileError),
}

impl fmt::Display for FrequencyListError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEntry { line } => write!(
                formatter,
                "frequency word list line {line} must be `word<TAB>unsigned-frequency`"
            ),
            Self::InvalidFrequency { line } => {
                write!(
                    formatter,
                    "frequency word list line {line} has an invalid frequency"
                )
            }
            Self::Compile(source) => source.fmt(formatter),
        }
    }
}

impl std::error::Error for FrequencyListError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Compile(source) => Some(source),
            Self::InvalidEntry { .. } | Self::InvalidFrequency { .. } => None,
        }
    }
}

/// A format-neutral exact-word dictionary input for the native compiler.
///
/// This deliberately represents only semantics supported by the version-1
/// `FLEXDIC` runtime: non-empty UTF-8 words with exact recognition. Importers
/// with morphology must either preserve that richer runtime separately or
/// explicitly project a reviewed exact-word subset; the compiler never drops
/// semantic features implicitly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactDictionaryIr {
    words: Vec<Box<str>>,
    frequencies: Vec<Option<u64>>,
}

impl ExactDictionaryIr {
    /// Builds a deterministic exact-word IR from non-empty UTF-8 input.
    ///
    /// # Errors
    ///
    /// Returns [`CompileError::EmptyWord`] for an empty input entry.
    pub fn new<I, S>(input: I) -> Result<Self, CompileError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut entries = Vec::<(Box<str>, Option<u64>)>::new();
        for (index, word) in input.into_iter().enumerate() {
            let word = word.as_ref();
            if word.is_empty() {
                return Err(CompileError::EmptyWord {
                    position: index + 1,
                });
            }
            entries.push((Box::from(word), None));
        }
        Ok(Self::from_entries(entries))
    }

    /// Builds an exact-word IR with optional frequency metadata for ranking.
    ///
    /// Repeated spellings retain their highest supplied frequency so the
    /// resulting artifact does not depend on input order.
    ///
    /// # Errors
    ///
    /// Returns [`CompileError::EmptyWord`] for an empty spelling.
    pub fn with_frequencies<I, S>(input: I) -> Result<Self, CompileError>
    where
        I: IntoIterator<Item = (S, u64)>,
        S: AsRef<str>,
    {
        let mut entries = Vec::<(Box<str>, Option<u64>)>::new();
        for (index, (word, frequency)) in input.into_iter().enumerate() {
            let word = word.as_ref();
            if word.is_empty() {
                return Err(CompileError::EmptyWord {
                    position: index + 1,
                });
            }
            entries.push((Box::from(word), Some(frequency)));
        }
        Ok(Self::from_entries(entries))
    }

    /// Visits normalized IR words in deterministic UTF-8 byte order.
    pub fn words(&self) -> impl ExactSizeIterator<Item = &str> + DoubleEndedIterator + '_ {
        self.words.iter().map(Box::as_ref)
    }

    /// Projects exact-word input into the shared linguistic IR.
    ///
    /// Exact word lists deliberately use only the lexeme portion of the IR;
    /// all other fields retain their semantics-free defaults.
    #[must_use]
    pub fn as_dictionary_ir(&self) -> DictionaryIr {
        DictionaryIr {
            lexemes: self
                .words
                .iter()
                .map(|word| LexemeIr {
                    stem: word.to_string(),
                    frequency: self.frequency_for(word),
                    flags: std::collections::BTreeSet::new(),
                    morphology: Vec::new(),
                })
                .collect(),
            ..DictionaryIr::default()
        }
    }

    fn from_entries(mut entries: Vec<(Box<str>, Option<u64>)>) -> Self {
        entries.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        let mut words: Vec<Box<str>> = Vec::with_capacity(entries.len());
        let mut frequencies: Vec<Option<u64>> = Vec::with_capacity(entries.len());
        for (word, frequency) in entries {
            if words
                .last()
                .map(<Box<str> as AsRef<str>>::as_ref)
                .is_some_and(|previous: &str| previous == word.as_ref())
            {
                if frequency > *frequencies.last().expect("frequency follows word") {
                    *frequencies.last_mut().expect("frequency follows word") = frequency;
                }
                continue;
            }
            words.push(word);
            frequencies.push(frequency);
        }
        Self { words, frequencies }
    }

    fn frequency_for(&self, word: &str) -> Option<u64> {
        self.words
            .binary_search_by(|candidate| candidate.as_ref().cmp(word))
            .ok()
            .and_then(|index| self.frequencies[index])
    }
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
    let ir = ExactDictionaryIr::new(words)?;
    compile_exact_ir(&ir)
}

/// Compiles a tab-separated `word<TAB>unsigned-frequency` list.
///
/// Empty lines and `#` comments are ignored. Repeated words retain their
/// highest frequency, making the result independent of input order. Frequency
/// influences suggestions only; exact dictionary recognition is unchanged.
///
/// # Errors
///
/// Returns [`FrequencyListError`] when a data line is malformed or the
/// resulting artifact exceeds native bounds.
pub fn compile_frequency_word_list(text: &str) -> Result<Vec<u8>, FrequencyListError> {
    let mut entries = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = if index == 0 {
            line.strip_prefix('\u{feff}').unwrap_or(line)
        } else {
            line
        }
        .trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((word, frequency)) = line.split_once('\t') else {
            return Err(FrequencyListError::InvalidEntry { line: index + 1 });
        };
        if word.is_empty() || frequency.is_empty() || frequency.contains('\t') {
            return Err(FrequencyListError::InvalidEntry { line: index + 1 });
        }
        let frequency = frequency
            .parse::<u64>()
            .map_err(|_| FrequencyListError::InvalidFrequency { line: index + 1 })?;
        entries.push((word, frequency));
    }
    let ir = ExactDictionaryIr::with_frequencies(entries).map_err(FrequencyListError::Compile)?;
    compile_exact_ir(&ir).map_err(FrequencyListError::Compile)
}

/// Compiles a format-neutral exact-word IR into `FLEXDIC` version 1.
///
/// # Errors
///
/// Returns [`CompileError::DictionaryTooLarge`] if the output cannot be
/// represented by the version-1 layout.
#[allow(
    clippy::too_many_lines,
    reason = "the compact artifact writer keeps every checked layout calculation auditable"
)]
pub fn compile_exact_ir(ir: &ExactDictionaryIr) -> Result<Vec<u8>, CompileError> {
    let sorted_words = &ir.words;

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
    let data_end = data_offset
        .checked_add(data_len)
        .ok_or(CompileError::DictionaryTooLarge)?;
    let has_frequencies = ir.frequencies.iter().any(Option::is_some);
    let frequency_offset = has_frequencies.then(|| align_to_eight(data_end)).flatten();
    let frequency_len = if has_frequencies {
        sorted_words
            .len()
            .checked_mul(std::mem::size_of::<u64>())
            .ok_or(CompileError::DictionaryTooLarge)?
    } else {
        0
    };
    let file_len = frequency_offset
        .map_or(Some(data_end), |offset| offset.checked_add(frequency_len))
        .ok_or(CompileError::DictionaryTooLarge)?;

    let mut bytes = vec![0_u8; file_len];
    bytes[..MAGIC.len()].copy_from_slice(&MAGIC);
    put_u16(&mut bytes, VERSION_OFFSET, FORMAT_VERSION);
    put_u16(&mut bytes, HEADER_SIZE_OFFSET, HEADER_SIZE_U16);
    put_u32(
        &mut bytes,
        FLAGS_OFFSET,
        if has_frequencies {
            FEATURE_FREQUENCIES
        } else {
            0
        },
    );
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
    if let Some(frequency_offset) = frequency_offset {
        for (entry, frequency) in ir.frequencies.iter().enumerate() {
            put_u64(
                &mut bytes,
                frequency_offset + entry * std::mem::size_of::<u64>(),
                frequency.unwrap_or(0),
            );
        }
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
#[derive(Clone, Debug)]
pub struct CompiledDictionary {
    bytes: Vec<u8>,
    word_count: usize,
    index_offset: usize,
    data_offset: usize,
    data_len: usize,
    frequency_offset: Option<usize>,
    candidate_index: Arc<OnceLock<CandidateIndex>>,
}

impl PartialEq for CompiledDictionary {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl Eq for CompiledDictionary {}

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
        let has_frequencies =
            read_u32(&bytes, FLAGS_OFFSET).is_some_and(|flags| flags & FEATURE_FREQUENCIES != 0);
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
                .is_none_or(|end| end > bytes.len())
        {
            return Err(LoadError::InvalidLayout {
                reason: LayoutError::IndexOutsideFile,
            });
        }
        if data_offset
            .checked_add(data_len)
            .is_none_or(|end| end > bytes.len())
        {
            return Err(LoadError::InvalidLayout {
                reason: LayoutError::DataOutsideFile,
            });
        }
        let frequency_offset = if has_frequencies {
            let offset = align_to_eight(data_offset.checked_add(data_len).ok_or(
                LoadError::InvalidLayout {
                    reason: LayoutError::DataOutsideFile,
                },
            )?)
            .ok_or(LoadError::InvalidLayout {
                reason: LayoutError::DataOutsideFile,
            })?;
            let length = word_count.checked_mul(std::mem::size_of::<u64>()).ok_or(
                LoadError::InvalidLayout {
                    reason: LayoutError::DataOutsideFile,
                },
            )?;
            if offset.checked_add(length) != Some(bytes.len()) {
                return Err(LoadError::InvalidLayout {
                    reason: LayoutError::DataOutsideFile,
                });
            }
            Some(offset)
        } else {
            if data_offset.checked_add(data_len) != Some(bytes.len()) {
                return Err(LoadError::InvalidLayout {
                    reason: LayoutError::DataOutsideFile,
                });
            }
            None
        };

        Ok(Self {
            bytes,
            word_count,
            index_offset,
            data_offset,
            data_len,
            frequency_offset,
            candidate_index: Arc::new(OnceLock::new()),
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

    /// Visits valid UTF-8 words in serialized byte-lexical order.
    ///
    /// This does not allocate or force paranoid validation. A malformed entry
    /// accepted by fast loading is simply not exposed as a candidate; callers
    /// that require a structural guarantee should call [`Self::validate`]
    /// before using the artifact.
    pub fn words(&self) -> impl Iterator<Item = &str> + '_ {
        (0..self.word_count).filter_map(|entry| {
            self.word_bytes(entry)
                .and_then(|word| std::str::from_utf8(word).ok())
        })
    }

    /// Returns an optional ranking frequency for an exact stored word.
    #[must_use]
    pub fn frequency(&self, word: &str) -> Option<u64> {
        self.word_index(word)
            .and_then(|entry| self.frequency_at(entry))
    }

    fn frequency_at(&self, entry: usize) -> Option<u64> {
        let offset = self.frequency_offset?.checked_add(entry.checked_mul(8)?)?;
        read_u64(&self.bytes, offset).filter(|frequency| *frequency != 0)
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

    fn word_index(&self, word: &str) -> Option<usize> {
        let mut left = 0_usize;
        let mut right = self.word_count;
        while left < right {
            let middle = left + (right - left) / 2;
            let candidate = self.word_bytes(middle)?;
            match candidate.cmp(word.as_bytes()) {
                std::cmp::Ordering::Less => left = middle + 1,
                std::cmp::Ordering::Equal => return Some(middle),
                std::cmp::Ordering::Greater => right = middle,
            }
        }
        None
    }
}

impl CandidateSource for CompiledDictionary {
    fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        for word in self.words() {
            if !visitor(word) {
                break;
            }
        }
    }

    fn visit_nearby_candidates(
        &self,
        query: &[char],
        max_edit_distance: usize,
        max_word_scalars: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        self.candidate_index
            .get_or_init(|| CandidateIndex::new(self.words(), max_word_scalars))
            .visit_nearby(query, max_edit_distance, max_word_scalars, visitor);
    }

    fn candidate_frequency(&self, candidate: &str) -> Option<u64> {
        self.frequency(candidate)
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
    if flags & !FEATURE_FREQUENCIES != 0 {
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
        self.word_index(word).is_some()
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

    fn extend(mut hash: u64, bytes: &[u8]) -> u64 {
        for &byte in bytes {
            hash = (hash ^ u64::from(byte)).wrapping_mul(PRIME);
        }
        hash
    }

    let checksum_offset = bytes.len().min(CHECKSUM_OFFSET);
    let checksum_end = bytes.len().min(CHECKSUM_END);
    let hash = extend(OFFSET_BASIS, &bytes[..checksum_offset]);
    let zeroed_checksum = [0; CHECKSUM_END - CHECKSUM_OFFSET];
    let hash = extend(
        hash,
        &zeroed_checksum[..checksum_end.saturating_sub(checksum_offset)],
    );
    extend(hash, &bytes[checksum_end..])
}

#[cfg(test)]
mod tests {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    use super::{
        checksum, compile_exact_ir, compile_frequency_word_list, compile_words,
        inspect_compiled_artifact, put_u64, CompileError, CompiledDictionary, ExactDictionaryIr,
        FrequencyListError, LoadError, ValidationError, CHECKSUM_END, CHECKSUM_OFFSET, DATA_OFFSET,
        INDEX_OFFSET,
    };
    use ferrolex_core::Dictionary;
    use ferrolex_suggest::{CandidateSource, SuggestConfig, Suggester};
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn compilation_is_deterministic_for_generated_word_sets(words in proptest::collection::vec("[a-z]{1,12}", 1..32)) {
            let forward = compile_words(words.iter().map(String::as_str)).expect("generated words are valid");
            let reverse = compile_words(words.iter().rev().map(String::as_str)).expect("generated words are valid");
            prop_assert_eq!(&forward, &reverse);
            let dictionary = CompiledDictionary::load(forward).expect("generated artifact loads");
            prop_assert!(dictionary.validate().is_ok());
            for word in words { prop_assert!(dictionary.contains(&word)); }
        }
    }

    #[test]
    fn compilation_is_byte_identical_across_input_order_and_duplicates() {
        let first =
            compile_words(["zebra", "東京", "apple", "zebra"]).expect("valid words compile");
        let second = compile_words(["東京", "apple", "zebra"]).expect("valid words compile");

        assert_eq!(first, second);
    }

    #[test]
    fn frequency_word_lists_round_trip_and_rank_equally_close_candidates() {
        let bytes =
            compile_frequency_word_list("cat\t1\ncut\t9\n").expect("frequency list compiles");
        let metadata = inspect_compiled_artifact(&bytes).expect("artifact metadata is valid");
        let dictionary = CompiledDictionary::load(bytes).expect("artifact loads");

        assert_eq!(metadata.feature_bits(), 1);
        assert_eq!(dictionary.frequency("cat"), Some(1));
        assert_eq!(dictionary.frequency("cut"), Some(9));
        assert_eq!(
            ExactDictionaryIr::with_frequencies([("cat", 1), ("cut", 9)])
                .expect("frequency IR is valid")
                .as_dictionary_ir()
                .lexemes
                .iter()
                .map(|lexeme| (lexeme.stem.as_str(), lexeme.frequency))
                .collect::<Vec<_>>(),
            [("cat", Some(1)), ("cut", Some(9))]
        );
        assert_eq!(
            Suggester::new(&dictionary, SuggestConfig::default())
                .suggest("cot")
                .suggestions()[0]
                .word(),
            "cut"
        );
    }

    #[test]
    fn frequency_word_lists_reject_malformed_rows() {
        assert_eq!(
            compile_frequency_word_list("word 1\n"),
            Err(FrequencyListError::InvalidEntry { line: 1 })
        );
        assert_eq!(
            compile_frequency_word_list("word\tfrequent\n"),
            Err(FrequencyListError::InvalidFrequency { line: 1 })
        );
    }

    #[test]
    fn inspection_exposes_exact_artifact_metadata_without_loading_words() {
        let bytes = compile_words(["zebra", "ant"]).expect("the words compile");

        let metadata = inspect_compiled_artifact(&bytes).expect("the header is valid");

        assert_eq!(metadata.format_version(), super::FORMAT_VERSION);
        assert_eq!(metadata.feature_bits(), 0);
        assert_eq!(metadata.word_count(), 2);
    }

    #[test]
    fn exact_ir_is_format_neutral_and_compiles_deterministically() {
        let ir = ExactDictionaryIr::new(["東京", "apple", "apple"])
            .expect("non-empty UTF-8 words build an IR");

        assert_eq!(ir.words().collect::<Vec<_>>(), ["apple", "東京"]);
        assert_eq!(
            compile_exact_ir(&ir).expect("IR compiles"),
            compile_words(["apple", "東京"]).expect("word list compiles")
        );
    }

    #[test]
    fn exact_word_ir_projects_to_the_shared_linguistic_model() {
        let ir = ExactDictionaryIr::new(["東京", "apple", "apple"])
            .expect("non-empty UTF-8 words build an IR");
        let linguistic = ir.as_dictionary_ir();

        assert_eq!(linguistic.lexemes.len(), 2);
        assert_eq!(linguistic.lexemes[0].stem, "apple");
        assert_eq!(linguistic.lexemes[1].stem, "東京");
        assert!(linguistic.prefixes.is_empty());
        assert!(linguistic.suffixes.is_empty());
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
    fn exposes_serialized_words_as_suggestion_candidates() {
        let dictionary = CompiledDictionary::load(
            compile_words(["zebra", "東京", "apple"]).expect("valid words compile"),
        )
        .expect("artifact loads");
        let mut candidates = Vec::new();
        dictionary.visit_candidates(&mut |word| {
            candidates.push(word.to_owned());
            true
        });

        assert_eq!(candidates, ["apple", "zebra", "東京"]);
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
    fn checksum_range_hashing_matches_the_bytewise_format_definition() {
        const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;

        for length in [0_usize, 15, 16, 20, 24, 25, 64] {
            let bytes: Vec<u8> = (0..length)
                .map(|index| u8::try_from(index).expect("test byte fits"))
                .collect();
            let reference = bytes
                .iter()
                .enumerate()
                .fold(OFFSET_BASIS, |hash, (index, byte)| {
                    let byte = if (CHECKSUM_OFFSET..CHECKSUM_END).contains(&index) {
                        0
                    } else {
                        *byte
                    };
                    (hash ^ u64::from(byte)).wrapping_mul(PRIME)
                });

            assert_eq!(checksum(&bytes), reference, "length {length}");
        }
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

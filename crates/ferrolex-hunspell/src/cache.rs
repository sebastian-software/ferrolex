//! Versioned, provenance-bound runtime artifacts for parsed Hunspell input.
//!
//! This deliberately does not share the plain word-list `FLEXDIC` format: a
//! Hunspell dictionary needs its flags, affix rules, conditions, and rule IDs
//! intact to retain recognition semantics.

#![allow(
    clippy::module_name_repetitions,
    reason = "the public cache names make the artifact boundary explicit at the crate root"
)]

use std::collections::BTreeSet;
use std::fmt;

use sha2::{Digest as _, Sha256};

use super::{
    decode_text_flag, encode_text_flag, AffixKind, AffixRule, BreakPattern, CaseLanguage,
    CompoundConfig, CompoundPattern, CompoundRule, CompoundSyllableLimit, Condition, ConditionAtom,
    Flag, FlagMode, FlagSet, HunspellDictionary, InputConversion, Lexeme, Morphology, MorphologyId,
    MorphologyTable, SpecialFlags, MAX_AFFIX_RULES, MAX_BREAK_PATTERNS, MAX_CHARACTER_MAPS,
    MAX_COMPOUND_PATTERNS, MAX_COMPOUND_RULES, MAX_COMPOUND_RULE_COMPONENTS,
    MAX_COMPOUND_RULE_EXPANSIONS, MAX_COMPOUND_RULE_EXPANSIONS_PER_RULE, MAX_COMPOUND_SCALARS,
    MAX_CONDITION_ATOMS, MAX_DICTIONARY_ENTRIES, MAX_FLAGS_PER_ENTRY, MAX_INPUT_CONVERSIONS,
    MAX_LINE_BYTES, MAX_MORPHOLOGY_FIELDS_PER_RECORD, MAX_MORPHOLOGY_STRINGS,
    MAX_REPLACEMENT_RULES,
};
use ferrolex_suggest::ReplacementRule;

const MAGIC: [u8; 8] = *b"FLXHSP\0\0";
const CHECKSUM_BYTES: usize = 32;
const HEADER_BYTES: usize = MAGIC.len() + 2 + 4 + (CHECKSUM_BYTES * 2);
const MAX_RUNTIME_CACHE_BYTES: usize = 128 * 1024 * 1024;

/// The on-disk layout version for a Hunspell runtime cache.
pub const HUNSPELL_CACHE_FORMAT_VERSION: u16 = 6;

/// The recognition semantics encoded by a Hunspell runtime cache.
///
/// This changes whenever the runtime's interpretation of any serialized field
/// changes. A cache with another semantics version is always rebuilt.
pub const HUNSPELL_CACHE_SEMANTICS_VERSION: u32 = 31;

/// SHA-256 provenance of the exact raw `.aff` and `.dic` source bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceDigests {
    aff: [u8; CHECKSUM_BYTES],
    dic: [u8; CHECKSUM_BYTES],
}

/// Immutable identity and compatibility data stored in a runtime cache.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeCacheMetadata {
    format_version: u16,
    semantics_version: u32,
    sources: SourceDigests,
}

impl RuntimeCacheMetadata {
    /// Artifact layout version.
    #[must_use]
    pub const fn format_version(self) -> u16 {
        self.format_version
    }

    /// Recognition-semantics version.
    #[must_use]
    pub const fn semantics_version(self) -> u32 {
        self.semantics_version
    }

    /// Exact source-byte digests used to compile the artifact.
    #[must_use]
    pub const fn sources(self) -> SourceDigests {
        self.sources
    }
}

impl SourceDigests {
    /// Creates provenance from precomputed SHA-256 digests.
    #[must_use]
    pub const fn new(aff: [u8; CHECKSUM_BYTES], dic: [u8; CHECKSUM_BYTES]) -> Self {
        Self { aff, dic }
    }

    /// Calculates provenance from the unmodified source bytes.
    #[must_use]
    pub fn from_source_bytes(aff: &[u8], dic: &[u8]) -> Self {
        Self::new(Sha256::digest(aff).into(), Sha256::digest(dic).into())
    }

    /// Returns the affix-file SHA-256 digest.
    #[must_use]
    pub const fn aff(self) -> [u8; CHECKSUM_BYTES] {
        self.aff
    }

    /// Returns the word-list SHA-256 digest.
    #[must_use]
    pub const fn dic(self) -> [u8; CHECKSUM_BYTES] {
        self.dic
    }
}

/// The source component whose provenance did not match a cache artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheSource {
    /// The `.aff` file.
    Aff,
    /// The `.dic` file.
    Dic,
}

impl fmt::Display for CacheSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Aff => "affix source",
            Self::Dic => "dictionary source",
        })
    }
}

/// A rejected Hunspell runtime-cache artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeCacheError {
    /// The artifact does not use the Hunspell runtime-cache format.
    InvalidMagic,
    /// The format layout cannot be decoded by this version of ferrolex.
    UnsupportedFormatVersion(u16),
    /// The serialized recognition rules have another semantics version.
    UnsupportedSemanticsVersion(u32),
    /// The cache was compiled from different source bytes.
    SourceDigestMismatch(CacheSource),
    /// The cache is larger than the configured defensive artifact limit.
    ArtifactTooLarge,
    /// The artifact checksum does not cover the exact preceding bytes.
    ChecksumMismatch,
    /// The artifact is truncated, malformed, or violates a runtime invariant.
    InvalidArtifact(&'static str),
    /// The supplied in-memory dictionary cannot be safely represented.
    InvalidDictionary(&'static str),
}

impl fmt::Display for RuntimeCacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => formatter.write_str("invalid Hunspell runtime-cache magic"),
            Self::UnsupportedFormatVersion(version) => write!(
                formatter,
                "unsupported Hunspell runtime-cache format version {version}"
            ),
            Self::UnsupportedSemanticsVersion(version) => write!(
                formatter,
                "unsupported Hunspell runtime-cache semantics version {version}"
            ),
            Self::SourceDigestMismatch(source) => {
                write!(
                    formatter,
                    "Hunspell runtime cache does not match its {source}"
                )
            }
            Self::ArtifactTooLarge => formatter
                .write_str("Hunspell runtime-cache artifact exceeds the configured 128 MiB limit"),
            Self::ChecksumMismatch => {
                formatter.write_str("Hunspell runtime-cache checksum mismatch")
            }
            Self::InvalidArtifact(reason) => {
                write!(
                    formatter,
                    "invalid Hunspell runtime-cache artifact: {reason}"
                )
            }
            Self::InvalidDictionary(reason) => {
                write!(formatter, "cannot compile Hunspell runtime cache: {reason}")
            }
        }
    }
}

impl std::error::Error for RuntimeCacheError {}

/// Serializes a parsed Hunspell dictionary into a deterministic runtime cache.
///
/// The cache stores every field that can affect recognition, its semantic
/// version, SHA-256 provenance for both raw source files, and an artifact
/// checksum. It is therefore derived data: callers should discard and rebuild
/// it when this function or [`load_runtime_cache`] returns an error.
///
/// # Errors
///
/// Returns [`RuntimeCacheError::InvalidDictionary`] if a manually constructed
/// or otherwise invalid dictionary violates an importer invariant.
#[allow(
    clippy::too_many_lines,
    reason = "the cache header and each recognition-affecting section are emitted together"
)]
pub fn compile_runtime_cache(
    dictionary: &HunspellDictionary,
    sources: SourceDigests,
) -> Result<Vec<u8>, RuntimeCacheError> {
    validate_dictionary(dictionary, DictionaryError::Compile)?;

    let mut output = Vec::new();
    output.extend_from_slice(&MAGIC);
    write_u16(&mut output, HUNSPELL_CACHE_FORMAT_VERSION);
    write_u32(&mut output, HUNSPELL_CACHE_SEMANTICS_VERSION);
    output.extend_from_slice(&sources.aff);
    output.extend_from_slice(&sources.dic);
    write_flag_mode(&mut output, dictionary.flag_mode);
    output.push(u8::from(dictionary.case_fallback));
    write_case_language(&mut output, dictionary.case_language);
    write_morphology_table(&mut output, &dictionary.morphology)?;
    write_lexemes(
        &mut output,
        &dictionary.lexemes,
        dictionary.flag_mode,
        &dictionary.morphology,
    )?;
    write_rules(
        &mut output,
        &dictionary.prefixes,
        dictionary.flag_mode,
        &dictionary.morphology,
    )?;
    write_rules(
        &mut output,
        &dictionary.suffixes,
        dictionary.flag_mode,
        &dictionary.morphology,
    )?;
    write_special_flags(&mut output, &dictionary.special_flags, dictionary.flag_mode)?;
    write_optional_string(
        &mut output,
        dictionary.keyboard.as_deref(),
        "keyboard layout",
    )?;
    write_character_maps(&mut output, &dictionary.character_maps)?;
    write_optional_flag(
        &mut output,
        dictionary.compound.flag.as_ref(),
        dictionary.flag_mode,
    )?;
    write_optional_flag(
        &mut output,
        dictionary.compound.begin.as_ref(),
        dictionary.flag_mode,
    )?;
    write_optional_flag(
        &mut output,
        dictionary.compound.middle.as_ref(),
        dictionary.flag_mode,
    )?;
    write_optional_flag(
        &mut output,
        dictionary.compound.end.as_ref(),
        dictionary.flag_mode,
    )?;
    write_optional_flag(
        &mut output,
        dictionary.compound.permit.as_ref(),
        dictionary.flag_mode,
    )?;
    write_optional_flag(
        &mut output,
        dictionary.compound.forbid.as_ref(),
        dictionary.flag_mode,
    )?;
    write_optional_flag(
        &mut output,
        dictionary.compound.force_uppercase.as_ref(),
        dictionary.flag_mode,
    )?;
    write_u64(
        &mut output,
        u64::try_from(dictionary.compound.minimum_length)
            .map_err(|_| RuntimeCacheError::InvalidDictionary("compound minimum is too large"))?,
    );
    write_optional_usize(
        &mut output,
        dictionary.compound.maximum_words,
        "compound maximum",
    )?;
    output.push(u8::from(dictionary.compound.check_duplicate));
    output.push(u8::from(dictionary.compound.check_replacement));
    output.push(u8::from(dictionary.compound.check_case));
    output.push(u8::from(dictionary.compound.check_triple));
    output.push(u8::from(dictionary.compound.simplified_triple));
    ensure_artifact_size(
        output.len().saturating_add(CHECKSUM_BYTES),
        DictionaryError::Compile,
    )?;
    write_count(
        &mut output,
        dictionary.compound.rules.len(),
        "compound rule count",
    )?;
    for rule in &dictionary.compound.rules {
        write_count(
            &mut output,
            rule.patterns.len(),
            "compound rule expansion count",
        )?;
        for pattern in &rule.patterns {
            write_count(&mut output, pattern.len(), "compound rule component count")?;
            for flag in pattern {
                write_flag(&mut output, *flag, dictionary.flag_mode)?;
            }
        }
    }
    write_compound_patterns(
        &mut output,
        &dictionary.compound.patterns,
        dictionary.flag_mode,
    )?;
    write_compound_syllable_limit(&mut output, dictionary.compound.syllable_limit.as_ref())?;
    write_break_patterns(&mut output, &dictionary.break_patterns)?;
    write_count(
        &mut output,
        dictionary.word_characters.len(),
        "word character count",
    )?;
    for character in &dictionary.word_characters {
        write_u32(&mut output, u32::from(*character));
    }
    write_replacement_rules(&mut output, &dictionary.replacement_rules)?;
    write_count(
        &mut output,
        dictionary.ignored_characters.len(),
        "ignored character count",
    )?;
    for character in &dictionary.ignored_characters {
        write_u32(&mut output, u32::from(*character));
    }
    write_input_conversions(&mut output, &dictionary.input_conversions)?;
    write_input_conversions(&mut output, &dictionary.output_conversions)?;
    output.push(u8::from(dictionary.full_strip));
    output.push(u8::from(dictionary.complex_prefixes));
    output.extend_from_slice(&Sha256::digest(&output));
    Ok(output)
}

/// Compiles a standalone Hunspell artifact.
///
/// `sources` is retained as descriptive provenance in the artifact, while the
/// standalone loader does not require the original files to be present.
///
/// # Errors
///
/// Returns [`RuntimeCacheError`] when the dictionary violates artifact bounds.
pub fn compile_runtime_artifact(
    dictionary: &HunspellDictionary,
    sources: SourceDigests,
) -> Result<Vec<u8>, RuntimeCacheError> {
    compile_runtime_cache(dictionary, sources)
}

/// Reads self-describing metadata from a checksummed runtime cache.
///
/// This only establishes that the artifact header and checksum are intact; use
/// [`load_runtime_cache`] to validate every serialized recognition field.
///
/// # Errors
///
/// Returns an error for an oversized, truncated, malformed, or checksum-invalid
/// artifact.
pub fn inspect_runtime_cache(bytes: &[u8]) -> Result<RuntimeCacheMetadata, RuntimeCacheError> {
    let checksum_at = verify_runtime_cache_integrity(bytes)?;
    read_runtime_cache_metadata(bytes, checksum_at)
}

/// Returns whether `bytes` start with the Hunspell runtime-artifact marker.
///
/// This is a cheap format discriminator only. Call [`inspect_runtime_cache`]
/// or a loader to validate the complete artifact.
#[must_use]
pub fn is_runtime_artifact(bytes: &[u8]) -> bool {
    bytes.starts_with(&MAGIC)
}

fn verify_runtime_cache_integrity(bytes: &[u8]) -> Result<usize, RuntimeCacheError> {
    if bytes.len() > MAX_RUNTIME_CACHE_BYTES {
        return Err(RuntimeCacheError::ArtifactTooLarge);
    }
    if bytes.len() < HEADER_BYTES + CHECKSUM_BYTES {
        return Err(RuntimeCacheError::InvalidArtifact("artifact is truncated"));
    }
    let checksum_at = bytes.len() - CHECKSUM_BYTES;
    if Sha256::digest(&bytes[..checksum_at]).as_slice() != &bytes[checksum_at..] {
        return Err(RuntimeCacheError::ChecksumMismatch);
    }
    Ok(checksum_at)
}

fn read_runtime_cache_metadata(
    bytes: &[u8],
    checksum_at: usize,
) -> Result<RuntimeCacheMetadata, RuntimeCacheError> {
    let mut reader = Reader::new(&bytes[..checksum_at]);
    if reader.take_array::<8>()? != MAGIC {
        return Err(RuntimeCacheError::InvalidMagic);
    }
    Ok(RuntimeCacheMetadata {
        format_version: reader.u16()?,
        semantics_version: reader.u32()?,
        sources: SourceDigests::new(
            reader.take_array::<CHECKSUM_BYTES>()?,
            reader.take_array::<CHECKSUM_BYTES>()?,
        ),
    })
}

/// Loads a standalone Hunspell artifact without requiring the original sources.
///
/// Source digests remain embedded as descriptive provenance, but are not used
/// as a runtime availability requirement. The complete serialized dictionary
/// is still checksum- and bounds-validated before it is returned.
///
/// # Errors
///
/// Returns the same errors as [`inspect_runtime_cache`] and
/// [`load_runtime_cache`] for malformed or unsupported artifacts.
pub fn load_runtime_artifact(bytes: &[u8]) -> Result<HunspellDictionary, RuntimeCacheError> {
    let checksum_at = verify_runtime_cache_integrity(bytes)?;
    let sources = read_runtime_cache_metadata(bytes, checksum_at)?.sources();
    load_verified_runtime_cache(bytes, checksum_at, sources)
}

/// Loads a fully validated Hunspell runtime cache for exact source provenance.
///
/// This performs all format, version, checksum, bounds, UTF-8, ordering, and
/// recognition-invariant checks before constructing a dictionary. It never
/// returns a partial dictionary.
///
/// # Errors
///
/// Returns a structured [`RuntimeCacheError`] for stale, unsupported, corrupt,
/// or malformed artifacts. Callers should rebuild the derived cache instead of
/// attempting to repair it in place.
pub fn load_runtime_cache(
    bytes: &[u8],
    sources: SourceDigests,
) -> Result<HunspellDictionary, RuntimeCacheError> {
    let checksum_at = verify_runtime_cache_integrity(bytes)?;
    load_verified_runtime_cache(bytes, checksum_at, sources)
}

#[allow(
    clippy::too_many_lines,
    reason = "validation and reconstruction share one bounded reader to keep the artifact boundary auditable"
)]
fn load_verified_runtime_cache(
    bytes: &[u8],
    checksum_at: usize,
    sources: SourceDigests,
) -> Result<HunspellDictionary, RuntimeCacheError> {
    let mut reader = Reader::new(&bytes[..checksum_at]);
    if reader.take_array::<8>()? != MAGIC {
        return Err(RuntimeCacheError::InvalidMagic);
    }
    let format_version = reader.u16()?;
    if format_version != HUNSPELL_CACHE_FORMAT_VERSION {
        return Err(RuntimeCacheError::UnsupportedFormatVersion(format_version));
    }
    let semantics_version = reader.u32()?;
    if semantics_version != HUNSPELL_CACHE_SEMANTICS_VERSION {
        return Err(RuntimeCacheError::UnsupportedSemanticsVersion(
            semantics_version,
        ));
    }
    if reader.take_array::<CHECKSUM_BYTES>()? != sources.aff {
        return Err(RuntimeCacheError::SourceDigestMismatch(CacheSource::Aff));
    }
    if reader.take_array::<CHECKSUM_BYTES>()? != sources.dic {
        return Err(RuntimeCacheError::SourceDigestMismatch(CacheSource::Dic));
    }
    let flag_mode = read_flag_mode(&mut reader)?;
    let case_fallback = read_boolean(&mut reader, "invalid LANG fallback marker")?;
    let case_language = read_case_language(&mut reader)?;
    let morphology = read_morphology_table(&mut reader)?;

    let lexemes = read_lexemes(&mut reader, flag_mode, &morphology)?;
    let prefixes = read_rules(&mut reader, AffixKind::Prefix, flag_mode, &morphology)?;
    let suffixes = read_rules(&mut reader, AffixKind::Suffix, flag_mode, &morphology)?;
    let special_flags = read_special_flags(&mut reader, flag_mode)?;
    let keyboard = read_optional_string(&mut reader, "keyboard layout")?.map(Box::from);
    let character_maps = read_character_maps(&mut reader)?;
    let flag = read_optional_flag(&mut reader, flag_mode)?;
    let begin = read_optional_flag(&mut reader, flag_mode)?;
    let middle = read_optional_flag(&mut reader, flag_mode)?;
    let end = read_optional_flag(&mut reader, flag_mode)?;
    let permit = read_optional_flag(&mut reader, flag_mode)?;
    let forbid = read_optional_flag(&mut reader, flag_mode)?;
    let force_uppercase = read_optional_flag(&mut reader, flag_mode)?;
    let minimum_length = usize::try_from(reader.u64()?)
        .map_err(|_| RuntimeCacheError::InvalidArtifact("compound minimum is too large"))?;
    let maximum_words = read_optional_usize(&mut reader, "compound maximum is too large")?;
    let check_duplicate = read_boolean(&mut reader, "invalid compound duplicate marker")?;
    let check_replacement = read_boolean(&mut reader, "invalid compound replacement marker")?;
    let check_case = read_boolean(&mut reader, "invalid compound case marker")?;
    let check_triple = read_boolean(&mut reader, "invalid compound triple marker")?;
    let simplified_triple = read_boolean(&mut reader, "invalid simplified compound triple marker")?;
    let rule_count = reader.count(MAX_COMPOUND_RULES, "compound rule count")?;
    let mut rules = Vec::with_capacity(rule_count);
    let mut compound_expansion_count = 0;
    for _ in 0..rule_count {
        let expansion_count = reader.count(
            MAX_COMPOUND_RULE_EXPANSIONS_PER_RULE,
            "compound rule expansion count",
        )?;
        if expansion_count == 0 {
            return Err(RuntimeCacheError::InvalidArtifact(
                "compound rule has no expansions",
            ));
        }
        compound_expansion_count += expansion_count;
        if compound_expansion_count > MAX_COMPOUND_RULE_EXPANSIONS {
            return Err(RuntimeCacheError::InvalidArtifact(
                "compound rules exceed the expansion limit",
            ));
        }
        let mut patterns = Vec::with_capacity(expansion_count);
        for _ in 0..expansion_count {
            let component_count = reader.count(
                MAX_COMPOUND_RULE_COMPONENTS,
                "compound rule component count",
            )?;
            if component_count < 2 {
                return Err(RuntimeCacheError::InvalidArtifact(
                    "compound rule has fewer than two components",
                ));
            }
            let mut flags = Vec::with_capacity(component_count);
            for _ in 0..component_count {
                flags.push(reader.flag(flag_mode)?);
            }
            patterns.push(flags);
        }
        rules.push(CompoundRule { patterns });
    }
    let patterns = read_compound_patterns(&mut reader, flag_mode)?;
    let syllable_limit = read_compound_syllable_limit(&mut reader)?;
    let break_patterns = read_break_patterns(&mut reader)?;
    let word_character_count = reader.count(MAX_LINE_BYTES, "word character count")?;
    reader.require_minimum_items(word_character_count, 4, "word characters")?;
    let mut word_characters = BTreeSet::new();
    for _ in 0..word_character_count {
        let character = char::from_u32(reader.u32()?).ok_or(RuntimeCacheError::InvalidArtifact(
            "word character is invalid",
        ))?;
        if !word_characters.insert(character) {
            return Err(RuntimeCacheError::InvalidArtifact(
                "duplicate word character",
            ));
        }
    }
    let replacement_rules = read_replacement_rules(&mut reader)?;
    let ignored_character_count = reader.count(MAX_LINE_BYTES, "ignored character count")?;
    reader.require_minimum_items(ignored_character_count, 4, "ignored characters")?;
    let mut ignored_characters = BTreeSet::new();
    for _ in 0..ignored_character_count {
        let character = reader.character()?;
        if !ignored_characters.insert(character) {
            return Err(RuntimeCacheError::InvalidArtifact(
                "duplicate ignored character",
            ));
        }
    }
    let input_conversions = read_input_conversions(&mut reader)?;
    let output_conversions = read_input_conversions(&mut reader)?;
    let full_strip = read_boolean(&mut reader, "invalid fullstrip marker")?;
    let complex_prefixes = read_boolean(&mut reader, "invalid complex-prefix marker")?;
    let compound = CompoundConfig {
        flag,
        begin,
        middle,
        end,
        permit,
        forbid,
        force_uppercase,
        minimum_length,
        maximum_words,
        check_duplicate,
        check_replacement,
        check_case,
        check_triple,
        simplified_triple,
        patterns,
        syllable_limit,
        rules,
    };
    if !reader.is_empty() {
        return Err(RuntimeCacheError::InvalidArtifact(
            "artifact has trailing payload bytes",
        ));
    }

    let dictionary = HunspellDictionary::from_parts(
        flag_mode,
        case_fallback,
        case_language,
        morphology,
        lexemes,
        prefixes,
        suffixes,
        special_flags,
        compound,
        break_patterns,
        word_characters,
        replacement_rules,
        keyboard,
        character_maps,
        ignored_characters,
        input_conversions,
        output_conversions,
        full_strip,
        complex_prefixes,
    );
    validate_dictionary(&dictionary, DictionaryError::Load)?;
    Ok(dictionary)
}

#[derive(Clone, Copy)]
enum DictionaryError {
    Compile,
    Load,
}

impl DictionaryError {
    fn error(self, message: &'static str) -> RuntimeCacheError {
        match self {
            Self::Compile => RuntimeCacheError::InvalidDictionary(message),
            Self::Load => RuntimeCacheError::InvalidArtifact(message),
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "the cache invariant checks stay together as one auditable boundary"
)]
fn validate_dictionary(
    dictionary: &HunspellDictionary,
    error: DictionaryError,
) -> Result<(), RuntimeCacheError> {
    if dictionary.morphology.ids.len() > MAX_MORPHOLOGY_STRINGS
        || dictionary
            .morphology
            .values_by_id()
            .iter()
            .any(|field| field.is_empty() || field.len() > MAX_LINE_BYTES)
    {
        return Err(error.error("morphology table exceeds importer limits"));
    }
    if dictionary.lexemes.len() > MAX_DICTIONARY_ENTRIES {
        return Err(error.error("dictionary entry count exceeds importer limit"));
    }
    let mut previous_stem = None;
    for lexeme in &dictionary.lexemes {
        if lexeme.stem.is_empty() || lexeme.stem.len() > MAX_LINE_BYTES {
            return Err(error.error("lexeme stem has an invalid byte length"));
        }
        if previous_stem.is_some_and(|previous| previous > lexeme.stem.as_ref()) {
            return Err(error.error("lexemes are not in sorted stem order"));
        }
        previous_stem = Some(lexeme.stem.as_ref());
        validate_flags(&lexeme.flags, dictionary.flag_mode, error)?;
        validate_morphology(&lexeme.morphology, &dictionary.morphology, error)?;
    }
    let mut stored_unique_stems = dictionary.unique_stem_indices.iter();
    let indices_match = dictionary
        .lexemes
        .iter()
        .enumerate()
        .filter(|(index, lexeme)| *index == 0 || dictionary.lexemes[*index - 1].stem != lexeme.stem)
        .all(|(index, _)| stored_unique_stems.next().copied() == u32::try_from(index).ok())
        && stored_unique_stems.next().is_none();
    if !indices_match {
        return Err(error.error("stem index does not match lexemes"));
    }

    let rule_count = dictionary
        .prefixes
        .len()
        .checked_add(dictionary.suffixes.len())
        .ok_or_else(|| error.error("affix rule count overflows"))?;
    if rule_count > MAX_AFFIX_RULES {
        return Err(error.error("affix rule count exceeds importer limit"));
    }
    let mut rule_ids = BTreeSet::new();
    for rule in dictionary.prefixes.iter().chain(&dictionary.suffixes) {
        if rule.id >= rule_count || !rule_ids.insert(rule.id) {
            return Err(error.error("affix rule IDs are not a unique dense range"));
        }
        validate_rule(rule, dictionary.flag_mode, error)?;
        validate_morphology(&rule.morphology, &dictionary.morphology, error)?;
    }
    if rule_ids.len() != rule_count {
        return Err(error.error("affix rule IDs are not a unique dense range"));
    }
    validate_optional_flag(
        dictionary.special_flags.circumfix.as_ref(),
        dictionary.flag_mode,
        error,
    )?;
    validate_optional_flag(
        dictionary.special_flags.forbidden_word.as_ref(),
        dictionary.flag_mode,
        error,
    )?;
    validate_optional_flag(
        dictionary.special_flags.keep_case.as_ref(),
        dictionary.flag_mode,
        error,
    )?;
    validate_optional_flag(
        dictionary.special_flags.need_affix.as_ref(),
        dictionary.flag_mode,
        error,
    )?;
    validate_optional_flag(
        dictionary.special_flags.only_in_compound.as_ref(),
        dictionary.flag_mode,
        error,
    )?;
    validate_optional_flag(
        dictionary.special_flags.no_suggest.as_ref(),
        dictionary.flag_mode,
        error,
    )?;
    validate_optional_flag(
        dictionary.compound.flag.as_ref(),
        dictionary.flag_mode,
        error,
    )?;
    validate_optional_flag(
        dictionary.compound.begin.as_ref(),
        dictionary.flag_mode,
        error,
    )?;
    validate_optional_flag(
        dictionary.compound.middle.as_ref(),
        dictionary.flag_mode,
        error,
    )?;
    validate_optional_flag(
        dictionary.compound.end.as_ref(),
        dictionary.flag_mode,
        error,
    )?;
    validate_optional_flag(
        dictionary.compound.permit.as_ref(),
        dictionary.flag_mode,
        error,
    )?;
    validate_optional_flag(
        dictionary.compound.forbid.as_ref(),
        dictionary.flag_mode,
        error,
    )?;
    validate_optional_flag(
        dictionary.compound.force_uppercase.as_ref(),
        dictionary.flag_mode,
        error,
    )?;
    if dictionary.compound.minimum_length == 0 {
        return Err(error.error("compound minimum must be greater than zero"));
    }
    if dictionary.compound.rules.len() > MAX_COMPOUND_RULES {
        return Err(error.error("compound rule count exceeds importer limit"));
    }
    if dictionary
        .compound
        .maximum_words
        .is_some_and(|maximum| maximum == 0 || maximum > MAX_COMPOUND_SCALARS)
    {
        return Err(error.error("compound maximum is outside the importer limit"));
    }
    if dictionary.compound.patterns.len() > MAX_COMPOUND_PATTERNS {
        return Err(error.error("compound pattern count exceeds importer limit"));
    }
    for pattern in &dictionary.compound.patterns {
        validate_optional_flag(pattern.ending_flag.as_ref(), dictionary.flag_mode, error)?;
        validate_optional_flag(pattern.beginning_flag.as_ref(), dictionary.flag_mode, error)?;
        if pattern.ending.len() > MAX_LINE_BYTES
            || pattern.beginning.len() > MAX_LINE_BYTES
            || pattern
                .replacement
                .as_ref()
                .is_some_and(|text| text.len() > MAX_LINE_BYTES)
        {
            return Err(error.error("compound pattern text exceeds importer line limit"));
        }
    }
    if let Some(limit) = &dictionary.compound.syllable_limit {
        if limit.vowels.is_empty() || limit.vowels.len() > MAX_LINE_BYTES {
            return Err(error.error("compound syllable vowel set is invalid"));
        }
    }
    let mut compound_expansion_count = 0;
    for rule in &dictionary.compound.rules {
        if rule.patterns.is_empty() || rule.patterns.len() > MAX_COMPOUND_RULE_EXPANSIONS_PER_RULE {
            return Err(error.error("compound rule has an invalid expansion count"));
        }
        compound_expansion_count += rule.patterns.len();
        if compound_expansion_count > MAX_COMPOUND_RULE_EXPANSIONS {
            return Err(error.error("compound rules exceed the expansion limit"));
        }
        for pattern in &rule.patterns {
            if !(2..=MAX_COMPOUND_RULE_COMPONENTS).contains(&pattern.len()) {
                return Err(error.error("compound rule has an invalid component count"));
            }
            for flag in pattern {
                validate_flag(*flag, dictionary.flag_mode, error)?;
            }
        }
    }
    if dictionary.break_patterns.len() > MAX_BREAK_PATTERNS
        || dictionary
            .break_patterns
            .iter()
            .any(|pattern| pattern.text.is_empty() || pattern.text.len() > MAX_LINE_BYTES)
    {
        return Err(error.error("break pattern is outside the importer limit"));
    }
    if dictionary.word_characters.len() > MAX_LINE_BYTES {
        return Err(error.error("word character count exceeds importer line limit"));
    }
    if dictionary.replacement_rules.len() > MAX_REPLACEMENT_RULES {
        return Err(error.error("replacement rule count exceeds importer limit"));
    }
    if dictionary
        .keyboard
        .as_ref()
        .is_some_and(|layout| layout.is_empty() || layout.len() > MAX_LINE_BYTES)
    {
        return Err(error.error("keyboard layout is outside the importer limit"));
    }
    if dictionary.character_maps.len() > MAX_CHARACTER_MAPS
        || dictionary
            .character_maps
            .iter()
            .any(|group| group.len() > MAX_LINE_BYTES || group.chars().count() < 2)
    {
        return Err(error.error("character map is outside the importer limit"));
    }
    if dictionary.ignored_characters.len() > MAX_LINE_BYTES {
        return Err(error.error("ignored character count exceeds importer line limit"));
    }
    if dictionary.input_conversions.len() > MAX_INPUT_CONVERSIONS {
        return Err(error.error("input conversion count exceeds importer limit"));
    }
    if dictionary.output_conversions.len() > MAX_INPUT_CONVERSIONS {
        return Err(error.error("output conversion count exceeds importer limit"));
    }
    for conversion in dictionary
        .input_conversions
        .iter()
        .chain(&dictionary.output_conversions)
    {
        if conversion.from.is_empty()
            || conversion.from.len() > MAX_LINE_BYTES
            || conversion.to.len() > MAX_LINE_BYTES
        {
            return Err(error.error("conversion has invalid string text"));
        }
    }
    for rule in &dictionary.replacement_rules {
        if rule.from().len() > MAX_LINE_BYTES || rule.to().len() > MAX_LINE_BYTES {
            return Err(error.error("replacement rule has invalid spelling text"));
        }
    }
    Ok(())
}

fn validate_morphology(
    morphology: &Morphology,
    table: &MorphologyTable,
    error: DictionaryError,
) -> Result<(), RuntimeCacheError> {
    if morphology.len() > MAX_MORPHOLOGY_FIELDS_PER_RECORD
        || morphology.iter().any(|id| !table.contains(*id))
    {
        return Err(error.error("morphology fields exceed importer limits"));
    }
    Ok(())
}

fn validate_rule(
    rule: &AffixRule,
    flag_mode: FlagMode,
    error: DictionaryError,
) -> Result<(), RuntimeCacheError> {
    validate_flag(rule.flag, flag_mode, error)?;
    if rule.strip.len() > MAX_LINE_BYTES || rule.add.len() > MAX_LINE_BYTES {
        return Err(error.error("affix rule text exceeds importer line limit"));
    }
    validate_flags(&rule.continuation_flags, flag_mode, error)?;
    if rule.condition.atoms.len() > MAX_CONDITION_ATOMS {
        return Err(error.error("affix condition exceeds importer atom limit"));
    }
    for atom in &rule.condition.atoms {
        validate_condition_atom(atom, error)?;
    }
    if let Some(atom) = &rule.condition.not_preceded_by {
        validate_condition_atom(atom, error)?;
    }
    Ok(())
}

fn validate_condition_atom(
    atom: &ConditionAtom,
    error: DictionaryError,
) -> Result<(), RuntimeCacheError> {
    if let ConditionAtom::Class { members, .. } = atom {
        if members.is_empty() || members.len() > MAX_LINE_BYTES {
            return Err(error.error("affix condition class has an invalid member count"));
        }
    }
    Ok(())
}

fn validate_optional_flag(
    flag: Option<&Flag>,
    flag_mode: FlagMode,
    error: DictionaryError,
) -> Result<(), RuntimeCacheError> {
    if let Some(flag) = flag {
        validate_flag(*flag, flag_mode, error)?;
    }
    Ok(())
}

fn validate_flags(
    flags: &[Flag],
    flag_mode: FlagMode,
    error: DictionaryError,
) -> Result<(), RuntimeCacheError> {
    if flags.len() > MAX_FLAGS_PER_ENTRY {
        return Err(error.error("flag count exceeds importer limit"));
    }
    for flag in flags {
        validate_flag(*flag, flag_mode, error)?;
    }
    Ok(())
}

fn validate_flag(
    flag: Flag,
    flag_mode: FlagMode,
    error: DictionaryError,
) -> Result<(), RuntimeCacheError> {
    if !flag.is_valid_for(flag_mode) {
        return Err(error.error("flag does not match the dictionary FLAG mode"));
    }
    Ok(())
}

fn write_lexemes(
    output: &mut Vec<u8>,
    lexemes: &[Lexeme],
    flag_mode: FlagMode,
    morphology_table: &MorphologyTable,
) -> Result<(), RuntimeCacheError> {
    write_count(output, lexemes.len(), "dictionary entry count")?;
    for lexeme in lexemes {
        write_string(output, &lexeme.stem, "lexeme stem")?;
        write_flags(output, &lexeme.flags, flag_mode)?;
        write_morphology(output, &lexeme.morphology, morphology_table)?;
    }
    Ok(())
}

fn write_rules(
    output: &mut Vec<u8>,
    rules: &[AffixRule],
    flag_mode: FlagMode,
    morphology_table: &MorphologyTable,
) -> Result<(), RuntimeCacheError> {
    write_count(output, rules.len(), "affix rule count")?;
    for rule in rules {
        write_u32(
            output,
            u32::try_from(rule.id)
                .map_err(|_| RuntimeCacheError::InvalidDictionary("affix rule ID is too large"))?,
        );
        output.push(match rule.kind {
            AffixKind::Prefix => 0,
            AffixKind::Suffix => 1,
        });
        write_flag(output, rule.flag, flag_mode)?;
        write_string(output, &rule.strip, "affix strip")?;
        write_string(output, &rule.add, "affix add")?;
        write_condition(output, &rule.condition)?;
        output.push(u8::from(rule.cross_product));
        write_flags(output, &rule.continuation_flags, flag_mode)?;
        write_morphology(output, &rule.morphology, morphology_table)?;
    }
    Ok(())
}

fn write_special_flags(
    output: &mut Vec<u8>,
    special_flags: &SpecialFlags,
    flag_mode: FlagMode,
) -> Result<(), RuntimeCacheError> {
    write_optional_flag(output, special_flags.circumfix.as_ref(), flag_mode)?;
    write_optional_flag(output, special_flags.forbidden_word.as_ref(), flag_mode)?;
    write_optional_flag(output, special_flags.keep_case.as_ref(), flag_mode)?;
    write_optional_flag(output, special_flags.need_affix.as_ref(), flag_mode)?;
    write_optional_flag(output, special_flags.only_in_compound.as_ref(), flag_mode)?;
    write_optional_flag(output, special_flags.no_suggest.as_ref(), flag_mode)?;
    output.push(u8::from(special_flags.check_sharps));
    Ok(())
}

fn write_optional_flag(
    output: &mut Vec<u8>,
    flag: Option<&Flag>,
    flag_mode: FlagMode,
) -> Result<(), RuntimeCacheError> {
    if let Some(flag) = flag {
        output.push(1);
        write_flag(output, *flag, flag_mode)
    } else {
        output.push(0);
        Ok(())
    }
}

fn write_flags(
    output: &mut Vec<u8>,
    flags: &[Flag],
    flag_mode: FlagMode,
) -> Result<(), RuntimeCacheError> {
    write_count(output, flags.len(), "flag count")?;
    for flag in flags {
        write_flag(output, *flag, flag_mode)?;
    }
    Ok(())
}

fn write_morphology_table(
    output: &mut Vec<u8>,
    table: &MorphologyTable,
) -> Result<(), RuntimeCacheError> {
    write_count(output, table.ids.len(), "morphology string count")?;
    for value in table.values_by_id() {
        if value.is_empty() {
            return Err(RuntimeCacheError::InvalidDictionary(
                "morphology table contains an empty field",
            ));
        }
        write_string(output, value, "morphology field")?;
    }
    Ok(())
}

fn write_morphology(
    output: &mut Vec<u8>,
    morphology: &Morphology,
    table: &MorphologyTable,
) -> Result<(), RuntimeCacheError> {
    write_count(output, morphology.len(), "morphology field count")?;
    for id in morphology {
        if !table.contains(*id) {
            return Err(RuntimeCacheError::InvalidDictionary(
                "morphology field references an unknown table entry",
            ));
        }
        write_u32(output, id.0);
    }
    Ok(())
}

fn write_flag(
    output: &mut Vec<u8>,
    flag: Flag,
    flag_mode: FlagMode,
) -> Result<(), RuntimeCacheError> {
    match flag_mode {
        FlagMode::Numeric => {
            write_u32(
                output,
                u32::try_from(flag.0).map_err(|_| {
                    RuntimeCacheError::InvalidDictionary(
                        "numeric flag does not fit in the cache representation",
                    )
                })?,
            );
            Ok(())
        }
        FlagMode::Unicode | FlagMode::Long => write_string(
            output,
            &decode_text_flag(flag.0).ok_or(RuntimeCacheError::InvalidDictionary(
                "flag contains an invalid Unicode scalar",
            ))?,
            "flag",
        ),
    }
}

fn write_flag_mode(output: &mut Vec<u8>, flag_mode: FlagMode) {
    output.push(match flag_mode {
        FlagMode::Unicode => 0,
        FlagMode::Long => 1,
        FlagMode::Numeric => 2,
    });
}

fn write_case_language(output: &mut Vec<u8>, case_language: CaseLanguage) {
    output.push(match case_language {
        CaseLanguage::Default => 0,
        CaseLanguage::Turkic => 1,
    });
}

fn write_condition(output: &mut Vec<u8>, condition: &Condition) -> Result<(), RuntimeCacheError> {
    output.push(u8::from(condition.anchored_at_start));
    match &condition.not_preceded_by {
        None => output.push(0),
        Some(atom) => {
            output.push(1);
            write_condition_atom(output, atom);
        }
    }
    write_count(output, condition.atoms.len(), "condition atom count")?;
    for atom in &condition.atoms {
        write_condition_atom(output, atom);
    }
    Ok(())
}

fn write_condition_atom(output: &mut Vec<u8>, atom: &ConditionAtom) {
    match atom {
        ConditionAtom::Any => output.push(0),
        ConditionAtom::Literal(character) => {
            output.push(1);
            write_u32(output, u32::from(*character));
        }
        ConditionAtom::Class { members, negated } => {
            output.push(2);
            output.push(u8::from(*negated));
            write_u32(
                output,
                u32::try_from(members.len()).expect("validated class size"),
            );
            for character in members {
                write_u32(output, u32::from(*character));
            }
        }
    }
}

fn write_string(
    output: &mut Vec<u8>,
    value: &str,
    name: &'static str,
) -> Result<(), RuntimeCacheError> {
    if value.len() > MAX_LINE_BYTES {
        return Err(RuntimeCacheError::InvalidDictionary(name));
    }
    write_u32(
        output,
        u32::try_from(value.len()).map_err(|_| RuntimeCacheError::InvalidDictionary(name))?,
    );
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn write_optional_string(
    output: &mut Vec<u8>,
    value: Option<&str>,
    name: &'static str,
) -> Result<(), RuntimeCacheError> {
    if let Some(value) = value {
        output.push(1);
        write_string(output, value, name)
    } else {
        output.push(0);
        Ok(())
    }
}

fn write_character_maps(
    output: &mut Vec<u8>,
    character_maps: &[String],
) -> Result<(), RuntimeCacheError> {
    write_count(output, character_maps.len(), "character map count")?;
    for group in character_maps {
        write_string(output, group, "character map group")?;
    }
    Ok(())
}

fn write_replacement_rules(
    output: &mut Vec<u8>,
    rules: &[ReplacementRule],
) -> Result<(), RuntimeCacheError> {
    write_count(output, rules.len(), "replacement rule count")?;
    for rule in rules {
        write_string(output, rule.from(), "replacement source spelling")?;
        write_string(output, rule.to(), "replacement target spelling")?;
        output.push(u8::from(rule.at_word_start()));
        output.push(u8::from(rule.at_word_end()));
    }
    Ok(())
}

fn write_input_conversions(
    output: &mut Vec<u8>,
    conversions: &[InputConversion],
) -> Result<(), RuntimeCacheError> {
    write_count(output, conversions.len(), "input conversion count")?;
    for conversion in conversions {
        write_string(output, &conversion.from, "input conversion source")?;
        write_string(output, &conversion.to, "input conversion target")?;
        output.push(u8::from(conversion.at_word_start));
        output.push(u8::from(conversion.at_word_end));
    }
    Ok(())
}

fn write_count(
    output: &mut Vec<u8>,
    count: usize,
    name: &'static str,
) -> Result<(), RuntimeCacheError> {
    write_u32(
        output,
        u32::try_from(count).map_err(|_| RuntimeCacheError::InvalidDictionary(name))?,
    );
    Ok(())
}

fn write_optional_usize(
    output: &mut Vec<u8>,
    value: Option<usize>,
    name: &'static str,
) -> Result<(), RuntimeCacheError> {
    let value = value.map_or(0, |value| u64::try_from(value).unwrap_or(u64::MAX));
    if value == u64::MAX {
        return Err(RuntimeCacheError::InvalidDictionary(name));
    }
    write_u64(output, value);
    Ok(())
}

fn write_break_patterns(
    output: &mut Vec<u8>,
    patterns: &[BreakPattern],
) -> Result<(), RuntimeCacheError> {
    write_count(output, patterns.len(), "break pattern count")?;
    for pattern in patterns {
        write_string(output, &pattern.text, "break pattern text")?;
        output.push(u8::from(pattern.at_start));
        output.push(u8::from(pattern.at_end));
    }
    Ok(())
}

fn write_compound_patterns(
    output: &mut Vec<u8>,
    patterns: &[CompoundPattern],
    flag_mode: FlagMode,
) -> Result<(), RuntimeCacheError> {
    write_count(output, patterns.len(), "compound pattern count")?;
    for pattern in patterns {
        write_string(output, &pattern.ending, "compound pattern ending")?;
        write_optional_flag(output, pattern.ending_flag.as_ref(), flag_mode)?;
        write_string(output, &pattern.beginning, "compound pattern beginning")?;
        write_optional_flag(output, pattern.beginning_flag.as_ref(), flag_mode)?;
        match &pattern.replacement {
            None => output.push(0),
            Some(replacement) => {
                output.push(1);
                write_string(output, replacement, "compound pattern replacement")?;
            }
        }
    }
    Ok(())
}

fn write_compound_syllable_limit(
    output: &mut Vec<u8>,
    limit: Option<&CompoundSyllableLimit>,
) -> Result<(), RuntimeCacheError> {
    match limit {
        None => output.push(0),
        Some(limit) => {
            output.push(1);
            write_u64(
                output,
                u64::try_from(limit.maximum)
                    .map_err(|_| RuntimeCacheError::InvalidDictionary("compound syllable limit"))?,
            );
            write_count(output, limit.vowels.len(), "compound syllable vowel count")?;
            for vowel in &limit.vowels {
                write_u32(output, u32::from(*vowel));
            }
        }
    }
    Ok(())
}

fn write_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn write_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn read_flag_mode(reader: &mut Reader<'_>) -> Result<FlagMode, RuntimeCacheError> {
    match reader.byte()? {
        0 => Ok(FlagMode::Unicode),
        1 => Ok(FlagMode::Long),
        2 => Ok(FlagMode::Numeric),
        _ => Err(RuntimeCacheError::InvalidArtifact("invalid FLAG mode")),
    }
}

fn read_case_language(reader: &mut Reader<'_>) -> Result<CaseLanguage, RuntimeCacheError> {
    match reader.byte()? {
        0 => Ok(CaseLanguage::Default),
        1 => Ok(CaseLanguage::Turkic),
        _ => Err(RuntimeCacheError::InvalidArtifact("invalid LANG case mode")),
    }
}

fn read_morphology_table(reader: &mut Reader<'_>) -> Result<MorphologyTable, RuntimeCacheError> {
    let count = reader.count(MAX_MORPHOLOGY_STRINGS, "morphology string count")?;
    reader.require_minimum_items(count, 4, "morphology strings")?;
    let mut table = MorphologyTable::default();
    for expected_id in 0..count {
        let field = reader.string(MAX_LINE_BYTES, "morphology field")?;
        if field.is_empty() {
            return Err(RuntimeCacheError::InvalidArtifact(
                "morphology field is empty",
            ));
        }
        let Some(id) = table.intern(&field) else {
            return Err(RuntimeCacheError::InvalidArtifact(
                "morphology string count exceeds importer limit",
            ));
        };
        if usize::try_from(id.0).ok() != Some(expected_id) {
            return Err(RuntimeCacheError::InvalidArtifact(
                "duplicate morphology field",
            ));
        }
    }
    Ok(table)
}

fn read_morphology(
    reader: &mut Reader<'_>,
    table: &MorphologyTable,
) -> Result<Morphology, RuntimeCacheError> {
    let count = reader.count(MAX_MORPHOLOGY_FIELDS_PER_RECORD, "morphology field count")?;
    reader.require_minimum_items(count, 4, "morphology fields")?;
    let mut morphology = Vec::with_capacity(count);
    for _ in 0..count {
        let id = MorphologyId(reader.u32()?);
        if !table.contains(id) {
            return Err(RuntimeCacheError::InvalidArtifact(
                "morphology field references an unknown table entry",
            ));
        }
        morphology.push(id);
    }
    Ok(morphology.into_boxed_slice())
}

fn read_lexemes(
    reader: &mut Reader<'_>,
    flag_mode: FlagMode,
    morphology_table: &MorphologyTable,
) -> Result<Vec<Lexeme>, RuntimeCacheError> {
    let count = reader.count(MAX_DICTIONARY_ENTRIES, "dictionary entry count")?;
    reader.require_minimum_items(count, 8, "dictionary entries")?;
    let mut lexemes = Vec::new();
    for _ in 0..count {
        let stem = reader.string(MAX_LINE_BYTES, "lexeme stem")?;
        if stem.is_empty() {
            return Err(RuntimeCacheError::InvalidArtifact("lexeme stem is empty"));
        }
        lexemes.push(Lexeme {
            stem: Box::<str>::from(stem),
            flags: read_flags(reader, flag_mode)?,
            morphology: read_morphology(reader, morphology_table)?,
        });
    }
    Ok(lexemes)
}

fn read_replacement_rules(
    reader: &mut Reader<'_>,
) -> Result<Vec<ReplacementRule>, RuntimeCacheError> {
    let count = reader.count(MAX_REPLACEMENT_RULES, "replacement rule count")?;
    reader.require_minimum_items(count, 8, "replacement rules")?;
    let mut rules = Vec::with_capacity(count);
    for _ in 0..count {
        let from = reader.string(MAX_LINE_BYTES, "replacement source spelling")?;
        let to = reader.string(MAX_LINE_BYTES, "replacement target spelling")?;
        let at_word_start = read_boolean(reader, "replacement start marker")?;
        let at_word_end = read_boolean(reader, "replacement end marker")?;
        let Some(rule) = ReplacementRule::with_boundaries(from, to, at_word_start, at_word_end)
        else {
            return Err(RuntimeCacheError::InvalidArtifact(
                "replacement rule has empty spelling text",
            ));
        };
        rules.push(rule);
    }
    Ok(rules)
}

fn read_boolean(reader: &mut Reader<'_>, name: &'static str) -> Result<bool, RuntimeCacheError> {
    match reader.byte()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(RuntimeCacheError::InvalidArtifact(name)),
    }
}

fn read_optional_usize(
    reader: &mut Reader<'_>,
    name: &'static str,
) -> Result<Option<usize>, RuntimeCacheError> {
    let value =
        usize::try_from(reader.u64()?).map_err(|_| RuntimeCacheError::InvalidArtifact(name))?;
    Ok((value != 0).then_some(value))
}

fn read_break_patterns(reader: &mut Reader<'_>) -> Result<Vec<BreakPattern>, RuntimeCacheError> {
    let count = reader.count(MAX_BREAK_PATTERNS, "break pattern count")?;
    reader.require_minimum_items(count, 10, "break patterns")?;
    let mut patterns = Vec::with_capacity(count);
    for _ in 0..count {
        let text = reader.string(MAX_LINE_BYTES, "break pattern text")?;
        let at_start = read_boolean(reader, "invalid break start marker")?;
        let at_end = read_boolean(reader, "invalid break end marker")?;
        if text.is_empty() || (at_start && at_end) {
            return Err(RuntimeCacheError::InvalidArtifact("invalid break pattern"));
        }
        patterns.push(BreakPattern {
            text: Box::from(text),
            at_start,
            at_end,
        });
    }
    Ok(patterns)
}

fn read_compound_patterns(
    reader: &mut Reader<'_>,
    flag_mode: FlagMode,
) -> Result<Vec<CompoundPattern>, RuntimeCacheError> {
    let count = reader.count(MAX_COMPOUND_PATTERNS, "compound pattern count")?;
    reader.require_minimum_items(count, 14, "compound patterns")?;
    let mut patterns = Vec::with_capacity(count);
    for _ in 0..count {
        let ending = reader.string(MAX_LINE_BYTES, "compound pattern ending")?;
        let ending_flag = read_optional_flag(reader, flag_mode)?;
        let beginning = reader.string(MAX_LINE_BYTES, "compound pattern beginning")?;
        let beginning_flag = read_optional_flag(reader, flag_mode)?;
        let replacement = match reader.byte()? {
            0 => None,
            1 => Some(Box::<str>::from(
                reader.string(MAX_LINE_BYTES, "compound pattern replacement")?,
            )),
            _ => {
                return Err(RuntimeCacheError::InvalidArtifact(
                    "invalid compound pattern replacement marker",
                ))
            }
        };
        if ending.is_empty()
            && beginning.is_empty()
            && ending_flag.is_none()
            && beginning_flag.is_none()
        {
            return Err(RuntimeCacheError::InvalidArtifact(
                "compound pattern is empty",
            ));
        }
        patterns.push(CompoundPattern {
            ending: Box::<str>::from(ending),
            ending_flag,
            beginning: Box::<str>::from(beginning),
            beginning_flag,
            replacement,
        });
    }
    Ok(patterns)
}

fn read_compound_syllable_limit(
    reader: &mut Reader<'_>,
) -> Result<Option<CompoundSyllableLimit>, RuntimeCacheError> {
    match reader.byte()? {
        0 => Ok(None),
        1 => {
            let maximum = usize::try_from(reader.u64()?).map_err(|_| {
                RuntimeCacheError::InvalidArtifact("compound syllable limit is too large")
            })?;
            let count = reader.count(MAX_LINE_BYTES, "compound syllable vowel count")?;
            if count == 0 {
                return Err(RuntimeCacheError::InvalidArtifact(
                    "compound syllable vowel set is empty",
                ));
            }
            let mut vowels = BTreeSet::new();
            for _ in 0..count {
                let vowel = reader.character()?;
                if !vowels.insert(vowel) {
                    return Err(RuntimeCacheError::InvalidArtifact(
                        "duplicate compound syllable vowel",
                    ));
                }
            }
            Ok(Some(CompoundSyllableLimit { maximum, vowels }))
        }
        _ => Err(RuntimeCacheError::InvalidArtifact(
            "invalid compound syllable marker",
        )),
    }
}

fn read_input_conversions(
    reader: &mut Reader<'_>,
) -> Result<Vec<InputConversion>, RuntimeCacheError> {
    let count = reader.count(MAX_INPUT_CONVERSIONS, "input conversion count")?;
    reader.require_minimum_items(count, 10, "input conversions")?;
    let mut conversions = Vec::with_capacity(count);
    for _ in 0..count {
        let from = reader.string(MAX_LINE_BYTES, "input conversion source")?;
        let to = reader.string(MAX_LINE_BYTES, "input conversion target")?;
        let at_word_start = match reader.byte()? {
            0 => false,
            1 => true,
            _ => {
                return Err(RuntimeCacheError::InvalidArtifact(
                    "invalid input conversion start marker",
                ))
            }
        };
        let at_word_end = match reader.byte()? {
            0 => false,
            1 => true,
            _ => {
                return Err(RuntimeCacheError::InvalidArtifact(
                    "invalid input conversion end marker",
                ))
            }
        };
        if from.is_empty() {
            return Err(RuntimeCacheError::InvalidArtifact(
                "input conversion has an empty source string",
            ));
        }
        conversions.push(InputConversion {
            from: Box::from(from),
            to: Box::from(to),
            at_word_start,
            at_word_end,
        });
    }
    Ok(conversions)
}

fn read_rules(
    reader: &mut Reader<'_>,
    expected_kind: AffixKind,
    flag_mode: FlagMode,
    morphology_table: &MorphologyTable,
) -> Result<Vec<AffixRule>, RuntimeCacheError> {
    let count = reader.count(MAX_AFFIX_RULES, "affix rule count")?;
    reader.require_minimum_items(count, 26, "affix rules")?;
    let mut rules = Vec::new();
    for _ in 0..count {
        let id = usize::try_from(reader.u32()?)
            .map_err(|_| RuntimeCacheError::InvalidArtifact("affix rule ID is too large"))?;
        let kind = match reader.byte()? {
            0 => AffixKind::Prefix,
            1 => AffixKind::Suffix,
            _ => {
                return Err(RuntimeCacheError::InvalidArtifact(
                    "invalid affix rule kind",
                ))
            }
        };
        if kind != expected_kind {
            return Err(RuntimeCacheError::InvalidArtifact(
                "affix rule is in the wrong kind section",
            ));
        }
        let flag = reader.flag(flag_mode)?;
        let strip = Box::<str>::from(reader.string(MAX_LINE_BYTES, "affix strip")?);
        let add = Box::<str>::from(reader.string(MAX_LINE_BYTES, "affix add")?);
        let condition = read_condition(reader)?;
        let cross_product = match reader.byte()? {
            0 => false,
            1 => true,
            _ => {
                return Err(RuntimeCacheError::InvalidArtifact(
                    "invalid affix cross-product marker",
                ))
            }
        };
        rules.push(AffixRule {
            id,
            kind,
            flag,
            strip,
            add,
            condition,
            cross_product,
            continuation_flags: read_flags(reader, flag_mode)?,
            morphology: read_morphology(reader, morphology_table)?,
        });
    }
    Ok(rules)
}

fn read_special_flags(
    reader: &mut Reader<'_>,
    flag_mode: FlagMode,
) -> Result<SpecialFlags, RuntimeCacheError> {
    Ok(SpecialFlags {
        circumfix: read_optional_flag(reader, flag_mode)?,
        forbidden_word: read_optional_flag(reader, flag_mode)?,
        keep_case: read_optional_flag(reader, flag_mode)?,
        need_affix: read_optional_flag(reader, flag_mode)?,
        only_in_compound: read_optional_flag(reader, flag_mode)?,
        no_suggest: read_optional_flag(reader, flag_mode)?,
        check_sharps: match reader.byte()? {
            0 => false,
            1 => true,
            _ => {
                return Err(RuntimeCacheError::InvalidArtifact(
                    "invalid CHECKSHARPS marker",
                ))
            }
        },
    })
}

fn read_optional_flag(
    reader: &mut Reader<'_>,
    flag_mode: FlagMode,
) -> Result<Option<Flag>, RuntimeCacheError> {
    match reader.byte()? {
        0 => Ok(None),
        1 => reader.flag(flag_mode).map(Some),
        _ => Err(RuntimeCacheError::InvalidArtifact(
            "invalid optional flag marker",
        )),
    }
}

fn read_optional_string(
    reader: &mut Reader<'_>,
    name: &'static str,
) -> Result<Option<String>, RuntimeCacheError> {
    match reader.byte()? {
        0 => Ok(None),
        1 => reader.string(MAX_LINE_BYTES, name).map(Some),
        _ => Err(RuntimeCacheError::InvalidArtifact(
            "invalid optional string marker",
        )),
    }
}

fn read_character_maps(reader: &mut Reader<'_>) -> Result<Vec<String>, RuntimeCacheError> {
    let count = reader.count(MAX_CHARACTER_MAPS, "character map count")?;
    reader.require_minimum_items(count, 5, "character maps")?;
    let mut character_maps = Vec::with_capacity(count);
    for _ in 0..count {
        let group = reader.string(MAX_LINE_BYTES, "character map group")?;
        if group.chars().count() < 2 {
            return Err(RuntimeCacheError::InvalidArtifact(
                "character map group has fewer than two characters",
            ));
        }
        character_maps.push(group);
    }
    Ok(character_maps)
}

fn read_flags(reader: &mut Reader<'_>, flag_mode: FlagMode) -> Result<FlagSet, RuntimeCacheError> {
    let count = reader.count(MAX_FLAGS_PER_ENTRY, "flag count")?;
    reader.require_minimum_items(count, 5, "flags")?;
    let mut flags = Vec::with_capacity(count);
    for _ in 0..count {
        let flag = reader.flag(flag_mode)?;
        if flags.last().is_some_and(|previous| previous >= &flag) {
            return Err(RuntimeCacheError::InvalidArtifact(
                "flags are not strictly sorted",
            ));
        }
        flags.push(flag);
    }
    Ok(flags.into_boxed_slice())
}

fn read_condition(reader: &mut Reader<'_>) -> Result<Condition, RuntimeCacheError> {
    let anchored_at_start = match reader.byte()? {
        0 => false,
        1 => true,
        _ => {
            return Err(RuntimeCacheError::InvalidArtifact(
                "invalid condition start-anchor marker",
            ))
        }
    };
    let not_preceded_by = match reader.byte()? {
        0 => None,
        1 => Some(read_condition_atom(reader)?),
        _ => {
            return Err(RuntimeCacheError::InvalidArtifact(
                "invalid condition lookbehind marker",
            ))
        }
    };
    let count = reader.count(MAX_CONDITION_ATOMS, "condition atom count")?;
    reader.require_minimum_items(count, 1, "condition atoms")?;
    let mut atoms = Vec::new();
    for _ in 0..count {
        atoms.push(read_condition_atom(reader)?);
    }
    Ok(Condition {
        atoms,
        not_preceded_by,
        anchored_at_start,
    })
}

fn read_condition_atom(reader: &mut Reader<'_>) -> Result<ConditionAtom, RuntimeCacheError> {
    match reader.byte()? {
        0 => Ok(ConditionAtom::Any),
        1 => Ok(ConditionAtom::Literal(reader.character()?)),
        2 => {
            let negated = match reader.byte()? {
                0 => false,
                1 => true,
                _ => {
                    return Err(RuntimeCacheError::InvalidArtifact(
                        "invalid condition class negation marker",
                    ))
                }
            };
            let member_count = reader.count(MAX_LINE_BYTES, "condition class member count")?;
            if member_count == 0 {
                return Err(RuntimeCacheError::InvalidArtifact(
                    "condition class has no members",
                ));
            }
            reader.require_minimum_items(member_count, 4, "condition class members")?;
            let mut members = BTreeSet::new();
            for _ in 0..member_count {
                let character = reader.character()?;
                if !members.insert(character) {
                    return Err(RuntimeCacheError::InvalidArtifact(
                        "condition class has duplicate members",
                    ));
                }
            }
            Ok(ConditionAtom::Class { members, negated })
        }
        _ => Err(RuntimeCacheError::InvalidArtifact(
            "invalid condition atom kind",
        )),
    }
}

fn ensure_artifact_size(length: usize, error: DictionaryError) -> Result<(), RuntimeCacheError> {
    if length > MAX_RUNTIME_CACHE_BYTES {
        return Err(error.error("serialized cache exceeds the configured artifact limit"));
    }
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn byte(&mut self) -> Result<u8, RuntimeCacheError> {
        let Some(byte) = self.bytes.get(self.position) else {
            return Err(RuntimeCacheError::InvalidArtifact("artifact is truncated"));
        };
        self.position += 1;
        Ok(*byte)
    }

    fn u16(&mut self) -> Result<u16, RuntimeCacheError> {
        Ok(u16::from_le_bytes(self.take_array()?))
    }

    fn u32(&mut self) -> Result<u32, RuntimeCacheError> {
        Ok(u32::from_le_bytes(self.take_array()?))
    }

    fn u64(&mut self) -> Result<u64, RuntimeCacheError> {
        Ok(u64::from_le_bytes(self.take_array()?))
    }

    fn take_array<const N: usize>(&mut self) -> Result<[u8; N], RuntimeCacheError> {
        let end = self
            .position
            .checked_add(N)
            .ok_or(RuntimeCacheError::InvalidArtifact(
                "artifact offset overflows",
            ))?;
        let Some(slice) = self.bytes.get(self.position..end) else {
            return Err(RuntimeCacheError::InvalidArtifact("artifact is truncated"));
        };
        self.position = end;
        slice
            .try_into()
            .map_err(|_| RuntimeCacheError::InvalidArtifact("artifact is truncated"))
    }

    fn count(&mut self, maximum: usize, name: &'static str) -> Result<usize, RuntimeCacheError> {
        let count = usize::try_from(self.u32()?)
            .map_err(|_| RuntimeCacheError::InvalidArtifact("count is too large"))?;
        if count > maximum {
            return Err(RuntimeCacheError::InvalidArtifact(name));
        }
        Ok(count)
    }

    fn string(&mut self, maximum: usize, name: &'static str) -> Result<String, RuntimeCacheError> {
        self.borrowed_string(maximum, name).map(str::to_owned)
    }

    fn borrowed_string(
        &mut self,
        maximum: usize,
        name: &'static str,
    ) -> Result<&'a str, RuntimeCacheError> {
        let length = self.count(maximum, name)?;
        let end = self
            .position
            .checked_add(length)
            .ok_or(RuntimeCacheError::InvalidArtifact(
                "artifact offset overflows",
            ))?;
        let Some(bytes) = self.bytes.get(self.position..end) else {
            return Err(RuntimeCacheError::InvalidArtifact("artifact is truncated"));
        };
        self.position = end;
        std::str::from_utf8(bytes)
            .map_err(|_| RuntimeCacheError::InvalidArtifact("string is not valid UTF-8"))
    }

    fn flag(&mut self, flag_mode: FlagMode) -> Result<Flag, RuntimeCacheError> {
        if flag_mode == FlagMode::Numeric {
            return Ok(Flag(u64::from(self.u32()?)));
        }
        let value = self.borrowed_string(MAX_LINE_BYTES, "flag")?;
        if flag_mode.flag_count(value) != Some(1) {
            return Err(RuntimeCacheError::InvalidArtifact(
                "flag does not match the dictionary FLAG mode",
            ));
        }
        encode_text_flag(value)
            .map(Flag)
            .ok_or(RuntimeCacheError::InvalidArtifact(
                "flag contains too many Unicode scalars",
            ))
    }

    fn character(&mut self) -> Result<char, RuntimeCacheError> {
        char::from_u32(self.u32()?)
            .ok_or(RuntimeCacheError::InvalidArtifact("invalid Unicode scalar"))
    }

    fn require_minimum_items(
        &self,
        count: usize,
        minimum_bytes: usize,
        name: &'static str,
    ) -> Result<(), RuntimeCacheError> {
        let required =
            count
                .checked_mul(minimum_bytes)
                .ok_or(RuntimeCacheError::InvalidArtifact(
                    "item byte count overflows",
                ))?;
        if self.bytes.len().saturating_sub(self.position) < required {
            return Err(RuntimeCacheError::InvalidArtifact(name));
        }
        Ok(())
    }

    const fn is_empty(&self) -> bool {
        self.position == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use std::panic::catch_unwind;

    use ferrolex_core::Dictionary;
    use proptest::prelude::*;
    use sha2::Digest as _;

    use super::{
        compile_runtime_artifact, compile_runtime_cache, inspect_runtime_cache,
        is_runtime_artifact, load_runtime_artifact, load_runtime_cache, CacheSource,
        RuntimeCacheError, SourceDigests, HUNSPELL_CACHE_FORMAT_VERSION,
        HUNSPELL_CACHE_SEMANTICS_VERSION,
    };
    use crate::{import, ImportMode};

    const AFF: &str = "CIRCUMFIX C\nFORBIDDENWORD F\nNEEDAFFIX N\nONLYINCOMPOUND O\nKEEPCASE K\nCHECKSHARPS\nFULLSTRIP\nWORDCHARS -.ß\nREP 1\nREP ^teh$ the\nIGNORE \u{301}\nICONV 3\nICONV æ ae\nICONV -_ x\nICONV q 0\nOCONV 2\nOCONV ae æ\nOCONV r_ 0\nCOMPOUNDFLAG M\nCOMPOUNDBEGIN X\nCOMPOUNDMIDDLE Y\nCOMPOUNDEND Z\nCOMPOUNDMIN 2\nCOMPOUNDRULE 1\nCOMPOUNDRULE XYZ\nBREAK 1\nBREAK -\nPFX A Y 1\nPFX A 0 un/C .\nSFX B Y 1\nSFX B 0 s/C .\nSFX D N 1\nSFX D 0 ed/E .\nSFX E N 1\nSFX E 0 ly .\nSFX G N 1\nSFX G word s .\n";
    const DIC: &str =
        "11\nword/ABG\nbad/AF\nfix/DN\nroot/D\nBahn/X\nHof/Y\nStraße/ZK\nTeil/XO\nMail\naer\nfinx\n";

    fn sources() -> SourceDigests {
        SourceDigests::from_source_bytes(AFF.as_bytes(), DIC.as_bytes())
    }

    fn dictionary() -> crate::HunspellDictionary {
        import("fixture.aff", AFF, "fixture.dic", DIC, ImportMode::Strict)
            .expect("fixture imports")
            .dictionary()
            .clone()
    }

    proptest! {
        #[test]
        fn cache_round_trip_preserves_generated_queries(query in "[A-Za-z]{0,24}") {
            let original = dictionary();
            let cache = compile_runtime_cache(&original, sources()).expect("cache compiles");
            let loaded = load_runtime_cache(&cache, sources()).expect("cache loads");
            prop_assert_eq!(loaded.contains(&query), original.contains(&query));
        }

        #[test]
        fn cache_loader_never_panics_for_generated_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..1024)) {
            let outcome = catch_unwind(|| {
                let _ = load_runtime_cache(&bytes, sources());
            });
            prop_assert!(outcome.is_ok());
        }
    }

    #[test]
    fn round_trip_preserves_recognition_affecting_semantics() {
        let original = dictionary();
        let cache = compile_runtime_cache(&original, sources()).expect("cache compiles");
        let loaded = load_runtime_cache(&cache, sources()).expect("cache loads");

        for word in [
            "word",
            "unwords",
            "unword",
            "words",
            "bad",
            "unbad",
            "fix",
            "fixed",
            "fixedly",
            "rooted",
            "rootedly",
            "BahnHofStraße",
            "Bahn-Mail",
            "STRASSE",
            "Teil",
            "unknown",
        ] {
            assert_eq!(
                loaded.contains(word),
                original.contains(word),
                "word `{word}`"
            );
        }
        assert!(loaded.contains("unwords"));
        assert!(!loaded.contains("unword"));
        assert!(loaded.contains("fixedly"));
        assert!(loaded.contains("BahnHofStraße"));
        assert!(loaded.contains("Bahn-Mail"));
        assert!(loaded.contains("STRASSE"));
        assert_eq!(
            loaded.word_characters().collect::<Vec<_>>(),
            ['-', '.', 'ß']
        );
        assert_eq!(loaded.replacement_rules(), original.replacement_rules());
        assert_eq!(loaded.normalize_output("aer"), "æ");
        assert!(loaded.contains("s"));
        assert!(loaded.contains("ær"));
        assert!(loaded.contains("fin-"));
        assert!(loaded.contains("worqd"));
        assert!(!loaded.contains("Teil"));
    }

    #[test]
    fn standalone_artifact_loads_without_the_original_source_pair() {
        let original = dictionary();
        let artifact = compile_runtime_artifact(&original, sources()).expect("artifact compiles");
        let loaded = load_runtime_artifact(&artifact).expect("artifact loads standalone");

        assert!(loaded.contains("unwords"));
        assert!(loaded.contains("BahnHofStraße"));
    }

    #[test]
    fn inspection_exposes_artifact_provenance_without_loading_a_dictionary() {
        let cache = compile_runtime_cache(&dictionary(), sources()).expect("cache compiles");
        let metadata = inspect_runtime_cache(&cache).expect("metadata is readable");

        assert_eq!(metadata.format_version(), HUNSPELL_CACHE_FORMAT_VERSION);
        assert_eq!(
            metadata.semantics_version(),
            HUNSPELL_CACHE_SEMANTICS_VERSION
        );
        assert_eq!(metadata.sources(), sources());
    }

    #[test]
    fn round_trip_preserves_bounded_condition_lookbehinds() {
        let aff = "SFX A N 1\nSFX A 0 x (?<!i)[z]word\n";
        let dic = "2\nzword/A\nizword/A\n";
        let original = import(
            "condition.aff",
            aff,
            "condition.dic",
            dic,
            ImportMode::Strict,
        )
        .expect("condition fixture imports")
        .dictionary()
        .clone();
        let sources = SourceDigests::from_source_bytes(aff.as_bytes(), dic.as_bytes());
        let cache = compile_runtime_cache(&original, sources).expect("cache compiles");
        let loaded = load_runtime_cache(&cache, sources).expect("cache loads");

        assert!(loaded.contains("zwordx"));
        assert!(!loaded.contains("izwordx"));
    }

    #[test]
    fn round_trip_preserves_variation_selector_utf8_flags() {
        let aff = "FLAG UTF-8\nPFX ☎️ N 1\nPFX ☎️ 0 tele .\n";
        let dic = "1\nphone/☎️\n";
        let original = import(
            "variation-selector.aff",
            aff,
            "variation-selector.dic",
            dic,
            ImportMode::Strict,
        )
        .expect("variation-selector fixture imports")
        .dictionary()
        .clone();
        let sources = SourceDigests::from_source_bytes(aff.as_bytes(), dic.as_bytes());
        let cache = compile_runtime_cache(&original, sources).expect("cache compiles");
        let loaded = load_runtime_cache(&cache, sources).expect("cache loads");

        assert!(loaded.contains("telephone"));
    }

    #[test]
    fn round_trip_preserves_lang_case_semantics() {
        let aff = "LANG tr_TR\nKEEPCASE K\n";
        let dic = "3\ni\nışık\nAnkara/K\n";
        let original = import("lang.aff", aff, "lang.dic", dic, ImportMode::Strict)
            .expect("LANG fixture imports")
            .dictionary()
            .clone();
        let sources = SourceDigests::from_source_bytes(aff.as_bytes(), dic.as_bytes());
        let cache = compile_runtime_cache(&original, sources).expect("cache compiles");
        let loaded = load_runtime_cache(&cache, sources).expect("cache loads");

        assert!(loaded.contains("İ"));
        assert!(loaded.contains("IŞIK"));
        assert!(!loaded.contains("ANKARA"));
    }

    #[test]
    fn round_trip_preserves_independent_homonym_flags() {
        let aff = "NEEDAFFIX N\n";
        let dic = "2\nfoo/N\nfoo/S\n";
        let original = import("homonyms.aff", aff, "homonyms.dic", dic, ImportMode::Strict)
            .expect("homonym fixture imports")
            .dictionary()
            .clone();
        let sources = SourceDigests::from_source_bytes(aff.as_bytes(), dic.as_bytes());
        let cache = compile_runtime_cache(&original, sources).expect("cache compiles");
        let loaded = load_runtime_cache(&cache, sources).expect("cache loads");

        assert!(original.contains("foo"));
        assert!(loaded.contains("foo"));
    }

    #[test]
    fn compilation_is_deterministic_and_embeds_source_provenance() {
        let dictionary = dictionary();
        let first = compile_runtime_cache(&dictionary, sources()).expect("first cache compiles");
        let second = compile_runtime_cache(&dictionary, sources()).expect("second cache compiles");

        assert_eq!(first, second);
        assert_eq!(&first[..8], b"FLXHSP\0\0");
        assert!(is_runtime_artifact(&first));
        assert!(!is_runtime_artifact(b"FLEXDIC\0"));
        assert_eq!(
            u16::from_le_bytes([first[8], first[9]]),
            HUNSPELL_CACHE_FORMAT_VERSION
        );
        assert_eq!(
            u32::from_le_bytes([first[10], first[11], first[12], first[13]]),
            HUNSPELL_CACHE_SEMANTICS_VERSION
        );
        assert_eq!(&first[14..46], &sources().aff());
        assert_eq!(&first[46..78], &sources().dic());
    }

    #[test]
    fn rejects_stale_source_provenance_before_constructing_a_dictionary() {
        let cache = compile_runtime_cache(&dictionary(), sources()).expect("cache compiles");
        let stale = SourceDigests::from_source_bytes(b"other aff", DIC.as_bytes());

        assert_eq!(
            load_runtime_cache(&cache, stale).expect_err("stale cache is rejected"),
            RuntimeCacheError::SourceDigestMismatch(CacheSource::Aff)
        );
    }

    #[test]
    fn rejects_caches_from_before_counted_sections_skipped_ignored_lines() {
        const PRE_COUNTED_SECTION_IGNORE_SEMANTICS: u32 = 30;
        let mut cache = compile_runtime_cache(&dictionary(), sources()).expect("cache compiles");
        cache[10..14].copy_from_slice(&PRE_COUNTED_SECTION_IGNORE_SEMANTICS.to_le_bytes());
        rewrite_checksum(&mut cache);

        assert_eq!(
            load_runtime_cache(&cache, sources()).expect_err("stale semantics are rejected"),
            RuntimeCacheError::UnsupportedSemanticsVersion(PRE_COUNTED_SECTION_IGNORE_SEMANTICS)
        );
    }

    #[test]
    fn corruption_and_trailing_payload_are_rejected() {
        let cache = compile_runtime_cache(&dictionary(), sources()).expect("cache compiles");
        let mut corrupted = cache.clone();
        corrupted[80] ^= 1;
        assert_eq!(
            load_runtime_cache(&corrupted, sources()).expect_err("corruption is rejected"),
            RuntimeCacheError::ChecksumMismatch
        );

        let mut trailing = cache;
        let checksum = trailing.split_off(trailing.len() - 32);
        trailing.push(0);
        let trailing_checksum = sha2::Sha256::digest(trailing.as_slice());
        trailing.extend_from_slice(&trailing_checksum);
        assert_eq!(
            load_runtime_cache(&trailing, sources()).expect_err("trailing data is rejected"),
            RuntimeCacheError::InvalidArtifact("artifact has trailing payload bytes")
        );
        assert_eq!(checksum.len(), 32);
    }

    #[test]
    fn malformed_versions_and_bounded_counts_are_rejected() {
        let cache = compile_runtime_cache(&dictionary(), sources()).expect("cache compiles");
        let mut format = cache.clone();
        let unsupported_format = HUNSPELL_CACHE_FORMAT_VERSION.saturating_add(1);
        format[8..10].copy_from_slice(&unsupported_format.to_le_bytes());
        rewrite_checksum(&mut format);
        assert_eq!(
            load_runtime_cache(&format, sources()).expect_err("format is rejected"),
            RuntimeCacheError::UnsupportedFormatVersion(unsupported_format)
        );

        let mut semantics = cache.clone();
        let unsupported_semantics = HUNSPELL_CACHE_SEMANTICS_VERSION.saturating_add(1);
        semantics[10..14].copy_from_slice(&unsupported_semantics.to_le_bytes());
        rewrite_checksum(&mut semantics);
        assert_eq!(
            load_runtime_cache(&semantics, sources()).expect_err("semantics are rejected"),
            RuntimeCacheError::UnsupportedSemanticsVersion(unsupported_semantics)
        );

        let mut excessive_entries = cache;
        excessive_entries[85..89].copy_from_slice(&u32::MAX.to_le_bytes());
        rewrite_checksum(&mut excessive_entries);
        assert_eq!(
            load_runtime_cache(&excessive_entries, sources())
                .expect_err("excessive count is rejected"),
            RuntimeCacheError::InvalidArtifact("dictionary entry count")
        );
    }

    #[test]
    fn deterministic_truncation_and_mutation_corpus_never_panics() {
        let cache = compile_runtime_cache(&dictionary(), sources()).expect("cache compiles");
        let mut candidates = vec![Vec::new(), cache[..1].to_vec(), cache[..78].to_vec()];
        for length in (0..cache.len()).step_by(17) {
            candidates.push(cache[..length].to_vec());
        }
        for offset in (0..cache.len()).step_by(19) {
            let mut mutated = cache.clone();
            mutated[offset] ^= 0xff;
            candidates.push(mutated);
        }

        for (index, candidate) in candidates.iter().enumerate() {
            let outcome = catch_unwind(|| {
                let _ = load_runtime_cache(candidate, sources());
            });
            assert!(outcome.is_ok(), "malformed cache case {index} panicked");
        }
    }

    fn rewrite_checksum(bytes: &mut Vec<u8>) {
        let at = bytes.len() - 32;
        bytes.truncate(at);
        let checksum = sha2::Sha256::digest(bytes.as_slice());
        bytes.extend_from_slice(&checksum);
    }
}

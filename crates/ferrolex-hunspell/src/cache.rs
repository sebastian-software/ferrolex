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
    AffixKind, AffixRule, CompoundConfig, CompoundRule, Condition, ConditionAtom, Flag,
    HunspellDictionary, Lexeme, SpecialFlags, MAX_AFFIX_RULES, MAX_COMPOUND_RULES,
    MAX_COMPOUND_RULE_COMPONENTS, MAX_CONDITION_ATOMS, MAX_DICTIONARY_ENTRIES, MAX_FLAGS_PER_ENTRY,
    MAX_LINE_BYTES,
};

const MAGIC: [u8; 8] = *b"FLXHSP\0\0";
const CHECKSUM_BYTES: usize = 32;
const HEADER_BYTES: usize = MAGIC.len() + 2 + 4 + (CHECKSUM_BYTES * 2);
const MAX_RUNTIME_CACHE_BYTES: usize = 128 * 1024 * 1024;

/// The on-disk layout version for a Hunspell runtime cache.
pub const HUNSPELL_CACHE_FORMAT_VERSION: u16 = 1;

/// The recognition semantics encoded by a Hunspell runtime cache.
///
/// This changes whenever the runtime's interpretation of any serialized field
/// changes. A cache with another semantics version is always rebuilt.
pub const HUNSPELL_CACHE_SEMANTICS_VERSION: u32 = 4;

/// SHA-256 provenance of the exact raw `.aff` and `.dic` source bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceDigests {
    aff: [u8; CHECKSUM_BYTES],
    dic: [u8; CHECKSUM_BYTES],
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
    write_lexemes(&mut output, &dictionary.lexemes)?;
    write_rules(&mut output, &dictionary.prefixes)?;
    write_rules(&mut output, &dictionary.suffixes)?;
    write_special_flags(&mut output, &dictionary.special_flags)?;
    write_optional_flag(&mut output, dictionary.compound.flag.as_ref())?;
    write_optional_flag(&mut output, dictionary.compound.begin.as_ref())?;
    write_optional_flag(&mut output, dictionary.compound.middle.as_ref())?;
    write_optional_flag(&mut output, dictionary.compound.end.as_ref())?;
    write_u64(
        &mut output,
        u64::try_from(dictionary.compound.minimum_length)
            .map_err(|_| RuntimeCacheError::InvalidDictionary("compound minimum is too large"))?,
    );
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
            rule.flags.len(),
            "compound rule component count",
        )?;
        for flag in &rule.flags {
            write_flag(&mut output, flag)?;
        }
    }
    output.extend_from_slice(&Sha256::digest(&output));
    Ok(output)
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

    let lexemes = read_lexemes(&mut reader)?;
    let prefixes = read_rules(&mut reader, AffixKind::Prefix)?;
    let suffixes = read_rules(&mut reader, AffixKind::Suffix)?;
    let special_flags = read_special_flags(&mut reader)?;
    let flag = read_optional_flag(&mut reader)?;
    let begin = read_optional_flag(&mut reader)?;
    let middle = read_optional_flag(&mut reader)?;
    let end = read_optional_flag(&mut reader)?;
    let minimum_length = usize::try_from(reader.u64()?)
        .map_err(|_| RuntimeCacheError::InvalidArtifact("compound minimum is too large"))?;
    let rule_count = reader.count(MAX_COMPOUND_RULES, "compound rule count")?;
    let mut rules = Vec::with_capacity(rule_count);
    for _ in 0..rule_count {
        let component_count = reader.count(
            MAX_COMPOUND_RULE_COMPONENTS,
            "compound rule component count",
        )?;
        if component_count < 2 {
            return Err(RuntimeCacheError::InvalidArtifact(
                "compound rule has fewer than two components",
            ));
        }
        reader.require_minimum_items(component_count, 4, "compound rule components")?;
        let mut flags = Vec::with_capacity(component_count);
        for _ in 0..component_count {
            flags.push(reader.flag()?);
        }
        rules.push(CompoundRule { flags });
    }
    let compound = CompoundConfig {
        flag,
        begin,
        middle,
        end,
        minimum_length,
        rules,
    };
    if !reader.is_empty() {
        return Err(RuntimeCacheError::InvalidArtifact(
            "artifact has trailing payload bytes",
        ));
    }

    let stems = lexemes
        .iter()
        .map(|lexeme| (lexeme.stem.clone(), lexeme.flags.clone()))
        .collect();
    let dictionary =
        HunspellDictionary::from_parts(stems, lexemes, prefixes, suffixes, special_flags, compound);
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

fn validate_dictionary(
    dictionary: &HunspellDictionary,
    error: DictionaryError,
) -> Result<(), RuntimeCacheError> {
    if dictionary.lexemes.len() > MAX_DICTIONARY_ENTRIES {
        return Err(error.error("dictionary entry count exceeds importer limit"));
    }
    if dictionary.stems.len() != dictionary.lexemes.len() {
        return Err(error.error("stem index does not match lexeme count"));
    }
    let mut previous_stem = None;
    for lexeme in &dictionary.lexemes {
        if lexeme.stem.is_empty() || lexeme.stem.len() > MAX_LINE_BYTES {
            return Err(error.error("lexeme stem has an invalid byte length"));
        }
        if previous_stem.is_some_and(|previous| previous >= lexeme.stem.as_ref()) {
            return Err(error.error("lexemes are not in strictly sorted stem order"));
        }
        previous_stem = Some(lexeme.stem.as_ref());
        validate_flags(&lexeme.flags, error)?;
    }
    for ((stem, flags), lexeme) in dictionary.stems.iter().zip(&dictionary.lexemes) {
        if lexeme.stem != *stem {
            return Err(error.error("stem index contains a missing lexeme"));
        }
        if lexeme.flags != *flags {
            return Err(error.error("stem index flags do not match lexeme flags"));
        }
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
        validate_rule(rule, error)?;
    }
    if rule_ids.len() != rule_count {
        return Err(error.error("affix rule IDs are not a unique dense range"));
    }
    validate_optional_flag(dictionary.special_flags.circumfix.as_ref(), error)?;
    validate_optional_flag(dictionary.special_flags.forbidden_word.as_ref(), error)?;
    validate_optional_flag(dictionary.special_flags.keep_case.as_ref(), error)?;
    validate_optional_flag(dictionary.special_flags.need_affix.as_ref(), error)?;
    validate_optional_flag(dictionary.special_flags.only_in_compound.as_ref(), error)?;
    validate_optional_flag(dictionary.compound.flag.as_ref(), error)?;
    validate_optional_flag(dictionary.compound.begin.as_ref(), error)?;
    validate_optional_flag(dictionary.compound.middle.as_ref(), error)?;
    validate_optional_flag(dictionary.compound.end.as_ref(), error)?;
    if dictionary.compound.minimum_length == 0 {
        return Err(error.error("compound minimum must be greater than zero"));
    }
    if dictionary.compound.rules.len() > MAX_COMPOUND_RULES {
        return Err(error.error("compound rule count exceeds importer limit"));
    }
    for rule in &dictionary.compound.rules {
        if !(2..=MAX_COMPOUND_RULE_COMPONENTS).contains(&rule.flags.len()) {
            return Err(error.error("compound rule has an invalid component count"));
        }
        for flag in &rule.flags {
            validate_flag(flag, error)?;
        }
    }
    Ok(())
}

fn validate_rule(rule: &AffixRule, error: DictionaryError) -> Result<(), RuntimeCacheError> {
    validate_flag(&rule.flag, error)?;
    if rule.strip.len() > MAX_LINE_BYTES || rule.add.len() > MAX_LINE_BYTES {
        return Err(error.error("affix rule text exceeds importer line limit"));
    }
    validate_flags(&rule.continuation_flags, error)?;
    if rule.condition.atoms.len() > MAX_CONDITION_ATOMS {
        return Err(error.error("affix condition exceeds importer atom limit"));
    }
    for atom in &rule.condition.atoms {
        if let ConditionAtom::Class { members, .. } = atom {
            if members.is_empty() || members.len() > MAX_LINE_BYTES {
                return Err(error.error("affix condition class has an invalid member count"));
            }
        }
    }
    Ok(())
}

fn validate_optional_flag(
    flag: Option<&Flag>,
    error: DictionaryError,
) -> Result<(), RuntimeCacheError> {
    if let Some(flag) = flag {
        validate_flag(flag, error)?;
    }
    Ok(())
}

fn validate_flags(flags: &BTreeSet<Flag>, error: DictionaryError) -> Result<(), RuntimeCacheError> {
    if flags.len() > MAX_FLAGS_PER_ENTRY {
        return Err(error.error("flag count exceeds importer limit"));
    }
    for flag in flags {
        validate_flag(flag, error)?;
    }
    Ok(())
}

fn validate_flag(flag: &Flag, error: DictionaryError) -> Result<(), RuntimeCacheError> {
    if flag.0.chars().count() != 1 {
        return Err(error.error("flag is not one Unicode scalar"));
    }
    Ok(())
}

fn write_lexemes(output: &mut Vec<u8>, lexemes: &[Lexeme]) -> Result<(), RuntimeCacheError> {
    write_count(output, lexemes.len(), "dictionary entry count")?;
    for lexeme in lexemes {
        write_string(output, &lexeme.stem, "lexeme stem")?;
        write_flags(output, &lexeme.flags)?;
    }
    Ok(())
}

fn write_rules(output: &mut Vec<u8>, rules: &[AffixRule]) -> Result<(), RuntimeCacheError> {
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
        write_flag(output, &rule.flag)?;
        write_string(output, &rule.strip, "affix strip")?;
        write_string(output, &rule.add, "affix add")?;
        write_condition(output, &rule.condition)?;
        output.push(u8::from(rule.cross_product));
        write_flags(output, &rule.continuation_flags)?;
    }
    Ok(())
}

fn write_special_flags(
    output: &mut Vec<u8>,
    special_flags: &SpecialFlags,
) -> Result<(), RuntimeCacheError> {
    write_optional_flag(output, special_flags.circumfix.as_ref())?;
    write_optional_flag(output, special_flags.forbidden_word.as_ref())?;
    write_optional_flag(output, special_flags.keep_case.as_ref())?;
    write_optional_flag(output, special_flags.need_affix.as_ref())?;
    write_optional_flag(output, special_flags.only_in_compound.as_ref())
}

fn write_optional_flag(output: &mut Vec<u8>, flag: Option<&Flag>) -> Result<(), RuntimeCacheError> {
    if let Some(flag) = flag {
        output.push(1);
        write_flag(output, flag)
    } else {
        output.push(0);
        Ok(())
    }
}

fn write_flags(output: &mut Vec<u8>, flags: &BTreeSet<Flag>) -> Result<(), RuntimeCacheError> {
    write_count(output, flags.len(), "flag count")?;
    for flag in flags {
        write_flag(output, flag)?;
    }
    Ok(())
}

fn write_flag(output: &mut Vec<u8>, flag: &Flag) -> Result<(), RuntimeCacheError> {
    let mut characters = flag.0.chars();
    let Some(character) = characters.next() else {
        return Err(RuntimeCacheError::InvalidDictionary("flag is empty"));
    };
    if characters.next().is_some() {
        return Err(RuntimeCacheError::InvalidDictionary(
            "flag is not one Unicode scalar",
        ));
    }
    write_u32(output, u32::from(character));
    Ok(())
}

fn write_condition(output: &mut Vec<u8>, condition: &Condition) -> Result<(), RuntimeCacheError> {
    write_count(output, condition.atoms.len(), "condition atom count")?;
    for atom in &condition.atoms {
        match atom {
            ConditionAtom::Any => output.push(0),
            ConditionAtom::Literal(character) => {
                output.push(1);
                write_u32(output, u32::from(*character));
            }
            ConditionAtom::Class { members, negated } => {
                output.push(2);
                output.push(u8::from(*negated));
                write_count(output, members.len(), "condition class member count")?;
                for character in members {
                    write_u32(output, u32::from(*character));
                }
            }
        }
    }
    Ok(())
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

fn write_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn write_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn read_lexemes(reader: &mut Reader<'_>) -> Result<Vec<Lexeme>, RuntimeCacheError> {
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
            flags: read_flags(reader)?,
        });
    }
    Ok(lexemes)
}

fn read_rules(
    reader: &mut Reader<'_>,
    expected_kind: AffixKind,
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
        let flag = reader.flag()?;
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
            continuation_flags: read_flags(reader)?,
        });
    }
    Ok(rules)
}

fn read_special_flags(reader: &mut Reader<'_>) -> Result<SpecialFlags, RuntimeCacheError> {
    Ok(SpecialFlags {
        circumfix: read_optional_flag(reader)?,
        forbidden_word: read_optional_flag(reader)?,
        keep_case: read_optional_flag(reader)?,
        need_affix: read_optional_flag(reader)?,
        only_in_compound: read_optional_flag(reader)?,
    })
}

fn read_optional_flag(reader: &mut Reader<'_>) -> Result<Option<Flag>, RuntimeCacheError> {
    match reader.byte()? {
        0 => Ok(None),
        1 => reader.flag().map(Some),
        _ => Err(RuntimeCacheError::InvalidArtifact(
            "invalid optional flag marker",
        )),
    }
}

fn read_flags(reader: &mut Reader<'_>) -> Result<BTreeSet<Flag>, RuntimeCacheError> {
    let count = reader.count(MAX_FLAGS_PER_ENTRY, "flag count")?;
    reader.require_minimum_items(count, 4, "flags")?;
    let mut flags = BTreeSet::new();
    for _ in 0..count {
        let flag = reader.flag()?;
        if !flags.insert(flag) {
            return Err(RuntimeCacheError::InvalidArtifact("duplicate flag"));
        }
    }
    Ok(flags)
}

fn read_condition(reader: &mut Reader<'_>) -> Result<Condition, RuntimeCacheError> {
    let count = reader.count(MAX_CONDITION_ATOMS, "condition atom count")?;
    reader.require_minimum_items(count, 1, "condition atoms")?;
    let mut atoms = Vec::new();
    for _ in 0..count {
        let atom = match reader.byte()? {
            0 => ConditionAtom::Any,
            1 => ConditionAtom::Literal(reader.character()?),
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
                ConditionAtom::Class { members, negated }
            }
            _ => {
                return Err(RuntimeCacheError::InvalidArtifact(
                    "invalid condition atom kind",
                ))
            }
        };
        atoms.push(atom);
    }
    Ok(Condition { atoms })
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
            .map(str::to_owned)
            .map_err(|_| RuntimeCacheError::InvalidArtifact("string is not valid UTF-8"))
    }

    fn flag(&mut self) -> Result<Flag, RuntimeCacheError> {
        Ok(Flag(Box::<str>::from(self.character()?.to_string())))
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
    use sha2::Digest as _;

    use super::{
        compile_runtime_cache, load_runtime_cache, CacheSource, RuntimeCacheError, SourceDigests,
        HUNSPELL_CACHE_FORMAT_VERSION, HUNSPELL_CACHE_SEMANTICS_VERSION,
    };
    use crate::{import, ImportMode};

    const AFF: &str = "CIRCUMFIX C\nFORBIDDENWORD F\nNEEDAFFIX N\nONLYINCOMPOUND O\nCOMPOUNDFLAG M\nCOMPOUNDBEGIN X\nCOMPOUNDMIDDLE Y\nCOMPOUNDEND Z\nCOMPOUNDMIN 2\nCOMPOUNDRULE 1\nCOMPOUNDRULE XYZ\nPFX A Y 1\nPFX A 0 un/C .\nSFX B Y 1\nSFX B 0 s/C .\nSFX D N 1\nSFX D 0 ed/E .\nSFX E N 1\nSFX E 0 ly .\n";
    const DIC: &str = "8\nword/AB\nbad/AF\nfix/DN\nroot/D\nBahn/X\nHof/Y\nStraße/Z\nTeil/XO\n";

    fn sources() -> SourceDigests {
        SourceDigests::from_source_bytes(AFF.as_bytes(), DIC.as_bytes())
    }

    fn dictionary() -> crate::HunspellDictionary {
        import("fixture.aff", AFF, "fixture.dic", DIC, ImportMode::Strict)
            .expect("fixture imports")
            .dictionary()
            .clone()
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
        assert!(!loaded.contains("Teil"));
    }

    #[test]
    fn compilation_is_deterministic_and_embeds_source_provenance() {
        let dictionary = dictionary();
        let first = compile_runtime_cache(&dictionary, sources()).expect("first cache compiles");
        let second = compile_runtime_cache(&dictionary, sources()).expect("second cache compiles");

        assert_eq!(first, second);
        assert_eq!(&first[..8], b"FLXHSP\0\0");
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
        format[8..10].copy_from_slice(&2_u16.to_le_bytes());
        rewrite_checksum(&mut format);
        assert_eq!(
            load_runtime_cache(&format, sources()).expect_err("format is rejected"),
            RuntimeCacheError::UnsupportedFormatVersion(2)
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
        excessive_entries[78..82].copy_from_slice(&u32::MAX.to_le_bytes());
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

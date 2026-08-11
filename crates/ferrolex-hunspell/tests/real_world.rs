//! Opt-in compatibility checks against locally supplied third-party data.
//!
//! No dictionary content is checked into ferrolex. See
//! `docs/compatibility-fixtures.md` for the source, licensing, and checksum
//! review procedure.

use std::collections::BTreeSet;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use encoding_rs::ISO_8859_2;
use ferrolex_core::Dictionary;
use ferrolex_hunspell::{
    compile_runtime_cache, import_bytes, import_bytes_with_encodings, load_runtime_cache,
    ByteEncoding, ByteImportEncodings, HunspellDictionary, ImportMode, SourceDigests,
};

const MANIFEST: &str = include_str!("real_world/manifest.tsv");

#[derive(Debug, Eq, PartialEq)]
struct Fixture {
    id: String,
    locale: String,
    aff_path: PathBuf,
    dic_path: PathBuf,
    aff_bytes: usize,
    dic_bytes: usize,
    aff_sha256: String,
    dic_sha256: String,
    aff_decode: Decode,
    dic_decode: Decode,
    import_expectation: ImportExpectation,
    accepted: Vec<Probe>,
    rejected: Vec<String>,
    source: String,
    license: String,
    license_evidence: String,
}

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Probe {
    category: ProbeCategory,
    word: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ProbeCategory {
    Stem,
    Affixed,
    Compound,
    SentenceInitial,
    AllCaps,
    KeepCase,
}

impl ProbeCategory {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "stem" => Ok(Self::Stem),
            "affixed" => Ok(Self::Affixed),
            "compound" => Ok(Self::Compound),
            "sentence-initial" => Ok(Self::SentenceInitial),
            "all-caps" => Ok(Self::AllCaps),
            "keepcase" => Ok(Self::KeepCase),
            _ => Err(format!("unknown probe category `{value}`")),
        }
    }

    const fn documentation(self) -> &'static str {
        match self {
            Self::Stem => "docs/compatibility-fixtures.md",
            Self::Affixed => "docs/affix-semantics.md",
            Self::Compound => "docs/compound-semantics.md",
            Self::SentenceInitial | Self::AllCaps | Self::KeepCase => "docs/hunspell-format.md",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Decode {
    Utf8,
    Latin1,
    Latin2,
    Utf8WithLatin2Fallback,
    NotUtf8,
}

impl Decode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "utf-8" => Ok(Self::Utf8),
            "iso-8859-1" => Ok(Self::Latin1),
            "iso-8859-2" => Ok(Self::Latin2),
            "utf-8+iso-8859-2-fallback" => Ok(Self::Utf8WithLatin2Fallback),
            "not-utf-8" => Ok(Self::NotUtf8),
            _ => Err(format!("unknown decoding mode `{value}`")),
        }
    }

    fn decode(self, bytes: &[u8]) -> Result<String, String> {
        match self {
            Self::Utf8 => String::from_utf8(bytes.to_vec()).map_err(|error| {
                format!(
                    "expected UTF-8 but found invalid byte {}",
                    error.utf8_error().valid_up_to()
                )
            }),
            // ISO-8859-1 has a direct one-code-point-per-byte mapping, so this
            // reporting conversion is lossless.
            Self::Latin1 => Ok(bytes.iter().map(|byte| char::from(*byte)).collect()),
            Self::Latin2 => {
                let (text, _, had_replacements) = ISO_8859_2.decode(bytes);
                (!had_replacements)
                    .then(|| text.into_owned())
                    .ok_or_else(|| "ISO-8859-2 decoding replaced invalid input".to_owned())
            }
            Self::Utf8WithLatin2Fallback => Ok(decode_utf8_with_latin2_fallback(bytes)),
            Self::NotUtf8 => Err("fixture is intentionally recorded as non-UTF-8".to_owned()),
        }
    }
}

fn decode_utf8_with_latin2_fallback(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    let mut remaining = bytes;

    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                text.push_str(valid);
                break;
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                text.push_str(
                    std::str::from_utf8(&remaining[..valid_up_to])
                        .expect("UTF-8 error valid prefix is valid UTF-8"),
                );
                let invalid_len = error.error_len().unwrap_or(remaining.len() - valid_up_to);
                for byte in &remaining[valid_up_to..valid_up_to + invalid_len] {
                    if *byte == 0x85 {
                        text.push('\n');
                    } else {
                        let encoded = [*byte];
                        let (decoded, _, had_replacements) = ISO_8859_2.decode(&encoded);
                        assert!(
                            !had_replacements,
                            "one ISO-8859-2 byte decodes without replacement"
                        );
                        text.push_str(&decoded);
                    }
                }
                remaining = &remaining[valid_up_to + invalid_len..];
            }
        }
    }

    text.replace('\u{0085}', "\n")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ImportExpectation {
    Lenient,
    Strict,
    Blocked,
}

impl ImportExpectation {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "lenient" => Ok(Self::Lenient),
            "strict" => Ok(Self::Strict),
            "blocked" => Ok(Self::Blocked),
            _ => Err(format!("unknown import expectation `{value}`")),
        }
    }

    const fn mode(self) -> Option<ImportMode> {
        match self {
            Self::Lenient => Some(ImportMode::Lenient),
            Self::Strict => Some(ImportMode::Strict),
            Self::Blocked => None,
        }
    }
}

#[test]
fn real_world_manifest_is_complete_and_source_pinned() {
    let fixtures = parse_manifest(MANIFEST).expect("checked-in compatibility manifest is valid");
    assert!(
        fixtures.len() >= 2,
        "the suite needs independent reference cases"
    );

    let mut identifiers = BTreeSet::new();
    for fixture in fixtures {
        assert!(
            identifiers.insert(fixture.id.clone()),
            "fixture IDs are unique"
        );
        assert!(!fixture.locale.is_empty());
        assert!(!fixture.aff_path.is_absolute());
        assert!(!fixture.dic_path.is_absolute());
        assert!(
            fixture.source.contains("/+/") || fixture.source.contains("/tree/"),
            "source is revision-pinned"
        );
        assert!(fixture.license_evidence.starts_with("https://"));
        assert_eq!(fixture.aff_sha256.len(), 64, "SHA-256 is recorded");
        assert_eq!(fixture.dic_sha256.len(), 64, "SHA-256 is recorded");
        assert!(!fixture.license.is_empty());
        assert!(!fixture.accepted.is_empty());
        for probe in &fixture.accepted {
            assert!(!probe.word.is_empty(), "positive probe words are non-empty");
        }
        assert!(!fixture.rejected.is_empty());
        if fixture.import_expectation == ImportExpectation::Blocked {
            assert_eq!(fixture.aff_decode, Decode::NotUtf8);
        }
    }
}

#[test]
fn local_real_world_fixtures_report_format_and_recognition() {
    let Ok(root) = env::var("FERROLEX_COMPAT_FIXTURES") else {
        eprintln!(
            "skipping real-world compatibility fixtures; set FERROLEX_COMPAT_FIXTURES to the reviewed fixture root"
        );
        return;
    };
    let root = Path::new(&root);
    let fixtures = parse_manifest(MANIFEST).expect("checked-in compatibility manifest is valid");
    let mut report = String::from("ferrolex real-world compatibility report\n");

    for fixture in &fixtures {
        run_fixture(root, fixture, &mut report);
    }

    eprintln!("{report}");
}

fn run_fixture(root: &Path, fixture: &Fixture, report: &mut String) {
    let (aff_path, dic_path, aff_bytes, dic_bytes) = read_verified_fixture(root, fixture);
    writeln!(report, "fixture={} locale={}", fixture.id, fixture.locale)
        .expect("writing to String does not fail");
    writeln!(report, "  source={}", fixture.source).expect("writing to String does not fail");
    writeln!(report, "  license={}", fixture.license).expect("writing to String does not fail");
    writeln!(
        report,
        "  sha256.aff={} sha256.dic={} (verified)",
        fixture.aff_sha256, fixture.dic_sha256
    )
    .expect("writing to String does not fail");

    let aff_text = match fixture.aff_decode.decode(&aff_bytes) {
        Ok(text) => text,
        Err(reason) => {
            writeln!(report, "  format=blocked ({reason})")
                .expect("writing to String does not fail");
            write_blocked_scorecard(fixture, "aff-decoding");
            return;
        }
    };
    let dic_text = match fixture.dic_decode.decode(&dic_bytes) {
        Ok(text) => text,
        Err(reason) => {
            writeln!(report, "  format=blocked ({reason})")
                .expect("writing to String does not fail");
            write_blocked_scorecard(fixture, "dic-decoding");
            return;
        }
    };
    let Some(mode) = fixture.import_expectation.mode() else {
        writeln!(report, "  format=blocked (manifested format boundary)")
            .expect("writing to String does not fail");
        write_blocked_scorecard(fixture, "manifested-format-boundary");
        return;
    };
    let directives = directives_in(&aff_text);
    assert_strict_probe_coverage(fixture, &directives);
    let imported =
        import_fixture_bytes(fixture, &aff_path, &aff_bytes, &dic_path, &dic_bytes, mode)
            .unwrap_or_else(|error| panic!("{} import unexpectedly failed: {error}", fixture.id));
    let diagnostics = imported
        .diagnostics()
        .iter()
        .map(ferrolex_hunspell::Diagnostic::directive)
        .collect::<BTreeSet<_>>();
    writeln!(
        report,
        "  format=imported directives={} recognition_diagnostics={}",
        join(&directives),
        join(&diagnostics)
    )
    .expect("writing to String does not fail");

    let sources = SourceDigests::from_source_bytes(&aff_bytes, &dic_bytes);
    let cache = compile_runtime_cache(imported.dictionary(), sources)
        .unwrap_or_else(|error| panic!("{} cache compilation failed: {error}", fixture.id));
    let dictionary = load_runtime_cache(&cache, sources)
        .unwrap_or_else(|error| panic!("{} cache loading failed: {error}", fixture.id));
    writeln!(report, "  runtime_cache=compiled-and-validated")
        .expect("writing to String does not fail");

    for probe in &fixture.accepted {
        assert!(
            dictionary.contains(&probe.word),
            "{} must recognize {} probe `{}`; see {}",
            fixture.id,
            probe_category_name(probe.category),
            probe.word,
            probe.category.documentation()
        );
    }
    for word in &fixture.rejected {
        assert!(
            !dictionary.contains(word),
            "{} must reject recorded negative probe `{word}`",
            fixture.id
        );
    }
    writeln!(
        report,
        "  recognition=accepted:{} rejected:{}",
        join(fixture.accepted.iter().map(|probe| probe.word.as_str())),
        join(&fixture.rejected)
    )
    .expect("writing to String does not fail");
    write_scorecard(fixture, &dictionary, &aff_path, report);
    report_keepcase_coverage(fixture, &aff_text, &dic_text, report);
}

fn import_fixture_bytes(
    fixture: &Fixture,
    aff_path: &Path,
    aff_bytes: &[u8],
    dic_path: &Path,
    dic_bytes: &[u8],
    mode: ImportMode,
) -> Result<ferrolex_hunspell::ImportResult, ferrolex_hunspell::ImportError> {
    let aff_source = aff_path.display().to_string();
    let dic_source = dic_path.display().to_string();
    match fixture.aff_decode {
        Decode::Utf8WithLatin2Fallback => {
            assert_eq!(
                fixture.dic_decode,
                Decode::Utf8,
                "the UTF-8/ISO-8859-2 AFF fallback only supports a UTF-8 DIC"
            );
            import_bytes_with_encodings(
                &aff_source,
                aff_bytes,
                &dic_source,
                dic_bytes,
                ByteImportEncodings::new(
                    ByteEncoding::Utf8WithIso8859_2Fallback,
                    ByteEncoding::Utf8,
                ),
                mode,
            )
        }
        _ => import_bytes(&aff_source, aff_bytes, &dic_source, dic_bytes, mode),
    }
}

fn assert_strict_probe_coverage(fixture: &Fixture, directives: &BTreeSet<&str>) {
    if fixture.import_expectation != ImportExpectation::Strict {
        return;
    }

    let categories = fixture
        .accepted
        .iter()
        .map(|probe| probe.category)
        .collect::<BTreeSet<_>>();
    for category in [ProbeCategory::Stem, ProbeCategory::Affixed] {
        assert!(
            categories.contains(&category),
            "{} strict fixture needs a {} probe; see {}",
            fixture.id,
            probe_category_name(category),
            category.documentation()
        );
    }
    if directives
        .iter()
        .any(|directive| directive.starts_with("COMPOUND"))
    {
        assert!(
            categories.contains(&ProbeCategory::Compound),
            "{} declares compound directives and needs a compound probe; see {}",
            fixture.id,
            ProbeCategory::Compound.documentation()
        );
    }
    if fixture
        .accepted
        .iter()
        .any(|probe| probe.word.chars().any(char::is_lowercase))
    {
        for category in [ProbeCategory::SentenceInitial, ProbeCategory::AllCaps] {
            assert!(
                categories.contains(&category),
                "{} has cased probes and needs a {} probe; see {}",
                fixture.id,
                probe_category_name(category),
                category.documentation()
            );
        }
    }
}

fn report_keepcase_coverage(
    fixture: &Fixture,
    aff_text: &str,
    dic_text: &str,
    report: &mut String,
) {
    let Some(flag) = keepcase_flag(aff_text) else {
        return;
    };
    let is_used = dic_text.lines().skip(1).any(|line| {
        line.rsplit_once('/')
            .is_some_and(|(_, flags)| flags.chars().any(|candidate| candidate == flag))
    });
    let categories = fixture
        .accepted
        .iter()
        .map(|probe| probe.category)
        .collect::<BTreeSet<_>>();
    let has_probe = categories.contains(&ProbeCategory::KeepCase);
    if fixture.import_expectation == ImportExpectation::Strict {
        assert!(
            !is_used || has_probe,
            "{} dictionary uses KEEPCASE `{flag}` and needs a keepcase probe; see {}",
            fixture.id,
            ProbeCategory::KeepCase.documentation()
        );
    }
    writeln!(
        report,
        "  keepcase={} ({})",
        match (is_used, has_probe) {
            (true, true) => "probe-recorded",
            (true, false) => "dictionary-uses-flag",
            (false, _) => "not-used-by-dictionary",
        },
        flag
    )
    .expect("writing to String does not fail");
}

fn keepcase_flag(aff_text: &str) -> Option<char> {
    aff_text.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next() == Some("KEEPCASE"))
            .then(|| fields.next())
            .flatten()
            .and_then(|flag| flag.chars().next())
    })
}

const fn probe_category_name(category: ProbeCategory) -> &'static str {
    match category {
        ProbeCategory::Stem => "stem",
        ProbeCategory::Affixed => "affixed",
        ProbeCategory::Compound => "compound",
        ProbeCategory::SentenceInitial => "sentence-initial",
        ProbeCategory::AllCaps => "all-caps",
        ProbeCategory::KeepCase => "keepcase",
    }
}

fn write_blocked_scorecard(fixture: &Fixture, reason: &str) {
    append_scorecard(&format!("{}\tblocked\t0\t0\t0\t{reason}\n", fixture.locale));
}

fn write_scorecard(
    fixture: &Fixture,
    dictionary: &HunspellDictionary,
    aff_path: &Path,
    report: &mut String,
) {
    let Ok(oracle) = env::var("FERROLEX_COMPAT_ORACLE") else {
        return;
    };
    assert!(
        oracle == "hunspell",
        "unknown compatibility oracle `{oracle}`; expected `hunspell`"
    );

    let mut corpus = dictionary
        .stems()
        .filter(|word| word.chars().all(char::is_alphabetic))
        .take(128)
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    corpus.extend(fixture.accepted.iter().map(|probe| probe.word.clone()));
    corpus.extend(fixture.rejected.iter().cloned());
    corpus.extend(
        fixture
            .accepted
            .iter()
            .map(|probe| format!("{}ferrolexcompat", probe.word)),
    );
    let corpus = corpus
        .into_iter()
        .filter(|word| is_hunspell_input(word))
        .collect::<Vec<_>>();
    let oracle_decisions = hunspell_decisions(aff_path, &corpus);
    let agreements = corpus
        .iter()
        .zip(oracle_decisions)
        .filter(|(word, accepted)| dictionary.contains(word) == *accepted)
        .count();
    let total = corpus.len();
    let disagreements = total - agreements;
    writeln!(
        report,
        "  scorecard=oracle:hunspell corpus:{total} agreements:{agreements} disagreements:{disagreements}"
    )
    .expect("writing to String does not fail");
    append_scorecard(&format!(
        "{}\tmeasured\t{total}\t{agreements}\t{disagreements}\thunspell\n",
        fixture.locale
    ));
}

fn append_scorecard(row: &str) {
    let Ok(path) = env::var("FERROLEX_COMPAT_SCORECARD") else {
        return;
    };
    let path = Path::new(&path);
    let needs_header = fs::metadata(path).map_or(true, |metadata| metadata.len() == 0);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap_or_else(|error| panic!("could not create scorecard {}: {error}", path.display()));
    if needs_header {
        file.write_all(b"locale\tstatus\tcorpus\tagreements\tdisagreements\toracle-or-reason\n")
            .expect("scorecard header is writable");
    }
    file.write_all(row.as_bytes())
        .expect("scorecard row is writable");
}

fn hunspell_decisions(aff_path: &Path, corpus: &[String]) -> Vec<bool> {
    let stem = aff_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_else(|| panic!("invalid affix filename {}", aff_path.display()));
    let dictionary = aff_path.with_file_name(stem);
    let mut command = Command::new("hunspell")
        .args(["-a", "-d"])
        .arg(&dictionary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("could not start hunspell oracle: {error}"));
    command
        .stdin
        .as_mut()
        .expect("hunspell stdin is piped")
        .write_all(format!("{}\n", corpus.join("\n")).as_bytes())
        .expect("hunspell oracle input is writable");
    let output = command
        .wait_with_output()
        .expect("hunspell oracle process completes");
    assert!(
        output.status.success(),
        "hunspell oracle failed for {}: {}",
        aff_path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    parse_hunspell_output(&String::from_utf8_lossy(&output.stdout), corpus.len())
}

fn parse_hunspell_output(output: &str, expected: usize) -> Vec<bool> {
    let decisions = output
        .lines()
        .skip(1)
        .filter(|line| !line.is_empty())
        .map(|line| matches!(line.as_bytes().first(), Some(b'*' | b'+' | b'-')))
        .collect::<Vec<_>>();
    assert_eq!(
        decisions.len(),
        expected,
        "hunspell emitted an unexpected number of decision lines: {output}"
    );
    decisions
}

fn is_hunspell_input(word: &str) -> bool {
    !matches!(
        word.as_bytes().first(),
        None | Some(b'#' | b'!' | b'@' | b'?' | b'^')
    )
}

#[test]
fn hunspell_oracle_output_ignores_separator_lines() {
    let output = "@(#) International Ispell Version\n*\n\n& misspelled 1 0: suggested\n\n";

    assert_eq!(parse_hunspell_output(output, 2), vec![true, false]);
}

#[test]
fn hunspell_oracle_skips_protocol_control_words() {
    for word in ["# comment", "!command", "@command", "?query", "^query", ""] {
        assert!(!is_hunspell_input(word));
    }
    assert!(is_hunspell_input("ferrolex"));
}

fn read_verified_fixture(root: &Path, fixture: &Fixture) -> (PathBuf, PathBuf, Vec<u8>, Vec<u8>) {
    let aff_path = root.join(&fixture.id).join(&fixture.aff_path);
    let dic_path = root.join(&fixture.id).join(&fixture.dic_path);
    let aff_bytes = fs::read(&aff_path)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", aff_path.display()));
    let dic_bytes = fs::read(&dic_path)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", dic_path.display()));

    assert_eq!(
        aff_bytes.len(),
        fixture.aff_bytes,
        "{} .aff byte length",
        fixture.id
    );
    assert_eq!(
        dic_bytes.len(),
        fixture.dic_bytes,
        "{} .dic byte length",
        fixture.id
    );
    assert_eq!(
        sha256_hex(&aff_bytes),
        fixture.aff_sha256,
        "{} .aff SHA-256",
        fixture.id
    );
    assert_eq!(
        sha256_hex(&dic_bytes),
        fixture.dic_sha256,
        "{} .dic SHA-256",
        fixture.id
    );
    (aff_path, dic_path, aff_bytes, dic_bytes)
}

fn directives_in(aff_text: &str) -> BTreeSet<&str> {
    aff_text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            (!line.is_empty() && !line.starts_with('#')).then(|| line.split_whitespace().next())?
        })
        .collect()
}

fn join<T>(items: impl IntoIterator<Item = T>) -> String
where
    T: AsRef<str>,
{
    items
        .into_iter()
        .map(|item| item.as_ref().to_owned())
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_manifest(text: &str) -> Result<Vec<Fixture>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.is_empty() && !line.starts_with('#'))
        .map(|(index, line)| parse_fixture(index + 1, line))
        .collect()
}

fn parse_fixture(line_number: usize, line: &str) -> Result<Fixture, String> {
    let fields = line.split('\t').collect::<Vec<_>>();
    if fields.len() != 16 {
        return Err(format!(
            "manifest line {line_number} has {} fields; expected 16",
            fields.len()
        ));
    }
    let parse_probes = |value: &str| {
        value
            .split(';')
            .filter(|probe| !probe.is_empty())
            .map(|probe| {
                let (category, word) = probe.split_once('=').ok_or_else(|| {
                    format!("manifest line {line_number} probe `{probe}` needs category=word")
                })?;
                Ok(Probe {
                    category: ProbeCategory::parse(category)?,
                    word: word.to_owned(),
                })
            })
            .collect::<Result<Vec<_>, String>>()
    };
    Ok(Fixture {
        id: fields[0].to_owned(),
        locale: fields[1].to_owned(),
        aff_path: PathBuf::from(fields[2]),
        dic_path: PathBuf::from(fields[3]),
        aff_bytes: fields[4]
            .parse()
            .map_err(|_| format!("manifest line {line_number} has invalid .aff byte length"))?,
        dic_bytes: fields[5]
            .parse()
            .map_err(|_| format!("manifest line {line_number} has invalid .dic byte length"))?,
        aff_sha256: fields[6].to_owned(),
        dic_sha256: fields[7].to_owned(),
        aff_decode: Decode::parse(fields[8])?,
        dic_decode: Decode::parse(fields[9])?,
        import_expectation: ImportExpectation::parse(fields[10])?,
        accepted: parse_probes(fields[11])?,
        rejected: fields[12]
            .split(',')
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect(),
        source: fields[13].to_owned(),
        license: fields[14].to_owned(),
        license_evidence: fields[15].to_owned(),
    })
}

// SHA-256's fixed eight-word working state intentionally follows the standard
// notation; keeping it local avoids introducing a test-only dependency.
#[allow(clippy::many_single_char_names, clippy::too_many_lines)]
fn sha256_hex(bytes: &[u8]) -> String {
    const INITIAL: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    const ROUND: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];

    let bit_length = u64::try_from(bytes.len())
        .expect("fixture length fits u64")
        .checked_mul(8)
        .expect("fixture bit length fits u64");
    let mut padded = Vec::from(bytes);
    padded.push(0x80);
    while (padded.len() + 8) % 64 != 0 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_length.to_be_bytes());

    let mut hash = INITIAL;
    for block in padded.chunks_exact(64) {
        let mut schedule = [0_u32; 64];
        for (index, word) in schedule[..16].iter_mut().enumerate() {
            *word = u32::from_be_bytes(
                block[(index * 4)..(index * 4 + 4)]
                    .try_into()
                    .expect("block is complete"),
            );
        }
        for index in 16..64 {
            let small_sigma0 = schedule[index - 15].rotate_right(7)
                ^ schedule[index - 15].rotate_right(18)
                ^ (schedule[index - 15] >> 3);
            let small_sigma1 = schedule[index - 2].rotate_right(17)
                ^ schedule[index - 2].rotate_right(19)
                ^ (schedule[index - 2] >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(small_sigma0)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(small_sigma1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = hash;
        for (index, word) in schedule.into_iter().enumerate() {
            let big_sigma1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let temporary1 = h
                .wrapping_add(big_sigma1)
                .wrapping_add(choose)
                .wrapping_add(ROUND[index])
                .wrapping_add(word);
            let big_sigma0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temporary2 = big_sigma0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temporary1);
            d = c;
            c = b;
            b = a;
            a = temporary1.wrapping_add(temporary2);
        }
        hash = [
            hash[0].wrapping_add(a),
            hash[1].wrapping_add(b),
            hash[2].wrapping_add(c),
            hash[3].wrapping_add(d),
            hash[4].wrapping_add(e),
            hash[5].wrapping_add(f),
            hash[6].wrapping_add(g),
            hash[7].wrapping_add(h),
        ];
    }

    let mut encoded = String::with_capacity(64);
    for word in hash {
        write!(encoded, "{word:08x}").expect("writing to String does not fail");
    }
    encoded
}

#[test]
fn sha256_fixture_integrity_uses_the_standard_digest() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

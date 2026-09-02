//! Command-line interface for ferrolex.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fmt;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, Read as _, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use ferrolex_code::{
    Analyzer, AnalyzerConfigError, CommentSyntax, DirectiveProblem, Document, ProjectConfig,
    ProjectConfigError,
};
use ferrolex_compiler::{
    compile_frequency_word_list, compile_words, inspect_compiled_artifact, CompileError,
    CompiledDictionary, FrequencyListError, LoadError, ValidationError,
    MAX_COMPILED_ARTIFACT_BYTES,
};
use ferrolex_core::{Checker, Dictionary, Normalization, UserDictionary, WordList};
use ferrolex_dictionaries::{
    find_locale, DictionaryInstaller, FetchError as DictionaryFetchError, InstalledDictionary,
    LibreOfficeDictionary, ManifestError as DictionaryManifestError, SourceEncoding, UreqFetcher,
    LIBREOFFICE_CATALOG,
};
use ferrolex_hunspell::{
    compile_runtime_artifact, compile_runtime_cache, import_bytes as import_hunspell_bytes,
    import_bytes_with_encodings as import_hunspell_bytes_with_encodings, inspect_runtime_cache,
    is_runtime_artifact, load_runtime_artifact, load_runtime_cache, Acceptance, AcceptanceKind,
    AppliedAffixKind, ByteEncoding, ByteImportEncodings, CasingPath, CompoundComponentRole,
    Diagnostic as ImportDiagnostic, HunspellDictionary, ImportError, ImportMode, ImportResult,
    LookupExplanation, Rejection, RejectionReason, RuntimeCacheError, Severity, SourceDigests,
};
use ferrolex_suggest::{
    CandidateSource, Completeness, ReplacementRule, SuggestConfig, SuggestScratch, Suggester,
    Suggestion,
};
use ferrolex_text::check_text;
use fs2::FileExt as _;
use serde_json::json;

const USAGE: &str = "Usage: ferrolex --help | --version\n       ferrolex check [--format <text|json>] [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] [--] <WORD>\n       ferrolex check [--format <text|json>] [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] --file <PATH|-> [--file <PATH|-> ...] [<PATH> ...]\n       ferrolex suggest [--format <text|json>] [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] [--max-results <COUNT>] [--max-edit-distance <DISTANCE>] [--max-candidates <COUNT>] [--max-edit-cells <COUNT>] <WORD>\n       ferrolex explain --hunspell <AFF_PATH> <WORD>\n       ferrolex analyze [--format <text|json>] [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] [--config <PATH>] [--include <GLOB> ...] [--exclude <GLOB> ...] [--suggest] [--comment-prefix <PREFIX> | --comment-syntax html] <PATH>\n       ferrolex compile (--dictionary <PLAIN_WORD_LIST> | <AFF_PATH> <DIC_PATH>) -o <ARTIFACT>\n       ferrolex inspect <ARTIFACT>\n       ferrolex validate [--format <text|json>] [--strict] <AFF_PATH> <DIC_PATH>\n       ferrolex validate [--format <text|json>] --compiled <ARTIFACT>\n       ferrolex dictionary list\n       ferrolex dictionary fetch <LOCALE> --cache <PATH>\n       ferrolex dictionary install <LOCALE> --cache <PATH>\n       ferrolex dictionary add-word [--workspace <PATH> | --global] <WORD>";
const RUNTIME_ERROR_EXIT_CODE: u8 = 3;
const EXIT_CODES: &str =
    "\nExit status: 0 success, 1 finding, 2 usage error, 3 operational failure.";
const HELP_CHECK: &str = "Usage: ferrolex check [--format <text|json>] [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] [--] <WORD>\n       ferrolex check [--format <text|json>] [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] --file <PATH|-> [--file <PATH|-> ...] [<PATH> ...]\n\nChecks one word or every natural-language word in one or more UTF-8 inputs.\nAutomatically includes workspace and global user dictionaries when present.\n  --format <text|json>  Human-readable text or JSON Lines output (default: text)\n  --dictionary <PATH>  Plain word-list dictionary (repeatable)\n  --compiled <PATH>    Compiled dictionary artifact (repeatable)\n  --hunspell <PATH>    Hunspell AFF path; uses an adjacent cache when present (repeatable)\n  --file <PATH|->      Check a UTF-8 file, or stdin with `-` (repeatable)\n  --                   End options, including before a word beginning with `-`\n\nAfter the first `--file`, positional arguments are additional file paths.\n\nExamples:\n  ferrolex check --dictionary words.txt -- --compound\n  printf 'some text' | ferrolex check --format json --dictionary words.txt --file -";
const HELP_SUGGEST: &str = "Usage: ferrolex suggest [--format <text|json>] [--dictionary <PATH> ...] [--compiled <PATH> ...] [--hunspell <AFF_PATH> ...] [OPTIONS] <WORD>\n\nPrints bounded deterministic spelling suggestions.\nAutomatically includes workspace and global user dictionaries when present.\n  --format <text|json>          Human-readable text or JSON Lines output (default: text)\n  --dictionary <PATH>          Plain word-list dictionary (repeatable)\n  --compiled <PATH>            Compiled dictionary artifact (repeatable)\n  --hunspell <PATH>            Hunspell AFF path; uses an adjacent cache when present (repeatable)\n  --max-results <COUNT>        Maximum returned suggestions\n  --max-edit-distance <COUNT>  Maximum OSA edit distance\n  --max-candidates <COUNT>     Maximum considered candidates\n  --max-edit-cells <COUNT>     Maximum edit-distance work\n\nExample: ferrolex suggest --format json --dictionary words.txt ferolex";
const HELP_EXPLAIN: &str = "Usage: ferrolex explain --hunspell <AFF_PATH> <WORD>\n\nExplains a Hunspell recognition decision.\n\nExample: ferrolex explain --hunspell de_DE.aff Haustürschlüssel";
const HELP_ANALYZE: &str = "Usage: ferrolex analyze [--format <text|json>] [--dictionary <PATH> | --compiled <PATH> | --hunspell <AFF_PATH> | --config <PATH>] [OPTIONS] <PATH>\n\nAnalyzes selected source files using dictionaries or a project config.\nAutomatically includes workspace and global user dictionaries when present.\n  --format <text|json>   Human-readable text or JSON Lines output (default: text)\n  --dictionary <PATH>   Plain word-list dictionary (repeatable)\n  --compiled <PATH>     Compiled dictionary artifact (repeatable)\n  --hunspell <PATH>     Hunspell AFF path; uses an adjacent cache when present (repeatable)\n  --config <PATH>       Project configuration\n  --include <GLOB>      Include glob (repeatable)\n  --exclude <GLOB>      Exclude glob (repeatable)\n  --suggest             Include suggestions for findings\n  --comment-prefix <P>  Line-comment directive prefix\n  --comment-syntax html HTML comment directives\n\nExample: ferrolex analyze --format json --dictionary words.txt src";
const HELP_COMPILE: &str = "Usage: ferrolex compile (--dictionary <PATH> | <AFF_PATH> <DIC_PATH>) -o <ARTIFACT>\n\nCompiles a plain word list or Hunspell pair to a native artifact.\n  -o <ARTIFACT>  Output artifact path\n\nExample: ferrolex compile --dictionary words.txt -o words.flexdic";
const HELP_INSPECT: &str = "Usage: ferrolex inspect <ARTIFACT>\n\nPrints native artifact metadata.\n\nExample: ferrolex inspect words.flexdic";
const HELP_VALIDATE: &str = "Usage: ferrolex validate [--format <text|json>] [--strict] <AFF_PATH> <DIC_PATH>\n       ferrolex validate [--format <text|json>] --compiled <ARTIFACT>\n\nValidates a Hunspell pair or compiled artifact. `--strict` rejects importer errors.\n  --format <text|json>  Human-readable text or JSON Lines output (default: text)\n\nExample: ferrolex validate --format json --strict dictionary.aff dictionary.dic";
const HELP_DICTIONARY: &str = "Usage: ferrolex dictionary <list | fetch | install | add-word> [OPTIONS]\n\nLists reviewed dictionaries, obtains a pinned source, installs a runtime cache, or records a user word.\nUser words are automatically included by check, suggest, and analyze.\n  fetch/install <LOCALE> --cache <PATH>  Use an explicit cache directory\n  add-word [--workspace <PATH> | --global] <WORD>\n\nExample: ferrolex dictionary install pl_PL --cache .ferrolex-dictionaries";

const HUNSPELL_RUNTIME_CACHE_EXTENSION: &str = "ferrolex-hunspell-v2.flexh";
const MAX_ANALYSIS_SUGGESTION_CACHE_ENTRIES: usize = 4_096;
const STALE_TEMPORARY_FILE_AGE: Duration = Duration::from_secs(60 * 60);
static CACHE_WRITE_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn main() -> ExitCode {
    match run(env::args()) {
        Ok(outcome) => outcome.exit_code(),
        Err(error) => {
            eprintln!("error: {error}");
            if error.is_usage() {
                eprintln!("{USAGE}");
                ExitCode::from(2)
            } else {
                ExitCode::from(RUNTIME_ERROR_EXIT_CODE)
            }
        }
    }
}

fn run(arguments: impl IntoIterator<Item = String>) -> Result<RunOutcome, CliError> {
    match parse_arguments(arguments)? {
        Command::Help(help) => {
            println!("{help}{EXIT_CODES}");
            Ok(RunOutcome::Success)
        }
        Command::Version => {
            println!("ferrolex {}", env!("CARGO_PKG_VERSION"));
            Ok(RunOutcome::Success)
        }
        Command::Check(command) => check(&command),
        Command::Suggest(command) => suggest(&command),
        Command::Explain(command) => explain(&command),
        Command::Analyze(command) => analyze(&command),
        Command::Compile(command) => compile(&command),
        Command::Inspect(path) => inspect_artifact(&path),
        Command::Validate(command) => validate(&command),
        Command::Dictionary(command) => dictionary(&command),
    }
}

fn explain(command: &ExplainCommand) -> Result<RunOutcome, CliError> {
    let dictionary = load_installed_hunspell_dictionary(&command.hunspell_affix_path)?;
    print!("{}", render_explanation(&dictionary.explain(&command.word)));
    Ok(RunOutcome::Success)
}

fn render_explanation(explanation: &LookupExplanation) -> String {
    let mut output = String::new();
    match explanation {
        LookupExplanation::Accepted(accepted) => render_accepted(&mut output, accepted),
        LookupExplanation::Rejected(rejected) => render_rejected(&mut output, rejected),
        _ => {
            writeln!(output, "status: unsupported diagnostic variant")
                .expect("writing to a String cannot fail");
        }
    }
    output
}

fn render_accepted(output: &mut String, accepted: &Acceptance) {
    writeln!(output, "status: accepted").expect("writing to a String cannot fail");
    match accepted.casing() {
        CasingPath::Exact => writeln!(output, "casing: exact"),
        CasingPath::CaseFallback { candidate } => {
            writeln!(output, "casing: fallback ({candidate})")
        }
        _ => writeln!(output, "casing: compatibility path"),
    }
    .expect("writing to a String cannot fail");
    match accepted.kind() {
        AcceptanceKind::Stem { stem } => {
            writeln!(output, "match: stem\nstem: {stem}").expect("writing to a String cannot fail");
        }
        AcceptanceKind::Affixed { stem, rules } => {
            writeln!(output, "match: affixed\nstem: {stem}")
                .expect("writing to a String cannot fail");
            for (index, rule) in rules.iter().enumerate() {
                let kind = match rule.kind() {
                    AppliedAffixKind::Prefix => "prefix",
                    AppliedAffixKind::Suffix => "suffix",
                };
                writeln!(
                    output,
                    "rule {}: {kind} strip={:?} add={:?}",
                    index + 1,
                    rule.strip(),
                    rule.add()
                )
                .expect("writing to a String cannot fail");
                if !rule.continuation_flags().is_empty() {
                    writeln!(
                        output,
                        "  continuation-flags: {}",
                        rule.continuation_flags().join(", ")
                    )
                    .expect("writing to a String cannot fail");
                }
            }
        }
        AcceptanceKind::Compound { components } => {
            writeln!(output, "match: compound").expect("writing to a String cannot fail");
            for (index, component) in components.iter().enumerate() {
                writeln!(
                    output,
                    "component {}: {} (stem: {}; role: {})",
                    index + 1,
                    component.spelling(),
                    component.stem(),
                    compound_role_label(component.role())
                )
                .expect("writing to a String cannot fail");
            }
        }
        AcceptanceKind::Compatibility { detail } => {
            writeln!(output, "match: compatibility\ndetail: {detail}")
                .expect("writing to a String cannot fail");
        }
        _ => {
            writeln!(output, "match: unsupported diagnostic variant")
                .expect("writing to a String cannot fail");
        }
    }
}

fn render_rejected(output: &mut String, rejected: &Rejection) {
    writeln!(output, "status: rejected").expect("writing to a String cannot fail");
    let reason = match rejected.reason() {
        RejectionReason::ForbiddenStem { stem } => format!("forbidden stem ({stem})"),
        RejectionReason::NeedsAffix { stem } => format!("stem requires an affix ({stem})"),
        RejectionReason::OnlyInCompound { stem } => {
            format!("stem is valid only in a compound ({stem})")
        }
        RejectionReason::KeepCase { stem } => format!("stem requires its stored case ({stem})"),
        RejectionReason::NoDerivation => "no accepted stem or derivation".to_owned(),
        _ => "unsupported diagnostic variant".to_owned(),
    };
    writeln!(output, "reason: {reason}").expect("writing to a String cannot fail");
}

const fn compound_role_label(role: CompoundComponentRole) -> &'static str {
    match role {
        CompoundComponentRole::Generic => "generic",
        CompoundComponentRole::Begin => "begin",
        CompoundComponentRole::Middle => "middle",
        CompoundComponentRole::End => "end",
    }
}

fn suggest(command: &SuggestCommand) -> Result<RunOutcome, CliError> {
    let source = load_analysis_dictionary(
        &command.dictionary_paths,
        &command.compiled_paths,
        &command.hunspell_affix_paths,
    )?;
    if source.is_empty() {
        return Err(CliError::Usage(
            "suggest requires a dictionary option or a workspace/global user dictionary".to_owned(),
        ));
    }
    let replacements = source.replacement_rules();
    let ranking_dictionary = source.hunspell_ranking_dictionary();
    let mut config = SuggestConfig::default();
    if let Some(max_results) = command.max_results {
        config.max_results = max_results;
    }
    if let Some(max_edit_distance) = command.max_edit_distance {
        config.max_edit_distance = max_edit_distance;
    }
    if let Some(max_candidates) = command.max_candidates {
        config.max_candidates = max_candidates;
    }
    if let Some(max_edit_cells) = command.max_edit_cells {
        config.max_edit_cells = max_edit_cells;
    }
    let result = if let Some(dictionary) = ranking_dictionary {
        Suggester::new(&source, config)
            .with_replacement_rules(&replacements)
            .with_ranking_signals(dictionary.ranking_signals())
            .suggest(&command.word)
    } else {
        Suggester::new(&source, config)
            .with_replacement_rules(&replacements)
            .suggest(&command.word)
    };
    for suggestion in result.suggestions() {
        let word = source.normalize_suggestion_output(suggestion.word());
        match command.output_format {
            OutputFormat::Text => {
                println!("suggestion: {word} (distance {})", suggestion.distance());
            }
            OutputFormat::Json => print_json(json!({
                "type": "suggestion",
                "word": word,
                "distance": suggestion.distance(),
            })),
        }
    }
    if command.output_format == OutputFormat::Json {
        print_json(json!({
            "type": "suggestion-summary",
            "word": command.word,
            "completeness": completeness_code(result.completeness()),
            "complete": result.completeness() == Completeness::Complete,
            "hint": incomplete_suggestion_hint(result.completeness(), config),
        }));
    } else if result.completeness() != Completeness::Complete {
        eprintln!(
            "suggestion search incomplete: {}",
            completeness_label(result.completeness())
        );
        if result.suggestions().is_empty() {
            if let Some(hint) = incomplete_suggestion_hint(result.completeness(), config) {
                eprintln!("hint: {hint}");
            }
        }
    }
    Ok(RunOutcome::Success)
}

const fn completeness_code(completeness: Completeness) -> &'static str {
    match completeness {
        Completeness::Complete => "complete",
        Completeness::CandidateLimitReached => "candidate-limit",
        Completeness::EditBudgetReached => "edit-budget",
        Completeness::QueryTooLong => "query-too-long",
        Completeness::RelatedSeedTooLong => "related-seed-too-long",
    }
}

fn incomplete_suggestion_hint(completeness: Completeness, config: SuggestConfig) -> Option<String> {
    match completeness {
        Completeness::CandidateLimitReached | Completeness::EditBudgetReached => Some(format!(
            "retry with larger work budgets, for example `--max-candidates {} --max-edit-cells {}`",
            config.max_candidates.saturating_mul(2),
            config.max_edit_cells.saturating_mul(2),
        )),
        Completeness::Complete | Completeness::QueryTooLong | Completeness::RelatedSeedTooLong => {
            None
        }
    }
}

const fn completeness_label(completeness: Completeness) -> &'static str {
    match completeness {
        Completeness::Complete => "complete",
        Completeness::CandidateLimitReached => "candidate limit reached",
        Completeness::EditBudgetReached => "edit-distance budget reached",
        Completeness::QueryTooLong => "query exceeds the scalar limit",
        Completeness::RelatedSeedTooLong => "related seed exceeds the scalar limit",
    }
}

fn dictionary(command: &DictionaryCommand) -> Result<RunOutcome, CliError> {
    match command {
        DictionaryCommand::List => {
            for source in LIBREOFFICE_CATALOG {
                println!(
                    "{}\trevision={}\tencoding={}\tspdx={}\tnotice={}",
                    source.locale(),
                    source.revision(),
                    source.encoding().label(),
                    source.license_spdx_expression(),
                    source.license_notice_url()
                );
            }
            Ok(RunOutcome::Success)
        }
        DictionaryCommand::Fetch { locale, cache_path } => {
            let (source, installed) = fetch_catalog_dictionary(locale, cache_path)?;
            println!("fetched: {}", installed.aff_path().display());
            println!("fetched: {}", installed.dic_path().display());
            println!("license: {}", source.license_spdx_expression());
            println!("notice: {}", source.license_notice_url());
            println!(
                "hint: build the runtime cache with `ferrolex dictionary install {locale} --cache {}` before using this catalog dictionary with `--hunspell`",
                cache_path.display()
            );
            Ok(RunOutcome::Success)
        }
        DictionaryCommand::Install { locale, cache_path } => {
            let (source, installed) = fetch_catalog_dictionary(locale, cache_path)?;
            println!("installed: {}", installed.aff_path().display());
            println!("installed: {}", installed.dic_path().display());
            println!("license: {}", source.license_spdx_expression());
            println!("notice: {}", source.license_notice_url());
            install_hunspell_runtime_cache(
                source.locale(),
                installed.aff_path(),
                installed.dic_path(),
                catalog_import_encodings(source.encoding()),
            )
        }
        DictionaryCommand::AddWord { word, path } => add_user_dictionary_word(word, path),
    }
}

fn add_user_dictionary_word(word: &str, path: &Path) -> Result<RunOutcome, CliError> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|source| CliError::WriteUserDictionary {
        path: path.to_path_buf(),
        source,
    })?;
    let _lock = UserDictionaryLock::acquire(path)?;
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(CliError::ReadDictionary {
                path: path.to_path_buf(),
                source,
            })
        }
    };
    let dictionary = UserDictionary::from_text(Normalization::Nfc, &text);
    let added = dictionary.insert(word).map_err(CliError::InvalidUserWord)?;
    atomic_write(path, &dictionary.to_text())?;
    println!(
        "{}: {}",
        if added { "added" } else { "already present" },
        path.display()
    );
    Ok(RunOutcome::Success)
}

fn atomic_write(path: &Path, text: &str) -> Result<(), CliError> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|source| CliError::WriteUserDictionary {
        path: path.to_path_buf(),
        source,
    })?;
    sweep_stale_temporary_siblings(path);
    let temporary = temporary_sibling(path);
    let mut created = false;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|source| CliError::WriteUserDictionary {
                path: temporary.clone(),
                source,
            })?;
        created = true;
        file.write_all(text.as_bytes())
            .map_err(|source| CliError::WriteUserDictionary {
                path: temporary.clone(),
                source,
            })?;
        file.sync_all()
            .map_err(|source| CliError::WriteUserDictionary {
                path: temporary.clone(),
                source,
            })?;
        fs::rename(&temporary, path).map_err(|source| CliError::WriteUserDictionary {
            path: path.to_path_buf(),
            source,
        })?;
        sync_parent_directory(parent).map_err(|source| CliError::WriteUserDictionary {
            path: parent.to_path_buf(),
            source,
        })
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(&temporary);
    }
    result
}

struct UserDictionaryLock {
    file: fs::File,
}

impl UserDictionaryLock {
    fn acquire(dictionary_path: &Path) -> Result<Self, CliError> {
        let lock_path = hidden_sibling(dictionary_path, "lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|source| CliError::WriteUserDictionary {
                path: lock_path.clone(),
                source,
            })?;
        file.lock_exclusive()
            .map_err(|source| CliError::WriteUserDictionary {
                path: lock_path,
                source,
            })?;
        Ok(Self { file })
    }
}

impl Drop for UserDictionaryLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

fn hidden_sibling(path: &Path, suffix: &str) -> PathBuf {
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut name = std::ffi::OsString::from(".");
    name.push(path.file_name().unwrap_or(std::ffi::OsStr::new("words")));
    name.push(".");
    name.push(suffix);
    parent.join(name)
}

fn temporary_sibling(path: &Path) -> PathBuf {
    hidden_sibling(
        path,
        &format!(
            "tmp-{}-{}",
            std::process::id(),
            CACHE_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ),
    )
}

fn sweep_stale_temporary_siblings(path: &Path) {
    let parent = path.parent().unwrap_or(Path::new("."));
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let current_prefix = format!(".{name}.tmp-");
    let legacy_runtime_prefix = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| format!("{stem}.tmp-"));
    let legacy_user_prefix = format!(".{name}.");
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let legacy_user_pid = file_name
            .strip_prefix(&legacy_user_prefix)
            .and_then(|value| value.strip_suffix(".tmp"))
            .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit()));
        let belongs_to_path = file_name.starts_with(&current_prefix)
            || legacy_runtime_prefix
                .as_deref()
                .is_some_and(|prefix| file_name.starts_with(prefix))
            || legacy_user_pid;
        if belongs_to_path && file_is_stale(&entry.path(), STALE_TEMPORARY_FILE_AGE) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn file_is_stale(path: &Path, maximum_age: Duration) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age >= maximum_age)
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) -> io::Result<()> {
    fs::File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "all platforms share one fallible directory-sync call site"
)]
fn sync_parent_directory(_parent: &Path) -> io::Result<()> {
    Ok(())
}

fn catalog_import_encodings(encoding: SourceEncoding) -> Option<ByteImportEncodings> {
    match encoding {
        SourceEncoding::MixedUtf8AndIso8859_1 => Some(ByteImportEncodings::new(
            ByteEncoding::Iso8859_1,
            ByteEncoding::Utf8,
        )),
        SourceEncoding::MixedUtf8AndIso8859_2Fallback => Some(ByteImportEncodings::new(
            ByteEncoding::Utf8WithIso8859_2Fallback,
            ByteEncoding::Utf8,
        )),
        SourceEncoding::Utf8 | SourceEncoding::Iso8859_1 | SourceEncoding::Iso8859_2 => None,
    }
}

fn fetch_catalog_dictionary(
    locale: &str,
    cache_path: &Path,
) -> Result<(LibreOfficeDictionary, InstalledDictionary), CliError> {
    let source = find_locale(locale).ok_or_else(|| {
        CliError::Usage(format!(
            "unsupported LibreOffice locale `{locale}`; run `ferrolex dictionary list`"
        ))
    })?;
    let manifest = source.manifest().map_err(CliError::DictionaryManifest)?;
    let aff_url = source.aff_url();
    let host = aff_url
        .strip_prefix("https://")
        .and_then(|url| url.split('/').next())
        .unwrap_or("the pinned dictionary source");
    eprintln!("fetching {locale} from {host}...");
    let installed = DictionaryInstaller::new(UreqFetcher)
        .install(&manifest, cache_path)
        .map_err(CliError::FetchDictionary)?;
    Ok((source, installed))
}

fn compile(command: &CompileCommand) -> Result<RunOutcome, CliError> {
    let (compiled, description) = match &command.input {
        CompileInput::WordList(path) => {
            eprintln!("reading word list from {}...", path.display());
            let text = fs::read_to_string(path).map_err(|source| CliError::ReadDictionary {
                path: path.clone(),
                source,
            })?;
            if text.lines().any(|line| line.contains('\t')) {
                eprintln!("building frequency-annotated word-list artifact...");
                (
                    compile_frequency_word_list(&text).map_err(CliError::CompileFrequencyList)?,
                    "frequency-annotated words".to_owned(),
                )
            } else {
                let dictionary = WordList::from_text(Normalization::Exact, &text);
                eprintln!("building word-list artifact...");
                (
                    compile_words(dictionary.words()).map_err(CliError::CompileDictionary)?,
                    format!("{} words", dictionary.len()),
                )
            }
        }
        CompileInput::Hunspell { aff_path, dic_path } => {
            eprintln!(
                "importing Hunspell sources {} and {}...",
                aff_path.display(),
                dic_path.display()
            );
            let (import, sources) = import_hunspell_files(aff_path, dic_path, None, true)?;
            let dictionary = match import {
                Ok(dictionary) => dictionary,
                Err(error) => {
                    for diagnostic in error.diagnostics() {
                        print_import_diagnostic_to_stderr(diagnostic);
                    }
                    eprintln!("error: could not compile Hunspell dictionary");
                    return Ok(RunOutcome::Failure);
                }
            };
            for diagnostic in dictionary.diagnostics() {
                print_import_diagnostic(diagnostic);
            }
            let lexemes = dictionary.ir().lexemes.len();
            eprintln!("building Hunspell runtime artifact...");
            (
                compile_runtime_artifact(dictionary.dictionary(), sources)
                    .map_err(CliError::CompileHunspellCache)?,
                format!("Hunspell, {lexemes} lexemes"),
            )
        }
    };
    eprintln!(
        "writing compiled artifact to {}...",
        command.output_path.display()
    );
    fs::write(&command.output_path, compiled).map_err(|source| CliError::WriteArtifact {
        path: command.output_path.clone(),
        source,
    })?;

    println!(
        "compiled: {} ({description})",
        command.output_path.display(),
    );
    Ok(RunOutcome::Success)
}

fn check(command: &CheckCommand) -> Result<RunOutcome, CliError> {
    let checker = load_checker(
        &command.dictionary_paths,
        &command.compiled_paths,
        &command.hunspell_affix_paths,
    )?;

    match &command.target {
        CheckTarget::Word(word) => Ok(check_word(&checker, word, command.output_format)),
        CheckTarget::Inputs(inputs) => check_inputs(&checker, inputs, command.output_format),
    }
}

fn load_checker(
    dictionary_paths: &[PathBuf],
    compiled_paths: &[PathBuf],
    hunspell_affix_paths: &[PathBuf],
) -> Result<Checker, CliError> {
    let mut builder = Checker::builder();
    let user_dictionaries = load_user_dictionaries()?;
    if user_dictionaries.is_empty()
        && dictionary_paths.is_empty()
        && compiled_paths.is_empty()
        && hunspell_affix_paths.is_empty()
    {
        return Err(CliError::Usage(
            "check requires a dictionary option or a workspace/global user dictionary".to_owned(),
        ));
    }
    for dictionary in user_dictionaries {
        builder = builder.dictionary(dictionary);
    }
    for path in dictionary_paths {
        let text = fs::read_to_string(path).map_err(|source| CliError::ReadDictionary {
            path: path.clone(),
            source,
        })?;
        builder = builder.dictionary(WordList::from_text(Normalization::Exact, &text));
    }
    for path in compiled_paths {
        builder = builder.dictionary(load_artifact(path)?);
    }
    for aff_path in hunspell_affix_paths {
        builder = builder.dictionary(load_installed_hunspell_dictionary(aff_path)?);
    }

    Ok(builder.build())
}

/// Dictionary inputs retained both for recognition and bounded suggestions.
struct AnalysisDictionary {
    sources: Vec<AnalysisSource>,
}

impl AnalysisDictionary {
    fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    fn replacement_rules(&self) -> Vec<ReplacementRule> {
        self.sources
            .iter()
            .flat_map(|source| match source {
                AnalysisSource::WordList(_) => Vec::new(),
                AnalysisSource::Artifact(dictionary) => dictionary.replacement_rules(),
                AnalysisSource::Hunspell(dictionary) => dictionary.replacement_rules().to_vec(),
            })
            .collect()
    }

    fn hunspell_ranking_dictionary(&self) -> Option<&HunspellDictionary> {
        self.sources.iter().find_map(|source| match source {
            AnalysisSource::WordList(_) => None,
            AnalysisSource::Artifact(dictionary) => dictionary.hunspell_dictionary(),
            AnalysisSource::Hunspell(dictionary) => Some(dictionary.as_ref()),
        })
    }

    fn normalize_suggestion_output(&self, candidate: &str) -> String {
        for source in &self.sources {
            match source {
                AnalysisSource::WordList(dictionary) if dictionary.contains(candidate) => {
                    return candidate.to_owned();
                }
                AnalysisSource::Artifact(dictionary)
                    if dictionary.contains(candidate)
                        && dictionary.is_suggestion_candidate(candidate) =>
                {
                    return dictionary.normalize_suggestion_output(candidate);
                }
                AnalysisSource::Hunspell(dictionary)
                    if dictionary.is_suggestion_candidate(candidate) =>
                {
                    return dictionary.normalize_output(candidate);
                }
                AnalysisSource::WordList(_)
                | AnalysisSource::Artifact(_)
                | AnalysisSource::Hunspell(_) => {}
            }
        }
        candidate.to_owned()
    }
}

enum AnalysisSource {
    WordList(WordList),
    Artifact(ArtifactDictionary),
    Hunspell(Box<HunspellDictionary>),
}

impl Dictionary for AnalysisDictionary {
    fn contains(&self, word: &str) -> bool {
        self.sources.iter().any(|source| match source {
            AnalysisSource::WordList(dictionary) => dictionary.contains(word),
            AnalysisSource::Artifact(dictionary) => dictionary.contains(word),
            AnalysisSource::Hunspell(dictionary) => dictionary.contains(word),
        })
    }
}

impl CandidateSource for AnalysisDictionary {
    fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        for source in &self.sources {
            let mut keep_going = true;
            match source {
                AnalysisSource::WordList(dictionary) => dictionary.visit_candidates(&mut |word| {
                    keep_going = visitor(word);
                    keep_going
                }),
                AnalysisSource::Artifact(dictionary) => dictionary.visit_candidates(&mut |word| {
                    keep_going = visitor(word);
                    keep_going
                }),
                AnalysisSource::Hunspell(dictionary) => dictionary.visit_candidates(&mut |word| {
                    keep_going = visitor(word);
                    keep_going
                }),
            }
            if !keep_going {
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
        for source in &self.sources {
            let mut keep_going = true;
            {
                let mut visit = |word: &str| {
                    keep_going = visitor(word);
                    keep_going
                };
                match source {
                    AnalysisSource::WordList(dictionary) => dictionary.visit_nearby_candidates(
                        query,
                        max_edit_distance,
                        max_word_scalars,
                        &mut visit,
                    ),
                    AnalysisSource::Artifact(dictionary) => dictionary.visit_nearby_candidates(
                        query,
                        max_edit_distance,
                        max_word_scalars,
                        &mut visit,
                    ),
                    AnalysisSource::Hunspell(dictionary) => dictionary.visit_nearby_candidates(
                        query,
                        max_edit_distance,
                        max_word_scalars,
                        &mut visit,
                    ),
                }
            }
            if !keep_going {
                break;
            }
        }
    }

    fn is_suggestion_candidate(&self, candidate: &str) -> bool {
        self.sources.iter().any(|source| match source {
            AnalysisSource::WordList(dictionary) => dictionary.contains(candidate),
            AnalysisSource::Artifact(dictionary) => {
                dictionary.contains(candidate) && dictionary.is_suggestion_candidate(candidate)
            }
            AnalysisSource::Hunspell(dictionary) => dictionary.is_suggestion_candidate(candidate),
        })
    }

    fn candidate_frequency(&self, candidate: &str) -> Option<u64> {
        self.sources.iter().find_map(|source| match source {
            AnalysisSource::WordList(_) => None,
            AnalysisSource::Artifact(dictionary) => dictionary.candidate_frequency(candidate),
            AnalysisSource::Hunspell(dictionary) => dictionary.candidate_frequency(candidate),
        })
    }

    fn visit_related_candidates(
        &self,
        query: &str,
        seed: &str,
        max_edit_distance: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        for source in &self.sources {
            let mut keep_going = true;
            match source {
                AnalysisSource::WordList(_) => {}
                AnalysisSource::Artifact(dictionary) => dictionary.visit_related_candidates(
                    query,
                    seed,
                    max_edit_distance,
                    &mut |word| {
                        keep_going = visitor(word);
                        keep_going
                    },
                ),
                AnalysisSource::Hunspell(dictionary) => dictionary.visit_related_candidates(
                    query,
                    seed,
                    max_edit_distance,
                    &mut |word| {
                        keep_going = visitor(word);
                        keep_going
                    },
                ),
            }
            if !keep_going {
                break;
            }
        }
    }

    fn visit_related_seeds(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        for source in &self.sources {
            let mut keep_going = true;
            match source {
                AnalysisSource::WordList(_) => {}
                AnalysisSource::Artifact(dictionary) => {
                    dictionary.visit_related_seeds(&mut |word| {
                        keep_going = visitor(word);
                        keep_going
                    });
                }
                AnalysisSource::Hunspell(dictionary) => {
                    dictionary.visit_related_seeds(&mut |word| {
                        keep_going = visitor(word);
                        keep_going
                    });
                }
            }
            if !keep_going {
                break;
            }
        }
    }
}

fn load_analysis_dictionary(
    dictionary_paths: &[PathBuf],
    compiled_paths: &[PathBuf],
    hunspell_affix_paths: &[PathBuf],
) -> Result<AnalysisDictionary, CliError> {
    let mut sources = load_user_dictionaries()?
        .into_iter()
        .map(AnalysisSource::WordList)
        .collect::<Vec<_>>();
    for path in dictionary_paths {
        let text = fs::read_to_string(path).map_err(|source| CliError::ReadDictionary {
            path: path.clone(),
            source,
        })?;
        sources.push(AnalysisSource::WordList(WordList::from_text(
            Normalization::Exact,
            &text,
        )));
    }
    for path in compiled_paths {
        sources.push(AnalysisSource::Artifact(load_artifact(path)?));
    }
    for aff_path in hunspell_affix_paths {
        sources.push(AnalysisSource::Hunspell(Box::new(
            load_installed_hunspell_dictionary(aff_path)?,
        )));
    }
    Ok(AnalysisDictionary { sources })
}

/// A standalone `--compiled` artifact, independent of its source-pair files.
enum ArtifactDictionary {
    Exact(CompiledDictionary),
    Hunspell(Box<HunspellDictionary>),
}

impl ArtifactDictionary {
    fn replacement_rules(&self) -> Vec<ReplacementRule> {
        match self {
            Self::Exact(_) => Vec::new(),
            Self::Hunspell(dictionary) => dictionary.replacement_rules().to_vec(),
        }
    }

    fn hunspell_dictionary(&self) -> Option<&HunspellDictionary> {
        match self {
            Self::Exact(_) => None,
            Self::Hunspell(dictionary) => Some(dictionary.as_ref()),
        }
    }

    fn normalize_suggestion_output(&self, candidate: &str) -> String {
        match self {
            Self::Exact(_) => candidate.to_owned(),
            Self::Hunspell(dictionary) => dictionary.normalize_output(candidate),
        }
    }
}

impl Dictionary for ArtifactDictionary {
    fn contains(&self, word: &str) -> bool {
        match self {
            Self::Exact(dictionary) => dictionary.contains(word),
            Self::Hunspell(dictionary) => dictionary.contains(word),
        }
    }
}

impl CandidateSource for ArtifactDictionary {
    fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        match self {
            Self::Exact(dictionary) => dictionary.visit_candidates(visitor),
            Self::Hunspell(dictionary) => dictionary.visit_candidates(visitor),
        }
    }

    fn visit_nearby_candidates(
        &self,
        query: &[char],
        max_edit_distance: usize,
        max_word_scalars: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        match self {
            Self::Exact(dictionary) => dictionary.visit_nearby_candidates(
                query,
                max_edit_distance,
                max_word_scalars,
                visitor,
            ),
            Self::Hunspell(dictionary) => dictionary.visit_nearby_candidates(
                query,
                max_edit_distance,
                max_word_scalars,
                visitor,
            ),
        }
    }

    fn candidate_frequency(&self, candidate: &str) -> Option<u64> {
        match self {
            Self::Exact(dictionary) => dictionary.frequency(candidate),
            Self::Hunspell(dictionary) => dictionary.candidate_frequency(candidate),
        }
    }

    fn is_suggestion_candidate(&self, candidate: &str) -> bool {
        match self {
            Self::Exact(_) => true,
            Self::Hunspell(dictionary) => dictionary.is_suggestion_candidate(candidate),
        }
    }

    fn visit_related_candidates(
        &self,
        query: &str,
        seed: &str,
        max_edit_distance: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        if let Self::Hunspell(dictionary) = self {
            dictionary.visit_related_candidates(query, seed, max_edit_distance, visitor);
        }
    }

    fn visit_related_seeds(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        if let Self::Hunspell(dictionary) = self {
            dictionary.visit_related_seeds(visitor);
        }
    }
}

fn load_artifact(path: &Path) -> Result<ArtifactDictionary, CliError> {
    let bytes = read_compiled_artifact(path)?;
    if is_runtime_artifact(&bytes) {
        load_runtime_artifact(&bytes)
            .map(|dictionary| ArtifactDictionary::Hunspell(Box::new(dictionary)))
            .map_err(|source| CliError::LoadHunspellArtifact {
                path: path.to_path_buf(),
                source,
            })
    } else {
        CompiledDictionary::load(bytes)
            .map(ArtifactDictionary::Exact)
            .map_err(|source| CliError::LoadArtifact {
                path: path.to_path_buf(),
                source,
            })
    }
}

fn load_installed_hunspell_dictionary(aff_path: &Path) -> Result<HunspellDictionary, CliError> {
    let dic_path = aff_path.with_extension("dic");
    let cache_path = runtime_cache_path(aff_path);
    let aff_bytes = fs::read(aff_path).map_err(|source| CliError::ReadInput {
        path: aff_path.to_path_buf(),
        source,
    })?;
    let dic_bytes = fs::read(&dic_path).map_err(|source| CliError::ReadInput {
        path: dic_path.clone(),
        source,
    })?;
    let cache = match fs::read(&cache_path) {
        Ok(cache) => cache,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            eprintln!(
                "notice: no Hunspell runtime cache found at `{}`; importing `{}` and `{}` directly (slower)",
                cache_path.display(),
                aff_path.display(),
                dic_path.display()
            );
            eprintln!(
                "hint: for repeated use or read-only source directories, run `ferrolex compile {} {} -o dictionary.flexh` and pass `--compiled dictionary.flexh`; catalog dictionaries can use `ferrolex dictionary install`",
                aff_path.display(),
                dic_path.display()
            );
            let aff_source = aff_path.display().to_string();
            let dic_source = dic_path.display().to_string();
            let imported = match import_hunspell_source_bytes(
                &aff_source,
                &aff_bytes,
                &dic_source,
                &dic_bytes,
                None,
                ImportMode::Strict,
            ) {
                Ok(imported) => imported,
                Err(source) => {
                    for diagnostic in source.diagnostics() {
                        print_import_diagnostic_to_stderr(diagnostic);
                    }
                    return Err(CliError::ImportHunspellSources {
                        aff_path: aff_path.to_path_buf(),
                        dic_path: dic_path.clone(),
                        source,
                    });
                }
            };
            for diagnostic in imported.diagnostics() {
                print_import_diagnostic_to_stderr(diagnostic);
            }
            return Ok(imported.dictionary().clone());
        }
        Err(source) => {
            return Err(CliError::ReadHunspellCache {
                path: cache_path.clone(),
                source,
            })
        }
    };

    load_runtime_cache(
        &cache,
        SourceDigests::from_source_bytes(&aff_bytes, &dic_bytes),
    )
    .map_err(|source| CliError::LoadHunspellCache {
        path: cache_path,
        source,
    })
}

fn check_word(checker: &Checker, word: &str, output_format: OutputFormat) -> RunOutcome {
    let accepted = checker.contains(word);
    if output_format == OutputFormat::Json {
        print_json(json!({
            "type": "word",
            "command": "check",
            "word": word,
            "status": if accepted { "accepted" } else { "misspelled" },
        }));
    } else if accepted {
        println!("accepted: {word}");
    } else {
        println!("misspelled: {word}");
    }

    if accepted {
        RunOutcome::Success
    } else {
        RunOutcome::Misspelled
    }
}

fn check_inputs(
    checker: &Checker,
    inputs: &[CheckInput],
    output_format: OutputFormat,
) -> Result<RunOutcome, CliError> {
    let mut outcome = RunOutcome::Success;

    for input in inputs {
        let input_outcome = match input {
            CheckInput::File(path) => check_file(checker, path, output_format)?,
            CheckInput::Stdin => check_stdin(checker, output_format)?,
        };
        if input_outcome == RunOutcome::Misspelled {
            outcome = RunOutcome::Misspelled;
        }
    }

    Ok(outcome)
}

fn check_file(
    checker: &Checker,
    path: &Path,
    output_format: OutputFormat,
) -> Result<RunOutcome, CliError> {
    let text = fs::read_to_string(path).map_err(|source| CliError::ReadInput {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(check_source(checker, path, &text, output_format))
}

fn check_stdin(checker: &Checker, output_format: OutputFormat) -> Result<RunOutcome, CliError> {
    let path = Path::new("-");
    let mut text = String::new();
    io::stdin()
        .read_to_string(&mut text)
        .map_err(|source| CliError::ReadInput {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(check_source(checker, path, &text, output_format))
}

fn check_source(
    checker: &Checker,
    path: &Path,
    text: &str,
    output_format: OutputFormat,
) -> RunOutcome {
    let mut misspelled = false;

    let line_index = LineIndex::new(text);
    for issue in check_text(checker, text) {
        print_finding(
            "check",
            output_format,
            path,
            text,
            &line_index,
            issue.range().start,
            issue.word(),
        );
        misspelled = true;
    }

    if misspelled {
        RunOutcome::Misspelled
    } else {
        RunOutcome::Success
    }
}

fn analyze(command: &AnalyzeCommand) -> Result<RunOutcome, CliError> {
    let project = command
        .config_path
        .as_ref()
        .map(|config_path| {
            let text =
                fs::read_to_string(config_path).map_err(|source| CliError::ReadProjectConfig {
                    path: config_path.clone(),
                    source,
                })?;
            ProjectConfig::from_text(&text).map_err(|source| CliError::ProjectConfig {
                path: config_path.clone(),
                source,
            })
        })
        .transpose()?;
    let mut dictionary_paths = command.dictionary_paths.clone();
    let mut compiled_paths = command.compiled_paths.clone();
    let mut hunspell_paths = command.hunspell_affix_paths.clone();
    if let Some(project) = &project {
        let base = command
            .config_path
            .as_ref()
            .and_then(|path| path.parent())
            .unwrap_or(Path::new("."));
        dictionary_paths.extend(project.dictionary_paths().map(|path| base.join(path)));
        compiled_paths.extend(
            project
                .compiled_dictionary_paths()
                .map(|path| base.join(path)),
        );
        hunspell_paths.extend(project.hunspell_paths().map(|path| base.join(path)));
    }
    let dictionary = load_analysis_dictionary(&dictionary_paths, &compiled_paths, &hunspell_paths)?;
    if dictionary.is_empty() {
        return Err(CliError::Usage(
            "analyze requires a dictionary option, configured source, or a workspace/global user dictionary"
                .to_owned(),
        ));
    }
    let mut builder = Analyzer::builder(&dictionary);
    let mut include_patterns = command.include_patterns.clone();
    let mut exclude_patterns = command.exclude_patterns.clone();
    let project_comment_syntax = project.as_ref().and_then(ProjectConfig::comment_syntax);
    if let Some(config) = &project {
        builder =
            builder
                .project_config(config)
                .map_err(|source| CliError::ApplyProjectConfig {
                    path: command
                        .config_path
                        .clone()
                        .expect("project has a config path"),
                    source,
                })?;
        include_patterns.extend(config.include_patterns().map(str::to_owned));
        exclude_patterns.extend(config.exclude_patterns().map(str::to_owned));
    }
    let analyzer = builder.build();
    let paths = analysis_paths(&command.path, &include_patterns, &exclude_patterns)?;
    let mut suggestion_engine = analysis_suggestion_engine(command.suggest, &dictionary);
    let mut has_diagnostic = false;
    for path in paths {
        let Some(source) = read_analysis_source(&path)? else {
            continue;
        };
        let line_index = LineIndex::new(&source);
        let document = analysis_document(
            &source,
            &path,
            command.comment_syntax.as_ref(),
            project_comment_syntax.as_ref(),
        );
        let analysis = analyzer.check(&document);
        for finding in analysis.findings() {
            print_analysis_finding(
                command.output_format,
                &path,
                &source,
                &line_index,
                finding,
                suggestion_engine.as_mut(),
            );
            has_diagnostic = true;
        }
        for diagnostic in analysis.directive_diagnostics() {
            print_directive_diagnostic(
                command.output_format,
                &path,
                &source,
                &line_index,
                diagnostic,
            );
            has_diagnostic = true;
        }
    }
    Ok(if has_diagnostic {
        RunOutcome::Misspelled
    } else {
        RunOutcome::Success
    })
}

fn print_directive_diagnostic(
    output_format: OutputFormat,
    path: &Path,
    source: &str,
    line_index: &LineIndex,
    diagnostic: &ferrolex_code::DirectiveDiagnostic,
) {
    let (line, column) = line_index.line_and_column(source, diagnostic.range().start);
    match output_format {
        OutputFormat::Text => println!(
            "{}:{line}:{column}: malformed directive: {:?}",
            path.display(),
            diagnostic.problem()
        ),
        OutputFormat::Json => print_json(json!({
            "type": "finding",
            "kind": "directive",
            "command": "analyze",
            "path": path.display().to_string(),
            "line": line,
            "column": column,
            "problem": directive_problem_code(diagnostic.problem()),
        })),
    }
}

fn read_analysis_source(path: &Path) -> Result<Option<String>, CliError> {
    match fs::read_to_string(path) {
        Ok(source) => Ok(Some(source)),
        Err(source) if source.kind() == io::ErrorKind::InvalidData => {
            eprintln!("warning: skipping non-UTF-8 input '{}'", path.display());
            Ok(None)
        }
        Err(source) => Err(CliError::ReadInput {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn analysis_document<'source>(
    source: &'source str,
    path: &Path,
    command_syntax: Option<&CommentSyntax>,
    project_syntax: Option<&CommentSyntax>,
) -> Document<'source> {
    let syntax = command_syntax
        .or(project_syntax)
        .cloned()
        .unwrap_or_else(|| comment_syntax_for_path(path));
    Document::new(source).with_comment_syntax(syntax)
}

fn comment_syntax_for_path(path: &Path) -> CommentSyntax {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(
            "rs" | "c" | "cc" | "cpp" | "h" | "hpp" | "java" | "js" | "jsx" | "ts" | "tsx" | "go"
            | "swift" | "kt",
        ) => CommentSyntax::line("//"),
        Some("py" | "rb" | "sh" | "bash" | "zsh" | "yaml" | "yml" | "toml") => {
            CommentSyntax::line("#")
        }
        Some("sql" | "lua" | "hs") => CommentSyntax::line("--"),
        Some("md" | "markdown" | "html" | "htm" | "xml") => CommentSyntax::Html,
        _ => CommentSyntax::None,
    }
}

fn analysis_paths(
    path: &Path,
    include_patterns: &[String],
    exclude_patterns: &[String],
) -> Result<Vec<PathBuf>, CliError> {
    let metadata = fs::metadata(path).map_err(|source| CliError::ReadInput {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut paths = Vec::new();
    collect_analysis_paths(path, path, include_patterns, exclude_patterns, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_analysis_paths(
    root: &Path,
    directory: &Path,
    includes: &[String],
    excludes: &[String],
    paths: &mut Vec<PathBuf>,
) -> Result<(), CliError> {
    let mut entries = fs::read_dir(directory)
        .map_err(|source| CliError::ReadInput {
            path: directory.to_path_buf(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| CliError::ReadInput {
            path: directory.to_path_buf(),
            source,
        })?;
    entries.sort_by_key(fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let file_type = entry.file_type().map_err(|source| CliError::ReadInput {
            path: path.clone(),
            source,
        })?;
        if matches_any(&relative, excludes)
            || (file_type.is_dir() && is_vcs_metadata_directory(&path))
        {
            continue;
        }
        if file_type.is_dir() {
            collect_analysis_paths(root, &path, includes, excludes, paths)?;
        } else if file_type.is_file() && (includes.is_empty() || matches_any(&relative, includes)) {
            paths.push(path);
        }
    }
    Ok(())
}

fn matches_any(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| glob_matches(pattern, path))
}

fn is_vcs_metadata_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, ".git" | ".hg" | ".svn"))
}

fn glob_matches(pattern: &str, path: &str) -> bool {
    glob_matches_bytes(pattern.as_bytes(), path.as_bytes())
}

fn glob_matches_bytes(pattern: &[u8], path: &[u8]) -> bool {
    match (pattern, path) {
        ([], []) => true,
        ([b'*', b'*', b'/', rest @ ..], _) => glob_starstar_directory(pattern, rest, path),
        ([b'*', b'*', rest @ ..], _) => glob_starstar(pattern, rest, path),
        ([b'*', rest @ ..], _) => {
            glob_matches_bytes(rest, path)
                || (!path.is_empty() && path[0] != b'/' && glob_matches_bytes(pattern, &path[1..]))
        }
        ([b'?', rest @ ..], [_, path_rest @ ..]) => glob_matches_bytes(rest, path_rest),
        ([first, rest @ ..], [candidate, path_rest @ ..]) if first == candidate => {
            glob_matches_bytes(rest, path_rest)
        }
        _ => false,
    }
}

fn glob_starstar(pattern: &[u8], rest: &[u8], path: &[u8]) -> bool {
    glob_matches_bytes(rest, path) || (!path.is_empty() && glob_matches_bytes(pattern, &path[1..]))
}

fn glob_starstar_directory(pattern: &[u8], rest: &[u8], path: &[u8]) -> bool {
    glob_starstar(pattern, rest, path)
}

fn print_analysis_finding(
    output_format: OutputFormat,
    path: &Path,
    source: &str,
    line_index: &LineIndex,
    finding: &ferrolex_code::Finding<'_>,
    suggestion_engine: Option<&mut AnalysisSuggestionEngine<'_, AnalysisDictionary>>,
) {
    let (line, column) = line_index.line_and_column(source, finding.range().start);
    let suggestions = suggestion_engine.map_or_else(Vec::new, |engine| engine.suggestions(finding));

    match output_format {
        OutputFormat::Text => {
            print_finding(
                "analyze",
                OutputFormat::Text,
                path,
                source,
                line_index,
                finding.range().start,
                finding.word(),
            );
            for (replacement, distance) in suggestions {
                println!(
                    "{}:{line}:{column}: suggestion: {replacement} (distance {distance})",
                    path.display()
                );
            }
        }
        OutputFormat::Json => {
            let suggestions = suggestions
                .into_iter()
                .map(|(word, distance)| json!({ "word": word, "distance": distance }))
                .collect::<Vec<_>>();
            print_json(json!({
                "type": "finding",
                "kind": "spelling",
                "command": "analyze",
                "path": path.display().to_string(),
                "line": line,
                "column": column,
                "word": finding.word(),
                "suggestions": suggestions,
            }));
        }
    }
}

struct AnalysisSuggestionEngine<'source, S: ?Sized> {
    suggester: Suggester<'source, S>,
    scratch: SuggestScratch,
    output: Vec<Suggestion>,
    cache: HashMap<String, Vec<(String, usize)>>,
}

fn analysis_suggestion_engine(
    include_suggestions: bool,
    dictionary: &AnalysisDictionary,
) -> Option<AnalysisSuggestionEngine<'_, AnalysisDictionary>> {
    include_suggestions.then(|| AnalysisSuggestionEngine::new(dictionary))
}

impl<'source, S: CandidateSource + ?Sized> AnalysisSuggestionEngine<'source, S> {
    fn new(source: &'source S) -> Self {
        let config = SuggestConfig {
            max_results: 3,
            ..SuggestConfig::default()
        };
        Self {
            suggester: Suggester::new(source, config),
            scratch: SuggestScratch::default(),
            output: Vec::new(),
            cache: HashMap::new(),
        }
    }

    fn suggestions(&mut self, finding: &ferrolex_code::Finding<'_>) -> Vec<(String, usize)> {
        self.base_suggestions(finding.word())
            .into_iter()
            .map(|(word, distance)| {
                let replacement = finding.whole_identifier_suggestion(&word).unwrap_or(word);
                (replacement, distance)
            })
            .collect()
    }

    fn base_suggestions(&mut self, word: &str) -> Vec<(String, usize)> {
        if let Some(suggestions) = self.cache.get(word) {
            return suggestions.clone();
        }

        self.suggester
            .suggest_into(word, &mut self.output, &mut self.scratch);
        let suggestions = self
            .output
            .iter()
            .map(|suggestion| (suggestion.word().to_owned(), suggestion.distance()))
            .collect::<Vec<_>>();
        if self.cache.len() < MAX_ANALYSIS_SUGGESTION_CACHE_ENTRIES {
            self.cache.insert(word.to_owned(), suggestions.clone());
        }
        suggestions
    }
}

const fn directive_problem_code(problem: DirectiveProblem) -> &'static str {
    match problem {
        DirectiveProblem::MissingIgnoredWords => "missing-ignored-words",
        DirectiveProblem::UnexpectedArguments => "unexpected-arguments",
        DirectiveProblem::UnknownDirective => "unknown-directive",
        _ => "unsupported",
    }
}

fn validate(command: &ValidateCommand) -> Result<RunOutcome, CliError> {
    match command {
        ValidateCommand::Hunspell {
            strict,
            aff_path,
            dic_path,
            output_format,
        } => validate_hunspell(*strict, aff_path, dic_path, None, *output_format),
        ValidateCommand::Compiled {
            path,
            output_format,
        } => validate_compiled(path, *output_format),
    }
}

fn validate_hunspell(
    strict: bool,
    aff_path: &Path,
    dic_path: &Path,
    encodings: Option<ByteImportEncodings>,
    output_format: OutputFormat,
) -> Result<RunOutcome, CliError> {
    let (import, _) = import_hunspell_files(aff_path, dic_path, encodings, strict)?;
    Ok(report_hunspell_import(import, dic_path, output_format))
}

fn install_hunspell_runtime_cache(
    locale: &str,
    aff_path: &Path,
    dic_path: &Path,
    encodings: Option<ByteImportEncodings>,
) -> Result<RunOutcome, CliError> {
    eprintln!("importing Hunspell sources for {locale}...");
    let (import, sources) = import_hunspell_files(aff_path, dic_path, encodings, true)?;
    let result = match import {
        Ok(result) => result,
        Err(error) => {
            for diagnostic in error.diagnostics() {
                print_import_diagnostic(diagnostic);
            }
            return Ok(RunOutcome::Misspelled);
        }
    };
    for diagnostic in result.diagnostics() {
        print_import_diagnostic(diagnostic);
    }
    eprintln!("building runtime cache for {locale}...");
    let cache = compile_runtime_cache(result.dictionary(), sources)
        .map_err(CliError::CompileHunspellCache)?;
    let cache_path = runtime_cache_path(aff_path);
    atomic_write_runtime_cache(&cache_path, &cache)?;
    println!("valid: {}", dic_path.display());
    println!("runtime-cache: {}", cache_path.display());
    println!("ready: {locale}");
    Ok(RunOutcome::Success)
}

fn runtime_cache_path(aff_path: &Path) -> PathBuf {
    aff_path.with_extension(HUNSPELL_RUNTIME_CACHE_EXTENSION)
}

fn import_hunspell_files(
    aff_path: &Path,
    dic_path: &Path,
    encodings: Option<ByteImportEncodings>,
    strict: bool,
) -> Result<(Result<ImportResult, ImportError>, SourceDigests), CliError> {
    let aff_bytes = fs::read(aff_path).map_err(|source| CliError::ReadInput {
        path: aff_path.to_path_buf(),
        source,
    })?;
    let dic_bytes = fs::read(dic_path).map_err(|source| CliError::ReadInput {
        path: dic_path.to_path_buf(),
        source,
    })?;
    let mode = if strict {
        ImportMode::Strict
    } else {
        ImportMode::Lenient
    };
    let aff_source = aff_path.display().to_string();
    let dic_source = dic_path.display().to_string();

    let import = import_hunspell_source_bytes(
        &aff_source,
        &aff_bytes,
        &dic_source,
        &dic_bytes,
        encodings,
        mode,
    );

    Ok((
        import,
        SourceDigests::from_source_bytes(&aff_bytes, &dic_bytes),
    ))
}

fn import_hunspell_source_bytes(
    aff_source: &str,
    aff_bytes: &[u8],
    dic_source: &str,
    dic_bytes: &[u8],
    encodings: Option<ByteImportEncodings>,
    mode: ImportMode,
) -> Result<ImportResult, ImportError> {
    match encodings {
        Some(encodings) => import_hunspell_bytes_with_encodings(
            aff_source, aff_bytes, dic_source, dic_bytes, encodings, mode,
        ),
        None => import_hunspell_bytes(aff_source, aff_bytes, dic_source, dic_bytes, mode),
    }
}

fn report_hunspell_import(
    import: Result<ImportResult, ImportError>,
    dic_path: &Path,
    output_format: OutputFormat,
) -> RunOutcome {
    match import {
        Ok(result) => {
            let has_errors = result
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.severity() == Severity::Error);
            for diagnostic in result.diagnostics() {
                print_import_diagnostic_with_format(diagnostic, output_format);
            }
            if has_errors {
                print_validation_result(dic_path, false, output_format);
                RunOutcome::Misspelled
            } else {
                print_validation_result(dic_path, true, output_format);
                RunOutcome::Success
            }
        }
        Err(error) => {
            for diagnostic in error.diagnostics() {
                print_import_diagnostic_with_format(diagnostic, output_format);
            }
            print_validation_result(dic_path, false, output_format);
            RunOutcome::Misspelled
        }
    }
}

fn print_validation_result(path: &Path, valid: bool, output_format: OutputFormat) {
    match output_format {
        OutputFormat::Text if valid => println!("valid: {}", path.display()),
        OutputFormat::Text => {}
        OutputFormat::Json => print_json(json!({
            "type": "validation",
            "path": path.display().to_string(),
            "status": if valid { "valid" } else { "invalid" },
        })),
    }
}

fn atomic_write_runtime_cache(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    sweep_stale_temporary_siblings(path);
    let temporary = temporary_sibling(path);
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut created = false;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|source| CliError::WriteHunspellCache {
                path: temporary.clone(),
                source,
            })?;
        created = true;
        file.write_all(bytes)
            .map_err(|source| CliError::WriteHunspellCache {
                path: temporary.clone(),
                source,
            })?;
        file.sync_all()
            .map_err(|source| CliError::WriteHunspellCache {
                path: temporary.clone(),
                source,
            })?;
        fs::rename(&temporary, path).map_err(|source| CliError::WriteHunspellCache {
            path: path.to_path_buf(),
            source,
        })?;
        sync_parent_directory(parent).map_err(|source| CliError::WriteHunspellCache {
            path: parent.to_path_buf(),
            source,
        })
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn validate_compiled(path: &Path, output_format: OutputFormat) -> Result<RunOutcome, CliError> {
    match load_artifact(path)? {
        ArtifactDictionary::Exact(dictionary) => {
            dictionary
                .validate()
                .map_err(|source| CliError::ValidateArtifact {
                    path: path.to_path_buf(),
                    source,
                })?;
        }
        ArtifactDictionary::Hunspell(_) => {}
    }
    print_validation_result(path, true, output_format);
    Ok(RunOutcome::Success)
}

fn inspect_artifact(path: &Path) -> Result<RunOutcome, CliError> {
    let bytes = read_compiled_artifact(path)?;
    match inspect_compiled_artifact(&bytes) {
        Ok(metadata) => {
            println!("artifact: {}", path.display());
            println!("format: FLEXDIC");
            println!("format-version: {}", metadata.format_version());
            println!("source-metadata: not recorded (plain word-list artifact)");
            println!("required-features: exact-word-lookup");
            println!("feature-bits: {:#x}", metadata.feature_bits());
            println!("entries: {}", metadata.word_count());
        }
        Err(LoadError::InvalidMagic) => {
            let metadata =
                inspect_runtime_cache(&bytes).map_err(|source| CliError::LoadHunspellArtifact {
                    path: path.to_path_buf(),
                    source,
                })?;
            let sources = metadata.sources();
            println!("artifact: {}", path.display());
            println!("format: FLXHSP");
            println!("format-version: {}", metadata.format_version());
            println!("semantics-version: {}", metadata.semantics_version());
            println!("source-aff-sha256: {}", hex_digest(sources.aff()));
            println!("source-dic-sha256: {}", hex_digest(sources.dic()));
            println!("required-features: lexemes, prefixes, suffixes, cross-product, continuation-flags, conditions, special-flags, compounds, breaks, input-conversions, replacement-rules, output-conversions");
        }
        Err(source) => {
            return Err(CliError::LoadArtifact {
                path: path.to_path_buf(),
                source,
            });
        }
    }
    Ok(RunOutcome::Success)
}

fn hex_digest(digest: [u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}

fn read_compiled_artifact(path: &Path) -> Result<Vec<u8>, CliError> {
    let size = fs::metadata(path)
        .map_err(|source| CliError::ReadInput {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    if size > u64::try_from(MAX_COMPILED_ARTIFACT_BYTES).expect("usize fits u64") {
        return Err(CliError::ArtifactTooLarge {
            path: path.to_path_buf(),
            actual: size,
        });
    }
    fs::read(path).map_err(|source| CliError::ReadInput {
        path: path.to_path_buf(),
        source,
    })
}

fn print_import_diagnostic(diagnostic: &ImportDiagnostic) {
    println!("{}", render_import_diagnostic(diagnostic));
}

fn print_import_diagnostic_with_format(diagnostic: &ImportDiagnostic, output_format: OutputFormat) {
    match output_format {
        OutputFormat::Text => print_import_diagnostic(diagnostic),
        OutputFormat::Json => print_json(json!({
            "type": "diagnostic",
            "command": "validate",
            "source": diagnostic.source(),
            "line": diagnostic.line(),
            "directive": diagnostic.directive(),
            "severity": severity_code(diagnostic.severity()),
            "message": diagnostic.message(),
        })),
    }
}

const fn severity_code(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

fn print_import_diagnostic_to_stderr(diagnostic: &ImportDiagnostic) {
    eprintln!("{}", render_import_diagnostic(diagnostic));
}

fn render_import_diagnostic(diagnostic: &ImportDiagnostic) -> String {
    let severity = match diagnostic.severity() {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    format!(
        "{}:{}: {severity}[{}]: {}",
        diagnostic.source(),
        diagnostic.line(),
        diagnostic.directive(),
        diagnostic.message()
    )
}

fn print_finding(
    command: &str,
    output_format: OutputFormat,
    path: &Path,
    source: &str,
    line_index: &LineIndex,
    byte_offset: usize,
    word: &str,
) {
    let (line, column) = line_index.line_and_column(source, byte_offset);
    match output_format {
        OutputFormat::Text => {
            println!("{}:{line}:{column}: misspelled: {word}", path.display());
        }
        OutputFormat::Json => print_json(json!({
            "type": "finding",
            "kind": "spelling",
            "command": command,
            "path": path.display().to_string(),
            "line": line,
            "column": column,
            "word": word,
        })),
    }
}

fn print_json(value: impl fmt::Display) {
    println!("{value}");
}

struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter_map(|(offset, byte)| (byte == b'\n').then_some(offset + 1)),
        );
        Self { starts }
    }

    fn line_and_column(&self, text: &str, byte_offset: usize) -> (usize, usize) {
        let line_index = self.starts.partition_point(|start| *start <= byte_offset) - 1;
        let column = text[self.starts[line_index]..byte_offset].chars().count() + 1;
        (line_index + 1, column)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RunOutcome {
    Success,
    Misspelled,
    Failure,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum OutputFormat {
    #[default]
    Text,
    Json,
}

impl RunOutcome {
    fn exit_code(self) -> ExitCode {
        match self {
            Self::Success => ExitCode::SUCCESS,
            Self::Misspelled => ExitCode::from(1),
            Self::Failure => ExitCode::from(RUNTIME_ERROR_EXIT_CODE),
        }
    }
}

fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Command, CliError> {
    let mut arguments = arguments.into_iter();
    let _program_name = arguments.next();

    match arguments.next().as_deref() {
        Some("--help" | "-h") => Ok(Command::Help(USAGE)),
        Some("--version" | "-V") if arguments.next().is_none() => Ok(Command::Version),
        Some("--version" | "-V") => Err(CliError::Usage(
            "`--version` does not accept arguments".to_owned(),
        )),
        Some("check") => parse_check_or_help(expand_long_option_values(arguments)),
        Some("suggest") => parse_suggest_or_help(expand_long_option_values(arguments)),
        Some("explain") => parse_explain_or_help(expand_long_option_values(arguments)),
        Some("analyze") => parse_analyze_or_help(expand_long_option_values(arguments)),
        Some("compile") => parse_compile_or_help(expand_long_option_values(arguments)),
        Some("inspect") => parse_inspect_or_help(expand_long_option_values(arguments)),
        Some("validate") => parse_validate_or_help(expand_long_option_values(arguments)),
        Some("dictionary") => parse_dictionary_or_help(expand_long_option_values(arguments)),
        Some(command) => Err(CliError::Usage(format!("unknown command `{command}`"))),
        None => Err(CliError::Usage("missing command".to_owned())),
    }
}

fn requests_help(arguments: &[String]) -> bool {
    let mut arguments = arguments.iter();

    while let Some(argument) = arguments.next() {
        if argument == "--" {
            break;
        } else if value_option(argument) {
            arguments.next();
        } else if matches!(argument.as_str(), "--help" | "-h") {
            return true;
        }
    }

    false
}

fn value_option(argument: &str) -> bool {
    matches!(
        argument,
        "--dictionary"
            | "--compiled"
            | "--hunspell"
            | "--file"
            | "--format"
            | "--max-results"
            | "--max-edit-distance"
            | "--max-candidates"
            | "--max-edit-cells"
            | "--config"
            | "--include"
            | "--exclude"
            | "--comment-prefix"
            | "--comment-syntax"
            | "--workspace"
            | "--cache"
            | "-o"
    )
}

macro_rules! parse_or_help {
    ($name:ident, $parser:ident, $help:ident) => {
        fn $name(arguments: Vec<String>) -> Result<Command, CliError> {
            if requests_help(&arguments) {
                Ok(Command::Help($help))
            } else {
                $parser(arguments)
            }
        }
    };
}

parse_or_help!(parse_check_or_help, parse_check_arguments, HELP_CHECK);
parse_or_help!(parse_suggest_or_help, parse_suggest_arguments, HELP_SUGGEST);
parse_or_help!(parse_explain_or_help, parse_explain_arguments, HELP_EXPLAIN);
parse_or_help!(parse_analyze_or_help, parse_analyze_arguments, HELP_ANALYZE);
parse_or_help!(parse_compile_or_help, parse_compile_arguments, HELP_COMPILE);
parse_or_help!(parse_inspect_or_help, parse_inspect_arguments, HELP_INSPECT);
parse_or_help!(
    parse_validate_or_help,
    parse_validate_arguments,
    HELP_VALIDATE
);
parse_or_help!(
    parse_dictionary_or_help,
    parse_dictionary_arguments,
    HELP_DICTIONARY
);

fn parse_explain_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Command, CliError> {
    let mut hunspell_affix_path = None;
    let mut word = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--hunspell" => set_once_path(&mut hunspell_affix_path, &mut arguments, "--hunspell")?,
            "--help" | "-h" => return Ok(Command::Help(USAGE)),
            option if option.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{option}`")));
            }
            _ if word.is_some() => {
                return Err(CliError::Usage(
                    "explain accepts exactly one word".to_owned(),
                ));
            }
            _ => word = Some(argument),
        }
    }
    let hunspell_affix_path = hunspell_affix_path.ok_or_else(|| {
        CliError::Usage("explain requires exactly one `--hunspell` path".to_owned())
    })?;
    let word =
        word.ok_or_else(|| CliError::Usage("explain requires exactly one word".to_owned()))?;
    Ok(Command::Explain(ExplainCommand {
        hunspell_affix_path,
        word,
    }))
}

fn parse_inspect_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Command, CliError> {
    let mut arguments = arguments.into_iter();
    let Some(path) = arguments.next() else {
        return Err(CliError::Usage(
            "inspect requires an artifact path".to_owned(),
        ));
    };
    if path == "--help" || path == "-h" {
        return Ok(Command::Help(USAGE));
    }
    if path.starts_with('-') || arguments.next().is_some() {
        return Err(CliError::Usage(
            "inspect accepts exactly one artifact path".to_owned(),
        ));
    }
    Ok(Command::Inspect(PathBuf::from(path)))
}

fn parse_suggest_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Command, CliError> {
    let mut dictionary_paths = Vec::new();
    let mut compiled_paths = Vec::new();
    let mut hunspell_affix_paths = Vec::new();
    let mut max_results = None;
    let mut max_edit_distance = None;
    let mut max_candidates = None;
    let mut max_edit_cells = None;
    let mut output_format = None;
    let mut word = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--dictionary" => {
                dictionary_paths.push(required_path(&mut arguments, "--dictionary")?);
            }
            "--hunspell" => {
                hunspell_affix_paths.push(required_path(&mut arguments, "--hunspell")?);
            }
            "--compiled" => compiled_paths.push(required_path(&mut arguments, "--compiled")?),
            "--format" => {
                set_once_output_format(&mut output_format, &mut arguments, "--format")?;
            }
            "--max-results" => {
                set_once_usize(&mut max_results, &mut arguments, "--max-results", true)?;
            }
            "--max-edit-distance" => {
                set_once_usize(
                    &mut max_edit_distance,
                    &mut arguments,
                    "--max-edit-distance",
                    false,
                )?;
            }
            "--max-candidates" => {
                set_once_usize(
                    &mut max_candidates,
                    &mut arguments,
                    "--max-candidates",
                    true,
                )?;
            }
            "--max-edit-cells" => {
                set_once_usize(
                    &mut max_edit_cells,
                    &mut arguments,
                    "--max-edit-cells",
                    true,
                )?;
            }
            "--help" | "-h" => return Ok(Command::Help(USAGE)),
            option if option.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{option}`")));
            }
            _ if word.is_some() => {
                return Err(CliError::Usage(
                    "suggest accepts exactly one word".to_owned(),
                ));
            }
            _ => word = Some(argument),
        }
    }
    let word =
        word.ok_or_else(|| CliError::Usage("suggest requires exactly one word".to_owned()))?;
    Ok(Command::Suggest(SuggestCommand {
        dictionary_paths,
        compiled_paths,
        hunspell_affix_paths,
        max_results,
        max_edit_distance,
        max_candidates,
        max_edit_cells,
        output_format: output_format.unwrap_or_default(),
        word,
    }))
}

fn parse_dictionary_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Command, CliError> {
    let mut arguments = arguments.into_iter();
    match arguments.next().as_deref() {
        Some("list") => {
            if arguments.next().is_some() {
                return Err(CliError::Usage(
                    "dictionary list does not accept arguments".to_owned(),
                ));
            }
            Ok(Command::Dictionary(DictionaryCommand::List))
        }
        Some("fetch") => parse_dictionary_catalog_arguments(arguments, "fetch"),
        Some("install") => parse_dictionary_catalog_arguments(arguments, "install"),
        Some("add-word") => parse_add_word_arguments(arguments),
        Some("--help" | "-h") => Ok(Command::Help(HELP_DICTIONARY)),
        Some(subcommand) => Err(CliError::Usage(format!(
            "unknown dictionary subcommand `{subcommand}`"
        ))),
        None => Err(CliError::Usage(
            "dictionary requires `list`, `fetch`, `install`, or `add-word`".to_owned(),
        )),
    }
}

fn parse_add_word_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Command, CliError> {
    let mut workspace = None;
    let mut global = false;
    let mut word = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--workspace" => set_once_path(&mut workspace, &mut arguments, "--workspace")?,
            "--global" => global = true,
            "--help" | "-h" => return Ok(Command::Help(USAGE)),
            option if option.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{option}`")))
            }
            _ => {
                if word.replace(argument).is_some() {
                    return Err(CliError::Usage(
                        "dictionary add-word accepts exactly one word".to_owned(),
                    ));
                }
            }
        }
    }
    if global && workspace.is_some() {
        return Err(CliError::Usage(
            "choose either `--workspace` or `--global`".to_owned(),
        ));
    }
    let word =
        word.ok_or_else(|| CliError::Usage("dictionary add-word requires one word".to_owned()))?;
    let path = if global {
        global_user_dictionary_path()?
    } else {
        workspace
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".ferrolex/words.txt")
    };
    Ok(Command::Dictionary(DictionaryCommand::AddWord {
        word,
        path,
    }))
}

fn global_user_dictionary_path() -> Result<PathBuf, CliError> {
    if let Some(directory) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(directory).join("ferrolex/words.txt"));
    }
    env::var_os("HOME")
        .map(|directory| PathBuf::from(directory).join(".config/ferrolex/words.txt"))
        .ok_or_else(|| CliError::Usage("`--global` requires HOME or XDG_CONFIG_HOME".to_owned()))
}

fn load_user_dictionaries() -> Result<Vec<WordList>, CliError> {
    let mut paths = vec![PathBuf::from(".ferrolex/words.txt")];
    if let Ok(global_path) = global_user_dictionary_path() {
        if global_path != paths[0] {
            paths.push(global_path);
        }
    }

    let mut dictionaries = Vec::new();
    for path in paths {
        match fs::read_to_string(&path) {
            Ok(text) => dictionaries.push(WordList::from_text(Normalization::Nfc, &text)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(CliError::ReadDictionary { path, source }),
        }
    }
    Ok(dictionaries)
}

fn parse_dictionary_catalog_arguments(
    arguments: impl IntoIterator<Item = String>,
    subcommand: &str,
) -> Result<Command, CliError> {
    let mut locale = None;
    let mut cache_path = None;
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--cache" => set_once_path(&mut cache_path, &mut arguments, "--cache")?,
            "--help" | "-h" => return Ok(Command::Help(USAGE)),
            option if option.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{option}`")));
            }
            _ => {
                if locale.replace(argument).is_some() {
                    return Err(CliError::Usage(format!(
                        "dictionary {subcommand} accepts exactly one locale"
                    )));
                }
            }
        }
    }

    let locale = locale.ok_or_else(|| {
        CliError::Usage(format!(
            "dictionary {subcommand} requires exactly one locale"
        ))
    })?;
    let cache_path = cache_path
        .ok_or_else(|| CliError::Usage(format!("dictionary {subcommand} requires `--cache`")))?;
    let command = match subcommand {
        "fetch" => DictionaryCommand::Fetch { locale, cache_path },
        "install" => DictionaryCommand::Install { locale, cache_path },
        _ => {
            return Err(CliError::Usage(format!(
                "unknown dictionary subcommand `{subcommand}`"
            )))
        }
    };
    Ok(Command::Dictionary(command))
}

fn parse_validate_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Command, CliError> {
    let mut strict = false;
    let mut compiled_path = None;
    let mut output_format = None;
    let mut paths = Vec::new();
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--strict" => strict = true,
            "--compiled" => {
                let path = required_path(&mut arguments, "--compiled")?;
                if compiled_path.replace(path).is_some() {
                    return Err(CliError::Usage(
                        "`--compiled` may only be supplied once".to_owned(),
                    ));
                }
            }
            "--format" => {
                set_once_output_format(&mut output_format, &mut arguments, "--format")?;
            }
            "--help" | "-h" => return Ok(Command::Help(USAGE)),
            option if option.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{option}`")));
            }
            _ => paths.push(PathBuf::from(argument)),
        }
    }
    if let Some(path) = compiled_path {
        if strict || !paths.is_empty() {
            return Err(CliError::Usage(
                "`validate --compiled` accepts only one compiled artifact path".to_owned(),
            ));
        }
        return Ok(Command::Validate(ValidateCommand::Compiled {
            path,
            output_format: output_format.unwrap_or_default(),
        }));
    }
    if paths.len() != 2 {
        return Err(CliError::Usage(
            "validate requires exactly an AFF path and a DIC path".to_owned(),
        ));
    }

    Ok(Command::Validate(ValidateCommand::Hunspell {
        strict,
        aff_path: paths.remove(0),
        dic_path: paths.remove(0),
        output_format: output_format.unwrap_or_default(),
    }))
}

fn parse_compile_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Command, CliError> {
    let mut dictionary_path = None;
    let mut output_path = None;
    let mut paths = Vec::new();
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--dictionary" => {
                let path = required_path(&mut arguments, "--dictionary")?;
                if dictionary_path.replace(path).is_some() {
                    return Err(CliError::Usage(
                        "`compile` accepts exactly one `--dictionary` path".to_owned(),
                    ));
                }
            }
            "-o" => {
                let path = required_path(&mut arguments, "-o")?;
                if output_path.replace(path).is_some() {
                    return Err(CliError::Usage(
                        "`compile` accepts exactly one `-o` path".to_owned(),
                    ));
                }
            }
            "--help" | "-h" => return Ok(Command::Help(USAGE)),
            option if option.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{option}`")));
            }
            _ => paths.push(PathBuf::from(argument)),
        }
    }

    let output_path = output_path
        .ok_or_else(|| CliError::Usage("compile requires an `-o` artifact path".to_owned()))?;
    let input = match (dictionary_path, paths.as_slice()) {
        (Some(path), []) => CompileInput::WordList(path),
        (None, [aff_path, dic_path]) => CompileInput::Hunspell {
            aff_path: aff_path.clone(),
            dic_path: dic_path.clone(),
        },
        (Some(_), _) => {
            return Err(CliError::Usage(
                "compile accepts either `--dictionary` or exactly an AFF and DIC path".to_owned(),
            ));
        }
        (None, _) => {
            return Err(CliError::Usage(
                "compile requires a `--dictionary` path or exactly an AFF and DIC path".to_owned(),
            ));
        }
    };

    Ok(Command::Compile(CompileCommand { input, output_path }))
}

fn parse_analyze_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Command, CliError> {
    let mut dictionary_paths = Vec::new();
    let mut compiled_paths = Vec::new();
    let mut hunspell_affix_paths = Vec::new();
    let mut config_path = None;
    let mut comment_syntax = None;
    let mut include_patterns = Vec::new();
    let mut exclude_patterns = Vec::new();
    let mut suggest = false;
    let mut output_format = None;
    let mut path = None;
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--dictionary" => {
                let dictionary_path = required_path(&mut arguments, "--dictionary")?;
                dictionary_paths.push(dictionary_path);
            }
            "--hunspell" => {
                hunspell_affix_paths.push(required_path(&mut arguments, "--hunspell")?);
            }
            "--compiled" => compiled_paths.push(required_path(&mut arguments, "--compiled")?),
            "--format" => {
                set_once_output_format(&mut output_format, &mut arguments, "--format")?;
            }
            "--config" => set_once_path(&mut config_path, &mut arguments, "--config")?,
            "--include" => include_patterns.push(required_string(&mut arguments, "--include")?),
            "--exclude" => exclude_patterns.push(required_string(&mut arguments, "--exclude")?),
            "--suggest" => suggest = true,
            "--comment-prefix" => {
                let prefix = arguments.next().ok_or_else(|| {
                    CliError::Usage("`--comment-prefix` requires a prefix".to_owned())
                })?;
                set_comment_prefix(&mut comment_syntax, prefix)?;
            }
            option if option.starts_with("--comment-prefix=") => {
                let prefix = option
                    .strip_prefix("--comment-prefix=")
                    .expect("option was matched by its prefix");
                set_comment_prefix(&mut comment_syntax, prefix)?;
            }
            "--comment-syntax" => {
                let syntax = arguments.next().ok_or_else(|| {
                    CliError::Usage("`--comment-syntax` requires a syntax".to_owned())
                })?;
                set_comment_syntax(&mut comment_syntax, parse_comment_syntax(&syntax)?)?;
            }
            "--help" | "-h" => return Ok(Command::Help(USAGE)),
            option if option.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{option}`")));
            }
            _ if path.is_some() => {
                return Err(CliError::Usage(
                    "analyze accepts exactly one path".to_owned(),
                ));
            }
            _ => path = Some(PathBuf::from(argument)),
        }
    }

    let path = path.ok_or_else(|| CliError::Usage("analyze requires a path".to_owned()))?;

    Ok(Command::Analyze(AnalyzeCommand {
        dictionary_paths,
        compiled_paths,
        hunspell_affix_paths,
        config_path,
        comment_syntax,
        include_patterns,
        exclude_patterns,
        suggest,
        output_format: output_format.unwrap_or_default(),
        path,
    }))
}

fn set_comment_prefix(
    slot: &mut Option<CommentSyntax>,
    prefix: impl Into<String>,
) -> Result<(), CliError> {
    let prefix = prefix.into();
    if prefix.is_empty() {
        return Err(CliError::Usage(
            "`--comment-prefix` requires a non-empty prefix".to_owned(),
        ));
    }
    set_comment_syntax(slot, CommentSyntax::line(prefix))
}

fn parse_comment_syntax(syntax: &str) -> Result<CommentSyntax, CliError> {
    match syntax {
        "html" => Ok(CommentSyntax::Html),
        _ => Err(CliError::Usage(
            "`--comment-syntax` supports only `html`".to_owned(),
        )),
    }
}

fn set_comment_syntax(
    slot: &mut Option<CommentSyntax>,
    syntax: CommentSyntax,
) -> Result<(), CliError> {
    if slot.replace(syntax).is_some() {
        return Err(CliError::Usage(
            "only one comment syntax may be supplied".to_owned(),
        ));
    }
    Ok(())
}

fn parse_check_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Command, CliError> {
    let mut dictionary_paths = Vec::new();
    let mut compiled_paths = Vec::new();
    let mut hunspell_affix_paths = Vec::new();
    let mut target = None;
    let mut output_format = None;
    let mut arguments = arguments.into_iter();
    let mut options_ended = false;

    while let Some(argument) = arguments.next() {
        if options_ended {
            push_check_positional(&mut target, argument)?;
            continue;
        }

        match argument.as_str() {
            "--dictionary" => {
                dictionary_paths.push(required_path(&mut arguments, "--dictionary")?);
            }
            "--hunspell" => {
                hunspell_affix_paths.push(required_path(&mut arguments, "--hunspell")?);
            }
            "--compiled" => compiled_paths.push(required_path(&mut arguments, "--compiled")?),
            "--format" => {
                set_once_output_format(&mut output_format, &mut arguments, "--format")?;
            }
            "--file" => {
                push_check_input(&mut target, required_check_input(&mut arguments)?)?;
            }
            "--" => options_ended = true,
            "--help" | "-h" => return Ok(Command::Help(USAGE)),
            option if option.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{option}`")));
            }
            _ => push_check_positional(&mut target, argument)?,
        }
    }

    let target =
        target.ok_or_else(|| CliError::Usage("check requires a word or `--file`".to_owned()))?;

    Ok(Command::Check(CheckCommand {
        dictionary_paths,
        compiled_paths,
        hunspell_affix_paths,
        output_format: output_format.unwrap_or_default(),
        target,
    }))
}

fn required_check_input(
    arguments: &mut impl Iterator<Item = String>,
) -> Result<CheckInput, CliError> {
    let path = arguments
        .next()
        .ok_or_else(|| CliError::Usage("`--file` requires a path or `-`".to_owned()))?;
    if path == "-" {
        return Ok(CheckInput::Stdin);
    }
    if path.is_empty() || path.starts_with('-') {
        return Err(CliError::Usage(
            "`--file` requires a path or `-`".to_owned(),
        ));
    }

    Ok(CheckInput::File(PathBuf::from(path)))
}

fn required_path(
    arguments: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<PathBuf, CliError> {
    let path = arguments
        .next()
        .ok_or_else(|| CliError::Usage(format!("`{option}` requires a path")))?;
    if path.is_empty() || path.starts_with('-') {
        return Err(CliError::Usage(format!("`{option}` requires a path")));
    }

    Ok(PathBuf::from(path))
}

fn expand_long_option_values(arguments: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut options_ended = false;
    arguments
        .into_iter()
        .flat_map(|argument| {
            if options_ended {
                return vec![argument];
            }
            if argument == "--" {
                options_ended = true;
                return vec![argument];
            }
            let Some((option, value)) = argument.split_once('=') else {
                return vec![argument];
            };
            if matches!(
                option,
                "--dictionary"
                    | "--hunspell"
                    | "--compiled"
                    | "--file"
                    | "--format"
                    | "--max-results"
                    | "--max-edit-distance"
                    | "--max-candidates"
                    | "--max-edit-cells"
                    | "--workspace"
                    | "--cache"
                    | "--config"
                    | "--include"
                    | "--exclude"
                    | "--comment-prefix"
                    | "--comment-syntax"
            ) {
                vec![option.to_owned(), value.to_owned()]
            } else {
                vec![argument]
            }
        })
        .collect()
}

fn required_string(
    arguments: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<String, CliError> {
    let value = arguments
        .next()
        .ok_or_else(|| CliError::Usage(format!("`{option}` requires a value")))?;
    if value.is_empty() || value.starts_with('-') {
        return Err(CliError::Usage(format!("`{option}` requires a value")));
    }
    Ok(value)
}

fn set_once_output_format(
    destination: &mut Option<OutputFormat>,
    arguments: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<(), CliError> {
    let value = required_string(arguments, option)?;
    let format = match value.as_str() {
        "text" => OutputFormat::Text,
        "json" => OutputFormat::Json,
        _ => {
            return Err(CliError::Usage(format!(
                "`{option}` supports only `text` or `json`"
            )))
        }
    };
    if destination.replace(format).is_some() {
        return Err(CliError::Usage(format!(
            "`{option}` may only be supplied once"
        )));
    }
    Ok(())
}

fn set_once_path(
    destination: &mut Option<PathBuf>,
    arguments: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<(), CliError> {
    let value = required_path(arguments, option)?;
    if destination.replace(value).is_some() {
        return Err(CliError::Usage(format!(
            "`{option}` may only be supplied once"
        )));
    }
    Ok(())
}

fn set_once_usize(
    destination: &mut Option<usize>,
    arguments: &mut impl Iterator<Item = String>,
    option: &str,
    must_be_positive: bool,
) -> Result<(), CliError> {
    let value = arguments
        .next()
        .ok_or_else(|| CliError::Usage(format!("`{option}` requires an integer")))?;
    let value = value
        .parse::<usize>()
        .map_err(|_| CliError::Usage(format!("`{option}` requires a non-negative integer")))?;
    if must_be_positive && value == 0 {
        return Err(CliError::Usage(format!(
            "`{option}` requires a positive integer"
        )));
    }
    if destination.replace(value).is_some() {
        return Err(CliError::Usage(format!(
            "`{option}` may only be supplied once"
        )));
    }
    Ok(())
}

fn push_check_input(target: &mut Option<CheckTarget>, input: CheckInput) -> Result<(), CliError> {
    match target {
        None => *target = Some(CheckTarget::Inputs(vec![input])),
        Some(CheckTarget::Inputs(inputs)) => {
            if input == CheckInput::Stdin && inputs.contains(&CheckInput::Stdin) {
                return Err(CliError::Usage(
                    "stdin (`--file -`) may only be supplied once".to_owned(),
                ));
            }
            inputs.push(input);
        }
        Some(CheckTarget::Word(_)) => {
            return Err(CliError::Usage(
                "check cannot mix a word with file inputs".to_owned(),
            ));
        }
    }

    Ok(())
}

fn push_check_positional(target: &mut Option<CheckTarget>, value: String) -> Result<(), CliError> {
    match target {
        None => *target = Some(CheckTarget::Word(value)),
        Some(CheckTarget::Inputs(_)) => {
            push_check_input(target, CheckInput::File(PathBuf::from(value)))?;
        }
        Some(CheckTarget::Word(_)) => {
            return Err(CliError::Usage(
                "check accepts one word, or one or more file inputs".to_owned(),
            ));
        }
    }

    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Check(CheckCommand),
    Suggest(SuggestCommand),
    Explain(ExplainCommand),
    Analyze(AnalyzeCommand),
    Compile(CompileCommand),
    Inspect(PathBuf),
    Validate(ValidateCommand),
    Dictionary(DictionaryCommand),
    Help(&'static str),
    Version,
}

#[derive(Debug, Eq, PartialEq)]
struct CheckCommand {
    dictionary_paths: Vec<PathBuf>,
    compiled_paths: Vec<PathBuf>,
    hunspell_affix_paths: Vec<PathBuf>,
    output_format: OutputFormat,
    target: CheckTarget,
}

#[derive(Debug, Eq, PartialEq)]
struct SuggestCommand {
    dictionary_paths: Vec<PathBuf>,
    compiled_paths: Vec<PathBuf>,
    hunspell_affix_paths: Vec<PathBuf>,
    max_results: Option<usize>,
    max_edit_distance: Option<usize>,
    max_candidates: Option<usize>,
    max_edit_cells: Option<usize>,
    output_format: OutputFormat,
    word: String,
}

#[derive(Debug, Eq, PartialEq)]
struct ExplainCommand {
    hunspell_affix_path: PathBuf,
    word: String,
}

#[derive(Debug, Eq, PartialEq)]
enum CheckTarget {
    Word(String),
    Inputs(Vec<CheckInput>),
}

#[derive(Debug, Eq, PartialEq)]
enum CheckInput {
    File(PathBuf),
    Stdin,
}

#[derive(Debug, Eq, PartialEq)]
struct AnalyzeCommand {
    dictionary_paths: Vec<PathBuf>,
    compiled_paths: Vec<PathBuf>,
    hunspell_affix_paths: Vec<PathBuf>,
    config_path: Option<PathBuf>,
    comment_syntax: Option<CommentSyntax>,
    include_patterns: Vec<String>,
    exclude_patterns: Vec<String>,
    suggest: bool,
    output_format: OutputFormat,
    path: PathBuf,
}

#[derive(Debug, Eq, PartialEq)]
struct CompileCommand {
    input: CompileInput,
    output_path: PathBuf,
}

#[derive(Debug, Eq, PartialEq)]
enum CompileInput {
    WordList(PathBuf),
    Hunspell {
        aff_path: PathBuf,
        dic_path: PathBuf,
    },
}

#[derive(Debug, Eq, PartialEq)]
enum ValidateCommand {
    Hunspell {
        strict: bool,
        aff_path: PathBuf,
        dic_path: PathBuf,
        output_format: OutputFormat,
    },
    Compiled {
        path: PathBuf,
        output_format: OutputFormat,
    },
}

#[derive(Debug, Eq, PartialEq)]
enum DictionaryCommand {
    List,
    Fetch { locale: String, cache_path: PathBuf },
    Install { locale: String, cache_path: PathBuf },
    AddWord { word: String, path: PathBuf },
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    ReadDictionary {
        path: PathBuf,
        source: io::Error,
    },
    ReadInput {
        path: PathBuf,
        source: io::Error,
    },
    ArtifactTooLarge {
        path: PathBuf,
        actual: u64,
    },
    ReadHunspellCache {
        path: PathBuf,
        source: io::Error,
    },
    ImportHunspellSources {
        aff_path: PathBuf,
        dic_path: PathBuf,
        source: ImportError,
    },
    WriteArtifact {
        path: PathBuf,
        source: io::Error,
    },
    CompileDictionary(CompileError),
    CompileFrequencyList(FrequencyListError),
    LoadArtifact {
        path: PathBuf,
        source: LoadError,
    },
    LoadHunspellArtifact {
        path: PathBuf,
        source: RuntimeCacheError,
    },
    ValidateArtifact {
        path: PathBuf,
        source: ValidationError,
    },
    CompileHunspellCache(RuntimeCacheError),
    LoadHunspellCache {
        path: PathBuf,
        source: RuntimeCacheError,
    },
    WriteHunspellCache {
        path: PathBuf,
        source: io::Error,
    },
    WriteUserDictionary {
        path: PathBuf,
        source: io::Error,
    },
    InvalidUserWord(ferrolex_core::WordListError),
    ReadProjectConfig {
        path: PathBuf,
        source: io::Error,
    },
    ProjectConfig {
        path: PathBuf,
        source: ProjectConfigError,
    },
    ApplyProjectConfig {
        path: PathBuf,
        source: AnalyzerConfigError,
    },
    DictionaryManifest(DictionaryManifestError),
    FetchDictionary(DictionaryFetchError),
}

impl CliError {
    const fn is_usage(&self) -> bool {
        matches!(self, Self::Usage(_))
    }
}

impl fmt::Display for CliError {
    #[allow(
        clippy::too_many_lines,
        reason = "each error variant keeps its path-aware diagnostic adjacent to its definition"
    )]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => formatter.write_str(message),
            Self::ReadDictionary { path, source } => {
                write!(
                    formatter,
                    "could not read dictionary `{}`: {source}",
                    path.display()
                )
            }
            Self::ReadInput { path, source } => {
                write!(
                    formatter,
                    "could not read input `{}`: {source}",
                    path.display()
                )
            }
            Self::ArtifactTooLarge { path, actual } => write!(
                formatter,
                "compiled artifact `{}` is {actual} bytes and exceeds the {} MiB runtime limit",
                path.display(),
                MAX_COMPILED_ARTIFACT_BYTES / (1024 * 1024)
            ),
            Self::ReadHunspellCache { path, source } => {
                write!(
                    formatter,
                    "could not read Hunspell runtime cache `{}`: {source}; rerun `ferrolex dictionary install` for catalog sources, or compile the AFF/DIC pair and use `--compiled` when the source directory is read-only",
                    path.display()
                )
            }
            Self::ImportHunspellSources {
                aff_path,
                dic_path,
                source,
            } => write!(
                formatter,
                "could not strictly import Hunspell sources `{}` and `{}` without a runtime cache: {source}; run `ferrolex validate --strict` on the pair before compiling it in a writable directory and using `--compiled`",
                aff_path.display(),
                dic_path.display()
            ),
            Self::WriteArtifact { path, source } => {
                write!(
                    formatter,
                    "could not write artifact `{}`: {source}",
                    path.display()
                )
            }
            Self::CompileDictionary(source) => {
                write!(formatter, "could not compile dictionary: {source}")
            }
            Self::CompileFrequencyList(source) => {
                write!(formatter, "could not compile frequency word list: {source}")
            }
            Self::LoadArtifact { path, source } => {
                write!(
                    formatter,
                    "invalid compiled artifact `{}`: {source}",
                    path.display()
                )
            }
            Self::LoadHunspellArtifact { path, source } => {
                write!(
                    formatter,
                    "invalid standalone Hunspell artifact `{}`: {source}",
                    path.display()
                )
            }
            Self::ValidateArtifact { path, source } => {
                write!(
                    formatter,
                    "invalid compiled artifact `{}`: {source}",
                    path.display()
                )
            }
            Self::CompileHunspellCache(source) => {
                write!(
                    formatter,
                    "could not compile Hunspell runtime cache: {source}"
                )
            }
            Self::LoadHunspellCache { path, source } => {
                write!(
                    formatter,
                    "invalid or stale Hunspell runtime cache `{}`: {source}; rerun `ferrolex dictionary install` for catalog sources, or compile the AFF/DIC pair and use `--compiled` when the source directory is read-only",
                    path.display()
                )
            }
            Self::WriteHunspellCache { path, source } => {
                write!(
                    formatter,
                    "could not atomically write Hunspell runtime cache `{}`: {source}",
                    path.display()
                )
            }
            Self::WriteUserDictionary { path, source } => {
                write!(
                    formatter,
                    "could not atomically write user dictionary `{}`: {source}",
                    path.display()
                )
            }
            Self::InvalidUserWord(source) => {
                write!(formatter, "invalid user dictionary word: {source}")
            }
            Self::ReadProjectConfig { path, source } => {
                write!(
                    formatter,
                    "could not read project config `{}`: {source}",
                    path.display()
                )
            }
            Self::ProjectConfig { path, source } => {
                write!(
                    formatter,
                    "invalid project config `{}`: {source}",
                    path.display()
                )
            }
            Self::ApplyProjectConfig { path, source } => {
                write!(
                    formatter,
                    "could not apply project config `{}`: {source}",
                    path.display()
                )
            }
            Self::DictionaryManifest(source) => {
                write!(formatter, "invalid dictionary review manifest: {source}")
            }
            Self::FetchDictionary(source) => {
                write!(formatter, "could not fetch dictionary: {source}")
            }
        }
    }
}

impl Error for CliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Usage(_) | Self::ArtifactTooLarge { .. } => None,
            Self::ReadDictionary { source, .. }
            | Self::ReadInput { source, .. }
            | Self::ReadHunspellCache { source, .. }
            | Self::WriteArtifact { source, .. }
            | Self::WriteHunspellCache { source, .. }
            | Self::WriteUserDictionary { source, .. }
            | Self::ReadProjectConfig { source, .. } => Some(source),
            Self::CompileDictionary(source) => Some(source),
            Self::CompileFrequencyList(source) => Some(source),
            Self::ImportHunspellSources { source, .. } => Some(source),
            Self::LoadArtifact { source, .. } => Some(source),
            Self::LoadHunspellArtifact { source, .. } => Some(source),
            Self::ValidateArtifact { source, .. } => Some(source),
            Self::CompileHunspellCache(source) | Self::LoadHunspellCache { source, .. } => {
                Some(source)
            }
            Self::ProjectConfig { source, .. } => Some(source),
            Self::ApplyProjectConfig { source, .. } => Some(source),
            Self::DictionaryManifest(source) => Some(source),
            Self::FetchDictionary(source) => Some(source),
            Self::InvalidUserWord(source) => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::{Duration, SystemTime};

    use ferrolex_compiler::{CompiledDictionary, ValidationError, MAX_COMPILED_ARTIFACT_BYTES};
    use ferrolex_core::Dictionary;
    use ferrolex_hunspell::{
        import, load_runtime_cache, CacheSource, ImportMode, RuntimeCacheError, SourceDigests,
    };

    use super::{
        add_user_dictionary_word, analysis_paths, analyze, catalog_import_encodings,
        comment_syntax_for_path, glob_matches, hidden_sibling, incomplete_suggestion_hint,
        install_hunspell_runtime_cache, load_analysis_dictionary, parse_arguments,
        read_analysis_source, read_compiled_artifact, render_explanation, run, runtime_cache_path,
        validate_hunspell, AnalysisDictionary, AnalysisSource, AnalysisSuggestionEngine,
        AnalyzeCommand, Analyzer, CandidateSource, CheckCommand, CheckInput, CheckTarget, CliError,
        Command, CommentSyntax, CompileCommand, CompileInput, DictionaryCommand, Document,
        ExplainCommand, LineIndex, Normalization, OutputFormat, RunOutcome, SourceEncoding,
        SuggestCommand, SuggestConfig, UserDictionaryLock, ValidateCommand, WordList, HELP_CHECK,
        STALE_TEMPORARY_FILE_AGE,
    };

    static NEXT_TEMPORARY_FILE: AtomicUsize = AtomicUsize::new(0);

    struct CountingCandidateSource {
        visits: AtomicUsize,
    }

    impl CandidateSource for CountingCandidateSource {
        fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
            self.visits.fetch_add(1, Ordering::Relaxed);
            visitor("receive");
        }
    }

    #[test]
    fn parses_repeated_dictionary_options() {
        let command = parse_arguments(
            [
                "ferrolex",
                "check",
                "--dictionary",
                "en.txt",
                "--dictionary",
                "technical.txt",
                "OAuth",
            ]
            .map(str::to_owned),
        )
        .expect("the command is valid");

        assert_eq!(
            command,
            Command::Check(CheckCommand {
                dictionary_paths: vec![PathBuf::from("en.txt"), PathBuf::from("technical.txt")],
                compiled_paths: Vec::new(),
                hunspell_affix_paths: Vec::new(),
                output_format: OutputFormat::Text,
                target: CheckTarget::Word("OAuth".to_owned()),
            })
        );
    }

    #[test]
    fn parses_repeated_and_positional_file_inputs() {
        let command = parse_arguments(
            [
                "ferrolex",
                "check",
                "--dictionary",
                "words.txt",
                "--file",
                "first.txt",
                "--file=-",
                "second.txt",
            ]
            .map(str::to_owned),
        )
        .expect("multiple file inputs are valid");

        assert_eq!(
            command,
            Command::Check(CheckCommand {
                dictionary_paths: vec![PathBuf::from("words.txt")],
                compiled_paths: Vec::new(),
                hunspell_affix_paths: Vec::new(),
                output_format: OutputFormat::Text,
                target: CheckTarget::Inputs(vec![
                    CheckInput::File(PathBuf::from("first.txt")),
                    CheckInput::Stdin,
                    CheckInput::File(PathBuf::from("second.txt")),
                ]),
            })
        );
    }

    #[test]
    fn parses_option_shaped_words_literally_after_the_option_separator() {
        for word in ["-ish", "--file=x"] {
            let command = parse_arguments(
                ["ferrolex", "check", "--dictionary", "words.txt", "--", word].map(str::to_owned),
            )
            .expect("the option separator makes the word unambiguous");

            assert_eq!(
                command,
                Command::Check(CheckCommand {
                    dictionary_paths: vec![PathBuf::from("words.txt")],
                    compiled_paths: Vec::new(),
                    hunspell_affix_paths: Vec::new(),
                    output_format: OutputFormat::Text,
                    target: CheckTarget::Word(word.to_owned()),
                })
            );
        }
    }

    #[test]
    fn rejects_mixed_word_and_file_inputs_or_repeated_stdin() {
        for arguments in [
            ["ferrolex", "check", "word", "--file", "input.txt"].as_slice(),
            ["ferrolex", "check", "--file", "-", "--file", "-"].as_slice(),
        ] {
            assert!(matches!(
                parse_arguments(arguments.iter().map(|argument| (*argument).to_owned())),
                Err(CliError::Usage(_))
            ));
        }
    }

    #[test]
    fn parses_explain_with_one_hunspell_source_and_word() {
        let command = parse_arguments(
            [
                "ferrolex",
                "explain",
                "--hunspell",
                "de_DE.aff",
                "Haustürschlüssel",
            ]
            .map(str::to_owned),
        )
        .expect("the explain command is valid");

        assert_eq!(
            command,
            Command::Explain(ExplainCommand {
                hunspell_affix_path: PathBuf::from("de_DE.aff"),
                word: "Haustürschlüssel".to_owned(),
            })
        );
        assert!(parse_arguments(["ferrolex", "explain", "word"].map(str::to_owned)).is_err());
    }

    #[test]
    fn renders_affixed_compound_and_rejected_explanations() {
        let dictionary = import(
            "explain.aff",
            "FORBIDDENWORD F\nSFX A Y 1\nSFX A 0 s .\nCOMPOUNDFLAG C\nCOMPOUNDMIN 1\n",
            "explain.dic",
            "6\nroot/A\nhaus/C\ntür/C\nschlüssel/C\nbad/F\nplain\n",
            ImportMode::Strict,
        )
        .expect("CLI explanation fixture imports")
        .dictionary()
        .clone();

        let affixed = render_explanation(&dictionary.explain("roots"));
        assert!(affixed.contains("status: accepted"));
        assert!(affixed.contains("match: affixed"));
        assert!(affixed.contains("stem: root"));
        assert!(affixed.contains("rule 1: suffix"));

        let compound = render_explanation(&dictionary.explain("haustürschlüssel"));
        assert!(compound.contains("match: compound"));
        assert!(compound.contains("component 1: haus"));
        assert!(compound.contains("component 3: schlüssel"));

        let rejected = render_explanation(&dictionary.explain("bad"));
        assert_eq!(rejected, "status: rejected\nreason: forbidden stem (bad)\n");
    }

    #[test]
    fn permits_an_automatic_user_dictionary_source() {
        let command = parse_arguments(["ferrolex", "check", "word"].map(str::to_owned))
            .expect("a user dictionary may supply the source at runtime");

        assert_eq!(
            command,
            Command::Check(CheckCommand {
                dictionary_paths: Vec::new(),
                compiled_paths: Vec::new(),
                hunspell_affix_paths: Vec::new(),
                output_format: OutputFormat::Text,
                target: CheckTarget::Word("word".to_owned()),
            })
        );
    }

    #[test]
    fn add_word_serializes_updates_and_sweeps_only_stale_temporary_files() {
        let directory = temporary_directory();
        let dictionary_path = directory.path.join("words.txt");
        let stale_temporary = directory.path.join(".words.txt.999999.tmp");
        let fresh_temporary = directory.path.join(".words.txt.tmp-active");
        fs::write(&stale_temporary, "stale").expect("stale fixture is writable");
        fs::write(&fresh_temporary, "active").expect("active fixture is writable");
        let stale_time = SystemTime::now()
            .checked_sub(STALE_TEMPORARY_FILE_AGE + Duration::from_secs(1))
            .expect("fixture timestamp remains representable");
        fs::File::options()
            .write(true)
            .open(&stale_temporary)
            .expect("stale fixture opens")
            .set_times(fs::FileTimes::new().set_modified(stale_time))
            .expect("stale fixture timestamp is writable");

        let lock_path = hidden_sibling(&dictionary_path, "lock");
        fs::write(&lock_path, "persistent lock fixture").expect("lock fixture is writable");
        let old_lock_time = SystemTime::now()
            .checked_sub(Duration::from_secs(60))
            .expect("fixture timestamp remains representable");
        fs::File::options()
            .write(true)
            .open(lock_path)
            .expect("lock fixture opens")
            .set_times(fs::FileTimes::new().set_modified(old_lock_time))
            .expect("lock fixture timestamp is writable");
        let lock = UserDictionaryLock::acquire(&dictionary_path).expect("test holds the lock");
        let concurrent_path = dictionary_path.clone();
        let writer = thread::spawn(move || {
            add_user_dictionary_word("second", &concurrent_path)
                .expect("concurrent word is eventually added");
        });
        thread::sleep(Duration::from_millis(50));
        fs::write(&dictionary_path, "first\n").expect("first writer commits while holding lock");
        drop(lock);
        writer.join().expect("concurrent writer does not panic");

        assert_eq!(
            fs::read_to_string(&dictionary_path).expect("dictionary remains readable"),
            "first\nsecond\n"
        );
        assert!(!stale_temporary.exists());
        assert!(fresh_temporary.exists());
    }

    #[test]
    fn accepts_help_after_the_check_command() {
        let command = parse_arguments(["ferrolex", "check", "--help"].map(str::to_owned))
            .expect("help is always valid");

        assert_eq!(command, Command::Help(HELP_CHECK));
    }

    #[test]
    fn parses_version_flags_without_arguments() {
        for flag in ["--version", "-V"] {
            assert_eq!(
                parse_arguments(["ferrolex", flag].map(str::to_owned))
                    .expect("version flag is valid"),
                Command::Version
            );
        }
    }

    #[test]
    fn accepts_equals_form_for_value_options() {
        let command = parse_arguments(
            [
                "ferrolex",
                "suggest",
                "--dictionary=words.txt",
                "--format=json",
                "--max-results=4",
                "word",
            ]
            .map(str::to_owned),
        )
        .expect("equals form is valid");

        assert_eq!(
            command,
            Command::Suggest(SuggestCommand {
                dictionary_paths: vec![PathBuf::from("words.txt")],
                compiled_paths: Vec::new(),
                hunspell_affix_paths: Vec::new(),
                max_results: Some(4),
                max_edit_distance: None,
                max_candidates: None,
                max_edit_cells: None,
                output_format: OutputFormat::Json,
                word: "word".to_owned(),
            })
        );
    }

    #[test]
    fn rejects_empty_equals_form_paths_as_usage_errors() {
        for arguments in [
            ["ferrolex", "check", "--dictionary=", "word"].as_slice(),
            ["ferrolex", "dictionary", "add-word", "--workspace=", "word"].as_slice(),
        ] {
            assert!(matches!(
                parse_arguments(arguments.iter().map(|argument| (*argument).to_owned())),
                Err(CliError::Usage(_))
            ));
        }
    }

    #[test]
    fn rejects_unknown_or_repeated_output_formats() {
        for arguments in [
            ["ferrolex", "check", "--format", "yaml", "word"].as_slice(),
            [
                "ferrolex", "check", "--format", "json", "--format", "text", "word",
            ]
            .as_slice(),
        ] {
            assert!(matches!(
                parse_arguments(arguments.iter().map(|argument| (*argument).to_owned())),
                Err(CliError::Usage(_))
            ));
        }
    }

    #[test]
    fn distinguishes_usage_and_runtime_errors() {
        assert!(CliError::Usage("invalid invocation".to_owned()).is_usage());
        assert!(!CliError::ReadInput {
            path: PathBuf::from("missing.txt"),
            source: io::Error::new(io::ErrorKind::NotFound, "missing"),
        }
        .is_usage());
        assert_eq!(RunOutcome::Failure.exit_code(), ExitCode::from(3));
    }

    #[test]
    fn parses_analyze_with_a_comment_prefix() {
        let command = parse_arguments(
            [
                "ferrolex",
                "analyze",
                "--dictionary",
                "words.txt",
                "--comment-prefix",
                "//",
                "lib.rs",
            ]
            .map(str::to_owned),
        )
        .expect("the command is valid");

        assert_eq!(
            command,
            Command::Analyze(AnalyzeCommand {
                dictionary_paths: vec![PathBuf::from("words.txt")],
                compiled_paths: Vec::new(),
                hunspell_affix_paths: Vec::new(),
                config_path: None,
                comment_syntax: Some(CommentSyntax::line("//")),
                include_patterns: Vec::new(),
                exclude_patterns: Vec::new(),
                suggest: false,
                output_format: OutputFormat::Text,
                path: PathBuf::from("lib.rs"),
            })
        );
    }

    #[test]
    fn parses_analyze_with_a_dash_comment_prefix() {
        let command = parse_arguments(
            [
                "ferrolex",
                "analyze",
                "--dictionary",
                "words.txt",
                "--comment-prefix=--",
                "query.sql",
            ]
            .map(str::to_owned),
        )
        .expect("the command is valid");

        assert_eq!(
            command,
            Command::Analyze(AnalyzeCommand {
                dictionary_paths: vec![PathBuf::from("words.txt")],
                compiled_paths: Vec::new(),
                hunspell_affix_paths: Vec::new(),
                config_path: None,
                comment_syntax: Some(CommentSyntax::line("--")),
                include_patterns: Vec::new(),
                exclude_patterns: Vec::new(),
                suggest: false,
                output_format: OutputFormat::Text,
                path: PathBuf::from("query.sql"),
            })
        );
    }

    #[test]
    fn treats_help_like_comment_prefixes_as_option_values() {
        let command = parse_arguments(
            [
                "ferrolex",
                "analyze",
                "--dictionary",
                "words.txt",
                "--comment-prefix",
                "-h",
                "query.sql",
            ]
            .map(str::to_owned),
        )
        .expect("the command is valid");

        assert_eq!(
            command,
            Command::Analyze(AnalyzeCommand {
                dictionary_paths: vec![PathBuf::from("words.txt")],
                compiled_paths: Vec::new(),
                hunspell_affix_paths: Vec::new(),
                config_path: None,
                comment_syntax: Some(CommentSyntax::line("-h")),
                include_patterns: Vec::new(),
                exclude_patterns: Vec::new(),
                suggest: false,
                output_format: OutputFormat::Text,
                path: PathBuf::from("query.sql"),
            })
        );
    }

    #[test]
    fn parses_analyze_with_html_comments() {
        let command = parse_arguments(
            [
                "ferrolex",
                "analyze",
                "--dictionary",
                "words.txt",
                "--suggest",
                "--comment-syntax",
                "html",
                "README.md",
            ]
            .map(str::to_owned),
        )
        .expect("the command is valid");

        assert_eq!(
            command,
            Command::Analyze(AnalyzeCommand {
                dictionary_paths: vec![PathBuf::from("words.txt")],
                compiled_paths: Vec::new(),
                hunspell_affix_paths: Vec::new(),
                config_path: None,
                comment_syntax: Some(CommentSyntax::Html),
                include_patterns: Vec::new(),
                exclude_patterns: Vec::new(),
                suggest: true,
                output_format: OutputFormat::Text,
                path: PathBuf::from("README.md"),
            })
        );
    }

    #[test]
    fn parses_analyze_with_a_persistent_project_config() {
        let command = parse_arguments(
            [
                "ferrolex",
                "analyze",
                "--dictionary",
                "words.txt",
                "--config",
                ".ferrolex/config",
                "src/lib.rs",
            ]
            .map(str::to_owned),
        )
        .expect("the command is valid");

        assert_eq!(
            command,
            Command::Analyze(AnalyzeCommand {
                dictionary_paths: vec![PathBuf::from("words.txt")],
                compiled_paths: Vec::new(),
                hunspell_affix_paths: Vec::new(),
                config_path: Some(PathBuf::from(".ferrolex/config")),
                comment_syntax: None,
                include_patterns: Vec::new(),
                exclude_patterns: Vec::new(),
                suggest: false,
                output_format: OutputFormat::Text,
                path: PathBuf::from("src/lib.rs"),
            })
        );
    }

    #[test]
    fn parses_analyze_file_selection_patterns() {
        let command = parse_arguments(
            [
                "ferrolex",
                "analyze",
                "--dictionary",
                "words.txt",
                "--include",
                "**/*.rs",
                "--exclude",
                "target/**",
                "src",
            ]
            .map(str::to_owned),
        )
        .expect("the command is valid");

        assert!(matches!(
            command,
            Command::Analyze(AnalyzeCommand { include_patterns, exclude_patterns, .. })
                if include_patterns == ["**/*.rs"] && exclude_patterns == ["target/**"]
        ));
    }

    #[test]
    fn matches_path_globs_without_matching_one_directory_star_across_slashes() {
        assert!(glob_matches("**/*.rs", "src/lib.rs"));
        assert!(glob_matches("**/*.rs", "lib.rs"));
        assert!(glob_matches("target/**", "target/debug/ferrolex"));
        assert!(!glob_matches("*.rs", "src/lib.rs"));
    }

    #[test]
    fn chooses_comment_presets_from_file_extensions() {
        assert_eq!(
            comment_syntax_for_path(Path::new("lib.rs")),
            CommentSyntax::line("//")
        );
        assert_eq!(
            comment_syntax_for_path(Path::new("query.sql")),
            CommentSyntax::line("--")
        );
        assert_eq!(
            comment_syntax_for_path(Path::new("README.md")),
            CommentSyntax::Html
        );
        assert_eq!(
            comment_syntax_for_path(Path::new("words.txt")),
            CommentSyntax::None
        );
    }

    #[test]
    fn parses_hunspell_cache_inputs_for_check_and_analysis() {
        let check = parse_arguments(
            [
                "ferrolex",
                "check",
                "--hunspell",
                "de.aff",
                "--hunspell",
                "en.aff",
                "Wort",
            ]
            .map(str::to_owned),
        )
        .expect("the cached Hunspell command is valid");
        let analyze = parse_arguments(
            ["ferrolex", "analyze", "--hunspell", "de.aff", "src/lib.rs"].map(str::to_owned),
        )
        .expect("the cached Hunspell command is valid");

        assert_eq!(
            check,
            Command::Check(CheckCommand {
                dictionary_paths: Vec::new(),
                compiled_paths: Vec::new(),
                hunspell_affix_paths: vec![PathBuf::from("de.aff"), PathBuf::from("en.aff")],
                output_format: OutputFormat::Text,
                target: CheckTarget::Word("Wort".to_owned()),
            })
        );
        assert_eq!(
            analyze,
            Command::Analyze(AnalyzeCommand {
                dictionary_paths: Vec::new(),
                compiled_paths: Vec::new(),
                hunspell_affix_paths: vec![PathBuf::from("de.aff")],
                output_format: OutputFormat::Text,
                config_path: None,
                comment_syntax: None,
                include_patterns: Vec::new(),
                exclude_patterns: Vec::new(),
                suggest: false,
                path: PathBuf::from("src/lib.rs"),
            })
        );
    }

    #[test]
    fn parses_strict_hunspell_validation() {
        let command = parse_arguments(
            ["ferrolex", "validate", "--strict", "de.aff", "de.dic"].map(str::to_owned),
        )
        .expect("the command is valid");

        assert_eq!(
            command,
            Command::Validate(ValidateCommand::Hunspell {
                strict: true,
                aff_path: PathBuf::from("de.aff"),
                dic_path: PathBuf::from("de.dic"),
                output_format: OutputFormat::Text,
            })
        );
    }

    #[test]
    fn parses_plain_word_list_compilation() {
        let command = parse_arguments(
            [
                "ferrolex",
                "compile",
                "--dictionary",
                "words.txt",
                "-o",
                "words.flex",
            ]
            .map(str::to_owned),
        )
        .expect("the command is valid");

        assert_eq!(
            command,
            Command::Compile(CompileCommand {
                input: CompileInput::WordList(PathBuf::from("words.txt")),
                output_path: PathBuf::from("words.flex"),
            })
        );
    }

    #[test]
    fn parses_hunspell_pair_compilation() {
        let command = parse_arguments(
            ["ferrolex", "compile", "de.aff", "de.dic", "-o", "de.flexh"].map(str::to_owned),
        )
        .expect("the command is valid");

        assert_eq!(
            command,
            Command::Compile(CompileCommand {
                input: CompileInput::Hunspell {
                    aff_path: PathBuf::from("de.aff"),
                    dic_path: PathBuf::from("de.dic"),
                },
                output_path: PathBuf::from("de.flexh"),
            })
        );
    }

    #[test]
    fn parses_artifact_inspection() {
        let command =
            parse_arguments(["ferrolex", "inspect", "dictionary.flexh"].map(str::to_owned))
                .expect("the command is valid");

        assert_eq!(command, Command::Inspect(PathBuf::from("dictionary.flexh")));
    }

    #[test]
    fn parses_bounded_plain_word_list_suggestions() {
        let command = parse_arguments(
            [
                "ferrolex",
                "suggest",
                "--dictionary",
                "words.txt",
                "recieve",
            ]
            .map(str::to_owned),
        )
        .expect("the command is valid");

        assert_eq!(
            command,
            Command::Suggest(SuggestCommand {
                dictionary_paths: vec![PathBuf::from("words.txt")],
                compiled_paths: Vec::new(),
                hunspell_affix_paths: Vec::new(),
                max_results: None,
                max_edit_distance: None,
                max_candidates: None,
                max_edit_cells: None,
                output_format: OutputFormat::Text,
                word: "recieve".to_owned(),
            })
        );
    }

    #[test]
    fn parses_layered_suggestion_sources() {
        let command = parse_arguments(
            [
                "ferrolex",
                "suggest",
                "--dictionary",
                "base.txt",
                "--dictionary",
                "technical.txt",
                "--compiled",
                "project.flex",
                "--hunspell",
                "de.aff",
                "recieve",
            ]
            .map(str::to_owned),
        )
        .expect("layered suggestion sources are valid");

        assert_eq!(
            command,
            Command::Suggest(SuggestCommand {
                dictionary_paths: vec![PathBuf::from("base.txt"), PathBuf::from("technical.txt"),],
                compiled_paths: vec![PathBuf::from("project.flex")],
                hunspell_affix_paths: vec![PathBuf::from("de.aff")],
                output_format: OutputFormat::Text,
                max_results: None,
                max_edit_distance: None,
                max_candidates: None,
                max_edit_cells: None,
                word: "recieve".to_owned(),
            })
        );
    }

    #[test]
    fn incomplete_empty_suggestions_offer_scaled_budget_flags() {
        let config = SuggestConfig {
            max_candidates: 300,
            max_edit_cells: 12_000,
            ..SuggestConfig::default()
        };

        let hint = incomplete_suggestion_hint(super::Completeness::EditBudgetReached, config)
            .expect("budget exhaustion has an actionable hint");

        assert!(hint.contains("--max-candidates 600"));
        assert!(hint.contains("--max-edit-cells 24000"));
        assert!(incomplete_suggestion_hint(super::Completeness::QueryTooLong, config).is_none());
        assert_eq!(
            super::completeness_code(super::Completeness::RelatedSeedTooLong),
            "related-seed-too-long"
        );
        assert!(
            incomplete_suggestion_hint(super::Completeness::RelatedSeedTooLong, config).is_none()
        );
    }

    #[test]
    fn parses_explicit_suggestion_limits() {
        let command = parse_arguments(
            [
                "ferrolex",
                "suggest",
                "--dictionary",
                "words.txt",
                "--max-results",
                "3",
                "--max-edit-distance",
                "0",
                "--max-candidates",
                "300",
                "--max-edit-cells",
                "12000",
                "recieve",
            ]
            .map(str::to_owned),
        )
        .expect("the command is valid");

        assert_eq!(
            command,
            Command::Suggest(SuggestCommand {
                dictionary_paths: vec![PathBuf::from("words.txt")],
                compiled_paths: Vec::new(),
                hunspell_affix_paths: Vec::new(),
                max_results: Some(3),
                max_edit_distance: Some(0),
                max_candidates: Some(300),
                max_edit_cells: Some(12_000),
                output_format: OutputFormat::Text,
                word: "recieve".to_owned(),
            })
        );
    }

    #[test]
    fn parses_installed_hunspell_suggestions() {
        let command = parse_arguments(
            ["ferrolex", "suggest", "--hunspell", "de.aff", "Hauser"].map(str::to_owned),
        )
        .expect("the command is valid");

        assert_eq!(
            command,
            Command::Suggest(SuggestCommand {
                dictionary_paths: Vec::new(),
                compiled_paths: Vec::new(),
                hunspell_affix_paths: vec![PathBuf::from("de.aff")],
                output_format: OutputFormat::Text,
                max_results: None,
                max_edit_distance: None,
                max_candidates: None,
                max_edit_cells: None,
                word: "Hauser".to_owned(),
            })
        );
    }

    #[test]
    fn rejects_invalid_suggestion_limits() {
        for arguments in [
            &[
                "ferrolex",
                "suggest",
                "--dictionary",
                "words.txt",
                "--max-results",
                "0",
                "recieve",
            ] as &[&str],
            &[
                "ferrolex",
                "suggest",
                "--dictionary",
                "words.txt",
                "--max-edit-distance",
                "two",
                "recieve",
            ] as &[&str],
            &[
                "ferrolex",
                "suggest",
                "--dictionary",
                "words.txt",
                "--max-results",
                "3",
                "--max-results",
                "4",
                "recieve",
            ] as &[&str],
        ] {
            assert!(
                parse_arguments(arguments.iter().map(|argument| (*argument).to_owned())).is_err()
            );
        }
    }

    #[test]
    fn parses_compiled_dictionary_check_input() {
        let command = parse_arguments(
            ["ferrolex", "check", "--compiled", "words.flex", "Straße"].map(str::to_owned),
        )
        .expect("the command is valid");

        assert_eq!(
            command,
            Command::Check(CheckCommand {
                dictionary_paths: Vec::new(),
                compiled_paths: vec![PathBuf::from("words.flex")],
                hunspell_affix_paths: Vec::new(),
                output_format: OutputFormat::Text,
                target: CheckTarget::Word("Straße".to_owned()),
            })
        );
    }

    #[test]
    fn suggests_from_a_plain_word_list() {
        let dictionary = temporary_dictionary("receive\nrecipe\n");
        let arguments = [
            "ferrolex".to_owned(),
            "suggest".to_owned(),
            "--dictionary".to_owned(),
            dictionary.path.to_string_lossy().into_owned(),
            "recieve".to_owned(),
        ];

        assert_eq!(
            run(arguments).expect("dictionary is readable"),
            RunOutcome::Success
        );
    }

    #[test]
    fn layered_word_lists_contribute_suggestion_candidates() {
        let base = temporary_dictionary("recipe\n");
        let technical = temporary_dictionary("receive\n");
        let dictionary =
            load_analysis_dictionary(&[base.path.clone(), technical.path.clone()], &[], &[])
                .expect("both word lists load");

        let result =
            super::Suggester::new(&dictionary, SuggestConfig::default()).suggest("recieve");

        assert!(result
            .suggestions()
            .iter()
            .any(|suggestion| suggestion.word() == "receive"));
    }

    #[test]
    fn analysis_suggestion_engine_memoizes_repeated_words() {
        let source = CountingCandidateSource {
            visits: AtomicUsize::new(0),
        };
        let mut engine = AnalysisSuggestionEngine::new(&source);

        assert_eq!(
            engine.base_suggestions("recieve"),
            vec![("receive".to_owned(), 1)]
        );
        let visits_after_first_query = source.visits.load(Ordering::Relaxed);
        assert!(visits_after_first_query > 0);

        assert_eq!(
            engine.base_suggestions("recieve"),
            vec![("receive".to_owned(), 1)]
        );
        assert_eq!(
            source.visits.load(Ordering::Relaxed),
            visits_after_first_query
        );
    }

    #[test]
    fn cached_analysis_suggestions_keep_identifier_context() {
        let dictionary = AnalysisDictionary {
            sources: vec![AnalysisSource::WordList(
                WordList::new(["Account", "Authentication", "OAuth", "Provider"])
                    .expect("test words are valid"),
            )],
        };
        let analyzer = Analyzer::builder(&dictionary).build();
        let analysis = analyzer.check(&Document::new(
            "OAuthAuthentcationProvider AccountAuthentcationProvider",
        ));
        let mut engine = AnalysisSuggestionEngine::new(&dictionary);

        assert_eq!(analysis.findings().len(), 2);
        assert_eq!(
            engine.suggestions(&analysis.findings()[0]),
            vec![("OAuthAuthenticationProvider".to_owned(), 1)]
        );
        assert_eq!(
            engine.suggestions(&analysis.findings()[1]),
            vec![("AccountAuthenticationProvider".to_owned(), 1)]
        );
    }

    #[test]
    fn layered_sources_preserve_hunspell_output_metadata() {
        let hunspell = import(
            "metadata.aff",
            "OCONV 2\nOCONV ae æ\nOCONV plain rewritten\n",
            "metadata.dic",
            "1\naer\n",
            ImportMode::Strict,
        )
        .expect("output metadata fixture imports")
        .dictionary()
        .clone();
        let dictionary = AnalysisDictionary {
            sources: vec![
                AnalysisSource::WordList(WordList::from_text(Normalization::Exact, "plain\n")),
                AnalysisSource::Hunspell(Box::new(hunspell)),
            ],
        };

        let ranking = dictionary
            .hunspell_ranking_dictionary()
            .expect("adding a plain layer preserves Hunspell ranking metadata");

        assert_eq!(ranking.normalize_output("aer"), "ær");
        assert_eq!(dictionary.normalize_suggestion_output("aer"), "ær");
        assert_eq!(dictionary.normalize_suggestion_output("plain"), "plain");
    }

    #[test]
    fn analyzes_with_a_persistent_project_config() {
        let dictionary = temporary_dictionary("Auth\n");
        let source = temporary_file("Ferrolex OAuth generated_token\n");
        let config = temporary_file(
            "ignore-word = Ferrolex\nignore-pattern = ^generated_[a-z]+$\nsingle-letter-prefix = separate\n",
        );
        let arguments = [
            "ferrolex".to_owned(),
            "analyze".to_owned(),
            "--dictionary".to_owned(),
            dictionary.path.to_string_lossy().into_owned(),
            "--config".to_owned(),
            config.path.to_string_lossy().into_owned(),
            source.path.to_string_lossy().into_owned(),
        ];

        assert_eq!(
            run(arguments).expect("project policy is readable"),
            RunOutcome::Success
        );
    }

    #[test]
    fn analyzes_html_comment_directives() {
        let dictionary = temporary_dictionary("known\n");
        let source = temporary_file("<!-- ferrolex:ignore typo -->\ntypo\n");
        let arguments = [
            "ferrolex".to_owned(),
            "analyze".to_owned(),
            "--dictionary".to_owned(),
            dictionary.path.to_string_lossy().into_owned(),
            "--suggest".to_owned(),
            "--comment-syntax".to_owned(),
            "html".to_owned(),
            source.path.to_string_lossy().into_owned(),
        ];

        assert_eq!(
            run(arguments).expect("HTML directives are recognized"),
            RunOutcome::Success
        );
    }

    #[test]
    fn parses_a_catalog_pinned_dictionary_fetch() {
        let command = parse_arguments([
            "ferrolex".to_owned(),
            "dictionary".to_owned(),
            "fetch".to_owned(),
            "de_DE".to_owned(),
            "--cache".to_owned(),
            ".dictionary-cache".to_owned(),
        ])
        .expect("the reviewed fetch command is valid");

        assert_eq!(
            command,
            Command::Dictionary(DictionaryCommand::Fetch {
                locale: "de_DE".to_owned(),
                cache_path: PathBuf::from(".dictionary-cache"),
            })
        );
    }

    #[test]
    fn parses_a_catalog_pinned_dictionary_install() {
        let command = parse_arguments([
            "ferrolex".to_owned(),
            "dictionary".to_owned(),
            "install".to_owned(),
            "de_DE".to_owned(),
            "--cache".to_owned(),
            ".dictionary-cache".to_owned(),
        ])
        .expect("the reviewed install command is valid");

        assert_eq!(
            command,
            Command::Dictionary(DictionaryCommand::Install {
                locale: "de_DE".to_owned(),
                cache_path: PathBuf::from(".dictionary-cache"),
            })
        );
    }

    #[test]
    fn dictionary_fetch_requires_a_caller_selected_cache() {
        let error =
            parse_arguments(["ferrolex", "dictionary", "fetch", "de_DE"].map(str::to_owned))
                .expect_err("the installer must not infer a cache location");

        assert!(matches!(error, CliError::Usage(message) if message.contains("--cache")));
    }

    #[test]
    fn parses_a_catalog_listing_without_network_parameters() {
        let command = parse_arguments(["ferrolex", "dictionary", "list"].map(str::to_owned))
            .expect("list needs no download configuration");

        assert_eq!(command, Command::Dictionary(DictionaryCommand::List));
    }

    #[test]
    fn rejects_compile_without_an_output_path() {
        let error = parse_arguments(
            ["ferrolex", "compile", "--dictionary", "words.txt"].map(str::to_owned),
        )
        .expect_err("an output artifact is required");

        assert!(matches!(error, CliError::Usage(message) if message.contains("-o")));
    }

    #[test]
    fn compiles_plain_word_list_semantics_into_an_artifact() {
        let dictionary = temporary_dictionary("# ignored\n Straße \n\n東京\n");
        let output = temporary_file("");
        let arguments = [
            "ferrolex".to_owned(),
            "compile".to_owned(),
            "--dictionary".to_owned(),
            dictionary.path.to_string_lossy().into_owned(),
            "-o".to_owned(),
            output.path.to_string_lossy().into_owned(),
        ];

        assert_eq!(
            run(arguments).expect("dictionary and output are usable"),
            RunOutcome::Success
        );
        let compiled = CompiledDictionary::load(
            fs::read(&output.path).expect("the compiler wrote the artifact"),
        )
        .expect("the compiler wrote a valid fast-load header");
        assert!(compiled.contains("Straße"));
        assert!(compiled.contains("東京"));
        compiled.validate().expect("the artifact is fully valid");
    }

    #[test]
    fn checks_a_compiled_dictionary_artifact() {
        let source = temporary_dictionary("Straße\n");
        let artifact = temporary_file("");
        run([
            "ferrolex".to_owned(),
            "compile".to_owned(),
            "--dictionary".to_owned(),
            source.path.to_string_lossy().into_owned(),
            "-o".to_owned(),
            artifact.path.to_string_lossy().into_owned(),
        ])
        .expect("the artifact compiles");

        assert_eq!(
            run([
                "ferrolex".to_owned(),
                "check".to_owned(),
                "--compiled".to_owned(),
                artifact.path.to_string_lossy().into_owned(),
                "Straße".to_owned(),
            ])
            .expect("the artifact is readable"),
            RunOutcome::Success
        );
    }

    #[test]
    fn compiles_and_uses_a_standalone_hunspell_artifact() {
        let affix = temporary_file("SET UTF-8\nSFX S Y 1\nSFX S 0 s .\n");
        let dictionary = temporary_file("1\nbook/S\n");
        let artifact = temporary_file("");
        run([
            "ferrolex".to_owned(),
            "compile".to_owned(),
            affix.path.to_string_lossy().into_owned(),
            dictionary.path.to_string_lossy().into_owned(),
            "-o".to_owned(),
            artifact.path.to_string_lossy().into_owned(),
        ])
        .expect("the Hunspell pair compiles");

        drop(affix);
        drop(dictionary);
        assert_eq!(
            run([
                "ferrolex".to_owned(),
                "check".to_owned(),
                "--compiled".to_owned(),
                artifact.path.to_string_lossy().into_owned(),
                "books".to_owned(),
            ])
            .expect("the standalone artifact is readable"),
            RunOutcome::Success
        );
    }

    #[test]
    fn inspects_native_and_standalone_hunspell_artifacts() {
        let words = temporary_dictionary("ant\nzebra\n");
        let native = temporary_file("");
        run([
            "ferrolex".to_owned(),
            "compile".to_owned(),
            "--dictionary".to_owned(),
            words.path.to_string_lossy().into_owned(),
            "-o".to_owned(),
            native.path.to_string_lossy().into_owned(),
        ])
        .expect("the word list compiles");
        assert_eq!(
            run([
                "ferrolex".to_owned(),
                "inspect".to_owned(),
                native.path.to_string_lossy().into_owned(),
            ])
            .expect("the native artifact is inspectable"),
            RunOutcome::Success
        );

        let affix = temporary_file("SET UTF-8\n");
        let dictionary = temporary_file("1\nbook\n");
        let hunspell = temporary_file("");
        run([
            "ferrolex".to_owned(),
            "compile".to_owned(),
            affix.path.to_string_lossy().into_owned(),
            dictionary.path.to_string_lossy().into_owned(),
            "-o".to_owned(),
            hunspell.path.to_string_lossy().into_owned(),
        ])
        .expect("the Hunspell pair compiles");
        assert_eq!(
            run([
                "ferrolex".to_owned(),
                "inspect".to_owned(),
                hunspell.path.to_string_lossy().into_owned(),
            ])
            .expect("the Hunspell artifact is inspectable"),
            RunOutcome::Success
        );
    }

    #[test]
    fn rejects_an_oversized_compiled_artifact_before_reading_it() {
        let artifact = temporary_file("");
        fs::OpenOptions::new()
            .write(true)
            .open(&artifact.path)
            .expect("temporary artifact is writable")
            .set_len(u64::try_from(MAX_COMPILED_ARTIFACT_BYTES + 1).expect("limit fits u64"))
            .expect("sparse length is supported");

        assert!(matches!(
            read_compiled_artifact(&artifact.path),
            Err(CliError::ArtifactTooLarge { .. })
        ));
    }

    #[test]
    fn validates_a_compiled_artifact_with_the_paranoid_check() {
        let source = temporary_dictionary("Straße\n");
        let artifact = temporary_file("");
        let compile_arguments = [
            "ferrolex".to_owned(),
            "compile".to_owned(),
            "--dictionary".to_owned(),
            source.path.to_string_lossy().into_owned(),
            "-o".to_owned(),
            artifact.path.to_string_lossy().into_owned(),
        ];
        run(compile_arguments).expect("compiler inputs are usable");
        let validate_arguments = [
            "ferrolex".to_owned(),
            "validate".to_owned(),
            "--compiled".to_owned(),
            artifact.path.to_string_lossy().into_owned(),
        ];

        assert_eq!(
            run(validate_arguments).expect("artifact is readable"),
            RunOutcome::Success
        );
    }

    #[test]
    fn compiled_validation_runs_the_full_structural_check_after_fast_loading() {
        let source = temporary_dictionary("word\n");
        let artifact = temporary_file("");
        let compile_arguments = [
            "ferrolex".to_owned(),
            "compile".to_owned(),
            "--dictionary".to_owned(),
            source.path.to_string_lossy().into_owned(),
            "-o".to_owned(),
            artifact.path.to_string_lossy().into_owned(),
        ];
        run(compile_arguments).expect("compiler inputs are usable");

        let mut bytes = fs::read(&artifact.path).expect("the artifact exists");
        let data_offset = u64::from_le_bytes(
            bytes[40..48]
                .try_into()
                .expect("compiled header has a data offset"),
        );
        let data_offset = usize::try_from(data_offset).expect("test platform supports offsets");
        bytes[data_offset] = 0xff;
        refresh_compiled_checksum(&mut bytes);
        fs::write(&artifact.path, bytes).expect("the artifact is writable");
        let validate_arguments = [
            "ferrolex".to_owned(),
            "validate".to_owned(),
            "--compiled".to_owned(),
            artifact.path.to_string_lossy().into_owned(),
        ];

        assert!(matches!(
            run(validate_arguments),
            Err(CliError::ValidateArtifact {
                source: ValidationError::InvalidUtf8 { entry: 0 },
                ..
            })
        ));
    }

    #[test]
    fn parses_compiled_artifact_validation_without_changing_hunspell_syntax() {
        let command = parse_arguments(
            ["ferrolex", "validate", "--compiled", "words.flex"].map(str::to_owned),
        )
        .expect("the command is valid");

        assert_eq!(
            command,
            Command::Validate(ValidateCommand::Compiled {
                path: PathBuf::from("words.flex"),
                output_format: OutputFormat::Text,
            })
        );
    }

    #[test]
    fn rejects_incomplete_hunspell_validation_paths() {
        let error = parse_arguments(["ferrolex", "validate", "de.aff"].map(str::to_owned))
            .expect_err("both dictionary files are required");

        assert!(matches!(error, CliError::Usage(message) if message.contains("AFF path")));
    }

    #[test]
    fn strict_hunspell_validation_reports_import_errors_with_a_failure_exit_code() {
        let affix = temporary_file("SET KOI8-R\n");
        let dictionary = temporary_file("1\nword\n");
        let arguments = [
            "ferrolex".to_owned(),
            "validate".to_owned(),
            "--strict".to_owned(),
            affix.path.to_string_lossy().into_owned(),
            dictionary.path.to_string_lossy().into_owned(),
        ];

        assert_eq!(
            run(arguments).expect("validation files are readable"),
            RunOutcome::Misspelled
        );
    }

    #[test]
    fn strict_hunspell_validation_decodes_iso_8859_1_files() {
        let affix = temporary_file("SET ISO-8859-1\n");
        let dictionary = temporary_bytes(b"1\ncaf\xe9\n");
        let arguments = [
            "ferrolex".to_owned(),
            "validate".to_owned(),
            "--strict".to_owned(),
            affix.path.to_string_lossy().into_owned(),
            dictionary.path.to_string_lossy().into_owned(),
        ];

        assert_eq!(
            run(arguments).expect("legacy-encoded files are readable"),
            RunOutcome::Success
        );
    }

    #[test]
    fn catalog_mixed_encoding_override_preserves_the_utf8_dictionary_file() {
        let affix = temporary_file("SET ISO-8859-1\n");
        let dictionary = temporary_file("1\ncafé\n");

        assert_eq!(
            validate_hunspell(
                true,
                &affix.path,
                &dictionary.path,
                catalog_import_encodings(SourceEncoding::MixedUtf8AndIso8859_1),
                OutputFormat::Text,
            )
            .expect("mixed-encoding files are readable"),
            RunOutcome::Success
        );
    }

    #[test]
    fn install_builds_a_provenance_bound_runtime_cache() {
        let affix = temporary_file("SET UTF-8\nSFX S N 1\nSFX S 0 s .\n");
        let dictionary = temporary_file("1\nword/S\n");
        let cache_path = runtime_cache_path(&affix.path);

        assert_eq!(
            install_hunspell_runtime_cache("test", &affix.path, &dictionary.path, None)
                .expect("fixture sources are readable"),
            RunOutcome::Success
        );

        let affix_bytes = fs::read(&affix.path).expect("affix source remains available");
        let dictionary_bytes =
            fs::read(&dictionary.path).expect("dictionary source remains available");
        let cache = fs::read(&cache_path).expect("runtime cache is written beside the affix file");
        let loaded = load_runtime_cache(
            &cache,
            SourceDigests::from_source_bytes(&affix_bytes, &dictionary_bytes),
        )
        .expect("runtime cache matches the exact sources");
        assert!(loaded.contains("words"));
        fs::remove_file(cache_path).expect("test removes its derived cache");
    }

    #[test]
    fn check_and_analyze_load_an_installed_hunspell_runtime_cache() {
        let sources =
            temporary_hunspell_sources("SET UTF-8\nSFX S N 1\nSFX S 0 s .\n", "1\nword/S\n");
        install_hunspell_runtime_cache("test", &sources.affix_path, &sources.dictionary_path, None)
            .expect("fixture sources are readable");

        let check_arguments = [
            "ferrolex".to_owned(),
            "check".to_owned(),
            "--hunspell".to_owned(),
            sources.affix_path.to_string_lossy().into_owned(),
            "words".to_owned(),
        ];
        assert_eq!(
            run(check_arguments).expect("the matching runtime cache loads"),
            RunOutcome::Success
        );

        let explain_arguments = [
            "ferrolex".to_owned(),
            "explain".to_owned(),
            "--hunspell".to_owned(),
            sources.affix_path.to_string_lossy().into_owned(),
            "words".to_owned(),
        ];
        assert_eq!(
            run(explain_arguments).expect("the matching runtime cache loads"),
            RunOutcome::Success
        );

        let source = temporary_file("words\n");
        let analyze_arguments = [
            "ferrolex".to_owned(),
            "analyze".to_owned(),
            "--hunspell".to_owned(),
            sources.affix_path.to_string_lossy().into_owned(),
            source.path.to_string_lossy().into_owned(),
        ];
        assert_eq!(
            run(analyze_arguments).expect("the matching runtime cache loads"),
            RunOutcome::Success
        );

        fs::write(&sources.dictionary_path, "1\nother/S\n")
            .expect("fixture dictionary is writable");
        let stale_arguments = [
            "ferrolex".to_owned(),
            "check".to_owned(),
            "--hunspell".to_owned(),
            sources.affix_path.to_string_lossy().into_owned(),
            "words".to_owned(),
        ];
        assert!(matches!(
            run(stale_arguments),
            Err(CliError::LoadHunspellCache {
                source: RuntimeCacheError::SourceDigestMismatch(CacheSource::Dic),
                ..
            })
        ));
    }

    #[test]
    fn legacy_runtime_cache_namespace_does_not_block_source_import() {
        let sources = temporary_hunspell_sources("\u{feff}SET UTF-8\n", "\u{feff}1\nword\n");
        let legacy_cache_path = sources
            .affix_path
            .with_extension("ferrolex-hunspell-v1.flexh");
        fs::write(&legacy_cache_path, b"legacy version-29 cache")
            .expect("legacy cache fixture is writable");
        let arguments = [
            "ferrolex".to_owned(),
            "check".to_owned(),
            "--hunspell".to_owned(),
            sources.affix_path.to_string_lossy().into_owned(),
            "word".to_owned(),
        ];

        assert_eq!(
            run(arguments).expect("legacy cache namespace falls back to the source pair"),
            RunOutcome::Success
        );
        assert!(!runtime_cache_path(&sources.affix_path).exists());
        fs::remove_file(legacy_cache_path).expect("test removes its legacy cache fixture");
    }

    #[test]
    fn rejects_an_option_where_a_dictionary_path_is_required() {
        let error = parse_arguments(
            ["ferrolex", "check", "--dictionary", "--unknown", "word"].map(str::to_owned),
        )
        .expect_err("an option is not a dictionary path");

        assert!(matches!(error, CliError::Usage(message) if message.contains("requires a path")));
    }

    #[test]
    fn returns_conventional_check_exit_codes() {
        let dictionary = temporary_dictionary("Straße\n");
        let arguments = |word: &str| {
            [
                "ferrolex".to_owned(),
                "check".to_owned(),
                "--dictionary".to_owned(),
                dictionary.path.to_string_lossy().into_owned(),
                word.to_owned(),
            ]
        };

        assert_eq!(
            run(arguments("Straße")).expect("dictionary is readable"),
            RunOutcome::Success
        );
        assert_eq!(
            run(arguments("Strasse")).expect("dictionary is readable"),
            RunOutcome::Misspelled
        );
    }

    #[test]
    fn checks_every_natural_language_word_in_a_file() {
        let dictionary = temporary_dictionary("Café\nStraße\n");
        let input = temporary_file("Café, Strasse!\nStraße\n");
        let arguments = [
            "ferrolex".to_owned(),
            "check".to_owned(),
            "--dictionary".to_owned(),
            dictionary.path.to_string_lossy().into_owned(),
            "--file".to_owned(),
            input.path.to_string_lossy().into_owned(),
        ];

        assert_eq!(
            run(arguments).expect("inputs are readable"),
            RunOutcome::Misspelled
        );
    }

    #[test]
    fn counts_columns_as_unicode_scalar_values() {
        let text = "Café\nStrasse";
        assert_eq!(LineIndex::new(text).line_and_column(text, 6), (2, 1));
    }

    #[test]
    fn analyze_skips_vcs_metadata_and_non_utf8_files() {
        let dictionary = temporary_dictionary("correct\n");
        let directory = temporary_directory();
        let source = directory.path.join("source.txt");
        let binary = directory.path.join("binary.bin");
        let git_index = directory.path.join(".git/index");
        let metadata_file = directory.path.join(".hg");
        fs::create_dir_all(git_index.parent().expect("index has a parent"))
            .expect("the temporary directory is writable");
        fs::write(&source, "misspelt").expect("the temporary directory is writable");
        fs::write(&binary, [0xff]).expect("the temporary directory is writable");
        fs::write(&git_index, [0xff]).expect("the temporary directory is writable");
        fs::write(&metadata_file, "correct").expect("the temporary directory is writable");

        assert_eq!(
            analysis_paths(&directory.path, &[], &[]).expect("paths are readable"),
            vec![metadata_file, binary.clone(), source]
        );
        assert_eq!(
            read_analysis_source(&binary).expect("binary files are skipped"),
            None
        );
        assert_eq!(
            analyze(&AnalyzeCommand {
                dictionary_paths: vec![dictionary.path.clone()],
                compiled_paths: Vec::new(),
                hunspell_affix_paths: Vec::new(),
                config_path: None,
                comment_syntax: None,
                include_patterns: Vec::new(),
                exclude_patterns: Vec::new(),
                suggest: false,
                output_format: OutputFormat::Text,
                path: directory.path.clone(),
            })
            .expect("analysis continues after a non-UTF-8 file"),
            RunOutcome::Misspelled
        );
    }

    struct TemporaryDictionary {
        path: PathBuf,
    }

    struct TemporaryDirectory {
        path: PathBuf,
    }

    struct TemporaryHunspellSources {
        affix_path: PathBuf,
        dictionary_path: PathBuf,
    }

    impl Drop for TemporaryHunspellSources {
        fn drop(&mut self) {
            let _ = fs::remove_file(runtime_cache_path(&self.affix_path));
            let _ = fs::remove_file(&self.affix_path);
            let _ = fs::remove_file(&self.dictionary_path);
        }
    }

    impl Drop for TemporaryDictionary {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn temporary_dictionary(contents: &str) -> TemporaryDictionary {
        temporary_file(contents)
    }

    fn temporary_file(contents: &str) -> TemporaryDictionary {
        temporary_bytes(contents.as_bytes())
    }

    fn temporary_bytes(contents: &[u8]) -> TemporaryDictionary {
        let sequence = NEXT_TEMPORARY_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrolex-cli-test-{}-{sequence}.txt",
            std::process::id()
        ));
        fs::write(&path, contents).expect("the temporary directory is writable");
        TemporaryDictionary { path }
    }

    fn temporary_directory() -> TemporaryDirectory {
        let sequence = NEXT_TEMPORARY_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrolex-cli-test-directory-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("the temporary directory is writable");
        TemporaryDirectory { path }
    }

    fn temporary_hunspell_sources(affix: &str, dictionary: &str) -> TemporaryHunspellSources {
        let sequence = NEXT_TEMPORARY_FILE.fetch_add(1, Ordering::Relaxed);
        let stem = std::env::temp_dir().join(format!(
            "ferrolex-cli-hunspell-test-{}-{sequence}",
            std::process::id()
        ));
        let affix_path = stem.with_extension("aff");
        let dictionary_path = stem.with_extension("dic");
        fs::write(&affix_path, affix).expect("the temporary directory is writable");
        fs::write(&dictionary_path, dictionary).expect("the temporary directory is writable");
        TemporaryHunspellSources {
            affix_path,
            dictionary_path,
        }
    }

    fn refresh_compiled_checksum(bytes: &mut [u8]) {
        const CHECKSUM_OFFSET: usize = 16;
        const CHECKSUM_END: usize = 24;
        const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;

        let checksum = bytes
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
        bytes[CHECKSUM_OFFSET..CHECKSUM_END].copy_from_slice(&checksum.to_le_bytes());
    }
}

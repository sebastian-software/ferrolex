//! Reference and diagnostic command-line interface for ferrolex.
//!
//! The CLI is a supporting interface for the engine and managed dictionary
//! workflow; library consumers should use the public Rust crates directly.

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

// ==== Argument model and parsing modules ====
mod args;
mod commands;

pub(crate) use args::{
    AnalyzeCommand, CheckCommand, CheckInput, CheckTarget, CliError, Command, CompileCommand,
    CompileInput, DictionaryCommand, ExplainCommand, LineIndex, OutputFormat, RunOutcome,
    SuggestCommand, ValidateCommand, parse_arguments,
};

// ==== Command implementations, output, and diagnostics ====
#[allow(unused_imports)]
pub(crate) use commands::{
    AnalysisDictionary, AnalysisSource, AnalysisSuggestionEngine, UserDictionaryLock,
    add_user_dictionary_word, analysis_paths, analyze, check, comment_syntax_for_path, compile,
    completeness_code, dictionary, explain, glob_matches, hidden_sibling,
    incomplete_suggestion_hint, inspect_artifact, install_hunspell_runtime_cache,
    load_analysis_dictionary, read_analysis_source, read_compiled_artifact, render_explanation,
    runtime_cache_path, suggest, validate, validate_hunspell,
};

// ==== CLI dependencies ====
use std::time::{Duration, SystemTime};

use ferrolex::catalog_import_encodings;
use ferrolex_code::{
    Analyzer, AnalyzerConfigError, CommentSyntax, DirectiveProblem, Document, ProjectConfig,
    ProjectConfigError,
};
use ferrolex_compiler::{
    CompileError, CompiledDictionary, FrequencyListError, LoadError, MAX_COMPILED_ARTIFACT_BYTES,
    ValidationError, compile_frequency_word_list, compile_words, inspect_compiled_artifact,
    is_frequency_word_list, parse_frequency_word_list,
};
use ferrolex_core::{
    Checker, Dictionary, Normalization, UserDictionary, WordList, contains_normalized,
};
use ferrolex_dictionaries::{
    DictionaryInstaller, FetchError as DictionaryFetchError, InstalledDictionary,
    LIBREOFFICE_CATALOG, LibreOfficeDictionary, ManifestError as DictionaryManifestError,
    UreqFetcher, find_locale,
};
use ferrolex_hunspell::{
    Acceptance, AcceptanceKind, AppliedAffixKind, ByteImportEncodings, CasingPath,
    CompoundComponentRole, Diagnostic as ImportDiagnostic, HunspellDictionary, ImportError,
    ImportMode, ImportResult, LookupExplanation, Rejection, RejectionReason, RuntimeCacheError,
    Severity, SourceDigests, compile_runtime_artifact, compile_runtime_cache,
    import_bytes as import_hunspell_bytes,
    import_bytes_with_encodings as import_hunspell_bytes_with_encodings, inspect_runtime_cache,
    is_runtime_artifact, load_runtime_artifact, load_runtime_cache,
};
use ferrolex_suggest::{
    CandidateSource, Completeness, ReplacementRule, SuggestConfig, SuggestScratch, Suggester,
    Suggestion,
};
use ferrolex_text::check_text;
use fs2::FileExt as _;
use serde_json::json;

// ==== Command help and runtime constants ====
const USAGE: &str = "Usage: ferrolex --help | --version\n       ferrolex check [--format <text|json>] [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] [--] <WORD>\n       ferrolex check [--format <text|json>] [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] --file <PATH|-> [--file <PATH|-> ...] [<PATH> ...]\n       ferrolex suggest [--format <text|json>] [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] [--max-results <COUNT>] [--max-edit-distance <DISTANCE>] [--max-candidates <COUNT>] [--max-edit-cells <COUNT>] <WORD>\n       ferrolex explain --hunspell <AFF_PATH> <WORD>\n       ferrolex analyze [--format <text|json>] [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] [--config <PATH>] [--include <GLOB> ...] [--exclude <GLOB> ...] [--suggest] [--comment-prefix <PREFIX> | --comment-syntax html] <PATH>\n       ferrolex compile (--dictionary <PLAIN_WORD_LIST> | <AFF_PATH> <DIC_PATH>) -o <ARTIFACT>\n       ferrolex inspect <ARTIFACT>\n       ferrolex validate [--format <text|json>] [--strict] <AFF_PATH> <DIC_PATH>\n       ferrolex validate [--format <text|json>] --compiled <ARTIFACT>\n       ferrolex dictionary list\n       ferrolex dictionary fetch <LOCALE> --cache <PATH>\n       ferrolex dictionary install <LOCALE> --cache <PATH>\n       ferrolex dictionary add-word [--workspace <PATH> | --global] <WORD>";
const RUNTIME_ERROR_EXIT_CODE: u8 = 3;
const EXIT_CODES: &str =
    "\nExit status: 0 success, 1 finding, 2 usage error, 3 operational failure.";
const HELP_CHECK: &str = "Usage: ferrolex check [--format <text|json>] [--dictionary <PATH> ...] [--compiled <PATH> ...] [--hunspell <AFF_PATH> ...] [--] <WORD>\n       ferrolex check [--format <text|json>] [--dictionary <PATH> ...] [--compiled <PATH> ...] [--hunspell <AFF_PATH> ...] --file <PATH|-> [--file <PATH|-> ...] [<PATH> ...]\n\nChecks one word or every natural-language word in one or more UTF-8 inputs.\nAutomatically includes workspace and global user dictionaries when present.\nPlain word-list and compiled-dictionary checks use exact casing; Hunspell imports apply Hunspell-style capitalization fallback for initial-capital and all-uppercase input.\n  --format <text|json>  Human-readable text or JSON Lines output (default: text)\n  --dictionary <PATH>  Plain word-list dictionary (repeatable)\n  --compiled <PATH>    Compiled dictionary artifact (repeatable)\n  --hunspell <PATH>    Hunspell AFF path; uses an adjacent cache when present (repeatable)\n  --file <PATH|->      Check a UTF-8 file, or stdin with `-` (repeatable)\n  --                   End options, including before a word beginning with `-`\n\nAfter the first `--file`, positional arguments are additional file paths.\n\nExamples:\n  ferrolex check --dictionary words.txt -- --compound\n  printf 'some text' | ferrolex check --format json --dictionary words.txt --file -";
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

// ==== Entry point and command dispatch ====
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
    if let Ok(global_path) = global_user_dictionary_path()
        && global_path != paths[0]
    {
        paths.push(global_path);
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

// ==== Tests ====
#[cfg(test)]
mod tests;

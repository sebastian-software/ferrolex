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
    SuggestCommand, ValidateCommand,
};

// ==== Command implementations, output, and diagnostics ====
#[allow(unused_imports)]
pub(crate) use commands::{
    add_user_dictionary_word, analysis_paths, analyze, check, comment_syntax_for_path, compile,
    completeness_code, dictionary, explain, glob_matches, hidden_sibling,
    incomplete_suggestion_hint, inspect_artifact, install_hunspell_runtime_cache,
    load_analysis_dictionary, read_analysis_source, read_compiled_artifact, render_explanation,
    runtime_cache_path, suggest, validate, validate_hunspell, AnalysisDictionary, AnalysisSource,
    AnalysisSuggestionEngine, UserDictionaryLock,
};

// ==== CLI dependencies ====
use std::time::{Duration, SystemTime};

use ferrolex::catalog_import_encodings;
use ferrolex_code::{
    Analyzer, AnalyzerConfigError, CommentSyntax, DirectiveProblem, Document, ProjectConfig,
    ProjectConfigError,
};
use ferrolex_compiler::{
    compile_frequency_word_list, compile_words, inspect_compiled_artifact, is_frequency_word_list,
    parse_frequency_word_list, CompileError, CompiledDictionary, FrequencyListError, LoadError,
    ValidationError, MAX_COMPILED_ARTIFACT_BYTES,
};
use ferrolex_core::{
    contains_normalized, Checker, Dictionary, Normalization, UserDictionary, WordList,
};
use ferrolex_dictionaries::{
    find_locale, DictionaryInstaller, FetchError as DictionaryFetchError, InstalledDictionary,
    LibreOfficeDictionary, ManifestError as DictionaryManifestError, UreqFetcher,
    LIBREOFFICE_CATALOG,
};
use ferrolex_hunspell::{
    compile_runtime_artifact, compile_runtime_cache, import_bytes as import_hunspell_bytes,
    import_bytes_with_encodings as import_hunspell_bytes_with_encodings, inspect_runtime_cache,
    is_runtime_artifact, load_runtime_artifact, load_runtime_cache, Acceptance, AcceptanceKind,
    AppliedAffixKind, ByteImportEncodings, CasingPath, CompoundComponentRole,
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

// ==== Command help and runtime constants ====
const USAGE: &str = "Usage: ferrolex --help | --version\n       ferrolex check [--format <text|json>] [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] [--] <WORD>\n       ferrolex check [--format <text|json>] [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] --file <PATH|-> [--file <PATH|-> ...] [<PATH> ...]\n       ferrolex suggest [--format <text|json>] [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] [--max-results <COUNT>] [--max-edit-distance <DISTANCE>] [--max-candidates <COUNT>] [--max-edit-cells <COUNT>] <WORD>\n       ferrolex explain --hunspell <AFF_PATH> <WORD>\n       ferrolex analyze [--format <text|json>] [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] [--config <PATH>] [--include <GLOB> ...] [--exclude <GLOB> ...] [--suggest] [--comment-prefix <PREFIX> | --comment-syntax html] <PATH>\n       ferrolex compile (--dictionary <PLAIN_WORD_LIST> | <AFF_PATH> <DIC_PATH>) -o <ARTIFACT>\n       ferrolex inspect <ARTIFACT>\n       ferrolex validate [--format <text|json>] [--strict] <AFF_PATH> <DIC_PATH>\n       ferrolex validate [--format <text|json>] --compiled <ARTIFACT>\n       ferrolex dictionary list\n       ferrolex dictionary fetch <LOCALE> --cache <PATH>\n       ferrolex dictionary install <LOCALE> --cache <PATH>\n       ferrolex dictionary add-word [--workspace <PATH> | --global] <WORD>";
const RUNTIME_ERROR_EXIT_CODE: u8 = 3;
const EXIT_CODES: &str =
    "\nExit status: 0 success, 1 finding, 2 usage error, 3 operational failure.";
const HELP_CHECK: &str = "Usage: ferrolex check [--format <text|json>] [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] [--] <WORD>\n       ferrolex check [--format <text|json>] [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] --file <PATH|-> [--file <PATH|-> ...] [<PATH> ...]\n\nChecks one word or every natural-language word in one or more UTF-8 inputs.\nAutomatically includes workspace and global user dictionaries when present.\nPlain word-list and compiled-dictionary checks use exact casing; Hunspell imports apply Hunspell-style capitalization fallback for initial-capital and all-uppercase input.\n  --format <text|json>  Human-readable text or JSON Lines output (default: text)\n  --dictionary <PATH>  Plain word-list dictionary (repeatable)\n  --compiled <PATH>    Compiled dictionary artifact (repeatable)\n  --hunspell <PATH>    Hunspell AFF path; uses an adjacent cache when present (repeatable)\n  --file <PATH|->      Check a UTF-8 file, or stdin with `-` (repeatable)\n  --                   End options, including before a word beginning with `-`\n\nAfter the first `--file`, positional arguments are additional file paths.\n\nExamples:\n  ferrolex check --dictionary words.txt -- --compound\n  printf 'some text' | ferrolex check --format json --dictionary words.txt --file -";
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

// ==== Argument parsing ====
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

// ==== Tests ====
#[cfg(test)]
mod tests;

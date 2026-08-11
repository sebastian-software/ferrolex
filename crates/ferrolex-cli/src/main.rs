//! Command-line interface for ferrolex.

#![forbid(unsafe_code)]

use std::env;
use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};

use ferrolex_code::{
    Analyzer, AnalyzerConfigError, CommentSyntax, Document, ProjectConfig, ProjectConfigError,
};
use ferrolex_compiler::{
    compile_words, CompileError, CompiledDictionary, LoadError, ValidationError,
    MAX_COMPILED_ARTIFACT_BYTES,
};
use ferrolex_core::{Checker, Dictionary, Normalization, WordList};
use ferrolex_dictionaries::{
    find_locale, DictionaryInstaller, FetchError as DictionaryFetchError, InstalledDictionary,
    LibreOfficeDictionary, ManifestError as DictionaryManifestError, SourceEncoding, UreqFetcher,
    LIBREOFFICE_CATALOG,
};
use ferrolex_hunspell::{
    compile_runtime_cache, import_bytes as import_hunspell_bytes,
    import_bytes_with_encodings as import_hunspell_bytes_with_encodings, load_runtime_cache,
    ByteEncoding, ByteImportEncodings, Diagnostic as ImportDiagnostic, HunspellDictionary,
    ImportError, ImportMode, ImportResult, RuntimeCacheError, Severity, SourceDigests,
};
use ferrolex_suggest::{CandidateSource, Completeness, ReplacementRule, SuggestConfig, Suggester};
use ferrolex_text::check_text;

const USAGE: &str = "Usage: ferrolex check [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] <WORD>\n       ferrolex check [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] --file <PATH>\n       ferrolex suggest (--dictionary <PLAIN_WORD_LIST> | --compiled <ARTIFACT> | --hunspell <AFF_PATH>) [--max-results <COUNT>] [--max-edit-distance <DISTANCE>] [--max-candidates <COUNT>] [--max-edit-cells <COUNT>] <WORD>\n       ferrolex analyze [--dictionary <PATH> ...] [--compiled <ARTIFACT> ...] [--hunspell <AFF_PATH> ...] [--config <PATH>] [--comment-prefix <PREFIX>] <PATH>\n       ferrolex compile --dictionary <PLAIN_WORD_LIST> -o <ARTIFACT>\n       ferrolex validate [--strict] <AFF_PATH> <DIC_PATH>\n       ferrolex validate --compiled <ARTIFACT>\n       ferrolex dictionary list\n       ferrolex dictionary fetch <LOCALE> --cache <PATH>\n       ferrolex dictionary install <LOCALE> --cache <PATH>";

const HUNSPELL_RUNTIME_CACHE_EXTENSION: &str = "ferrolex-hunspell-v1.flexh";
static CACHE_WRITE_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn main() -> ExitCode {
    match run(env::args()) {
        Ok(outcome) => outcome.exit_code(),
        Err(error) => {
            eprintln!("error: {error}");
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run(arguments: impl IntoIterator<Item = String>) -> Result<RunOutcome, CliError> {
    match parse_arguments(arguments)? {
        Command::Help => {
            println!("{USAGE}");
            Ok(RunOutcome::Success)
        }
        Command::Check(command) => check(&command),
        Command::Suggest(command) => suggest(&command),
        Command::Analyze(command) => analyze(&command),
        Command::Compile(command) => compile(&command),
        Command::Validate(command) => validate(&command),
        Command::Dictionary(command) => dictionary(&command),
    }
}

fn suggest(command: &SuggestCommand) -> Result<RunOutcome, CliError> {
    let (source, replacements, output_dictionary): (
        Box<dyn CandidateSource>,
        Vec<ReplacementRule>,
        Option<HunspellDictionary>,
    ) = match (
        command.dictionary_path.as_ref(),
        command.compiled_path.as_ref(),
        command.hunspell_affix_path.as_ref(),
    ) {
        (Some(path), None, None) => {
            let text = fs::read_to_string(path).map_err(|source| CliError::ReadDictionary {
                path: path.clone(),
                source,
            })?;
            (
                Box::new(WordList::from_text(Normalization::Exact, &text)),
                Vec::new(),
                None,
            )
        }
        (None, Some(path), None) => (
            Box::new(
                CompiledDictionary::load(read_compiled_artifact(path)?).map_err(|source| {
                    CliError::LoadArtifact {
                        path: path.clone(),
                        source,
                    }
                })?,
            ),
            Vec::new(),
            None,
        ),
        (None, None, Some(path)) => {
            let dictionary = load_installed_hunspell_dictionary(path)?;
            let replacements = dictionary.replacement_rules().to_vec();
            (Box::new(dictionary.clone()), replacements, Some(dictionary))
        }
        _ => unreachable!("suggest command parsing requires exactly one source"),
    };
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
    let result = Suggester::new(source.as_ref(), config)
        .with_replacement_rules(&replacements)
        .suggest(&command.word);
    for suggestion in result.suggestions() {
        println!(
            "suggestion: {} (distance {})",
            output_dictionary.as_ref().map_or_else(
                || suggestion.word().to_owned(),
                |dictionary| dictionary.normalize_output(suggestion.word()),
            ),
            suggestion.distance()
        );
    }
    if result.completeness() != Completeness::Complete {
        eprintln!(
            "suggestion search incomplete: {}",
            completeness_label(result.completeness())
        );
    }
    Ok(RunOutcome::Success)
}

const fn completeness_label(completeness: Completeness) -> &'static str {
    match completeness {
        Completeness::Complete => "complete",
        Completeness::CandidateLimitReached => "candidate limit reached",
        Completeness::EditBudgetReached => "edit-distance budget reached",
        Completeness::QueryTooLong => "query exceeds the scalar limit",
    }
}

fn dictionary(command: &DictionaryCommand) -> Result<RunOutcome, CliError> {
    match command {
        DictionaryCommand::List => {
            for source in LIBREOFFICE_CATALOG {
                println!(
                    "{}\trevision={}\tencoding={}\tlicense={}\tnotice={}",
                    source.locale(),
                    source.revision(),
                    source.encoding().label(),
                    source.license_label(),
                    source.license_notice_url()
                );
            }
            Ok(RunOutcome::Success)
        }
        DictionaryCommand::Fetch { locale, cache_path } => {
            let (source, installed) = fetch_catalog_dictionary(locale, cache_path)?;
            println!("installed: {}", installed.aff_path().display());
            println!("installed: {}", installed.dic_path().display());
            println!("license: {}", source.license_label());
            println!("notice: {}", source.license_notice_url());
            Ok(RunOutcome::Success)
        }
        DictionaryCommand::Install { locale, cache_path } => {
            let (source, installed) = fetch_catalog_dictionary(locale, cache_path)?;
            println!("installed: {}", installed.aff_path().display());
            println!("installed: {}", installed.dic_path().display());
            println!("license: {}", source.license_label());
            println!("notice: {}", source.license_notice_url());
            install_hunspell_runtime_cache(
                source.locale(),
                installed.aff_path(),
                installed.dic_path(),
                catalog_import_encodings(source.encoding()),
            )
        }
    }
}

fn catalog_import_encodings(encoding: SourceEncoding) -> Option<ByteImportEncodings> {
    match encoding {
        SourceEncoding::MixedUtf8AndIso8859_1 => Some(ByteImportEncodings::new(
            ByteEncoding::Iso8859_1,
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
    let installed = DictionaryInstaller::new(UreqFetcher)
        .install(&manifest, cache_path)
        .map_err(CliError::FetchDictionary)?;
    Ok((source, installed))
}

fn compile(command: &CompileCommand) -> Result<RunOutcome, CliError> {
    let text = fs::read_to_string(&command.dictionary_path).map_err(|source| {
        CliError::ReadDictionary {
            path: command.dictionary_path.clone(),
            source,
        }
    })?;
    let dictionary = WordList::from_text(Normalization::Exact, &text);
    let compiled = compile_words(dictionary.words()).map_err(CliError::CompileDictionary)?;
    fs::write(&command.output_path, compiled).map_err(|source| CliError::WriteArtifact {
        path: command.output_path.clone(),
        source,
    })?;

    println!(
        "compiled: {} ({} words)",
        command.output_path.display(),
        dictionary.len()
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
        CheckTarget::Word(word) => Ok(check_word(&checker, word)),
        CheckTarget::File(path) => check_file(&checker, path),
    }
}

fn load_checker(
    dictionary_paths: &[PathBuf],
    compiled_paths: &[PathBuf],
    hunspell_affix_paths: &[PathBuf],
) -> Result<Checker, CliError> {
    let mut builder = Checker::builder();
    for path in dictionary_paths {
        let text = fs::read_to_string(path).map_err(|source| CliError::ReadDictionary {
            path: path.clone(),
            source,
        })?;
        builder = builder.dictionary(WordList::from_text(Normalization::Exact, &text));
    }
    for path in compiled_paths {
        let bytes = read_compiled_artifact(path)?;
        let dictionary =
            CompiledDictionary::load(bytes).map_err(|source| CliError::LoadArtifact {
                path: path.clone(),
                source,
            })?;
        builder = builder.dictionary(dictionary);
    }
    for aff_path in hunspell_affix_paths {
        builder = builder.dictionary(load_installed_hunspell_dictionary(aff_path)?);
    }

    Ok(builder.build())
}

fn load_installed_hunspell_dictionary(aff_path: &Path) -> Result<HunspellDictionary, CliError> {
    let dic_path = aff_path.with_extension("dic");
    let cache_path = runtime_cache_path(aff_path);
    let aff_bytes = fs::read(aff_path).map_err(|source| CliError::ReadInput {
        path: aff_path.to_path_buf(),
        source,
    })?;
    let dic_bytes = fs::read(&dic_path).map_err(|source| CliError::ReadInput {
        path: dic_path,
        source,
    })?;
    let cache = fs::read(&cache_path).map_err(|source| CliError::ReadHunspellCache {
        path: cache_path.clone(),
        source,
    })?;

    load_runtime_cache(
        &cache,
        SourceDigests::from_source_bytes(&aff_bytes, &dic_bytes),
    )
    .map_err(|source| CliError::LoadHunspellCache {
        path: cache_path,
        source,
    })
}

fn check_word(checker: &Checker, word: &str) -> RunOutcome {
    if checker.contains(word) {
        println!("accepted: {word}");
        RunOutcome::Success
    } else {
        println!("misspelled: {word}");
        RunOutcome::Misspelled
    }
}

fn check_file(checker: &Checker, path: &Path) -> Result<RunOutcome, CliError> {
    let text = fs::read_to_string(path).map_err(|source| CliError::ReadInput {
        path: path.to_path_buf(),
        source,
    })?;
    let mut misspelled = false;

    for issue in check_text(checker, &text) {
        print_finding(path, &text, issue.range().start, issue.word());
        misspelled = true;
    }

    Ok(if misspelled {
        RunOutcome::Misspelled
    } else {
        RunOutcome::Success
    })
}

fn analyze(command: &AnalyzeCommand) -> Result<RunOutcome, CliError> {
    let checker = load_checker(
        &command.dictionary_paths,
        &command.compiled_paths,
        &command.hunspell_affix_paths,
    )?;
    let source = fs::read_to_string(&command.path).map_err(|source| CliError::ReadInput {
        path: command.path.clone(),
        source,
    })?;
    let document = match &command.comment_prefix {
        Some(prefix) => Document::new(&source).with_comment_syntax(CommentSyntax::line(prefix)),
        None => Document::new(&source),
    };
    let mut builder = Analyzer::builder(&checker);
    if let Some(config_path) = &command.config_path {
        let text =
            fs::read_to_string(config_path).map_err(|source| CliError::ReadProjectConfig {
                path: config_path.clone(),
                source,
            })?;
        let config = ProjectConfig::from_text(&text).map_err(|source| CliError::ProjectConfig {
            path: config_path.clone(),
            source,
        })?;
        builder =
            builder
                .project_config(&config)
                .map_err(|source| CliError::ApplyProjectConfig {
                    path: config_path.clone(),
                    source,
                })?;
    }
    let analysis = builder.build().check(&document);
    let mut has_diagnostic = false;

    for finding in analysis.findings() {
        print_finding(
            &command.path,
            &source,
            finding.range().start,
            finding.word(),
        );
        has_diagnostic = true;
    }
    for diagnostic in analysis.directive_diagnostics() {
        let (line, column) = line_and_column(&source, diagnostic.range().start);
        println!(
            "{}:{line}:{column}: malformed directive: {:?}",
            command.path.display(),
            diagnostic.problem()
        );
        has_diagnostic = true;
    }

    Ok(if has_diagnostic {
        RunOutcome::Misspelled
    } else {
        RunOutcome::Success
    })
}

fn validate(command: &ValidateCommand) -> Result<RunOutcome, CliError> {
    match command {
        ValidateCommand::Hunspell {
            strict,
            aff_path,
            dic_path,
        } => validate_hunspell(*strict, aff_path, dic_path, None),
        ValidateCommand::Compiled { path } => validate_compiled(path),
    }
}

fn validate_hunspell(
    strict: bool,
    aff_path: &Path,
    dic_path: &Path,
    encodings: Option<ByteImportEncodings>,
) -> Result<RunOutcome, CliError> {
    let (import, _) = import_hunspell_files(aff_path, dic_path, encodings, strict)?;
    Ok(report_hunspell_import(import, dic_path))
}

fn install_hunspell_runtime_cache(
    locale: &str,
    aff_path: &Path,
    dic_path: &Path,
    encodings: Option<ByteImportEncodings>,
) -> Result<RunOutcome, CliError> {
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

    let import = match encodings {
        Some(encodings) => import_hunspell_bytes_with_encodings(
            &aff_source,
            &aff_bytes,
            &dic_source,
            &dic_bytes,
            encodings,
            mode,
        ),
        None => import_hunspell_bytes(&aff_source, &aff_bytes, &dic_source, &dic_bytes, mode),
    };

    Ok((
        import,
        SourceDigests::from_source_bytes(&aff_bytes, &dic_bytes),
    ))
}

fn report_hunspell_import(
    import: Result<ImportResult, ImportError>,
    dic_path: &Path,
) -> RunOutcome {
    match import {
        Ok(result) => {
            let has_errors = result
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.severity() == Severity::Error);
            for diagnostic in result.diagnostics() {
                print_import_diagnostic(diagnostic);
            }
            if has_errors {
                RunOutcome::Misspelled
            } else {
                println!("valid: {}", dic_path.display());
                RunOutcome::Success
            }
        }
        Err(error) => {
            for diagnostic in error.diagnostics() {
                print_import_diagnostic(diagnostic);
            }
            RunOutcome::Misspelled
        }
    }
}

fn atomic_write_runtime_cache(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let temporary = path.with_extension(format!(
        "tmp-{}-{}",
        std::process::id(),
        CACHE_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
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
        })
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn validate_compiled(path: &Path) -> Result<RunOutcome, CliError> {
    let bytes = read_compiled_artifact(path)?;
    let dictionary = CompiledDictionary::load(bytes).map_err(|source| CliError::LoadArtifact {
        path: path.to_path_buf(),
        source,
    })?;
    dictionary
        .validate()
        .map_err(|source| CliError::ValidateArtifact {
            path: path.to_path_buf(),
            source,
        })?;
    println!("valid: {}", path.display());
    Ok(RunOutcome::Success)
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
    let severity = match diagnostic.severity() {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    println!(
        "{}:{}: {severity}[{}]: {}",
        diagnostic.source(),
        diagnostic.line(),
        diagnostic.directive(),
        diagnostic.message()
    );
}

fn print_finding(path: &Path, source: &str, byte_offset: usize, word: &str) {
    let (line, column) = line_and_column(source, byte_offset);
    println!("{}:{line}:{column}: misspelled: {word}", path.display());
}

fn line_and_column(text: &str, byte_offset: usize) -> (usize, usize) {
    let prefix = &text[..byte_offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, current_line)| current_line)
        .chars()
        .count()
        + 1;

    (line, column)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RunOutcome {
    Success,
    Misspelled,
}

impl RunOutcome {
    fn exit_code(self) -> ExitCode {
        match self {
            Self::Success => ExitCode::SUCCESS,
            Self::Misspelled => ExitCode::from(1),
        }
    }
}

fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Command, CliError> {
    let mut arguments = arguments.into_iter();
    let _program_name = arguments.next();

    match arguments.next().as_deref() {
        Some("--help" | "-h") => Ok(Command::Help),
        Some("check") => parse_check_arguments(arguments),
        Some("suggest") => parse_suggest_arguments(arguments),
        Some("analyze") => parse_analyze_arguments(arguments),
        Some("compile") => parse_compile_arguments(arguments),
        Some("validate") => parse_validate_arguments(arguments),
        Some("dictionary") => parse_dictionary_arguments(arguments),
        Some(command) => Err(CliError::Usage(format!("unknown command `{command}`"))),
        None => Err(CliError::Usage("missing command".to_owned())),
    }
}

fn parse_suggest_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Command, CliError> {
    let mut dictionary_path = None;
    let mut compiled_path = None;
    let mut hunspell_affix_path = None;
    let mut max_results = None;
    let mut max_edit_distance = None;
    let mut max_candidates = None;
    let mut max_edit_cells = None;
    let mut word = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--dictionary" => set_once_path(&mut dictionary_path, &mut arguments, "--dictionary")?,
            "--hunspell" => set_once_path(&mut hunspell_affix_path, &mut arguments, "--hunspell")?,
            "--compiled" => set_once_path(&mut compiled_path, &mut arguments, "--compiled")?,
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
            "--help" | "-h" => return Ok(Command::Help),
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
    if usize::from(dictionary_path.is_some())
        + usize::from(compiled_path.is_some())
        + usize::from(hunspell_affix_path.is_some())
        != 1
    {
        return Err(CliError::Usage(
            "suggest requires exactly one `--dictionary`, `--compiled`, or `--hunspell` path"
                .to_owned(),
        ));
    }
    let word =
        word.ok_or_else(|| CliError::Usage("suggest requires exactly one word".to_owned()))?;
    Ok(Command::Suggest(SuggestCommand {
        dictionary_path,
        compiled_path,
        hunspell_affix_path,
        max_results,
        max_edit_distance,
        max_candidates,
        max_edit_cells,
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
        Some("--help" | "-h") => Ok(Command::Help),
        Some(subcommand) => Err(CliError::Usage(format!(
            "unknown dictionary subcommand `{subcommand}`"
        ))),
        None => Err(CliError::Usage(
            "dictionary requires `list`, `fetch`, or `install`".to_owned(),
        )),
    }
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
            "--help" | "-h" => return Ok(Command::Help),
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
            "--help" | "-h" => return Ok(Command::Help),
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
        return Ok(Command::Validate(ValidateCommand::Compiled { path }));
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
    }))
}

fn parse_compile_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Command, CliError> {
    let mut dictionary_path = None;
    let mut output_path = None;
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
            "--help" | "-h" => return Ok(Command::Help),
            option if option.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{option}`")));
            }
            _ => {
                return Err(CliError::Usage(
                    "compile does not accept positional arguments".to_owned(),
                ));
            }
        }
    }

    let dictionary_path = dictionary_path
        .ok_or_else(|| CliError::Usage("compile requires a `--dictionary` path".to_owned()))?;
    let output_path = output_path
        .ok_or_else(|| CliError::Usage("compile requires an `-o` artifact path".to_owned()))?;

    Ok(Command::Compile(CompileCommand {
        dictionary_path,
        output_path,
    }))
}

fn parse_analyze_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Command, CliError> {
    let mut dictionary_paths = Vec::new();
    let mut compiled_paths = Vec::new();
    let mut hunspell_affix_paths = Vec::new();
    let mut config_path = None;
    let mut comment_prefix = None;
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
            "--config" => set_once_path(&mut config_path, &mut arguments, "--config")?,
            "--comment-prefix" => {
                let prefix = arguments.next().ok_or_else(|| {
                    CliError::Usage("`--comment-prefix` requires a prefix".to_owned())
                })?;
                if prefix.is_empty() || prefix.starts_with('-') {
                    return Err(CliError::Usage(
                        "`--comment-prefix` requires a non-option prefix".to_owned(),
                    ));
                }
                if comment_prefix.replace(prefix).is_some() {
                    return Err(CliError::Usage(
                        "`--comment-prefix` may only be supplied once".to_owned(),
                    ));
                }
            }
            "--help" | "-h" => return Ok(Command::Help),
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

    if dictionary_paths.is_empty() && compiled_paths.is_empty() && hunspell_affix_paths.is_empty() {
        return Err(CliError::Usage(
            "analyze requires at least one `--dictionary` or `--hunspell` path".to_owned(),
        ));
    }
    let path = path.ok_or_else(|| CliError::Usage("analyze requires a path".to_owned()))?;

    Ok(Command::Analyze(AnalyzeCommand {
        dictionary_paths,
        compiled_paths,
        hunspell_affix_paths,
        config_path,
        comment_prefix,
        path,
    }))
}

fn parse_check_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Command, CliError> {
    let mut dictionary_paths = Vec::new();
    let mut compiled_paths = Vec::new();
    let mut hunspell_affix_paths = Vec::new();
    let mut target = None;
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
            "--file" => {
                set_target(
                    &mut target,
                    CheckTarget::File(required_path(&mut arguments, "--file")?),
                )?;
            }
            "--help" | "-h" => return Ok(Command::Help),
            option if option.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{option}`")));
            }
            _ => set_target(&mut target, CheckTarget::Word(argument))?,
        }
    }

    let target =
        target.ok_or_else(|| CliError::Usage("check requires a word or `--file`".to_owned()))?;
    if dictionary_paths.is_empty() && compiled_paths.is_empty() && hunspell_affix_paths.is_empty() {
        return Err(CliError::Usage(
            "check requires at least one `--dictionary` or `--hunspell` path".to_owned(),
        ));
    }

    Ok(Command::Check(CheckCommand {
        dictionary_paths,
        compiled_paths,
        hunspell_affix_paths,
        target,
    }))
}

fn required_path(
    arguments: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<PathBuf, CliError> {
    let path = arguments
        .next()
        .ok_or_else(|| CliError::Usage(format!("`{option}` requires a path")))?;
    if path.starts_with('-') {
        return Err(CliError::Usage(format!("`{option}` requires a path")));
    }

    Ok(PathBuf::from(path))
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

fn set_target(target: &mut Option<CheckTarget>, value: CheckTarget) -> Result<(), CliError> {
    if target.replace(value).is_some() {
        return Err(CliError::Usage(
            "check accepts exactly one word or `--file` target".to_owned(),
        ));
    }

    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Check(CheckCommand),
    Suggest(SuggestCommand),
    Analyze(AnalyzeCommand),
    Compile(CompileCommand),
    Validate(ValidateCommand),
    Dictionary(DictionaryCommand),
    Help,
}

#[derive(Debug, Eq, PartialEq)]
struct CheckCommand {
    dictionary_paths: Vec<PathBuf>,
    compiled_paths: Vec<PathBuf>,
    hunspell_affix_paths: Vec<PathBuf>,
    target: CheckTarget,
}

#[derive(Debug, Eq, PartialEq)]
struct SuggestCommand {
    dictionary_path: Option<PathBuf>,
    compiled_path: Option<PathBuf>,
    hunspell_affix_path: Option<PathBuf>,
    max_results: Option<usize>,
    max_edit_distance: Option<usize>,
    max_candidates: Option<usize>,
    max_edit_cells: Option<usize>,
    word: String,
}

#[derive(Debug, Eq, PartialEq)]
enum CheckTarget {
    Word(String),
    File(PathBuf),
}

#[derive(Debug, Eq, PartialEq)]
struct AnalyzeCommand {
    dictionary_paths: Vec<PathBuf>,
    compiled_paths: Vec<PathBuf>,
    hunspell_affix_paths: Vec<PathBuf>,
    config_path: Option<PathBuf>,
    comment_prefix: Option<String>,
    path: PathBuf,
}

#[derive(Debug, Eq, PartialEq)]
struct CompileCommand {
    dictionary_path: PathBuf,
    output_path: PathBuf,
}

#[derive(Debug, Eq, PartialEq)]
enum ValidateCommand {
    Hunspell {
        strict: bool,
        aff_path: PathBuf,
        dic_path: PathBuf,
    },
    Compiled {
        path: PathBuf,
    },
}

#[derive(Debug, Eq, PartialEq)]
enum DictionaryCommand {
    List,
    Fetch { locale: String, cache_path: PathBuf },
    Install { locale: String, cache_path: PathBuf },
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
    WriteArtifact {
        path: PathBuf,
        source: io::Error,
    },
    CompileDictionary(CompileError),
    LoadArtifact {
        path: PathBuf,
        source: LoadError,
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
                    "could not read Hunspell runtime cache `{}`: {source}",
                    path.display()
                )
            }
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
            Self::LoadArtifact { path, source } => {
                write!(
                    formatter,
                    "invalid compiled artifact `{}`: {source}",
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
                    "invalid or stale Hunspell runtime cache `{}`: {source}; rerun `ferrolex dictionary install`",
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
            | Self::ReadProjectConfig { source, .. } => Some(source),
            Self::CompileDictionary(source) => Some(source),
            Self::LoadArtifact { source, .. } => Some(source),
            Self::ValidateArtifact { source, .. } => Some(source),
            Self::CompileHunspellCache(source) | Self::LoadHunspellCache { source, .. } => {
                Some(source)
            }
            Self::ProjectConfig { source, .. } => Some(source),
            Self::ApplyProjectConfig { source, .. } => Some(source),
            Self::DictionaryManifest(source) => Some(source),
            Self::FetchDictionary(source) => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ferrolex_compiler::{CompiledDictionary, ValidationError, MAX_COMPILED_ARTIFACT_BYTES};
    use ferrolex_core::Dictionary;
    use ferrolex_hunspell::{load_runtime_cache, CacheSource, RuntimeCacheError, SourceDigests};

    use super::{
        catalog_import_encodings, install_hunspell_runtime_cache, line_and_column, parse_arguments,
        read_compiled_artifact, run, runtime_cache_path, validate_hunspell, AnalyzeCommand,
        CheckCommand, CheckTarget, CliError, Command, CompileCommand, DictionaryCommand,
        RunOutcome, SourceEncoding, SuggestCommand, ValidateCommand,
    };

    static NEXT_TEMPORARY_FILE: AtomicUsize = AtomicUsize::new(0);

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
                target: CheckTarget::Word("OAuth".to_owned()),
            })
        );
    }

    #[test]
    fn rejects_missing_dictionary_paths() {
        let error = parse_arguments(["ferrolex", "check", "word"].map(str::to_owned))
            .expect_err("a dictionary is required");

        assert!(matches!(error, CliError::Usage(message) if message.contains("--dictionary")));
    }

    #[test]
    fn accepts_help_after_the_check_command() {
        let command = parse_arguments(["ferrolex", "check", "--help"].map(str::to_owned))
            .expect("help is always valid");

        assert_eq!(command, Command::Help);
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
                comment_prefix: Some("//".to_owned()),
                path: PathBuf::from("lib.rs"),
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
                comment_prefix: None,
                path: PathBuf::from("src/lib.rs"),
            })
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
                target: CheckTarget::Word("Wort".to_owned()),
            })
        );
        assert_eq!(
            analyze,
            Command::Analyze(AnalyzeCommand {
                dictionary_paths: Vec::new(),
                compiled_paths: Vec::new(),
                hunspell_affix_paths: vec![PathBuf::from("de.aff")],
                config_path: None,
                comment_prefix: None,
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
                dictionary_path: PathBuf::from("words.txt"),
                output_path: PathBuf::from("words.flex"),
            })
        );
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
                dictionary_path: Some(PathBuf::from("words.txt")),
                compiled_path: None,
                hunspell_affix_path: None,
                max_results: None,
                max_edit_distance: None,
                max_candidates: None,
                max_edit_cells: None,
                word: "recieve".to_owned(),
            })
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
                dictionary_path: Some(PathBuf::from("words.txt")),
                compiled_path: None,
                hunspell_affix_path: None,
                max_results: Some(3),
                max_edit_distance: Some(0),
                max_candidates: Some(300),
                max_edit_cells: Some(12_000),
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
                dictionary_path: None,
                compiled_path: None,
                hunspell_affix_path: Some(PathBuf::from("de.aff")),
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
                "--hunspell",
                "de.aff",
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
        assert_eq!(line_and_column("Café\nStrasse", 6), (2, 1));
    }

    struct TemporaryDictionary {
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

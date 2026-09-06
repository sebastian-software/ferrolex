//! CLI command types, output state, and user-facing errors.

use super::*;

use clap::{ArgAction, Args, Parser, Subcommand};

pub(crate) struct LineIndex {
    pub(crate) starts: Vec<usize>,
}

impl LineIndex {
    pub(crate) fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter_map(|(offset, byte)| (byte == b'\n').then_some(offset + 1)),
        );
        Self { starts }
    }

    pub(crate) fn line_and_column(&self, text: &str, byte_offset: usize) -> (usize, usize) {
        let line_index = self.starts.partition_point(|start| *start <= byte_offset) - 1;
        let column = text[self.starts[line_index]..byte_offset].chars().count() + 1;
        (line_index + 1, column)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RunOutcome {
    Success,
    Misspelled,
    Failure,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum OutputFormat {
    #[default]
    Text,
    Json,
}

impl RunOutcome {
    pub(crate) fn exit_code(self) -> ExitCode {
        match self {
            Self::Success => ExitCode::SUCCESS,
            Self::Misspelled => ExitCode::from(1),
            Self::Failure => ExitCode::from(RUNTIME_ERROR_EXIT_CODE),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Command {
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
pub(crate) struct CheckCommand {
    pub(crate) dictionary_paths: Vec<PathBuf>,
    pub(crate) compiled_paths: Vec<PathBuf>,
    pub(crate) hunspell_affix_paths: Vec<PathBuf>,
    pub(crate) output_format: OutputFormat,
    pub(crate) target: CheckTarget,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct SuggestCommand {
    pub(crate) dictionary_paths: Vec<PathBuf>,
    pub(crate) compiled_paths: Vec<PathBuf>,
    pub(crate) hunspell_affix_paths: Vec<PathBuf>,
    pub(crate) max_results: Option<usize>,
    pub(crate) max_edit_distance: Option<usize>,
    pub(crate) max_candidates: Option<usize>,
    pub(crate) max_edit_cells: Option<usize>,
    pub(crate) output_format: OutputFormat,
    pub(crate) word: String,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ExplainCommand {
    pub(crate) hunspell_affix_path: PathBuf,
    pub(crate) word: String,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum CheckTarget {
    Word(String),
    Inputs(Vec<CheckInput>),
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum CheckInput {
    File(PathBuf),
    Stdin,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct AnalyzeCommand {
    pub(crate) dictionary_paths: Vec<PathBuf>,
    pub(crate) compiled_paths: Vec<PathBuf>,
    pub(crate) hunspell_affix_paths: Vec<PathBuf>,
    pub(crate) config_path: Option<PathBuf>,
    pub(crate) comment_syntax: Option<CommentSyntax>,
    pub(crate) include_patterns: Vec<String>,
    pub(crate) exclude_patterns: Vec<String>,
    pub(crate) suggest: bool,
    pub(crate) output_format: OutputFormat,
    pub(crate) path: PathBuf,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct CompileCommand {
    pub(crate) input: CompileInput,
    pub(crate) output_path: PathBuf,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum CompileInput {
    WordList(PathBuf),
    Hunspell {
        aff_path: PathBuf,
        dic_path: PathBuf,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ValidateCommand {
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
pub(crate) enum DictionaryCommand {
    List,
    Fetch { locale: String, cache_path: PathBuf },
    Install { locale: String, cache_path: PathBuf },
    AddWord { word: String, path: PathBuf },
}

// The public command model above deliberately stays independent from clap so
// the command implementations and library users of the test-only parser do
// not depend on clap's generated types. These raw types provide the single
// declarative boundary between argv and that model.
#[derive(Debug, Parser)]
#[command(
    name = "ferrolex",
    disable_help_flag = true,
    disable_version_flag = true
)]
struct RawCli {
    #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
    help: bool,
    #[arg(short = 'V', long = "version", action = ArgAction::SetTrue)]
    version: bool,
    #[command(subcommand)]
    command: Option<RawCommand>,
}

#[derive(Debug, Subcommand)]
enum RawCommand {
    Check(RawCheck),
    Suggest(RawSuggest),
    Explain(RawExplain),
    Analyze(RawAnalyze),
    Compile(RawCompile),
    Inspect(RawInspect),
    Validate(RawValidate),
    #[command(subcommand)]
    Dictionary(RawDictionary),
}

#[derive(Debug, Args)]
struct RawSources {
    #[arg(long = "dictionary", value_name = "PATH", action = ArgAction::Append)]
    dictionary: Vec<String>,
    #[arg(long = "compiled", value_name = "PATH", action = ArgAction::Append)]
    compiled: Vec<String>,
    #[arg(long = "hunspell", value_name = "AFF_PATH", action = ArgAction::Append)]
    hunspell: Vec<String>,
    #[arg(long = "format", value_name = "text|json", action = ArgAction::Append)]
    format: Vec<String>,
}

#[derive(Debug, Args)]
struct RawCheck {
    #[command(flatten)]
    sources: RawSources,
    #[arg(long = "file", value_name = "PATH|-", action = ArgAction::Append)]
    files: Vec<String>,
    #[arg(value_name = "WORD_OR_PATH", num_args = 0.., allow_hyphen_values = true)]
    positionals: Vec<String>,
}

#[derive(Debug, Args)]
struct RawSuggest {
    #[command(flatten)]
    sources: RawSources,
    #[arg(long = "max-results", value_name = "COUNT", action = ArgAction::Append)]
    max_results: Vec<usize>,
    #[arg(long = "max-edit-distance", value_name = "DISTANCE", action = ArgAction::Append)]
    max_edit_distance: Vec<usize>,
    #[arg(long = "max-candidates", value_name = "COUNT", action = ArgAction::Append)]
    max_candidates: Vec<usize>,
    #[arg(long = "max-edit-cells", value_name = "COUNT", action = ArgAction::Append)]
    max_edit_cells: Vec<usize>,
    #[arg(value_name = "WORD")]
    word: String,
}

#[derive(Debug, Args)]
struct RawExplain {
    #[arg(long = "hunspell", value_name = "AFF_PATH", action = ArgAction::Append)]
    hunspell: Vec<String>,
    #[arg(value_name = "WORD")]
    word: String,
}

#[derive(Debug, Args)]
struct RawAnalyze {
    #[command(flatten)]
    sources: RawSources,
    #[arg(long = "config", value_name = "PATH", action = ArgAction::Append)]
    config: Vec<String>,
    #[arg(long = "include", value_name = "GLOB", action = ArgAction::Append)]
    include: Vec<String>,
    #[arg(long = "exclude", value_name = "GLOB", action = ArgAction::Append)]
    exclude: Vec<String>,
    #[arg(long = "suggest", action = ArgAction::SetTrue)]
    suggest: bool,
    #[arg(
        long = "comment-prefix",
        value_name = "PREFIX",
        action = ArgAction::Append,
        allow_hyphen_values = true
    )]
    comment_prefix: Vec<String>,
    #[arg(long = "comment-syntax", value_name = "SYNTAX", action = ArgAction::Append)]
    comment_syntax: Vec<String>,
    #[arg(value_name = "PATH", num_args = 0..)]
    paths: Vec<String>,
}

#[derive(Debug, Args)]
struct RawCompile {
    #[arg(long = "dictionary", value_name = "PATH", action = ArgAction::Append)]
    dictionary: Vec<String>,
    #[arg(short = 'o', value_name = "ARTIFACT", action = ArgAction::Append)]
    output: Vec<String>,
    #[arg(value_name = "PATH", num_args = 0..)]
    paths: Vec<String>,
}

#[derive(Debug, Args)]
struct RawInspect {
    #[arg(value_name = "ARTIFACT")]
    path: String,
}

#[derive(Debug, Args)]
struct RawValidate {
    #[arg(long = "format", value_name = "text|json", action = ArgAction::Append)]
    format: Vec<String>,
    #[arg(long = "strict", action = ArgAction::SetTrue)]
    strict: bool,
    #[arg(long = "compiled", value_name = "ARTIFACT", action = ArgAction::Append)]
    compiled: Vec<String>,
    #[arg(value_name = "PATH", num_args = 0..)]
    paths: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum RawDictionary {
    List,
    Fetch {
        locale: String,
        #[arg(long = "cache", value_name = "PATH", action = ArgAction::Append)]
        cache: Vec<String>,
    },
    Install {
        locale: String,
        #[arg(long = "cache", value_name = "PATH", action = ArgAction::Append)]
        cache: Vec<String>,
    },
    AddWord {
        #[arg(long = "workspace", value_name = "PATH", action = ArgAction::Append)]
        workspace: Vec<String>,
        #[arg(long = "global", action = ArgAction::SetTrue)]
        global: bool,
        #[arg(value_name = "WORD")]
        word: String,
    },
}

pub(crate) fn parse_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Command, CliError> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    let Some(_) = arguments.first() else {
        return Err(CliError::Usage("missing command".to_owned()));
    };

    match arguments.get(1).map(String::as_str) {
        Some("--help" | "-h") => return Ok(Command::Help(USAGE)),
        Some("--version" | "-V") if arguments.len() == 2 => return Ok(Command::Version),
        Some("--version" | "-V") => {
            return Err(CliError::Usage(
                "`--version` does not accept arguments".to_owned(),
            ));
        }
        None => return Err(CliError::Usage("missing command".to_owned())),
        _ => {}
    }

    if requests_help(&arguments) {
        return Ok(command_help(arguments.get(1).map(String::as_str)));
    }

    let parsed = RawCli::try_parse_from(&arguments)
        .map_err(|error| CliError::Usage(error.to_string().trim().to_owned()))?;
    if parsed.help {
        return Ok(Command::Help(USAGE));
    }
    if parsed.version {
        return Ok(Command::Version);
    }

    parsed
        .command
        .ok_or_else(|| CliError::Usage("missing command".to_owned()))
        .and_then(|command| convert_command(command, &arguments))
}

fn command_help(command: Option<&str>) -> Command {
    Command::Help(match command {
        Some("check") => HELP_CHECK,
        Some("suggest") => HELP_SUGGEST,
        Some("explain") => HELP_EXPLAIN,
        Some("analyze") => HELP_ANALYZE,
        Some("compile") => HELP_COMPILE,
        Some("inspect") => HELP_INSPECT,
        Some("validate") => HELP_VALIDATE,
        Some("dictionary") => HELP_DICTIONARY,
        _ => USAGE,
    })
}

fn requests_help(arguments: &[String]) -> bool {
    let mut arguments = arguments.iter().skip(2);
    while let Some(argument) = arguments.next() {
        if argument == "--" {
            break;
        }
        if takes_value(argument) {
            if !argument.contains('=') {
                let _ = arguments.next();
            }
            continue;
        }
        if matches!(argument.as_str(), "--help" | "-h") {
            return true;
        }
    }
    false
}

fn takes_value(argument: &str) -> bool {
    let option = argument
        .split_once('=')
        .map_or(argument, |(option, _)| option);
    matches!(
        option,
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

fn convert_command(command: RawCommand, arguments: &[String]) -> Result<Command, CliError> {
    match command {
        RawCommand::Check(command) => convert_check(&command, arguments),
        RawCommand::Suggest(command) => Ok(Command::Suggest(SuggestCommand {
            dictionary_paths: paths(&command.sources.dictionary, "--dictionary")?,
            compiled_paths: paths(&command.sources.compiled, "--compiled")?,
            hunspell_affix_paths: paths(&command.sources.hunspell, "--hunspell")?,
            max_results: one_usize(&command.max_results, "--max-results", true)?,
            max_edit_distance: one_usize(&command.max_edit_distance, "--max-edit-distance", false)?,
            max_candidates: one_usize(&command.max_candidates, "--max-candidates", true)?,
            max_edit_cells: one_usize(&command.max_edit_cells, "--max-edit-cells", true)?,
            output_format: output_format(&command.sources.format)?,
            word: command.word,
        })),
        RawCommand::Explain(command) => Ok(Command::Explain(ExplainCommand {
            hunspell_affix_path: one_path(&command.hunspell, "--hunspell")?.ok_or_else(|| {
                CliError::Usage("explain requires exactly one `--hunspell` path".to_owned())
            })?,
            word: command.word,
        })),
        RawCommand::Analyze(command) => convert_analyze(command),
        RawCommand::Compile(command) => convert_compile(&command),
        RawCommand::Inspect(command) => Ok(Command::Inspect(PathBuf::from(command.path))),
        RawCommand::Validate(command) => convert_validate(&command),
        RawCommand::Dictionary(command) => convert_dictionary(command),
    }
}

fn convert_check(command: &RawCheck, arguments: &[String]) -> Result<Command, CliError> {
    let target = check_target(arguments)?;
    Ok(Command::Check(CheckCommand {
        dictionary_paths: paths(&command.sources.dictionary, "--dictionary")?,
        compiled_paths: paths(&command.sources.compiled, "--compiled")?,
        hunspell_affix_paths: paths(&command.sources.hunspell, "--hunspell")?,
        output_format: output_format(&command.sources.format)?,
        target,
    }))
}

fn check_target(arguments: &[String]) -> Result<CheckTarget, CliError> {
    let mut target = None;
    let mut options_ended = false;
    let mut arguments = arguments.iter().skip(2);
    while let Some(argument) = arguments.next() {
        if options_ended {
            push_check_positional(&mut target, argument.to_owned())?;
            continue;
        }
        if argument == "--" {
            options_ended = true;
            continue;
        }
        if takes_value(argument) {
            let option = argument
                .split_once('=')
                .map_or(argument.as_str(), |(option, _)| option);
            if option == "--file" {
                let value = argument.split_once('=').map_or_else(
                    || arguments.next().map(ToOwned::to_owned),
                    |(_, value)| Some(value.to_owned()),
                );
                let value = value
                    .ok_or_else(|| CliError::Usage("`--file` requires a path or `-`".to_owned()))?;
                push_check_input(&mut target, check_input(value)?)?;
            } else if !argument.contains('=') {
                let _ = arguments.next();
            }
            continue;
        }
        if argument.starts_with('-') {
            continue;
        }
        push_check_positional(&mut target, argument.to_owned())?;
    }
    target.ok_or_else(|| CliError::Usage("check requires a word or `--file`".to_owned()))
}

fn check_input(path: String) -> Result<CheckInput, CliError> {
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
        Some(CheckTarget::Inputs(inputs)) => inputs.push(CheckInput::File(PathBuf::from(value))),
        Some(CheckTarget::Word(_)) => {
            return Err(CliError::Usage(
                "check accepts one word, or one or more file inputs".to_owned(),
            ));
        }
    }
    Ok(())
}

fn convert_analyze(command: RawAnalyze) -> Result<Command, CliError> {
    let path = match command.paths.as_slice() {
        [path] => PathBuf::from(path),
        [] => return Err(CliError::Usage("analyze requires a path".to_owned())),
        _ => {
            return Err(CliError::Usage(
                "analyze accepts exactly one path".to_owned(),
            ));
        }
    };
    let comment_syntax = match (
        command.comment_prefix.as_slice(),
        command.comment_syntax.as_slice(),
    ) {
        ([], []) => None,
        ([prefix], []) if !prefix.is_empty() => Some(CommentSyntax::line(prefix.clone())),
        ([], [syntax]) if syntax == "html" => Some(CommentSyntax::Html),
        ([], [_]) => {
            return Err(CliError::Usage(
                "`--comment-syntax` supports only `html`".to_owned(),
            ));
        }
        _ => {
            return Err(CliError::Usage(
                "only one comment syntax may be supplied".to_owned(),
            ));
        }
    };
    if command.comment_prefix.iter().any(String::is_empty) {
        return Err(CliError::Usage(
            "`--comment-prefix` requires a non-empty prefix".to_owned(),
        ));
    }
    Ok(Command::Analyze(AnalyzeCommand {
        dictionary_paths: paths(&command.sources.dictionary, "--dictionary")?,
        compiled_paths: paths(&command.sources.compiled, "--compiled")?,
        hunspell_affix_paths: paths(&command.sources.hunspell, "--hunspell")?,
        config_path: one_path(&command.config, "--config")?,
        comment_syntax,
        include_patterns: non_empty_strings(command.include, "--include")?,
        exclude_patterns: non_empty_strings(command.exclude, "--exclude")?,
        suggest: command.suggest,
        output_format: output_format(&command.sources.format)?,
        path,
    }))
}

fn convert_compile(command: &RawCompile) -> Result<Command, CliError> {
    let output_path = one_path(&command.output, "-o")?
        .ok_or_else(|| CliError::Usage("compile requires an `-o` artifact path".to_owned()))?;
    let dictionary = one_path(&command.dictionary, "--dictionary")?;
    let input = match (dictionary, command.paths.as_slice()) {
        (Some(path), []) => CompileInput::WordList(path),
        (None, [aff_path, dic_path]) => CompileInput::Hunspell {
            aff_path: PathBuf::from(aff_path),
            dic_path: PathBuf::from(dic_path),
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

fn convert_validate(command: &RawValidate) -> Result<Command, CliError> {
    let output_format = output_format(&command.format)?;
    let compiled = one_path(&command.compiled, "--compiled")?;
    if let Some(path) = compiled {
        if command.strict || !command.paths.is_empty() {
            return Err(CliError::Usage(
                "`validate --compiled` accepts only one compiled artifact path".to_owned(),
            ));
        }
        return Ok(Command::Validate(ValidateCommand::Compiled {
            path,
            output_format,
        }));
    }
    if command.paths.len() != 2 {
        return Err(CliError::Usage(
            "validate requires exactly an AFF path and a DIC path".to_owned(),
        ));
    }
    Ok(Command::Validate(ValidateCommand::Hunspell {
        strict: command.strict,
        aff_path: PathBuf::from(&command.paths[0]),
        dic_path: PathBuf::from(&command.paths[1]),
        output_format,
    }))
}

fn convert_dictionary(command: RawDictionary) -> Result<Command, CliError> {
    let command = match command {
        RawDictionary::List => DictionaryCommand::List,
        RawDictionary::Fetch { locale, cache } => DictionaryCommand::Fetch {
            locale,
            cache_path: one_path(&cache, "--cache")?
                .ok_or_else(|| CliError::Usage("dictionary fetch requires `--cache`".to_owned()))?,
        },
        RawDictionary::Install { locale, cache } => DictionaryCommand::Install {
            locale,
            cache_path: one_path(&cache, "--cache")?.ok_or_else(|| {
                CliError::Usage("dictionary install requires `--cache`".to_owned())
            })?,
        },
        RawDictionary::AddWord {
            workspace,
            global,
            word,
        } => {
            if global && !workspace.is_empty() {
                return Err(CliError::Usage(
                    "choose either `--workspace` or `--global`".to_owned(),
                ));
            }
            let path = if global {
                super::global_user_dictionary_path()?
            } else {
                one_path(&workspace, "--workspace")?
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".ferrolex/words.txt")
            };
            DictionaryCommand::AddWord { word, path }
        }
    };
    Ok(Command::Dictionary(command))
}

fn paths(values: &[String], option: &str) -> Result<Vec<PathBuf>, CliError> {
    values
        .iter()
        .map(|value| path_value(value, option))
        .collect()
}

fn one_path(values: &[String], option: &str) -> Result<Option<PathBuf>, CliError> {
    match values {
        [] => Ok(None),
        [value] => path_value(value, option).map(Some),
        _ => Err(CliError::Usage(format!(
            "`{option}` may only be supplied once"
        ))),
    }
}

fn path_value(value: &str, option: &str) -> Result<PathBuf, CliError> {
    if value.is_empty() || value.starts_with('-') {
        return Err(CliError::Usage(format!("`{option}` requires a path")));
    }
    Ok(PathBuf::from(value))
}

fn non_empty_strings(values: Vec<String>, option: &str) -> Result<Vec<String>, CliError> {
    if values.iter().any(String::is_empty) {
        return Err(CliError::Usage(format!("`{option}` requires a value")));
    }
    Ok(values)
}

fn output_format(values: &[String]) -> Result<OutputFormat, CliError> {
    match values {
        [] => Ok(OutputFormat::Text),
        [value] => match value.as_str() {
            "text" => Ok(OutputFormat::Text),
            "json" => Ok(OutputFormat::Json),
            _ => Err(CliError::Usage(
                "`--format` supports only `text` or `json`".to_owned(),
            )),
        },
        _ => Err(CliError::Usage(
            "`--format` may only be supplied once".to_owned(),
        )),
    }
}

fn one_usize(
    values: &[usize],
    option: &str,
    must_be_positive: bool,
) -> Result<Option<usize>, CliError> {
    match values {
        [] => Ok(None),
        [value] if must_be_positive && *value == 0 => Err(CliError::Usage(format!(
            "`{option}` requires a positive integer"
        ))),
        [value] => Ok(Some(*value)),
        _ => Err(CliError::Usage(format!(
            "`{option}` may only be supplied once"
        ))),
    }
}

#[derive(Debug)]
pub(crate) enum CliError {
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
    InvalidDictionary(ferrolex_core::WordListError),
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
    pub(crate) const fn is_usage(&self) -> bool {
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
            Self::InvalidDictionary(source) => {
                write!(formatter, "invalid dictionary: {source}")
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
            Self::InvalidDictionary(source) | Self::InvalidUserWord(source) => Some(source),
        }
    }
}

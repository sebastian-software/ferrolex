//! CLI command types, output state, and user-facing errors.

use super::*;

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

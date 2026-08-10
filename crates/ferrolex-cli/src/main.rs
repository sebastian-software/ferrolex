//! Command-line interface for ferrolex.

#![forbid(unsafe_code)]

use std::env;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use ferrolex_core::{Checker, Dictionary, Normalization, WordList};
use ferrolex_text::check_text;

const USAGE: &str = "Usage: ferrolex check --dictionary <PATH> [--dictionary <PATH> ...] <WORD>\n       ferrolex check --dictionary <PATH> [--dictionary <PATH> ...] --file <PATH>";

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
    }
}

fn check(command: &CheckCommand) -> Result<RunOutcome, CliError> {
    let checker = load_checker(&command.dictionary_paths)?;

    match &command.target {
        CheckTarget::Word(word) => Ok(check_word(&checker, word)),
        CheckTarget::File(path) => check_file(&checker, path),
    }
}

fn load_checker(dictionary_paths: &[PathBuf]) -> Result<Checker, CliError> {
    let dictionaries = dictionary_paths
        .iter()
        .map(|path| {
            fs::read_to_string(path)
                .map(|text| WordList::from_text(Normalization::Exact, &text))
                .map_err(|source| CliError::ReadDictionary {
                    path: path.clone(),
                    source,
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let checker = dictionaries
        .into_iter()
        .fold(Checker::builder(), |builder, dictionary| {
            builder.dictionary(dictionary)
        })
        .build();

    Ok(checker)
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

fn check_file(checker: &Checker, path: &PathBuf) -> Result<RunOutcome, CliError> {
    let text = fs::read_to_string(path).map_err(|source| CliError::ReadInput {
        path: path.clone(),
        source,
    })?;
    let mut misspelled = false;

    for issue in check_text(checker, &text) {
        let (line, column) = line_and_column(&text, issue.range().start);
        println!(
            "{}:{line}:{column}: misspelled: {}",
            path.display(),
            issue.word()
        );
        misspelled = true;
    }

    Ok(if misspelled {
        RunOutcome::Misspelled
    } else {
        RunOutcome::Success
    })
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
        Some(command) => Err(CliError::Usage(format!("unknown command `{command}`"))),
        None => Err(CliError::Usage("missing command".to_owned())),
    }
}

fn parse_check_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Command, CliError> {
    let mut dictionary_paths = Vec::new();
    let mut target = None;
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--dictionary" => {
                let path = arguments
                    .next()
                    .ok_or_else(|| CliError::Usage("`--dictionary` requires a path".to_owned()))?;
                if path.starts_with('-') {
                    return Err(CliError::Usage("`--dictionary` requires a path".to_owned()));
                }
                dictionary_paths.push(PathBuf::from(path));
            }
            "--file" => {
                let path = arguments
                    .next()
                    .ok_or_else(|| CliError::Usage("`--file` requires a path".to_owned()))?;
                if path.starts_with('-') {
                    return Err(CliError::Usage("`--file` requires a path".to_owned()));
                }
                set_target(&mut target, CheckTarget::File(PathBuf::from(path)))?;
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
    if dictionary_paths.is_empty() {
        return Err(CliError::Usage(
            "check requires at least one `--dictionary` path".to_owned(),
        ));
    }

    Ok(Command::Check(CheckCommand {
        dictionary_paths,
        target,
    }))
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
    Help,
}

#[derive(Debug, Eq, PartialEq)]
struct CheckCommand {
    dictionary_paths: Vec<PathBuf>,
    target: CheckTarget,
}

#[derive(Debug, Eq, PartialEq)]
enum CheckTarget {
    Word(String),
    File(PathBuf),
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    ReadDictionary { path: PathBuf, source: io::Error },
    ReadInput { path: PathBuf, source: io::Error },
}

impl fmt::Display for CliError {
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
        }
    }
}

impl Error for CliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Usage(_) => None,
            Self::ReadDictionary { source, .. } | Self::ReadInput { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{
        line_and_column, parse_arguments, run, CheckCommand, CheckTarget, CliError, Command,
        RunOutcome,
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

    impl Drop for TemporaryDictionary {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    fn temporary_dictionary(contents: &str) -> TemporaryDictionary {
        temporary_file(contents)
    }

    fn temporary_file(contents: &str) -> TemporaryDictionary {
        let sequence = NEXT_TEMPORARY_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrolex-cli-test-{}-{sequence}.txt",
            std::process::id()
        ));
        fs::write(&path, contents).expect("the temporary directory is writable");
        TemporaryDictionary { path }
    }
}

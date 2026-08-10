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

const USAGE: &str = "Usage: ferrolex check --dictionary <PATH> [--dictionary <PATH> ...] <WORD>";

fn main() -> ExitCode {
    match run(env::args()) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("error: {error}");
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run(arguments: impl IntoIterator<Item = String>) -> Result<ExitCode, CliError> {
    match parse_arguments(arguments)? {
        Command::Help => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Command::Check(command) => check(&command),
    }
}

fn check(command: &CheckCommand) -> Result<ExitCode, CliError> {
    let dictionaries = command
        .dictionary_paths
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

    if checker.contains(&command.word) {
        println!("accepted: {}", command.word);
        Ok(ExitCode::SUCCESS)
    } else {
        println!("misspelled: {}", command.word);
        Ok(ExitCode::from(1))
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
    let mut word = None;
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
            "--help" | "-h" => return Ok(Command::Help),
            option if option.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{option}`")));
            }
            _ if word.is_some() => {
                return Err(CliError::Usage("check accepts exactly one word".to_owned()));
            }
            _ => word = Some(argument),
        }
    }

    let word = word.ok_or_else(|| CliError::Usage("check requires a word".to_owned()))?;
    if dictionary_paths.is_empty() {
        return Err(CliError::Usage(
            "check requires at least one `--dictionary` path".to_owned(),
        ));
    }

    Ok(Command::Check(CheckCommand {
        dictionary_paths,
        word,
    }))
}

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Check(CheckCommand),
    Help,
}

#[derive(Debug, Eq, PartialEq)]
struct CheckCommand {
    dictionary_paths: Vec<PathBuf>,
    word: String,
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    ReadDictionary { path: PathBuf, source: io::Error },
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
        }
    }
}

impl Error for CliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Usage(_) => None,
            Self::ReadDictionary { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::ExitCode;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{parse_arguments, run, CheckCommand, CliError, Command};

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
                word: "OAuth".to_owned(),
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
            ExitCode::SUCCESS
        );
        assert_eq!(
            run(arguments("Strasse")).expect("dictionary is readable"),
            ExitCode::from(1)
        );
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
        let sequence = NEXT_TEMPORARY_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrolex-cli-test-{}-{sequence}.txt",
            std::process::id()
        ));
        fs::write(&path, contents).expect("the temporary directory is writable");
        TemporaryDictionary { path }
    }
}

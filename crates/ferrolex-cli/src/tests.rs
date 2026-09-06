//! CLI integration and parser tests.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, SystemTime};

use ferrolex_compiler::{CompiledDictionary, MAX_COMPILED_ARTIFACT_BYTES, ValidationError};
use ferrolex_core::{Dictionary, WordListError};
use ferrolex_dictionaries::SourceEncoding;
use ferrolex_hunspell::{
    CacheSource, ImportMode, RuntimeCacheError, SourceDigests, import, load_runtime_cache,
};

use super::{
    AnalysisDictionary, AnalysisSource, AnalysisSuggestionEngine, AnalyzeCommand, Analyzer,
    CandidateSource, CheckCommand, CheckInput, CheckTarget, CliError, Command, CommentSyntax,
    CompileCommand, CompileInput, DictionaryCommand, Document, ExplainCommand, HELP_CHECK,
    LineIndex, Normalization, OutputFormat, RunOutcome, STALE_TEMPORARY_FILE_AGE, SuggestCommand,
    SuggestConfig, UserDictionaryLock, ValidateCommand, WordList, add_user_dictionary_word,
    analysis_paths, analyze, comment_syntax_for_path, glob_matches, hidden_sibling,
    incomplete_suggestion_hint, install_hunspell_runtime_cache, load_analysis_dictionary,
    parse_arguments, read_analysis_source, read_compiled_artifact, render_explanation, run,
    runtime_cache_path, validate_hunspell,
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
            parse_arguments(["ferrolex", flag].map(str::to_owned)).expect("version flag is valid"),
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
    assert!(
        !CliError::ReadInput {
            path: PathBuf::from("missing.txt"),
            source: io::Error::new(io::ErrorKind::NotFound, "missing"),
        }
        .is_usage()
    );
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
    let command = parse_arguments(["ferrolex", "inspect", "dictionary.flexh"].map(str::to_owned))
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
    assert!(incomplete_suggestion_hint(super::Completeness::RelatedSeedTooLong, config).is_none());
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
        assert!(parse_arguments(arguments.iter().map(|argument| (*argument).to_owned())).is_err());
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

    let result = super::Suggester::new(&dictionary, SuggestConfig::default()).suggest("recieve");

    assert!(
        result
            .suggestions()
            .iter()
            .any(|suggestion| suggestion.word() == "receive")
    );
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
    let error = parse_arguments(["ferrolex", "dictionary", "fetch", "de_DE"].map(str::to_owned))
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
    let error =
        parse_arguments(["ferrolex", "compile", "--dictionary", "words.txt"].map(str::to_owned))
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
    let compiled =
        CompiledDictionary::load(fs::read(&output.path).expect("the compiler wrote the artifact"))
            .expect("the compiler wrote a valid fast-load header");
    assert!(compiled.contains("Straße"));
    assert!(compiled.contains("東京"));
    compiled.validate().expect("the artifact is fully valid");
}

#[test]
fn compile_and_check_share_frequency_word_list_semantics() {
    let dictionary = temporary_dictionary("# comment\t5\ncat\t1\ncut\t9\n");
    assert_eq!(
        run([
            "ferrolex".to_owned(),
            "check".to_owned(),
            "--dictionary".to_owned(),
            dictionary.path.to_string_lossy().into_owned(),
            "cat".to_owned(),
        ])
        .expect("frequency word lists are recognized by check"),
        RunOutcome::Success
    );

    let output = temporary_file("");
    run([
        "ferrolex".to_owned(),
        "compile".to_owned(),
        "--dictionary".to_owned(),
        dictionary.path.to_string_lossy().into_owned(),
        "-o".to_owned(),
        output.path.to_string_lossy().into_owned(),
    ])
    .expect("frequency word list compiles");
    let compiled =
        CompiledDictionary::load(fs::read(&output.path).expect("the compiler wrote the artifact"))
            .expect("the artifact loads");
    assert!(compiled.contains("cat"));
    assert!(!compiled.contains("cat\t1"));
}

#[test]
fn tabs_in_comments_and_trailing_tabs_remain_plain_word_list_syntax() {
    let dictionary = temporary_dictionary("# comment\t5\ncat\t\n");
    let output = temporary_file("");
    run([
        "ferrolex".to_owned(),
        "compile".to_owned(),
        "--dictionary".to_owned(),
        dictionary.path.to_string_lossy().into_owned(),
        "-o".to_owned(),
        output.path.to_string_lossy().into_owned(),
    ])
    .expect("plain word list with tabs compiles");
    let compiled =
        CompiledDictionary::load(fs::read(&output.path).expect("the compiler wrote the artifact"))
            .expect("the artifact loads");
    assert!(compiled.contains("cat"));
    assert_eq!(
        run([
            "ferrolex".to_owned(),
            "check".to_owned(),
            "--dictionary".to_owned(),
            dictionary.path.to_string_lossy().into_owned(),
            "cat".to_owned(),
        ])
        .expect("plain word list remains usable by check"),
        RunOutcome::Success
    );
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
    let command =
        parse_arguments(["ferrolex", "validate", "--compiled", "words.flex"].map(str::to_owned))
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
            ferrolex::catalog_import_encodings(SourceEncoding::MixedUtf8AndIso8859_1),
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
    let dictionary_bytes = fs::read(&dictionary.path).expect("dictionary source remains available");
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
    let sources = temporary_hunspell_sources("SET UTF-8\nSFX S N 1\nSFX S 0 s .\n", "1\nword/S\n");
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

    fs::write(&sources.dictionary_path, "1\nother/S\n").expect("fixture dictionary is writable");
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
fn checks_single_words_with_the_same_nfc_fallback_as_files() {
    let dictionary = temporary_dictionary("café\n");
    let arguments = [
        "ferrolex".to_owned(),
        "check".to_owned(),
        "--dictionary".to_owned(),
        dictionary.path.to_string_lossy().into_owned(),
        "cafe\u{301}".to_owned(),
    ];

    assert_eq!(
        run(arguments).expect("dictionary and word are readable"),
        RunOutcome::Success
    );
}

#[test]
fn add_word_rejects_entries_that_would_be_lost_on_reload() {
    let dictionary = temporary_dictionary("normal\n");

    let error = add_user_dictionary_word("#tag", &dictionary.path)
        .expect_err("comment-looking words cannot be persisted");

    assert!(matches!(
        error,
        CliError::InvalidUserWord(WordListError::InvalidEntry { position: 1 })
    ));
    assert_eq!(
        fs::read_to_string(&dictionary.path).expect("dictionary remains readable"),
        "normal\n"
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

use std::fs;
use std::io::Write as _;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEMPORARY_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

fn temporary_directory(label: &str) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "ferrolex-cli-{label}-{}-{}",
        std::process::id(),
        NEXT_TEMPORARY_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&directory).expect("temporary directory is created");
    directory
}

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ferrolex"))
        .args(arguments)
        .output()
        .expect("CLI binary runs")
}

fn run_in(directory: &std::path::Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ferrolex"))
        .current_dir(directory)
        .args(arguments)
        .output()
        .expect("CLI binary runs")
}

fn run_with_config_home(
    directory: &std::path::Path,
    config_home: &std::path::Path,
    arguments: &[&str],
) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ferrolex"))
        .current_dir(directory)
        .env("XDG_CONFIG_HOME", config_home)
        .args(arguments)
        .output()
        .expect("CLI binary runs")
}

fn run_with_stdin(directory: &std::path::Path, arguments: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ferrolex"))
        .current_dir(directory)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("CLI binary starts");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(input.as_bytes())
        .expect("stdin fixture is written");
    child.wait_with_output().expect("CLI binary exits")
}

#[test]
fn every_command_has_a_focused_help_screen() {
    for command in [
        "check",
        "suggest",
        "explain",
        "analyze",
        "compile",
        "inspect",
        "validate",
        "dictionary",
    ] {
        let output = run(&[command, "--help"]);
        assert!(output.status.success(), "{command} help succeeds");
        let stdout = String::from_utf8(output.stdout).expect("help is UTF-8");
        assert!(stdout.starts_with(&format!("Usage: ferrolex {command}")));
        assert!(stdout.contains("Exit status:"));
    }
}

#[test]
fn version_and_error_contract_cross_the_process_boundary() {
    let version = run(&["--version"]);
    assert!(version.status.success());
    assert!(String::from_utf8(version.stdout)
        .expect("version is UTF-8")
        .starts_with("ferrolex "));

    let usage = run(&["check", "word"]);
    assert_eq!(usage.status.code(), Some(2));
    assert!(String::from_utf8(usage.stderr)
        .expect("usage diagnostic is UTF-8")
        .contains("Usage: ferrolex"));

    let runtime = run(&["check", "--dictionary", "does-not-exist.txt", "word"]);
    assert_eq!(runtime.status.code(), Some(3));
    let stderr = String::from_utf8(runtime.stderr).expect("runtime diagnostic is UTF-8");
    assert!(stderr.contains("could not read dictionary"));
    assert!(!stderr.contains("Usage: ferrolex"));
}

#[test]
fn check_accepts_stdin_and_reports_its_source() {
    let directory = temporary_directory("check-stdin");
    fs::write(directory.join("words.txt"), "correct\n").expect("dictionary fixture is written");

    let output = run_with_stdin(
        &directory,
        &["check", "--dictionary", "words.txt", "--file", "-"],
        "correct mispelt\n",
    );

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "-:1:9: misspelled: mispelt\n"
    );
    assert!(output.stderr.is_empty());

    fs::remove_dir_all(directory).expect("temporary fixture is removed");
}

#[test]
fn check_reuses_one_dictionary_for_multiple_files() {
    let directory = temporary_directory("check-multiple-files");
    fs::write(directory.join("words.txt"), "correct\n").expect("dictionary fixture is written");
    fs::write(directory.join("first.txt"), "correct\n").expect("first input is written");
    fs::write(directory.join("second.txt"), "mispelt\n").expect("second input is written");
    fs::write(directory.join("third.txt"), "correct\n").expect("third input is written");

    let output = run_in(
        &directory,
        &[
            "check",
            "--dictionary",
            "words.txt",
            "--file",
            "first.txt",
            "--file",
            "second.txt",
            "third.txt",
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "second.txt:1:1: misspelled: mispelt\n"
    );
    assert!(output.stderr.is_empty());

    fs::remove_dir_all(directory).expect("temporary fixture is removed");
}

#[test]
fn check_accepts_a_hyphen_leading_word_after_the_option_separator() {
    let directory = temporary_directory("check-option-separator");
    fs::write(directory.join("words.txt"), "-ish\n").expect("dictionary fixture is written");

    let output = run_in(
        &directory,
        &["check", "--dictionary", "words.txt", "--", "-ish"],
    );

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "accepted: -ish\n"
    );
    assert!(output.stderr.is_empty());

    fs::remove_dir_all(directory).expect("temporary fixture is removed");
}

#[test]
fn workspace_user_words_round_trip_across_cli_processes() {
    let directory = temporary_directory("workspace-user-dictionary");
    fs::write(directory.join("source.txt"), "projectterm\n").expect("source fixture is written");

    let added = run_in(&directory, &["dictionary", "add-word", "projectterm"]);
    assert!(added.status.success());
    let added_stdout = String::from_utf8(added.stdout).expect("stdout is UTF-8");
    let added_path = added_stdout
        .strip_prefix("added: ")
        .expect("the add command reports its path")
        .trim();
    assert_eq!(
        std::path::Path::new(added_path).file_name(),
        Some(std::ffi::OsStr::new("words.txt"))
    );
    assert!(added_path.contains(".ferrolex"));

    let checked = run_in(&directory, &["check", "projectterm"]);
    assert!(checked.status.success());
    assert_eq!(
        String::from_utf8(checked.stdout).expect("stdout is UTF-8"),
        "accepted: projectterm\n"
    );

    let suggested = run_in(&directory, &["suggest", "projecttrm"]);
    assert!(suggested.status.success());
    assert!(String::from_utf8(suggested.stdout)
        .expect("stdout is UTF-8")
        .contains("suggestion: projectterm"));

    let analyzed = run_in(&directory, &["analyze", "source.txt"]);
    assert!(analyzed.status.success());
    assert!(analyzed.stdout.is_empty());

    fs::remove_dir_all(directory).expect("temporary fixture is removed");
}

#[test]
fn global_user_words_are_loaded_outside_the_creating_workspace() {
    let config_home = temporary_directory("global-user-config");
    let first_workspace = temporary_directory("global-user-first-workspace");
    let second_workspace = temporary_directory("global-user-second-workspace");

    let added = run_with_config_home(
        &first_workspace,
        &config_home,
        &["dictionary", "add-word", "--global", "globalterm"],
    );
    assert!(added.status.success());

    let checked = run_with_config_home(&second_workspace, &config_home, &["check", "globalterm"]);
    assert!(checked.status.success());
    assert_eq!(
        String::from_utf8(checked.stdout).expect("stdout is UTF-8"),
        "accepted: globalterm\n"
    );

    fs::remove_dir_all(config_home).expect("temporary fixture is removed");
    fs::remove_dir_all(first_workspace).expect("temporary fixture is removed");
    fs::remove_dir_all(second_workspace).expect("temporary fixture is removed");
}

#[test]
fn hunspell_sources_without_a_cache_are_imported_with_actionable_guidance() {
    let directory = temporary_directory("hunspell");
    let aff_path = directory.join("en.aff");
    let dic_path = directory.join("en.dic");
    let cache_path = directory.join("en.ferrolex-hunspell-v1.flexh");
    fs::write(&aff_path, "SET UTF-8\nSFX S N 1\nSFX S 0 s .\n").expect("affix fixture is written");
    fs::write(&dic_path, "1\nword/S\n").expect("dictionary fixture is written");

    let output = run(&[
        "check",
        "--hunspell",
        aff_path.to_str().expect("temporary path is UTF-8"),
        "words",
    ]);

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "accepted: words\n"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.contains("no Hunspell runtime cache found"));
    assert!(stderr.contains("importing"));
    assert!(stderr.contains("directly (slower)"));
    assert!(stderr.contains("ferrolex compile"));
    assert!(stderr.contains("--compiled"));
    assert!(
        !cache_path.exists(),
        "direct import does not need to write beside read-only sources"
    );

    fs::remove_dir_all(directory).expect("temporary fixture is removed");
}

#[test]
fn a_catalog_shaped_filename_does_not_override_local_source_encoding() {
    let directory = temporary_directory("catalog-name-collision");
    let aff_path = directory.join("id_ID.aff");
    let dic_path = directory.join("id_ID.dic");
    fs::write(&aff_path, "SET UTF-8\n").expect("affix fixture is written");
    fs::write(&dic_path, "1\ncafé\n").expect("UTF-8 dictionary fixture is written");

    let output = run(&[
        "check",
        "--hunspell",
        aff_path.to_str().expect("temporary path is UTF-8"),
        "café",
    ]);

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "accepted: café\n"
    );
    assert!(String::from_utf8(output.stderr)
        .expect("stderr is UTF-8")
        .contains("directly (slower)"));

    fs::remove_dir_all(directory).expect("temporary fixture is removed");
}

#[test]
fn cacheless_importer_errors_fail_closed_with_diagnostics() {
    let directory = temporary_directory("strict-import");
    let aff_path = directory.join("invalid.aff");
    let dic_path = directory.join("invalid.dic");
    fs::write(&aff_path, "SET UTF-8\nICONV 1\nICONV only-source\n")
        .expect("affix fixture is written");
    fs::write(&dic_path, "1\nword\n").expect("dictionary fixture is written");

    let output = run(&[
        "check",
        "--hunspell",
        aff_path.to_str().expect("temporary path is UTF-8"),
        "word",
    ]);

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty(), "no partial dictionary is queried");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.contains("error[ICONV]"));
    assert!(stderr.contains("could not strictly import Hunspell sources"));
    assert!(stderr.contains("ferrolex validate --strict"));

    fs::remove_dir_all(directory).expect("temporary fixture is removed");
}

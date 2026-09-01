use std::fs;
use std::process::{Command, Output};
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

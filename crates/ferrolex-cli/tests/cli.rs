use std::process::{Command, Output};

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

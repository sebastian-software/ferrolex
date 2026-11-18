//! Generic stdio Language Server Protocol integration for ferrolex.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use ferrolex_code::{Analyzer, Document, Finding};
use ferrolex_core::{Checker, Normalization, UserDictionary, WordList};
use ferrolex_suggest::{SuggestConfig, Suggester};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use serde::Deserialize;
use serde_json::{Value, json};

const SOURCE: &str = "ferrolex";
const UNKNOWN_WORD: &str = "ferrolex.unknown-word";

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    #[serde(default)]
    words: Vec<String>,
    #[serde(default)]
    ignored_words: Vec<String>,
    comment_prefix: Option<String>,
    user_dictionary_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
struct OpenDocument {
    version: i64,
    text: String,
}

struct State {
    base: Arc<WordList>,
    user: Arc<UserDictionary>,
    ignored_words: BTreeSet<String>,
    comment_prefix: String,
    user_dictionary_path: Option<PathBuf>,
    documents: BTreeMap<String, OpenDocument>,
    pending_words: BTreeSet<String>,
}

impl State {
    fn new(config: Config) -> Self {
        let mut state = Self {
            base: Arc::new(word_list(config.words)),
            user: Arc::new(UserDictionary::new(Normalization::Exact)),
            ignored_words: config.ignored_words.into_iter().collect(),
            comment_prefix: config.comment_prefix.unwrap_or_else(|| "//".to_owned()),
            user_dictionary_path: config.user_dictionary_path,
            documents: BTreeMap::new(),
            pending_words: BTreeSet::new(),
        };
        state.reload_user_dictionary();
        state
    }

    fn configure(&mut self, config: Config) {
        if !config.words.is_empty() {
            self.base = Arc::new(word_list(config.words));
        }
        self.ignored_words = config.ignored_words.into_iter().collect();
        if let Some(prefix) = config.comment_prefix {
            self.comment_prefix = prefix;
        }
        if config.user_dictionary_path.is_some() {
            self.pending_words.clear();
            self.user_dictionary_path = config.user_dictionary_path;
            self.user = Arc::new(UserDictionary::new(Normalization::Exact));
            self.reload_user_dictionary();
        }
    }

    fn reload_user_dictionary(&mut self) {
        let Some(path) = &self.user_dictionary_path else {
            return;
        };
        if let Ok(text) = fs::read_to_string(path) {
            self.user = Arc::new(UserDictionary::from_text(Normalization::Exact, &text));
        }
    }

    fn persist_user_dictionary(&mut self) -> Result<(), String> {
        let Some(path) = &self.user_dictionary_path else {
            self.pending_words.clear();
            return Ok(());
        };
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        // Lock the dictionary itself so hard links and symlinks share the lock,
        // and preserve its permissions and identity when adding words.
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(path)
            .map_err(|error| error.to_string())?;
        file.lock().map_err(|error| error.to_string())?;
        let mut text = String::new();
        file.read_to_string(&mut text)
            .map_err(|error| error.to_string())?;
        let original_len = text.len() as u64;
        let merged = UserDictionary::from_text(Normalization::Exact, &text);
        let mut additions = String::new();
        for word in &self.pending_words {
            if merged.insert(word).map_err(|error| error.to_string())? {
                if additions.is_empty() && !text.is_empty() && !text.ends_with('\n') {
                    additions.push('\n');
                }
                additions.push_str(word);
                additions.push('\n');
            }
        }
        if let Err(error) = file
            .write_all(additions.as_bytes())
            .and_then(|()| file.sync_all())
        {
            // Restore the original length after a partial append where possible.
            if let Err(rollback) = file.set_len(original_len) {
                return Err(format!(
                    "{error}; could not roll back partial write: {rollback}"
                ));
            }
            return Err(error.to_string());
        }
        self.user = Arc::new(merged);
        self.pending_words.clear();
        Ok(())
    }

    fn findings<'text>(&self, text: &'text str) -> Vec<Finding<'text>> {
        let checker = Checker::builder()
            .shared_dictionary(self.base.clone())
            .shared_dictionary(self.user.clone())
            .build();
        let mut builder = Analyzer::builder(&checker);
        for word in &self.ignored_words {
            builder = builder.ignore_word(word.as_str());
        }
        builder
            .build()
            .check(&Document::new(text))
            .findings()
            .to_vec()
    }

    fn suggestions(&self, word: &str) -> Vec<String> {
        Suggester::new(self.base.as_ref(), SuggestConfig::default())
            .suggest(word)
            .suggestions()
            .iter()
            .map(|suggestion| suggestion.word().to_owned())
            .collect()
    }

    fn open(&mut self, uri: String, version: i64, text: String) {
        self.documents.insert(uri, OpenDocument { version, text });
    }

    fn change(&mut self, uri: &str, version: i64, changes: Vec<TextChange>) -> Result<(), String> {
        let document = self
            .documents
            .get_mut(uri)
            .ok_or_else(|| "document is not open".to_owned())?;
        for change in changes {
            if let Some(range) = change.range {
                let start = position_to_byte(&document.text, range.start)?;
                let end = position_to_byte(&document.text, range.end)?;
                if start > end {
                    return Err("change range is reversed".to_owned());
                }
                document.text.replace_range(start..end, &change.text);
            } else {
                document.text = change.text;
            }
        }
        document.version = version;
        Ok(())
    }

    fn diagnostics(&self, uri: &str) -> Vec<Value> {
        let Some(document) = self.documents.get(uri) else {
            return Vec::new();
        };
        self.findings(&document.text)
            .into_iter()
            .map(|finding| diagnostic(&document.text, &finding))
            .collect()
    }

    fn code_actions(&self, uri: &str, range: LspRange) -> Vec<Value> {
        let Some(document) = self.documents.get(uri) else {
            return Vec::new();
        };
        let (Ok(start), Ok(end)) = (
            position_to_byte(&document.text, range.start),
            position_to_byte(&document.text, range.end),
        ) else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        for finding in self.findings(&document.text) {
            let finding_range = finding.range();
            if finding_range.end <= start || finding_range.start >= end {
                continue;
            }
            let diagnostic = diagnostic(&document.text, &finding);
            for suggestion in self.suggestions(finding.word()) {
                let (edit_range, replacement) = finding
                    .whole_identifier_suggestion(&suggestion)
                    .map_or_else(
                        || (finding.range(), suggestion),
                        |whole| (finding.token_range(), whole),
                    );
                actions.push(json!({
                    "title": format!("Replace with {replacement}"), "kind": "quickfix", "diagnostics": [diagnostic],
                    "edit": { "changes": { uri: [{ "range": byte_range_to_lsp(&document.text, edit_range), "newText": replacement }]}}
                }));
            }
            actions.push(json!({
                "title": format!("Add '{}' to user dictionary", finding.word()), "kind": "quickfix", "diagnostics": [diagnostic],
                "command": { "title": "Add to user dictionary", "command": "ferrolex.addToDictionary", "arguments": [{"word": finding.word()}] }
            }));
            let line_start = line_start_byte(&document.text, finding_range.start);
            actions.push(json!({
                "title": format!("Ignore '{}' in this document", finding.word()), "kind": "quickfix", "diagnostics": [diagnostic],
                "edit": { "changes": { uri: [{ "range": byte_range_to_lsp(&document.text, line_start..line_start), "newText": format!("{} ferrolex:ignore {}\n", self.comment_prefix, finding.word()) }]}}
            }));
        }
        actions
    }

    fn add_user_word(&mut self, word: &str) -> Result<(), String> {
        self.user.insert(word).map_err(|error| error.to_string())?;
        self.pending_words.insert(word.trim().to_owned());
        self.persist_user_dictionary()
    }
}

fn word_list(words: Vec<String>) -> WordList {
    let words = if words.is_empty() {
        vec!["ferrolex".to_owned()]
    } else {
        words
    };
    WordList::new(words).unwrap_or_else(|_| WordList::from_text(Normalization::Exact, "ferrolex"))
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct LspPosition {
    line: u32,
    character: u32,
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct LspRange {
    start: LspPosition,
    end: LspPosition,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextChange {
    range: Option<LspRange>,
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DidOpen {
    text_document: VersionedDocument,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionedDocument {
    uri: String,
    version: i64,
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DidChange {
    text_document: DocumentVersion,
    content_changes: Vec<TextChange>,
}

#[derive(Debug, Deserialize)]
struct DocumentVersion {
    uri: String,
    version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodeAction {
    text_document: DocumentId,
    range: LspRange,
}

#[derive(Debug, Deserialize)]
struct DocumentId {
    uri: String,
}

#[derive(Debug, Deserialize)]
struct ExecuteCommand {
    command: String,
    #[serde(default)]
    arguments: Vec<Value>,
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let (connection, io_threads) = Connection::stdio();
    let initialized = connection.initialize(json!({
        "capabilities": { "textDocumentSync": 2, "codeActionProvider": true,
            "executeCommandProvider": { "commands": ["ferrolex.addToDictionary"] } },
        "serverInfo": { "name": "ferrolex-lsp", "version": env!("CARGO_PKG_VERSION") }
    }))?;
    run(&connection, State::new(config_from(&initialized)))?;
    io_threads.join()?;
    Ok(())
}

fn run(connection: &Connection, mut state: State) -> Result<(), Box<dyn Error + Send + Sync>> {
    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    break;
                }
                handle_request(connection, &mut state, request)?;
            }
            Message::Notification(notification) => {
                handle_notification(connection, &mut state, notification)?;
            }
            Message::Response(_) => {}
        }
    }
    Ok(())
}

fn handle_notification(
    connection: &Connection,
    state: &mut State,
    notification: Notification,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    match notification.method.as_str() {
        "textDocument/didOpen" => {
            let open: DidOpen = serde_json::from_value(notification.params)?;
            let uri = open.text_document.uri;
            state.open(
                uri.clone(),
                open.text_document.version,
                open.text_document.text,
            );
            publish(connection, state, &uri)?;
        }
        "textDocument/didChange" => {
            let change: DidChange = serde_json::from_value(notification.params)?;
            let uri = change.text_document.uri;
            state
                .change(&uri, change.text_document.version, change.content_changes)
                .map_err(|error| format!("invalid change: {error}"))?;
            publish(connection, state, &uri)?;
        }
        "textDocument/didClose" => {
            let uri = notification.params["textDocument"]["uri"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            state.documents.remove(&uri);
            notify(
                connection,
                "textDocument/publishDiagnostics",
                json!({"uri": uri, "diagnostics": []}),
            )?;
        }
        "workspace/didChangeConfiguration" => {
            state.configure(config_from(&notification.params));
            publish_all(connection, state)?;
        }
        _ => {}
    }
    Ok(())
}

fn handle_request(
    connection: &Connection,
    state: &mut State,
    request: Request,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    match request.method.as_str() {
        "textDocument/codeAction" => {
            let params: CodeAction = serde_json::from_value(request.params)?;
            ok(
                connection,
                request.id,
                state.code_actions(&params.text_document.uri, params.range),
            )?;
        }
        "workspace/executeCommand" => {
            let params: ExecuteCommand = serde_json::from_value(request.params)?;
            if params.command != "ferrolex.addToDictionary" {
                error(
                    connection,
                    request.id,
                    ErrorCode::MethodNotFound,
                    "unknown ferrolex command",
                )?;
            } else if let Some(word) = params
                .arguments
                .first()
                .and_then(|value| value["word"].as_str())
            {
                match state.add_user_word(word) {
                    Ok(()) => {
                        publish_all(connection, state)?;
                        ok(connection, request.id, Value::Null)?;
                    }
                    Err(message) => {
                        error(connection, request.id, ErrorCode::InternalError, &message)?;
                    }
                }
            } else {
                error(
                    connection,
                    request.id,
                    ErrorCode::InvalidParams,
                    "missing command word",
                )?;
            }
        }
        _ => error(
            connection,
            request.id,
            ErrorCode::MethodNotFound,
            "unsupported request",
        )?,
    }
    Ok(())
}

fn publish_all(connection: &Connection, state: &State) -> Result<(), Box<dyn Error + Send + Sync>> {
    for uri in state.documents.keys() {
        publish(connection, state, uri)?;
    }
    Ok(())
}

fn publish(
    connection: &Connection,
    state: &State,
    uri: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    notify(
        connection,
        "textDocument/publishDiagnostics",
        json!({
            "uri": uri, "version": state.documents.get(uri).map(|document| document.version), "diagnostics": state.diagnostics(uri)
        }),
    )
}

fn notify(
    connection: &Connection,
    method: &str,
    params: Value,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    connection
        .sender
        .send(Notification::new(method.to_owned(), params).into())?;
    Ok(())
}

fn ok(
    connection: &Connection,
    id: RequestId,
    result: impl serde::Serialize,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    connection
        .sender
        .send(Response::new_ok(id, serde_json::to_value(result)?).into())?;
    Ok(())
}

fn error(
    connection: &Connection,
    id: RequestId,
    code: ErrorCode,
    message: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    connection
        .sender
        .send(Response::new_err(id, code as i32, message.to_owned()).into())?;
    Ok(())
}

fn config_from(value: &Value) -> Config {
    serde_json::from_value(value["initializationOptions"]["ferrolex"].clone())
        .or_else(|_| serde_json::from_value(value["ferrolex"].clone()))
        .or_else(|_| serde_json::from_value(value.clone()))
        .unwrap_or_default()
}

fn diagnostic(text: &str, finding: &Finding<'_>) -> Value {
    json!({"range": byte_range_to_lsp(text, finding.range()), "severity": 2, "code": UNKNOWN_WORD,
        "source": SOURCE, "message": format!("Unknown word: {}", finding.word()), "data": {"word": finding.word()}})
}

fn byte_range_to_lsp(text: &str, range: std::ops::Range<usize>) -> Value {
    json!({"start": byte_to_position(text, range.start), "end": byte_to_position(text, range.end)})
}

// Each range excludes its line ending; CRLF is one line break.
fn line_ranges(text: &str) -> impl Iterator<Item = std::ops::Range<usize>> + '_ {
    let mut start = 0;
    std::iter::from_fn(move || {
        if start > text.len() {
            return None;
        }
        let line_start = start;
        let ending = text.as_bytes()[start..]
            .iter()
            .position(|byte| matches!(byte, b'\r' | b'\n'));
        let end = ending.map_or(text.len(), |offset| start + offset);
        start = if end == text.len() {
            text.len() + 1
        } else if text.as_bytes()[end..].starts_with(b"\r\n") {
            end + 2
        } else {
            end + 1
        };
        Some(line_start..end)
    })
}

// The byte between CR and LF belongs to the preceding line, since LSP
// positions cannot address the interior of a line ending.
fn line_at_byte(text: &str, byte: usize) -> (usize, usize) {
    let bytes = text.as_bytes();
    let mut line = 0;
    let mut start = 0;
    let mut index = 0;
    while index < byte {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                if index + 1 == byte {
                    break;
                }
                index += 2;
                line += 1;
                start = index;
            }
            b'\r' | b'\n' => {
                index += 1;
                line += 1;
                start = index;
            }
            _ => index += 1,
        }
    }
    (line, start)
}

fn byte_to_position(text: &str, byte: usize) -> Value {
    let (line, start) = line_at_byte(text, byte);
    let end = if byte > start
        && text.as_bytes()[byte - 1] == b'\r'
        && text.as_bytes().get(byte) == Some(&b'\n')
    {
        byte - 1
    } else {
        byte
    };
    json!({"line": line, "character": text[start..end].encode_utf16().count()})
}

fn position_to_byte(text: &str, position: LspPosition) -> Result<usize, String> {
    let range = line_ranges(text)
        .nth(position.line as usize)
        .ok_or_else(|| "position line is outside the document".to_owned())?;
    let line = &text[range.clone()];
    let mut units = 0u32;
    for (offset, character) in line.char_indices() {
        if units == position.character {
            return Ok(range.start + offset);
        }
        units += match character.len_utf16() {
            1 => 1,
            2 => 2,
            _ => unreachable!(),
        };
        if units > position.character {
            return Err("position splits a UTF-16 character".to_owned());
        }
    }
    (units == position.character)
        .then_some(range.end)
        .ok_or_else(|| "position character is outside the line".to_owned())
}

fn line_start_byte(text: &str, byte: usize) -> usize {
    line_at_byte(text, byte).1
}

#[cfg(test)]
mod tests {
    use super::{
        Config, LspPosition, LspRange, State, TextChange, byte_to_position, position_to_byte,
    };

    // Always reap children, including when a handshake or assertion fails.
    struct TestChildren(Vec<std::process::Child>);

    impl Drop for TestChildren {
        fn drop(&mut self) {
            for child in &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    #[test]
    fn concurrent_process_additions_preserve_manual_content() {
        if let Ok(path) = std::env::var("FERROLEX_TEST_DICTIONARY") {
            let mut state = State::new(Config {
                user_dictionary_path: Some(path.into()),
                ..Config::default()
            });
            let word = std::env::var("FERROLEX_TEST_WORD").unwrap();
            // Both children have loaded their initial snapshots before the parent releases them.
            std::fs::write(format!("{word}.ready"), "").unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !std::path::Path::new("release").exists() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "parent did not release children"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            state.add_user_word(&word).unwrap();
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("words");
        std::fs::write(&path, "# manual comment\noriginal\n").unwrap();
        let alias = dir.path().join("alias");
        std::fs::hard_link(&path, &alias).unwrap();
        let exe = std::env::current_exe().unwrap();
        let mut children = TestChildren(Vec::new());
        for word in ["alpha", "beta"] {
            children.0.push(
                std::process::Command::new(&exe)
                    .current_dir(dir.path())
                    .args([
                        "--exact",
                        "tests::concurrent_process_additions_preserve_manual_content",
                    ])
                    .env(
                        "FERROLEX_TEST_DICTIONARY",
                        if word == "alpha" { &path } else { &alias },
                    )
                    .env("FERROLEX_TEST_WORD", word)
                    .spawn()
                    .unwrap(),
            );
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !dir.path().join("alpha.ready").exists() || !dir.path().join("beta.ready").exists() {
            for child in &mut children.0 {
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "child exited before handshake"
                );
            }
            assert!(
                std::time::Instant::now() < deadline,
                "children did not become ready"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        std::fs::write(&path, "# manual comment\noriginal\nmanual edit").unwrap();
        std::fs::write(dir.path().join("release"), "").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        for child in &mut children.0 {
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success(), "child failed: {status}");
                    break;
                }
                assert!(std::time::Instant::now() < deadline, "child did not finish");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# manual comment\noriginal\nmanual edit\n"));
        assert_eq!(std::fs::read_to_string(alias).unwrap(), text);
        let state = State::new(Config {
            user_dictionary_path: Some(path),
            ..Config::default()
        });
        assert!(state.findings("original alpha beta").is_empty());
    }

    #[test]
    fn failed_write_keeps_pending_words_for_retry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("words");
        std::fs::create_dir(&path).unwrap();
        let mut state = State::new(Config {
            user_dictionary_path: Some(path.clone()),
            ..Config::default()
        });
        assert!(state.add_user_word("first").is_err());
        std::fs::remove_dir(&path).unwrap();
        state.add_user_word("second").unwrap();
        assert!(state.findings("first second").is_empty());
        let reloaded = State::new(Config {
            user_dictionary_path: Some(path),
            ..Config::default()
        });
        assert!(reloaded.findings("first second").is_empty());
    }

    #[test]
    fn incrementally_reanalyzes_utf16_documents() {
        let mut state = State::new(Config {
            words: vec!["ferrolex".to_owned()],
            ..Config::default()
        });
        state.open("file:///test".to_owned(), 1, "ferolex 😀".to_owned());
        assert_eq!(state.diagnostics("file:///test").len(), 1);
        state
            .change(
                "file:///test",
                2,
                vec![TextChange {
                    range: Some(LspRange {
                        start: LspPosition {
                            line: 0,
                            character: 0,
                        },
                        end: LspPosition {
                            line: 0,
                            character: 7,
                        },
                    }),
                    text: "ferrolex".to_owned(),
                }],
            )
            .expect("valid change");
        assert!(state.diagnostics("file:///test").is_empty());
        assert_eq!(
            position_to_byte(
                "😀",
                LspPosition {
                    line: 0,
                    character: 2
                }
            ),
            Ok(4)
        );
        assert_eq!(byte_to_position("😀", 4)["character"], 2);
    }

    #[test]
    fn line_endings_preserve_diagnostics_edits_and_ignore_actions() {
        for ending in ["\n", "\r\n", "\r"] {
            let mut state = State::new(Config {
                words: vec!["hello".to_owned(), "world".to_owned()],
                ..Config::default()
            });
            let uri = "file:///test";
            state.open(uri.to_owned(), 1, format!("hello{ending}😀 wrld"));
            let diagnostics = state.diagnostics(uri);
            assert_eq!(diagnostics.len(), 1);
            let range = serde_json::json!({"start": {"line": 1, "character": 3}, "end": {"line": 1, "character": 7}});
            assert_eq!(diagnostics[0]["range"], range);
            let edit_range = LspRange {
                start: LspPosition {
                    line: 1,
                    character: 3,
                },
                end: LspPosition {
                    line: 1,
                    character: 7,
                },
            };
            let actions = state.code_actions(
                uri,
                LspRange {
                    start: LspPosition {
                        line: 1,
                        character: 3,
                    },
                    end: LspPosition {
                        line: 1,
                        character: 7,
                    },
                },
            );
            let ignore = actions
                .iter()
                .find(|action| {
                    action["title"]
                        .as_str()
                        .is_some_and(|title| title.starts_with("Ignore "))
                })
                .unwrap();
            assert_eq!(
                ignore["edit"]["changes"][uri][0]["range"],
                serde_json::json!({"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 0}})
            );
            state
                .change(
                    uri,
                    2,
                    vec![TextChange {
                        range: Some(edit_range),
                        text: "world".to_owned(),
                    }],
                )
                .unwrap();
            assert_eq!(state.documents[uri].text, format!("hello{ending}😀 world"));
            assert!(state.diagnostics(uri).is_empty());
        }
    }

    #[test]
    fn crlf_interior_maps_to_the_end_of_the_preceding_line() {
        let text = "😀\r\nnext";
        assert_eq!(
            byte_to_position(text, 5),
            serde_json::json!({"line": 0, "character": 2})
        );
        assert_eq!(super::line_start_byte(text, 5), 0);
        assert_eq!(
            byte_to_position(text, 6),
            serde_json::json!({"line": 1, "character": 0})
        );
    }

    #[test]
    fn rejects_invalid_utf16_positions_and_accepts_trailing_empty_lines() {
        for ending in ["\n", "\r\n", "\r"] {
            let text = format!("😀{ending}");
            assert_eq!(
                position_to_byte(
                    &text,
                    LspPosition {
                        line: 1,
                        character: 0
                    }
                ),
                Ok(text.len())
            );
            assert!(
                position_to_byte(
                    &text,
                    LspPosition {
                        line: 2,
                        character: 0
                    }
                )
                .is_err()
            );
            assert!(
                position_to_byte(
                    &text,
                    LspPosition {
                        line: 0,
                        character: 1
                    }
                )
                .is_err()
            );
            assert!(
                position_to_byte(
                    &text,
                    LspPosition {
                        line: 0,
                        character: 3
                    }
                )
                .is_err()
            );
        }
    }

    #[test]
    fn offers_whole_identifier_edits_and_adds_user_words() {
        let mut state = State::new(Config {
            words: vec!["Ferrolex".to_owned(), "project".to_owned()],
            ..Config::default()
        });
        state.open("file:///test".to_owned(), 1, "FerrolexProjec".to_owned());
        let actions = state.code_actions(
            "file:///test",
            LspRange {
                start: LspPosition {
                    line: 0,
                    character: 0,
                },
                end: LspPosition {
                    line: 0,
                    character: 14,
                },
            },
        );
        assert!(actions.iter().any(|action| {
            action["title"]
                .as_str()
                .is_some_and(|title| title.contains("FerrolexProject"))
        }));
        state.add_user_word("Projec").expect("valid word");
        assert!(state.diagnostics("file:///test").is_empty());
    }
}

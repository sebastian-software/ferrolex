//! Generic stdio Language Server Protocol integration for ferrolex.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use ferrolex_code::{Analyzer, Document, Finding};
use ferrolex_core::{Checker, Normalization, UserDictionary, WordList};
use ferrolex_suggest::{SuggestConfig, Suggester};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use serde::Deserialize;
use serde_json::{json, Value};

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

    fn persist_user_dictionary(&self) -> Result<(), String> {
        let Some(path) = &self.user_dictionary_path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(path, self.user.to_text()).map_err(|error| error.to_string())
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

fn byte_to_position(text: &str, byte: usize) -> Value {
    let before = &text[..byte];
    let line = before.bytes().filter(|byte| *byte == b'\n').count();
    let start = before.rfind('\n').map_or(0, |index| index + 1);
    json!({"line": line, "character": text[start..byte].encode_utf16().count()})
}

fn position_to_byte(text: &str, position: LspPosition) -> Result<usize, String> {
    let start = text
        .split_inclusive('\n')
        .take(position.line as usize)
        .map(str::len)
        .sum::<usize>();
    if start > text.len() {
        return Err("position line is outside the document".to_owned());
    }
    let line = text[start..].split('\n').next().unwrap_or_default();
    let mut units = 0u32;
    for (offset, character) in line.char_indices() {
        if units == position.character {
            return Ok(start + offset);
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
        .then_some(start + line.len())
        .ok_or_else(|| "position character is outside the line".to_owned())
}

fn line_start_byte(text: &str, byte: usize) -> usize {
    text[..byte].rfind('\n').map_or(0, |index| index + 1)
}

#[cfg(test)]
mod tests {
    use super::{
        byte_to_position, position_to_byte, Config, LspPosition, LspRange, State, TextChange,
    };

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
        assert!(actions.iter().any(|action| action["title"]
            .as_str()
            .is_some_and(|title| title.contains("FerrolexProject"))));
        state.add_user_word("Projec").expect("valid word");
        assert!(state.diagnostics("file:///test").is_empty());
    }
}

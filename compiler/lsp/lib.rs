use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use compiler__check_session::CheckSession;
use compiler__reports::{CompilerFailure, CompilerFailureKind, RenderedDiagnostic};
use compiler__source::path_to_key;
use serde_json::{Value, json};

pub fn run_lsp_stdio(workspace_root_override: Option<&str>) -> Result<(), CompilerFailure> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = BufWriter::new(stdout.lock());
    let mut lsp_server = LspServer::new(workspace_root_override);
    lsp_server.run(&mut reader, &mut writer)
}

struct LspServer {
    check_session: CheckSession,
    shutdown_requested: bool,
    published_diagnostic_uri_set: BTreeSet<String>,
    source_override_by_path: BTreeMap<String, String>,
}

impl LspServer {
    fn new(workspace_root_override: Option<&str>) -> Self {
        Self {
            check_session: CheckSession::new(workspace_root_override.map(ToString::to_string)),
            shutdown_requested: false,
            published_diagnostic_uri_set: BTreeSet::new(),
            source_override_by_path: BTreeMap::new(),
        }
    }

    fn run<R: BufRead, W: Write>(
        &mut self,
        reader: &mut R,
        writer: &mut W,
    ) -> Result<(), CompilerFailure> {
        loop {
            let Some(message_bytes) = read_lsp_message(reader)? else {
                return Ok(());
            };
            let message: Value =
                serde_json::from_slice(&message_bytes).map_err(|error| CompilerFailure {
                    kind: CompilerFailureKind::RunFailed,
                    message: format!("invalid lsp json payload: {error}"),
                    path: None,
                    details: Vec::new(),
                })?;

            if let Some(method) = message.get("method").and_then(Value::as_str) {
                if message.get("id").is_some() {
                    self.handle_request(writer, &message, method)?;
                } else {
                    let should_exit = self.handle_notification(writer, &message, method)?;
                    if should_exit {
                        if self.shutdown_requested {
                            return Ok(());
                        }
                        return Err(CompilerFailure {
                            kind: CompilerFailureKind::RunFailed,
                            message: "received exit notification before shutdown request"
                                .to_string(),
                            path: None,
                            details: Vec::new(),
                        });
                    }
                }
            }
        }
    }

    fn handle_request<W: Write>(
        &mut self,
        writer: &mut W,
        message: &Value,
        method: &str,
    ) -> Result<(), CompilerFailure> {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        match method {
            "initialize" => {
                let result = json!({
                    "capabilities": {
                        "textDocumentSync": {
                            "openClose": true,
                            "change": 1
                        }
                    },
                    "serverInfo": {
                        "name": "coppice-lsp",
                        "version": "dev"
                    }
                });
                write_lsp_message(
                    writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": result,
                    }),
                )
            }
            "shutdown" => {
                self.shutdown_requested = true;
                write_lsp_message(
                    writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": Value::Null,
                    }),
                )
            }
            _ => write_lsp_message(
                writer,
                &json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("method not found: {method}"),
                    },
                }),
            ),
        }
    }

    fn handle_notification<W: Write>(
        &mut self,
        writer: &mut W,
        message: &Value,
        method: &str,
    ) -> Result<bool, CompilerFailure> {
        match method {
            "exit" => Ok(true),
            "textDocument/didOpen" => {
                let Some(params) = message.get("params") else {
                    return Ok(false);
                };
                let Some(text_document) = params.get("textDocument") else {
                    return Ok(false);
                };
                let Some(uri) = text_document.get("uri").and_then(Value::as_str) else {
                    return Ok(false);
                };
                let Some(text) = text_document.get("text").and_then(Value::as_str) else {
                    return Ok(false);
                };
                self.update_document_and_publish(writer, uri, text.to_string())?;
                Ok(false)
            }
            "textDocument/didChange" => {
                let Some(params) = message.get("params") else {
                    return Ok(false);
                };
                let Some(text_document) = params.get("textDocument") else {
                    return Ok(false);
                };
                let Some(uri) = text_document.get("uri").and_then(Value::as_str) else {
                    return Ok(false);
                };
                let Some(content_changes) = params.get("contentChanges").and_then(Value::as_array)
                else {
                    return Ok(false);
                };
                let Some(new_text) = content_changes
                    .last()
                    .and_then(|change| change.get("text"))
                    .and_then(Value::as_str)
                else {
                    return Ok(false);
                };
                self.update_document_and_publish(writer, uri, new_text.to_string())?;
                Ok(false)
            }
            "textDocument/didClose" => {
                let Some(params) = message.get("params") else {
                    return Ok(false);
                };
                let Some(text_document) = params.get("textDocument") else {
                    return Ok(false);
                };
                let Some(uri) = text_document.get("uri").and_then(Value::as_str) else {
                    return Ok(false);
                };
                self.close_document_and_publish(writer, uri)?;
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn update_document_and_publish<W: Write>(
        &mut self,
        writer: &mut W,
        uri: &str,
        text: String,
    ) -> Result<(), CompilerFailure> {
        let Some(absolute_path) = uri_to_file_path(uri) else {
            return Ok(());
        };
        let target_path = path_to_key(&absolute_path);
        self.check_session
            .open_or_update_document(&target_path, text.clone());
        self.source_override_by_path
            .insert(target_path.clone(), text);
        self.recheck_target_and_publish(writer, &target_path)
    }

    fn close_document_and_publish<W: Write>(
        &mut self,
        writer: &mut W,
        uri: &str,
    ) -> Result<(), CompilerFailure> {
        let Some(absolute_path) = uri_to_file_path(uri) else {
            return Ok(());
        };
        let target_path = path_to_key(&absolute_path);
        self.check_session.close_document(&target_path);
        self.source_override_by_path.remove(&target_path);
        self.recheck_target_and_publish(writer, &target_path)
    }

    fn recheck_target_and_publish<W: Write>(
        &mut self,
        writer: &mut W,
        target_path: &str,
    ) -> Result<(), CompilerFailure> {
        match self.check_session.check_target(target_path) {
            Ok(checked_target) => {
                self.publish_checked_target(writer, checked_target.diagnostics, target_path)
            }
            Err(error) => {
                Self::publish_log_message(writer, &error.message)?;
                if let Some(target_uri) = Self::path_to_uri(target_path) {
                    Self::publish_diagnostics(writer, &target_uri, &[])?;
                    self.published_diagnostic_uri_set.insert(target_uri);
                }
                Ok(())
            }
        }
    }

    fn publish_checked_target<W: Write>(
        &mut self,
        writer: &mut W,
        diagnostics: Vec<RenderedDiagnostic>,
        target_path: &str,
    ) -> Result<(), CompilerFailure> {
        let mut diagnostics_by_uri = BTreeMap::<String, Vec<Value>>::new();
        let mut source_by_diagnostic_path = BTreeMap::<String, Option<String>>::new();
        for diagnostic in diagnostics {
            let Some(uri) = Self::diagnostic_path_to_uri(&diagnostic.path) else {
                continue;
            };
            let source = source_by_diagnostic_path
                .entry(diagnostic.path.clone())
                .or_insert_with(|| self.load_source_for_diagnostic_path(&diagnostic.path));
            diagnostics_by_uri
                .entry(uri)
                .or_default()
                .push(rendered_diagnostic_to_lsp_diagnostic(
                    &diagnostic,
                    source.as_deref(),
                ));
        }

        if let Some(target_uri) = Self::path_to_uri(target_path) {
            diagnostics_by_uri.entry(target_uri).or_default();
        }

        let current_uri_set: BTreeSet<String> = diagnostics_by_uri.keys().cloned().collect();
        for (uri, diagnostics) in diagnostics_by_uri {
            Self::publish_diagnostics(writer, &uri, &diagnostics)?;
        }

        for stale_uri in self
            .published_diagnostic_uri_set
            .difference(&current_uri_set)
            .cloned()
            .collect::<Vec<_>>()
        {
            Self::publish_diagnostics(writer, &stale_uri, &[])?;
        }

        self.published_diagnostic_uri_set = current_uri_set;
        Ok(())
    }

    fn publish_log_message<W: Write>(writer: &mut W, message: &str) -> Result<(), CompilerFailure> {
        write_lsp_message(
            writer,
            &json!({
                "jsonrpc": "2.0",
                "method": "window/logMessage",
                "params": {
                    "type": 1,
                    "message": message,
                }
            }),
        )
    }

    fn publish_diagnostics<W: Write>(
        writer: &mut W,
        uri: &str,
        diagnostics: &[Value],
    ) -> Result<(), CompilerFailure> {
        write_lsp_message(
            writer,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": {
                    "uri": uri,
                    "diagnostics": diagnostics,
                },
            }),
        )
    }

    fn diagnostic_path_to_uri(diagnostic_path: &str) -> Option<String> {
        let diagnostic_file_path = Path::new(diagnostic_path);
        let absolute_path = if diagnostic_file_path.is_absolute() {
            diagnostic_file_path.to_path_buf()
        } else {
            std::env::current_dir()
                .ok()
                .map(|current_directory| current_directory.join(diagnostic_file_path))?
        };
        Some(file_path_to_uri(&absolute_path))
    }

    fn path_to_uri(path: &str) -> Option<String> {
        let path = PathBuf::from(path);
        let absolute_path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .ok()
                .map(|current_directory| current_directory.join(path))?
        };
        Some(file_path_to_uri(&absolute_path))
    }

    fn load_source_for_diagnostic_path(&self, diagnostic_path: &str) -> Option<String> {
        let diagnostic_file_path = Path::new(diagnostic_path);
        let absolute_path = if diagnostic_file_path.is_absolute() {
            diagnostic_file_path.to_path_buf()
        } else {
            std::env::current_dir()
                .ok()
                .map(|current_directory| current_directory.join(diagnostic_file_path))?
        };
        let path_key = path_to_key(&absolute_path);
        if let Some(source_override) = self.source_override_by_path.get(&path_key) {
            return Some(source_override.clone());
        }
        std::fs::read_to_string(absolute_path).ok()
    }
}

fn rendered_diagnostic_to_lsp_diagnostic(
    diagnostic: &RenderedDiagnostic,
    source: Option<&str>,
) -> Value {
    let ((start_line, start_character), (end_line, end_character)) =
        if let Some(source_text) = source {
            span_to_lsp_range(source_text, diagnostic.span.start, diagnostic.span.end)
        } else {
            let line = diagnostic.span.line.saturating_sub(1);
            let character = diagnostic.span.column.saturating_sub(1);
            ((line, character), (line, character + 1))
        };
    json!({
        "range": {
            "start": {
                "line": start_line,
                "character": start_character,
            },
            "end": {
                "line": end_line,
                "character": end_character,
            },
        },
        "severity": 1,
        "source": "coppice",
        "message": diagnostic.message,
    })
}

fn span_to_lsp_range(
    source: &str,
    raw_start_byte_offset: usize,
    raw_end_byte_offset: usize,
) -> ((usize, usize), (usize, usize)) {
    let start_byte_offset = clamp_to_char_boundary(source, raw_start_byte_offset);
    let mut end_byte_offset = clamp_to_char_boundary(source, raw_end_byte_offset);
    if end_byte_offset < start_byte_offset {
        end_byte_offset = start_byte_offset;
    }
    if end_byte_offset == start_byte_offset {
        end_byte_offset =
            next_char_boundary(source, start_byte_offset).unwrap_or(start_byte_offset);
    }
    (
        byte_offset_to_lsp_position(source, start_byte_offset),
        byte_offset_to_lsp_position(source, end_byte_offset),
    )
}

fn byte_offset_to_lsp_position(source: &str, raw_byte_offset: usize) -> (usize, usize) {
    let byte_offset = clamp_to_char_boundary(source, raw_byte_offset);
    let prefix = &source[..byte_offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let line_start_byte_offset = prefix.rfind('\n').map_or(0, |index| index + 1);
    let utf16_character = prefix[line_start_byte_offset..].encode_utf16().count();
    (line, utf16_character)
}

fn clamp_to_char_boundary(source: &str, raw_byte_offset: usize) -> usize {
    let mut byte_offset = raw_byte_offset.min(source.len());
    while byte_offset > 0 && !source.is_char_boundary(byte_offset) {
        byte_offset -= 1;
    }
    byte_offset
}

fn next_char_boundary(source: &str, byte_offset: usize) -> Option<usize> {
    if byte_offset >= source.len() {
        return None;
    }
    source[byte_offset..]
        .chars()
        .next()
        .map(|character| byte_offset + character.len_utf8())
}

fn read_lsp_message<R: BufRead>(reader: &mut R) -> Result<Option<Vec<u8>>, CompilerFailure> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut header_line = String::new();
        let read_bytes = reader
            .read_line(&mut header_line)
            .map_err(|error| CompilerFailure {
                kind: CompilerFailureKind::RunFailed,
                message: format!("failed reading lsp header: {error}"),
                path: None,
                details: Vec::new(),
            })?;
        if read_bytes == 0 {
            return Ok(None);
        }
        if header_line == "\r\n" || header_line == "\n" {
            break;
        }
        if let Some(length_value) = header_line.strip_prefix("Content-Length:") {
            let parsed_length =
                length_value
                    .trim()
                    .parse::<usize>()
                    .map_err(|error| CompilerFailure {
                        kind: CompilerFailureKind::RunFailed,
                        message: format!("invalid content length header: {error}"),
                        path: None,
                        details: Vec::new(),
                    })?;
            content_length = Some(parsed_length);
        }
    }

    let Some(content_length) = content_length else {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::RunFailed,
            message: "lsp message missing Content-Length header".to_string(),
            path: None,
            details: Vec::new(),
        });
    };

    let mut payload = vec![0_u8; content_length];
    reader
        .read_exact(&mut payload)
        .map_err(|error| CompilerFailure {
            kind: CompilerFailureKind::RunFailed,
            message: format!("failed reading lsp payload: {error}"),
            path: None,
            details: Vec::new(),
        })?;
    Ok(Some(payload))
}

fn write_lsp_message<W: Write>(writer: &mut W, message: &Value) -> Result<(), CompilerFailure> {
    let payload = serde_json::to_vec(message).map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::RunFailed,
        message: format!("failed serializing lsp payload: {error}"),
        path: None,
        details: Vec::new(),
    })?;
    write!(writer, "Content-Length: {}\r\n\r\n", payload.len()).map_err(|error| {
        CompilerFailure {
            kind: CompilerFailureKind::RunFailed,
            message: format!("failed writing lsp header: {error}"),
            path: None,
            details: Vec::new(),
        }
    })?;
    writer
        .write_all(&payload)
        .map_err(|error| CompilerFailure {
            kind: CompilerFailureKind::RunFailed,
            message: format!("failed writing lsp payload: {error}"),
            path: None,
            details: Vec::new(),
        })?;
    writer.flush().map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::RunFailed,
        message: format!("failed flushing lsp output: {error}"),
        path: None,
        details: Vec::new(),
    })
}

fn uri_to_file_path(uri: &str) -> Option<PathBuf> {
    let path_component = uri.strip_prefix("file://")?;
    let decoded_path = percent_decode(path_component)?;
    Some(PathBuf::from(decoded_path))
}

fn file_path_to_uri(path: &Path) -> String {
    format!("file://{}", percent_encode(path.to_string_lossy().as_ref()))
}

fn percent_decode(value: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut index = 0;
    let raw = value.as_bytes();
    while index < raw.len() {
        if raw[index] == b'%' {
            if index + 2 >= raw.len() {
                return None;
            }
            let high = hex_value(raw[index + 1])?;
            let low = hex_value(raw[index + 2])?;
            bytes.push((high << 4) | low);
            index += 3;
            continue;
        }
        bytes.push(raw[index]);
        index += 1;
    }
    String::from_utf8(bytes).ok()
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        let should_encode = matches!(
            byte,
            b' ' | b'"' | b'%' | b'<' | b'>' | b'?' | b'`' | b'{' | b'}' | b'#'
        );
        if should_encode {
            encoded.push('%');
            encoded.push(to_hex((byte >> 4) & 0x0f));
            encoded.push(to_hex(byte & 0x0f));
        } else {
            encoded.push(char::from(byte));
        }
    }
    encoded
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn to_hex(value: u8) -> char {
    match value {
        0..=9 => char::from(b'0' + value),
        _ => char::from(b'A' + (value - 10)),
    }
}

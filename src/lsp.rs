//! Language Server。stdio の JSON-RPC で、診断・補完・ホバー・定義・参照・名前の変更・アウトライン・クイックフィックスを提供する

use std::collections::HashMap;
use std::error::Error;

use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::notification::{DidChangeTextDocument, DidOpenTextDocument, DidSaveTextDocument, Notification as _, PublishDiagnostics};
use lsp_types::request::{CodeActionRequest, Completion, DocumentSymbolRequest, GotoDefinition, HoverRequest, References, Rename, Request as _};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CodeActionProviderCapability, CompletionItem, CompletionItemKind,
    CompletionOptions, CompletionParams, CompletionResponse, Diagnostic, DiagnosticSeverity, DocumentSymbol, DocumentSymbolParams,
    DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    Location, MarkupContent, MarkupKind, OneOf, Position, PublishDiagnosticsParams, Range, ReferenceParams, RenameParams,
    ServerCapabilities, SymbolKind, TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri, WorkspaceEdit,
};

use crate::docs::{BUILTINS, METHODS};
use crate::stdlib::math::DOCS as MATH;
use crate::lang::eval::{Interp, KINDS, schema};
use crate::lang::lexer::{Tok, Token, lex};
use crate::lang::value::Value;

const KEYWORDS: &[&str] = &[
    "let", "func", "if", "else", "for", "in", "and", "or", "not", "true", "false", "return", "context", "as", "motion", "output",
    "import", "export", "from", "type", "record",
];
const SYMBOLS: &[&str] = &[
    "center", "topLeft", "topRight", "bottomLeft", "bottomRight", "top", "bottom", "left", "right", "linear", "ease", "ease_in", "ease_out",
    "fade",
];

/// 開いている文書と、最後に保存時に実行したときのトップレベルの値
#[derive(Default)]
struct Doc {
    text: String,
    globals: Vec<(String, Value)>,
}

pub fn run() -> Result<(), Box<dyn Error>> {
    let (connection, io_threads) = Connection::stdio();
    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        completion_provider: Some(CompletionOptions { trigger_characters: Some(vec![".".into(), ":".into()]), ..Default::default() }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        ..Default::default()
    };
    connection.initialize(serde_json::to_value(capabilities)?)?;

    let mut docs: HashMap<String, Doc> = HashMap::new();
    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    break;
                }
                let response = handle_request(&docs, &req);
                connection.sender.send(Message::Response(response))?;
            }
            Message::Notification(note) => {
                if let Some((uri, text, full_check)) = document_update(&note) {
                    let doc = docs.entry(uri.as_str().to_string()).or_default();
                    doc.text = text.clone();
                    let (diagnostics, globals) = diagnose(&uri, &text, full_check);
                    if let Some(g) = globals {
                        doc.globals = g;
                    }
                    // import 先で起きたエラーはそのファイルに出す。開いているファイルには空の診断を送って消す
                    let mut by_file: HashMap<String, Vec<Diagnostic>> = HashMap::new();
                    by_file.insert(uri.as_str().to_string(), Vec::new());
                    for (target, d) in diagnostics {
                        by_file.entry(target.as_str().to_string()).or_default().push(d);
                    }
                    for (target, list) in by_file {
                        let Ok(target_uri) = target.parse::<Uri>() else { continue };
                        let params = PublishDiagnosticsParams { uri: target_uri, diagnostics: list, version: None };
                        connection.sender.send(Message::Notification(Notification::new(PublishDiagnostics::METHOD.into(), params)))?;
                    }
                }
            }
            Message::Response(_) => {}
        }
    }
    // sender を落とさないと書き込みスレッドが終わらない
    drop(connection);
    io_threads.join()?;
    Ok(())
}

/// didOpen / didChange / didSave から (uri, 本文, 実行まで検査するか) を取り出す
fn document_update(note: &Notification) -> Option<(Uri, String, bool)> {
    match note.method.as_str() {
        DidOpenTextDocument::METHOD => {
            let p: lsp_types::DidOpenTextDocumentParams = serde_json::from_value(note.params.clone()).ok()?;
            Some((p.text_document.uri, p.text_document.text, true))
        }
        DidChangeTextDocument::METHOD => {
            let p: lsp_types::DidChangeTextDocumentParams = serde_json::from_value(note.params.clone()).ok()?;
            let text = p.content_changes.into_iter().last()?.text;
            Some((p.text_document.uri, text, false))
        }
        DidSaveTextDocument::METHOD => {
            let p: lsp_types::DidSaveTextDocumentParams = serde_json::from_value(note.params.clone()).ok()?;
            let text = p.text?;
            Some((p.text_document.uri, text, true))
        }
        _ => None,
    }
}

/// 構文解析し、保存時は実行もする。戻り値は (診断の宛先 uri と診断, 実行できたときのトップレベルの値)
fn diagnose(uri: &Uri, text: &str, run: bool) -> (Vec<(Uri, Diagnostic)>, Option<Vec<(String, Value)>>) {
    let stmts = match crate::lang::parser::parse(text) {
        Ok(s) => s,
        Err(e) => return (vec![(uri.clone(), diagnostic(text, &e.kind, &e.message))], None),
    };
    if !run {
        return (vec![], None);
    }
    let mut interp = Interp::new();
    interp.base_dir = file_path(uri).and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_default();
    match interp.run(&stmts) {
        Ok(_) => (vec![], Some(interp.globals())),
        Err(e) => {
            // "in ./x.moph: line N: in ./y.moph: line M: message" は一番内側のファイルに出す
            let (target, message) = innermost_file(uri, &e.message);
            let target_text = if target.as_str() == uri.as_str() { text.to_string() } else { file_path(&target).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default() };
            (vec![(target, diagnostic(&target_text, &e.kind, &message))], None)
        }
    }
}

/// "line N: in ./x.moph: line M: ..." の連鎖を辿り、最後の "in <path>:" のファイルと、その後のメッセージを返す
fn innermost_file(uri: &Uri, message: &str) -> (Uri, String) {
    let mut current = uri.clone();
    let mut rest = message.to_string();
    while let Some(i) = rest.find("in ./").or_else(|| rest.find("in ../")) {
        let after = &rest[i + 3..];
        let Some(end) = after.find(':') else { break };
        let path = &after[..end];
        let Some(resolved) = resolve(&current, path).and_then(|p| file_uri(&p)) else { break };
        current = resolved;
        rest = after[end + 1..].trim_start().to_string();
    }
    (current, rest)
}

/// "line N:C: message" から位置を取り出す。列が無ければ行全体
fn diagnostic(text: &str, kind: &str, message: &str) -> Diagnostic {
    let (line, col) = parse_position(message).unwrap_or((1, 0));
    let line_text = text.lines().nth(line - 1).unwrap_or("");
    let width = line_text.chars().count() as u32;
    let start = if col == 0 { 0 } else { (col - 1) as u32 };
    Diagnostic {
        range: Range { start: Position::new(line as u32 - 1, start), end: Position::new(line as u32 - 1, width.max(start + 1)) },
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("mophila".into()),
        message: format!("{kind}: {message}"),
        ..Default::default()
    }
}

fn parse_position(message: &str) -> Option<(usize, usize)> {
    let rest = message.split("line ").nth(1)?;
    let mut parts = rest.split(':');
    let line: usize = parts.next()?.trim().parse().ok()?;
    let col: usize = parts.next().and_then(|c| c.trim().parse().ok()).unwrap_or(0);
    Some((line, col))
}

fn file_path(uri: &Uri) -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(percent_decode(uri.path().as_str())))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn handle_request(docs: &HashMap<String, Doc>, req: &Request) -> Response {
    fn params<T: serde::de::DeserializeOwned>(req: &Request) -> Result<T, Response> {
        serde_json::from_value(req.params.clone()).map_err(|e| Response::new_err(req.id.clone(), -32602, e.to_string()))
    }
    let doc_of = |uri: &Uri| docs.get(uri.as_str());
    match req.method.as_str() {
        Completion::METHOD => {
            let p: CompletionParams = match params(req) { Ok(p) => p, Err(r) => return r };
            let uri = &p.text_document_position.text_document.uri;
            let text = doc_of(uri).map(|d| d.text.as_str()).unwrap_or("");
            Response::new_ok(req.id.clone(), CompletionResponse::Array(complete(uri, text, p.text_document_position.position)))
        }
        HoverRequest::METHOD => {
            let p: HoverParams = match params(req) { Ok(p) => p, Err(r) => return r };
            let doc = doc_of(&p.text_document_position_params.text_document.uri);
            let text = doc.map(|d| d.text.as_str()).unwrap_or("");
            let globals: &[(String, Value)] = doc.map(|d| d.globals.as_slice()).unwrap_or(&[]);
            Response::new_ok(req.id.clone(), hover(text, globals, p.text_document_position_params.position))
        }
        GotoDefinition::METHOD => {
            let p: GotoDefinitionParams = match params(req) { Ok(p) => p, Err(r) => return r };
            let uri = p.text_document_position_params.text_document.uri;
            let text = doc_of(&uri).map(|d| d.text.as_str()).unwrap_or("");
            let location = definition(&uri, text, p.text_document_position_params.position);
            Response::new_ok(req.id.clone(), location.map(GotoDefinitionResponse::Scalar))
        }
        References::METHOD => {
            let p: ReferenceParams = match params(req) { Ok(p) => p, Err(r) => return r };
            let uri = p.text_document_position.text_document.uri;
            let text = doc_of(&uri).map(|d| d.text.as_str()).unwrap_or("");
            let locations: Vec<Location> = occurrences(text, p.text_document_position.position)
                .into_iter()
                .map(|range| Location { uri: uri.clone(), range })
                .collect();
            Response::new_ok(req.id.clone(), locations)
        }
        Rename::METHOD => {
            let p: RenameParams = match params(req) { Ok(p) => p, Err(r) => return r };
            let uri = p.text_document_position.text_document.uri;
            let text = doc_of(&uri).map(|d| d.text.as_str()).unwrap_or("");
            let edits: Vec<TextEdit> = occurrences(text, p.text_document_position.position)
                .into_iter()
                .map(|range| TextEdit { range, new_text: p.new_name.clone() })
                .collect();
            let mut changes = HashMap::new();
            changes.insert(uri, edits);
            Response::new_ok(req.id.clone(), WorkspaceEdit { changes: Some(changes), ..Default::default() })
        }
        DocumentSymbolRequest::METHOD => {
            let p: DocumentSymbolParams = match params(req) { Ok(p) => p, Err(r) => return r };
            let text = doc_of(&p.text_document.uri).map(|d| d.text.as_str()).unwrap_or("");
            Response::new_ok(req.id.clone(), DocumentSymbolResponse::Nested(symbols(text)))
        }
        CodeActionRequest::METHOD => {
            let p: CodeActionParams = match params(req) { Ok(p) => p, Err(r) => return r };
            Response::new_ok(req.id.clone(), code_actions(&p))
        }
        _ => Response::new_err(req.id.clone(), -32601, format!("unsupported: {}", req.method)),
    }
}

/// カーソル位置の行と、その前の文字列
fn line_before(text: &str, pos: Position) -> String {
    let line = text.lines().nth(pos.line as usize).unwrap_or("");
    line.chars().take(pos.character as usize).collect()
}

fn item(label: &str, kind: CompletionItemKind, detail: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        detail: if detail.is_empty() { None } else { Some(detail.to_string()) },
        ..Default::default()
    }
}

fn complete(uri: &Uri, text: &str, pos: Position) -> Vec<CompletionItem> {
    let before = line_before(text, pos);
    let trimmed = before.trim_end_matches(|c: char| c.is_alphanumeric() || c == '_');
    // :name → 列挙値。ただし `position:` のように名前に続く ':' は属性の区切り
    if trimmed.ends_with(':') {
        let prev = trimmed[..trimmed.len() - 1].chars().last();
        if !prev.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == ':') {
            return SYMBOLS.iter().map(|s| item(s, CompletionItemKind::ENUM_MEMBER, "")).collect();
        }
    }
    let tokens = lex(text).unwrap_or_default();
    // module. → import 先の export
    if trimmed.ends_with('.') {
        let owner: String = trimmed.trim_end_matches('.').chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_').collect::<String>().chars().rev().collect();
        if let Some(path) = import_path_for(&tokens, &owner) {
            if let Some(exports) = exports_of(uri, &path) {
                return exports.into_iter().map(|(n, kind)| item(&n, kind, &path)).collect();
            }
        }
        let mut items: Vec<CompletionItem> = Vec::new();
        for kind in KINDS {
            for (attr, ty) in schema(kind).unwrap_or(&[]) {
                if !items.iter().any(|i| i.label == *attr) {
                    items.push(item(attr, CompletionItemKind::FIELD, &format!("{ty} ({kind} など)")));
                }
            }
        }
        for (m, doc) in METHODS.iter().map(|m| (&m.name, &m.doc)) {
            items.push(item(m, CompletionItemKind::METHOD, doc));
        }
        return items;
    }
    // new Circle { の中 → その型の属性
    if let Some(kind) = enclosing_new(text, pos) {
        if let Some(attrs) = schema(&kind) {
            return attrs.iter().map(|(a, t)| item(a, CompletionItemKind::FIELD, t)).collect();
        }
    }
    // それ以外: キーワード、型名、ファイル内の識別子
    let mut items: Vec<CompletionItem> = KEYWORDS.iter().map(|k| item(k, CompletionItemKind::KEYWORD, "")).collect();
    items.extend(KINDS.iter().map(|k| item(k, CompletionItemKind::CLASS, &attrs_doc(k))));
    for b in ["log", "type_of", "vector!", "apos!", "rgb!", "rgba!"] {
        items.push(item(b, CompletionItemKind::FUNCTION, ""));
    }
    items.extend(crate::stdlib::names().into_iter().map(|n| item(n, CompletionItemKind::MODULE, "標準ライブラリ")));
    let mut seen = std::collections::HashSet::new();
    for t in &tokens {
        if let Tok::Ident(name) = &t.tok {
            if seen.insert(name.clone()) && !items.iter().any(|i| i.label == *name) {
                items.push(item(name, CompletionItemKind::VARIABLE, ""));
            }
        }
    }
    items
}

/// カーソルが `T(` の中なら T (型の生成の途中)
fn enclosing_new(text: &str, pos: Position) -> Option<String> {
    let upto: String = text.lines().take(pos.line as usize).map(|l| format!("{l}\n")).collect::<String>() + &line_before(text, pos);
    let tokens = lex(&upto).ok()?;
    let mut depth: i32 = 0;
    for w in tokens.windows(2).rev() {
        match &w[1].tok {
            Tok::RParen => depth += 1,
            Tok::LParen => {
                if depth == 0 {
                    return match &w[0].tok {
                        Tok::Ident(k) if k.starts_with(char::is_uppercase) => Some(k.clone()),
                        _ => None,
                    };
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// import 先のソース。ファイルか、本体に埋め込んだ標準ライブラリの .moph
fn module_source(uri: &Uri, path: &str) -> Option<String> {
    if !path.ends_with(".moph") {
        return match crate::stdlib::find(path)? {
            crate::stdlib::Lib::Script(src) => Some(src.to_string()),
            crate::stdlib::Lib::Native(_) => None,
        };
    }
    std::fs::read_to_string(resolve(uri, path)?).ok()
}

/// import 先の export と output
fn exports_of(uri: &Uri, path: &str) -> Option<Vec<(String, CompletionItemKind)>> {
    if path == "math" {
        return Some(MATH.iter().map(|e| (e.name.to_string(), if e.signature.contains('(') { CompletionItemKind::FUNCTION } else { CompletionItemKind::CONSTANT })).collect());
    }
    let src = module_source(uri, path)?;
    let tokens = lex(&src).ok()?;
    let mut out = Vec::new();
    for w in tokens.windows(3) {
        if w[0].tok == Tok::Export {
            match (&w[1].tok, &w[2].tok) {
                (Tok::Let, Tok::Ident(n)) => out.push((n.clone(), CompletionItemKind::VARIABLE)),
                (Tok::Func, Tok::Ident(n)) => out.push((n.clone(), CompletionItemKind::FUNCTION)),
                _ => {}
            }
        }
    }
    if tokens.iter().any(|t| t.tok == Tok::Output) {
        out.push(("output".into(), CompletionItemKind::VARIABLE));
    }
    Some(out)
}

fn attrs_doc(kind: &str) -> String {
    schema(kind).map(|s| s.iter().map(|(a, t)| format!("{a}: {t}")).collect::<Vec<_>>().join(", ")).unwrap_or_default()
}

fn hover(text: &str, globals: &[(String, Value)], pos: Position) -> Option<Hover> {
    let (word, _) = word_at(text, pos)?;
    let body = if let Some(attrs) = schema(&word) {
        let list = attrs.iter().map(|(a, t)| format!("- `{a}`: {t}")).collect::<Vec<_>>().join("\n");
        format!("**{word}**\n\n{list}")
    } else if let Some((_, v)) = globals.iter().find(|(n, _)| *n == word) {
        let shown = v.to_string();
        let shown: String = shown.chars().take(120).collect();
        format!("`{word}`: {}\n\n```\n{shown}\n```\n\n(保存時の実行結果)", v.type_name())
    } else if let Some(b) = BUILTINS.iter().chain(MATH).find(|b| b.name.trim_end_matches('!') == word) {
        format!("`{}` — {}", b.signature, b.doc)
    } else if let Some(m) = METHODS.iter().find(|m| m.name == word) {
        let all: Vec<String> = METHODS.iter().filter(|m| m.name == word).map(|m| format!("- `{}` ({}) — {}", m.signature, m.receiver, m.doc)).collect();
        if all.len() > 1 { format!("**{word}**\n\n{}", all.join("\n")) } else { format!("`{}` ({}) — {}", m.signature, m.receiver, m.doc) }
    } else if KEYWORDS.contains(&word.as_str()) {
        format!("キーワード `{word}`")
    } else {
        return None;
    };
    Some(Hover { contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value: body }), range: None })
}

/// カーソルの下の語と同じ識別子の出現 (ファイル内)
fn occurrences(text: &str, pos: Position) -> Vec<Range> {
    let Some((word, _)) = word_at(text, pos) else { return vec![] };
    let Ok(tokens) = lex(text) else { return vec![] };
    tokens
        .iter()
        .filter(|t| matches!(&t.tok, Tok::Ident(n) if *n == word))
        .map(|t| token_range(t, word.chars().count()))
        .collect()
}

fn token_range(t: &Token, len: usize) -> Range {
    Range { start: Position::new(t.line as u32 - 1, t.col as u32 - 1), end: Position::new(t.line as u32 - 1, (t.col + len) as u32 - 1) }
}

/// アウトライン: let / func / type / record の定義
fn symbols(text: &str) -> Vec<DocumentSymbol> {
    let Ok(tokens) = lex(text) else { return vec![] };
    let mut out = Vec::new();
    for w in tokens.windows(2) {
        let (kind, detail) = match (&w[0].tok, &w[1].tok) {
            (Tok::Let, Tok::Ident(_)) => (SymbolKind::VARIABLE, "let"),
            (Tok::Func, Tok::Ident(_)) => (SymbolKind::FUNCTION, "func"),
            (Tok::Type, Tok::Ident(_)) => (SymbolKind::CLASS, "type"),
            (Tok::RecordKw, Tok::Ident(_)) => (SymbolKind::STRUCT, "record"),
            _ => continue,
        };
        let Tok::Ident(name) = &w[1].tok else { continue };
        let range = token_range(&w[1], name.chars().count());
        #[allow(deprecated)]
        out.push(DocumentSymbol {
            name: name.clone(),
            detail: Some(detail.into()),
            kind,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: None,
        });
    }
    out
}

/// 診断に「add "import x"」のヒントがあれば、先頭に import を挿入する修正を出す
fn code_actions(p: &CodeActionParams) -> Vec<CodeActionOrCommand> {
    let mut out = Vec::new();
    for d in &p.context.diagnostics {
        let Some(i) = d.message.find("add \"import ") else { continue };
        let rest = &d.message[i + "add \"".len()..];
        let Some(end) = rest.find('"') else { continue };
        let statement = &rest[..end];
        let mut changes = HashMap::new();
        changes.insert(
            p.text_document.uri.clone(),
            vec![TextEdit { range: Range { start: Position::new(0, 0), end: Position::new(0, 0) }, new_text: format!("{statement}\n") }],
        );
        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: format!("先頭に {statement} を挿入"),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![d.clone()]),
            edit: Some(WorkspaceEdit { changes: Some(changes), ..Default::default() }),
            ..Default::default()
        }));
    }
    out
}

/// カーソルの下の語を、このファイルか import 先のファイルの定義に結び付ける
fn definition(uri: &Uri, text: &str, pos: Position) -> Option<Location> {
    let (word, before) = word_at(text, pos)?;
    let tokens = lex(text).ok()?;
    // module.name → module が import で束縛されていれば、そのファイルの name
    if before.ends_with('.') {
        let module: String = before.trim_end_matches('.').chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_').collect::<String>().chars().rev().collect();
        if let Some(path) = import_path_for(&tokens, &module) {
            if !path.ends_with(".moph") {
                return None;
            }
            let target = resolve(uri, &path)?;
            let src = std::fs::read_to_string(&target).ok()?;
            let def = find_definition(&lex(&src).ok()?, &word).or_else(|| if word == "output" { Some((1, 1)) } else { None })?;
            return Some(Location { uri: file_uri(&target)?, range: point(def) });
        }
    }
    if let Some(def) = find_definition(&tokens, &word) {
        return Some(Location { uri: uri.clone(), range: point(def) });
    }
    if let Some(path) = import_path_for(&tokens, &word) {
        if !path.ends_with(".moph") {
            return None;
        }
        let target = resolve(uri, &path)?;
        let src = std::fs::read_to_string(&target).ok()?;
        let def = find_definition(&lex(&src).ok()?, &word).unwrap_or((1, 1));
        return Some(Location { uri: file_uri(&target)?, range: point(def) });
    }
    None
}

fn point((line, col): (usize, usize)) -> Range {
    let p = Position::new(line as u32 - 1, col as u32 - 1);
    Range { start: p, end: p }
}

/// let / export let / func / type / record の直後にある name の位置
fn find_definition(tokens: &[Token], name: &str) -> Option<(usize, usize)> {
    tokens.windows(2).find_map(|w| match (&w[0].tok, &w[1].tok) {
        (Tok::Let | Tok::Func | Tok::Type | Tok::RecordKw, Tok::Ident(n)) if n == name => Some((w[1].line, w[1].col)),
        _ => None,
    })
}

/// name を束縛している import の元 (相対パス)。import .a.b / import "f" as name / import { name } from .x
fn import_path_for(tokens: &[Token], name: &str) -> Option<String> {
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].tok != Tok::Import {
            i += 1;
            continue;
        }
        i += 1;
        let mut names: Vec<String> = Vec::new();
        let mut selective = false;
        if tokens.get(i).map(|t| &t.tok) == Some(&Tok::LBrace) {
            selective = true;
            i += 1;
            while let Some(t) = tokens.get(i) {
                match &t.tok {
                    Tok::Ident(n) => names.push(n.clone()),
                    Tok::RBrace => {
                        i += 1;
                        break;
                    }
                    _ => {}
                }
                i += 1;
            }
            if matches!(tokens.get(i).map(|t| &t.tok), Some(Tok::Ident(k)) if k == "from") {
                i += 1;
            }
        }
        let (path, default) = match tokens.get(i).map(|t| &t.tok) {
            // 名前だけなら標準ライブラリ。path はその名前のまま
            Some(Tok::Ident(n)) if n != "from" => {
                i += 1;
                (n.clone(), n.clone())
            }
            Some(Tok::Str(p)) => {
                i += 1;
                let stem = std::path::Path::new(p).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                (p.clone(), stem)
            }
            Some(Tok::Dot | Tok::DotDot) => {
                let mut up = 0;
                loop {
                    match tokens.get(i).map(|t| &t.tok) {
                        Some(Tok::Dot) => {
                            i += 1;
                            break;
                        }
                        Some(Tok::DotDot) => {
                            i += 1;
                            up += 1;
                            if !matches!(tokens.get(i).map(|t| &t.tok), Some(Tok::Dot | Tok::DotDot)) {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
                let mut parts = Vec::new();
                while let Some(Tok::Ident(n)) = tokens.get(i).map(|t| &t.tok) {
                    parts.push(n.clone());
                    i += 1;
                    if tokens.get(i).map(|t| &t.tok) == Some(&Tok::Dot) {
                        i += 1;
                    } else {
                        break;
                    }
                }
                let prefix = if up == 0 { "./".to_string() } else { "../".repeat(up) };
                (format!("{prefix}{}.moph", parts.join("/")), parts.last().cloned().unwrap_or_default())
            }
            _ => continue,
        };
        let bound = if selective {
            names.contains(&name.to_string())
        } else {
            let alias = match (tokens.get(i).map(|t| &t.tok), tokens.get(i + 1).map(|t| &t.tok)) {
                (Some(Tok::As), Some(Tok::Ident(a))) => a.clone(),
                _ => default,
            };
            alias == name
        };
        if bound {
            return Some(path);
        }
    }
    None
}

fn resolve(uri: &Uri, path: &str) -> Option<std::path::PathBuf> {
    let base = file_path(uri)?;
    let dir = base.parent()?;
    Some(std::path::PathBuf::from(crate::bundle::normalize(&dir.join(path))))
}

fn file_uri(path: &std::path::Path) -> Option<Uri> {
    let abs = std::path::absolute(path).ok()?;
    let mut encoded = String::from("file://");
    for c in abs.to_string_lossy().chars() {
        match c {
            '/' | '.' | '-' | '_' | '~' => encoded.push(c),
            c if c.is_ascii_alphanumeric() => encoded.push(c),
            c => {
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).bytes() {
                    encoded.push_str(&format!("%{b:02X}"));
                }
            }
        }
    }
    encoded.parse().ok()
}

/// カーソルの下の語と、その前の文字列
fn word_at(text: &str, pos: Position) -> Option<(String, String)> {
    let line = text.lines().nth(pos.line as usize)?;
    let chars: Vec<char> = line.chars().collect();
    let at = (pos.character as usize).min(chars.len());
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut start = at;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = at;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    Some((chars[start..end].iter().collect(), chars[..start].iter().collect()))
}

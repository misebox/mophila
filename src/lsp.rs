//! Language Server。stdio の JSON-RPC で、診断・補完・ホバー・定義・参照・名前の変更・アウトライン・クイックフィックスを提供する

use std::collections::HashMap;
use std::error::Error;

use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::notification::{DidChangeTextDocument, DidOpenTextDocument, DidSaveTextDocument, Notification as _, PublishDiagnostics};
use lsp_types::request::{CodeActionRequest, Completion, DocumentSymbolRequest, GotoDefinition, HoverRequest, References, Rename, Request as _};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CodeActionProviderCapability, CompletionItem, CompletionItemKind,
    CompletionItemLabelDetails, CompletionOptions, CompletionParams, CompletionResponse, Diagnostic, DiagnosticSeverity, DocumentSymbol, DocumentSymbolParams,
    DocumentSymbolResponse, Documentation, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    Location, MarkupContent, MarkupKind, OneOf, Position, PublishDiagnosticsParams, Range, ReferenceParams, RenameParams,
    ServerCapabilities, SymbolKind, TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri, WorkspaceEdit,
};

use crate::docs::BUILTINS;
use crate::lang::method::METHODS;
use crate::stdlib::math::DOCS as MATH;
use crate::lang::eval::{Attr, Interp, KINDS, schema};
use crate::lang::lexer::{Tok, Token, lex};
use crate::lang::value::Value;

const KEYWORDS: &[&str] = &[
    "let", "func", "if", "else", "for", "in", "and", "or", "not", "true", "false", "return", "context", "as", "motion", "output",
    "import", "export", "from", "type", "record", "struct", "method", "private", "alias",
];
const SYMBOLS: &[&str] = &[
    "center", "topLeft", "topRight", "bottomLeft", "bottomRight", "top", "bottom", "left", "right", "linear", "ease", "ease_in", "ease_out",
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
        Err(e) => return (vec![(uri.clone(), diagnostic(text, e.kind.name(), &e.message))], None),
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
            (vec![(target, diagnostic(&target_text, e.kind.name(), &message))], None)
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
            let uri = &p.text_document_position_params.text_document.uri;
            Response::new_ok(req.id.clone(), hover(uri, text, globals, p.text_document_position_params.position))
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

/// 属性 1 つ。必須なら型のうしろに出し、一覧の先頭へ並べる
fn attr_item(a: &Attr, where_: &str) -> CompletionItem {
    let detail = match (a.required, where_.is_empty()) {
        (true, true) => format!("{} (必須)", a.ty),
        (true, false) => format!("{} (必須) — {where_}", a.ty),
        (false, true) => a.ty.to_string(),
        (false, false) => format!("{} — {where_}", a.ty),
    };
    CompletionItem {
        label: a.name.to_string(),
        kind: Some(CompletionItemKind::FIELD),
        detail: Some(detail),
        label_details: Some(CompletionItemLabelDetails { detail: Some(if a.required { " 必須".into() } else { String::new() }), description: Some(a.ty.to_string()) }),
        sort_text: Some(format!("{}{}", if a.required { 0 } else { 1 }, a.name)),
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
                return exports;
            }
        }
        let mut items: Vec<CompletionItem> = Vec::new();
        for kind in KINDS {
            for a in schema(kind).unwrap_or(&[]) {
                if !items.iter().any(|i| i.label == a.name) {
                    items.push(attr_item(a, &format!("{kind} など")));
                }
            }
        }
        for (m, doc) in METHODS.iter().map(|m| (&m.name, &m.doc)) {
            items.push(item(m, CompletionItemKind::METHOD, doc));
        }
        return items;
    }
    // T( の中 → その型の属性。f( の中 → その関数の引数。どちらも必須が先に並ぶ
    if let Some(callee) = enclosing_call(text, pos) {
        if let Some(attrs) = schema(&callee) {
            return attrs.iter().map(|a| attr_item(a, "")).collect();
        }
        if let Some(params) = call_params(uri, text, &callee) {
            return params;
        }
    }
    // それ以外: キーワード、型名、ファイル内の識別子
    let mut items: Vec<CompletionItem> = KEYWORDS.iter().map(|k| item(k, CompletionItemKind::KEYWORD, "")).collect();
    items.extend(KINDS.iter().map(|k| item(k, CompletionItemKind::CLASS, &attrs_doc(k))));
    items.extend(BUILTINS.iter().map(|b| item(b.name, CompletionItemKind::FUNCTION, b.signature)));
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

/// カーソルが `f(` の中なら、その f の名前 (`Circle` `chart.bar` `dot` など)
fn enclosing_call(text: &str, pos: Position) -> Option<String> {
    let upto: String = text.lines().take(pos.line as usize).map(|l| format!("{l}\n")).collect::<String>() + &line_before(text, pos);
    let tokens = lex(&upto).ok()?;
    let mut depth: i32 = 0;
    for (i, t) in tokens.iter().enumerate().rev() {
        match &t.tok {
            Tok::RParen => depth += 1,
            Tok::LParen => {
                if depth == 0 {
                    // 直前が 名前、または module . 名前
                    let Some(Tok::Ident(name)) = tokens.get(i.checked_sub(1)?).map(|t| &t.tok) else { return None };
                    let owner = match (tokens.get(i.wrapping_sub(2)).map(|t| &t.tok), tokens.get(i.wrapping_sub(3)).map(|t| &t.tok)) {
                        (Some(Tok::Dot), Some(Tok::Ident(m))) if i >= 3 => Some(m.clone()),
                        _ => None,
                    };
                    return Some(match owner {
                        Some(m) => format!("{m}.{name}"),
                        None => name.clone(),
                    });
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// import 先の export と output。署名と説明も付ける
fn exports_of(uri: &Uri, path: &str) -> Option<Vec<CompletionItem>> {
    if path == "math" {
        return Some(
            MATH.iter()
                .map(|e| {
                    let kind = if e.signature.contains('(') { CompletionItemKind::FUNCTION } else { CompletionItemKind::CONSTANT };
                    let mut it = item(e.name, kind, e.signature);
                    it.documentation = Some(Documentation::String(e.doc.to_string()));
                    it
                })
                .collect(),
        );
    }
    let (_, src) = module_source(uri, path)?;
    let tokens = lex(&src).ok()?;
    let mut out = Vec::new();
    for w in tokens.windows(3) {
        if w[0].tok == Tok::Export {
            let (name, kind) = match (&w[1].tok, &w[2].tok) {
                (Tok::Let, Tok::Ident(n)) => (n.clone(), CompletionItemKind::VARIABLE),
                (Tok::Func, Tok::Ident(n)) => (n.clone(), CompletionItemKind::FUNCTION),
                _ => continue,
            };
            // 署名を出しておくと、既定値の付いた引数 (= 省ける引数) がその場で分かる
            let detail = signature_in(&src, &name).unwrap_or_else(|| path.to_string());
            let mut it = item(&name, kind, &detail);
            if let Some(doc) = doc_comment_of(&src, &name) {
                it.documentation = Some(Documentation::MarkupContent(MarkupContent { kind: MarkupKind::Markdown, value: doc }));
            }
            out.push(it);
        }
    }
    if tokens.iter().any(|t| t.tok == Tok::Output) {
        out.push(item("output", CompletionItemKind::VARIABLE, "output した View"));
    }
    Some(out)
}

/// その名前を宣言している行の位置。同じ名前が関数の中にもあり得るので、
/// import した側から見える行頭の宣言を先に取る
fn declaration_line(src: &str, name: &str) -> Option<usize> {
    let at = |indented: bool| src.lines().position(|l| defines(l, name) && l.starts_with(char::is_whitespace) == indented);
    at(false).or_else(|| at(true))
}

/// その名前を宣言している行。引数が複数行に折り返してあれば、括弧が閉じるまでつなぐ
fn declaration_of(src: &str, name: &str) -> Option<String> {
    let lines: Vec<&str> = src.lines().collect();
    let start = declaration_line(src, name)?;
    let mut out = String::new();
    for line in &lines[start..] {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(line.trim());
        let depth: i32 = out.chars().map(|c| match c { '(' => 1, ')' => -1, _ => 0 }).sum();
        if depth <= 0 {
            break;
        }
    }
    Some(out)
}

/// その名前を宣言している行の署名
fn signature_in(src: &str, name: &str) -> Option<String> {
    Some(signature_of(declaration_of(src, name)?.trim_start_matches("export ").trim()))
}

/// 関数の引数 1 つ。既定値が無いものが必須
struct Param {
    name: String,
    ty: String,
    default: Option<String>,
}

/// 署名の丸括弧の中を引数に分ける。入れ子の括弧と [ ] の中のカンマでは切らない
fn params_of(signature: &str) -> Vec<Param> {
    let Some(open) = signature.find('(') else { return Vec::new() };
    let mut depth = 0;
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    for c in signature[open..].chars() {
        match c {
            '(' | '[' | '{' | '<' => {
                depth += 1;
                if depth > 1 {
                    current.push(c);
                }
            }
            ')' | ']' | '}' | '>' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                current.push(c);
            }
            ',' if depth == 1 => parts.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    parts.push(current);
    parts
        .iter()
        .filter_map(|p| {
            let p = p.trim();
            if p.is_empty() {
                return None;
            }
            let (head, default) = match p.split_once('=') {
                Some((h, d)) => (h.trim(), Some(d.trim().to_string())),
                None => (p, None),
            };
            let (name, ty) = match head.split_once(':') {
                Some((n, t)) => (n.trim(), t.trim()),
                None => (head, ""),
            };
            Some(Param { name: name.to_string(), ty: ty.to_string(), default })
        })
        .collect()
}

/// 引数 1 つの候補。必須は先に並べ、省けるものは既定値を見せる
fn param_item(p: &Param) -> CompletionItem {
    let detail = match (&p.default, p.ty.is_empty()) {
        (None, true) => "必須".to_string(),
        (None, false) => format!("{} (必須)", p.ty),
        (Some(d), true) => format!("既定 {d}"),
        (Some(d), false) => format!("{} = {d}", p.ty),
    };
    CompletionItem {
        label: p.name.clone(),
        kind: Some(CompletionItemKind::VARIABLE),
        detail: Some(detail),
        insert_text: Some(format!("{} = ", p.name)),
        sort_text: Some(format!("{}{}", if p.default.is_none() { 0 } else { 1 }, p.name)),
        ..Default::default()
    }
}

/// `f(` の中で、その f の引数を候補にする。module.f と、このファイルの func を見る
fn call_params(uri: &Uri, text: &str, callee: &str) -> Option<Vec<CompletionItem>> {
    let signature = match callee.split_once('.') {
        Some((module, name)) => {
            if module == "math" {
                MATH.iter().find(|e| e.name == name).map(|e| e.signature.to_string())?
            } else {
                let path = import_path_for(&lex(text).ok()?, module)?;
                let (_, src) = module_source(uri, &path)?;
                signature_in(&src, name)?
            }
        }
        None => match BUILTINS.iter().find(|b| b.name == callee) {
            Some(b) => b.signature.to_string(),
            None => signature_in(text, callee)?,
        },
    };
    let params = params_of(&signature);
    if params.is_empty() {
        return None;
    }
    Some(params.iter().map(param_item).collect())
}

/// 型の属性を 1 行に。必須には * を付ける
fn attrs_doc(kind: &str) -> String {
    let mark = |a: &Attr| format!("{}{}: {}", a.name, if a.required { "*" } else { "" }, a.ty);
    schema(kind).map(|s| s.iter().map(mark).collect::<Vec<_>>().join(", ")).unwrap_or_default()
}

fn hover(uri: &Uri, text: &str, globals: &[(String, Value)], pos: Position) -> Option<Hover> {
    let (word, before) = word_at(text, pos)?;
    let body = if let Some(attrs) = schema(&word) {
        let line = |a: &Attr| format!("- `{}`: {}{}", a.name, a.ty, if a.required { " **必須**" } else { "" });
        let need: Vec<String> = attrs.iter().filter(|a| a.required).map(line).collect();
        let rest: Vec<String> = attrs.iter().filter(|a| !a.required).map(line).collect();
        format!("**{word}**\n\n{}", [need, rest].concat().join("\n"))
    } else if let Some(doc) = imported_doc(uri, text, &word, &before) {
        doc
    } else if let Some(doc) = doc_comment_of(text, &word) {
        doc
    } else if let Some((_, v)) = globals.iter().find(|(n, _)| *n == word) {
        let shown = v.to_string();
        let shown: String = shown.chars().take(120).collect();
        format!("`{word}`: {}\n\n```\n{shown}\n```\n\n(保存時の実行結果)", v.type_name())
    } else if let Some(b) = BUILTINS.iter().chain(MATH).find(|b| b.name.trim_end_matches('!') == word) {
        format!("`{}` — {}", b.signature, b.doc)
    } else if let Some(m) = METHODS.iter().find(|m| m.name == word) {
        let all: Vec<String> = METHODS.iter().filter(|m| m.name == word).map(|m| format!("- `{}` ({}) — {}", m.signature, m.receivers.join(" / "), m.doc)).collect();
        if all.len() > 1 { format!("**{word}**\n\n{}", all.join("\n")) } else { format!("`{}` ({}) — {}", m.signature, m.receivers.join(" / "), m.doc) }
    } else if KEYWORDS.contains(&word.as_str()) {
        format!("キーワード `{word}`")
    } else {
        return None;
    };
    Some(Hover { contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value: body }), range: None })
}

/// import した名前なら、import 先の ## ドキュメントコメントと署名
fn imported_doc(uri: &Uri, text: &str, word: &str, before: &str) -> Option<String> {
    let tokens = lex(text).ok()?;
    let path = if before.ends_with('.') {
        let module: String = before.trim_end_matches('.').chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_').collect::<String>().chars().rev().collect();
        import_path_for(&tokens, &module)?
    } else {
        import_path_for(&tokens, word)?
    };
    let (_, src) = module_source(uri, &path)?;
    doc_comment_of(&src, word)
}

/// その行が name を定義しているか。let / func / struct / record / type、export 付きも
fn defines(line: &str, name: &str) -> bool {
    let Ok(tokens) = lex(line.trim()) else { return false };
    let mut i = 0;
    if matches!(tokens.first().map(|t| &t.tok), Some(Tok::Export)) {
        i = 1;
    }
    let starts = matches!(
        tokens.get(i).map(|t| &t.tok),
        Some(Tok::Let | Tok::Func | Tok::Type | Tok::RecordKw | Tok::StructKw)
    );
    starts && matches!(tokens.get(i + 1).map(|t| &t.tok), Some(Tok::Ident(n)) if n == name)
}

/// export の行から、本体の { を落とした署名だけを取る
fn signature_of(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let Some(open) = chars.iter().position(|c| *c == '(') else {
        return line.trim_end_matches(" {").trim_end().to_string();
    };
    let mut depth = 0;
    let mut end = chars.len();
    for (i, c) in chars.iter().enumerate().skip(open) {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    // 引数の後ろに -> 型 が続くなら、本体の { の手前まで
    let rest: String = chars[end..].iter().collect();
    let tail = match rest.trim_start().starts_with("->") {
        true => rest.split('{').next().unwrap_or("").trim_end().to_string(),
        false => String::new(),
    };
    format!("{}{}", chars[..end].iter().collect::<String>(), tail)
}

/// `##` の並びと、その次の export の 1 行
fn doc_comment_of(src: &str, name: &str) -> Option<String> {
    let lines: Vec<&str> = src.lines().collect();
    let at = declaration_line(src, name)?;
    // 宣言のすぐ上に続いている ## の並びが説明
    let mut block: Vec<String> = Vec::new();
    for line in lines[..at].iter().rev() {
        match line.strip_prefix("##") {
            Some(rest) => block.push(rest.trim().to_string()),
            None => break,
        }
    }
    block.reverse();
    let signature = signature_in(src, name).unwrap_or_else(|| signature_of(lines[at]));
    let mut out = vec![format!("```\n{signature}\n```")];
    let summary: Vec<&String> = block.iter().take_while(|b| !b.starts_with('@')).collect();
    if !summary.is_empty() {
        out.push(summary.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" "));
    }
    // @param は必須かどうかが分かるように、署名の既定値と突き合わせて出す
    let params = params_of(&signature);
    let tags: Vec<String> = block
        .iter()
        .filter(|b| b.starts_with("@param ") || b.starts_with("@returns "))
        .map(|b| match b.strip_prefix("@param ") {
            Some(rest) => {
                let (n, d) = rest.split_once(' ').unwrap_or((rest, ""));
                let mark = match params.iter().find(|p| p.name == n) {
                    Some(p) if p.default.is_none() => " (必須)",
                    _ => "",
                };
                format!("- `{n}`{mark} — {d}")
            }
            None => format!("- 戻り値 — {}", b.trim_start_matches("@returns ")),
        })
        .collect();
    if !tags.is_empty() {
        out.push(tags.join("\n"));
    }
    Some(out.join("\n\n"))
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
            let (target, src) = module_source(uri, &path)?;
            let def = find_definition(&lex(&src).ok()?, &word).or_else(|| if word == "output" { Some((1, 1)) } else { None })?;
            return Some(Location { uri: file_uri(&target)?, range: point(def) });
        }
    }
    if let Some(def) = find_definition(&tokens, &word) {
        return Some(Location { uri: uri.clone(), range: point(def) });
    }
    if let Some(path) = import_path_for(&tokens, &word) {
        let (target, src) = module_source(uri, &path)?;
        let def = find_definition(&lex(&src).ok()?, &word).unwrap_or((1, 1));
        return Some(Location { uri: file_uri(&target)?, range: point(def) });
    }
    None
}

/// import 先のファイルと中身。標準ライブラリは実行ファイルに埋め込んであるので、
/// 定義へ飛べるように、読める場所へ書き出してからそこを指す
fn module_source(uri: &Uri, path: &str) -> Option<(std::path::PathBuf, String)> {
    if path.ends_with(".moph") {
        let target = resolve(uri, path)?;
        let src = std::fs::read_to_string(&target).ok()?;
        return Some((target, src));
    }
    let (_, src) = crate::stdlib::SCRIPTS.iter().find(|(n, _)| *n == path)?;
    let dir = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("mophila")
        .join("stdlib");
    std::fs::create_dir_all(&dir).ok()?;
    let target = dir.join(format!("{path}.moph"));
    if std::fs::read_to_string(&target).ok().as_deref() != Some(*src) {
        std::fs::write(&target, src).ok()?;
    }
    Some((target, (*src).to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    #[test]
    fn params_split_on_top_level_commas_only() {
        let sig = "bar(values: List<Number>, labels: List<String>, w: Number, colors: List<Color> = [#e04040, #4080e0], duration: Duration = 0s) -> View";
        let params = params_of(sig);
        let names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["values", "labels", "w", "colors", "duration"]);
        assert!(params[0].default.is_none());
        assert_eq!(params[0].ty, "List<Number>");
        assert_eq!(params[3].default.as_deref(), Some("[#e04040, #4080e0]"));
        assert_eq!(params[4].default.as_deref(), Some("0s"));
    }

    #[test]
    fn required_params_come_first() {
        let params = params_of("f(a: Number, b: Number = 1)");
        let items: Vec<CompletionItem> = params.iter().map(param_item).collect();
        assert!(items[0].sort_text < items[1].sort_text);
        assert_eq!(items[0].detail.as_deref(), Some("Number (必須)"));
        assert_eq!(items[1].detail.as_deref(), Some("Number = 1"));
    }

    #[test]
    fn required_attrs_come_first() {
        let attrs = schema("Circle").unwrap();
        let items: Vec<CompletionItem> = attrs.iter().map(|a| attr_item(a, "")).collect();
        let radius = items.iter().find(|i| i.label == "radius").unwrap();
        let fill = items.iter().find(|i| i.label == "fill").unwrap();
        assert_eq!(radius.detail.as_deref(), Some("Number (必須)"));
        assert_eq!(fill.detail.as_deref(), Some("Paint"));
        assert!(radius.sort_text < fill.sort_text);
    }

    #[test]
    fn callee_is_the_name_before_the_open_paren() {
        let text = "let v = View(box = Vector(16, 9))\nv.place(Circle(position = Pos(1, 1), ";
        assert_eq!(enclosing_call(text, pos(1, 36)).as_deref(), Some("Circle"));
        let text = "import chart\nv.place(chart.bar(values, ";
        assert_eq!(enclosing_call(text, pos(1, 26)).as_deref(), Some("chart.bar"));
        let text = "let a = (1 + 2) * 3\n";
        assert_eq!(enclosing_call(text, pos(1, 0)), None);
    }

    #[test]
    fn signature_comes_from_the_defining_line() {
        let src = "## 説明\nexport func make(w: Number, h: Number = 1) -> View {\n  View(box = Vector(w, h))\n}\n";
        assert_eq!(signature_in(src, "make").as_deref(), Some("func make(w: Number, h: Number = 1) -> View"));
    }

    #[test]
    fn the_top_level_declaration_wins_over_one_inside_a_function() {
        let src = "func axes() {\n  let line = gray(150)\n}\n\n## 折れ線\nexport func line(points: List<Vector>) -> View {\n  v\n}\n";
        assert_eq!(signature_in(src, "line").as_deref(), Some("func line(points: List<Vector>) -> View"));
        assert!(doc_comment_of(src, "line").unwrap().contains("折れ線"));
    }

    #[test]
    fn doc_comment_marks_required_params() {
        let src = "## 説明\n## @param a 幅\n## @param b 高さ\nexport func f(a: Number, b: Number = 1) -> View {\n  v\n}\n";
        let doc = doc_comment_of(src, "f").unwrap();
        assert!(doc.contains("- `a` (必須) — 幅"), "{doc}");
        assert!(doc.contains("- `b` — 高さ"), "{doc}");
    }

    #[test]
    fn signature_joins_wrapped_argument_lines() {
        let src = "export func bar(values: List<Number>, labels: List<String>,\n                w: Number, h: Number = 1) -> View {\n  v\n}\n";
        let sig = signature_in(src, "bar").unwrap();
        assert_eq!(sig, "func bar(values: List<Number>, labels: List<String>, w: Number, h: Number = 1) -> View");
        assert_eq!(params_of(&sig).len(), 4);
    }
}

//! JSON-RPC language server (stdio).
//! Supports diagnostics, completion, definitions, document/workspace symbols,
//! import-graph references and rename, workspace signature help, and inferred
//! type inlay hints. The implementation remains intentionally conservative:
//! only top-level functions/structs and confidently inferred local types are
//! offered for refactoring or hints.

use crate::desugar;
use crate::lexer::{Lexer, TokenKind};
use crate::parser::Parser;
use crate::types::TypeChecker;
use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

const KEYWORDS: &[&str] = &[
    "actor",
    "and",
    "arena",
    "as",
    "break",
    "const",
    "continue",
    "crew",
    "default",
    "defer",
    "else",
    "enum",
    "extern",
    "false",
    "fan",
    "fn",
    "for",
    "hold",
    "if",
    "import",
    "in",
    "interface",
    "join",
    "kick",
    "let",
    "match",
    "mut",
    "not",
    "on",
    "or",
    "pack",
    "package",
    "pull",
    "range",
    "receive",
    "return",
    "select",
    "share",
    "struct",
    "timeout",
    "true",
    "while",
];

#[derive(Clone, Debug)]
struct LspSymbol {
    uri: String,
    name: String,
    kind: u32,
    line: u32,
    col: u32,
    len: u32,
    signature: Option<String>,
}

#[derive(Clone, Debug)]
struct LspImport {
    target_uri: String,
    alias: Option<String>,
}

#[derive(Default)]
struct WorkspaceIndex {
    docs: HashMap<String, String>,
    symbols: Vec<LspSymbol>,
    imports: HashMap<String, Vec<LspImport>>,
}

fn read_message(stdin: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = stdin.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        let lower = trimmed.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            content_length = rest.trim().parse().ok();
        }
    }
    let len = content_length
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"))?;
    let mut buf = vec![0u8; len];
    stdin.read_exact(&mut buf)?;
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

fn write_message(stdout: &mut impl Write, body: &str) -> io::Result<()> {
    write!(stdout, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    stdout.flush()
}

fn json_get_str<'a>(obj: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\"");
    let i = obj.find(&pat)?;
    let after = &obj[i + pat.len()..];
    let colon = after.find(':')?;
    let rest = after[colon + 1..].trim_start();
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(&stripped[..end])
    } else {
        None
    }
}

fn json_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('u') => {
                    // skip \uXXXX roughly
                    for _ in 0..4 {
                        let _ = chars.next();
                    }
                }
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn json_get_id(obj: &str) -> String {
    if let Some(i) = obj.find("\"id\"") {
        let after = &obj[i + 4..];
        if let Some(colon) = after.find(':') {
            let rest = after[colon + 1..].trim_start();
            if let Some(stripped) = rest.strip_prefix('"') {
                if let Some(end) = stripped.find('"') {
                    return format!("\"{}\"", &stripped[..end]);
                }
            } else {
                let num: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '-')
                    .collect();
                if !num.is_empty() {
                    return num;
                }
            }
        }
    }
    "null".into()
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn diagnose(src: &str) -> Vec<(u32, u32, u32, u32, String)> {
    // LSP uses 0-based line/character. Lexer/parser use 1-based line/col.
    let tokens = match Lexer::new(src).tokenize() {
        Ok(t) => t,
        Err(e) => {
            let msg = format!("{e}");
            let (line, col) = parse_loc_from_msg(&msg).unwrap_or((1, 1));
            let sl = line.saturating_sub(1) as u32;
            let sc = col.saturating_sub(1) as u32;
            return vec![(sl, sc, sl, sc + 1, msg)];
        }
    };
    let program = match Parser::new(tokens).parse() {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("{e}");
            let (line, col) = parse_loc_from_msg(&msg).unwrap_or((1, 1));
            let sl = line.saturating_sub(1) as u32;
            let sc = col.saturating_sub(1) as u32;
            return vec![(sl, sc, sl, sc + 1, msg)];
        }
    };
    let program = desugar::desugar(program);
    if let Err(e) = TypeChecker::new().check(&program) {
        let msg = format!("{e}");
        let (line, col) = match &e {
            crate::types::TypeError::At { line, col, .. } if *line > 0 => (*line, *col),
            _ => parse_loc_from_msg(&msg).unwrap_or((0, 0)),
        };
        if line > 0 {
            let sl = line.saturating_sub(1) as u32;
            let sc = col.saturating_sub(1) as u32;
            return vec![(sl, sc, sl, sc.saturating_add(1), msg)];
        }
        // Cheap span: locate `` `name` `` from "cannot find `name`" in source.
        if let Some(name) = extract_backtick_name(&msg) {
            if let Some((sl, sc, el, ec)) = find_ident_span(src, &name) {
                return vec![(sl, sc, el, ec, msg)];
            }
        }
        return vec![(0, 0, 0, 1, msg)];
    }
    Vec::new()
}

fn extract_backtick_name(msg: &str) -> Option<String> {
    let start = msg.find('`')?;
    let rest = &msg[start + 1..];
    let end = rest.find('`')?;
    let name = &rest[..end];
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !name.is_empty() {
        Some(name.to_string())
    } else {
        None
    }
}

fn find_ident_span(src: &str, name: &str) -> Option<(u32, u32, u32, u32)> {
    let Ok(tokens) = Lexer::new(src).tokenize() else {
        return None;
    };
    for t in tokens {
        if let TokenKind::Ident(ref n) = t.kind {
            if n == name {
                let sl = t.line.saturating_sub(1) as u32;
                let sc = t.col.saturating_sub(1) as u32;
                return Some((sl, sc, sl, sc + name.len() as u32));
            }
        }
    }
    None
}

/// Parse `at L:C` or `at L:C:` style locations from error Display strings.
fn parse_loc_from_msg(msg: &str) -> Option<(usize, usize)> {
    // "parse error at 2:1: ..." or "unexpected character 'x' at 1:3"
    for marker in [" at ", "at "] {
        if let Some(i) = msg.rfind(marker) {
            let rest = &msg[i + marker.len()..];
            let mut nums = rest
                .split(|c: char| !c.is_ascii_digit())
                .filter(|s| !s.is_empty());
            let line: usize = nums.next()?.parse().ok()?;
            let col: usize = nums.next()?.parse().ok()?;
            return Some((line, col));
        }
    }
    // "parse error at {line}:{col}:"
    if let Some(i) = msg.find("parse error at ") {
        let rest = &msg[i + "parse error at ".len()..];
        let mut parts = rest.split(|c: char| c == ':' || c == ' ');
        let line: usize = parts.next()?.parse().ok()?;
        let col: usize = parts.next()?.parse().ok()?;
        return Some((line, col));
    }
    None
}

/// Collect `fn name` definitions: (name, 0-based line, 0-based col, name_len).
fn collect_fn_defs(src: &str) -> Vec<(String, u32, u32, u32)> {
    let Ok(tokens) = Lexer::new(src).tokenize() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < tokens.len() {
        if matches!(tokens[i].kind, crate::lexer::TokenKind::Fn) {
            if let crate::lexer::TokenKind::Ident(ref name) = tokens[i + 1].kind {
                let line = tokens[i + 1].line.saturating_sub(1) as u32;
                let col = tokens[i + 1].col.saturating_sub(1) as u32;
                out.push((name.clone(), line, col, name.len() as u32));
            }
        }
        i += 1;
    }
    out
}

/// Collect fn signature labels: (name, "name(a: int, b: int) -> int", param_count).
fn collect_fn_sigs(src: &str) -> Vec<(String, String, usize)> {
    let Ok(tokens) = Lexer::new(src).tokenize() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < tokens.len() {
        if matches!(tokens[i].kind, TokenKind::Fn) {
            if let TokenKind::Ident(ref name) = tokens[i + 1].kind {
                let mut j = i + 2;
                if j >= tokens.len() || !matches!(tokens[j].kind, TokenKind::LParen) {
                    i += 1;
                    continue;
                }
                j += 1;
                let mut params: Vec<String> = Vec::new();
                let mut cur = String::new();
                let mut depth = 1i32;
                while j < tokens.len() && depth > 0 {
                    match &tokens[j].kind {
                        TokenKind::LParen => {
                            depth += 1;
                            cur.push('(');
                        }
                        TokenKind::RParen => {
                            depth -= 1;
                            if depth == 0 {
                                if !cur.trim().is_empty() {
                                    params.push(cur.trim().to_string());
                                }
                                break;
                            }
                            cur.push(')');
                        }
                        TokenKind::Comma if depth == 1 => {
                            if !cur.trim().is_empty() {
                                params.push(cur.trim().to_string());
                            }
                            cur.clear();
                        }
                        TokenKind::Ident(s) => {
                            if !cur.is_empty()
                                && !cur.ends_with(' ')
                                && !cur.ends_with(':')
                                && !cur.ends_with(',')
                                && !cur.ends_with('(')
                                && !cur.ends_with('[')
                            {
                                cur.push(' ');
                            }
                            cur.push_str(s);
                        }
                        TokenKind::Colon => cur.push_str(": "),
                        TokenKind::LBracket => cur.push('['),
                        TokenKind::RBracket => cur.push(']'),
                        TokenKind::Arrow => cur.push_str(" -> "),
                        other => {
                            let s = format!("{other}");
                            if s.len() <= 2 {
                                cur.push_str(&s);
                            }
                        }
                    }
                    j += 1;
                }
                let mut ret = String::new();
                j += 1; // past )
                if j < tokens.len() && matches!(tokens[j].kind, TokenKind::Arrow) {
                    j += 1;
                    while j < tokens.len() {
                        match &tokens[j].kind {
                            TokenKind::Ident(s) => {
                                if !ret.is_empty() {
                                    ret.push(' ');
                                }
                                ret.push_str(s);
                                break;
                            }
                            TokenKind::LBrace | TokenKind::Assign => break,
                            _ => {}
                        }
                        j += 1;
                    }
                }
                let label = if ret.is_empty() {
                    format!("{name}({})", params.join(", "))
                } else {
                    format!("{name}({}) -> {ret}", params.join(", "))
                };
                out.push((name.clone(), label, params.len()));
            }
        }
        i += 1;
    }
    out
}

fn signature_parameters(label: &str) -> Vec<String> {
    let Some(open) = label.find('(') else {
        return Vec::new();
    };
    let mut depth = 0i32;
    let mut close = None;
    for (offset, ch) in label[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close) = close else {
        return Vec::new();
    };
    let body = &label[open + 1..close];
    let mut params = Vec::new();
    let mut start = 0usize;
    let mut nested = 0i32;
    for (i, ch) in body.char_indices() {
        match ch {
            '[' | '(' => nested += 1,
            ']' | ')' => nested -= 1,
            ',' if nested == 0 => {
                let part = body[start..i].trim();
                if !part.is_empty() {
                    params.push(part.to_string());
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = body[start..].trim();
    if !last.is_empty() {
        params.push(last.to_string());
    }
    params
}

fn signature_json(label: &str, active_parameter: usize) -> String {
    let params = signature_parameters(label);
    let active = if params.is_empty() {
        0
    } else {
        active_parameter.min(params.len().saturating_sub(1))
    };
    let parameter_json = params
        .iter()
        .map(|param| format!(r#"{{"label":"{}"}}"#, json_escape(param)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"signatures":[{{"label":"{}","parameters":[{}]}}],"activeSignature":0,"activeParameter":{active}}}"#,
        json_escape(label),
        parameter_json
    )
}

fn builtin_signature(name: &str) -> Option<&'static str> {
    match name {
        "print" => Some("print(s: string)"),
        "print_int" => Some("print_int(n: int)"),
        "assert_eq" => Some("assert_eq(a, b)"),
        "pb_encode_simple" => Some("pb_encode_simple(name: string, id: int) -> string"),
        "http2_headers_frame" => {
            Some("http2_headers_frame(stream: int, block: string, flags: int) -> string")
        }
        _ => None,
    }
}

fn call_context(src: &str, line: u32, character: u32) -> Option<(String, u32, u32, usize)> {
    let Ok(tokens) = Lexer::new(src).tokenize() else {
        return None;
    };
    let mut at = None;
    for (i, t) in tokens.iter().enumerate() {
        let tl = t.line.saturating_sub(1) as u32;
        let tc = t.col.saturating_sub(1) as u32;
        if tl < line || (tl == line && tc <= character) {
            at = Some(i);
        } else {
            break;
        }
    }
    let mut i = at?;
    let mut depth = 0i32;
    let mut commas = 0usize;
    loop {
        match &tokens[i].kind {
            TokenKind::RParen => depth += 1,
            TokenKind::LParen => {
                if depth == 0 {
                    if i > 0 {
                        if let TokenKind::Ident(ref name) = tokens[i - 1].kind {
                            let token = &tokens[i - 1];
                            return Some((
                                name.clone(),
                                token.line.saturating_sub(1) as u32,
                                token.col.saturating_sub(1) as u32,
                                commas,
                            ));
                        }
                    }
                    return None;
                }
                depth -= 1;
            }
            TokenKind::Comma if depth == 0 => commas += 1,
            _ => {}
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    None
}

/// textDocument/signatureHelp — resolve call signatures from the workspace index.
fn signature_help(
    index: &WorkspaceIndex,
    uri: &str,
    src: &str,
    line: u32,
    character: u32,
) -> String {
    let Some((name, name_line, name_col, active)) = call_context(src, line, character) else {
        return "null".into();
    };
    if let Some(symbol) = symbol_for_position(index, uri, src, name_line, name_col) {
        if let Some(label) = symbol.signature {
            return signature_json(&label, active);
        }
    }
    builtin_signature(&name)
        .map(|label| signature_json(label, active))
        .unwrap_or_else(|| "null".into())
}

/// Collect `struct Name` definitions (same shape as fn defs).
fn collect_struct_defs(src: &str) -> Vec<(String, u32, u32, u32)> {
    let Ok(tokens) = Lexer::new(src).tokenize() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < tokens.len() {
        if matches!(tokens[i].kind, TokenKind::Struct) {
            if let TokenKind::Ident(ref name) = tokens[i + 1].kind {
                let line = tokens[i + 1].line.saturating_sub(1) as u32;
                let col = tokens[i + 1].col.saturating_sub(1) as u32;
                out.push((name.clone(), line, col, name.len() as u32));
            }
        }
        i += 1;
    }
    out
}

/// LSP DocumentSymbol[] for top-level `fn` / `struct` (kind Function=12, Struct=23).
fn document_symbols(src: &str) -> String {
    let mut items = Vec::new();
    for (name, dl, dc, len) in collect_fn_defs(src) {
        let end = dc + len;
        items.push(format!(
            r#"{{"name":"{name}","kind":12,"range":{{"start":{{"line":{dl},"character":{dc}}},"end":{{"line":{dl},"character":{end}}}}},"selectionRange":{{"start":{{"line":{dl},"character":{dc}}},"end":{{"line":{dl},"character":{end}}}}}}}"#
        ));
    }
    for (name, dl, dc, len) in collect_struct_defs(src) {
        let end = dc + len;
        items.push(format!(
            r#"{{"name":"{name}","kind":23,"range":{{"start":{{"line":{dl},"character":{dc}}},"end":{{"line":{dl},"character":{end}}}}},"selectionRange":{{"start":{{"line":{dl},"character":{dc}}},"end":{{"line":{dl},"character":{end}}}}}}}"#
        ));
    }
    format!("[{}]", items.join(","))
}

fn workspace_symbols_from_index(index: &WorkspaceIndex, query: &str) -> String {
    let q = query.to_ascii_lowercase();
    let mut symbols = index.symbols.clone();
    symbols.sort_by(|a, b| {
        a.uri
            .cmp(&b.uri)
            .then(a.line.cmp(&b.line))
            .then(a.name.cmp(&b.name))
    });
    let items = symbols
        .into_iter()
        .filter(|symbol| q.is_empty() || symbol.name.to_ascii_lowercase().contains(&q))
        .map(|symbol| {
            let end = symbol.col + symbol.len;
            format!(
                r#"{{"name":"{}","kind":{},"location":{{"uri":"{}","range":{{"start":{{"line":{},"character":{}}},"end":{{"line":{},"character":{}}}}}}}}}"#,
                json_escape(&symbol.name),
                symbol.kind,
                json_escape(&symbol.uri),
                symbol.line,
                symbol.col,
                symbol.line,
                end
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", items.join(","))
}

/// Byte offset of (line, col) in src (0-based).
fn offset_at(src: &str, line: u32, character: u32) -> Option<usize> {
    let mut cur_line = 0u32;
    let mut cur_col = 0u32;
    for (i, ch) in src.char_indices() {
        if cur_line == line && cur_col == character {
            return Some(i);
        }
        if ch == '\n' {
            cur_line += 1;
            cur_col = 0;
        } else {
            cur_col += 1;
        }
    }
    if cur_line == line && cur_col == character {
        return Some(src.len());
    }
    None
}

fn resolve_import_path(base: &Path, imp_path: &str) -> PathBuf {
    if Path::new(imp_path).is_absolute() {
        PathBuf::from(imp_path)
    } else {
        base.join(imp_path)
    }
}

fn path_to_uri(path: &Path) -> String {
    let normalized = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    format!("file://{}", normalized.display())
}

fn normalize_uri(uri: &str) -> String {
    uri_to_path(uri)
        .map(|path| path_to_uri(&path))
        .unwrap_or_else(|| uri.to_string())
}

fn import_default_alias(path: &str) -> Option<String> {
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn source_for_uri(uri: &str, open_docs: &HashMap<String, String>) -> Option<String> {
    if let Some(src) = open_docs.get(uri) {
        return Some(src.clone());
    }
    let normalized = normalize_uri(uri);
    if let Some(src) = open_docs.get(&normalized) {
        return Some(src.clone());
    }
    uri_to_path(uri).and_then(|path| std::fs::read_to_string(path).ok())
}

fn build_workspace_index(open_docs: &HashMap<String, String>) -> WorkspaceIndex {
    let mut index = WorkspaceIndex::default();
    let normalized_docs: HashMap<String, String> = open_docs
        .iter()
        .map(|(uri, src)| (normalize_uri(uri), src.clone()))
        .collect();
    let mut pending: Vec<String> = normalized_docs.keys().cloned().collect();
    let mut queued: HashSet<String> = pending.iter().cloned().collect();

    while let Some(uri) = pending.pop() {
        if index.docs.contains_key(&uri) {
            continue;
        }
        let Some(src) = source_for_uri(&uri, &normalized_docs) else {
            continue;
        };
        let imports = if let Some(path) = uri_to_path(&uri) {
            let base = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            collect_imports(&src)
                .into_iter()
                .map(|(imp_path, alias)| {
                    let target = resolve_import_path(&base, &imp_path);
                    let target_uri = path_to_uri(&target);
                    if queued.insert(target_uri.clone()) {
                        pending.push(target_uri.clone());
                    }
                    LspImport { target_uri, alias }
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        index.imports.insert(uri.clone(), imports);
        index.docs.insert(uri.clone(), src.clone());

        let sigs = collect_fn_sigs(&src);
        for (name, line, col, len) in collect_fn_defs(&src) {
            let signature = sigs
                .iter()
                .find(|(sig_name, _, _)| sig_name == &name)
                .map(|(_, label, _)| label.clone());
            index.symbols.push(LspSymbol {
                uri: uri.clone(),
                name,
                kind: 12,
                line,
                col,
                len,
                signature,
            });
        }
        for (name, line, col, len) in collect_struct_defs(&src) {
            index.symbols.push(LspSymbol {
                uri: uri.clone(),
                name,
                kind: 23,
                line,
                col,
                len,
                signature: None,
            });
        }
    }
    index
}

fn import_reaches(index: &WorkspaceIndex, from: &str, wanted: &str) -> bool {
    if from == wanted {
        return true;
    }
    let mut seen = HashSet::new();
    let mut stack = vec![from.to_string()];
    while let Some(uri) = stack.pop() {
        if !seen.insert(uri.clone()) {
            continue;
        }
        for edge in index.imports.get(&uri).into_iter().flatten() {
            if edge.target_uri == wanted {
                return true;
            }
            stack.push(edge.target_uri.clone());
        }
    }
    false
}

fn qualifier_at(src: &str, line: u32, character: u32) -> Option<String> {
    let line_text = src.lines().nth(line as usize)?;
    let word_end = character as usize;
    if word_end > line_text.len() {
        return None;
    }
    let mut start = word_end;
    while start > 0 && line_text.as_bytes()[start - 1].is_ascii_alphanumeric()
        || start > 0 && line_text.as_bytes()[start - 1] == b'_'
    {
        start -= 1;
    }
    let before = line_text[..start].trim_end();
    if !before.ends_with('.') {
        return None;
    }
    let before_dot = before[..before.len() - 1].trim_end();
    let mut qstart = before_dot.len();
    while qstart > 0
        && (before_dot.as_bytes()[qstart - 1].is_ascii_alphanumeric()
            || before_dot.as_bytes()[qstart - 1] == b'_')
    {
        qstart -= 1;
    }
    let qualifier = &before_dot[qstart..];
    (!qualifier.is_empty()).then(|| qualifier.to_string())
}

fn symbol_for_position(
    index: &WorkspaceIndex,
    uri: &str,
    src: &str,
    line: u32,
    character: u32,
) -> Option<LspSymbol> {
    let uri = normalize_uri(uri);
    let word = word_at(src, line, character);
    if word.is_empty() {
        return None;
    }
    let qualifier = qualifier_at(src, line, character);
    if qualifier.is_none()
        && offset_at(src, line, character)
            .is_some_and(|offset| offset_in_ranges(offset, &function_shadow_ranges(src, &word)))
    {
        return None;
    }
    let local = index
        .symbols
        .iter()
        .find(|s| s.uri == uri && s.name == word)
        .cloned();
    if qualifier.is_none() {
        if local.is_some() {
            return local;
        }
    }

    let mut candidates = index
        .symbols
        .iter()
        .filter(|s| s.name == word && s.uri != uri)
        .filter(|s| import_reaches(index, &uri, &s.uri));
    if let Some(q) = qualifier {
        return candidates
            .find(|candidate| {
                let mut stack = vec![(uri.to_string(), q.clone())];
                let mut seen = HashSet::new();
                while let Some((from, alias)) = stack.pop() {
                    if !seen.insert((from.clone(), alias.clone())) {
                        continue;
                    }
                    for edge in index.imports.get(&from).into_iter().flatten() {
                        let edge_alias = edge
                            .alias
                            .clone()
                            .or_else(|| import_default_alias(&edge.target_uri));
                        if edge_alias.as_deref() != Some(alias.as_str()) {
                            continue;
                        }
                        if edge.target_uri == candidate.uri {
                            return true;
                        }
                        stack.push((edge.target_uri.clone(), alias.clone()));
                    }
                }
                false
            })
            .cloned();
    }
    candidates.next().cloned().or(local)
}

fn token_offset(src: &str, token: &crate::lexer::Token) -> Option<usize> {
    offset_at(
        src,
        token.line.saturating_sub(1) as u32,
        token.col.saturating_sub(1) as u32,
    )
}

fn function_shadow_ranges(src: &str, needle: &str) -> Vec<(usize, usize)> {
    let Ok(tokens) = Lexer::new(src).tokenize() else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    let mut i = 0usize;
    while i < tokens.len() {
        if !matches!(tokens[i].kind, TokenKind::Fn | TokenKind::Func) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < tokens.len() && !matches!(tokens[j].kind, TokenKind::LParen | TokenKind::LBrace) {
            j += 1;
        }
        let mut shadowed = false;
        if j < tokens.len() && matches!(tokens[j].kind, TokenKind::LParen) {
            let mut depth = 1i32;
            let mut k = j + 1;
            while k < tokens.len() && depth > 0 {
                match tokens[k].kind {
                    TokenKind::LParen => depth += 1,
                    TokenKind::RParen => depth -= 1,
                    TokenKind::Ident(ref name) if depth == 1 && name == needle => {
                        shadowed = true;
                    }
                    _ => {}
                }
                k += 1;
            }
            j = k;
        }
        while j < tokens.len() && !matches!(tokens[j].kind, TokenKind::LBrace) {
            j += 1;
        }
        if j >= tokens.len() {
            break;
        }
        let open = j;
        let mut depth = 1i32;
        j += 1;
        while j < tokens.len() && depth > 0 {
            match tokens[j].kind {
                TokenKind::LBrace => depth += 1,
                TokenKind::RBrace => depth -= 1,
                TokenKind::Let | TokenKind::Var | TokenKind::Const => {
                    if let Some(TokenKind::Ident(name)) = tokens.get(j + 1).map(|t| &t.kind) {
                        if name == needle {
                            shadowed = true;
                        }
                    }
                }
                TokenKind::For => {
                    let mut k = j + 1;
                    while k < tokens.len()
                        && !matches!(tokens[k].kind, TokenKind::In | TokenKind::LBrace)
                    {
                        if let TokenKind::Ident(name) = &tokens[k].kind {
                            if name == needle {
                                shadowed = true;
                            }
                        }
                        k += 1;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        if shadowed {
            if let (Some(start), Some(end)) = (
                token_offset(src, &tokens[open]),
                tokens
                    .get(j.saturating_sub(1))
                    .and_then(|t| token_offset(src, t)),
            ) {
                ranges.push((start, end));
            }
        }
        i = j;
    }
    ranges
}

fn offset_in_ranges(offset: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|(start, end)| offset >= *start && offset <= *end)
}

fn collect_symbol_occurrences(src: &str, name: &str, kind: u32) -> Vec<(u32, u32, u32)> {
    let Ok(tokens) = Lexer::new(src).tokenize() else {
        return Vec::new();
    };
    let shadowed = function_shadow_ranges(src, name);
    let mut out = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        if !matches!(&token.kind, TokenKind::Ident(found) if found == name) {
            continue;
        }
        let Some(offset) = token_offset(src, token) else {
            continue;
        };
        if offset_in_ranges(offset, &shadowed) {
            continue;
        }
        let previous = i.checked_sub(1).and_then(|n| tokens.get(n));
        let next = tokens.get(i + 1);
        let is_definition = previous.is_some_and(|t| {
            matches!(
                t.kind,
                TokenKind::Fn | TokenKind::Func if kind == 12
            ) || matches!(t.kind, TokenKind::Struct if kind == 23)
        });
        let is_reference = if kind == 12 {
            next.is_some_and(|t| matches!(t.kind, TokenKind::LParen))
                || previous.is_some_and(|t| matches!(t.kind, TokenKind::Dot))
                || previous.is_some_and(|t| {
                    matches!(
                        t.kind,
                        TokenKind::Assign
                            | TokenKind::Comma
                            | TokenKind::Return
                            | TokenKind::LParen
                    )
                })
        } else {
            next.is_some_and(|t| matches!(t.kind, TokenKind::LBrace | TokenKind::LBracket))
                || previous.is_some_and(|t| {
                    matches!(
                        t.kind,
                        TokenKind::Colon | TokenKind::Comma | TokenKind::LBracket
                    )
                })
        };
        if is_definition || is_reference {
            out.push((
                token.line.saturating_sub(1) as u32,
                token.col.saturating_sub(1) as u32,
                name.len() as u32,
            ));
        }
    }
    out
}

fn occurrence_matches_import(
    index: &WorkspaceIndex,
    from_uri: &str,
    target_uri: &str,
    src: &str,
    line: u32,
    col: u32,
) -> bool {
    if from_uri == target_uri {
        return true;
    }
    let qualifier = qualifier_at(src, line, col + 1);
    if let Some(q) = qualifier {
        let mut stack = vec![(from_uri.to_string(), q)];
        let mut seen = HashSet::new();
        while let Some((uri, alias)) = stack.pop() {
            if !seen.insert((uri.clone(), alias.clone())) {
                continue;
            }
            for edge in index.imports.get(&uri).into_iter().flatten() {
                let edge_alias = edge
                    .alias
                    .clone()
                    .or_else(|| import_default_alias(&edge.target_uri));
                if edge_alias.as_deref() != Some(alias.as_str()) {
                    continue;
                }
                if edge.target_uri == target_uri {
                    return true;
                }
                stack.push((edge.target_uri.clone(), alias.clone()));
            }
        }
        false
    } else {
        import_reaches(index, from_uri, target_uri)
    }
}

fn symbol_locations(index: &WorkspaceIndex, symbol: &LspSymbol) -> Vec<(String, u32, u32, u32)> {
    let mut locations = Vec::new();
    for (uri, src) in &index.docs {
        if !import_reaches(index, uri, &symbol.uri) {
            continue;
        }
        for (line, col, len) in collect_symbol_occurrences(src, &symbol.name, symbol.kind) {
            if uri != &symbol.uri
                && !occurrence_matches_import(index, uri, &symbol.uri, src, line, col)
            {
                continue;
            }
            locations.push((uri.clone(), line, col, len));
        }
    }
    locations
}

fn apply_occurrence_rename(src: &str, occurrences: &[(u32, u32, u32)], new_name: &str) -> String {
    let mut edits = Vec::new();
    for (line, col, len) in occurrences {
        if let Some(start) = offset_at(src, *line, *col) {
            edits.push((start, start + *len as usize));
        }
    }
    edits.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out = src.to_string();
    for (start, end) in edits {
        out.replace_range(start..end, new_name);
    }
    out
}

fn full_doc_end(old_src: &str) -> (u32, u32) {
    let line = old_src.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let character = old_src
        .rsplit('\n')
        .next()
        .map(|last_line| last_line.chars().count() as u32)
        .unwrap_or(0);
    (line, character)
}

fn full_doc_edit(uri: &str, old_src: &str, new_src: &str) -> String {
    let (end_line, end_character) = full_doc_end(old_src);
    format!(
        r#"{{"{}":[{{"range":{{"start":{{"line":0,"character":0}},"end":{{"line":{},"character":{}}}}},"newText":"{}"}}]}}"#,
        json_escape(uri),
        end_line,
        end_character,
        json_escape(new_src)
    )
}

/// textDocument/prepareRename — resolve a top-level symbol across the import graph.
fn prepare_rename(
    index: &WorkspaceIndex,
    uri: &str,
    src: &str,
    line: u32,
    character: u32,
) -> String {
    let uri = normalize_uri(uri);
    let Some(symbol) = symbol_for_position(index, &uri, src, line, character) else {
        return "null".into();
    };
    for (loc_uri, dl, dc, len) in symbol_locations(index, &symbol) {
        if loc_uri == uri && dl == line && character >= dc && character <= dc + len {
            return format!(
                r#"{{"range":{{"start":{{"line":{dl},"character":{dc}}},"end":{{"line":{dl},"character":{}}}}},"placeholder":"{}"}}"#,
                dc + len,
                json_escape(&symbol.name)
            );
        }
    }
    "null".into()
}

fn valid_rename_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.chars().next().is_some_and(|c| !c.is_ascii_digit())
        && !KEYWORDS.contains(&name)
}

/// textDocument/rename — safe workspace edit for a top-level function or struct.
fn rename_symbol(
    index: &WorkspaceIndex,
    uri: &str,
    src: &str,
    line: u32,
    character: u32,
    new_name: &str,
) -> String {
    let uri = normalize_uri(uri);
    if !valid_rename_name(new_name) {
        return "null".into();
    }
    let Some(symbol) = symbol_for_position(index, &uri, src, line, character) else {
        return "null".into();
    };
    let locations = symbol_locations(index, &symbol);
    if !locations.iter().any(|(loc_uri, dl, dc, len)| {
        *loc_uri == uri && *dl == line && character >= *dc && character <= *dc + *len
    }) {
        return "null".into();
    }

    let mut by_uri: HashMap<String, Vec<(u32, u32, u32)>> = HashMap::new();
    for (loc_uri, dl, dc, len) in locations {
        by_uri.entry(loc_uri).or_default().push((dl, dc, len));
    }
    let mut edits = Vec::new();
    let mut uris: Vec<String> = by_uri.keys().cloned().collect();
    uris.sort();
    for loc_uri in uris {
        let Some(old_src) = index.docs.get(&loc_uri) else {
            continue;
        };
        let new_src = apply_occurrence_rename(old_src, &by_uri[&loc_uri], new_name);
        edits.push(full_doc_edit(&loc_uri, old_src, &new_src));
    }
    let inner = edits
        .iter()
        .map(|p| p.trim_start_matches('{').trim_end_matches('}').to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!(r#"{{"changes":{{{inner}}}}}"#)
}

/// textDocument/references — all reachable project references, including the declaration.
fn find_references(
    index: &WorkspaceIndex,
    uri: &str,
    src: &str,
    line: u32,
    character: u32,
) -> String {
    let Some(symbol) = symbol_for_position(index, uri, src, line, character) else {
        return "[]".into();
    };
    let mut locations = symbol_locations(index, &symbol);
    locations.sort();
    let locs = locations
        .into_iter()
        .map(|(loc_uri, dl, dc, len)| location_json(&loc_uri, dl, dc, len))
        .collect::<Vec<_>>();
    format!("[{}]", locs.join(","))
}

fn word_at(src: &str, line: u32, character: u32) -> String {
    let mut cur_line = 0u32;
    let mut cur_col = 0u32;
    let mut idx = src.len();
    for (i, ch) in src.char_indices() {
        if cur_line == line && cur_col == character {
            idx = i;
            break;
        }
        if ch == '\n' {
            cur_line += 1;
            cur_col = 0;
        } else {
            cur_col += 1;
        }
    }
    // expand to full identifier around idx
    let bytes = src.as_bytes();
    let mut start = idx.min(bytes.len());
    let mut end = start;
    while start > 0 {
        let c = bytes[start - 1] as char;
        if c.is_ascii_alphanumeric() || c == '_' {
            start -= 1;
        } else {
            break;
        }
    }
    while end < bytes.len() {
        let c = bytes[end] as char;
        if c.is_ascii_alphanumeric() || c == '_' {
            end += 1;
        } else {
            break;
        }
    }
    src[start..end].to_string()
}

fn goto_definition_index(
    index: &WorkspaceIndex,
    uri: &str,
    src: &str,
    line: u32,
    character: u32,
) -> Option<String> {
    let symbol = symbol_for_position(index, uri, src, line, character)?;
    Some(location_json(
        &symbol.uri,
        symbol.line,
        symbol.col,
        symbol.len,
    ))
}

fn location_json(uri: &str, dl: u32, dc: u32, len: u32) -> String {
    format!(
        r#"{{"uri":"{}","range":{{"start":{{"line":{dl},"character":{dc}}},"end":{{"line":{dl},"character":{}}}}}}}"#,
        json_escape(uri),
        dc + len
    )
}

/// Parse `import "./x.mko"` and `import "./x.mko" as foo` from source tokens.
fn collect_imports(src: &str) -> Vec<(String, Option<String>)> {
    let Ok(tokens) = Lexer::new(src).tokenize() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let is_import_kw = matches!(tokens[i].kind, TokenKind::Import)
            || matches!(tokens[i].kind, TokenKind::Ident(ref s) if s == "pull");
        if is_import_kw {
            if i + 1 < tokens.len() {
                if let TokenKind::String(ref path) = tokens[i + 1].kind {
                    let mut alias = None;
                    if i + 3 < tokens.len() && matches!(tokens[i + 2].kind, TokenKind::As) {
                        if let TokenKind::Ident(ref a) = tokens[i + 3].kind {
                            alias = Some(a.clone());
                        }
                    }
                    out.push((path.clone(), alias));
                }
            }
        }
        i += 1;
    }
    out
}

fn publish_diagnostics(stdout: &mut impl Write, uri: &str, src: &str) -> io::Result<()> {
    let diags = diagnose(src);
    let mut arr = String::from("[");
    for (i, (sl, sc, el, ec, msg)) in diags.iter().enumerate() {
        if i > 0 {
            arr.push(',');
        }
        arr.push_str(&format!(
            r#"{{"range":{{"start":{{"line":{sl},"character":{sc}}},"end":{{"line":{el},"character":{ec}}}}},"severity":1,"source":"mako","message":"{}"}}"#,
            json_escape(msg)
        ));
    }
    arr.push(']');
    let body = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{{"uri":"{}","diagnostics":{arr}}}}}"#,
        json_escape(uri)
    );
    write_message(stdout, &body)
}

fn completion_items(prefix: &str) -> String {
    let mut items = Vec::new();
    for kw in KEYWORDS {
        if prefix.is_empty() || kw.starts_with(prefix) {
            items.push(format!(
                r#"{{"label":"{kw}","kind":14,"detail":"keyword"}}"#
            ));
        }
    }
    // builtins seed
    for b in [
        "print",
        "print_int",
        "assert",
        "assert_eq",
        "len",
        "append",
        "regex_match",
        "regex_find",
        "regex_capture",
        "sleep_ms",
        "now_ms",
        "now_ns",
        "exit",
    ] {
        if prefix.is_empty() || b.starts_with(prefix) {
            items.push(format!(r#"{{"label":"{b}","kind":3,"detail":"builtin"}}"#));
        }
    }
    format!("[{}]", items.join(","))
}

fn code_actions(has_diagnostics: bool) -> String {
    let mut actions = Vec::new();
    if has_diagnostics {
        actions.push(
            r#"{"title":"Mako: Check current file","kind":"quickfix","command":{"title":"Mako: Check current file","command":"mako.check"}}"#
                .to_string(),
        );
    }
    actions.push(
        r#"{"title":"Mako: Format current file","kind":"source.fixAll.mako","command":{"title":"Mako: Format current file","command":"mako.format"}}"#
            .to_string(),
    );
    actions.push(
        r#"{"title":"Mako: Run tests","kind":"source","command":{"title":"Mako: Run tests","command":"mako.test"}}"#
            .to_string(),
    );
    format!("[{}]", actions.join(","))
}

fn hover(index: &WorkspaceIndex, uri: &str, src: &str, line: u32, character: u32) -> String {
    let Some(symbol) = symbol_for_position(index, uri, src, line, character) else {
        return "null".into();
    };
    let detail = match (&symbol.signature, symbol.kind) {
        (Some(signature), 12) => format!("`{signature}`"),
        (_, 23) => format!("`struct {}`", symbol.name),
        _ => return "null".into(),
    };
    let value = format!("**mako**\n\n{detail}");
    format!(
        r#"{{"contents":{{"kind":"markdown","value":"{}"}},"range":{{"start":{{"line":{},"character":{}}},"end":{{"line":{},"character":{}}}}}}}"#,
        json_escape(&value),
        symbol.line,
        symbol.col,
        symbol.line,
        symbol.col + symbol.len
    )
}

fn word_prefix_at(src: &str, line: u32, character: u32) -> String {
    let mut cur_line = 0u32;
    let mut cur_col = 0u32;
    let mut idx = src.len();
    for (i, ch) in src.char_indices() {
        if cur_line == line && cur_col == character {
            idx = i;
            break;
        }
        if ch == '\n' {
            cur_line += 1;
            cur_col = 0;
        } else {
            cur_col += 1;
        }
    }
    let before = &src[..idx.min(src.len())];
    let mut start = before.len();
    for (i, ch) in before.char_indices().rev() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            start = i;
        } else {
            break;
        }
    }
    before[start..].to_string()
}

fn signature_return_type(label: &str) -> Option<String> {
    label
        .split_once("->")
        .map(|(_, ret)| ret.trim().to_string())
        .filter(|ret| !ret.is_empty())
}

fn function_return_type(index: &WorkspaceIndex, uri: &str, name: &str) -> Option<String> {
    let uri = normalize_uri(uri);
    index
        .symbols
        .iter()
        .find(|symbol| {
            symbol.kind == 12
                && symbol.name == name
                && (symbol.uri == uri || import_reaches(index, &uri, &symbol.uri))
        })
        .and_then(|symbol| symbol.signature.as_deref().and_then(signature_return_type))
}

fn infer_expression_type(
    tokens: &[crate::lexer::Token],
    start: usize,
    index: &WorkspaceIndex,
    uri: &str,
    inferred: &HashMap<String, String>,
) -> Option<String> {
    let token = tokens.get(start)?;
    match &token.kind {
        TokenKind::Int(_) => Some("int".into()),
        TokenKind::Float(_) => Some("float".into()),
        TokenKind::String(_) | TokenKind::FString(_) => Some("string".into()),
        TokenKind::True | TokenKind::False => Some("bool".into()),
        TokenKind::Minus => match tokens.get(start + 1).map(|t| &t.kind) {
            Some(TokenKind::Int(_)) => Some("int".into()),
            Some(TokenKind::Float(_)) => Some("float".into()),
            _ => None,
        },
        TokenKind::Ident(name) => {
            if let Some(known) = inferred.get(name) {
                return Some(known.clone());
            }
            if matches!(
                tokens.get(start + 1).map(|t| &t.kind),
                Some(TokenKind::LBrace)
            ) {
                if index
                    .symbols
                    .iter()
                    .any(|s| s.kind == 23 && s.name == *name)
                {
                    return Some(name.clone());
                }
            }
            let call_name = if matches!(
                tokens.get(start + 1).map(|t| &t.kind),
                Some(TokenKind::LParen)
            ) {
                Some(name.as_str())
            } else if matches!(tokens.get(start + 1).map(|t| &t.kind), Some(TokenKind::Dot)) {
                match tokens.get(start + 2).map(|t| &t.kind) {
                    Some(TokenKind::Ident(function))
                        if matches!(
                            tokens.get(start + 3).map(|t| &t.kind),
                            Some(TokenKind::LParen)
                        ) =>
                    {
                        Some(function.as_str())
                    }
                    _ => None,
                }
            } else {
                None
            };
            if let Some(call_name) = call_name {
                return function_return_type(index, uri, call_name);
            }
            None
        }
        _ => None,
    }
}

fn position_in_range(line: u32, character: u32, range: (u32, u32, u32, u32)) -> bool {
    let (start_line, start_char, end_line, end_char) = range;
    (line > start_line || (line == start_line && character >= start_char))
        && (line < end_line || (line == end_line && character <= end_char))
}

fn inlay_hints(
    index: &WorkspaceIndex,
    uri: &str,
    src: &str,
    range: (u32, u32, u32, u32),
) -> String {
    let Ok(tokens) = Lexer::new(src).tokenize() else {
        return "[]".into();
    };
    let mut inferred = HashMap::new();
    let mut hints = Vec::new();
    let mut i = 0usize;
    while i < tokens.len() {
        if !matches!(tokens[i].kind, TokenKind::Let | TokenKind::Var) {
            i += 1;
            continue;
        }
        let mut name_at = i + 1;
        while matches!(
            tokens.get(name_at).map(|t| &t.kind),
            Some(TokenKind::Mut | TokenKind::Hold | TokenKind::Share)
        ) {
            name_at += 1;
        }
        let Some(crate::lexer::Token {
            kind: TokenKind::Ident(name),
            line,
            col,
        }) = tokens.get(name_at)
        else {
            i += 1;
            continue;
        };
        let name = name.clone();
        let name_line = line.saturating_sub(1) as u32;
        let name_col = col.saturating_sub(1) as u32;
        let Some(next) = tokens.get(name_at + 1) else {
            i = name_at + 1;
            continue;
        };
        if matches!(next.kind, TokenKind::Colon) {
            i = name_at + 1;
            continue;
        }
        if !matches!(next.kind, TokenKind::Assign) {
            i = name_at + 1;
            continue;
        }
        let Some(ty) = infer_expression_type(&tokens, name_at + 2, index, uri, &inferred) else {
            i = name_at + 2;
            continue;
        };
        inferred.insert(name.clone(), ty.clone());
        let hint_character = name_col + name.len() as u32;
        if position_in_range(name_line, hint_character, range) {
            hints.push(format!(
                r#"{{"position":{{"line":{name_line},"character":{hint_character}}},"label":": {ty}","kind":1,"paddingLeft":true}}"#
            ));
        }
        i = name_at + 2;
    }
    format!("[{}]", hints.join(","))
}

fn parse_position(msg: &str) -> (u32, u32) {
    let line = msg
        .find("\"line\"")
        .and_then(|i| {
            let after = &msg[i + 6..];
            let colon = after.find(':')?;
            let rest = after[colon + 1..].trim_start();
            rest.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        })
        .unwrap_or(0);
    let character = msg
        .find("\"character\"")
        .and_then(|i| {
            let after = &msg[i + 11..];
            let colon = after.find(':')?;
            let rest = after[colon + 1..].trim_start();
            rest.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        })
        .unwrap_or(0);
    (line, character)
}

fn parse_range(msg: &str) -> (u32, u32, u32, u32) {
    let start = msg.find("\"start\"").unwrap_or(0);
    let end = msg.find("\"end\"").unwrap_or(start);
    let (start_line, start_char) = parse_position(&msg[start..]);
    let (end_line, end_char) = parse_position(&msg[end..]);
    (start_line, start_char, end_line, end_char)
}

fn extract_text_document_text(msg: &str) -> Option<String> {
    // "text":"...."  (may contain escapes)
    let key = "\"text\"";
    let i = msg.find(key)?;
    let after = &msg[i + key.len()..];
    let colon = after.find(':')?;
    let rest = after[colon + 1..].trim_start();
    if !rest.starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut chars = rest[1..].chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => break,
            }
        } else if c == '"' {
            break;
        } else {
            out.push(c);
        }
    }
    Some(out)
}

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let u = json_unescape(uri);
    let path = u.strip_prefix("file://")?;
    Some(PathBuf::from(path))
}

/// Run LSP on stdio until exit. Returns Ok(()) on clean shutdown.
pub fn run_stdio() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mut shutdown = false;
    let mut docs: HashMap<String, String> = HashMap::new();

    while let Some(msg) = read_message(&mut stdin)? {
        let method = json_get_str(&msg, "method").unwrap_or("");
        let id = json_get_id(&msg);

        match method {
            "initialize" => {
                let result = r#"{"capabilities":{"hoverProvider":true,"completionProvider":{"triggerCharacters":["."]},"textDocumentSync":{"openClose":true,"change":1},"definitionProvider":true,"documentSymbolProvider":true,"workspaceSymbolProvider":true,"referencesProvider":true,"codeActionProvider":true,"signatureHelpProvider":{"triggerCharacters":["(",","]},"renameProvider":{"prepareProvider":true},"inlayHintProvider":{"resolveProvider":false}},"serverInfo":{"name":"mako-lsp","version":"0.5.0"}}"#;
                let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#);
                write_message(&mut stdout, &body)?;
            }
            "initialized" => {}
            "textDocument/didOpen" => {
                if let Some(uri) = json_get_str(&msg, "uri") {
                    let text = extract_text_document_text(&msg).unwrap_or_default();
                    // Prefer disk if file:// and text empty
                    let src = if text.is_empty() {
                        uri_to_path(uri)
                            .and_then(|p| std::fs::read_to_string(p).ok())
                            .unwrap_or_default()
                    } else {
                        text
                    };
                    docs.insert(uri.to_string(), src.clone());
                    publish_diagnostics(&mut stdout, uri, &src)?;
                }
            }
            "textDocument/didChange" => {
                if let Some(uri) = json_get_str(&msg, "uri") {
                    // Full-document sync: last contentChanges[].text
                    let text = extract_text_document_text(&msg).unwrap_or_else(|| {
                        // Prefer the last "text" occurrence inside contentChanges
                        extract_text_document_text(&msg).unwrap_or_default()
                    });
                    // If extractor got the uri's unrelated field, fall back to scanning all texts —
                    // our extractor finds first "text"; for didChange the change text is typically last.
                    let text = {
                        let mut last = text;
                        let mut search = msg.as_str();
                        while let Some(i) = search.find("\"text\"") {
                            let slice = &search[i..];
                            if let Some(t) = extract_text_document_text(slice) {
                                last = t;
                            }
                            search = &search[i + 6..];
                        }
                        last
                    };
                    docs.insert(uri.to_string(), text.clone());
                    publish_diagnostics(&mut stdout, uri, &text)?;
                }
            }
            "textDocument/didClose" => {
                if let Some(uri) = json_get_str(&msg, "uri") {
                    docs.remove(uri);
                    let body = format!(
                        r#"{{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{{"uri":"{}","diagnostics":[]}}}}"#,
                        json_escape(uri)
                    );
                    write_message(&mut stdout, &body)?;
                }
            }
            "textDocument/hover" => {
                let uri = json_get_str(&msg, "uri").unwrap_or("");
                let (line, character) = parse_position(&msg);
                let src = source_for_uri(uri, &docs).unwrap_or_default();
                let index = build_workspace_index(&docs);
                let result = hover(&index, uri, &src, line, character);
                let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#);
                write_message(&mut stdout, &body)?;
            }
            "textDocument/completion" => {
                let uri = json_get_str(&msg, "uri").unwrap_or("");
                let (line, character) = parse_position(&msg);
                let src = docs.get(uri).map(|s| s.as_str()).unwrap_or("");
                let prefix = word_prefix_at(src, line, character);
                let items = completion_items(&prefix);
                let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{items}}}"#);
                write_message(&mut stdout, &body)?;
            }
            "textDocument/codeAction" => {
                let uri = json_get_str(&msg, "uri").unwrap_or("");
                let src = docs.get(uri).map(|s| s.as_str()).unwrap_or("");
                let has_diagnostics = !diagnose(src).is_empty();
                let result = code_actions(has_diagnostics);
                let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#);
                write_message(&mut stdout, &body)?;
            }
            "textDocument/definition" => {
                let uri = json_get_str(&msg, "uri").unwrap_or("");
                let (line, character) = parse_position(&msg);
                let src = source_for_uri(uri, &docs).unwrap_or_default();
                let index = build_workspace_index(&docs);
                let result = match goto_definition_index(&index, uri, &src, line, character) {
                    Some(loc) => loc,
                    None => "null".into(),
                };
                let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#);
                write_message(&mut stdout, &body)?;
            }
            "textDocument/documentSymbol" => {
                let uri = json_get_str(&msg, "uri").unwrap_or("");
                let src = docs.get(uri).map(|s| s.as_str()).unwrap_or("");
                let result = document_symbols(src);
                let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#);
                write_message(&mut stdout, &body)?;
            }
            "workspace/symbol" => {
                let query = json_get_str(&msg, "query").unwrap_or("");
                let index = build_workspace_index(&docs);
                let result = workspace_symbols_from_index(&index, query);
                let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#);
                write_message(&mut stdout, &body)?;
            }
            "textDocument/references" => {
                let uri = json_get_str(&msg, "uri").unwrap_or("");
                let (line, character) = parse_position(&msg);
                let src = source_for_uri(uri, &docs).unwrap_or_default();
                let index = build_workspace_index(&docs);
                let result = find_references(&index, uri, &src, line, character);
                let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#);
                write_message(&mut stdout, &body)?;
            }
            "textDocument/signatureHelp" => {
                let uri = json_get_str(&msg, "uri").unwrap_or("");
                let (line, character) = parse_position(&msg);
                let src = source_for_uri(uri, &docs).unwrap_or_default();
                let index = build_workspace_index(&docs);
                let result = signature_help(&index, uri, &src, line, character);
                let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#);
                write_message(&mut stdout, &body)?;
            }
            "textDocument/inlayHint" => {
                let uri = json_get_str(&msg, "uri").unwrap_or("");
                let src = source_for_uri(uri, &docs).unwrap_or_default();
                let range = parse_range(&msg);
                let index = build_workspace_index(&docs);
                let result = inlay_hints(&index, uri, &src, range);
                let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#);
                write_message(&mut stdout, &body)?;
            }
            "textDocument/prepareRename" => {
                let uri = json_get_str(&msg, "uri").unwrap_or("");
                let (line, character) = parse_position(&msg);
                let src = source_for_uri(uri, &docs).unwrap_or_default();
                let index = build_workspace_index(&docs);
                let result = prepare_rename(&index, uri, &src, line, character);
                let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#);
                write_message(&mut stdout, &body)?;
            }
            "textDocument/rename" => {
                let uri = json_get_str(&msg, "uri").unwrap_or("");
                let (line, character) = parse_position(&msg);
                let new_name = json_get_str(&msg, "newName").unwrap_or("");
                let src = source_for_uri(uri, &docs).unwrap_or_default();
                let index = build_workspace_index(&docs);
                let result = rename_symbol(&index, uri, &src, line, character, new_name);
                let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#);
                write_message(&mut stdout, &body)?;
            }
            "shutdown" => {
                shutdown = true;
                let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":null}}"#);
                write_message(&mut stdout, &body)?;
            }
            "exit" => {
                break;
            }
            "" if msg.contains("\"id\"") => {
                let body = format!(
                    r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32601,"message":"Method not found"}}}}"#
                );
                write_message(&mut stdout, &body)?;
            }
            _ => {}
        }
        if shutdown && method == "exit" {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn position_of(src: &str, needle: &str) -> (u32, u32) {
        let offset = src.find(needle).expect("needle exists");
        let before = &src[..offset];
        let line = before.bytes().filter(|b| *b == b'\n').count() as u32;
        let character = before.rsplit('\n').next().unwrap_or_default().len() as u32;
        (line, character)
    }

    fn fixture() -> (PathBuf, String, String, String, WorkspaceIndex) {
        let root = std::env::temp_dir().join(format!(
            "mako-lsp-test-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("worker")
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create fixture");
        let lib_path = root.join("lib.mko");
        let app_path = root.join("app.mko");
        let lib = "fn add(a: int, b: int) -> int {\n    return a + b\n}\n\nfn shadow() -> int {\n    let add = 9\n    return add\n}\n";
        let app = "pull \"./lib.mko\" as lib\n\nfn make() -> int {\n    return 7\n}\n\nfn main() {\n    let value = lib.add(1, 2)\n    let again = lib.add(value, 3)\n    let made = make()\n    let text = \"ok\"\n    let copied = made\n}\n\nfn local_shadow() {\n    let add = 100\n    let result = add\n}\n";
        fs::write(&lib_path, lib).expect("write lib");
        fs::write(&app_path, app).expect("write app");
        let lib_uri = path_to_uri(&lib_path);
        let app_uri = path_to_uri(&app_path);
        let mut open = HashMap::new();
        open.insert(lib_uri.clone(), lib.to_string());
        open.insert(app_uri.clone(), app.to_string());
        let index = build_workspace_index(&open);
        (root, lib_uri, app_uri, app.to_string(), index)
    }

    #[test]
    fn workspace_index_resolves_imported_definition_and_references() {
        let (root, lib_uri, app_uri, app, index) = fixture();
        let (line, character) = position_of(&app, "lib.add(1");
        let add_col = character + 4;
        let definition =
            goto_definition_index(&index, &app_uri, &app, line, add_col).expect("definition");
        assert!(definition.contains(&lib_uri));
        assert!(definition.contains("\"line\":0"));
        let hover_result = hover(&index, &app_uri, &app, line, add_col);
        assert!(hover_result.contains("add(a: int, b: int) -> int"));

        let references = find_references(&index, &app_uri, &app, line, add_col);
        assert_eq!(references.matches("\"uri\"").count(), 3);
        assert!(references.contains(&lib_uri));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_index_normalizes_noncanonical_file_uris() {
        let (root, _lib_uri, _app_uri, app, _) = fixture();
        let lib_path = root.join("lib.mko");
        let app_path = root.join("app.mko");
        let lib_uri = format!("file://{}", lib_path.display());
        let app_uri = format!("file://{}", app_path.display());
        let lib = fs::read_to_string(&lib_path).expect("read lib");
        let mut open = HashMap::new();
        open.insert(lib_uri.clone(), lib);
        open.insert(app_uri.clone(), app.clone());
        let index = build_workspace_index(&open);
        let (line, character) = position_of(&app, "lib.add(1");
        let add_col = character + 4;
        let definition = goto_definition_index(&index, &app_uri, &app, line, add_col)
            .expect("definition with raw file URI");
        assert!(definition.contains(&path_to_uri(&lib_path)));
        let edit = rename_symbol(&index, &app_uri, &app, line, add_col, "sum");
        assert!(edit.contains("sum"));
        assert!(edit.contains(&path_to_uri(&app_path)));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rename_is_project_wide_and_does_not_touch_shadowed_locals() {
        let (root, lib_uri, app_uri, app, index) = fixture();
        let (line, character) = position_of(index.docs.get(&lib_uri).unwrap(), "add");
        let edit = rename_symbol(
            &index,
            &lib_uri,
            index.docs.get(&lib_uri).unwrap(),
            line,
            character,
            "sum",
        );
        assert!(edit.contains("\"changes\""));
        assert!(edit.contains(&lib_uri));
        assert!(edit.contains(&app_uri));
        assert!(edit.contains("sum"));
        assert!(edit.contains("let add = 100"));
        assert_eq!(
            rename_symbol(
                &index,
                &lib_uri,
                index.docs.get(&lib_uri).unwrap(),
                line,
                character,
                "fn"
            ),
            "null"
        );
        let (shadow_line, shadow_start) = position_of(&app, "let add = 100");
        let shadow_col = shadow_start + 4;
        assert_eq!(
            goto_definition_index(&index, &app_uri, &app, shadow_line, shadow_col),
            None
        );
        assert_eq!(
            prepare_rename(&index, &app_uri, &app, shadow_line, shadow_col),
            "null"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn signature_help_contains_parameter_metadata_and_active_index() {
        let (root, _lib_uri, app_uri, app, index) = fixture();
        let (line, start_character) = position_of(&app, "lib.add(1,");
        let character = start_character + "lib.add(1,".len() as u32;
        let result = signature_help(&index, &app_uri, &app, line, character);
        assert!(result.contains("\"activeParameter\":1"));
        assert_eq!(result.matches("\"label\"").count(), 3);
        assert!(result.contains("a: int"));
        assert!(result.contains("b: int"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn inlay_hints_report_only_confident_inferred_types() {
        let (root, _lib_uri, app_uri, app, index) = fixture();
        let hints = inlay_hints(&index, &app_uri, &app, (0, 0, 99, 0));
        assert!(hints.contains("\"label\":\": int\""));
        assert!(hints.contains("\"label\":\": string\""));
        assert!(!hints.contains("\": ?\""));
        assert_eq!(hints.matches("\"position\"").count(), 7);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn empty_or_invalid_lsp_requests_return_null_without_panicking() {
        assert_eq!(
            signature_help(&WorkspaceIndex::default(), "", "", 0, 0),
            "null"
        );
        assert_eq!(
            prepare_rename(&WorkspaceIndex::default(), "", "", 0, 0),
            "null"
        );
        assert_eq!(
            rename_symbol(&WorkspaceIndex::default(), "", "", 0, 0, "bad-name"),
            "null"
        );
    }

    #[test]
    fn full_document_edits_end_at_the_actual_text_position() {
        assert_eq!(full_doc_end("abc\nxyz"), (1, 3));
        assert_eq!(full_doc_end("abc\n"), (1, 0));
        let edit = full_doc_edit("file:///app.mko", "abc\nxyz", "renamed");
        assert!(edit.contains(r#""line":1,"character":3"#));
    }
}

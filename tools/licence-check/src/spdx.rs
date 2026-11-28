//! Whether a licence expression is permissive (T-001-06).
//!
//! The permissive set is the one `licences/RECORD`'s header names: MIT, Apache-2.0, the BSD licences, ISC, Zlib,
//! Unicode-3.0, MIT-0, Unlicense, BSL-1.0, CC0-1.0 and 0BSD, plus `Apache-2.0 WITH LLVM-exception`. An `OR` is
//! permissive when any alternative is, an `AND` when every term is, so `MIT OR GPL-2.0-only` is permissive and
//! `(MIT OR Apache-2.0) AND GPL-3.0-only` is not. A `/` reads as `OR`. An expression this reader cannot parse, or
//! an identifier outside the set, is not permissive: the check fails closed.

const PERMISSIVE: [&str; 13] = [
    "MIT",
    "Apache-2.0",
    "BSD-1-Clause",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
    "MIT-0",
    "Unlicense",
    "BSL-1.0",
    "CC0-1.0",
    "0BSD",
];

/// Whether `expr` is permissive, as described in the module documentation.
pub fn is_permissive(expr: &str) -> bool {
    let tokens = tokenize(expr);
    let mut pos = 0;
    match or_expr(&tokens, &mut pos) {
        Some(permissive) if pos == tokens.len() => permissive,
        _ => false,
    }
}

fn tokenize(expr: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let flush = |current: &mut String, tokens: &mut Vec<String>| {
        if !current.is_empty() {
            tokens.push(std::mem::take(current));
        }
    };
    for c in expr.chars() {
        match c {
            '(' | ')' => {
                flush(&mut current, &mut tokens);
                tokens.push(c.to_string());
            }
            '/' => {
                flush(&mut current, &mut tokens);
                tokens.push("OR".to_string());
            }
            c if c.is_whitespace() => flush(&mut current, &mut tokens),
            c => current.push(c),
        }
    }
    flush(&mut current, &mut tokens);
    tokens
}

fn or_expr(tokens: &[String], pos: &mut usize) -> Option<bool> {
    let mut result = and_expr(tokens, pos)?;
    while tokens.get(*pos).map(String::as_str) == Some("OR") {
        *pos += 1;
        let next = and_expr(tokens, pos)?;
        result = result || next;
    }
    Some(result)
}

fn and_expr(tokens: &[String], pos: &mut usize) -> Option<bool> {
    let mut result = with_expr(tokens, pos)?;
    while tokens.get(*pos).map(String::as_str) == Some("AND") {
        *pos += 1;
        let next = with_expr(tokens, pos)?;
        result = result && next;
    }
    Some(result)
}

fn with_expr(tokens: &[String], pos: &mut usize) -> Option<bool> {
    let base = atom(tokens, pos)?;
    if tokens.get(*pos).map(String::as_str) == Some("WITH") {
        *pos += 1;
        let exception = tokens.get(*pos)?;
        *pos += 1;
        return Some(base && exception == "LLVM-exception");
    }
    Some(base)
}

fn atom(tokens: &[String], pos: &mut usize) -> Option<bool> {
    let token = tokens.get(*pos)?;
    *pos += 1;
    match token.as_str() {
        "(" => {
            let inner = or_expr(tokens, pos)?;
            if tokens.get(*pos).map(String::as_str) != Some(")") {
                return None;
            }
            *pos += 1;
            Some(inner)
        }
        ")" | "AND" | "OR" | "WITH" => None,
        id => Some(PERMISSIVE.contains(&id)),
    }
}

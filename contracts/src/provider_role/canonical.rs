//! The one canonical JSON encoder of the contracts: RFC 8785, the JSON Canonicalization Scheme (JCS).
//!
//! The signing rule ([`super::Statement`]'s module docs) signs the RFC 8785 canonical JSON of the envelope with only
//! `signature.value` removed, and the host contract calls two recorded inputs equal when they have the same RFC
//! 8785 canonical JSON (`contracts/state-machine/host/README.md`, `exactly-once`). This is the encoder both mean
//! (`EXP-1` outcome, finding 7; `REQ-PRT-1`, `REQ-STM-4`, `INV-8`).
//!
//! - **Input** is I-JSON (RFC 7493), read here by its own parser rather than through `serde_json::Value`: UTF-8,
//!   no duplicate member names (the value model would keep the last one silently, so two parties could read one
//!   signed text two ways), no lone surrogate in an escape, and no number outside the IEEE 754 double range.
//!   Each is refused with a [`CanonicalError`], never repaired.
//! - **Output** is RFC 8785 §3.2: no whitespace; members sorted by their names' UTF-16 code units, recursively;
//!   strings with only `"`, `\`, and U+0000 to U+001F escaped, the five with short forms as `\b`, `\t`, `\n`, `\f`,
//!   `\r` and the rest as lower-case `\u00hh`; every number as the double it denotes, written as ECMAScript's
//!   `Number.prototype.toString` writes it (ECMA-262 §7.1.12.1, with its Note 2: of two shortest candidates, the
//!   one closer to the exact value, and of two as close, the even one).
//!
//! It needs only `core` and `alloc`, and no dependency beyond the crate's (`DEC-26`).

use alloc::{
    borrow::ToOwned,
    collections::BTreeSet,
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::fmt::Write as _;

use serde::Serialize;

/// The deepest nesting of arrays and objects the parser reads, as `serde_json`'s own limit.
pub const MAX_DEPTH: usize = 128;

/// Why a text has no canonical form. Offsets count bytes from the start of the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalError {
    /// The text isn't UTF-8.
    NotUtf8,
    /// The text isn't JSON at this offset, or has more after its one value.
    Syntax {
        /// Where.
        at: usize,
    },
    /// A `\u` escape names a surrogate that isn't half of a pair (RFC 8785 §3.2.2.2).
    LoneSurrogate {
        /// Where the escape starts.
        at: usize,
    },
    /// An object names a member twice (RFC 7493 §2.3).
    DuplicateMember {
        /// Where the second name starts.
        at: usize,
    },
    /// A number beyond the largest finite double (RFC 8785 §3.2.2.3).
    NumberOutOfRange {
        /// Where the number starts.
        at: usize,
    },
    /// Arrays and objects nest deeper than [`MAX_DEPTH`].
    TooDeep {
        /// Where the level that is too deep starts.
        at: usize,
    },
    /// A value that doesn't serialise as JSON.
    Unserialisable,
    /// Not a statement envelope: an object whose `signature` member is an object with a string `value`.
    NotAnEnvelope,
}

/// The RFC 8785 canonical form of a JSON text.
pub fn canonicalize(json: &[u8]) -> Result<Vec<u8>, CanonicalError> {
    let value = parse(json)?;
    let mut out = String::new();
    write_value(&value, &mut out);
    Ok(out.into_bytes())
}

/// The RFC 8785 canonical form of a value, through its `serde` JSON form.
///
/// Unlike RFC 8785 §3.2.2.3, which makes a non-finite number an error, this writes one as `null`: it has no JSON
/// form, and `serde_json` writes it so before the encoder sees it. A caller holding floats must refuse them first. No type of the contracts holds one: integers are at most 2^53 - 1 or carried as strings.
pub fn to_canonical<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, CanonicalError> {
    let json = serde_json::to_vec(value).map_err(|_| CanonicalError::Unserialisable)?;
    canonicalize(&json)
}

/// The bytes a statement's maker signs, under the signing rule: the RFC 8785 canonical form of the statement as
/// received, with only the top-level `signature.value` removed. `received` is the statement's text as it
/// arrived, never a re-serialisation of a parsed type, so a field the receiver's types don't model is still
/// signed.
pub fn signing_input(received: &[u8]) -> Result<Vec<u8>, CanonicalError> {
    let mut value = parse(received)?;
    let Value::Object(members) = &mut value else {
        return Err(CanonicalError::NotAnEnvelope);
    };
    let Some(Value::Object(signature)) = members
        .iter_mut()
        .find(|m| m.name == "signature")
        .map(|m| &mut m.value)
    else {
        return Err(CanonicalError::NotAnEnvelope);
    };
    let Some(index) = signature
        .iter()
        .position(|m| m.name == "value" && matches!(m.value, Value::String(_)))
    else {
        return Err(CanonicalError::NotAnEnvelope);
    };
    signature.remove(index);
    let mut out = String::new();
    write_value(&value, &mut out);
    Ok(out.into_bytes())
}

/// The text of the member that `path` names, by member name from the top-level object, exactly as it appears in
/// `json`; `None` if a name on the path is missing or names no object to go on through. `json` must be I-JSON as
/// [`canonicalize`] reads it. A statement nested in another, such as a judgement carried in a verifier's result,
/// is received as this text.
pub fn member<'a>(json: &'a [u8], path: &[&str]) -> Result<Option<&'a [u8]>, CanonicalError> {
    let value = parse(json)?;
    let mut current = &value;
    let mut span = None;
    for name in path {
        let Value::Object(members) = current else {
            return Ok(None);
        };
        let Some(found) = members.iter().find(|m| m.name == *name) else {
            return Ok(None);
        };
        current = &found.value;
        span = Some(found.span);
    }
    Ok(span.map(|(start, end)| &json[start..end]))
}

/// A JSON value as I-JSON reads it.
enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<Member>),
}

/// An object's member, with where its value's text lies in the input.
struct Member {
    name: String,
    value: Value,
    span: (usize, usize),
}

fn parse(json: &[u8]) -> Result<Value, CanonicalError> {
    let text = core::str::from_utf8(json).map_err(|_| CanonicalError::NotUtf8)?;
    let mut parser = Parser {
        text,
        bytes: json,
        at: 0,
    };
    parser.whitespace();
    let value = parser.value(0)?;
    parser.whitespace();
    if parser.at != json.len() {
        return Err(CanonicalError::Syntax { at: parser.at });
    }
    Ok(value)
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn syntax(&self) -> CanonicalError {
        CanonicalError::Syntax { at: self.at }
    }

    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), CanonicalError> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.syntax())
        }
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value, CanonicalError> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.syntax())
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, CanonicalError> {
        match self.peek() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => self.string().map(Value::String),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.syntax()),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, CanonicalError> {
        if depth > MAX_DEPTH {
            return Err(CanonicalError::TooDeep { at: self.at });
        }
        self.expect(b'{')?;
        let mut members: Vec<Member> = Vec::new();
        // Names seen so far, so a repeat costs a lookup and not a scan of every earlier member.
        let mut names: BTreeSet<String> = BTreeSet::new();
        self.whitespace();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Value::Object(members));
        }
        loop {
            self.whitespace();
            let name_at = self.at;
            let name = self.string()?;
            if !names.insert(name.clone()) {
                return Err(CanonicalError::DuplicateMember { at: name_at });
            }
            self.whitespace();
            self.expect(b':')?;
            self.whitespace();
            let start = self.at;
            let value = self.value(depth)?;
            let span = (start, self.at);
            members.push(Member { name, value, span });
            self.whitespace();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Value::Object(members));
                }
                _ => return Err(self.syntax()),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, CanonicalError> {
        if depth > MAX_DEPTH {
            return Err(CanonicalError::TooDeep { at: self.at });
        }
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.whitespace();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.whitespace();
            items.push(self.value(depth)?);
            self.whitespace();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.syntax()),
            }
        }
    }

    fn hex4(&mut self) -> Result<u16, CanonicalError> {
        let digits = self
            .text
            .get(self.at..self.at + 4)
            .filter(|d| d.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| self.syntax())?;
        let unit = u16::from_str_radix(digits, 16).map_err(|_| self.syntax())?;
        self.at += 4;
        Ok(unit)
    }

    fn string(&mut self) -> Result<String, CanonicalError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let run = self.at;
            while matches!(self.peek(), Some(b) if b != b'"' && b != b'\\' && b >= 0x20) {
                self.at += 1;
            }
            // The run ends on an ASCII byte or the end, so it is a whole UTF-8 slice.
            out.push_str(&self.text[run..self.at]);
            match self.peek() {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    let escape = self.at;
                    self.at += 1;
                    let Some(kind) = self.peek() else {
                        return Err(self.syntax());
                    };
                    self.at += 1;
                    match kind {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape(escape)?),
                        _ => return Err(CanonicalError::Syntax { at: escape }),
                    }
                }
                // A control character unescaped, or the text ends inside the string.
                _ => return Err(self.syntax()),
            }
        }
    }

    /// The character a `\u` escape names, its `\u` already read; a surrogate must be followed by its other half.
    fn unicode_escape(&mut self, escape: usize) -> Result<char, CanonicalError> {
        let lone = CanonicalError::LoneSurrogate { at: escape };
        let unit = self.hex4()?;
        match unit {
            0xD800..=0xDBFF => {
                if !self.bytes[self.at..].starts_with(b"\\u") {
                    return Err(lone);
                }
                self.at += 2;
                let low = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err(lone);
                }
                let code = 0x10000 + ((u32::from(unit) - 0xD800) << 10) + (u32::from(low) - 0xDC00);
                char::from_u32(code).ok_or(lone)
            }
            0xDC00..=0xDFFF => Err(lone),
            _ => char::from_u32(u32::from(unit)).ok_or(lone),
        }
    }

    /// A number in RFC 8259's grammar, read as the double nearest it, as ECMAScript reads it.
    fn number(&mut self) -> Result<Value, CanonicalError> {
        let start = self.at;
        let digits = |p: &mut Self| {
            let from = p.at;
            while matches!(p.peek(), Some(b'0'..=b'9')) {
                p.at += 1;
            }
            p.at - from
        };
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => {
                digits(self);
            }
            _ => return Err(self.syntax()),
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if digits(self) == 0 {
                return Err(self.syntax());
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if digits(self) == 0 {
                return Err(self.syntax());
            }
        }
        // `core`'s parser rounds correctly, to the nearest double and ties to even, as ECMAScript does.
        let number: f64 = self.text[start..self.at]
            .parse()
            .map_err(|_| CanonicalError::Syntax { at: start })?;
        if number.is_finite() {
            Ok(Value::Number(number))
        } else {
            Err(CanonicalError::NumberOutOfRange { at: start })
        }
    }
}

fn write_value(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => out.push_str(&number_to_string(*n)),
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        Value::Object(members) => {
            let mut sorted: Vec<&Member> = members.iter().collect();
            sorted.sort_by(|a, b| a.name.encode_utf16().cmp(b.name.encode_utf16()));
            out.push('{');
            for (index, member) in sorted.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(&member.name, out);
                out.push(':');
                write_value(&member.value, out);
            }
            out.push('}');
        }
    }
}

fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A finite double as ECMAScript's `Number.prototype.toString` writes it (ECMA-262 §7.1.12.1, Note 2).
fn number_to_string(value: f64) -> String {
    if value == 0.0 {
        // Minus zero too.
        return "0".to_owned();
    }
    if value < 0.0 {
        return format!("-{}", number_to_string(-value));
    }
    let (digits, n) = shortest_digits(value);
    let k = digits.len() as i32;
    if k <= n && n <= 21 {
        let mut text = digits;
        text.extend(core::iter::repeat_n('0', (n - k) as usize));
        text
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{}", "0".repeat((-n) as usize), digits)
    } else {
        let exponent = n - 1;
        let sign = if exponent < 0 { '-' } else { '+' };
        let (first, rest) = digits.split_at(1);
        if rest.is_empty() {
            format!("{first}e{sign}{}", exponent.unsigned_abs())
        } else {
            format!("{first}.{rest}e{sign}{}", exponent.unsigned_abs())
        }
    }
}

/// The digits `s` of ECMA-262's `k`, `n` for a positive finite double: the fewest digits that read back as the
/// double, with the double equal to `0.s × 10^n` to within those digits. Of two candidates, the closer to the
/// exact value; of two as close, the even one (Note 2).
fn shortest_digits(value: f64) -> (String, i32) {
    // `core` writes the shortest digits that read back, choosing the closer of two; it doesn't break a tie to
    // even, which is checked below.
    let shortest = format!("{value:e}");
    let (mantissa, exponent) = shortest.split_once('e').expect("an exponent");
    let exponent: i32 = exponent.parse().expect("a decimal exponent");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let n = exponent + 1;
    let last = digits.as_bytes()[digits.len() - 1] - b'0';
    if last % 2 == 1 {
        // A tie needs the exact value to sit halfway between two candidates: the digits, or the digits less one
        // in the last place, followed by a 5 and nothing more.
        let exact = format!("{value:.780e}");
        let (exact_mantissa, exact_exponent) = exact.split_once('e').expect("an exponent");
        let exact_digits: String = exact_mantissa
            .chars()
            .filter(char::is_ascii_digit)
            .collect::<String>()
            .trim_end_matches('0')
            .to_string();
        let same_magnitude = exact_exponent.parse::<i32>().ok() == Some(exponent);
        let above = format!("{digits}5");
        let mut lower = digits.clone();
        lower.pop();
        lower.push(char::from(b'0' + last - 1));
        let below = format!("{lower}5");
        let candidate = if same_magnitude && exact_digits == above {
            Some(increment(&digits, n))
        } else if same_magnitude && exact_digits == below {
            Some((lower, n))
        } else {
            None
        };
        if let Some((even, even_n)) = candidate
            && reads_back(&even, even_n, value)
        {
            return (trim(even), even_n);
        }
    }
    (trim(digits), n)
}

/// The digits plus one in the last place, with `n` raised by one where a carry passes the first digit.
fn increment(digits: &str, n: i32) -> (String, i32) {
    let mut bytes = digits.as_bytes().to_vec();
    let n = match bytes.iter().rposition(|b| *b != b'9') {
        Some(index) => {
            bytes[index] += 1;
            for b in &mut bytes[index + 1..] {
                *b = b'0';
            }
            n
        }
        None => {
            bytes = core::iter::once(b'1')
                .chain(core::iter::repeat_n(b'0', bytes.len()))
                .collect();
            n + 1
        }
    };
    (String::from_utf8(bytes).expect("ASCII digits"), n)
}

fn trim(digits: String) -> String {
    let trimmed = digits.trim_end_matches('0');
    if trimmed.is_empty() {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Whether `0.digits × 10^n` reads back as `value`.
fn reads_back(digits: &str, n: i32, value: f64) -> bool {
    format!("0.{digits}e{n}").parse::<f64>() == Ok(value)
}

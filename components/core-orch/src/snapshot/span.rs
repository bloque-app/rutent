//! The text of one value inside a JSON text, exactly as it appears there, addressed by member names and list
//! places. A statement nested in another is verified over its own bytes as received, never a re-serialisation,
//! so a field Core's types don't model, or a member named twice, stays visible to the verifier; provider-role's
//! `member` reaches values by member names only, and a decision record carries its statements in a list
//! (`snapshot.figures`).
//!
//! The scanner only finds where a value starts and ends. It runs on a text whose outer statement already passed
//! the catalogue's verifier, which parsed it strictly, and the text it finds is verified on its own in turn, so a
//! text it misreads verifies as nothing: it fails closed.

/// One step of a path: a member, by its name, or a place in a list, from 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathStep {
    /// A member of an object.
    Member(String),
    /// A place in a list.
    Item(usize),
}

/// The text of the value `path` names in `json`, or `None` where the path names nothing.
pub(super) fn value_at<'a>(json: &'a [u8], path: &[PathStep]) -> Option<&'a [u8]> {
    let mut start = skip_space(json, 0);
    for step in path {
        start = match step {
            PathStep::Member(name) => member_start(json, start, name)?,
            PathStep::Item(place) => item_start(json, start, *place)?,
        };
    }
    let end = value_end(json, start)?;
    Some(&json[start..end])
}

fn skip_space(json: &[u8], mut at: usize) -> usize {
    while at < json.len() && matches!(json[at], b' ' | b'\t' | b'\n' | b'\r') {
        at += 1;
    }
    at
}

/// Where the value of member `name` of the object at `at` starts.
fn member_start(json: &[u8], at: usize, name: &str) -> Option<usize> {
    if json.get(at) != Some(&b'{') {
        return None;
    }
    let mut at = skip_space(json, at + 1);
    if json.get(at) == Some(&b'}') {
        return None;
    }
    loop {
        let key_end = string_end(json, at)?;
        let key: String = serde_json::from_slice(&json[at..key_end]).ok()?;
        at = skip_space(json, key_end);
        if json.get(at) != Some(&b':') {
            return None;
        }
        let value = skip_space(json, at + 1);
        if key == name {
            return Some(value);
        }
        at = skip_space(json, value_end(json, value)?);
        match json.get(at) {
            Some(b',') => at = skip_space(json, at + 1),
            _ => return None,
        }
    }
}

/// Where item `place` of the list at `at` starts.
fn item_start(json: &[u8], at: usize, place: usize) -> Option<usize> {
    if json.get(at) != Some(&b'[') {
        return None;
    }
    let mut at = skip_space(json, at + 1);
    if json.get(at) == Some(&b']') {
        return None;
    }
    for _ in 0..place {
        at = skip_space(json, value_end(json, at)?);
        match json.get(at) {
            Some(b',') => at = skip_space(json, at + 1),
            _ => return None,
        }
    }
    Some(at)
}

/// Where the value starting at `at` ends (one past its last byte).
fn value_end(json: &[u8], at: usize) -> Option<usize> {
    match json.get(at)? {
        b'"' => string_end(json, at),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut i = at;
            while i < json.len() {
                match json[i] {
                    b'"' => {
                        i = string_end(json, i)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(i + 1);
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            None
        }
        _ => {
            let mut i = at;
            while i < json.len()
                && !matches!(json[i], b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r')
            {
                i += 1;
            }
            (i > at).then_some(i)
        }
    }
}

/// Where the string starting at `at` ends (one past its closing quote).
fn string_end(json: &[u8], at: usize) -> Option<usize> {
    if json.get(at) != Some(&b'"') {
        return None;
    }
    let mut i = at + 1;
    while i < json.len() {
        match json[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // verifies: REQ-AUD-4
    #[test]
    fn a_value_is_found_by_member_names_and_list_places_exactly_as_written() {
        let json = br#" { "a" : [ 1 , { "b\"q" : "x]}" , "c" : { "d" : [ ] } } ] , "e" : true } "#;
        let path = |steps: &[PathStep]| {
            value_at(json, steps).map(|t| String::from_utf8(t.to_vec()).unwrap())
        };
        let m = |n: &str| PathStep::Member(n.into());
        assert_eq!(path(&[m("e")]).as_deref(), Some("true"));
        assert_eq!(path(&[m("a"), PathStep::Item(0)]).as_deref(), Some("1"));
        assert_eq!(
            path(&[m("a"), PathStep::Item(1), m("c")]).as_deref(),
            Some(r#"{ "d" : [ ] }"#)
        );
        assert_eq!(
            path(&[m("a"), PathStep::Item(1), m("b\"q")]).as_deref(),
            Some(r#""x]}""#)
        );
        assert_eq!(path(&[m("a"), PathStep::Item(2)]), None);
        assert_eq!(path(&[m("z")]), None);
        assert_eq!(path(&[m("e"), m("f")]), None);
    }
}

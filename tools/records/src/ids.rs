//! Which identifiers the documents define, and which they have withdrawn.

use crate::table::tables;
use std::collections::BTreeSet;
use std::io;
use std::path::Path;

/// True when `s` has the shape of an identifier: `REQ-MOD-1`, `T-001-03`, `ADR-0001`, `C1`, `M1`.
///
/// Patterns such as `DEC-*` and placeholders such as `EXP-n` are not identifiers.
pub fn is_id(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    let upper = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_uppercase());
    if parts.len() == 1 {
        let letters = s.trim_end_matches(|c: char| c.is_ascii_digit());
        let digits = &s[letters.len()..];
        return upper(letters) && !digits.is_empty();
    }
    let (last, rest) = parts.split_last().expect("at least two parts");
    let digits = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit());
    upper(rest[0]) && rest[1..].iter().all(|p| upper(p) || digits(p)) && digits(last)
}

/// Every identifier-shaped word in `text`, in order, each with whether the word before it is "Narthex"
/// (Narthex's own identifiers are never read as rutent's).
pub fn ids_in(text: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut previous = String::new();
    for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
        let word = word.trim_matches('-');
        if word.is_empty() {
            continue;
        }
        if is_id(word) {
            out.push((word.to_string(), previous.eq_ignore_ascii_case("narthex")));
        }
        previous = word.to_string();
    }
    out
}

/// The identifiers the documents define and the ones `SPEC.md` withdraws.
#[derive(Debug, Default, Clone)]
pub struct Defined {
    /// Defined identifiers.
    pub ids: BTreeSet<String>,
    /// Identifiers the specification marks withdrawn; never reusable, never citable.
    pub withdrawn: BTreeSet<String>,
}

impl Defined {
    /// Reads the specification, the ADRs, the plan and the product record (`PS-*` are the owner's statements,
    /// which `PRODUCT.md` defines and `SPEC.md` and the plan cite).
    pub fn collect(spec: &str, adrs: &[&str], plan: &str, product: &str) -> Defined {
        let mut d = Defined::default();
        d.ids.extend(table_and_heading_ids(spec));
        d.ids.extend(table_and_heading_ids(plan));
        d.ids.extend(table_and_heading_ids(product));
        for adr in adrs {
            for line in adr.lines() {
                if let Some(title) = line.strip_prefix("# ") {
                    if let Some((id, _)) = ids_in(title).into_iter().next()
                        && id.starts_with("ADR-")
                    {
                        d.ids.insert(id);
                    }
                    break;
                }
            }
        }
        d.withdrawn = withdrawn_in(spec);
        for w in &d.withdrawn {
            d.ids.remove(w);
        }
        d
    }

    /// True when the identifier is defined and not withdrawn.
    pub fn is_defined(&self, id: &str) -> bool {
        self.ids.contains(id)
    }
}

/// Identifiers a document defines: the first cell of a table row (when it is exactly one identifier) and the
/// backticked identifiers in a heading.
fn table_and_heading_ids(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for table in tables(text) {
        for (_, row) in table.rows {
            if let Some(first) = row.first() {
                let cell = first.trim().trim_matches('`');
                if is_id(cell) {
                    out.insert(cell.to_string());
                }
            }
        }
    }
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        if !fenced && line.starts_with('#') {
            for part in line.split('`').skip(1).step_by(2) {
                if is_id(part) {
                    out.insert(part.to_string());
                }
            }
        }
    }
    out
}

/// Withdrawn identifiers. The specification says, under a family's heading (`` ### `REQ-AUZ-*`: … ``), a line
/// such as "Number 10 in this family is withdrawn"; that withdraws `REQ-AUZ-10`. A prose line saying a named
/// identifier "is withdrawn" counts too. Table rows never do: a threat may talk about a yes being withdrawn.
fn withdrawn_in(spec: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut family: Option<String> = None;
    for line in spec.lines() {
        if line.starts_with('#') {
            family = line
                .split('`')
                .skip(1)
                .step_by(2)
                .find_map(|p| p.strip_suffix("-*").map(str::to_string));
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if line.trim_start().starts_with('|') || !lower.contains("withdrawn") {
            continue;
        }
        if let (Some(prefix), Some(at)) = (&family, lower.find("number ")) {
            let digits: String = lower[at + 7..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if !digits.is_empty() && lower[at..].contains("in this family") {
                out.insert(format!("{prefix}-{digits}"));
            }
        }
        for part in line.split('`').skip(1).step_by(2) {
            if is_id(part) && lower.contains(&format!("{} is withdrawn", part.to_ascii_lowercase()))
            {
                out.insert(part.to_string());
            }
        }
    }
    out
}

/// Reads `SPEC.md`, `PLAN.md`, `PRODUCT.md` and `adr/*.md` under `root`.
pub fn load_defined(root: &Path) -> io::Result<Defined> {
    let spec = std::fs::read_to_string(root.join("SPEC.md"))?;
    let plan = std::fs::read_to_string(root.join("PLAN.md"))?;
    let product = std::fs::read_to_string(root.join("PRODUCT.md"))?;
    let mut adrs = Vec::new();
    for entry in std::fs::read_dir(root.join("adr"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "md") {
            adrs.push(std::fs::read_to_string(path)?);
        }
    }
    let adrs: Vec<&str> = adrs.iter().map(String::as_str).collect();
    Ok(Defined::collect(&spec, &adrs, &plan, &product))
}

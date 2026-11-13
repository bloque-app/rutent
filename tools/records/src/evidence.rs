//! The requirement-to-evidence table (`T-024-09`): which of the specification's identifiers `M3` must verify, and
//! which test or check verifies each.
//!
//! The scope file (`conformance/evidence/v0-scope`) classes every requirement, invariant, acceptance criterion,
//! non-functional requirement and guarantee `SPEC.md` defines. The table joins each `M3` row to the tests whose
//! `// verifies:` comment names it and to a results file that says which of those tests passed.
//!
//! Everything here reads text and returns findings; only the binary touches files. Two kinds of finding come
//! apart: those of the table itself ([`Report::table`]), which a switch in the scope file may stop from failing a
//! run, and those of the comparison with `develop` ([`Report::ratchet`]), which fail whatever the switch says.
//!
//! The scope file is plain lines of `ID | class | citations | note`, `#` for comments, and one `strict: off` or
//! `strict: on` line:
//!
//! - `m3`: verified by a tagged test that passed. Citations may carry `later-part:F-033`, the owner of an `M4` half.
//! - `partly`: `M3`'s but not met; the citation is the form item it waits on (`form:cat5-timing-residual`).
//! - `inspect`: verified by inspection; the citation is a check (`check:documentation`, `check:boundary`,
//!   `check:licence`) or a recorded review (`review:F-011`, whose notes must exist).
//! - `m4`: a feature (`F-030`) and the placement row of `PLAN.md` §7.1 (`7.1:REQ-OPS`).
//! - `deferred`: the decision, or the assumption and risk, or the feature and §7.1 row that defers it
//!   (`DEC-8`; `ASM-1 6.2:risk-9`; `F-052 7.1:REQ-EXT`).

use crate::table::tables;
use crate::tags::TestCase;
use crate::{
    Finding,
    ids::{Defined, ids_in},
};
use std::collections::{BTreeMap, BTreeSet};

/// The prefixes of the identifiers the table covers.
const FAMILIES: [&str; 5] = ["REQ-", "INV-", "AC-", "NFR-", "GUA-"];

/// The checks a row verified by inspection may name: the documentation, boundary and licence checks.
const CHECKS: [&str; 3] = ["documentation", "boundary", "licence"];

/// The class a scope file gives an identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// `M3`'s: needs a passing verifier.
    M3,
    /// `M3`'s, and not met yet: waits on a form item.
    Partly,
    /// `M3`'s, verified by a named check or a recorded review.
    Inspect,
    /// `M4`'s, with its feature.
    M4,
    /// Deferred, with the decision that defers it.
    Deferred,
}

impl Class {
    fn parse(word: &str) -> Option<Class> {
        Some(match word.to_ascii_lowercase().as_str() {
            "m3" => Class::M3,
            "partly" => Class::Partly,
            "inspect" => Class::Inspect,
            "m4" => Class::M4,
            "deferred" => Class::Deferred,
            _ => return None,
        })
    }

    /// The word the scope file uses.
    pub fn word(self) -> &'static str {
        match self {
            Class::M3 => "m3",
            Class::Partly => "partly",
            Class::Inspect => "inspect",
            Class::M4 => "m4",
            Class::Deferred => "deferred",
        }
    }

    /// True for the classes that `M3`'s exit holds to account.
    pub fn is_m3(self) -> bool {
        matches!(self, Class::M3 | Class::Partly | Class::Inspect)
    }
}

/// One row of the scope file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The 1-based line in the scope file.
    pub line: usize,
    /// The identifier.
    pub id: String,
    /// Its class.
    pub class: Class,
    /// The citations: whitespace- or comma-separated words of the third column.
    pub cites: Vec<String>,
    /// The fourth column.
    pub note: String,
}

/// A scope file, read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    /// Whether the file reads `strict: on`. A file with no such line reads off.
    pub strict_on: bool,
    /// Its rows, in order.
    pub rows: Vec<Row>,
}

impl Scope {
    fn row(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.id == id)
    }
}

fn finding(rule: &'static str, location: String, message: String) -> Finding {
    Finding {
        rule,
        location,
        message,
    }
}

/// Reads a scope file. A line that is not a comment, the switch or a row is a finding of the table.
pub fn parse_scope(text: &str) -> (Scope, Vec<Finding>) {
    let mut scope = Scope::default();
    let mut problems = Vec::new();
    let mut switch_seen = false;
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let location = format!("scope:{}", index + 1);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(value) = line.strip_prefix("strict:") {
            match value.trim() {
                "on" => scope.strict_on = true,
                "off" => scope.strict_on = false,
                other => problems.push(finding(
                    "bad-row",
                    location,
                    format!("the switch reads `strict: on` or `strict: off`, not `{other}`"),
                )),
            }
            switch_seen = true;
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let class = cells.get(1).and_then(|w| Class::parse(w));
        match (cells.first(), class) {
            (Some(id), Some(class)) if crate::ids::is_id(id) => {
                if scope.row(id).is_some() {
                    problems.push(finding(
                        "duplicate-row",
                        location,
                        format!("{id} is listed twice"),
                    ));
                    continue;
                }
                scope.rows.push(Row {
                    line: index + 1,
                    id: (*id).to_string(),
                    class,
                    cites: cells
                        .get(2)
                        .map(|c| {
                            c.split(|ch: char| ch == ',' || ch.is_whitespace())
                                .filter(|w| !w.is_empty())
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                    note: cells.get(3).copied().unwrap_or_default().to_string(),
                });
            }
            _ => problems.push(finding(
                "bad-row",
                location,
                format!(
                    "`{line}` is not `ID | class | citations | note` with a class of m3, partly, inspect, m4 or deferred"
                ),
            )),
        }
    }
    if !switch_seen {
        problems.push(finding(
            "bad-row",
            "scope".into(),
            "the file has no `strict: on` or `strict: off` line, so it reads off".into(),
        ));
    }
    (scope, problems)
}

/// The identifiers the table covers: those `SPEC.md` defines in the five families, minus the withdrawn ones.
pub fn universe(spec: &str) -> BTreeSet<String> {
    Defined::collect(spec, &[], "", "")
        .ids
        .into_iter()
        .filter(|id| FAMILIES.iter().any(|f| id.starts_with(f)))
        .collect()
}

/// What a test run said about one test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// It ran and passed.
    Passed,
    /// It ran and failed.
    Failed,
    /// It was ignored.
    Ignored,
}

/// The lines `cargo test` printed, read: each test's outcomes by the last `::` segment of its name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Results {
    by_name: BTreeMap<String, Vec<Outcome>>,
}

impl Results {
    /// Reads libtest's `test <path> ... ok|FAILED|ignored` lines; every other line is ignored.
    pub fn parse(text: &str) -> Results {
        let mut by_name: BTreeMap<String, Vec<Outcome>> = BTreeMap::new();
        for line in text.lines() {
            let Some(rest) = line.trim().strip_prefix("test ") else {
                continue;
            };
            let Some((path, outcome)) = rest.rsplit_once(" ... ") else {
                continue;
            };
            let outcome = match outcome.split_whitespace().next() {
                Some("ok") => Outcome::Passed,
                Some("FAILED") => Outcome::Failed,
                Some("ignored") => Outcome::Ignored,
                _ => continue,
            };
            let name = path.rsplit("::").next().unwrap_or(path).trim();
            by_name.entry(name.to_string()).or_default().push(outcome);
        }
        Results { by_name }
    }

    /// A test passed when it ran at least once and every run of that name passed. A name that ran twice and
    /// failed once, or was ignored once, does not pass: the join is by name, so it fails closed.
    pub fn passed(&self, name: &str) -> bool {
        self.by_name
            .get(name)
            .is_some_and(|runs| !runs.is_empty() && runs.iter().all(|o| *o == Outcome::Passed))
    }

    /// The word for a test's state in the report: `passed`, `failed`, `ignored` or `not run`.
    pub fn state(&self, name: &str) -> &'static str {
        match self.by_name.get(name) {
            None => "not run",
            Some(runs) if runs.contains(&Outcome::Failed) => "failed",
            Some(runs) if runs.contains(&Outcome::Ignored) => "ignored",
            Some(_) => "passed",
        }
    }

    /// True when no test line was read.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

/// What the table says of one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The identifier.
    pub id: String,
    /// Its class.
    pub class: Class,
    /// `verified`, `partly met`, `inspected`, `unverified`, or what places it elsewhere.
    pub status: String,
    /// The tests, checks and owners behind it.
    pub detail: String,
}

/// The table, and the findings of its two kinds.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// One line per row, in the file's order.
    pub lines: Vec<Line>,
    /// The table's own findings: a switch may stop them from failing a run.
    pub table: Vec<Finding>,
    /// The comparison with `develop`'s: they fail a strict run whatever the switch says.
    pub ratchet: Vec<Finding>,
}

/// What the table needs to know of the repository beyond the scope file.
pub struct Inputs<'a> {
    /// `SPEC.md`'s identifiers in the five families ([`universe`]).
    pub universe: &'a BTreeSet<String>,
    /// Every identifier the documents define (a decision, a feature, an assumption).
    pub defined: &'a Defined,
    /// The tests of the workspace, with their tags.
    pub tests: &'a [TestCase],
    /// What the test run said.
    pub results: &'a Results,
    /// True when a feature's notes exist, so that a recorded review can be cited.
    pub review_exists: &'a dyn Fn(&str) -> bool,
}

fn cited_ids(cites: &[String]) -> impl Iterator<Item = &String> {
    cites.iter().filter(|c| crate::ids::is_id(c))
}

/// Joins the scope file to the specification's identifiers, the tags and the results.
pub fn build(scope: &Scope, inputs: &Inputs<'_>) -> Report {
    let mut report = Report::default();
    let at = |row: &Row| format!("scope:{}", row.line);

    for id in inputs.universe {
        if scope.row(id).is_none() {
            report.table.push(finding(
                "missing-id",
                "scope".into(),
                format!("{id} is defined by SPEC.md and is in no row of the scope file"),
            ));
        }
    }

    for row in &scope.rows {
        let id = &row.id;
        let location = at(row);
        if !inputs.universe.contains(id) {
            report.table.push(finding(
                "unknown-id",
                location.clone(),
                format!("{id} is not an identifier SPEC.md defines and has not withdrawn"),
            ));
        }
        let (status, detail) = match row.class {
            Class::M3 => m3_line(row, inputs, &mut report.table, &location),
            Class::Partly => {
                let item = row.cites.iter().find_map(|c| c.strip_prefix("form:"));
                if item.is_none_or(str::is_empty) {
                    report.table.push(finding(
                        "partly-without-item",
                        location.clone(),
                        format!(
                            "{id} is partly met and names no form item it waits on (`form:<item>`)"
                        ),
                    ));
                }
                (
                    "partly met".to_string(),
                    format!("waits on form item {}", item.unwrap_or("(none)")),
                )
            }
            Class::Inspect => inspect_line(row, inputs, &mut report.table, &location),
            Class::M4 => {
                let feature = cited_ids(&row.cites).find(|c| c.starts_with("F-"));
                if feature.is_none_or(|f| !inputs.defined.is_defined(f)) {
                    report.table.push(finding(
                        "m4-without-feature",
                        location.clone(),
                        format!("{id} is M4's and names no feature the plan defines (`F-0NN`)"),
                    ));
                }
                (
                    "M4's".to_string(),
                    format!("owner {}", feature.map_or("(none)", String::as_str)),
                )
            }
            Class::Deferred => {
                let decisions: Vec<&String> = cited_ids(&row.cites)
                    .filter(|c| {
                        c.starts_with("DEC-") || c.starts_with("ASM-") || c.starts_with("F-")
                    })
                    .collect();
                let placed = decisions.iter().any(|c| inputs.defined.is_defined(c));
                if !placed {
                    report.table.push(finding(
                        "deferred-without-decision",
                        location.clone(),
                        format!("{id} is deferred and cites no decision, assumption or feature the plan defines"),
                    ));
                }
                let by: Vec<&str> = decisions.iter().map(|c| c.as_str()).collect();
                ("deferred".to_string(), format!("by {}", by.join(", ")))
            }
        };
        report.lines.push(Line {
            id: id.clone(),
            class: row.class,
            status,
            detail,
        });
    }
    report
}

fn m3_line(
    row: &Row,
    inputs: &Inputs<'_>,
    out: &mut Vec<Finding>,
    location: &str,
) -> (String, String) {
    if let Some(owner) = row.cites.iter().find_map(|c| c.strip_prefix("later-part:"))
        && !inputs.defined.is_defined(owner)
    {
        out.push(finding(
            "later-part-unknown",
            location.to_string(),
            format!(
                "{} names `{owner}` as its M4 owner, which the plan does not define",
                row.id
            ),
        ));
    }
    let tagged: Vec<&TestCase> = inputs
        .tests
        .iter()
        .filter(|t| t.tag.as_ref().is_some_and(|tag| tag.contains(&row.id)))
        .collect();
    if tagged.is_empty() {
        out.push(finding(
            "no-tagged-test",
            location.to_string(),
            format!(
                "{} is M3's and no test has a `// verifies:` comment naming it",
                row.id
            ),
        ));
        return ("unverified".into(), "no tagged test".into());
    }
    let passing: Vec<&&TestCase> = tagged
        .iter()
        .filter(|t| inputs.results.passed(&t.name))
        .collect();
    if passing.is_empty() {
        let states: Vec<String> = tagged
            .iter()
            .map(|t| format!("{} ({})", t.name, inputs.results.state(&t.name)))
            .collect();
        out.push(finding(
            "no-passing-verifier",
            location.to_string(),
            format!(
                "{} is M3's and none of its tests passed: {}",
                row.id,
                states.join(", ")
            ),
        ));
        return (
            "unverified".into(),
            format!("none passed of {}", tagged.len()),
        );
    }
    (
        "verified".into(),
        format!("{} of {} tests passed", passing.len(), tagged.len()),
    )
}

fn inspect_line(
    row: &Row,
    inputs: &Inputs<'_>,
    out: &mut Vec<Finding>,
    location: &str,
) -> (String, String) {
    let check = row.cites.iter().find_map(|c| c.strip_prefix("check:"));
    let review = row.cites.iter().find_map(|c| c.strip_prefix("review:"));
    let named = check.is_some_and(|c| CHECKS.contains(&c))
        || review.is_some_and(|f| {
            f.starts_with("F-") && inputs.defined.is_defined(f) && (inputs.review_exists)(f)
        });
    if !named {
        out.push(finding(
            "inspection-unnamed",
            location.to_string(),
            format!(
                "{} is verified by inspection and names no check ({}) or recorded review with notes (`review:F-0NN`)",
                row.id,
                CHECKS.join(", ")
            ),
        ));
        return ("unverified".into(), "no check or review named".into());
    }
    let by = check.map_or_else(
        || format!("review {}", review.unwrap_or_default()),
        |c| format!("{c} check"),
    );
    ("inspected".into(), by)
}

/// The text of the table row whose first cell is `key`, outside `PLAN.md` §7.1.
fn plan_row(plan: &str, key: &str) -> Option<String> {
    tables(plan).into_iter().find_map(|t| {
        t.rows.into_iter().find_map(|(_, row)| {
            (row.first().map(|c| c.trim().trim_matches('`')) == Some(key)).then(|| row.join(" | "))
        })
    })
}

/// The text of `PLAN.md` §7.1's row for a family (`REQ-OPS`).
fn placement_row(plan: &str, family: &str) -> Option<String> {
    tables(plan)
        .into_iter()
        .filter(|t| t.heading3.starts_with("7.1"))
        .find_map(|t| {
            t.rows.into_iter().find_map(|(_, row)| {
                (row.first().map(|c| c.trim().trim_matches('`')) == Some(family))
                    .then(|| row.join(" | "))
            })
        })
}

fn names(text: &str, id: &str) -> bool {
    ids_in(text).iter().any(|(word, _)| word == id)
}

/// The id's family: `REQ-OPS` for `REQ-OPS-4`.
fn family_of(id: &str) -> &str {
    id.rsplit_once('-').map_or(id, |(family, _)| family)
}

/// True when a clause of `text` names `id` and, for `needle`, says it too.
fn places(text: &str, id: &str, needle: Option<&str>) -> bool {
    text.split([';', '|'])
        .any(|clause| names(clause, id) && needle.is_none_or(|n| clause.contains(n)))
}

/// True when `head` places `id` (in a clause that says `needle`, if one is given) where `base` did not.
fn newly_places(base: Option<&str>, head: Option<&str>, id: &str, needle: Option<&str>) -> bool {
    head.is_some_and(|h| places(h, id, needle)) && !base.is_some_and(|b| places(b, id, needle))
}

/// What `develop`'s scope file and plan say against the head's. Fails closed on each:
///
/// - `develop` reads `strict: on` and the head reads off;
/// - an identifier `develop` classes as `M3`'s and the head classes as `M4`'s or deferred, unless its new entry
///   cites a row of the plan that the head changed against `develop`'s, to name the identifier where `develop`'s
///   did not: for a deferral, a decision's row; for `M4`, `PLAN.md` §7.1's row for the family, in a clause that
///   says `M4`, and the feature's row, also saying `M4`. An edit to a row that already named the identifier
///   excuses nothing.
pub fn compare(head: &Scope, base: &Scope, head_plan: &str, base_plan: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    if base.strict_on && !head.strict_on {
        out.push(finding(
            "strict-switch-off",
            "scope".into(),
            "develop's scope file reads `strict: on` and this one reads `strict: off`: the switch only turns on".into(),
        ));
    }
    for row in &head.rows {
        let Some(before) = base.row(&row.id) else {
            continue;
        };
        if !before.class.is_m3() || row.class.is_m3() {
            continue;
        }
        let excused = match row.class {
            Class::Deferred => row
                .cites
                .iter()
                .filter(|c| c.starts_with("DEC-"))
                .any(|dec| {
                    newly_places(
                        plan_row(base_plan, dec).as_deref(),
                        plan_row(head_plan, dec).as_deref(),
                        &row.id,
                        None,
                    )
                }),
            Class::M4 => {
                let family = family_of(&row.id);
                let placed = row
                    .cites
                    .iter()
                    .any(|c| c.strip_prefix("7.1:") == Some(family))
                    && newly_places(
                        placement_row(base_plan, family).as_deref(),
                        placement_row(head_plan, family).as_deref(),
                        &row.id,
                        Some("M4"),
                    );
                let feature = row.cites.iter().filter(|c| c.starts_with("F-")).any(|f| {
                    newly_places(
                        plan_row(base_plan, f).as_deref(),
                        plan_row(head_plan, f).as_deref(),
                        &row.id,
                        None,
                    ) && plan_row(head_plan, f).is_some_and(|text| text.contains("M4"))
                });
                placed && feature
            }
            Class::M3 | Class::Partly | Class::Inspect => true,
        };
        if !excused {
            out.push(finding(
                "reclassified",
                format!("scope:{}", row.line),
                format!(
                    "{} is M3's on develop and {} here, and the entry cites no row of the plan that this change made to place it there",
                    row.id,
                    if row.class == Class::M4 { "M4's" } else { "deferred" }
                ),
            ));
        }
    }
    out
}

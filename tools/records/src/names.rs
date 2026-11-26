//! The names check (`T-024-24`): each test `PLAN.md` names for a row of `M1` to `M3` is a test function of the
//! workspace, so a count of named tests is derived from the plan and the code and never stated.
//!
//! A test is named in a task row's `Tests` cell as a phrase in backticks, such as
//! `` `operator's fee is shown apart from the quotes' sum` [`REQ-QTE-4`] ``. The phrase, with its apostrophes dropped and
//! its other runs of non-alphanumeric characters made one underscore, is the function's name. A backticked word
//! with no space in it (a path, a branch, a type, an identifier), a phrase in square brackets or in parentheses
//! (a tag, a note on a renaming), and a row of a feature of `M4` or later name no test.

use crate::plan::{bare, column, feature_milestones, feature_of, is_task_id};
use crate::table::tables;
use crate::{Finding, ids::ids_in};
use std::collections::{BTreeMap, BTreeSet};

/// One test a task row names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    /// The task row.
    pub task: String,
    /// The 1-based line of the row in `PLAN.md`.
    pub line: usize,
    /// The words as the plan writes them.
    pub text: String,
}

/// The function name a plan's words stand for: lower case, apostrophes dropped, every other run of characters
/// that are not letters or digits one underscore, none at either end.
pub fn snake(text: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in text.chars().filter(|c| !matches!(c, '\'' | '\u{2019}')) {
        if c.is_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('_');
            }
            gap = false;
            out.extend(c.to_lowercase());
        } else {
            gap = true;
        }
    }
    out
}

/// The backticked phrases of a cell that name tests: two words or more, outside brackets and parentheses.
fn phrases(cell: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut square, mut round) = (0usize, 0usize);
    let mut open: Option<String> = None;
    for c in cell.chars() {
        if let Some(span) = open.as_mut() {
            if c == '`' {
                let span = open.take().unwrap_or_default();
                if square == 0 && round == 0 && span.split_whitespace().count() > 1 {
                    out.push(span);
                }
            } else {
                span.push(c);
            }
            continue;
        }
        match c {
            '`' => open = Some(String::new()),
            '[' => square += 1,
            ']' => square = square.saturating_sub(1),
            '(' => round += 1,
            ')' => round = round.saturating_sub(1),
            _ => {}
        }
    }
    out
}

/// The wave of each task, from §5's wave table.
fn waves(all: &[crate::table::Table]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for t in all
        .iter()
        .filter(|t| column(t, "Wave") == Some(0) && column(t, "Tasks") == Some(1))
    {
        for (_, row) in &t.rows {
            let Ok(wave) = bare(&row[0]).parse::<usize>() else {
                continue;
            };
            for (id, _) in ids_in(row.get(1).map_or("", String::as_str)) {
                out.entry(id).or_insert(wave);
            }
        }
    }
    out
}

/// Every test the task rows of `M1` to `M3` name, in the plan's order. With `through_wave`, only the rows of that
/// wave or an earlier one: a row written for a later wave names tests that no code has yet. A row in no wave is
/// kept.
pub fn named_tests(plan: &str, through_wave: Option<usize>) -> Vec<Named> {
    let all = tables(plan);
    let milestones = feature_milestones(&all);
    let wave_of = waves(&all);
    let mut out = Vec::new();
    for t in &all {
        let (Some(tests), Some(_)) = (column(t, "Tests"), column(t, "Depends on")) else {
            continue;
        };
        if column(t, "Task") != Some(0) {
            continue;
        }
        for (line, row) in &t.rows {
            let task = bare(&row[0]).to_string();
            if !is_task_id(&task) {
                continue;
            }
            // A feature the plan gives no milestone, or one after `M3`, names no test the workspace needs yet.
            if !milestones.get(&feature_of(&task)).is_some_and(|m| *m <= 3) {
                continue;
            }
            if let (Some(limit), Some(wave)) = (through_wave, wave_of.get(&task))
                && *wave > limit
            {
                continue;
            }
            for text in phrases(row.get(tests).map_or("", String::as_str)) {
                out.push(Named {
                    task: task.clone(),
                    line: *line,
                    text,
                });
            }
        }
    }
    out
}

/// Reports each named test that no function of `tests` carries the name of (`plan-names-drift`).
pub fn check_names(
    plan: &str,
    tests: &BTreeSet<String>,
    through_wave: Option<usize>,
) -> Vec<Finding> {
    named_tests(plan, through_wave)
        .into_iter()
        .filter(|n| !tests.contains(&snake(&n.text)))
        .map(|n| Finding {
            rule: "plan-names-drift",
            location: format!("PLAN.md:{} {}", n.line, n.task),
            message: format!(
                "names `{}`, and no test function is called `{}`",
                n.text,
                snake(&n.text)
            ),
        })
        .collect()
}

//! The plan check: `PLAN.md`'s task tables (§4) and wave table (§5) hold together.

use crate::table::{Table, tables};
use crate::{Finding, ids::Defined, ids::ids_in};
use std::collections::{BTreeMap, BTreeSet};

struct Task {
    line: usize,
    depends: Vec<String>,
    serves: Vec<(String, bool)>,
}

fn find(f: &mut Vec<Finding>, rule: &'static str, location: impl Into<String>, message: String) {
    f.push(Finding {
        rule,
        location: location.into(),
        message,
    });
}

fn column(t: &Table, name: &str) -> Option<usize> {
    t.header
        .iter()
        .position(|h| h.trim().eq_ignore_ascii_case(name))
}

fn bare(cell: &str) -> &str {
    cell.trim().trim_matches('`')
}

fn milestone_number(text: &str) -> Option<u32> {
    let mut found = None;
    for part in text.split('`').skip(1).step_by(2) {
        if let Some(n) = part.strip_prefix('M').and_then(|d| d.parse::<u32>().ok())
            && found.replace(n).is_some()
        {
            return None;
        }
    }
    found
}

/// The milestone of each feature: from a Milestone column, else from the one milestone in the section heading.
fn feature_milestones(all: &[Table]) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    for t in all.iter().filter(|t| column(t, "Feature") == Some(0)) {
        let by_column = column(t, "Milestone");
        for (_, row) in &t.rows {
            let milestone = match by_column {
                Some(c) => row.get(c).and_then(|cell| milestone_number(cell)),
                None => milestone_number(&t.heading3),
            };
            if let (Some(m), Some(feature)) = (milestone, row.first()) {
                out.insert(bare(feature).to_string(), m);
            }
        }
    }
    out
}

/// `T-NNN-NN`: three digits for the feature, two for the task.
fn is_task_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('-').collect();
    let digits = |p: &str, n: usize| p.len() == n && p.chars().all(|c| c.is_ascii_digit());
    parts.len() == 3 && parts[0] == "T" && digits(parts[1], 3) && digits(parts[2], 2)
}

fn feature_of(task: &str) -> String {
    let mut parts = task.split('-');
    format!("F-{}", parts.nth(1).unwrap_or_default())
}

/// Checks `PLAN.md`'s task tables (§4) and wave table (§5).
///
/// Rules: every task serves at least one defined identifier and none that is unknown or withdrawn; every
/// dependency names a task; there is no cycle; no task waits on a task of a later milestone; every task is in
/// exactly one wave, and a later one than each task it waits on.
pub fn check_plan(plan: &str, defined: &Defined) -> Vec<Finding> {
    let all = tables(plan);
    let milestones = feature_milestones(&all);
    let mut findings = Vec::new();
    let mut tasks: BTreeMap<String, Task> = BTreeMap::new();

    for t in &all {
        let (Some(dep), Some(serves)) = (column(t, "Depends on"), column(t, "Serves")) else {
            continue;
        };
        if column(t, "Task") != Some(0) {
            continue;
        }
        for (line, row) in &t.rows {
            let id = bare(&row[0]).to_string();
            if !is_task_id(&id) {
                find(
                    &mut findings,
                    "bad-task-id",
                    format!("PLAN.md:{line}"),
                    format!("task row `{id}` is not of the form T-NNN-NN"),
                );
                continue;
            }
            let task = Task {
                line: *line,
                depends: ids_in(row.get(dep).map_or("", String::as_str))
                    .into_iter()
                    .map(|(i, _)| i)
                    .collect(),
                serves: ids_in(row.get(serves).map_or("", String::as_str)),
            };
            if tasks.insert(id.clone(), task).is_some() {
                find(
                    &mut findings,
                    "duplicate-task",
                    format!("PLAN.md:{line}"),
                    format!("{id} is defined twice"),
                );
            }
        }
    }

    for (id, task) in &tasks {
        let at = format!("PLAN.md:{} {id}", task.line);
        let mut traced = false;
        for (served, narthex) in &task.serves {
            if *narthex {
                continue;
            }
            if defined.withdrawn.contains(served) {
                find(
                    &mut findings,
                    "unknown-serves",
                    &at,
                    format!("{id} serves {served}, which is withdrawn"),
                );
            } else if defined.is_defined(served) {
                traced = true;
            } else {
                find(
                    &mut findings,
                    "unknown-serves",
                    &at,
                    format!("{id} serves {served}, which no document defines"),
                );
            }
        }
        if !traced {
            find(
                &mut findings,
                "no-serves",
                &at,
                format!("{id} serves no defined identifier"),
            );
        }
        for dep in &task.depends {
            let Some(target) = tasks.get(dep) else {
                find(
                    &mut findings,
                    "unknown-dependency",
                    &at,
                    format!("{id} waits on {dep}, which is not a task"),
                );
                continue;
            };
            let _ = target;
            match (
                milestones.get(&feature_of(id)),
                milestones.get(&feature_of(dep)),
            ) {
                (Some(mine), Some(theirs)) if theirs > mine => find(
                    &mut findings,
                    "later-milestone",
                    &at,
                    format!("{id} (M{mine}) waits on {dep} (M{theirs}), a later milestone"),
                ),
                (Some(_), Some(_)) => {}
                _ => find(
                    &mut findings,
                    "unknown-milestone",
                    &at,
                    format!("the milestone of {id} or {dep} is not in the feature tables"),
                ),
            }
        }
    }

    findings.extend(cycles(&tasks));
    findings.extend(waves(&all, &tasks));
    findings
}

/// One finding per distinct cycle, found by depth-first search.
fn cycles(tasks: &BTreeMap<String, Task>) -> Vec<Finding> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Open,
        Done,
    }
    fn visit(
        id: &str,
        tasks: &BTreeMap<String, Task>,
        marks: &mut BTreeMap<String, Mark>,
        path: &mut Vec<String>,
        seen: &mut BTreeSet<Vec<String>>,
        out: &mut Vec<Finding>,
    ) {
        marks.insert(id.to_string(), Mark::Open);
        path.push(id.to_string());
        for dep in &tasks[id].depends {
            if !tasks.contains_key(dep) {
                continue;
            }
            match marks.get(dep) {
                Some(Mark::Open) => {
                    let start = path
                        .iter()
                        .position(|p| p == dep)
                        .expect("open task is on the path");
                    let mut ring: Vec<String> = path[start..].to_vec();
                    let low = ring
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, s)| (*s).clone())
                        .map_or(0, |(i, _)| i);
                    ring.rotate_left(low);
                    if seen.insert(ring.clone()) {
                        let shown = ring
                            .iter()
                            .chain(ring.first())
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(" -> ");
                        out.push(Finding {
                            rule: "cycle",
                            location: format!("PLAN.md:{} {}", tasks[&ring[0]].line, ring[0]),
                            message: format!("dependency cycle: {shown}"),
                        });
                    }
                }
                Some(Mark::Done) => {}
                None => visit(dep, tasks, marks, path, seen, out),
            }
        }
        path.pop();
        marks.insert(id.to_string(), Mark::Done);
    }
    let (mut marks, mut seen, mut out) = (BTreeMap::new(), BTreeSet::new(), Vec::new());
    for id in tasks.keys() {
        if !marks.contains_key(id) {
            visit(id, tasks, &mut marks, &mut Vec::new(), &mut seen, &mut out);
        }
    }
    out
}

/// §5's wave table against the dependencies: every task once, each wave after its dependencies'.
fn waves(all: &[Table], tasks: &BTreeMap<String, Task>) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut wave_of: BTreeMap<String, usize> = BTreeMap::new();
    let mut seen_table = false;
    for t in all
        .iter()
        .filter(|t| column(t, "Wave") == Some(0) && column(t, "Tasks") == Some(1))
    {
        seen_table = true;
        for (line, row) in &t.rows {
            let Ok(wave) = bare(&row[0]).parse::<usize>() else {
                continue;
            };
            for (id, _) in ids_in(row.get(1).map_or("", String::as_str)) {
                if !tasks.contains_key(&id) {
                    find(
                        &mut out,
                        "wave-unknown",
                        format!("PLAN.md:{line} wave {wave}"),
                        format!("{id} is in the waves but is not a task"),
                    );
                } else if wave_of.insert(id.clone(), wave).is_some() {
                    find(
                        &mut out,
                        "wave-duplicate",
                        format!("PLAN.md:{line} wave {wave}"),
                        format!("{id} is in more than one wave"),
                    );
                }
            }
        }
    }
    if !seen_table {
        find(
            &mut out,
            "wave-missing",
            "PLAN.md",
            "no wave table (`| Wave | Tasks |`) found".into(),
        );
        return out;
    }
    for (id, task) in tasks {
        let Some(mine) = wave_of.get(id) else {
            find(
                &mut out,
                "wave-missing",
                format!("PLAN.md:{} {id}", task.line),
                format!("{id} is in no wave"),
            );
            continue;
        };
        for dep in &task.depends {
            if let Some(theirs) = wave_of.get(dep)
                && theirs >= mine
            {
                find(
                    &mut out,
                    "wave-order",
                    format!("PLAN.md:{} {id}", task.line),
                    format!("{id} is in wave {mine} but waits on {dep} in wave {theirs}"),
                );
            }
        }
    }
    out
}

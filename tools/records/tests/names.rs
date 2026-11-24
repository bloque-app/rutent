//! Tests of the names check (`T-024-24`), on fixtures and on the real plan and workspace.

use rutent_records::names::{check_names, named_tests, snake};
use rutent_records::tags::{find_rust_files, scan_tests};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A plan of two `M1`-to-`M3` features and one of `M4`; each row is `(task, tests cell)`.
fn plan(rows: &[(&str, &str)], waves: &[(u32, &str)]) -> String {
    let mut lines: Vec<String> = [
        "### 4.1 `M1` features",
        "",
        "| Feature | Title | Tasks |",
        "|---|---|---|",
        "| `F-001` | One | 2 |",
        "",
        "### 4.2 `M2` to `M5` features",
        "",
        "| Feature | Milestone | Title |",
        "|---|---|---|",
        "| `F-010` | `M2` | Later |",
        "| `F-031` | `M4` | Production |",
        "",
        "| Task | Work | Depends on | Serves | Tests | Done when |",
        "|---|---|---|---|---|---|",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    for (task, tests) in rows {
        lines.push(format!(
            "| `{task}` | work | none | `DEC-21` | {tests} | done |"
        ));
    }
    lines.extend(["".into(), "| Wave | Tasks |".into(), "|---|---|".into()]);
    for (wave, ids) in waves {
        lines.push(format!("| {wave} | {ids} |"));
    }
    lines.join("\n")
}

fn names(tests: &[&str]) -> BTreeSet<String> {
    tests.iter().map(ToString::to_string).collect()
}

fn found(text: &str, tests: &[&str], through: Option<usize>) -> Vec<String> {
    check_names(text, &names(tests), through)
        .iter()
        .map(ToString::to_string)
        .collect()
}

// verifies: DEC-21
#[test]
fn a_name_is_snake_cased_with_its_apostrophes_dropped() {
    assert_eq!(
        snake("operator's fee is shown apart from the quotes' sum"),
        "operators_fee_is_shown_apart_from_the_quotes_sum"
    );
    assert_eq!(snake("a  b-c, d"), "a_b_c_d");
}

// verifies: DEC-21
#[test]
fn a_named_test_no_function_carries_is_reported_with_its_row() {
    let text = plan(
        &[(
            "T-001-01",
            "`one thing holds` [`DEC-21`]; `another thing holds` [`DEC-21`]",
        )],
        &[(0, "`T-001-01`")],
    );
    let got = found(&text, &["one_thing_holds"], None);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].starts_with("plan-names-drift"), "{got:?}");
    assert!(
        got[0].contains("T-001-01") && got[0].contains("another thing holds"),
        "{got:?}"
    );
    assert!(found(&text, &["one_thing_holds", "another_thing_holds"], None).is_empty());
}

// verifies: DEC-21
#[test]
fn only_a_phrase_outside_brackets_and_parentheses_is_a_named_test() {
    let text = plan(
        &[(
            "T-001-01",
            "`std`; `T-001-02`; `a real test holds` [`DEC-21`, `AC-1`] (renamed on `some branch name`, from \"x\")",
        )],
        &[(0, "`T-001-01`")],
    );
    let got = named_tests(&text, None);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].text, "a real test holds");
}

// verifies: DEC-21
#[test]
fn a_row_of_a_later_milestone_names_no_test_the_workspace_needs() {
    let text = plan(
        &[("T-031-01", "`production test that is not built` [`DEC-21`]")],
        &[(0, "`T-031-01`")],
    );
    assert!(found(&text, &[], None).is_empty());
}

// verifies: DEC-21
#[test]
fn a_row_after_the_given_wave_is_left_out() {
    let text = plan(
        &[
            ("T-001-01", "`built test` [`DEC-21`]"),
            ("T-001-02", "`unbuilt test` [`DEC-21`]"),
        ],
        &[(3, "`T-001-01`"), (4, "`T-001-02`")],
    );
    let all = found(&text, &[], None);
    assert_eq!(all.len(), 2, "{all:?}");
    let early = found(&text, &[], Some(3));
    assert_eq!(early.len(), 1, "{early:?}");
    assert!(early[0].contains("T-001-01"), "{early:?}");
}

// verifies: DEC-21
#[test]
fn every_test_the_plan_names_is_a_test_of_the_workspace() {
    let root = root();
    let plan = std::fs::read_to_string(root.join("PLAN.md")).expect("PLAN.md is readable");
    let mut tests = BTreeSet::new();
    for path in find_rust_files(&root).expect("sources are readable") {
        let text = std::fs::read_to_string(&path).expect("source is readable");
        tests.extend(scan_tests("", &text).into_iter().map(|t| t.name));
    }
    // A floor, so that a plan whose tables changed shape and names nothing is not a pass.
    assert!(
        rutent_records::names::named_tests(&plan, Some(38)).len() > 500,
        "the plan names its tests in its task rows' Tests cells"
    );
    // Waves 39 and later are rows other branches write; the exit's third run holds them to the whole plan.
    let drift: Vec<String> = check_names(&plan, &tests, Some(38))
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(drift.is_empty(), "{drift:#?}");
}

//! Tests of the tag check and the plan check, on fixtures and on the real repository.

use rutent_records::ids::{Defined, load_defined};
use rutent_records::plan::check_plan;
use rutent_records::tags::{check_tests, find_rust_files, scan_tests};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Lines joined into one document, so a fixture never has a line of its own that reads as a real test.
fn doc(lines: &[&str]) -> String {
    lines.join("\n")
}

fn spec() -> String {
    doc(&[
        "### `REQ-AUZ-*`: authorisation and policy",
        "",
        "| ID | Statement |",
        "|---|---|",
        "| `REQ-AUZ-1` | Policy MUST decide. |",
        "| `REQ-AUZ-2` | Roles MUST differ. |",
        "",
        "Number 10 in this family is withdrawn (R115), and isn't reused.",
        "",
        "### `REQ-MOD-*`: modules",
        "",
        "| ID | Statement |",
        "|---|---|",
        "| `REQ-MOD-1` | A library MUST stand alone. |",
    ])
}

fn defined(plan: &str) -> Defined {
    let adr = "# ADR-0001: Core's decomposition\n";
    Defined::collect(&spec(), &[adr], plan, "")
}

fn tag_findings(source: &[&str]) -> Vec<String> {
    let tests = scan_tests("fixture.rs", &doc(source));
    check_tests(&tests, &defined(""))
        .iter()
        .map(ToString::to_string)
        .collect()
}

// verifies: DEC-21
#[test]
fn test_without_a_verifies_comment_is_reported() {
    let found = tag_findings(&["#[test]", "fn lonely() {}"]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("untagged-test"), "{found:?}");
    assert!(found[0].contains("fixture.rs:1"), "{found:?}");
}

// verifies: DEC-21
#[test]
fn verifies_comment_naming_an_unknown_id_is_reported() {
    let found = tag_findings(&[
        "// verifies: REQ-MOD-1, REQ-MOD-99",
        "#[test]",
        "fn typo() {}",
    ]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("unknown-id"), "{found:?}");
    assert!(found[0].contains("REQ-MOD-99"), "{found:?}");
}

// verifies: DEC-21
#[test]
fn withdrawn_id_is_reported() {
    let found = tag_findings(&["// verifies: REQ-AUZ-10", "#[test]", "fn old() {}"]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("withdrawn-id"), "{found:?}");
    assert!(found[0].contains("REQ-AUZ-10"), "{found:?}");
}

// verifies: DEC-21
#[test]
fn tagged_tests_with_attributes_between_pass() {
    let found = tag_findings(&[
        "// verifies: REQ-AUZ-1, ADR-0001",
        "#[test]",
        "#[should_panic]",
        "fn first() {}",
        "",
        "// verifies: REQ-MOD-1",
        "#[ignore]",
        "#[test]",
        "fn second() {}",
    ]);
    assert!(found.is_empty(), "{found:?}");
}

// verifies: DEC-21
#[test]
fn a_comment_that_is_not_directly_above_does_not_count() {
    let found = tag_findings(&["// verifies: REQ-MOD-1", "", "#[test]", "fn gap() {}"]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("untagged-test"), "{found:?}");
}

const PLAN_HEAD: [&str; 10] = [
    "### 4.1 `M1` features",
    "",
    "| Feature | Title | Tasks |",
    "|---|---|---|",
    "| `F-001` | One | 2 |",
    "| `F-002` | Two | 1 |",
    "",
    "### 4.2 `M2` to `M3` features",
    "",
    "| Feature | Milestone | Title |",
];

/// A plan from task rows `(task, depends on, serves)` and waves `(wave, tasks)`.
fn plan(tasks: &[(&str, &str, &str)], waves: &[(u32, &str)]) -> String {
    let mut lines: Vec<String> = PLAN_HEAD.iter().map(ToString::to_string).collect();
    lines.extend([
        "|---|---|---|".into(),
        "| `F-010` | `M2` | Later |".into(),
        "".into(),
        "#### `F-001` One".into(),
        "".into(),
        "| Task | Work | Depends on | Serves | Tests | Done when |".into(),
        "|---|---|---|---|---|---|".into(),
    ]);
    for (task, deps, serves) in tasks {
        lines.push(format!(
            "| `{task}` | work | {deps} | {serves} | a test | done |"
        ));
    }
    lines.extend(["".into(), "| Wave | Tasks |".into(), "|---|---|".into()]);
    for (wave, ids) in waves {
        lines.push(format!("| {wave} | {ids} |"));
    }
    lines.join("\n")
}

fn plan_findings(plan_text: &str) -> Vec<String> {
    check_plan(plan_text, &defined(plan_text))
        .iter()
        .map(ToString::to_string)
        .collect()
}

fn good_plan() -> String {
    plan(
        &[
            ("T-001-01", "none", "`REQ-MOD-1`"),
            (
                "T-001-02",
                "`T-001-01`",
                "`REQ-AUZ-1`, Narthex `REQ-CHN-12`",
            ),
            ("T-002-01", "`T-001-02`", "ADR-0001"),
        ],
        &[(0, "`T-001-01`"), (1, "`T-001-02`"), (2, "`T-002-01`")],
    )
}

// verifies: DEC-21
#[test]
fn a_consistent_plan_passes() {
    let found = plan_findings(&good_plan());
    assert!(found.is_empty(), "{found:?}");
}

// verifies: DEC-21
#[test]
fn task_depending_on_a_later_milestone_is_reported() {
    // T-001-01 (M1) waits on a task of F-010 (M2).
    let text = plan(
        &[
            ("T-001-01", "`T-010-01`", "`REQ-MOD-1`"),
            ("T-010-01", "none", "`REQ-MOD-1`"),
        ],
        &[(0, "`T-010-01`"), (1, "`T-001-01`")],
    );
    let found = plan_findings(&text);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("later-milestone"), "{found:?}");
    assert!(
        found[0].contains("T-001-01") && found[0].contains("T-010-01"),
        "{found:?}"
    );
}

// verifies: DEC-21
#[test]
fn dependency_cycle_is_reported() {
    let text = plan(
        &[
            ("T-001-01", "`T-001-02`", "`REQ-MOD-1`"),
            ("T-001-02", "`T-001-01`", "`REQ-MOD-1`"),
        ],
        &[(0, "`T-001-01`"), (1, "`T-001-02`")],
    );
    let found = plan_findings(&text);
    assert!(found.iter().any(|f| f.starts_with("cycle")), "{found:?}");
    assert!(
        found
            .iter()
            .any(|f| f.contains("T-001-01") && f.contains("T-001-02")),
        "{found:?}"
    );
}

// verifies: DEC-21
#[test]
fn task_serving_nothing_defined_is_reported() {
    let text = plan(
        &[
            ("T-001-01", "none", "`REQ-MOD-77`"),
            ("T-001-02", "none", "Narthex `REQ-CHN-12`"),
        ],
        &[(0, "`T-001-01`, `T-001-02`")],
    );
    let found = plan_findings(&text);
    assert!(
        found
            .iter()
            .any(|f| f.starts_with("unknown-serves") && f.contains("REQ-MOD-77")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|f| f.starts_with("no-serves") && f.contains("T-001-02")),
        "{found:?}"
    );
}

// verifies: DEC-21
#[test]
fn wave_not_after_its_dependency_is_reported() {
    let text = plan(
        &[
            ("T-001-01", "none", "`REQ-MOD-1`"),
            ("T-001-02", "`T-001-01`", "`REQ-MOD-1`"),
        ],
        &[(1, "`T-001-01`, `T-001-02`")],
    );
    let found = plan_findings(&text);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("wave-order"), "{found:?}");
}

// verifies: DEC-21
#[test]
fn task_missing_from_the_waves_is_reported() {
    let text = plan(&[("T-001-01", "none", "`REQ-MOD-1`")], &[(0, "`T-009-09`")]);
    let found = plan_findings(&text);
    assert!(
        found
            .iter()
            .any(|f| f.starts_with("wave-missing") && f.contains("T-001-01")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|f| f.starts_with("wave-unknown") && f.contains("T-009-09")),
        "{found:?}"
    );
}

// verifies: DEC-21
#[test]
fn the_real_plan_passes_the_plan_check() {
    let root = root();
    let defined = load_defined(&root).expect("documents are readable");
    let text = std::fs::read_to_string(root.join("PLAN.md")).expect("PLAN.md is readable");
    let found: Vec<String> = check_plan(&text, &defined)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

// verifies: DEC-21
#[test]
fn the_real_documents_define_and_withdraw_what_they_say() {
    let defined = load_defined(&root()).expect("documents are readable");
    for id in [
        "REQ-MOD-1",
        "ADR-0014",
        "DEC-21",
        "T-001-03",
        "F-001",
        "M1",
        "C1",
        "EXP-1",
        "INV-1",
    ] {
        assert!(defined.is_defined(id), "{id} should be defined");
    }
    assert!(defined.withdrawn.contains("REQ-AUZ-10"));
    assert!(!defined.is_defined("REQ-AUZ-10"));
}

// verifies: DEC-21
#[test]
fn the_real_workspace_has_no_untagged_test() {
    let root = root();
    let defined = load_defined(&root).expect("documents are readable");
    let mut tests = Vec::new();
    for path in find_rust_files(&root).expect("sources are readable") {
        let text = std::fs::read_to_string(&path).expect("source is readable");
        tests.extend(scan_tests(&path.display().to_string(), &text));
    }
    assert!(
        tests.len() >= 10,
        "the walk should find the workspace's tests"
    );
    let found: Vec<String> = check_tests(&tests, &defined)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

// verifies: DEC-21
#[test]
fn untagged_attribute_path_tests_are_reported() {
    for attr in [
        "#[tokio::test]",
        "#[tokio::test(flavor = \"multi_thread\")]",
        "#[rstest]",
        "#[test_case(1, 2)]",
        "#[test_case]",
    ] {
        let found = tag_findings(&[attr, "async fn lonely() {}"]);
        assert_eq!(found.len(), 1, "{attr}: {found:?}");
        assert!(found[0].starts_with("untagged-test"), "{attr}: {found:?}");
        assert!(found[0].contains("lonely"), "{attr}: {found:?}");
    }
    let tagged = tag_findings(&[
        "// verifies: REQ-MOD-1",
        "#[tokio::test]",
        "async fn fine() {}",
    ]);
    assert!(tagged.is_empty(), "{tagged:?}");
    let not_tests = tag_findings(&[
        "#[derive(Debug)]",
        "#[cfg(test)]",
        "#[tests_helper]",
        "fn helper() {}",
    ]);
    assert!(not_tests.is_empty(), "{not_tests:?}");
}

// verifies: DEC-21
#[test]
fn test_whose_attribute_follows_other_code_on_its_line_is_found_and_needs_its_tag() {
    // Each of these is one test with no tag, whatever stands before its attribute on the line.
    for (source, name) in [
        ("mod t { #[test] fn inside() {} }", "inside"),
        (
            "#[allow(dead_code)] #[test] fn after_attr() {}",
            "after_attr",
        ),
        ("#[test] fn bare() {}", "bare"),
        ("#[tokio::test] async fn runs() {}", "runs"),
        ("#[cfg(test)] mod t { #[test] fn nested() {} }", "nested"),
        ("fn helper() {} #[test] fn after_code() {}", "after_code"),
    ] {
        let found = tag_findings(&[source]);
        assert_eq!(found.len(), 1, "{source}: {found:?}");
        assert!(found[0].starts_with("untagged-test"), "{source}: {found:?}");
        assert!(found[0].contains(name), "{source}: {found:?}");
    }
    // Two tests on a line are two tests; a tag above the line tags only the first of two lines' tests when it is
    // not directly above the second.
    let two = tag_findings(&["#[test] fn first() {} #[test] fn second() {}"]);
    assert_eq!(two.len(), 2, "{two:?}");
    let sequence = tag_findings(&[
        "// verifies: REQ-MOD-1",
        "#[test] fn tagged() {}",
        "#[test] fn untagged() {}",
    ]);
    assert_eq!(sequence.len(), 1, "{sequence:?}");
    assert!(sequence[0].contains("untagged"), "{sequence:?}");
    // A tag above a line of attributes tags the test that ends it, and a tagged same-line test passes.
    let tagged = tag_findings(&[
        "// verifies: REQ-MOD-1",
        "#[allow(dead_code)] #[test] fn fine() {}",
    ]);
    assert!(tagged.is_empty(), "{tagged:?}");
    // The module of a test on the line that opens its module is that module.
    let modules = |source: &str| scan_tests("a.rs", source).remove(0).module;
    assert_eq!(modules("mod t { #[test] fn x() {} }"), ["t"]);
    assert_eq!(
        modules("#[cfg(test)] mod tests { #[test] fn x() {} }"),
        ["tests"]
    );
    assert_eq!(modules("mod a { mod b { #[test] fn x() {} } }"), ["a", "b"]);
    assert_eq!(
        modules("mod a {\n    fn f() {}\n}\n#[test] fn x() {}"),
        Vec::<String>::new()
    );
    // Nothing that is not a test is counted: a literal, a comment, another attribute, a function named for one.
    let not_tests = tag_findings(&[
        "fn f() { let s = \"#[test] fn x() {}\"; }",
        "// #[test] fn commented() {}",
        "/// #[test] fn documented() {}",
        "#[derive(Debug)] struct S;",
        "#[cfg(test)] mod t { fn helper() {} }",
        "#[tests_helper] fn h() {}",
        "#![cfg(test)]",
    ]);
    assert!(not_tests.is_empty(), "{not_tests:?}");
}

// verifies: DEC-21
#[test]
fn tests_of_an_excluded_crate_still_need_their_tags() {
    let dir = std::env::temp_dir().join(format!("rutent-records-excluded-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let write = |path: &str, text: &str| {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"a\"]\nexclude = [\"fixtures/f\"]\n",
    );
    write("a/Cargo.toml", "[package]\nname = \"a\"\n");
    write("fixtures/f/Cargo.toml", "[package]\nname = \"f\"\n");
    write("fixtures/f/tests/t.rs", "#[test]\nfn lonely() {}\n");
    let mut tests = Vec::new();
    for path in find_rust_files(&dir).expect("sources are readable") {
        let text = std::fs::read_to_string(&path).expect("source is readable");
        let name = path.strip_prefix(&dir).unwrap().display().to_string();
        tests.extend(scan_tests(&name, &text));
    }
    std::fs::remove_dir_all(&dir).unwrap();
    let found: Vec<String> = check_tests(&tests, &defined(""))
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("fixtures/f/tests/t.rs"), "{found:?}");
    assert!(found[0].contains("lonely"), "{found:?}");
}

// verifies: DEC-21
#[test]
fn malformed_task_id_is_reported() {
    let text = plan(
        &[
            ("T-001-01", "none", "`REQ-MOD-1`"),
            ("T-001-4", "none", "`REQ-MOD-1`"),
        ],
        &[(0, "`T-001-01`")],
    );
    let found = plan_findings(&text);
    assert!(
        found
            .iter()
            .any(|f| f.starts_with("bad-task-id") && f.contains("T-001-4")),
        "{found:?}"
    );
}

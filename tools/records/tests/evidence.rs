//! Tests of the requirement-to-evidence table (`T-024-09`), on fixtures and on the real scope file.

use rutent_records::Finding;
use rutent_records::evidence::{Inputs, Report, Results, build, compare, parse_scope, universe};
use rutent_records::ids::Defined;
use rutent_records::tags::scan_tests;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Lines joined into one document, so a fixture never has a line of its own that reads as a real test.
fn doc(lines: &[&str]) -> String {
    lines.join("\n")
}

const IDS: [&str; 9] = [
    "REQ-MOD-1",
    "REQ-MOD-2",
    "REQ-MOD-3",
    "REQ-OPS-4",
    "REQ-OPS-5",
    "INV-1",
    "AC-1",
    "NFR-1",
    "GUA-6",
];

fn spec() -> String {
    let mut lines = vec![
        "### `REQ-MOD-*`: modules".to_string(),
        String::new(),
        "| ID | Statement |".to_string(),
        "|---|---|".to_string(),
    ];
    for id in IDS {
        lines.push(format!("| `{id}` | A statement. |"));
    }
    lines.push(String::new());
    lines.push("| ID | Assumption |".to_string());
    lines.push("|---|---|".to_string());
    lines.push("| `ASM-1` | No number. |".to_string());
    lines.join("\n")
}

/// A plan whose decisions, placement table and feature rows are the ones a row may cite.
fn plan(dec16: &str, placement: &str, feature: &str) -> String {
    doc(&[
        "## 1. Decisions",
        "",
        "| ID | Decision |",
        "|---|---|",
        "| `DEC-8` | The router is pure. |",
        &format!("| `DEC-16` | {dec16} |"),
        "| `DEC-17` | Always in v0. |",
        "",
        "### 4.2 `M2` to `M5` features",
        "",
        "| Feature | Milestone | Name | Serves |",
        "|---|---|---|---|",
        &format!("| `F-030` | `M4` | Standalone | {feature} |"),
        "| `F-011` | `M2` | Router | `REQ-MOD-2` |",
        "| `F-033` | `M4` | Operations | `REQ-MOD-1` |",
        "",
        "### 7.1 Placement by family",
        "",
        "| Family | Placed in | Deferred |",
        "|---|---|---|",
        &format!("| `REQ-OPS` | {placement} | none |"),
    ])
}

fn plain_plan() -> String {
    plan(
        "Verifiers wait.",
        "`M3`: `REQ-OPS-4`, `REQ-OPS-5`",
        "`REQ-OPS-3`",
    )
}

/// The scope file that holds every fixture identifier as `M3`'s, with lines of its own in place of some.
fn scope_text(switch: &str, replace: &[&str]) -> String {
    let mut lines = vec![format!("strict: {switch}")];
    for id in IDS {
        match replace.iter().find(|l| l.starts_with(&format!("{id} |"))) {
            Some(line) => lines.push((*line).to_string()),
            None => lines.push(format!("{id} | m3")),
        }
    }
    lines.join("\n")
}

/// A passing test for each identifier, but the ones named in `without`.
fn sources(without: &[&str]) -> String {
    let mut lines = Vec::new();
    for id in IDS.iter().filter(|id| !without.contains(id)) {
        lines.push(format!("// verifies: {id}"));
        lines.push("#[test]".to_string());
        lines.push(format!(
            "fn checks_{}() {{}}",
            id.to_lowercase().replace('-', "_")
        ));
    }
    lines.join("\n")
}

fn all_pass() -> String {
    IDS.iter()
        .map(|id| {
            format!(
                "test checks::checks_{} ... ok",
                id.to_lowercase().replace('-', "_")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn report(scope: &str, source: &str, results: &str, review_notes: bool) -> (Report, Vec<Finding>) {
    let (parsed, problems) = parse_scope(scope);
    let spec = spec();
    let defined = Defined::collect(&spec, &[], &plain_plan(), "");
    let universe = universe(&spec);
    let tests = scan_tests("fixture.rs", source);
    let results = Results::parse(results);
    let exists = move |_: &str| review_notes;
    let inputs = Inputs {
        universe: &universe,
        defined: &defined,
        tests: &tests,
        results: &results,
        review_exists: &exists,
    };
    (build(&parsed, &inputs), problems)
}

fn table_findings(scope: &str, source: &str, results: &str) -> Vec<String> {
    let (report, problems) = report(scope, source, results, true);
    problems
        .iter()
        .chain(report.table.iter())
        .map(ToString::to_string)
        .collect()
}

fn ratchet(head: &str, base: &str, head_plan: &str, base_plan: &str) -> Vec<String> {
    let (head, _) = parse_scope(head);
    let (base, _) = parse_scope(base);
    compare(&head, &base, head_plan, base_plan)
        .iter()
        .map(ToString::to_string)
        .collect()
}

fn some_with(found: &[String], rule: &str, needle: &str) -> bool {
    found
        .iter()
        .any(|f| f.starts_with(rule) && f.contains(needle))
}

// verifies: DEC-17
#[test]
fn a_fixture_with_everything_in_place_has_no_finding() {
    let found = table_findings(&scope_text("off", &[]), &sources(&[]), &all_pass());
    assert!(found.is_empty(), "{found:?}");
}

// verifies: DEC-21
#[test]
fn scope_file_missing_a_spec_identifier_is_reported() {
    let scope = scope_text("off", &[])
        .lines()
        .filter(|l| !l.starts_with("INV-1 |"))
        .collect::<Vec<_>>()
        .join("\n");
    let found = table_findings(&scope, &sources(&[]), &all_pass());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(some_with(&found, "missing-id", "INV-1"), "{found:?}");
    let stale = format!("{}\nREQ-MOD-77 | m3", scope_text("off", &[]));
    let found = table_findings(&stale, &sources(&[]), &all_pass());
    assert!(some_with(&found, "unknown-id", "REQ-MOD-77"), "{found:?}");
    let twice = format!("{}\nINV-1 | m3", scope_text("off", &[]));
    let found = table_findings(&twice, &sources(&[]), &all_pass());
    assert!(some_with(&found, "duplicate-row", "INV-1"), "{found:?}");
}

// verifies: DEC-21
#[test]
fn m3_row_whose_only_tests_failed_is_reported() {
    let results = all_pass().replace(
        "test checks::checks_ac_1 ... ok",
        "test checks::checks_ac_1 ... FAILED",
    );
    let found = table_findings(&scope_text("off", &[]), &sources(&[]), &results);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        some_with(&found, "no-passing-verifier", "AC-1"),
        "{found:?}"
    );
    assert!(found[0].contains("failed"), "{found:?}");
    for (label, line) in [
        ("ignored", "test checks::checks_ac_1 ... ignored"),
        ("not run", ""),
    ] {
        let results = all_pass().replace("test checks::checks_ac_1 ... ok", line);
        let found = table_findings(&scope_text("off", &[]), &sources(&[]), &results);
        assert!(
            some_with(&found, "no-passing-verifier", "AC-1") && found[0].contains(label),
            "{label}: {found:?}"
        );
    }
    let two = format!(
        "{}\n// verifies: AC-1\n#[test]\nfn also_checks_ac_one() {{}}",
        sources(&[])
    );
    let results = format!(
        "{}\ntest checks::also_checks_ac_one ... ok",
        all_pass().replace(
            "test checks::checks_ac_1 ... ok",
            "test checks::checks_ac_1 ... FAILED",
        )
    );
    let found = table_findings(&scope_text("off", &[]), &two, &results);
    assert!(
        found.is_empty(),
        "one passing test is a verifier: {found:?}"
    );
}

// verifies: DEC-21
#[test]
fn m3_row_with_no_tagged_test_is_reported() {
    let found = table_findings(&scope_text("off", &[]), &sources(&["GUA-6"]), &all_pass());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(some_with(&found, "no-tagged-test", "GUA-6"), "{found:?}");
}

// verifies: DEC-17
#[test]
fn deferred_row_without_its_plan_decision_is_reported() {
    let without = scope_text("off", &["NFR-1 | deferred"]);
    let found = table_findings(&without, &sources(&["NFR-1"]), &all_pass());
    assert!(
        some_with(&found, "deferred-without-decision", "NFR-1"),
        "{found:?}"
    );
    let unknown = scope_text("off", &["NFR-1 | deferred | DEC-99"]);
    let found = table_findings(&unknown, &sources(&["NFR-1"]), &all_pass());
    assert!(
        some_with(&found, "deferred-without-decision", "NFR-1"),
        "{found:?}"
    );
    for cite in ["DEC-16", "ASM-1 6.2:risk-9", "F-030 7.1:REQ-OPS"] {
        let cited = scope_text("off", &[&format!("NFR-1 | deferred | {cite}")]);
        let found = table_findings(&cited, &sources(&["NFR-1"]), &all_pass());
        assert!(found.is_empty(), "{cite}: {found:?}");
    }
}

// verifies: DEC-17
#[test]
fn partly_met_row_without_its_form_item_is_reported() {
    let without = scope_text("off", &["REQ-MOD-1 | partly"]);
    let found = table_findings(&without, &sources(&["REQ-MOD-1"]), &all_pass());
    assert!(
        some_with(&found, "partly-without-item", "REQ-MOD-1"),
        "{found:?}"
    );
    let named = scope_text("off", &["REQ-MOD-1 | partly | form:cat5-timing-residual"]);
    let (report, problems) = report(&named, &sources(&["REQ-MOD-1"]), &all_pass(), true);
    assert!(problems.is_empty() && report.table.is_empty(), "{report:?}");
    let line = report.lines.iter().find(|l| l.id == "REQ-MOD-1").unwrap();
    assert_eq!(
        line.status, "partly met",
        "listed as partly met, not verified"
    );
}

// verifies: DEC-21
#[test]
fn inspection_row_naming_no_check_or_review_is_reported() {
    let without = scope_text("off", &["REQ-MOD-2 | inspect"]);
    let found = table_findings(&without, &sources(&["REQ-MOD-2"]), &all_pass());
    assert!(
        some_with(&found, "inspection-unnamed", "REQ-MOD-2"),
        "{found:?}"
    );
    for cite in ["check:vibes", "review:F-099", "review:INV-1"] {
        let scope = scope_text("off", &[&format!("REQ-MOD-2 | inspect | {cite}")]);
        let found = table_findings(&scope, &sources(&["REQ-MOD-2"]), &all_pass());
        assert!(
            some_with(&found, "inspection-unnamed", "REQ-MOD-2"),
            "{cite}: {found:?}"
        );
    }
    for cite in [
        "check:documentation",
        "check:boundary",
        "check:licence",
        "review:F-011",
    ] {
        let scope = scope_text("off", &[&format!("REQ-MOD-2 | inspect | {cite}")]);
        let found = table_findings(&scope, &sources(&["REQ-MOD-2"]), &all_pass());
        assert!(found.is_empty(), "{cite}: {found:?}");
    }
    let scope = scope_text("off", &["REQ-MOD-2 | inspect | review:F-011"]);
    let (report, _) = report(&scope, &sources(&["REQ-MOD-2"]), &all_pass(), false);
    assert!(
        report.table.iter().any(|f| f.rule == "inspection-unnamed"),
        "a review whose notes are absent verifies nothing: {report:?}"
    );
}

// verifies: DEC-17
#[test]
fn strict_switch_turned_off_after_develop_has_it_on_fails() {
    let (on, off) = (scope_text("on", &[]), scope_text("off", &[]));
    let plan = plain_plan();
    let found = ratchet(&off, &on, &plan, &plan);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("strict-switch-off"), "{found:?}");
    assert!(ratchet(&on, &on, &plan, &plan).is_empty());
    assert!(ratchet(&on, &off, &plan, &plan).is_empty());
    assert!(ratchet(&off, &off, &plan, &plan).is_empty());
}

// verifies: DEC-17
#[test]
fn m3_row_reclassified_against_develop_is_reported() {
    let base = scope_text("off", &[]);
    let plan = plain_plan();
    for line in [
        "REQ-MOD-3 | deferred | DEC-16",
        "REQ-OPS-4 | m4 | F-030 7.1:REQ-OPS",
    ] {
        let head = scope_text("off", &[line]);
        let found = ratchet(&head, &base, &plan, &plan);
        let id = line.split(' ').next().unwrap();
        assert_eq!(found.len(), 1, "{line}: {found:?}");
        assert!(some_with(&found, "reclassified", id), "{found:?}");
    }
    // A partly met or inspected row is still M3's, and a row that was never M3's moves freely.
    let head = scope_text("off", &["REQ-MOD-3 | partly | form:cat5-timing-residual"]);
    assert!(ratchet(&head, &base, &plan, &plan).is_empty());
    let was_m4 = scope_text("off", &["REQ-MOD-3 | deferred | DEC-16"]);
    let now_deferred = scope_text("off", &["REQ-MOD-3 | deferred | DEC-8"]);
    assert!(ratchet(&now_deferred, &was_m4, &plan, &plan).is_empty());
}

// verifies: DEC-17
#[test]
fn reclassification_is_excused_only_by_a_changed_placement_that_it_cites() {
    let base_plan = plain_plan();
    let base = scope_text("off", &[]);

    // A deferral: the decision it cites now names the identifier.
    let head_plan = plan(
        "Verifiers wait. Deferred: `REQ-MOD-3`.",
        "`M3`: `REQ-OPS-4`, `REQ-OPS-5`",
        "`REQ-OPS-3`",
    );
    let head = scope_text("off", &["REQ-MOD-3 | deferred | DEC-16"]);
    let found = ratchet(&head, &base, &head_plan, &base_plan);
    assert!(found.is_empty(), "{found:?}");
    // ... but not a decision it cites that was left as it was,
    let head = scope_text("off", &["REQ-MOD-3 | deferred | DEC-8"]);
    let found = ratchet(&head, &base, &head_plan, &base_plan);
    assert!(some_with(&found, "reclassified", "REQ-MOD-3"), "{found:?}");
    // nor one that the entry doesn't cite at all.
    let head = scope_text("off", &["REQ-MOD-3 | deferred | ASM-1"]);
    let found = ratchet(&head, &base, &head_plan, &base_plan);
    assert!(some_with(&found, "reclassified", "REQ-MOD-3"), "{found:?}");

    // A move to M4: the family's row of §7.1 and the feature's row, each saying `M4` and naming it anew.
    let moved = plan(
        "Verifiers wait.",
        "`M3`: `REQ-OPS-5`; `M4`: `REQ-OPS-4` (`F-030`)",
        "`REQ-OPS-3`, `REQ-OPS-4`",
    );
    let head = scope_text("off", &["REQ-OPS-4 | m4 | F-030 7.1:REQ-OPS"]);
    let found = ratchet(&head, &base, &moved, &base_plan);
    assert!(found.is_empty(), "{found:?}");
    // Both rows must be cited,
    for cite in ["F-030", "7.1:REQ-OPS"] {
        let head = scope_text("off", &[&format!("REQ-OPS-4 | m4 | {cite}")]);
        let found = ratchet(&head, &base, &moved, &base_plan);
        assert!(
            some_with(&found, "reclassified", "REQ-OPS-4"),
            "{cite}: {found:?}"
        );
    }
    // and the placement must say `M4`: a row that names it under `M3` places it nowhere new.
    let wrong = plan(
        "Verifiers wait.",
        "`M3`: `REQ-OPS-4`, `REQ-OPS-5`",
        "`REQ-OPS-3`, `REQ-OPS-4`",
    );
    let head = scope_text("off", &["REQ-OPS-4 | m4 | F-030 7.1:REQ-OPS"]);
    let found = ratchet(&head, &base, &wrong, &base_plan);
    assert!(some_with(&found, "reclassified", "REQ-OPS-4"), "{found:?}");
}

// verifies: DEC-17
#[test]
fn unrelated_edit_to_a_row_naming_the_id_does_not_excuse_a_reclassification() {
    let base = scope_text("off", &[]);
    // A deferral: develop's DEC-16 already named REQ-MOD-3, and the head only rewords it.
    let base_plan = plan(
        "Verifiers wait. Mentions `REQ-MOD-3`.",
        "`M4`: `REQ-OPS-4`; `M3`: `REQ-OPS-5`",
        "`REQ-OPS-3`, `REQ-OPS-4`",
    );
    let head_plan = plan(
        "Verifiers wait a while. Mentions `REQ-MOD-3`.",
        "`M4`: `REQ-OPS-4`; `M3`: `REQ-OPS-5`; `M5`: nothing",
        "`REQ-OPS-3`, `REQ-OPS-4`, `REQ-OPS-5`",
    );
    assert_ne!(base_plan, head_plan);
    let head = scope_text("off", &["REQ-MOD-3 | deferred | DEC-16"]);
    let found = ratchet(&head, &base, &head_plan, &base_plan);
    assert!(some_with(&found, "reclassified", "REQ-MOD-3"), "{found:?}");
    // A move to M4: both cited rows changed, both already named the identifier.
    let head = scope_text("off", &["REQ-OPS-4 | m4 | F-030 7.1:REQ-OPS"]);
    let found = ratchet(&head, &base, &head_plan, &base_plan);
    assert!(some_with(&found, "reclassified", "REQ-OPS-4"), "{found:?}");
}

// verifies: DEC-21
#[test]
fn tagged_test_with_a_namesake_that_never_ran_is_not_a_verifier() {
    // Two sources hold a test of one name, and the run read one line for it: the other never ran (a workspace the
    // root run doesn't reach, or a module behind a cfg), so the line proves nothing about the tagged one.
    let two = format!("{}\n{}", sources(&[]), sources(&[]));
    let found = table_findings(&scope_text("off", &[]), &two, &all_pass());
    assert!(
        some_with(&found, "no-passing-verifier", "AC-1"),
        "{found:?}"
    );
    let twice = format!("{}\n{}", all_pass(), all_pass());
    let found = table_findings(&scope_text("off", &[]), &two, &twice);
    assert!(found.is_empty(), "every copy ran and passed: {found:?}");
}

// verifies: DEC-21
#[test]
fn results_are_joined_by_the_last_segment_of_a_test_name_and_fail_closed() {
    let results = Results::parse(&doc(&[
        "running 3 tests",
        "test a::one ... ok",
        "test b::one ... FAILED",
        "test a::two ... ok",
        "test a::two ... ok",
        "test a::slow ... ignored",
        "test a::skipped ... ignored, with a reason",
        "test result: FAILED. 3 passed; 1 failed",
    ]));
    assert!(
        !results.passed("one", 1),
        "a name that failed once passes nowhere"
    );
    assert!(results.passed("two", 1));
    assert!(!results.passed("slow", 1) && !results.passed("skipped", 1));
    assert!(!results.passed("absent", 1));
    assert_eq!(results.state("absent"), "not run");
    assert_eq!(results.state("one"), "failed");
    assert!(Results::parse("nothing here").is_empty());
}

// verifies: DEC-17
#[test]
fn the_real_scope_file_is_well_formed_and_places_every_spec_identifier() {
    let root = root();
    let scope_text = std::fs::read_to_string(root.join("conformance/evidence/v0-scope"))
        .expect("the scope file is readable");
    let spec = std::fs::read_to_string(root.join("SPEC.md")).expect("SPEC.md is readable");
    let (scope, problems) = parse_scope(&scope_text);
    let defined = rutent_records::ids::load_defined(&root).expect("documents are readable");
    let universe = universe(&spec);
    assert!(
        universe.len() > 100,
        "the specification defines its identifiers"
    );
    let none: Vec<rutent_records::tags::TestCase> = Vec::new();
    let results = Results::default();
    let exists = |feature: &str| {
        std::fs::read_dir(root.join("features"))
            .map(|dir| {
                dir.flatten().any(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with(&format!("{feature}-"))
                        && e.path().join("notes.md").is_file()
                })
            })
            .unwrap_or(false)
    };
    let inputs = Inputs {
        universe: &universe,
        defined: &defined,
        tests: &none,
        results: &results,
        review_exists: &exists,
    };
    let report = build(&scope, &inputs);
    // What `M3`'s exit adds is a passing verifier for each `M3` row; here the rows must only be well formed.
    let structural: Vec<String> = problems
        .iter()
        .chain(report.table.iter())
        .filter(|f| !matches!(f.rule, "no-tagged-test" | "no-passing-verifier"))
        .map(ToString::to_string)
        .collect();
    assert!(structural.is_empty(), "{structural:#?}");
    assert!(!scope.strict_on || !scope_text.contains("strict: off"));
}

fn binary() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_evidence-table"));
    cmd.arg("--root").arg(root());
    cmd
}

// verifies: DEC-17
#[test]
fn the_strict_flag_always_runs_strictly_and_the_ci_run_follows_the_switch() {
    // No results file: no `M3` row has a passing verifier, so a strict run fails on the real table.
    let strict = binary().arg("--strict").output().expect("the binary runs");
    assert_eq!(
        strict.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&strict.stdout)
    );
    let report = binary().output().expect("the binary runs");
    assert_eq!(
        report.status.code(),
        Some(0),
        "report mode never fails a table"
    );
    let scope = std::fs::read_to_string(root().join("conformance/evidence/v0-scope")).unwrap();
    let ci = binary().arg("--ci").output().expect("the binary runs");
    let stdout = String::from_utf8_lossy(&ci.stdout);
    if scope.lines().any(|l| l.trim() == "strict: off") {
        assert_eq!(ci.status.code(), Some(0), "{stdout}");
        assert!(stdout.contains("switch off"), "{stdout}");
    } else {
        assert_eq!(ci.status.code(), Some(1), "{stdout}");
    }
}

/// A directory holding what the table reads of a repository: the documents and a scope file, but no source.
fn records_dir(label: &str, scope: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rutent-evidence-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("conformance/evidence")).unwrap();
    std::fs::create_dir_all(dir.join("adr")).unwrap();
    for file in ["SPEC.md", "PLAN.md", "PRODUCT.md"] {
        std::fs::copy(root().join(file), dir.join(file)).unwrap();
    }
    for entry in std::fs::read_dir(root().join("adr")).unwrap().flatten() {
        std::fs::copy(entry.path(), dir.join("adr").join(entry.file_name())).unwrap();
    }
    std::fs::write(dir.join("conformance/evidence/v0-scope"), scope).unwrap();
    dir
}

// verifies: DEC-17
#[test]
fn the_ci_run_fails_a_switch_turned_off_against_a_base_that_has_it_on() {
    // The head and the base are directories of their own, so the test reads the same whatever the real file says.
    let real = std::fs::read_to_string(root().join("conformance/evidence/v0-scope")).unwrap();
    let off = real.replace("strict: on", "strict: off");
    let on = off.replace("strict: off", "strict: on");
    assert_ne!(off, on);
    let head = records_dir("head", &off);
    let base = records_dir("base", &on);
    let run = |head: &Path, base: &Path| {
        let out = Command::new(env!("CARGO_BIN_EXE_evidence-table"))
            .args(["--ci", "--root"])
            .arg(head)
            .arg("--base")
            .arg(base)
            .output()
            .expect("the binary runs");
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).to_string(),
        )
    };
    let (code, stdout) = run(&head, &base);
    assert_eq!(code, Some(1), "{stdout}");
    assert!(stdout.contains("strict-switch-off"), "{stdout}");
    // Off against off compares clean, and the switch keeps the table's own findings from failing the run.
    let (code, stdout) = run(&head, &head);
    assert_eq!(code, Some(0), "{stdout}");

    // A base that has no scope file yet is skipped, and said so; a base with no plan can't be read at all.
    std::fs::remove_file(base.join("conformance/evidence/v0-scope")).unwrap();
    let (code, stdout) = run(&head, &base);
    assert_eq!(code, Some(0), "{stdout}");
    assert!(stdout.contains("not on the base yet"), "{stdout}");
    std::fs::remove_file(base.join("PLAN.md")).unwrap();
    let (code, _) = run(&head, &base);
    assert_eq!(code, Some(2));
    std::fs::remove_dir_all(&base).unwrap();
    std::fs::remove_dir_all(&head).unwrap();
}

// verifies: DEC-21
#[test]
fn an_ignored_test_with_a_reason_is_read_as_ignored() {
    let results = Results::parse("test a::skipped ... ignored, needs a network\ntest a::plain ... ignored");
    assert_eq!(results.state("skipped"), "ignored");
    assert_eq!(results.state("plain"), "ignored");
    assert!(!results.passed("skipped", 1));
}

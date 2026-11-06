use rutent_boundary_check::{Graph, load, members, parse_manifest, violations};
use std::path::{Path, PathBuf};

fn workspace() -> Graph {
    let root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    load(&root).expect("workspace manifests are readable")
}

/// A graph from manifest text, so a failing case is written the way a real one reads.
fn fixture(manifests: &[&str]) -> Graph {
    manifests
        .iter()
        .map(|m| parse_manifest(m).expect("fixture manifest has a package name"))
        .collect()
}

fn manifest(name: &str, deps: &str, dev_deps: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\n\n[dependencies]\n{deps}\n\n[dev-dependencies]\n{dev_deps}\n"
    )
}

/// The check on `name` is silent on the real workspace and speaks on a fixture where it breaks the rule.
fn holds_only_to_contracts(name: &str, library: &str) {
    let contracts = manifest("rutent-contracts", "", "");
    let leaky = fixture(&[
        &contracts,
        &manifest(
            "rutent-prov",
            "rutent-contracts = { path = \"../contracts\" }",
            "",
        ),
        &manifest(name, &format!("{library} = {{ path = \"../x\" }}"), ""),
    ]);
    let found = violations(&leaky, &[name]);
    assert_eq!(found.len(), 1, "fixture must break the rule: {found:?}");
    assert_eq!(
        (found[0].krate.as_str(), found[0].dep.as_str()),
        (name, library)
    );

    let real = workspace();
    assert!(real.contains_key(name), "{name} is a workspace member");
    assert_eq!(violations(&real, &[name]), vec![]);
}

// verifies: REQ-MOD-1, REQ-MOD-3
#[test]
fn router_imports_nothing_of_ours_but_contracts() {
    holds_only_to_contracts("rutent-router", "rutent-stm");
}

// verifies: REQ-MOD-1, REQ-MOD-3
#[test]
fn state_machine_imports_nothing_of_ours_but_contracts() {
    holds_only_to_contracts("rutent-stm", "rutent-router");
}

// verifies: REQ-MOD-1, REQ-MOD-3
#[test]
fn provider_system_imports_nothing_of_ours_but_contracts() {
    holds_only_to_contracts("rutent-prov", "rutent-core-orch");
}

// verifies: REQ-MOD-1, REQ-MOD-3
#[test]
fn core_reaches_libraries_only_through_their_public_contracts() {
    let core = ["rutent-core-orch", "rutent-core-authz", "rutent-core-audit"];
    let contracts = manifest("rutent-contracts", "", "");
    let libs =
        "rutent-contracts = { path = \"../contracts\" }\nrutent-router = { path = \"../r\" }";
    // Core on another Core crate, and on the conformance suites outside a dev-dependency.
    let leaky = fixture(&[
        &contracts,
        &manifest(
            "rutent-router",
            "rutent-contracts = { path = \"../contracts\" }",
            "",
        ),
        &manifest(
            "rutent-conformance",
            "rutent-contracts = { path = \"../contracts\" }",
            "",
        ),
        &manifest("rutent-core-orch", libs, ""),
        &manifest(
            "rutent-core-authz",
            &format!("{libs}\nrutent-core-orch = {{ path = \"../o\" }}"),
            "",
        ),
        &manifest(
            "rutent-core-audit",
            &format!("{libs}\nrutent-conformance = {{ path = \"../c\" }}"),
            "rutent-conformance = { path = \"../c\" }",
        ),
    ]);
    let found = violations(&leaky, &core);
    let pairs: Vec<_> = found
        .iter()
        .map(|v| (v.krate.as_str(), v.dep.as_str()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("rutent-core-authz", "rutent-core-orch"),
            ("rutent-core-audit", "rutent-conformance"),
        ]
    );

    let real = workspace();
    for name in core {
        assert!(real.contains_key(name), "{name} is a workspace member");
    }
    assert_eq!(violations(&real, &core), vec![]);
    // The whole graph, so nothing outside Core leans on Core or on conformance either.
    let all: Vec<&str> = real.keys().map(String::as_str).collect();
    assert_eq!(violations(&real, &all), vec![]);
}

/// A router manifest that depends on the state machine in a form the reader must not skip.
fn router_with(extra: &str) -> String {
    format!("[package]\nname = \"rutent-router\"\n\n{extra}\n")
}

fn assert_reported(extra: &str) {
    let result = parse_manifest(&router_with(extra));
    assert!(result.is_err(), "must be an error, not skipped: {result:?}");
}

// verifies: REQ-MOD-1, REQ-MOD-3
#[test]
fn renamed_dependency_is_reported() {
    assert_reported("[dependencies]\nfoo = { package = \"rutent-stm\", path = \"../stm\" }");
    assert_reported("[dependencies]\nfoo.package = \"rutent-stm\"\nfoo.path = \"../stm\"");
}

// verifies: REQ-MOD-1, REQ-MOD-3
#[test]
fn target_table_is_reported() {
    assert_reported("[target.'cfg(unix)'.dependencies]\nrutent-stm = { path = \"../stm\" }");
    assert_reported(
        "[target.x86_64-unknown-linux-gnu.dev-dependencies]\nrutent-stm = { path = \"../stm\" }",
    );
}

// verifies: REQ-MOD-1, REQ-MOD-3
#[test]
fn dependency_sub_table_is_reported() {
    assert_reported("[dependencies.rutent-stm]\npath = \"../stm\"");
    assert_reported("[dev-dependencies.rutent-stm]\npath = \"../stm\"");
}

// verifies: REQ-MOD-1, REQ-MOD-3
#[test]
fn quoted_dependency_key_is_reported() {
    assert_reported("[dependencies]\n\"rutent-stm\" = { path = \"../stm\" }");
    assert_reported("[dependencies]\n'rutent-stm' = { path = \"../stm\" }");
}

// verifies: REQ-MOD-1, REQ-MOD-3
#[test]
fn multi_line_dependency_value_is_reported() {
    assert_reported("[dependencies]\nrutent-stm = {\n    path = \"../stm\",\n}");
}

// verifies: REQ-MOD-1, REQ-MOD-3
#[test]
fn members_key_is_read_not_default_members_or_comments() {
    let root = "# members = [\"decoy\"]\n[workspace]\ndefault-members = [\"decoy\"]\nmembers = [\n    \"a\", # c\n    \"b\",\n]\n";
    assert_eq!(members(root), Ok(vec!["a".to_string(), "b".to_string()]));
    assert!(members("[workspace]\ndefault-members = [\"a\"]\n").is_err());
}

// verifies: REQ-MOD-1, REQ-MOD-3
#[test]
fn array_of_tables_headers_are_accepted_with_dependencies_intact() {
    let text = "[package]\nname = \"rutent-router\"\n\n[dependencies]\nrutent-contracts = { path = \"../contracts\" }\n\n[[bin]]\nname = \"tool\"\npath = \"src/tool.rs\"\n\n[[test]]\nname = \"suite\"\n\n[dev-dependencies]\nrutent-conformance = { path = \"../conformance\" }\n";
    let (name, deps) =
        parse_manifest(text).expect("array-of-tables headers are valid manifest syntax");
    assert_eq!(name, "rutent-router");
    let deps: Vec<_> = deps.iter().map(|d| (d.name.as_str(), d.dev)).collect();
    assert_eq!(
        deps,
        vec![("rutent-contracts", false), ("rutent-conformance", true)]
    );
}

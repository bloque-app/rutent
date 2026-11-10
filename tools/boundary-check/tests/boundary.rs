//! Tests of the boundary check.

use rutent_boundary_check::{
    Graph, OUTER, SourceFinding, Why, load, members, parse_manifest, sources_naming_core_internal,
    violations,
};
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

// verifies: REQ-MOD-2, NFR-7
#[test]
fn a_manifest_with_lints_tables_still_parses() {
    let manifest = "[package]\nname = \"rutent-x\"\n\n[lints]\nworkspace = true\n\n\
        [dependencies]\nrutent-contracts = { path = \"../contracts\" }\n";
    let (name, deps) = parse_manifest(manifest).expect("lints table is accepted");
    assert_eq!((name.as_str(), deps.len()), ("rutent-x", 1));
    let root =
        "[workspace]\nmembers = [\"a\"]\n\n[workspace.lints.rust]\nmissing_docs = \"deny\"\n";
    assert_eq!(members(root).unwrap(), ["a"]);
}

const CORE: [&str; 3] = ["rutent-core-orch", "rutent-core-authz", "rutent-core-audit"];

/// Core's three crates, each on the contracts and a library, and on the Core crates `extra` names.
fn core_with(extra: [&str; 3]) -> Vec<String> {
    let libs = "rutent-contracts = { path = \"../contracts\" }\nrutent-stm = { path = \"../stm\" }";
    CORE.iter()
        .zip(extra)
        .map(|(name, dep)| {
            let deps = if dep.is_empty() {
                libs.to_string()
            } else {
                format!("{libs}\n{dep} = {{ path = \"../x\" }}")
            };
            manifest(name, &deps, "")
        })
        .collect()
}

// verifies: REQ-MOD-3, DEC-2
#[test]
fn core_crate_importing_another_core_crate_fails_the_boundary_check() {
    let contracts = manifest("rutent-contracts", "", "");
    let stm = manifest(
        "rutent-stm",
        "rutent-contracts = { path = \"../contracts\" }",
        "",
    );
    // Each of the three on the next: orchestration on the log, policy on orchestration, the log on policy.
    let mut manifests = vec![contracts.clone(), stm.clone()];
    manifests.extend(core_with([
        "rutent-core-audit",
        "rutent-core-orch",
        "rutent-core-authz",
    ]));
    let refs: Vec<&str> = manifests.iter().map(String::as_str).collect();
    let found = violations(&fixture(&refs), &CORE);
    let pairs: Vec<_> = found
        .iter()
        .map(|v| (v.krate.as_str(), v.dep.as_str()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("rutent-core-orch", "rutent-core-audit"),
            ("rutent-core-authz", "rutent-core-orch"),
            ("rutent-core-audit", "rutent-core-authz"),
        ]
    );
    // As a dev-dependency too.
    let dev = manifest(
        "rutent-core-orch",
        "rutent-contracts = { path = \"../contracts\" }",
        "rutent-core-authz = { path = \"../a\" }",
    );
    let found = violations(&fixture(&[&contracts, &dev]), &["rutent-core-orch"]);
    assert_eq!(found.len(), 1, "{found:?}");

    // Without those edges the same crates pass, and so does the real workspace.
    let mut clean = vec![contracts, stm];
    clean.extend(core_with(["", "", ""]));
    let refs: Vec<&str> = clean.iter().map(String::as_str).collect();
    assert_eq!(violations(&fixture(&refs), &CORE), vec![]);
    assert_eq!(violations(&workspace(), &CORE), vec![]);
}

// verifies: DEC-2
#[test]
fn only_the_scenario_assembly_may_import_core_crates() {
    let contracts = manifest("rutent-contracts", "", "");
    let every_core = CORE
        .iter()
        .map(|c| format!("{c} = {{ path = \"../{c}\" }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let scenarios = manifest(
        "rutent-scenarios",
        &format!(
            "{every_core}\nrutent-contracts = {{ path = \"../c\" }}\nrutent-router = {{ path = \"../r\" }}\n\
             rutent-conformance = {{ path = \"../conf\" }}"
        ),
        "",
    );
    let leaky = fixture(&[
        &contracts,
        &scenarios,
        &manifest(
            "rutent-conformance",
            "rutent-core-orch = { path = \"../o\" }",
            "",
        ),
        &manifest(
            "rutent-router",
            "rutent-core-audit = { path = \"../a\" }",
            "",
        ),
        &manifest(
            "rutent-records",
            "",
            "rutent-core-authz = { path = \"../z\" }",
        ),
        // Nothing imports the assembly either, not even Core, not even as a dev-dependency.
        &manifest(
            "rutent-core-orch",
            "rutent-contracts = { path = \"../c\" }",
            "rutent-scenarios = { path = \"../s\" }",
        ),
    ]);
    let all: Vec<&str> = leaky.keys().map(String::as_str).collect();
    let mut pairs: Vec<_> = violations(&leaky, &all)
        .into_iter()
        .map(|v| (v.krate, v.dep))
        .collect();
    pairs.sort();
    let expected = [
        ("rutent-conformance", "rutent-core-orch"),
        ("rutent-core-orch", "rutent-scenarios"),
        ("rutent-records", "rutent-core-authz"),
        ("rutent-router", "rutent-core-audit"),
    ];
    assert_eq!(pairs, expected.map(|(k, d)| (k.to_string(), d.to_string())));

    // The real workspace has the assembly as a member, and nothing in it imports the assembly.
    let real = workspace();
    assert!(
        real.contains_key("rutent-scenarios"),
        "the assembly is a member"
    );
    let all: Vec<&str> = real.keys().map(String::as_str).collect();
    assert_eq!(violations(&real, &all), vec![]);
    assert!(
        real.values()
            .all(|deps| deps.iter().all(|d| d.name != "rutent-scenarios")),
        "nothing imports the assembly"
    );
}

/// A throwaway workspace with the four outer libraries, the conformance suites, the scenario
/// assembly inside them, and the contracts crate, each with one source file.
struct Sources(PathBuf);

impl Sources {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rutent-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let packages =
            OUTER
                .iter()
                .chain(&["rutent-contracts", "rutent-conformance", "rutent-scenarios"]);
        let mut members = Vec::new();
        for krate in packages {
            let at = match *krate {
                "rutent-scenarios" => "conformance/scenarios",
                other => other.trim_start_matches("rutent-"),
            };
            members.push(format!("\"{at}\""));
            std::fs::create_dir_all(dir.join(at).join("src")).unwrap();
            std::fs::write(dir.join(at).join("Cargo.toml"), manifest(krate, "", "")).unwrap();
            std::fs::write(dir.join(at).join("src/lib.rs"), "//! A library.\n").unwrap();
        }
        std::fs::write(
            dir.join("Cargo.toml"),
            format!("[workspace]\nmembers = [{}]\n", members.join(", ")),
        )
        .unwrap();
        let sources = Sources(dir);
        sources.write(
            "contracts/src/lib.rs",
            "pub mod core_internal;\npub mod router_api;\n",
        );
        sources.write(
            "contracts/src/core_internal/mod.rs",
            "//! core_internal names itself.\n",
        );
        sources
    }

    fn write(&self, file: &str, text: &str) {
        let path = self.0.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn findings(&self) -> Vec<(String, usize, Why)> {
        sources_naming_core_internal(&self.0)
            .unwrap()
            .into_iter()
            .map(|SourceFinding { file, line, why }| {
                (file.to_string_lossy().into_owned(), line, why)
            })
            .collect()
    }
}

impl Drop for Sources {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn expect(found: Vec<(String, usize, Why)>, expected: &[(&str, usize, Why)]) {
    let expected: Vec<_> = expected
        .iter()
        .map(|(f, l, w)| (f.to_string(), *l, *w))
        .collect();
    assert_eq!(found, expected);
}

// verifies: DEC-2
#[test]
fn outer_library_naming_core_internal_fails_the_boundary_check() {
    let sources = Sources::new("core-internal-names");
    // The contract's own module and the crate root's declaration of it are not findings.
    assert_eq!(sources.findings(), vec![]);

    sources.write(
        "router/src/quote.rs",
        "use rutent_contracts::core_internal::Entry;\n",
    );
    sources.write(
        "stm/vos-program/src/main.rs",
        "fn f() {}\n// see rutent_contracts::core_internal\n",
    );
    sources.write(
        "prov/guests/plain-wallet/src/lib.rs",
        "use c::core_internal as x;\n",
    );
    // Only the package's own `target` is not its source: one deeper in the tree is.
    sources.write("prov/target/debug/build/out.rs", "core_internal\n");
    sources.write("prov/guests/plain-wallet/target/gen.rs", "core_internal\n");
    sources.write("evidence/tests/t.rs", "type T = r#core_internal::Raise;\n");
    sources.write(
        "contracts/src/router_api/mod.rs",
        "pub use crate::core_internal::Raise as Flag;\n",
    );
    sources.write(
        "contracts/src/lib.rs",
        "pub mod core_internal;\npub use core_internal::Entry;\n",
    );
    expect(
        sources.findings(),
        &[
            ("contracts/src/lib.rs", 2, Why::Names),
            ("contracts/src/router_api/mod.rs", 1, Why::Names),
            ("evidence/tests/t.rs", 1, Why::Names),
            ("prov/guests/plain-wallet/src/lib.rs", 1, Why::Names),
            ("prov/guests/plain-wallet/target/gen.rs", 1, Why::Names),
            ("router/src/quote.rs", 1, Why::Names),
            ("stm/vos-program/src/main.rs", 2, Why::Names),
        ],
    );

    // A library missing from the workspace is an error, not a pass.
    sources.write("Cargo.toml", "[workspace]\nmembers = [\"contracts\"]\n");
    assert!(sources_naming_core_internal(&sources.0).is_err());

    // The real workspace: no outer library names it.
    let root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert_eq!(sources_naming_core_internal(&root).unwrap(), vec![]);
}

// verifies: DEC-2
#[test]
fn a_symbolic_link_in_a_checked_tree_fails_the_boundary_check() {
    let sources = Sources::new("core-internal-links");
    let target = sources.0.join("elsewhere");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(
        target.join("lib.rs"),
        "use rutent_contracts::core_internal;\n",
    )
    .unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, sources.0.join("router/src/linked")).unwrap();
    #[cfg(unix)]
    assert!(sources_naming_core_internal(&sources.0).is_err());
}

// verifies: DEC-2
#[test]
fn mounting_a_file_from_outside_the_package_fails_the_boundary_check() {
    let sources = Sources::new("core-internal-mounts");
    // Mounting a file inside the package is how tests share support code, and passes.
    sources.write(
        "prov/tests/egress.rs",
        "#[path = \"egress_support/mod.rs\"]\nmod support;\nconst S: &str = include_str!(\"x.txt\");\n",
    );
    sources.write(
        "router/src/lib.rs",
        "#[path = \"../../core-orch/src/intake/mod.rs\"]\nmod intake;\n",
    );
    sources.write("stm/src/lib.rs", "include!(\"../../outside.rs\");\n");
    sources.write(
        "evidence/src/lib.rs",
        "include!(concat!(env!(\"OUT_DIR\"), \"/x.rs\"));\n",
    );
    sources.write(
        "conformance/src/lib.rs",
        "#[path = \"/abs/core-audit/src/log/mod.rs\"]\nmod log;\n",
    );
    expect(
        sources.findings(),
        &[
            ("conformance/src/lib.rs", 1, Why::Mounts),
            ("evidence/src/lib.rs", 1, Why::Mounts),
            ("router/src/lib.rs", 1, Why::Mounts),
            ("stm/src/lib.rs", 1, Why::Mounts),
        ],
    );
}

// verifies: DEC-2
#[test]
fn conformance_exposing_core_internal_in_a_public_item_fails_the_boundary_check() {
    let sources = Sources::new("core-internal-exposes");
    // The suites may use the contract inside their own code, and the scenario assembly, another
    // member, isn't theirs.
    sources.write(
        "conformance/src/lib.rs",
        "use rutent_contracts::core_internal::Entry;\n\
         pub use rutent_contracts::router_api;\n\
         // pub type Mentioned = core_internal::Entry;\n\
         fn private(e: core_internal::Entry) {}\n\
         pub fn public() {\n    let _ = core_internal::Entry::default();\n}\n\
         pub(crate) type Inner = core_internal::Entry;\n\
         struct Hidden { e: core_internal::Entry }\n\
         const TEXT: &str = \"pub type T = core_internal::Entry;\";\n",
    );
    sources.write(
        "conformance/scenarios/src/lib.rs",
        "pub use rutent_contracts::core_internal;\n",
    );
    assert_eq!(sources.findings(), vec![]);

    sources.write(
        "conformance/src/suite.rs",
        "pub use rutent_contracts::core_internal::Entry;\n\
         pub use rutent_contracts::{\n    core_internal,\n    router_api,\n};\n\
         pub type Alias = rutent_contracts::core_internal::Entry;\n\
         pub fn run(\n    log: &mut dyn core_internal::calls::Log,\n) {}\n\
         pub struct Holder {\n    pub entry: core_internal::Entry,\n}\n\
         pub struct Tuple(pub core_internal::Entry);\n\
         pub enum Either {\n    One(core_internal::Entry),\n}\n\
         pub trait Suite {\n    fn take(&self, e: core_internal::Entry);\n}\n\
         impl From<core_internal::Entry> for Holder {\n    fn from(entry: Entry) -> Self { todo!() }\n}\n",
    );
    let lines: Vec<usize> = sources
        .findings()
        .into_iter()
        .map(|(file, line, why)| {
            assert_eq!(
                (file.as_str(), why),
                ("conformance/src/suite.rs", Why::Exposes)
            );
            line
        })
        .collect();
    assert_eq!(lines, [1, 3, 6, 8, 11, 13, 15, 18, 20]);
}

// verifies: DEC-2
#[test]
fn a_path_key_in_any_attribute_form_is_a_mount() {
    let sources = Sources::new("core-internal-attributes");
    sources.write(
        "router/src/lib.rs",
        "# [path = \"../../core-orch/src/intake/mod.rs\"]\nmod a;\n\
         #[cfg_attr(all(), path = \"../../core-authz/src/engine/mod.rs\")]\nmod b;\n\
         #[cfg_attr(\n    all(),\n    path =\n        \"../../core-audit/src/log/mod.rs\"\n)]\nmod c;\n\
         include\n    !(\n        \"../../outside.rs\");\n\
         // #[path = \"../../commented.rs\"]\n\
         #[path = \"inside/mod.rs\"]\nmod d;\n",
    );
    expect(
        sources.findings(),
        &[
            ("router/src/lib.rs", 1, Why::Mounts),
            ("router/src/lib.rs", 3, Why::Mounts),
            ("router/src/lib.rs", 5, Why::Mounts),
            ("router/src/lib.rs", 11, Why::Mounts),
        ],
    );
}

// verifies: DEC-2
#[test]
fn a_macro_exported_from_core_internal_fails_the_boundary_check() {
    let sources = Sources::new("core-internal-macros");
    sources.write(
        "contracts/src/core_internal/macros.rs",
        "#[macro_export]\nmacro_rules! entry { () => {} }\n",
    );
    expect(
        sources.findings(),
        &[(
            "contracts/src/core_internal/macros.rs",
            1,
            Why::ExportsAMacro,
        )],
    );
}

//! Tests of the boundary check.

use rutent_boundary_check::{
    Graph, OUTER, PROV_CONFINE, PROV_KEYS, PROV_KEYS_FACADE, PROV_KEYS_WITHHELD, READER_HOST,
    SourceFinding, Why, defined_in, holds_its_reexports, load, members, parse_manifest,
    public_items_naming_any, sources_naming_core_internal, violations,
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
/// The admitted paths' files as the real crate writes them.
const PROV_CONFINE_FILE: &str = "//! The jail.\n\
    pub use crate::module::sandbox::{Limits, PROVIDER_ADDRESS_SPACE, Role, confine, isolated};\n";
const PROV_KEYS_FILE: &str = "//! The keys.\n\
    pub use crate::proxy::kv::Server;\n\
    pub use crate::proxy::resolver::{ResolveError, Resolver};\n\
    pub use crate::proxy::settings::{InvalidSettings, StoreAddress, StoreScheme};\n\
    pub use crate::proxy::statement_keys::{KeyStoreSettings, StatementKeys};\n";
/// The facade over a role's statement keys, as the real crate writes it: what it wraps is named in its private
/// imports and bodies alone.
const PROV_FACADE_FILE: &str = "//! The facade.\n\
    use super::kv::{KvConfig, KvResolver, Token};\n\
    use super::resolver::Resolver;\n\
    use super::settings::{InvalidSettings, StoreAddress};\n\
    pub struct KeyStoreSettings {\n    pub store: StoreAddress,\n    pub mount: String,\n    pub keys: String,\n}\n\
    pub struct StatementKeys {\n    keys: Box<dyn Resolver + Send + Sync>,\n}\n\
    impl StatementKeys {\n\
    \x20   pub fn new(settings: &KeyStoreSettings, token: String) -> Result<Self, InvalidSettings> {\n\
    \x20       let _ = (KvConfig::new(Token::new(token)), KvResolver::vault());\n\
    \x20       todo!()\n\
    \x20   }\n\
    }\n\
    impl Resolver for StatementKeys {\n\
    \x20   fn resolve(&self) -> u8 {\n        let _: &KvResolver = todo!();\n    }\n\
    }\n";
const PROV_ROOT: &str =
    "//! A library.\npub mod confine;\npub mod keys;\npub mod module;\npub mod proxy;\n";
/// The modules the admitted paths re-export from, each defining its items, as the real crate lays them out.
const PROV_MODULES: [(&str, &str); 9] = [
    (
        "prov/src/module/mod.rs",
        "//! The runtime.\npub mod sandbox;\n",
    ),
    (
        "prov/src/module/sandbox.rs",
        "//! The jail.\npub struct Limits {}\npub const PROVIDER_ADDRESS_SPACE: u64 = 1;\n\
         pub enum Role { Provider }\npub fn confine(role: Role) {}\npub fn isolated() {}\n\
         mod linux {\n    pub(super) fn confine() {}\n}\n",
    ),
    (
        "prov/src/proxy/mod.rs",
        "//! The proxy.\nmod calls;\npub(crate) mod https;\npub(crate) mod kv;\npub(crate) mod resolver;\n\
         pub(crate) mod settings;\npub(crate) mod statement_keys;\npub(crate) mod transport;\n\
         pub use calls::CredentialProxy;\n",
    ),
    ("prov/src/proxy/https.rs", "pub struct HttpsTransport {}\n"),
    (
        "prov/src/proxy/kv.rs",
        "pub struct InvalidConfig(pub &'static str);\npub struct KvConfig {}\npub struct KvResolver<T> { t: T }\n\
         pub enum Server { Vault }\npub struct Token(String);\n",
    ),
    (
        "prov/src/proxy/resolver.rs",
        "pub trait Resolver {}\npub enum ResolveError {}\n",
    ),
    (
        "prov/src/proxy/settings.rs",
        "pub struct Bucket<R> { r: R }\npub struct InvalidSettings(pub String);\npub struct RoleSettings {}\n\
         pub struct StoreAddress {}\npub enum StoreScheme {}\npub fn kv_bucket<T: Clone>(t: T) {}\n",
    ),
    (
        "prov/src/proxy/transport.rs",
        "pub struct TransportError {}\n",
    ),
    ("prov/src/proxy/statement_keys.rs", PROV_FACADE_FILE),
];

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
        // The provider system's admitted paths, as the real crate declares them.
        sources.write("prov/src/lib.rs", PROV_ROOT);
        sources.write("prov/src/confine.rs", PROV_CONFINE_FILE);
        sources.write("prov/src/keys.rs", PROV_KEYS_FILE);
        for (file, text) in PROV_MODULES {
            sources.write(file, text);
        }
        sources.write("prov/src/proxy/calls.rs", "pub struct CredentialProxy;\n");
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

/// A crate's dev-dependency on itself, as the contracts crate's for its test-only `rule-switches` feature
/// (`T-020-45`), is no edge between two crates and passes; the same crate depending on itself outside its
/// dev-dependencies, and a real dev-dependency on another crate of ours, are each still reported.
// verifies: REQ-MOD-1, REQ-MOD-3
#[test]
fn a_crates_dev_dependency_on_itself_is_no_boundary_crossing() {
    let own = "rutent-contracts = { path = \".\", features = [\"rule-switches\"] }";
    let graph = fixture(&[
        &manifest("rutent-contracts", "", own),
        &manifest(
            "rutent-router",
            "rutent-contracts = { path = \"../contracts\" }",
            "rutent-router = { path = \".\" }\nrutent-stm = { path = \"../stm\" }",
        ),
        &manifest("rutent-stm", "rutent-stm = { path = \".\" }", ""),
    ]);
    let found = violations(&graph, &["rutent-contracts", "rutent-router", "rutent-stm"]);
    let pairs: Vec<_> = found
        .iter()
        .map(|v| (v.krate.as_str(), v.dep.as_str()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("rutent-router", "rutent-stm"),
            ("rutent-stm", "rutent-stm"),
        ]
    );
}

/// The reader host (`DEC-28`, `T-015-04`'s crate) depends on the contracts and on the provider system, for its egress
/// layer only, and nothing of ours depends on it but the scenario assembly's tests, as a dev-dependency (`T-020-59`):
/// any other edge from it, and any other edge to it, is reported. Its sources never name Core's internal contract, and name the provider system only
/// through `rutent_prov::egress`.
// verifies: DEC-28, DEC-2, REQ-MOD-3
#[test]
fn reader_host_reaches_only_the_contracts_and_the_providers_egress_and_nothing_reaches_it() {
    let contracts = manifest("rutent-contracts", "", "");
    let prov = manifest(
        "rutent-prov",
        "rutent-contracts = { path = \"../contracts\" }",
        "",
    );
    let host = |deps: &str, dev: &str| manifest(READER_HOST, deps, dev);
    let allowed =
        "rutent-contracts = { path = \"../../contracts\" }\nrutent-prov = { path = \"../prov\" }";
    let clean = fixture(&[&contracts, &prov, &host(allowed, "")]);
    assert_eq!(violations(&clean, &[READER_HOST]), vec![]);

    // The host on Core, on the evidence checker, or on the conformance suites outside its dev-dependencies.
    let leaky = fixture(&[
        &contracts,
        &prov,
        &host(
            &format!(
                "{allowed}\nrutent-core-orch = {{ path = \"../o\" }}\nrutent-evidence = {{ path = \"../e\" }}\n\
                 rutent-conformance = {{ path = \"../c\" }}"
            ),
            "",
        ),
    ]);
    let pairs: Vec<_> = violations(&leaky, &[READER_HOST])
        .into_iter()
        .map(|v| v.dep)
        .collect();
    assert_eq!(
        pairs,
        ["rutent-core-orch", "rutent-evidence", "rutent-conformance"]
    );

    // Nothing of ours on it: a library, Core or the conformance suites, as a dependency or a dev-dependency, nor the
    // assembly as a dependency of its build.
    let on_host = format!("{READER_HOST} = {{ path = \"../reader-host\" }}");
    let leaning = fixture(&[
        &contracts,
        &host(allowed, ""),
        &manifest("rutent-prov", "", &on_host),
        &manifest("rutent-core-orch", &on_host, ""),
        &manifest("rutent-conformance", &on_host, ""),
        &manifest("rutent-scenarios", &on_host, ""),
        &manifest("rutent-evidence", "", &on_host),
    ]);
    let all: Vec<&str> = leaning.keys().map(String::as_str).collect();
    let mut pairs: Vec<_> = violations(&leaning, &all)
        .into_iter()
        .map(|v| (v.krate, v.dep))
        .collect();
    pairs.sort();
    let expected = [
        "rutent-conformance",
        "rutent-core-orch",
        "rutent-evidence",
        "rutent-prov",
        "rutent-scenarios",
    ]
    .map(|k| (k.to_string(), READER_HOST.to_string()));
    assert_eq!(pairs, expected);

    // Its sources, once it is a member: only paths into the provider system's egress layer, and never the contract.
    let sources = Sources::new("reader-host");
    sources.write(
        "Cargo.toml",
        &format!(
            "{}\n",
            std::fs::read_to_string(sources.0.join("Cargo.toml"))
                .unwrap()
                .replace("members = [", "members = [\"components/reader-host\", ")
        ),
    );
    sources.write("components/reader-host/Cargo.toml", &host(allowed, ""));
    sources.write(
        "components/reader-host/src/lib.rs",
        "//! The reader host; see rutent_prov's egress.\n\
         use rutent_prov::egress::Egress;\n\
         use rutent_prov :: egress;\n\
         fn f() -> rutent_prov::egress::Call { todo!() }\n\
         const S: &str = \"rutent_prov::module\";\n",
    );
    assert_eq!(sources.findings(), vec![]);
    sources.write(
        "components/reader-host/src/egress.rs",
        "use rutent_prov::module::Platform;\n\
         use rutent_prov::{egress, catalogue};\n\
         extern crate rutent_prov;\n\
         use rutent_prov::egressive::X;\n\
         fn f(_: rutent_prov::catalogue::Snapshot) {}\n\
         use rutent_contracts::core_internal::Entry;\n\
         use my_rutent_prov::x;\n",
    );
    expect(
        sources.findings(),
        &[
            (
                "components/reader-host/src/egress.rs",
                1,
                Why::ReachesBeyondItsPaths,
            ),
            (
                "components/reader-host/src/egress.rs",
                2,
                Why::ReachesBeyondItsPaths,
            ),
            (
                "components/reader-host/src/egress.rs",
                3,
                Why::ReachesBeyondItsPaths,
            ),
            (
                "components/reader-host/src/egress.rs",
                4,
                Why::ReachesBeyondItsPaths,
            ),
            (
                "components/reader-host/src/egress.rs",
                5,
                Why::ReachesBeyondItsPaths,
            ),
            ("components/reader-host/src/egress.rs", 6, Why::Names),
        ],
    );

    // The real workspace holds to it, whether the host is a member yet or not.
    let real = workspace();
    let all: Vec<&str> = real.keys().map(String::as_str).collect();
    assert_eq!(violations(&real, &all), vec![]);
    assert!(
        real.values()
            .all(|deps| deps.iter().all(|d| d.name != READER_HOST)),
        "nothing of ours depends on the reader host"
    );
}

/// The reader host reaches the provider's jail and `T-014-01`'s key resolver too, each through a narrow public path of
/// the provider system that re-exports just them (`rutent_prov::confine`, `rutent_prov::keys`), beside its egress layer
/// (`T-015-04`'s part 7, `DEC-28`): those paths pass, and every other path into `module` or `proxy` is still refused,
/// as is an admitted path's file that re-exports more than it is admitted with.
// verifies: DEC-28, DEC-2, REQ-MDL-6, REQ-MDL-7
#[test]
fn reader_host_reaches_the_providers_jail_and_key_resolver_through_their_admitted_paths_only() {
    let host = manifest(
        READER_HOST,
        "rutent-contracts = { path = \"../../contracts\" }\nrutent-prov = { path = \"../prov\" }",
        "",
    );
    let sources = Sources::new("reader-host-paths");
    sources.write(
        "Cargo.toml",
        &format!(
            "{}\n",
            std::fs::read_to_string(sources.0.join("Cargo.toml"))
                .unwrap()
                .replace("members = [", "members = [\"components/reader-host\", ")
        ),
    );
    sources.write("components/reader-host/Cargo.toml", &host);
    sources.write(
        "components/reader-host/src/lib.rs",
        "use rutent_prov::confine::{Limits, Role, confine, isolated};\n\
         use rutent_prov::keys::{Resolver, StatementKeys};\n\
         use rutent_prov::egress::Egress;\n\
         fn f(_: &dyn rutent_prov::keys::Resolver) -> rutent_prov::confine::Role { todo!() }\n",
    );
    assert_eq!(sources.findings(), vec![]);

    // Everything else of `module` and `proxy`, named directly or by a longer name, a group or a glob.
    sources.write(
        "components/reader-host/src/jail.rs",
        "use rutent_prov::module::sandbox::isolated;\n\
         use rutent_prov::proxy::Resolver;\n\
         use rutent_prov::proxy::CredentialProxy;\n\
         use rutent_prov::module::platform::OperationClock;\n\
         use rutent_prov::keystore::Resolver;\n\
         use rutent_prov::confined::isolated;\n\
         use rutent_prov::{confine, keys};\n\
         use rutent_prov::*;\n",
    );
    let jail = "components/reader-host/src/jail.rs";
    expect(
        sources.findings(),
        &(1..=8)
            .map(|line| (jail, line, Why::ReachesBeyondItsPaths))
            .collect::<Vec<_>>(),
    );
    sources.write(jail, "//! The jail.\n");

    // The admitted paths' files hold their one re-export, or the path widens.
    let confine = "pub use crate::module::sandbox::{Limits, PROVIDER_ADDRESS_SPACE, Role, confine, isolated};\n";
    assert_eq!(sources.findings(), vec![]);
    for widened in [
        "pub use crate::proxy::{Resolver, CredentialProxy};\n",
        "pub use crate::proxy::*;\n",
        "pub use crate::proxy::{Resolver as R};\n",
        "pub use crate::module::sandbox::{Limits, PROVIDER_ADDRESS_SPACE, Role, confine, isolated};\n",
    ] {
        sources.write("prov/src/keys.rs", widened);
        expect(
            sources.findings(),
            &[("prov/src/keys.rs", 1, Why::WidensAnAdmittedPath)],
        );
    }
    sources.write("prov/src/keys.rs", "");
    sources.write(
        "prov/src/confine.rs",
        &format!("{confine}pub mod platform {{}}\n"),
    );
    expect(
        sources.findings(),
        &[
            ("prov/src/confine.rs", 1, Why::WidensAnAdmittedPath),
            ("prov/src/keys.rs", 1, Why::WidensAnAdmittedPath),
        ],
    );
    sources.write("prov/src/confine.rs", PROV_CONFINE_FILE);
    sources.write("prov/src/keys.rs", PROV_KEYS_FILE);
    assert_eq!(sources.findings(), vec![]);

    // The crate root mounts another file under an admitted name, by `#[path]` on its own line, on the same line, or
    // through `cfg_attr`; or gives it an inline body; or leaves it out, or declares it twice; while the real file
    // stays as a decoy. Each is refused.
    sources.write("prov/src/confine_wide.rs", "pub use crate::module::*;\n");
    // A `#[path]` is refused wherever it is, and the declaration it sits on is not plain: both lines are named.
    let roots: [(&str, &[usize]); 6] = [
        (
            "#[path = \"confine_wide.rs\"]\npub mod confine;\npub mod keys;\n",
            &[2, 3],
        ),
        (
            "#[path = \"confine_wide.rs\"] pub mod confine;\npub mod keys;\n",
            &[2],
        ),
        (
            "#[cfg_attr(all(), path = \"confine_wide.rs\")]\npub mod confine;\npub mod keys;\n",
            &[2, 3],
        ),
        (
            "pub mod confine {\n    pub use crate::module::*;\n}\npub mod keys;\n",
            &[2],
        ),
        ("pub mod keys;\n", &[1]),
        ("pub mod confine;\nmod confine;\npub mod keys;\n", &[3]),
    ];
    for (root, lines) in roots {
        sources.write(
            "prov/src/lib.rs",
            &format!("//! A library.\n{root}pub mod module;\npub mod proxy;\n"),
        );
        let expected: Vec<_> = lines
            .iter()
            .map(|line| ("prov/src/lib.rs", *line, Why::WidensAnAdmittedPath))
            .collect();
        expect(sources.findings(), &expected);
    }
    sources.write("prov/src/lib.rs", PROV_ROOT);
    assert_eq!(sources.findings(), vec![]);

    // A missing file fails closed, and so does a module directory beside the file.
    std::fs::remove_file(sources.0.join("prov/src/keys.rs")).unwrap();
    expect(
        sources.findings(),
        &[("prov/src/keys.rs", 1, Why::WidensAnAdmittedPath)],
    );
    sources.write("prov/src/keys.rs", PROV_KEYS_FILE);
    sources.write("prov/src/confine/mod.rs", "pub use crate::module::*;\n");
    expect(
        sources.findings(),
        &[("prov/src/confine", 1, Why::WidensAnAdmittedPath)],
    );

    // The real files hold, and the real workspace's sources are clean.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read =
        |file: &str| std::fs::read_to_string(root.join("components/prov/src").join(file)).unwrap();
    assert!(holds_its_reexports(&read("confine.rs"), PROV_CONFINE));
    assert!(holds_its_reexports(&read("keys.rs"), PROV_KEYS));
    assert_eq!(sources_naming_core_internal(&root).unwrap(), vec![]);
}

/// What an admitted path re-exports resolves, by file layout alone, to an item its source file defines (`T-015-04`'s
/// review round 2, M1′): a `#[path]` anywhere under the provider system's `src/`, or a `[lib]` table in its manifest,
/// moves a module and is refused; a module on the way declared other than plainly, or with a directory beside its file,
/// is refused; and a re-exported name that its source file doesn't define itself (a further `pub use`, a glob, or a
/// module of the same name riding on a value's path, as `pub use crate::proxy as PROVIDER_ADDRESS_SPACE;`) is refused.
// verifies: DEC-28, DEC-2
#[test]
fn providers_admitted_paths_resolve_by_layout_to_the_items_their_files_define() {
    let host = manifest(
        READER_HOST,
        "rutent-contracts = { path = \"../../contracts\" }\nrutent-prov = { path = \"../prov\" }",
        "",
    );
    let sources = Sources::new("reader-host-resolve");
    sources.write(
        "Cargo.toml",
        &format!(
            "{}\n",
            std::fs::read_to_string(sources.0.join("Cargo.toml"))
                .unwrap()
                .replace("members = [", "members = [\"components/reader-host\", ")
        ),
    );
    sources.write("components/reader-host/Cargo.toml", &host);
    sources.write("components/reader-host/src/lib.rs", "//! The host.\n");
    assert_eq!(sources.findings(), vec![]);
    let sandbox = PROV_MODULES[1].1;
    let refused = |what: &str, at: &str| {
        let found = sources.findings();
        assert!(
            found
                .iter()
                .any(|(file, _, why)| file == at && *why == Why::WidensAnAdmittedPath),
            "{what}: {found:?}"
        );
    };

    // The reviewer's alias: a module of the same name as a value, beside it.
    sources.write(
        "prov/src/module/sandbox.rs",
        &format!("{sandbox}pub use crate::proxy as PROVIDER_ADDRESS_SPACE;\n"),
    );
    refused("an alias riding on a value", "prov/src/module/sandbox.rs");
    // A nested re-export: the name defined elsewhere and re-exported here.
    sources.write(
        "prov/src/module/sandbox.rs",
        &sandbox.replace(
            "pub fn isolated() {}\n",
            "mod inner {\n    pub fn isolated() {}\n}\npub use inner::isolated;\n",
        ),
    );
    refused("a nested re-export", "prov/src/module/sandbox.rs");
    for widened in [
        format!("{sandbox}pub mod kv_bucket {{}}\npub use crate::proxy::*;\n"),
        format!("{sandbox}pub fn isolated() {{}}\n"),
        sandbox.replace("pub const PROVIDER_ADDRESS_SPACE: u64 = 1;\n", ""),
    ] {
        sources.write("prov/src/module/sandbox.rs", &widened);
        refused(
            "a glob, a second definition or none",
            "prov/src/module/sandbox.rs",
        );
    }
    sources.write(
        "prov/src/proxy/settings.rs",
        &format!("{}pub mod StoreAddress {{}}\n", PROV_MODULES[6].1),
    );
    refused("a module beside a function", "prov/src/proxy/settings.rs");
    sources.write("prov/src/proxy/settings.rs", PROV_MODULES[6].1);
    sources.write("prov/src/module/sandbox.rs", sandbox);
    assert_eq!(sources.findings(), vec![]);

    // The module redirected under the admitted path's way, through its parent; a `#[path]` elsewhere in the package;
    // a directory beside a module's file; the library's own path moved in the manifest.
    sources.write("prov/src/module/sandbox_wide.rs", sandbox);
    sources.write(
        "prov/src/module/mod.rs",
        "//! The runtime.\n#[path = \"sandbox_wide.rs\"]\npub mod sandbox;\n",
    );
    refused("a redirect in module/mod.rs", "prov/src/module/mod.rs");
    sources.write("prov/src/module/mod.rs", PROV_MODULES[0].1);
    sources.write(
        "prov/src/catalogue.rs",
        "#[path = \"elsewhere.rs\"]\nmod elsewhere;\n",
    );
    refused("a #[path] anywhere under src", "prov/src/catalogue.rs");
    std::fs::remove_file(sources.0.join("prov/src/catalogue.rs")).unwrap();
    sources.write("prov/src/proxy/kv/mod.rs", "pub struct KvResolver;\n");
    refused("a directory beside a module's file", "prov/src/proxy/kv");
    std::fs::remove_dir_all(sources.0.join("prov/src/proxy/kv")).unwrap();
    let prov_manifest = std::fs::read_to_string(sources.0.join("prov/Cargo.toml")).unwrap();
    sources.write(
        "prov/Cargo.toml",
        &format!("{prov_manifest}\n[lib]\npath = \"src/lib_wide.rs\"\n"),
    );
    refused("a [lib] table", "prov/Cargo.toml");
    sources.write("prov/Cargo.toml", &prov_manifest);
    assert_eq!(sources.findings(), vec![]);

    // The real crate's items are each defined where its admitted paths re-export them from.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../components/prov/src");
    for (module, items) in PROV_CONFINE.iter().chain(PROV_KEYS) {
        let file = root.join(format!("{}.rs", module.replace("::", "/")));
        let text = std::fs::read_to_string(&file).unwrap();
        for item in *items {
            assert!(
                defined_in(&text, item),
                "{item} is defined in {}",
                file.display()
            );
        }
    }
}

/// `rutent_prov::keys` is a facade over a process role's statement keys alone (`T-015-05`, the reviewer's Low on
/// `T-015-04`): it re-exports the resolver interface, the facade and the settings it is built from, and not the bucket,
/// the vault resolver, the store's token or its transport; and the facade's own public items name none of them, so a
/// field, a getter, a re-export or a `Deref` can't hand one back to the reader host.
// verifies: DEC-28, DEC-2, REQ-MDL-6
#[test]
fn providers_keys_path_is_a_facade_over_statement_keys_that_gives_back_nothing_it_keeps_inside() {
    let host = manifest(
        READER_HOST,
        "rutent-contracts = { path = \"../../contracts\" }\nrutent-prov = { path = \"../prov\" }",
        "",
    );
    let sources = Sources::new("reader-host-facade");
    sources.write(
        "Cargo.toml",
        &format!(
            "{}\n",
            std::fs::read_to_string(sources.0.join("Cargo.toml"))
                .unwrap()
                .replace("members = [", "members = [\"components/reader-host\", ")
        ),
    );
    sources.write("components/reader-host/Cargo.toml", &host);
    sources.write(
        "components/reader-host/src/lib.rs",
        "use rutent_prov::keys::{KeyStoreSettings, Resolver, StatementKeys};\n",
    );
    assert_eq!(sources.findings(), vec![]);

    // What T-015-04's `keys` re-exported beside the resolver: each, put back, widens the path.
    for widened in [
        "pub use crate::proxy::settings::Bucket;\n",
        "pub use crate::proxy::settings::kv_bucket;\n",
        "pub use crate::proxy::kv::{KvConfig, KvResolver};\n",
        "pub use crate::proxy::kv::Token;\n",
        "pub use crate::proxy::https::HttpsTransport;\n",
        "pub use crate::proxy::transport::TransportError;\n",
    ] {
        let keys = format!("{PROV_KEYS_FILE}{widened}");
        sources.write("prov/src/keys.rs", &keys);
        expect(
            sources.findings(),
            &[("prov/src/keys.rs", 1, Why::WidensAnAdmittedPath)],
        );
    }
    // A process role's settings, or the proxy's that hold them, beside the others from their file: a role's settings
    // carry its `secrets` path, which the keys' resolver could be pointed at (the bypass of the first facade, #354's
    // review, L1).
    for role in ["RoleSettings", "ProxySettings"] {
        sources.write(
            "prov/src/keys.rs",
            &PROV_KEYS_FILE.replace(
                "{InvalidSettings, StoreAddress",
                &format!("{{InvalidSettings, {role}, StoreAddress"),
            ),
        );
        expect(
            sources.findings(),
            &[("prov/src/keys.rs", 1, Why::WidensAnAdmittedPath)],
        );
    }
    sources.write("prov/src/keys.rs", PROV_KEYS_FILE);
    assert_eq!(sources.findings(), vec![]);

    // The facade hands back what it keeps inside: a public field, a getter, a re-export, a `Deref`, the bucket's
    // secrets. Each is refused at its line.
    let facade = "prov/src/proxy/statement_keys.rs";
    let lines = PROV_FACADE_FILE.lines().count();
    for (widening, line) in [
        (
            "pub struct Exposed {\n    pub keys: KvResolver<HttpsTransport>,\n}\n",
            2,
        ),
        (
            "impl StatementKeys {\n    pub fn transport(&self) -> &HttpsTransport { todo!() }\n}\n",
            2,
        ),
        ("pub use super::kv::Token;\n", 1),
        (
            "impl core::ops::Deref for StatementKeys {\n    type Target = KvResolver;\n    \
             fn deref(&self) -> &u8 { todo!() }\n}\n",
            2,
        ),
        (
            "impl StatementKeys {\n    pub fn secrets(&self) -> Box<dyn Resolver> { todo!() }\n}\n",
            2,
        ),
        ("pub type Store = Bucket<u8>;\n", 1),
        // The facade built from a process role's settings again, or its settings given a `secrets` path: L1's bypass.
        (
            "impl StatementKeys {\n    pub fn for_role(role: &RoleSettings) -> u8 { todo!() }\n}\n",
            2,
        ),
        ("pub struct Settings {\n    pub secrets: String,\n}\n", 2),
        (
            "pub fn bucket() -> u8 {\n    kv_bucket()\n}\npub const T: Token = todo!();\n",
            4,
        ),
    ] {
        sources.write(facade, &format!("{PROV_FACADE_FILE}{widening}"));
        expect(
            sources.findings(),
            &[(facade, lines + line, Why::WidensAnAdmittedPath)],
        );
    }
    // Named in a private item or a body, it stays inside.
    sources.write(
        facade,
        &format!(
            "{PROV_FACADE_FILE}fn inner() -> KvResolver {{ todo!() }}\n\
             pub(crate) fn transport() -> HttpsTransport {{ todo!() }}\n"
        ),
    );
    assert_eq!(sources.findings(), vec![]);
    // And a facade that isn't there fails closed.
    std::fs::remove_file(sources.0.join(facade)).unwrap();
    assert!(
        sources
            .findings()
            .iter()
            .any(|(file, _, why)| file == facade && *why == Why::WidensAnAdmittedPath)
    );

    // The real crate: `keys` re-exports the facade, and the facade's public items name nothing it keeps inside.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../components/prov/src");
    assert!(
        PROV_KEYS
            .iter()
            .any(|(module, items)| *module == PROV_KEYS_FACADE && items.contains(&"StatementKeys"))
    );
    let real = std::fs::read_to_string(root.join("proxy/statement_keys.rs")).unwrap();
    assert_eq!(
        public_items_naming_any(&real, PROV_KEYS_WITHHELD),
        Vec::<usize>::new()
    );
    assert!(
        PROV_KEYS
            .iter()
            .flat_map(|(_, items)| items.iter())
            .all(|item| !PROV_KEYS_WITHHELD.contains(item))
    );
}

/// The scenario assembly's tests run the simulated networks' readers in the test harness (`T-024-25`), so the assembly
/// may take the reader host as a dev-dependency, and never as a dependency of its build; the host never takes the
/// assembly, either way (`T-020-59`'s review, L6).
// verifies: DEC-28, DEC-2
#[test]
fn scenario_assembly_takes_the_reader_host_for_its_tests_only() {
    let contracts = manifest("rutent-contracts", "", "");
    let host_dep = format!("{READER_HOST} = {{ path = \"../../components/reader-host\" }}");
    let host = |deps: &str, dev: &str| manifest(READER_HOST, deps, dev);
    let allowed = "rutent-contracts = { path = \"../../contracts\" }";
    // The assembly's tests: allowed.
    let tests_only = fixture(&[
        &contracts,
        &host(allowed, ""),
        &manifest("rutent-scenarios", allowed, &host_dep),
    ]);
    let all: Vec<&str> = tests_only.keys().map(String::as_str).collect();
    assert_eq!(violations(&tests_only, &all), vec![]);
    // The assembly's build: refused.
    let built = fixture(&[
        &contracts,
        &host(allowed, ""),
        &manifest("rutent-scenarios", &format!("{allowed}\n{host_dep}"), ""),
    ]);
    let all: Vec<&str> = built.keys().map(String::as_str).collect();
    let pairs: Vec<_> = violations(&built, &all)
        .into_iter()
        .map(|v| (v.krate, v.dep))
        .collect();
    assert_eq!(
        pairs,
        [("rutent-scenarios".to_string(), READER_HOST.to_string())]
    );
    // The other way: the host on the assembly, as a dependency or a dev-dependency, is refused.
    let on_assembly = "rutent-scenarios = { path = \"../../conformance/scenarios\" }";
    for (deps, dev) in [
        (format!("{allowed}\n{on_assembly}"), String::new()),
        (allowed.to_string(), on_assembly.to_string()),
    ] {
        let graph = fixture(&[
            &contracts,
            &host(&deps, &dev),
            &manifest("rutent-scenarios", allowed, ""),
        ]);
        let pairs: Vec<_> = violations(&graph, &[READER_HOST])
            .into_iter()
            .map(|v| v.dep)
            .collect();
        assert_eq!(pairs, ["rutent-scenarios"]);
    }
}

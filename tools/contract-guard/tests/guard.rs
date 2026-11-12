//! Behaviour of the contract guard core, on in-memory fixtures. No test here creates a commit (`T-001-05`, `DEC-4`).

use rutent_contract_guard::{Files, Violation, check, parse_contracts, split_nul_names};
use std::collections::HashMap;

const CONTRACTS: &str = "\
# name             artefacts                       rust module
provider-role      contracts/provider-role         contracts/src/provider_role
module-interface   contracts/module-interface      contracts/src/module_interface
router-api         contracts/router-api            contracts/src/router_api
state-machine      contracts/state-machine         contracts/src/state_machine
host               contracts/state-machine/host    contracts/src/state_machine/host
";

const SHARED_CHANGELOG: &str =
    "# contracts crate changelog\n\n- Layout and conventions. (DEC-3, DEC-4, DEC-5)\n";

/// The base `CONTRACTS`: the same five contracts, so a fixture contract is known at the base unless a test drops it.
const CONTRACTS_WITHOUT_MODULE_INTERFACE: &str = "\
# name             artefacts                       rust module
provider-role      contracts/provider-role         contracts/src/provider_role
router-api         contracts/router-api            contracts/src/router_api
state-machine      contracts/state-machine         contracts/src/state_machine
host               contracts/state-machine/host    contracts/src/state_machine/host
";

/// The head `PLAN.md`, cut down to the rows of its decisions table that the fixtures name.
const PLAN: &str = "\
| `DEC-3` | Layout. | x | accepted |
| `DEC-4` | Versioning. | x | accepted |
| `DEC-5` | Fixtures. | x | accepted |
| `DEC-6` | Dependencies. | x | accepted |
| `DEC-20` | Owner's click. | x | accepted |
";

/// A root `Cargo.lock` where `rutent-contracts` depends on `serde`, which depends on `serde_core`; `other` is
/// a package nothing in the contracts crate reaches.
fn lock(serde: &str, other: &str) -> String {
    format!(
        "version = 4\n\n\
[[package]]\nname = \"other\"\nversion = \"{other}\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"o{other}\"\n\n\
[[package]]\nname = \"rutent-contracts\"\nversion = \"0.0.0\"\ndependencies = [\n \"serde\",\n]\n\n\
[[package]]\nname = \"serde\"\nversion = \"{serde}\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"s{serde}\"\ndependencies = [\n \"serde_core\",\n]\n\n\
[[package]]\nname = \"serde_core\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"c\"\n"
    )
}

/// `lock`, with `serde_core` also depending on a package that `rutent-contracts` already reaches through `serde`.
fn lock_with_extra_edge(serde: &str) -> String {
    lock(serde, "1.0.0").replace(
        "checksum = \"c\"\n",
        "checksum = \"c\"\ndependencies = [\n \"serde\",\n]\n",
    )
}

struct Fixture {
    base: HashMap<String, String>,
    head: HashMap<String, String>,
}

impl Default for Fixture {
    fn default() -> Self {
        let mut base = HashMap::new();
        base.insert("contracts/CONTRACTS".to_string(), CONTRACTS.to_string());
        let mut head = HashMap::new();
        head.insert("PLAN.md".to_string(), PLAN.to_string());
        Fixture { base, head }
    }
}

impl Fixture {
    fn at_both(mut self, path: &str, text: &str) -> Self {
        self.base.insert(path.to_string(), text.to_string());
        self.head.insert(path.to_string(), text.to_string());
        self
    }
    fn base_only(mut self, path: &str, text: &str) -> Self {
        self.base.insert(path.to_string(), text.to_string());
        self
    }
    fn head_only(mut self, path: &str, text: &str) -> Self {
        self.head.insert(path.to_string(), text.to_string());
        self
    }
    fn changed_head(mut self, path: &str, text: &str) -> Self {
        self.head.insert(path.to_string(), text.to_string());
        self
    }
}

impl Files for Fixture {
    fn base_files(&self) -> Result<Vec<String>, String> {
        Ok(self.base.keys().cloned().collect())
    }

    fn base(&self, path: &str) -> Option<String> {
        self.base.get(path).cloned()
    }
    fn head(&self, path: &str) -> Option<String> {
        self.head.get(path).cloned()
    }
}

fn paths(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn run(changed: &[&str], fixture: &Fixture) -> Vec<Violation> {
    run_with(changed, CONTRACTS, fixture)
}

fn run_with(changed: &[&str], head_contracts: &str, fixture: &Fixture) -> Vec<Violation> {
    check(&paths(changed), head_contracts, fixture).expect("the fixture CONTRACTS parses")
}

// verifies: DEC-4
#[test]
fn contract_change_without_a_version_bump_fails() {
    let fixture = Fixture::default()
        .at_both(
            "contracts/provider-role/CHANGELOG.md",
            "# provider-role changelog\n\n## 0.1.0\n- Adverts. (DEC-4, T-002-01)\n",
        )
        .changed_head("contracts/provider-role/schemas/advert.schema.json", "{}");
    let violations = run(
        &["contracts/provider-role/schemas/advert.schema.json"],
        &fixture,
    );
    assert!(
        violations
            .iter()
            .any(|v| v.subject == "provider-role" && v.message.contains("version")),
        "expected a version violation for provider-role, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn changelog_entry_without_a_plan_decision_fails() {
    let fixture = Fixture::default()
        .base_only("contracts/provider-role/CHANGELOG.md", "# provider-role changelog\n\n## 0.1.0\n- Adverts. (DEC-4, T-002-01)\n")
        .head_only(
            "contracts/provider-role/CHANGELOG.md",
            "# provider-role changelog\n\n## 0.2.0\n- Operations, with no decision named. (T-002-02)\n\n## 0.1.0\n- Adverts. (DEC-4, T-002-01)\n",
        )
        .changed_head("contracts/provider-role/schemas/operation.schema.json", "{}");
    let violations = run(
        &[
            "contracts/provider-role/CHANGELOG.md",
            "contracts/provider-role/schemas/operation.schema.json",
        ],
        &fixture,
    );
    assert!(
        violations
            .iter()
            .any(|v| v.subject == "provider-role" && v.message.contains("DEC")),
        "expected a DEC violation for provider-role, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn new_contract_with_a_decided_changelog_passes() {
    let fixture = Fixture::default()
        .base_only("contracts/CONTRACTS", CONTRACTS_WITHOUT_MODULE_INTERFACE)
        .head_only(
            "contracts/module-interface/CHANGELOG.md",
            "# module-interface changelog\n\n## 0.1.0\n- Interface schemas. (DEC-4, T-003-01)\n",
        )
        .changed_head("contracts/module-interface/schemas/call.schema.json", "{}");
    let violations = run(
        &[
            "contracts/module-interface/CHANGELOG.md",
            "contracts/module-interface/schemas/call.schema.json",
        ],
        &fixture,
    );
    assert!(
        violations.is_empty(),
        "a new contract should pass, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn shared_file_change_without_entry_fails() {
    let fixture = Fixture::default()
        .at_both("contracts/CHANGELOG.md", SHARED_CHANGELOG)
        .changed_head("contracts/README.md", "# rutent contracts, reworded\n");
    let violations = run(&["contracts/README.md"], &fixture);
    assert!(
        violations.iter().any(|v| v.subject == "shared"),
        "expected a shared-file violation, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn shared_file_change_with_a_decided_entry_passes() {
    let head_changelog = format!("{SHARED_CHANGELOG}- Reworded the README. (DEC-6)\n");
    let fixture = Fixture::default()
        .base_only("contracts/CHANGELOG.md", SHARED_CHANGELOG)
        .head_only("contracts/CHANGELOG.md", &head_changelog)
        .changed_head("contracts/README.md", "# rutent contracts, reworded\n");
    let violations = run(&["contracts/README.md", "contracts/CHANGELOG.md"], &fixture);
    assert!(
        violations.is_empty(),
        "expected no violations, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn shared_contracts_file_changed_with_only_a_contracts_bump_fails() {
    let fixture = Fixture::default()
        .at_both("contracts/CHANGELOG.md", SHARED_CHANGELOG)
        .base_only("contracts/provider-role/CHANGELOG.md", "# provider-role changelog\n\n## 0.1.0\n- Adverts. (DEC-4, T-002-01)\n")
        .head_only(
            "contracts/provider-role/CHANGELOG.md",
            "# provider-role changelog\n\n## 0.2.0\n- Operations. (DEC-4, T-002-02)\n\n## 0.1.0\n- Adverts. (DEC-4, T-002-01)\n",
        )
        .changed_head("contracts/provider-role/schemas/operation.schema.json", "{}")
        .changed_head("contracts/README.md", "# rutent contracts, reworded\n");
    let violations = run(
        &[
            "contracts/README.md",
            "contracts/provider-role/CHANGELOG.md",
            "contracts/provider-role/schemas/operation.schema.json",
        ],
        &fixture,
    );
    assert!(
        violations.iter().any(|v| v.subject == "shared"),
        "a contract's bump must not cover a shared file, got {violations:?}"
    );
    assert!(
        !violations.iter().any(|v| v.subject == "provider-role"),
        "provider-role itself is bumped, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn lockfile_change_moving_a_contract_dependency_without_a_bump_fails() {
    let fixture = Fixture::default()
        .at_both("contracts/CHANGELOG.md", SHARED_CHANGELOG)
        .base_only("Cargo.lock", &lock("1.0.1", "1.0.0"))
        .head_only("Cargo.lock", &lock("1.0.2", "1.0.0"));
    let violations = run(&["Cargo.lock"], &fixture);
    assert!(
        violations.iter().any(|v| v.message.contains("Cargo.lock")),
        "a moved serde must name the lockfile, got {violations:?}"
    );
    assert!(
        violations.iter().any(|v| v.subject == "router-api"),
        "every contract needs a bump when its dependency moves, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn lockfile_change_moving_a_contract_dependency_with_every_bump_passes() {
    let mut fixture = Fixture::default()
        .at_both("contracts/CHANGELOG.md", SHARED_CHANGELOG)
        .base_only("Cargo.lock", &lock("1.0.1", "1.0.0"))
        .head_only("Cargo.lock", &lock("1.0.2", "1.0.0"));
    for dir in [
        "provider-role",
        "module-interface",
        "router-api",
        "state-machine",
        "state-machine/host",
    ] {
        fixture = fixture
            .base_only(
                &format!("contracts/{dir}/CHANGELOG.md"),
                "# changelog\n\n## 1.0.0\n- First. (DEC-4)\n",
            )
            .head_only(
                &format!("contracts/{dir}/CHANGELOG.md"),
                "# changelog\n\n## 1.0.1\n- serde moved. (DEC-4, DEC-20)\n\n## 1.0.0\n- First. (DEC-4)\n",
            );
    }
    let violations = run(&["Cargo.lock"], &fixture);
    assert!(
        violations.is_empty(),
        "bumped contracts cover the move, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn lockfile_change_adding_only_dependency_edges_needs_the_crate_changelog() {
    let fixture = Fixture::default()
        .at_both("contracts/CHANGELOG.md", SHARED_CHANGELOG)
        .base_only("Cargo.lock", &lock("1.0.1", "1.0.0"))
        .head_only("Cargo.lock", &lock_with_extra_edge("1.0.1"));
    let violations = run(&["Cargo.lock"], &fixture);
    assert!(
        violations
            .iter()
            .any(|v| v.subject == "shared" && v.message.contains("serde_core")),
        "new edges need the shared changelog entry, got {violations:?}"
    );
    assert!(
        !violations.iter().any(|v| v.subject != "shared"),
        "new edges need no contract's bump, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn lockfile_change_adding_only_dependency_edges_with_the_crate_changelog_passes() {
    let head_changelog = format!("{SHARED_CHANGELOG}- A dependency gained edges. (DEC-4)\n");
    let fixture = Fixture::default()
        .base_only("contracts/CHANGELOG.md", SHARED_CHANGELOG)
        .head_only("contracts/CHANGELOG.md", &head_changelog)
        .base_only("Cargo.lock", &lock("1.0.1", "1.0.0"))
        .head_only("Cargo.lock", &lock_with_extra_edge("1.0.1"));
    let violations = run(&["Cargo.lock", "contracts/CHANGELOG.md"], &fixture);
    assert!(
        violations.is_empty(),
        "an entry covers new edges, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn dependency_edge_change_with_a_version_change_still_needs_every_bump() {
    let head_changelog = format!("{SHARED_CHANGELOG}- A dependency moved. (DEC-4)\n");
    let fixture = Fixture::default()
        .base_only("contracts/CHANGELOG.md", SHARED_CHANGELOG)
        .head_only("contracts/CHANGELOG.md", &head_changelog)
        .base_only("Cargo.lock", &lock("1.0.1", "1.0.0"))
        .head_only("Cargo.lock", &lock_with_extra_edge("1.0.2"));
    let violations = run(&["Cargo.lock", "contracts/CHANGELOG.md"], &fixture);
    assert!(
        violations.iter().any(|v| v.subject == "router-api"),
        "a version change needs every bump whatever its edges do, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn new_edge_to_a_package_not_reached_before_needs_every_bump() {
    let head_changelog = format!("{SHARED_CHANGELOG}- A dependency gained an edge. (DEC-4)\n");
    let head_lock = lock("1.0.1", "1.0.0").replace(
        "checksum = \"c\"\n",
        "checksum = \"c\"\ndependencies = [\n \"other\",\n]\n",
    );
    let fixture = Fixture::default()
        .base_only("contracts/CHANGELOG.md", SHARED_CHANGELOG)
        .head_only("contracts/CHANGELOG.md", &head_changelog)
        .base_only("Cargo.lock", &lock("1.0.1", "1.0.0"))
        .head_only("Cargo.lock", &head_lock);
    let violations = run(&["Cargo.lock", "contracts/CHANGELOG.md"], &fixture);
    assert!(
        violations
            .iter()
            .any(|v| v.subject == "router-api" || v.message.contains("other")),
        "a newly reached package needs every bump, got {violations:?}"
    );
    assert!(
        violations.iter().any(|v| v.subject == "router-api"),
        "every contract needs its bump, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn lockfile_change_to_an_unrelated_package_passes() {
    let fixture = Fixture::default()
        .at_both("contracts/CHANGELOG.md", SHARED_CHANGELOG)
        .base_only("Cargo.lock", &lock("1.0.1", "1.0.0"))
        .head_only("Cargo.lock", &lock("1.0.1", "2.0.0"));
    let violations = run(&["Cargo.lock"], &fixture);
    assert!(
        violations.is_empty(),
        "a package the contracts crate does not reach is free, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn lockfile_block_after_the_packages_is_not_part_of_the_last_package() {
    let tail = "\n[[patch.unused]]\nname = \"serde_core\"\nversion = \"9.9.9\"\n";
    let fixture = Fixture::default()
        .at_both("contracts/CHANGELOG.md", SHARED_CHANGELOG)
        .base_only("Cargo.lock", &lock("1.0.1", "1.0.0"))
        .head_only("Cargo.lock", &format!("{}{tail}", lock("1.0.1", "1.0.0")));
    let violations = run(&["Cargo.lock"], &fixture);
    assert!(
        violations.is_empty(),
        "an unused patch block moves no package, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn changelog_entry_naming_an_undefined_plan_decision_fails() {
    let fixture = Fixture::default()
        .base_only("contracts/provider-role/CHANGELOG.md", "# provider-role changelog\n\n## 0.1.0\n- Adverts. (DEC-4, T-002-01)\n")
        .head_only(
            "contracts/provider-role/CHANGELOG.md",
            "# provider-role changelog\n\n## 0.2.0\n- Operations. (DEC-4, DEC-99, T-002-02)\n\n## 0.1.0\n- Adverts. (DEC-4, T-002-01)\n",
        )
        .changed_head("contracts/provider-role/schemas/operation.schema.json", "{}");
    let violations = run(
        &[
            "contracts/provider-role/CHANGELOG.md",
            "contracts/provider-role/schemas/operation.schema.json",
        ],
        &fixture,
    );
    assert!(
        violations
            .iter()
            .any(|v| v.subject == "provider-role" && v.message.contains("DEC-99")),
        "DEC-99 is not in PLAN.md, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn shared_changelog_entry_naming_an_undefined_plan_decision_fails() {
    let head_changelog = format!("{SHARED_CHANGELOG}- Reworded the README. (DEC-99)\n");
    let fixture = Fixture::default()
        .base_only("contracts/CHANGELOG.md", SHARED_CHANGELOG)
        .head_only("contracts/CHANGELOG.md", &head_changelog)
        .changed_head("contracts/README.md", "# rutent contracts, reworded\n");
    let violations = run(&["contracts/README.md", "contracts/CHANGELOG.md"], &fixture);
    assert!(
        violations.iter().any(|v| v.message.contains("DEC-99")),
        "an undefined decision in the shared changelog fails, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn host_path_belongs_to_host_not_state_machine() {
    let state_machine_v1 = "# state-machine changelog\n\n## 0.1.0\n- States. (DEC-5, T-005-01)\n";
    let state_machine_v2 = "# state-machine changelog\n\n## 0.2.0\n- Inputs. (DEC-5, T-005-02)\n\n## 0.1.0\n- States. (DEC-5, T-005-01)\n";
    let host_v1 = "# host changelog\n\n## 0.1.0\n- Host records. (DEC-3, T-005-03)\n";
    let fixture = Fixture::default()
        .at_both("contracts/state-machine/host/CHANGELOG.md", host_v1)
        .base_only("contracts/state-machine/CHANGELOG.md", state_machine_v1)
        .head_only("contracts/state-machine/CHANGELOG.md", state_machine_v2)
        .changed_head("contracts/src/state_machine/host/mod.rs", "pub mod host;\n");
    let violations = run(
        &[
            "contracts/state-machine/CHANGELOG.md",
            "contracts/src/state_machine/host/mod.rs",
        ],
        &fixture,
    );
    assert!(
        violations.iter().any(|v| v.subject == "host"),
        "a host path must be owned by host, got {violations:?}"
    );
    assert!(
        !violations.iter().any(|v| v.subject == "state-machine"),
        "state-machine owns no changed path, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn version_must_be_greater_than_the_base() {
    let fixture = Fixture::default()
        .base_only("contracts/router-api/CHANGELOG.md", "# router-api changelog\n\n## 0.2.0\n- Routes. (DEC-4, T-004-01)\n")
        .head_only("contracts/router-api/CHANGELOG.md", "# router-api changelog\n\n## 0.1.9\n- Routes again. (DEC-4, T-004-02)\n\n## 0.2.0\n- Routes. (DEC-4, T-004-01)\n")
        .changed_head("contracts/router-api/schemas/route.schema.json", "{}");
    let violations = run(
        &[
            "contracts/router-api/CHANGELOG.md",
            "contracts/router-api/schemas/route.schema.json",
        ],
        &fixture,
    );
    assert!(
        violations
            .iter()
            .any(|v| v.subject == "router-api" && v.message.contains("version")),
        "a lower version must fail, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn contract_test_file_belongs_to_its_contract() {
    let changelog = "# provider-role changelog\n\n## 0.1.0\n- Adverts. (DEC-4, T-002-01)\n";
    for file in [
        "contracts/tests/provider_role.rs",
        "contracts/tests/provider-role.rs",
    ] {
        let fixture = Fixture::default()
            .at_both("contracts/provider-role/CHANGELOG.md", changelog)
            .at_both("contracts/CHANGELOG.md", SHARED_CHANGELOG)
            .changed_head(file, "// provider-role tests\n");
        let violations = run(&[file], &fixture);
        assert!(
            violations
                .iter()
                .any(|v| v.subject == "provider-role" && v.message.contains("version")),
            "{file} should belong to provider-role and need its version bump, got {violations:?}"
        );
        assert!(
            !violations.iter().any(|v| v.subject == "shared"),
            "{file} is not a shared file, got {violations:?}"
        );
    }
}

// verifies: DEC-4
#[test]
fn other_contract_test_files_stay_shared() {
    let fixture = Fixture::default()
        .at_both("contracts/CHANGELOG.md", SHARED_CHANGELOG)
        .changed_head("contracts/tests/helpers.rs", "// shared helpers\n");
    let violations = run(&["contracts/tests/helpers.rs"], &fixture);
    assert!(
        violations.iter().any(|v| v.subject == "shared"),
        "a test file that names no contract stays shared, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn nul_separated_paths_keep_names_git_would_quote() {
    let output = "contracts/provider-role/schemas/\u{e9}.json\0contracts/README.md\0";
    assert_eq!(
        split_nul_names(output),
        vec![
            "contracts/provider-role/schemas/\u{e9}.json".to_string(),
            "contracts/README.md".to_string()
        ]
    );
}

// verifies: DEC-4
#[test]
fn contract_remapped_away_still_needs_its_bump() {
    let head_contracts = CONTRACTS.replace(
        "contracts/provider-role         ",
        "contracts/provider-role-v2      ",
    );
    assert_ne!(
        head_contracts, CONTRACTS,
        "the fixture must remap provider-role"
    );
    let fixture = Fixture::default()
        .base_only("contracts/CONTRACTS", CONTRACTS)
        .base_only(
            "contracts/provider-role/CHANGELOG.md",
            "# provider-role changelog\n\n## 0.1.0\n- Adverts. (DEC-4, T-002-01)\n",
        )
        .at_both("contracts/CHANGELOG.md", SHARED_CHANGELOG);
    let violations = run_with(
        &["contracts/provider-role/schemas/advert.schema.json"],
        &head_contracts,
        &fixture,
    );
    assert!(
        violations.iter().any(|v| v.subject == "provider-role"),
        "files the base mapped to provider-role must still require its bump, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn decision_lookalikes_do_not_name_a_decision() {
    let fixture = Fixture::default()
        .base_only("contracts/provider-role/CHANGELOG.md", "# provider-role changelog\n\n## 0.1.0\n- Adverts. (DEC-4, T-002-01)\n")
        .head_only(
            "contracts/provider-role/CHANGELOG.md",
            "# provider-role changelog\n\n## 0.2.0\n- Operations. (NODEC-4, T-002-02)\n- Other. (XDEC-4, T-002-03)\n\n## 0.1.0\n- Adverts. (DEC-4, T-002-01)\n",
        )
        .changed_head("contracts/provider-role/schemas/operation.schema.json", "{}");
    let violations = run(
        &[
            "contracts/provider-role/CHANGELOG.md",
            "contracts/provider-role/schemas/operation.schema.json",
        ],
        &fixture,
    );
    assert!(
        violations
            .iter()
            .any(|v| v.subject == "provider-role" && v.message.contains("DEC")),
        "NODEC-4 and XDEC-4 name no decision, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn heading_edit_alone_is_not_a_version_bump() {
    let fixture = Fixture::default()
        .base_only(
            "contracts/provider-role/CHANGELOG.md",
            "# provider-role changelog\n\n## 0.1.0\n- Adverts. (DEC-4, T-002-01)\n",
        )
        .head_only(
            "contracts/provider-role/CHANGELOG.md",
            "# provider-role changelog\n\n## 0.2.0\n- Adverts. (DEC-4, T-002-01)\n",
        )
        .changed_head("contracts/provider-role/schemas/advert.schema.json", "{}");
    let violations = run(
        &[
            "contracts/provider-role/CHANGELOG.md",
            "contracts/provider-role/schemas/advert.schema.json",
        ],
        &fixture,
    );
    assert!(
        violations
            .iter()
            .any(|v| v.subject == "provider-role" && v.message.contains("bullet")),
        "a new heading over the old bullets is not a bump, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn contracts_sharing_a_directory_are_rejected() {
    let overlapping = "\
provider-role      contracts/provider-role    contracts/src/provider_role
router-api         contracts/provider-role    contracts/src/router_api
";
    assert!(
        parse_contracts(overlapping).is_err(),
        "two contracts with the same artefact directory must not parse"
    );
}

// verifies: DEC-4
#[test]
fn loose_decision_tokens_do_not_name_a_decision() {
    for bullet in [
        "- Operations. (see DEC-4x, T-002-02)",
        "- Operations. (_DEC-4, T-002-02)",
    ] {
        let head = format!(
            "# provider-role changelog\n\n## 0.2.0\n{bullet}\n\n## 0.1.0\n- Adverts. (DEC-4, T-002-01)\n"
        );
        let fixture = Fixture::default()
            .base_only(
                "contracts/provider-role/CHANGELOG.md",
                "# provider-role changelog\n\n## 0.1.0\n- Adverts. (DEC-4, T-002-01)\n",
            )
            .head_only("contracts/provider-role/CHANGELOG.md", &head)
            .changed_head(
                "contracts/provider-role/schemas/operation.schema.json",
                "{}",
            );
        let violations = run(
            &[
                "contracts/provider-role/CHANGELOG.md",
                "contracts/provider-role/schemas/operation.schema.json",
            ],
            &fixture,
        );
        assert!(
            violations
                .iter()
                .any(|v| v.subject == "provider-role" && v.message.contains("DEC")),
            "{bullet} names no decision, got {violations:?}"
        );
    }
}

// verifies: DEC-4
#[test]
fn listed_contract_with_no_base_files_first_version_passes() {
    // The base CONTRACTS lists module-interface, but no file of it exists at the base yet.
    let fixture = Fixture::default()
        .head_only(
            "contracts/module-interface/CHANGELOG.md",
            "# module-interface changelog\n\n## 0.1.0\n- Interface schemas. (DEC-4, T-003-01)\n",
        )
        .changed_head("contracts/module-interface/schemas/call.schema.json", "{}");
    let violations = run(
        &[
            "contracts/module-interface/CHANGELOG.md",
            "contracts/module-interface/schemas/call.schema.json",
        ],
        &fixture,
    );
    assert!(
        violations.is_empty(),
        "a contract with no base files starts new, got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn listed_contract_with_no_base_files_needs_a_decision() {
    let fixture = Fixture::default()
        .head_only(
            "contracts/module-interface/CHANGELOG.md",
            "# module-interface changelog\n\n## 0.1.0\n- Interface schemas. (T-003-01)\n",
        )
        .changed_head("contracts/module-interface/schemas/call.schema.json", "{}");
    let violations = run(
        &[
            "contracts/module-interface/CHANGELOG.md",
            "contracts/module-interface/schemas/call.schema.json",
        ],
        &fixture,
    );
    assert_eq!(
        violations.len(),
        1,
        "the missing DEC is the only violation, got {violations:?}"
    );
    assert!(
        violations[0].subject == "module-interface" && violations[0].message.contains("DEC"),
        "got {violations:?}"
    );
}

// verifies: DEC-4
#[test]
fn base_files_without_a_changelog_fail() {
    let fixture = Fixture::default()
        .base_only("contracts/provider-role/schemas/advert.schema.json", "{}")
        .head_only(
            "contracts/provider-role/CHANGELOG.md",
            "# provider-role changelog\n\n## 0.1.0\n- Adverts. (DEC-4, T-002-01)\n",
        )
        .changed_head(
            "contracts/provider-role/schemas/advert.schema.json",
            "{\"v\": 2}",
        );
    let violations = run(
        &[
            "contracts/provider-role/CHANGELOG.md",
            "contracts/provider-role/schemas/advert.schema.json",
        ],
        &fixture,
    );
    assert!(
        violations
            .iter()
            .any(|v| v.subject == "provider-role" && v.message.contains("files")),
        "a contract that owns base files must keep a base changelog, got {violations:?}"
    );
}

/// The state machine's major moves, and the host's schemas reference its shapes; `host` is the host's version at
/// the head, its changelog changed only if it moved.
fn referenced_major(state_machine: &str, host: &str) -> (Fixture, Vec<&'static str>) {
    let host_base = "# host changelog\n\n## 2.1.0\n- no_std. (DEC-4, T-010-01)\n";
    let mut fixture = Fixture::default()
        .base_only(
            "contracts/state-machine/CHANGELOG.md",
            "# state-machine changelog\n\n## 2.1.0\n- no_std. (DEC-4, T-010-01)\n",
        )
        .head_only(
            "contracts/state-machine/CHANGELOG.md",
            &format!(
                "# state-machine changelog\n\n## {state_machine}\n- A new input. (DEC-4, DEC-20, T-020-01)\n\n## 2.1.0\n- no_std. (DEC-4, T-010-01)\n"
            ),
        )
        .at_both(
            "contracts/state-machine/schemas/order.schema.json",
            "{\"$id\": \"urn:rutent:state-machine:order\"}",
        )
        .at_both(
            "contracts/state-machine/host/schemas/snapshot.schema.json",
            "{\"$ref\": \"urn:rutent:state-machine:order\"}",
        )
        .base_only("contracts/state-machine/host/CHANGELOG.md", host_base);
    let mut changed = vec![
        "contracts/state-machine/CHANGELOG.md",
        "contracts/state-machine/schemas/order.schema.json",
    ];
    if host == "2.1.0" {
        fixture = fixture.head_only("contracts/state-machine/host/CHANGELOG.md", host_base);
    } else {
        fixture = fixture.head_only(
            "contracts/state-machine/host/CHANGELOG.md",
            &format!("# host changelog\n\n## {host}\n- Follows state-machine. (DEC-4, DEC-20, T-020-01)\n\n## 2.1.0\n- no_std. (DEC-4, T-010-01)\n"),
        );
        changed.push("contracts/state-machine/host/CHANGELOG.md");
    }
    (fixture, changed)
}

// verifies: DEC-4, DEC-20
#[test]
fn a_referenced_contract_s_major_bump_needs_the_referencing_contract_s_major_bump() {
    let (fixture, changed) = referenced_major("3.0.0", "2.1.0");
    let violations = run(&changed, &fixture);
    assert!(
        violations
            .iter()
            .any(|v| v.subject == "host" && v.message.contains("reference state-machine")),
        "expected the host to need a bump, got {violations:?}"
    );
    let (fixture, changed) = referenced_major("3.0.0", "2.2.0");
    let violations = run(&changed, &fixture);
    assert!(
        violations
            .iter()
            .any(|v| v.subject == "host" && v.message.contains("major bump")),
        "a minor bump of the host is not enough, got {violations:?}"
    );
    let (fixture, changed) = referenced_major("3.0.0", "3.0.0");
    assert_eq!(run(&changed, &fixture), Vec::new());
    // A minor bump of the referenced contract asks nothing of the host.
    let (fixture, changed) = referenced_major("2.2.0", "2.1.0");
    assert_eq!(run(&changed, &fixture), Vec::new());
}

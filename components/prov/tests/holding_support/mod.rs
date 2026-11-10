//! What the plain wallet's tests share (`T-016-02`): the wallet's two components, built by
//! `guests/plain-wallet/build.sh` into the build directory the first time a test asks and again when the guest's
//! source, the WIT world's or a contract's is newer; a platform that has admitted wallets as signed releases, the
//! way `T-013-04` admits any module; and the operator's operations with the tests' placeholder signature.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rutent_conformance::role::{SuiteContext, placeholder_signature};
use rutent_contracts::module_interface::{Declaration, Parts};
use rutent_contracts::provider_role::{Operation, Statement};
use rutent_prov::holding::wallet::Operator;
use rutent_prov::module::types::Admitted;
use serde_json::json;

use crate::common::declaration;
use crate::signed::{self, Signed};

/// The operations a plain wallet handles.
pub const HANDLES: [&str; 4] = [
    "deposit",
    "withdrawal",
    "balance_verification",
    "holding_balance",
];

/// The wallet's two components, built once for the whole test run.
pub fn wallet_binaries() -> Parts<Vec<u8>> {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    let dir = DIR.get_or_init(built_wallet);
    Parts {
        sandboxed: std::fs::read(dir.join("plain-wallet-sandboxed.wasm"))
            .expect("the sandboxed part"),
        outside: std::fs::read(dir.join("plain-wallet-outside.wasm")).expect("the outside part"),
    }
}

fn newest(dir: &Path) -> std::time::SystemTime {
    let mut latest = std::time::SystemTime::UNIX_EPOCH;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return latest;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().is_some_and(|n| n == "target") {
            continue;
        }
        let time = if path.is_dir() {
            newest(&path)
        } else {
            entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        };
        latest = latest.max(time);
    }
    latest
}

/// Builds the wallet with `guests/plain-wallet/build.sh` unless it is built and current.
fn built_wallet() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Some(dir) = std::env::var_os("RUTENT_PLAIN_WALLET") {
        return PathBuf::from(dir);
    }
    // The module guests' tool componentizes the wallet; this makes sure it is built.
    let standins = crate::common::standins();
    let exe = std::env::current_exe().expect("the test's own path");
    let target = exe
        .ancestors()
        .nth(3)
        .expect("a build directory")
        .to_path_buf();
    let out = target.join("rutent-plain-wallet");
    let built = out.join("built");
    let sources = [
        newest(&manifest.join("guests")),
        newest(&manifest.join("src/module/wit")),
        newest(&manifest.join("../../contracts/src")),
    ]
    .into_iter()
    .max()
    .expect("a time");
    let current = std::fs::metadata(&built)
        .and_then(|m| m.modified())
        .is_ok_and(|built| built >= sources);
    if !current {
        let tool = standins
            .parent()
            .map(|t| t.join("module-guests/debug/rutent-wasm-guests"))
            .filter(|t| t.exists());
        let mut build = std::process::Command::new(manifest.join("guests/plain-wallet/build.sh"));
        build
            .arg(&out)
            .env("CARGO_TARGET_DIR", target.join("plain-wallet"));
        if let Some(tool) = tool {
            build.env("RUTENT_GUEST_TOOL", tool);
        }
        let status = build.status().expect("guests/plain-wallet/build.sh runs");
        assert!(
            status.success(),
            "guests/plain-wallet/build.sh failed: {status}"
        );
    }
    out
}

/// A release of the wallet of `provider` at `version`: its declaration, which declares nothing outside (a plain
/// wallet has no outside interaction) and whose statement key is the provider's statements key, and the wallet's
/// binaries.
pub fn wallet_release(provider: &str, version: &str) -> (Declaration, Parts<Vec<u8>>) {
    let mut value = serde_json::to_value(declaration(provider, version, &HANDLES)).expect("JSON");
    value["outside"]["operations"] = json!([]);
    value["outside"]["destinations"] = json!([]);
    value["outside"]["secrets"] = json!([]);
    value["outside"]["statement_key"] =
        signed::catalogue_support::public_key(&signed::catalogue_support::statements_key(provider))
            .as_str()
            .into();
    let declaration: Declaration = serde_json::from_value(value).expect("a valid declaration");
    (declaration, wallet_binaries())
}

/// Signs the wallet's release with the provider's advertising key and admits it, as a provider sends one.
pub fn admit_wallet(s: &mut Signed, provider: &str, version: &str) -> Admitted {
    let (declaration, binaries) = wallet_release(provider, version);
    let key = signed::catalogue_support::advertising_key(provider);
    let assertion = signed::assertion(&declaration, &binaries, &key);
    s.admit(&assertion, &binaries)
}

/// Admits the wallet and gives it its first release's fresh acceptance, so that it runs.
pub fn run_wallet(s: &mut Signed, provider: &str, version: &str) {
    let admitted = admit_wallet(s, provider, version);
    let Admitted::Artefact(artefact) = admitted else {
        panic!("the wallet wasn't admitted: {admitted:?}");
    };
    if artefact.acceptance.accepted.is_none() {
        s.accept(provider, &artefact.assertion_digest, "test-reviewer")
            .expect("the first release is accepted");
    }
    assert!(
        s.platform.active_version(provider).is_some(),
        "the wallet runs"
    );
}

/// The operator, signing with the tests' placeholder.
pub struct TestOperator;

impl Operator for TestOperator {
    fn statement(&self, operation: Operation) -> Statement<Operation> {
        let context = SuiteContext::default();
        serde_json::from_value(json!({
            "deployment": context.deployment,
            "protocol_version": context.protocol_version,
            "shape": rutent_contracts::provider_role::OPERATION_SHAPE,
            "maker": context.operator,
            "made_at": signed::MADE_AT,
            "signature": {
                "scheme": "ed25519",
                "key": context.deployment.operator_key,
                "value": placeholder_signature(),
            },
            "statement": operation,
        }))
        .expect("a valid operation")
    }
}

/// Whether the wallet of `provider` says the order `account` holds at least `amount` of `asset`, asked as a
/// `balance_verification`: the wallet's own word, not the host's record.
pub fn covers(s: &mut Signed, provider: &str, asset: &str, account: &str, amount: u128) -> bool {
    use rutent_contracts::provider_role::{BalanceOutcome, ResultBody};
    use rutent_prov::module::types::Handled;
    let statement = TestOperator.statement(
        serde_json::from_value(json!({
            "kind": "balance_verification",
            "transaction": format!("check-{account}-{amount}"),
            "provider": provider,
            "session": "s-1",
            "account": account,
            "asset": asset,
            "amount": amount.to_string(),
        }))
        .expect("a valid operation"),
    );
    let Handled::Answered(result) = s.platform.handle(provider, &statement) else {
        panic!("the wallet's answer wasn't passed on");
    };
    let ResultBody::BalanceVerification(r) = result.statement else {
        panic!("not a balance verification's result");
    };
    let BalanceOutcome::Answered(answer) = r.outcome else {
        panic!("the wallet refused a balance verification");
    };
    answer.sufficient
}

/// The second wallet provider: a reserve of its own, for a second node.
pub const ETHEREUM_WALLET: &str = "plain-wallet-ethereum";

/// Adds a reserve provider named `name` to the catalogue of `s`: the reserve fixture's advertisement under its own
/// name and no private nodes, with a grant, joined and signed with the provider's advertising key.
pub fn add_reserve_provider(s: &mut Signed, name: &str) {
    use rutent_prov::catalogue::tuples_of;
    let mut ad = signed::catalogue_support::reserve_advertisement();
    ad.provider = name.to_owned();
    ad.private_nodes = Vec::new();
    if let Some(reserve) = ad.offers.reserve.as_mut() {
        reserve.declaration.maker = name.to_owned();
    }
    s.catalogue
        .grant(signed::catalogue_support::grant(
            &format!("g-{name}"),
            name,
            tuples_of(&ad),
            4_000_000_000,
        ))
        .expect("a grant");
    let advertised = rutent_prov::catalogue::Instant(signed::AT.0 - 86_400);
    signed::catalogue_support::join_made_at(&mut s.catalogue, &ad, advertised, signed::AT)
        .expect("the advertisement joins");
}

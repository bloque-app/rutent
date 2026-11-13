//! Party statements (`T-022-02`): what a party stated kept apart from what rutent verified, each statement attributed
//! to its maker, a restatement by anyone else detectable, each signature kept with the bytes it signed, a signature
//! that doesn't verify never logged as attributed, each operator command naming its operator, and each role recorded
//! apart.
//!
//! The runs use the real evidence store (`EvidenceStore`, in memory) and the real log (`AuditLog`, on a memory
//! store): the evidence store's keeper takes each statement's bytes, the log attributes each stated entry, and the
//! providers are granted and advertised through the log first, as `T-022-01`'s are.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rutent_conformance::role::SuiteContext;
use rutent_conformance::signing::{
    Ed25519Key, advertisement, advertising_key, another_key, public_key, sign_value, signing_key,
    statements_key,
};
use rutent_contracts::core_internal::calls::{
    self, AppendFailure, DecisionEntry, Evidence, GeneralEntry, KeptStatements, Log, PolicyLog,
};
use rutent_contracts::core_internal::{
    Actor, Attribution, Decision, Entry, PermissionChange, ReadAnswer, ReadCheck, Receipt,
    StatedEntry,
};
use rutent_contracts::provider_role::{Deployment, TERMS_SHAPE, VERSION, signing_input};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_core_audit::log::merkle::sha256;
use rutent_core_audit::log::{
    AuditLog, Clock, General, MemoryStore, Settings, Store, digest, record,
};
use rutent_core_audit::receipts::{EvidenceStore, Keeper, LogsRead, bearing_out};
use rutent_core_audit::statements::{
    NOT_A_STATEMENT, NOT_BORNE_OUT, Presented, Received, Restatement, Statements, restatements,
    unproven,
};
use serde_json::{Value, json};

// --- The deployment ----------------------------------------------------------------------------------------------

const LOG: &str = "log-main";
const OPERATOR: &str = "operator";
const GRANTED_AT: &str = "2026-11-05T00:00:00Z";
const MADE_AT: &str = "2026-11-06T15:04:05Z";
const RECEIVED_AT: &str = "2026-11-06T15:04:06Z";
/// A second after `ADVERTISED_AT`, when the conformance crate's advertisements are made.
const ADVERTISED_RECEIVED_AT: &str = "2026-11-06T00:00:01Z";

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn operator_key() -> Ed25519Key {
    signing_key("the operator of rutent-test", 0)
}

fn deployment() -> Deployment {
    Deployment {
        name: "rutent-test".to_owned(),
        operator_key: public_key(&operator_key()),
    }
}

/// The log's clock: whatever a test last set.
#[derive(Debug, Clone)]
struct Stepped(Rc<Cell<i128>>);

impl Clock for Stepped {
    fn unix_nanos(&self) -> i128 {
        self.0.get()
    }
}

fn actor(party: &str, acts: &str, role: &str, kind: &str) -> Actor {
    serde_json::from_value(json!({"party": party, "acts": acts, "role": role, "kind": kind}))
        .unwrap()
}

/// A provider, which states for itself.
fn provider(name: &str) -> Actor {
    actor(name, name, "provider", "component")
}

/// The login of the deployment's operator.
fn operator_login() -> Actor {
    actor(OPERATOR, "op-login-2", "operator", "person")
}

// --- Statements ----------------------------------------------------------------------------------------------------

/// `maker`'s statement of `shape`, `body` its body, made at `made_at`, signed by `key`, as received.
fn statement(maker: &str, key: &Ed25519Key, made_at: &str, body: Value) -> Vec<u8> {
    let mut envelope = json!({
        "deployment": deployment(),
        "protocol_version": VERSION,
        "shape": TERMS_SHAPE,
        "maker": maker,
        "made_at": made_at,
        "signature": {"scheme": "ed25519", "key": "", "value": ""},
        "statement": body,
    });
    sign_value(&mut envelope, key);
    serde_json::to_vec(&envelope).unwrap()
}

/// `maker`'s quote under `transaction`, for the price `price`.
fn quote(maker: &str, transaction: &str, price: &str) -> Vec<u8> {
    statement(
        maker,
        &statements_key(maker),
        MADE_AT,
        json!({"kind": "quote", "transaction": transaction, "price": price}),
    )
}

/// `provider`'s advertisement, stating for itself, its ledger its own private node.
fn advertised(provider: &str) -> Vec<u8> {
    let context = SuiteContext {
        deployment: deployment(),
        ..SuiteContext::default()
    };
    let own = String::from_utf8(advertisement(provider, &context))
        .unwrap()
        .replace("stand-in-ledger", &format!("{provider}-ledger"));
    let mut value: Value = serde_json::from_str(&own).unwrap();
    value["statement"]["publisher"] = provider.into();
    sign_value(&mut value, &advertising_key(provider));
    serde_json::to_vec(&value).unwrap()
}

// --- Policy's read check, which the keeper holds ---------------------------------------------------------------------

/// Policy's read check: nothing here reads, so it answers none.
struct NoReads;

impl calls::Reads for NoReads {
    fn check_read(&mut self, _: &ReadCheck) -> Result<ReadAnswer, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }

    fn grant_permission(&mut self, _: &PermissionChange) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }

    fn revoke_permission(&mut self, _: &PermissionChange) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}

// --- The run -------------------------------------------------------------------------------------------------------

type Shared = Rc<RefCell<AuditLog<MemoryStore, LogsRead>>>;

struct Run {
    log: Shared,
    /// The way statements enter.
    statements: Statements<Keeper, General<AuditLog<MemoryStore, LogsRead>>>,
    evidence: EvidenceStore,
    /// Another handle on the same evidence store, to keep bytes the log has no entry for.
    keeper: Keeper,
    now: Rc<Cell<i128>>,
}

impl Run {
    fn new() -> Self {
        Run::over(MemoryStore::new(), EvidenceStore::in_memory())
    }

    fn over(store: MemoryStore, evidence: EvidenceStore) -> Self {
        let now = Rc::new(Cell::new(Timestamp::new(RECEIVED_AT).unwrap().unix_nanos()));
        let settings = Settings::new(id(LOG), deployment(), id(OPERATOR), id(OPERATOR))
            .on(Rc::new(Stepped(Rc::clone(&now))));
        let log = Rc::new(RefCell::new(
            AuditLog::open(settings, store, evidence.for_the_log()).unwrap(),
        ));
        Run {
            statements: Statements::new(evidence.keeper(Box::new(NoReads)), General::new(&log)),
            keeper: evidence.keeper(Box::new(NoReads)),
            log,
            evidence,
            now,
        }
    }

    /// The log's read of the evidence store.
    fn kept(&self) -> LogsRead {
        self.evidence.for_the_log()
    }

    fn set_now(&self, at: &str) {
        self.now.set(Timestamp::new(at).unwrap().unix_nanos());
    }

    /// Policy's permitting decision on `action`, its effect and its `done` result, by the operator's login, on the
    /// log's clock at `at`: the receipt of the decision.
    fn carry_out(&self, action: Value, at: &str) -> Receipt {
        self.set_now(at);
        let n = self.log.borrow().len();
        let (request, decision) = (format!("req-{n}"), format!("dec-{n}"));
        let operator = operator_login();
        let decided: calls::DecisionEntry = DecisionEntry::try_from(
            serde_json::from_value::<rutent_contracts::core_internal::VerifiedEntry>(json!({
                "kind": "verified",
                "who": operator,
                "what": {"call": "decide", "request": {"request": request, "caller": operator, "action": action, "at": at}, "decision": decision},
                "when": {"at": at, "source": "core_clock"},
                "outcome": {"status": "permitted"},
            }))
            .unwrap(),
        )
        .unwrap();
        let permit = self.log.borrow_mut().append_decision(&decided).unwrap();
        let core = actor("rutent", "core-orch", "core", "component");
        let effect = self.append_json(json!({
            "kind": "verified",
            "who": core,
            "what": {"call": "effect", "action": action, "decision": {"request": request, "decision": decision, "answer": "permit", "receipt": permit}},
            "when": {"at": at, "source": "core_clock"},
            "outcome": {"status": "to_take_effect"},
        }))
        .unwrap();
        self.append_json(json!({
            "kind": "verified",
            "who": core,
            "what": {"call": "result", "effect": effect},
            "when": {"at": at, "source": "core_clock"},
            "outcome": {"status": "done"},
        }))
        .unwrap();
        permit
    }

    fn append_json(&self, entry: Value) -> Result<Receipt, AppendFailure> {
        let entry: Entry = serde_json::from_value(entry).unwrap();
        self.log
            .borrow_mut()
            .append(&GeneralEntry::try_from(entry).unwrap())
    }

    /// Grants `name` a tuple and records its advertisement, so its statements keys sign.
    fn admit(&mut self, name: &str) {
        let action = json!({"kind": "change_catalogue", "change": "grant", "provider": name, "tuple": format!("tuple-{name}"), "advertising_keys": [public_key(&advertising_key(name)).as_str()]});
        self.carry_out(action, GRANTED_AT);
        let bytes = advertised(name);
        let envelope: Value = serde_json::from_slice(&bytes).unwrap();
        let made_at = envelope["made_at"].as_str().unwrap();
        let _ = made_at;
        // Received a second after it was made.
        self.set_now(ADVERTISED_RECEIVED_AT);
        let received_at = Timestamp::new(ADVERTISED_RECEIVED_AT).unwrap();
        self.statements
            .receive(&Presented {
                bytes,
                maker: provider(name),
                presented_by: provider(name),
                received_at,
                order: None,
                segment: None,
            })
            .unwrap();
        self.set_now(RECEIVED_AT);
    }

    /// `bytes` as `presented_by` hands them in, claimed to be `claimed`'s, received at `RECEIVED_AT`.
    fn present(
        &mut self,
        bytes: &[u8],
        claimed: &Actor,
        presented_by: &Actor,
    ) -> Result<Received, AppendFailure> {
        self.set_now(RECEIVED_AT);
        self.statements.receive(&Presented {
            bytes: bytes.to_vec(),
            maker: claimed.clone(),
            presented_by: presented_by.clone(),
            received_at: Timestamp::new(RECEIVED_AT).unwrap(),
            order: None,
            segment: None,
        })
    }

    /// The stated entry the log holds at `receipt`.
    fn stated_at(&self, receipt: &Receipt) -> StatedEntry {
        match &self.log.borrow().entries()[receipt.index.get() as usize].1 {
            Entry::Stated(s) => s.clone(),
            Entry::Verified(_) => panic!("a stated entry"),
        }
    }

    fn entries(&self) -> Vec<(Receipt, Entry)> {
        self.log.borrow().entries().to_vec()
    }

    fn restatements(&self) -> Vec<Restatement> {
        let mut kept = self.kept();
        restatements(&self.entries(), &mut kept).unwrap().found
    }
}

/// Whether `entry` is attributed `verified`.
fn verified(entry: &StatedEntry) -> bool {
    matches!(entry.attribution, Attribution::Verified { .. })
}

// --- A restated quote ------------------------------------------------------------------------------------------------

/// A relay passes on a provider's quote exactly as it received it. The evidence store finds it repeats the first
/// copy, and the entry stays the provider's, verified, with the relay recorded apart; a read of the log finds the
/// restatement afterwards, whether or not the entry names the relay. A copy recorded as another party's own word is
/// unverified, since the signature is the provider's, and is found with no relay named; the relay's own statement,
/// signed by its own key, repeats nothing and is the relay's. A copy with its signature garbled repeats the signed
/// content, so it is found too, though it verifies as no one's.
// verifies: REQ-AUD-2
#[test]
fn restated_quote_is_detected() {
    let mut run = Run::new();
    run.admit("us-bank");
    run.admit("relay-net");
    let (bank, relay) = (provider("us-bank"), provider("relay-net"));
    let quote = quote("us-bank", "tx-1", "100.00");

    // The provider's own copy, the first.
    let first = run.present(&quote, &bank, &bank).unwrap();
    assert_eq!(
        (first.restates.clone(), first.relayed_by.clone()),
        (None, None)
    );
    assert!(verified(&run.stated_at(&first.receipt)));
    assert!(run.restatements().is_empty());

    // The relay passes it on exactly: the same signed content, another salt, the relay named.
    let copy = run.present(&quote, &bank, &relay).unwrap();
    assert_ne!(copy.commitment, first.commitment);
    assert_eq!(copy.restates, Some(first.commitment.clone()));
    assert_eq!(copy.relayed_by, Some(relay.clone()));
    let entry = run.stated_at(&copy.receipt);
    assert!(verified(&entry), "the copy stays its maker's word");
    assert_eq!(entry.attribution.maker(), &bank);
    assert_eq!(entry.relayed_by, Some(relay.clone()));
    let found = run.restatements();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].first, first.receipt);
    assert_eq!(found[0].again, copy.receipt);
    assert_eq!(found[0].restated, bank);
    assert_eq!(found[0].relayed_by, Some(relay.clone()));
    assert!(!found[0].is_passed_off() && !found[0].is_resent_by_its_maker());

    // Recorded as the relay's own word: the signature is not the relay's, so the entry is unverified, and the read
    // finds it repeating the quote with no relay named: passed off.
    let own = run.present(&quote, &relay, &relay).unwrap();
    assert_eq!(own.restates, Some(first.commitment.clone()));
    assert_eq!(own.relayed_by, None);
    let entry = run.stated_at(&own.receipt);
    assert!(!verified(&entry));
    assert_eq!(entry.attribution.maker(), &relay);
    let found = run.restatements();
    assert_eq!(found.len(), 2);
    assert_eq!(found[1].again, own.receipt);
    assert!(found[1].is_passed_off());

    // The relay's own statement, signed with its own key: another statement, the relay's.
    let theirs = statement(
        "relay-net",
        &statements_key("relay-net"),
        MADE_AT,
        json!({"kind": "quote", "transaction": "tx-1", "price": "100.00"}),
    );
    let theirs = run.present(&theirs, &relay, &relay).unwrap();
    assert_eq!(theirs.restates, None);
    let entry = run.stated_at(&theirs.receipt);
    assert!(verified(&entry));
    assert_eq!(entry.attribution.maker(), &relay);
    assert_eq!(run.restatements().len(), 2);

    // The provider's quote under the provider's name but the relay's key: other signed content, signed by no one the
    // provider's keys name: unverified, and no restatement.
    let forged = statement(
        "us-bank",
        &statements_key("relay-net"),
        MADE_AT,
        json!({"kind": "quote", "transaction": "tx-1", "price": "100.00"}),
    );
    let forged = run.present(&forged, &bank, &relay).unwrap();
    assert_eq!(forged.restates, None);
    assert!(!verified(&run.stated_at(&forged.receipt)));
    assert_eq!(run.restatements().len(), 2);

    // The same copy with its signature's value garbled: the signed content is the same, so it is a restatement, and
    // it verifies as no one's.
    let mut garbled: Value = serde_json::from_slice(&quote).unwrap();
    garbled["signature"]["value"] = Value::String(
        garbled["signature"]["value"]
            .as_str()
            .unwrap()
            .chars()
            .rev()
            .collect(),
    );
    let garbled = serde_json::to_vec(&garbled).unwrap();
    let altered = run.present(&garbled, &bank, &relay).unwrap();
    assert_eq!(altered.restates, Some(first.commitment));
    assert!(!verified(&run.stated_at(&altered.receipt)));
    let found = run.restatements();
    assert_eq!(found.len(), 3);
    assert_eq!(found[2].again, altered.receipt);
}

// --- A signature kept with the bytes it signed ---------------------------------------------------------------------

/// What the evidence store keeps for a statement is the bytes exactly as received, the signature's value among them,
/// so the signature checks against exactly what was signed, from the kept statement alone (`EXP-1`'s `G-04`): the log's
/// read answers the same bytes, they bear out the entry's commitment, name the key the entry names, and the signature
/// verifies strictly; one byte changed fails it.
// verifies: REQ-AUD-2
#[test]
fn signature_is_kept_with_the_bytes_it_signed() {
    use rutent_contracts::core_internal::calls::Signatures;
    use rutent_core_audit::log::Ed25519Strict;

    let mut run = Run::new();
    run.admit("us-bank");
    let bank = provider("us-bank");
    let bytes = quote("us-bank", "tx-2", "99.50");
    let got = run.present(&bytes, &bank, &bank).unwrap();
    let entry = run.stated_at(&got.receipt);

    let mut kept = run.kept();
    let statement = kept.kept(&entry.commitment).unwrap().expect("kept");
    let held = bearing_out(&statement).expect("the bytes bear out the commitment");
    assert_eq!(held, bytes, "the bytes as received");
    let envelope: Value = serde_json::from_slice(&held).unwrap();
    let value = envelope["signature"]["value"].as_str().unwrap();
    assert!(!value.is_empty(), "the signature's value is kept");
    assert_eq!(
        envelope["signature"]["key"],
        serde_json::to_value(&entry.signed_with).unwrap()["key"]
    );
    assert!(Ed25519Strict.verifies(&held), "checks from what was kept");
    assert_eq!(
        sha256(&signing_input(&held).unwrap()),
        sha256(&signing_input(&bytes).unwrap())
    );
    let mut changed = held.clone();
    let place = changed.windows(6).position(|w| w == b"99.50\"").unwrap();
    changed[place] = b'8';
    assert!(!Ed25519Strict.verifies(&changed), "one byte changed");

    // Bytes that aren't an envelope are refused before anything is kept.
    let before = run.entries().len();
    for bytes in [&b"{}"[..], b"not json", b"[1]"] {
        assert!(matches!(
            run.present(bytes, &bank, &bank),
            Err(AppendFailure::Refused(v)) if v.0 == NOT_A_STATEMENT
        ));
    }
    assert_eq!(run.entries().len(), before);
}

// --- A signature that doesn't verify is never attributed ---------------------------------------------------------------

/// A statement whose signature fails, whatever the reason, is logged unverified: signed by another key under the
/// provider's key's name, altered after signing, its signature garbled, or naming a maker the claim doesn't. Only the
/// genuine statement is attributed (`EXP-1`'s `G-09`). The log checks it on append; a stated entry marked `verified`
/// that a log's store holds with nothing kept to bear it out is found by [`unproven`], which the log's opening doesn't
/// do, and what is found is the checker's to raise.
// verifies: REQ-AUD-2
#[test]
fn statement_whose_signature_doesnt_verify_is_not_logged_as_attributed() {
    let mut run = Run::new();
    run.admit("us-bank");
    run.admit("relay-net");
    let (bank, relay) = (provider("us-bank"), provider("relay-net"));
    let body = json!({"kind": "quote", "transaction": "tx-3", "price": "10.00"});

    // Signed by another key, naming the provider's own statements key.
    let mut envelope: Value =
        serde_json::from_slice(&statement("us-bank", &another_key(), MADE_AT, body.clone()))
            .unwrap();
    envelope["signature"]["key"] = public_key(&statements_key("us-bank")).as_str().into();
    let by_another = serde_json::to_vec(&envelope).unwrap();
    // The genuine one, altered after it was signed.
    let genuine = statement("us-bank", &statements_key("us-bank"), MADE_AT, body.clone());
    let altered = String::from_utf8(genuine.clone())
        .unwrap()
        .replace("10.00", "1.00")
        .into_bytes();
    // Its signature garbled.
    let mut garbled: Value = serde_json::from_slice(&genuine).unwrap();
    garbled["signature"]["value"] = "AAAA".into();
    let garbled = serde_json::to_vec(&garbled).unwrap();
    for (name, bytes) in [
        ("signed by another key", by_another),
        ("altered after signing", altered),
        ("signature garbled", garbled),
    ] {
        let got = run.present(&bytes, &bank, &bank).unwrap();
        assert!(!verified(&run.stated_at(&got.receipt)), "{name}");
    }
    // A genuine statement the entry dates other than its envelope does: the entry's `when.at` is the claim, appended
    // as a writer that dated it wrongly would, so the log doesn't attribute it.
    let kept = run.keeper.keep(&genuine).unwrap();
    let misdated = run
        .append_json(json!({
            "kind": "stated",
            "attribution": {"signature": "unverified", "claimed_by": bank},
            "what": {"statement": TERMS_SHAPE},
            "when": {"at": "2026-11-06T15:04:06Z", "source": "party_stated"},
            "received_at": RECEIVED_AT,
            "outcome": {"status": "stated"},
            "signed_with": {"scheme": "ed25519", "key": public_key(&statements_key("us-bank")).as_str()},
            "commitment": kept.commitment,
            "about": {"transaction": "tx-3"},
        }))
        .unwrap();
    assert!(!verified(&run.stated_at(&misdated)), "made at another time");
    // A genuine statement the claim attributes to another maker.
    let got = run.present(&genuine, &relay, &relay).unwrap();
    assert!(
        !verified(&run.stated_at(&got.receipt)),
        "claimed by another"
    );
    // The genuine statement, claimed by its maker: attributed.
    let got = run.present(&genuine, &bank, &bank).unwrap();
    assert!(verified(&run.stated_at(&got.receipt)));

    // `unproven` holds nothing against a log whose attributions all stand.
    let mut kept = run.kept();
    let clean = unproven(&run.entries(), &mut kept, &deployment()).unwrap();
    assert!(clean.found.is_empty() && clean.unread.is_empty());

    // The reopen path: the log opens on a store holding entries marked `verified` that no kept statement bears out,
    // as the operator with access to the file could write them, so the log's opening doesn't make them true; the
    // read finds each.
    let signed_by_another = run
        .keeper
        .keep(&{
            let mut envelope: Value = serde_json::from_slice(&statement(
                "us-bank",
                &another_key(),
                MADE_AT,
                json!({"kind": "quote", "transaction": "tx-9", "price": "1.00"}),
            ))
            .unwrap();
            envelope["signature"]["key"] = public_key(&statements_key("us-bank")).as_str().into();
            serde_json::to_vec(&envelope).unwrap()
        })
        .unwrap();
    let genuine_kept = run.keeper.keep(&genuine).unwrap();
    let forged_entry = |who: &Actor,
                        commitment: &rutent_contracts::module_interface::Digest,
                        key: &Ed25519Key|
     -> Entry {
        serde_json::from_value(json!({
            "kind": "stated",
            "attribution": {"signature": "verified", "who": who},
            "what": {"statement": TERMS_SHAPE},
            "when": {"at": MADE_AT, "source": "party_stated"},
            "received_at": RECEIVED_AT,
            "outcome": {"status": "stated"},
            "signed_with": {"scheme": "ed25519", "key": public_key(key).as_str()},
            "commitment": commitment,
            "about": {"transaction": "tx-3"},
        }))
        .unwrap()
    };
    let nothing_kept = digest(&[7; 32]);
    let forged = [
        // Signature fails: bytes signed by another key.
        forged_entry(
            &bank,
            &signed_by_another.commitment,
            &statements_key("us-bank"),
        ),
        // The genuine bytes, attributed to a maker they don't name.
        forged_entry(&relay, &genuine_kept.commitment, &statements_key("us-bank")),
        // The genuine bytes, with the key of another entry.
        forged_entry(&bank, &genuine_kept.commitment, &another_key()),
    ];
    let mut store = MemoryStore::new();
    for (_, entry) in run.entries() {
        store.append(&record(&entry).unwrap()).unwrap();
    }
    for entry in &forged {
        store.append(&record(entry).unwrap()).unwrap();
    }
    let unkept = forged_entry(&bank, &nothing_kept, &statements_key("us-bank"));
    store.append(&record(&unkept).unwrap()).unwrap();
    let settings = Settings::new(id(LOG), deployment(), id(OPERATOR), id(OPERATOR));
    let reopened = AuditLog::open(settings, store, run.kept())
        .expect("the log opens: it re-attributes nothing on opening (`T-022-01`)");
    let before = run.entries().len();
    let entries = reopened.entries();
    assert_eq!(entries.len(), before + 4);
    let mut kept = run.kept();
    let found = unproven(entries, &mut kept, &deployment()).unwrap();
    let places: Vec<u64> = found.found.iter().map(|(r, _)| r.index.get()).collect();
    assert_eq!(
        places,
        vec![before as u64, before as u64 + 1, before as u64 + 2]
    );
    assert!(found.found.iter().all(|(_, v)| v.0 == NOT_BORNE_OUT));
    assert_eq!(
        found
            .unread
            .iter()
            .map(|r| r.index.get())
            .collect::<Vec<_>>(),
        vec![before as u64 + 3],
        "an attribution with nothing kept is reported apart: erased, or lost"
    );
}

// --- An operator command names its operator ---------------------------------------------------------------------------

/// An operator's command is an entry whose actor is the deployment's operator, by the login that made it: the log
/// refuses one by anyone else, a provider, another party's operator, or a login in another role, so a command can't
/// be logged with no operator or the wrong one. The decided actions an operator takes (granting a role or installing
/// a module, `EXP-1`'s probes) are recorded in a `decide` entry whose `who` and whose request's `caller` name the
/// operator's login. "Names its operator" reads the actor's reference (`acts`), never a signing key.
// verifies: REQ-AUD-1
#[test]
fn operator_commands_entry_names_its_operator() {
    let run = Run::new();
    let command = |who: &Actor| {
        json!({
            "kind": "verified",
            "who": who,
            "what": {"call": "command", "command": "appoint_checker", "subject": "checker-org-1"},
            "when": {"at": "2026-11-10T09:00:00Z", "source": "core_clock"},
            "outcome": {"status": "done"},
        })
    };
    run.set_now("2026-11-10T09:00:00Z");
    for impostor in [
        provider("bridge-pay"),
        actor("bridge-pay", "bridge-pay-ops", "operator", "person"),
        actor(OPERATOR, "op-login-2", "provider", "person"),
        actor(OPERATOR, "op-login-2", "core", "component"),
    ] {
        assert!(
            matches!(
                run.append_json(command(&impostor)),
                Err(AppendFailure::Refused(_))
            ),
            "{impostor:?}"
        );
    }
    assert_eq!(run.entries().len(), 0, "nothing recorded");
    let receipt = run.append_json(command(&operator_login())).unwrap();
    let entries = run.entries();
    let Entry::Verified(entry) = &entries[receipt.index.get() as usize].1 else {
        panic!("a verified entry");
    };
    assert_eq!(entry.who, operator_login());
    assert_eq!(entry.who.acts.as_str(), "op-login-2");

    // The decided actions an operator takes: granting a provider, which `EXP-1`'s `G-05` names, is recorded with the
    // operator's login as the entry's actor and as the request's caller.
    let permit = run.carry_out(
        json!({"kind": "change_catalogue", "change": "grant", "provider": "us-bank", "tuple": "tuple-us-bank", "advertising_keys": [public_key(&advertising_key("us-bank")).as_str()]}),
        GRANTED_AT,
    );
    let entries = run.entries();
    let Entry::Verified(decided) = &entries[permit.index.get() as usize].1 else {
        panic!("a verified entry");
    };
    assert_eq!(decided.who, operator_login());
    let rutent_contracts::core_internal::What::Decide { request, .. } = &decided.what else {
        panic!("a decision");
    };
    assert_eq!(request.caller, operator_login());
}

// --- Each role recorded apart ---------------------------------------------------------------------------------------

/// One party in three roles: the deployment's operator as the operator (a command, its own question), and as a
/// provider through a module it publishes, whose statements verify to the operator's party under the release's key
/// (`THR-68`'s case). Each entry keeps its own role, the roles never merge, and the operator-command relation refuses
/// the module's actor, which is the same party in the provider's role.
// verifies: REQ-AUD-8
#[test]
fn one_party_with_several_roles_is_recorded_per_role() {
    const MODULE: &str = "public-node-holding";
    let release = signing_key("the public node's holding module, 1.0.0", 14);
    let mut run = Run::new();
    run.carry_out(
        json!({"kind": "change_catalogue", "change": "accept_module", "provider": MODULE, "version": "1.0.0", "statement_key": public_key(&release).as_str()}),
        "2026-11-06T00:00:00Z",
    );
    let as_operator = operator_login();
    let as_provider = actor(OPERATOR, MODULE, "provider", "component");
    let as_signer = actor(OPERATOR, OPERATOR, "operator", "component");

    // As a provider: the module's statement, verified to the operator's party in the provider's role.
    let module = statement(
        MODULE,
        &release,
        MADE_AT,
        json!({"kind": "holding_balance", "transaction": "tx-8"}),
    );
    let stated = run.present(&module, &as_provider, &as_provider).unwrap();
    let entry = run.stated_at(&stated.receipt);
    assert!(verified(&entry));
    assert_eq!(entry.attribution.maker(), &as_provider);

    // As the operator: its own signed statement, and a command by its login.
    let own = statement(
        OPERATOR,
        &operator_key(),
        MADE_AT,
        json!({"kind": "balance_verification", "transaction": "tx-7"}),
    );
    let signed = run.present(&own, &as_signer, &as_signer).unwrap();
    assert!(verified(&run.stated_at(&signed.receipt)));
    run.set_now("2026-11-10T09:00:00Z");
    let command = |who: &Actor| {
        json!({
            "kind": "verified",
            "who": who,
            "what": {"call": "command", "command": "appoint_checker", "subject": "checker-org-1"},
            "when": {"at": "2026-11-10T09:00:00Z", "source": "core_clock"},
            "outcome": {"status": "done"},
        })
    };
    // The same party in the provider's role can't command.
    assert!(matches!(
        run.append_json(command(&as_provider)),
        Err(AppendFailure::Refused(_))
    ));
    let commanded = run.append_json(command(&as_operator)).unwrap();

    // Each entry holds one role, and the party's roles are three records apart.
    let entries = run.entries();
    let roles: Vec<(String, String)> = entries
        .iter()
        .filter_map(|(r, e)| {
            let who = match e {
                Entry::Stated(s) => s.attribution.maker().clone(),
                Entry::Verified(v) if r.index.get() >= commanded.index.get() => v.who.clone(),
                Entry::Verified(_) => return None,
            };
            (who.party.as_str() == OPERATOR).then(|| {
                (
                    serde_json::to_value(who.role)
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_owned(),
                    who.acts.as_str().to_owned(),
                )
            })
        })
        .collect();
    assert!(roles.contains(&("provider".into(), MODULE.into())));
    assert!(roles.contains(&("operator".into(), OPERATOR.into())));
    assert!(roles.contains(&("operator".into(), "op-login-2".into())));
    assert!(
        !roles.contains(&("operator".into(), MODULE.into())),
        "the module never records as the operator"
    );
    assert!(
        !roles.contains(&("provider".into(), OPERATOR.into())),
        "the operator never records as a provider"
    );
}

/// `unproven` holds a `verified` entry to what the append does: the envelope names this deployment, and the entry
/// is about the transaction the body names. Bytes of another deployment, or of another transaction than the entry
/// says, signed with the provider's own key, are found; a genuine entry beside them is not.
// verifies: REQ-AUD-2
#[test]
fn unproven_holds_the_deployment_and_the_transaction_the_append_does() {
    let mut run = Run::new();
    run.admit("us-bank");
    let bank = provider("us-bank");
    let genuine = quote("us-bank", "tx-3", "10.00");
    let ok = run.present(&genuine, &bank, &bank).unwrap();
    assert!(verified(&run.stated_at(&ok.receipt)));

    let key = statements_key("us-bank");
    let signed = |deployment: Value, transaction: Option<&str>| -> Vec<u8> {
        let mut statement = json!({"kind": "quote", "price": "10.00"});
        if let Some(t) = transaction {
            statement["transaction"] = t.into();
        }
        let mut envelope = json!({
            "deployment": deployment,
            "protocol_version": VERSION,
            "shape": TERMS_SHAPE,
            "maker": "us-bank",
            "made_at": MADE_AT,
            "signature": {"scheme": "ed25519", "key": "", "value": ""},
            "statement": statement,
        });
        sign_value(&mut envelope, &key);
        serde_json::to_vec(&envelope).unwrap()
    };
    let other_deployment =
        json!({"name": "another-deployment", "operator_key": public_key(&operator_key()).as_str()});
    let cases = [
        ("another deployment", signed(other_deployment, Some("tx-3"))),
        (
            "another transaction",
            signed(json!(deployment()), Some("tx-other")),
        ),
        ("a body naming none", signed(json!(deployment()), None)),
    ];
    let marked = |commitment: &rutent_contracts::module_interface::Digest| -> Entry {
        serde_json::from_value(json!({
            "kind": "stated",
            "attribution": {"signature": "verified", "who": bank},
            "what": {"statement": TERMS_SHAPE},
            "when": {"at": MADE_AT, "source": "party_stated"},
            "received_at": RECEIVED_AT,
            "outcome": {"status": "stated"},
            "signed_with": {"scheme": "ed25519", "key": public_key(&key).as_str()},
            "commitment": commitment,
            "about": {"transaction": "tx-3"},
        }))
        .unwrap()
    };
    let mut store = MemoryStore::new();
    for (_, entry) in run.entries() {
        store.append(&record(&entry).unwrap()).unwrap();
    }
    for (_, bytes) in &cases {
        let kept = run.keeper.keep(bytes).unwrap();
        store
            .append(&record(&marked(&kept.commitment)).unwrap())
            .unwrap();
    }
    let settings = Settings::new(id(LOG), deployment(), id(OPERATOR), id(OPERATOR));
    let reopened = AuditLog::open(settings, store, run.kept()).unwrap();
    let before = run.entries().len() as u64;
    let mut kept = run.kept();
    let found = unproven(reopened.entries(), &mut kept, &deployment()).unwrap();
    let places: Vec<u64> = found.found.iter().map(|(r, _)| r.index.get()).collect();
    assert_eq!(places, vec![before, before + 1, before + 2], "{found:?}");
    assert!(found.unread.is_empty());
}

//! `T-021-02`'s tests: sessions only for an account's holder or its delegate and bounded in time, and reads only of
//! what their reader may read, each recorded, against a log that holds every entry to core-internal's admission.

use super::*;
use rutent_contracts::core_internal::calls::{DecisionEntry, PolicyLog};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Entry, NotKnownCause, ReadOutcome, Receipt, Through, VerifiedEntry,
    admit_entry,
};
use rutent_contracts::provider_role::Operation;
use rutent_prov::catalogue::{CheckBound, CheckRefusal};
use serde_json::{Value, json};

fn read_fixture(path: &str) -> Value {
    let path = format!(
        "{}/../../contracts/core-internal/fixtures/{path}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A valid fixture of the contract, read as whatever type the call site wants.
macro_rules! valid {
    ($name:expr) => {
        serde_json::from_value(read_fixture(&format!("valid/{}", $name))).unwrap()
    };
}

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn at(text: &str) -> Timestamp {
    Timestamp::new(text).unwrap()
}

fn actor(party: &str, acts: &str, role: Role) -> Actor {
    Actor {
        party: id(party),
        acts: id(acts),
        role,
        kind: ActorKind::Person,
    }
}

/// A log that holds every entry to the contract's admission, as `CMP-CORE-AUDIT`'s log must, reached only through
/// policy's handle.
#[derive(Default)]
struct Kept(Vec<(Receipt, Entry)>);

impl PolicyLog for Kept {
    fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
        let entry = Entry::Verified(entry.entry().clone());
        entry.check().map_err(AppendFailure::Refused)?;
        if let Some((receipt, _)) = self.0.iter().find(|(_, e)| e == &entry) {
            return Ok(receipt.clone());
        }
        admit_entry(&self.0, &entry, Through::Policy).map_err(AppendFailure::Refused)?;
        let index = 1_000 + self.0.len();
        let receipt: Receipt = serde_json::from_value(json!({
            "log": "log-main",
            "index": index,
            "entry": {"algorithm": "sha-256", "value": format!("{index:064x}")},
        }))
        .unwrap();
        self.0.push((receipt.clone(), entry));
        Ok(receipt)
    }
}

impl Kept {
    /// A log holding one coherent world before any read permission was granted: order `ord-1`, accepted by
    /// `cust-1` as its requester, beneficiary and payer, and value moved on its segments `p1-s1` through
    /// `bridge-pay` and `p1-s2` through `fx-desk`.
    fn world() -> Self {
        let world = read_fixture("admission/world--grant");
        let history = world["history"].as_array().unwrap();
        Kept(
            history
                .iter()
                .map(|h| {
                    (
                        serde_json::from_value(h["receipt"].clone()).unwrap(),
                        serde_json::from_value(h["entry"].clone()).unwrap(),
                    )
                })
                .collect(),
        )
    }

    /// The last entry, which must be the one whose receipt `receipt` is.
    fn last(&self, receipt: &Receipt) -> &VerifiedEntry {
        let (r, Entry::Verified(entry)) = self.0.last().unwrap() else {
            panic!("a verified entry");
        };
        assert_eq!(r, receipt, "the answer carries its entry's receipt");
        entry
    }
}

/// The deployment's policy: it needs nothing and always rules the same, and counts how often it ruled.
struct Rules {
    ruling: Ruling,
    ruled: usize,
}

impl Rules {
    fn permitting() -> Self {
        Rules {
            ruling: Ruling::Permit,
            ruled: 0,
        }
    }

    fn refusing() -> Self {
        Rules {
            ruling: Ruling::Refuse(RefusalClass::NotPermitted),
            ruled: 0,
        }
    }
}

impl Policy for Rules {
    fn needs(&mut self, _: Asked<'_>) -> Reply<Vec<Need>> {
        Reply::Given(Vec::new())
    }

    fn rule(&mut self, _: Asked<'_>, _: &[Input]) -> Reply<Ruling> {
        self.ruled += 1;
        Reply::Given(self.ruling)
    }
}

/// Holders and records whose answers the test sets, which keep what they were asked.
#[derive(Default)]
struct Says {
    says: Vec<(Need, Reply<Said>)>,
    asked: Vec<Need>,
}

impl Says {
    fn of(says: Vec<(Need, Reply<Said>)>) -> Self {
        Says {
            says,
            asked: Vec::new(),
        }
    }
}

impl Sources for Says {
    fn supply(&mut self, need: &Need) -> Reply<Said> {
        self.asked.push(need.clone());
        self.says
            .iter()
            .find(|(n, _)| n == need)
            .map_or(Reply::Missing, |(_, said)| said.clone())
    }
}

/// The record of `account`, asked whether `party` holds it.
fn holds(party: &str, account: &str) -> Need {
    Need {
        input: id(party),
        from: Source::Account(id(account)),
    }
}

/// The holder of `account`, asked whether `party` is its delegate on it.
fn delegation(party: &str, account: &str, holder: &str) -> Need {
    Need {
        input: id(party),
        from: Source::Holder {
            account: id(account),
            holder: id(holder),
        },
    }
}

/// The record of `order`, asked whether `party` is its party.
fn party_of(party: &str, order: &str) -> Need {
    Need {
        input: id(party),
        from: Source::OrderParties(id(order)),
    }
}

/// The commitment a kept statement is named by: `byte`, two hex digits, 32 times.
fn commitment(byte: &str) -> Value {
    json!({"algorithm": "sha-256", "value": byte.repeat(32)})
}

/// The evidence store's record of the statement under `commitment(byte)`, asked whether it is `party`'s own.
fn own_statement(party: &str, byte: &str) -> Need {
    Need {
        input: id(party),
        from: Source::Statement(serde_json::from_value(commitment(byte)).unwrap()),
    }
}

/// The record of `segment`, asked whether it belongs to `order`.
fn segment_of(order: &str, segment: &str) -> Need {
    Need {
        input: id(order),
        from: Source::SegmentOrder(id(segment)),
    }
}

/// The record of a segment, asked whether `party` is its provider.
fn provider_of(party: &str, order: &str, segment: &str) -> Need {
    Need {
        input: id(party),
        from: Source::SegmentProvider {
            order: id(order),
            segment: id(segment),
        },
    }
}

/// Core's clock, stopped at the fixtures' moment, 09:00 on 2026-11-10.
fn nine() -> Box<dyn Clock> {
    Stopped::at(&at("2026-11-10T09:00:00Z"))
}

/// The holder `cust-1`'s own request for a session on `acct-ref-4`, from 09:00 to 09:15.
fn own_session(request: &str) -> SessionRequest {
    let mut own: SessionRequest = valid!("session-request-of-a-delegate");
    own.request = id(request);
    own.requester = actor("cust-1", "cust-1", Role::Customer);
    own
}

/// A read by `reader` of `reads`, naming `permission` where one is given.
fn read(request: &str, reader: Actor, reads: Value, permission: Option<&str>) -> ReadCheck {
    let mut check = json!({
        "request": request,
        "reader": reader,
        "reads": reads,
        "at": "2026-11-10T09:00:00Z",
    });
    if let Some(p) = permission {
        check["permission"] = json!(p);
    }
    serde_json::from_value(check).unwrap()
}

fn refused() -> ReadOutcome {
    ReadOutcome::Refused {
        class: RefusalClass::NotPermitted,
    }
}

/// Reads `check` and returns its answer, having checked that it was recorded as answered, as the log's last entry.
fn read_recorded<P: Policy, S: Sources>(
    engine: &mut Engine<Kept, P, S>,
    check: &ReadCheck,
) -> ReadOutcome {
    let before = engine.log.0.len();
    let answer = engine.check_read(check).unwrap();
    assert_eq!(engine.log.0.len(), before + 1, "{check:?} is recorded");
    let entry = engine.log.last(&answer.receipt);
    assert_eq!(
        entry.what,
        What::CheckRead {
            check: check.clone()
        }
    );
    assert_eq!(entry.outcome, answer.outcome.outcome());
    answer.outcome
}

/// A session goes only to the account's holder, or to a party the holder names as its delegate on that account: a
/// stranger is refused even where the deployment's policy would open anything, and the deployment's policy still
/// decides for the holder. The holder's silence is a refusal, its unreachability not known. Each answer is recorded,
/// an open session naming itself and ending when it was asked to, and only an open one reaches the register. A
/// session asked to last longer than the longest is refused, the holder's own included.
// verifies: REQ-CAT-3
#[test]
fn session_is_issued_only_to_the_accounts_holder_or_delegate() {
    // The account's record says `cust-1` holds `acct-ref-4`, in every case.
    let holder = holds("cust-1", "acct-ref-4");
    let open = |mut says: Vec<(Need, Reply<Said>)>, policy: Rules, request: &SessionRequest| {
        says.push((holds("cust-1", "acct-ref-4"), Reply::Given(Said::Yes)));
        let mut engine = Engine::with_policy(Kept::default(), policy, Says::of(says));
        engine.set_clock(nine());
        let answer = engine.open_session(request).unwrap();
        let entry = engine.log.last(&answer.receipt).clone();
        assert_eq!(entry.outcome, answer.outcome.outcome());
        (answer.outcome, entry, engine)
    };
    let delegate: SessionRequest = valid!("session-request-of-a-delegate");
    let names = delegation("acme-treasury", "acct-ref-4", "cust-1");
    let refused = SessionOutcome::Refused {
        class: RefusalClass::NotPermitted,
    };

    // The holder's own: opened, its id its request's, ending when asked; only the account's record asked.
    let own = own_session("req-301");
    let (outcome, entry, engine) = open(Vec::new(), Rules::permitting(), &own);
    let opened = Opened {
        session: id("req-301"),
        expires_at: at("2026-11-10T09:15:00Z"),
    };
    assert_eq!(
        outcome,
        SessionOutcome::Opened {
            session: opened.session.clone(),
            expires_at: opened.expires_at.clone()
        }
    );
    assert_eq!(
        entry.what,
        What::OpenSession {
            request: own.clone(),
            opened: Some(opened.clone())
        }
    );
    assert_eq!(engine.sources.asked, std::slice::from_ref(&holder));
    assert_eq!(
        engine.register().sessions(),
        [OpenSession {
            session: opened.session,
            account: id("acct-ref-4"),
            holder: id("cust-1"),
            expires_at: opened.expires_at,
        }]
    );

    // An id already issued opens no other session, on another account or for another holder, and the deployment
    // isn't asked; the same request again opens the same session, once.
    let mut engine = engine;
    let mut elsewhere = own.clone();
    elsewhere.account = id("acct-ref-5");
    let answer = engine.open_session(&elsewhere).unwrap();
    assert_eq!(answer.outcome, refused);
    assert_eq!(
        engine.log.last(&answer.receipt).what,
        What::OpenSession {
            request: elsewhere,
            opened: None
        }
    );
    let mut another_holder = own.clone();
    another_holder.holder = id("cust-2");
    another_holder.requester = actor("cust-2", "cust-2", Role::Customer);
    let answer = engine.open_session(&another_holder).unwrap();
    assert_eq!(answer.outcome, refused);
    assert_eq!(engine.policy.1.ruled, 1, "only the first was ruled on");
    let again = engine.open_session(&own).unwrap();
    assert!(matches!(again.outcome, SessionOutcome::Opened { .. }));
    assert_eq!(engine.register().sessions().len(), 1);

    // A delegate the holder names: opened, on the holder's word.
    let (outcome, _, engine) = open(
        vec![(names.clone(), Reply::Given(Said::Yes))],
        Rules::permitting(),
        &delegate,
    );
    assert!(matches!(outcome, SessionOutcome::Opened { .. }));
    assert_eq!(engine.sources.asked, [holder.clone(), names.clone()]);
    assert_eq!(engine.register().sessions().len(), 1);

    // A stranger the holder doesn't name: refused, recorded without a session, and the deployment not asked.
    let (outcome, entry, engine) = open(
        vec![(names.clone(), Reply::Given(Said::No))],
        Rules::permitting(),
        &delegate,
    );
    assert_eq!(outcome, refused);
    assert_eq!(
        entry.what,
        What::OpenSession {
            request: delegate.clone(),
            opened: None
        }
    );
    assert!(engine.register().sessions().is_empty());
    assert_eq!(engine.policy.1.ruled, 0);
    let mut stranger = delegate.clone();
    stranger.requester = actor("someone-else", "someone-else", Role::Customer);
    let (outcome, _, _) = open(
        vec![(names.clone(), Reply::Given(Said::Yes))],
        Rules::permitting(),
        &stranger,
    );
    assert_eq!(
        outcome,
        SessionOutcome::Refused {
            class: RefusalClass::MissingInput
        },
        "the holder named another party, and was asked about this one"
    );

    // The holder silent: a refusal; out of reach: not known. No session either way.
    let (outcome, _, engine) = open(
        vec![(names.clone(), Reply::Missing)],
        Rules::permitting(),
        &delegate,
    );
    assert_eq!(
        outcome,
        SessionOutcome::Refused {
            class: RefusalClass::MissingInput
        }
    );
    assert!(engine.register().sessions().is_empty());
    let (outcome, _, engine) = open(
        vec![(names.clone(), Reply::Unreachable)],
        Rules::permitting(),
        &delegate,
    );
    assert_eq!(
        outcome,
        SessionOutcome::NotKnown {
            cause: NotKnownCause::VerifierUnreachable
        }
    );
    assert!(engine.register().sessions().is_empty());

    // The deployment's policy still decides for the holder.
    let (outcome, _, engine) = open(Vec::new(), Rules::refusing(), &own);
    assert_eq!(outcome, refused);
    assert!(engine.register().sessions().is_empty());

    // Bounded in time, from Core's now: the longest is allowed, a second past it is refused, whoever asks.
    let mut longest = own_session("req-302");
    longest.until = at("2026-11-10T09:15:00Z");
    assert_eq!(
        LONGEST_SESSION_SECS,
        u64::try_from((longest.until.unix_nanos() - longest.at.unix_nanos()) / 1_000_000_000)
            .unwrap()
    );
    let (outcome, _, _) = open(Vec::new(), Rules::permitting(), &longest);
    assert!(matches!(outcome, SessionOutcome::Opened { .. }));
    let mut too_long = own_session("req-303");
    too_long.until = at("2026-11-10T09:15:01Z");
    let (outcome, _, engine) = open(Vec::new(), Rules::permitting(), &too_long);
    assert_eq!(outcome, refused);
    assert_eq!(engine.policy.1.ruled, 0, "the deployment isn't asked");
    let mut delegate_too_long = delegate.clone();
    delegate_too_long.until = at("2026-11-10T09:15:01Z");
    let (outcome, _, engine) = open(
        vec![(names, Reply::Given(Said::Yes))],
        Rules::permitting(),
        &delegate_too_long,
    );
    assert_eq!(outcome, refused);
    assert!(engine.sources.asked.is_empty(), "the holder isn't asked");
}

/// A balance verification on `account` in `session`, of the reserve.
fn balance_check(session: &str, account: &str) -> Operation {
    serde_json::from_value(json!({
        "kind": "balance_verification", "transaction": "tx-1", "provider": "polygon-wallet-reserve",
        "session": session, "account": account, "asset": "USDC", "amount": "50",
    }))
    .unwrap()
}

/// A session the engine opens reaches the catalogue's register, which answers no balance check in it once it has
/// ended: until its end the check gets past the session to the reserve's own standing, from its end on it is refused
/// for want of a session. A session the engine refused never reaches the catalogue.
// verifies: REQ-CAT-3
#[test]
fn expired_session_answers_no_balance_check() {
    let mut engine = Engine::with_policy(
        Kept::default(),
        Rules::permitting(),
        Says::of(vec![
            (holds("cust-1", "acct-ref-4"), Reply::Given(Said::Yes)),
            (
                delegation("acme-treasury", "acct-ref-4", "cust-1"),
                Reply::Given(Said::No),
            ),
        ]),
    );
    engine.set_clock(nine());
    let answer = engine.open_session(&own_session("req-310")).unwrap();
    assert!(matches!(answer.outcome, SessionOutcome::Opened { .. }));
    let refused: SessionRequest = valid!("session-request-of-a-delegate");
    let answer = engine.open_session(&refused).unwrap();
    assert!(matches!(answer.outcome, SessionOutcome::Refused { .. }));

    let mut catalogue = Catalogue::new();
    catalogue.set_check_bound(CheckBound {
        max_counted: 3,
        window_secs: 60,
    });
    catalogue.set_answer_within(10);
    engine.register().open_in(&mut catalogue);

    let check = |catalogue: &mut Catalogue, session: &str, when: &str| {
        catalogue.begin_check(
            &balance_check(session, "acct-ref-4"),
            None,
            instant(&at(when)),
        )
    };
    // Inside the session, the check is held only to the reserve's standing, which this catalogue grants nothing.
    for when in ["2026-11-10T09:00:00Z", "2026-11-10T09:14:59Z"] {
        assert!(
            matches!(
                check(&mut catalogue, "req-310", when),
                Err(CheckRefusal::Unusable(_))
            ),
            "{when}"
        );
    }
    for when in [
        "2026-11-10T09:15:00Z",
        "2026-11-10T09:15:01Z",
        "2026-11-11T09:00:00Z",
    ] {
        assert_eq!(
            check(&mut catalogue, "req-310", when).unwrap_err(),
            CheckRefusal::NoSession,
            "{when}"
        );
    }
    assert_eq!(
        check(&mut catalogue, "req-300", "2026-11-10T09:00:00Z").unwrap_err(),
        CheckRefusal::NoSession,
        "a refused session is no session"
    );

    // The catalogue never holds a session past its end: a fraction of a second is taken off, never added.
    assert_eq!(
        instant(&at("2026-11-10T09:15:00.999Z")),
        instant(&at("2026-11-10T09:15:00Z"))
    );
}

/// A customer reads only its own orders: another customer is refused its order and the order's segments, recorded,
/// even where the deployment's policy would allow anything, and no customer reads the trail. The order's own
/// customer is allowed, and the log admits its read. An order's record out of reach refuses the read. Were the
/// record to say yes wrongly, the log would still refuse the read as allowed, and it is recorded refused.
// verifies: REQ-AUZ-7
#[test]
fn customer_cannot_read_another_customers_order() {
    let says = || {
        Says::of(vec![
            (party_of("cust-1", "ord-1"), Reply::Given(Said::Yes)),
            (party_of("cust-2", "ord-1"), Reply::Given(Said::No)),
            (segment_of("ord-1", "p1-s1"), Reply::Given(Said::Yes)),
        ])
    };
    let mut engine = Engine::with_policy(Kept::world(), Rules::permitting(), says());
    let cust = |party: &str| actor(party, party, Role::Customer);
    let order = json!({"kind": "order", "order": "ord-1"});
    let segment =
        json!({"kind": "segment", "order": "ord-1", "segment": "p1-s1", "provider": "bridge-pay"});

    let other = read("req-410", cust("cust-2"), order.clone(), None);
    assert_eq!(read_recorded(&mut engine, &other), refused());
    let other_segment = read("req-411", cust("cust-2"), segment.clone(), None);
    assert_eq!(read_recorded(&mut engine, &other_segment), refused());
    let trail = read("req-412", cust("cust-1"), json!({"kind": "trail"}), None);
    let asked = engine.sources.asked.len();
    assert_eq!(read_recorded(&mut engine, &trail), refused());
    assert_eq!(
        engine.sources.asked.len(),
        asked,
        "nothing asked to refuse the trail"
    );

    let own = read("req-413", cust("cust-1"), order.clone(), None);
    assert_eq!(read_recorded(&mut engine, &own), ReadOutcome::Allowed {});
    let own_segment = read("req-414", cust("cust-1"), segment, None);
    assert_eq!(
        read_recorded(&mut engine, &own_segment),
        ReadOutcome::Allowed {}
    );

    let mut out_of_reach = Engine::with_policy(
        Kept::world(),
        Rules::permitting(),
        Says::of(vec![(party_of("cust-1", "ord-1"), Reply::Unreachable)]),
    );
    assert_eq!(read_recorded(&mut out_of_reach, &own), refused());

    let mut lying = Engine::with_policy(
        Kept::world(),
        Rules::permitting(),
        Says::of(vec![(party_of("cust-2", "ord-1"), Reply::Given(Said::Yes))]),
    );
    assert_eq!(read_recorded(&mut lying, &other), refused());
}

/// A provider reads only its own segments: another provider is refused a segment it isn't the provider of, whether
/// it names the segment's provider or itself, recorded, even where the deployment's policy would allow anything,
/// and no provider reads an order or the trail. Each segment's own provider is allowed, and the log admits its read.
// verifies: REQ-AUZ-7
#[test]
fn provider_cannot_read_another_providers_segment() {
    let mut engine = Engine::with_policy(
        Kept::world(),
        Rules::permitting(),
        Says::of(vec![
            (
                provider_of("bridge-pay", "ord-1", "p1-s1"),
                Reply::Given(Said::Yes),
            ),
            (
                provider_of("fx-desk", "ord-1", "p1-s1"),
                Reply::Given(Said::No),
            ),
            (
                provider_of("fx-desk", "ord-1", "p1-s2"),
                Reply::Given(Said::Yes),
            ),
        ]),
    );
    let provider = |party: &str| actor(party, party, Role::Provider);
    let segment = |seg: &str, of: &str| json!({"kind": "segment", "order": "ord-1", "segment": seg, "provider": of});

    let naming_another = read(
        "req-420",
        provider("fx-desk"),
        segment("p1-s1", "bridge-pay"),
        None,
    );
    assert_eq!(read_recorded(&mut engine, &naming_another), refused());
    assert!(engine.sources.asked.is_empty(), "refused on its face");
    let naming_itself = read(
        "req-421",
        provider("fx-desk"),
        segment("p1-s1", "fx-desk"),
        None,
    );
    assert_eq!(read_recorded(&mut engine, &naming_itself), refused());
    let order = read(
        "req-422",
        provider("bridge-pay"),
        json!({"kind": "order", "order": "ord-1"}),
        None,
    );
    assert_eq!(read_recorded(&mut engine, &order), refused());
    let trail = read(
        "req-423",
        provider("bridge-pay"),
        json!({"kind": "trail"}),
        None,
    );
    assert_eq!(read_recorded(&mut engine, &trail), refused());

    let own = read(
        "req-424",
        provider("bridge-pay"),
        segment("p1-s1", "bridge-pay"),
        None,
    );
    assert_eq!(read_recorded(&mut engine, &own), ReadOutcome::Allowed {});
    let own = read(
        "req-425",
        provider("fx-desk"),
        segment("p1-s2", "fx-desk"),
        None,
    );
    assert_eq!(read_recorded(&mut engine, &own), ReadOutcome::Allowed {});
    assert_eq!(
        engine.policy.1.ruled, 2,
        "the deployment ruled on the two it allowed"
    );
}

/// A regulator's read rests on an identified permission: without one, naming one never granted, one granted to
/// another party, or one revoked, it is refused and recorded, even where the deployment's policy would allow
/// anything. Granted by the operator through policy, the same read is allowed, and the log admits it. A checker's
/// permission over one order covers that order's segments and nothing else. Only a permission granted can be
/// revoked, and the deployment's policy still decides a read the permission covers.
// verifies: REQ-AUZ-7, REQ-AUD-12
#[test]
fn regulators_read_without_its_permission_is_refused_and_recorded() {
    let mut engine = Engine::with_policy(
        Kept::world(),
        Rules::permitting(),
        Says::of(vec![(
            segment_of("ord-1", "p1-s1"),
            Reply::Given(Said::Yes),
        )]),
    );
    let regulator = actor("fin-regulator", "examiner-3", Role::Regulator);
    let trail = || json!({"kind": "trail"});

    let unnamed = read("req-430", regulator.clone(), trail(), None);
    assert_eq!(read_recorded(&mut engine, &unnamed), refused());
    let named = read("req-431", regulator.clone(), trail(), Some("perm-reg-1"));
    assert_eq!(
        read_recorded(&mut engine, &named),
        refused(),
        "not yet granted"
    );
    assert_eq!(
        engine.policy.1.ruled, 0,
        "refused before the deployment is asked"
    );

    let grant: PermissionChange = valid!("permission-change-granting-a-regulator-the-trail");
    assert!(engine.grant_permission(&grant).unwrap().permit().is_some());
    let named = read("req-432", regulator.clone(), trail(), Some("perm-reg-1"));
    assert_eq!(read_recorded(&mut engine, &named), ReadOutcome::Allowed {});
    let order = read(
        "req-433",
        regulator.clone(),
        json!({"kind": "order", "order": "ord-1"}),
        Some("perm-reg-1"),
    );
    assert_eq!(
        read_recorded(&mut engine, &order),
        ReadOutcome::Allowed {},
        "the trail covers its orders"
    );

    let another = actor("other-regulator", "examiner-9", Role::Regulator);
    let borrowed = read("req-434", another, trail(), Some("perm-reg-1"));
    assert_eq!(
        read_recorded(&mut engine, &borrowed),
        refused(),
        "granted to another party"
    );

    let checker = actor("audit-firm", "checker-7", Role::Checker);
    let mut over_an_order = grant.clone();
    over_an_order.request = id("req-g2");
    over_an_order.permission = id("perm-chk-1");
    over_an_order.grantee = checker.clone();
    over_an_order.scope =
        serde_json::from_value(json!({"kind": "order", "order": "ord-1"})).unwrap();
    assert!(
        engine
            .grant_permission(&over_an_order)
            .unwrap()
            .permit()
            .is_some()
    );
    let its_segment = read(
        "req-435",
        checker.clone(),
        json!({"kind": "segment", "order": "ord-1", "segment": "p1-s1", "provider": "bridge-pay"}),
        Some("perm-chk-1"),
    );
    assert_eq!(
        read_recorded(&mut engine, &its_segment),
        ReadOutcome::Allowed {}
    );
    for (request, reads) in [
        ("req-436", json!({"kind": "order", "order": "ord-2"})),
        ("req-437", trail()),
    ] {
        let outside = read(request, checker.clone(), reads, Some("perm-chk-1"));
        assert_eq!(
            read_recorded(&mut engine, &outside),
            refused(),
            "outside its scope"
        );
    }

    let mut revocation = grant.clone();
    revocation.request = id("req-r1");
    assert!(
        engine
            .revoke_permission(&revocation)
            .unwrap()
            .permit()
            .is_some()
    );
    let revoked = read("req-438", regulator.clone(), trail(), Some("perm-reg-1"));
    assert_eq!(read_recorded(&mut engine, &revoked), refused(), "revoked");

    // A permission never granted can't be revoked: policy refuses it, and the log takes no revocation of it at all.
    let mut never = revocation.clone();
    never.request = id("req-r2");
    never.permission = id("perm-never");
    let ruled = engine.policy.1.ruled;
    assert!(matches!(
        engine.revoke_permission(&never),
        Err(AppendFailure::Refused(_))
    ));
    assert_eq!(engine.policy.1.ruled, ruled, "the deployment isn't asked");

    // The deployment's policy still decides a read the permission covers.
    let mut denying = Engine::with_policy(Kept::world(), Rules::refusing(), Says::default());
    denying.register.grants.push(grant.clone());
    let named = read("req-439", regulator, trail(), Some("perm-reg-1"));
    assert_eq!(read_recorded(&mut denying, &named), refused());
    assert_eq!(denying.policy.1.ruled, 1);
}

/// A session goes only to a party that holds the account, as the account's record says, or to its delegate: naming
/// oneself the holder of another's account opens nothing, and neither does naming an accomplice holder who vouches
/// for one. The real holder, the record saying so, is opened; the record out of reach gives not known.
// verifies: REQ-CAT-3
#[test]
fn session_naming_a_party_that_does_not_hold_the_account_is_refused() {
    let says = || {
        Says::of(vec![
            (holds("cust-1", "acct-ref-4"), Reply::Given(Said::Yes)),
            (holds("mallory", "acct-ref-4"), Reply::Given(Said::No)),
            (holds("accomplice", "acct-ref-4"), Reply::Given(Said::No)),
            (
                delegation("mallory", "acct-ref-4", "accomplice"),
                Reply::Given(Said::Yes),
            ),
        ])
    };
    let open = |request: &SessionRequest, says: Says| {
        let mut engine = Engine::with_policy(Kept::default(), Rules::permitting(), says);
        engine.set_clock(nine());
        let answer = engine.open_session(request).unwrap();
        assert_eq!(
            engine.log.last(&answer.receipt).outcome,
            answer.outcome.outcome()
        );
        (answer.outcome, engine)
    };
    let refused = SessionOutcome::Refused {
        class: RefusalClass::NotPermitted,
    };

    let mut herself = own_session("req-320");
    herself.holder = id("mallory");
    herself.requester = actor("mallory", "mallory", Role::Customer);
    let (outcome, engine) = open(&herself, says());
    assert_eq!(outcome, refused, "naming herself the holder");
    assert_eq!(engine.sources.asked, [holds("mallory", "acct-ref-4")]);
    assert!(engine.register().sessions().is_empty());

    let mut accomplice = herself.clone();
    accomplice.request = id("req-321");
    accomplice.holder = id("accomplice");
    let (outcome, engine) = open(&accomplice, says());
    assert_eq!(outcome, refused, "an accomplice named holder");
    assert!(engine.register().sessions().is_empty());

    let (outcome, _) = open(&own_session("req-322"), says());
    assert!(matches!(outcome, SessionOutcome::Opened { .. }));
    let (outcome, _) = open(
        &own_session("req-323"),
        Says::of(vec![(holds("cust-1", "acct-ref-4"), Reply::Unreachable)]),
    );
    assert_eq!(
        outcome,
        SessionOutcome::NotKnown {
            cause: NotKnownCause::VerifierUnreachable
        }
    );
}

/// A segment read is of a segment of the order it names, as the segment's record says: a customer naming its own
/// order beside another order's segment is refused, and so is a checker whose permission covers that order. The same
/// readers are allowed a segment of the order, and the log admits each.
// verifies: REQ-AUZ-7
#[test]
fn customer_cannot_read_another_orders_segment_by_naming_its_own_order() {
    let mut engine = Engine::with_policy(
        Kept::world(),
        Rules::permitting(),
        Says::of(vec![
            (party_of("cust-1", "ord-1"), Reply::Given(Said::Yes)),
            (segment_of("ord-1", "p1-s1"), Reply::Given(Said::Yes)),
            (segment_of("ord-1", "ord-9-s1"), Reply::Given(Said::No)),
        ]),
    );
    let segment = |seg: &str| json!({"kind": "segment", "order": "ord-1", "segment": seg, "provider": "bridge-pay"});
    let customer = actor("cust-1", "cust-1", Role::Customer);

    let foreign = read("req-450", customer.clone(), segment("ord-9-s1"), None);
    assert_eq!(read_recorded(&mut engine, &foreign), refused());
    let own = read("req-451", customer, segment("p1-s1"), None);
    assert_eq!(read_recorded(&mut engine, &own), ReadOutcome::Allowed {});

    let checker = actor("audit-firm", "checker-7", Role::Checker);
    let mut over_an_order: PermissionChange =
        valid!("permission-change-granting-a-regulator-the-trail");
    over_an_order.request = id("req-g2");
    over_an_order.permission = id("perm-chk-1");
    over_an_order.grantee = checker.clone();
    over_an_order.scope =
        serde_json::from_value(json!({"kind": "order", "order": "ord-1"})).unwrap();
    assert!(
        engine
            .grant_permission(&over_an_order)
            .unwrap()
            .permit()
            .is_some()
    );
    let foreign = read(
        "req-452",
        checker.clone(),
        segment("ord-9-s1"),
        Some("perm-chk-1"),
    );
    assert_eq!(read_recorded(&mut engine, &foreign), refused());
    let own = read("req-453", checker, segment("p1-s1"), Some("perm-chk-1"));
    assert_eq!(read_recorded(&mut engine, &own), ReadOutcome::Allowed {});
}

/// A read is refused unless a rule lets its reader make it: the operator, a verifier and Core's own parts, roles the
/// customer's and the provider's rules don't name, read neither the trail, nor an order, nor a segment, nor a
/// resource without a permission, even where the deployment's policy would allow anything. A customer and a provider
/// still read who the checker is.
// verifies: REQ-AUZ-7
#[test]
fn roles_the_read_rule_does_not_list_read_nothing_without_a_permission() {
    let mut engine = Engine::with_policy(
        Kept::world(),
        Rules::permitting(),
        Says::of(vec![(
            segment_of("ord-1", "p1-s1"),
            Reply::Given(Said::Yes),
        )]),
    );
    let reads = [
        json!({"kind": "trail"}),
        json!({"kind": "order", "order": "ord-1"}),
        json!({"kind": "segment", "order": "ord-1", "segment": "p1-s1", "provider": "bridge-pay"}),
        json!({"kind": "resource", "resource": "checker"}),
    ];
    let mut n = 0;
    for role in [Role::Operator, Role::Verifier, Role::Core] {
        for what in &reads {
            n += 1;
            let check = read(
                &format!("req-46{n:02}"),
                actor("operator", "op-login-2", role),
                what.clone(),
                None,
            );
            assert_eq!(
                read_recorded(&mut engine, &check),
                refused(),
                "{role:?} {what}"
            );
        }
    }
    assert_eq!(
        engine.policy.1.ruled, 0,
        "refused before the deployment is asked"
    );
    for (request, role) in [("req-470", Role::Customer), ("req-471", Role::Provider)] {
        let who = read(
            request,
            actor("cust-1", "cust-1", role),
            reads[3].clone(),
            None,
        );
        assert_eq!(
            read_recorded(&mut engine, &who),
            ReadOutcome::Allowed {},
            "{role:?}"
        );
    }
}

/// A kept statement is read by a party as its own, as the evidence store's record says, or by a checker by a
/// permission over it: another party is refused it, the operator too, and a checker without that permission, each
/// recorded.
// verifies: REQ-AUZ-7, REQ-AUD-5, REQ-AUD-10
#[test]
fn party_reads_only_its_own_evidence_and_a_checker_by_its_permission() {
    let mut engine = Engine::with_policy(
        Kept::world(),
        Rules::permitting(),
        Says::of(vec![
            (own_statement("bridge-pay", "86"), Reply::Given(Said::Yes)),
            (own_statement("fx-desk", "86"), Reply::Given(Said::No)),
            (own_statement("operator", "86"), Reply::Given(Said::No)),
        ]),
    );
    let evidence = |byte: &str| json!({"kind": "evidence", "commitment": commitment(byte)});
    let provider = |party: &str| actor(party, party, Role::Provider);

    let own = read("req-480", provider("bridge-pay"), evidence("86"), None);
    assert_eq!(read_recorded(&mut engine, &own), ReadOutcome::Allowed {});
    let another = read("req-481", provider("fx-desk"), evidence("86"), None);
    assert_eq!(read_recorded(&mut engine, &another), refused());
    let operator = read(
        "req-482",
        actor("operator", "op-login-2", Role::Operator),
        evidence("86"),
        None,
    );
    assert_eq!(read_recorded(&mut engine, &operator), refused());

    let checker = actor("audit-firm", "checker-7", Role::Checker);
    let without = read("req-483", checker.clone(), evidence("86"), None);
    assert_eq!(read_recorded(&mut engine, &without), refused());
    let mut over_it: PermissionChange = valid!("permission-change-granting-a-regulator-the-trail");
    over_it.request = id("req-g3");
    over_it.permission = id("perm-chk-ev");
    over_it.grantee = checker.clone();
    over_it.scope = serde_json::from_value(evidence("86")).unwrap();
    assert!(
        engine
            .grant_permission(&over_it)
            .unwrap()
            .permit()
            .is_some()
    );
    let by_permission = read(
        "req-484",
        checker.clone(),
        evidence("86"),
        Some("perm-chk-ev"),
    );
    assert_eq!(
        read_recorded(&mut engine, &by_permission),
        ReadOutcome::Allowed {}
    );
    let other_statement = read("req-485", checker, evidence("87"), Some("perm-chk-ev"));
    assert_eq!(read_recorded(&mut engine, &other_statement), refused());
}

/// A session's bound is measured from Core's clock, never from the time its request states: a request stating a
/// moment a year on, asking a session of ten minutes from it, is refused; one ending more than the longest after
/// Core's now is refused, and so is one already over by Core's clock; one ending within the longest of Core's now is
/// opened, however long after its stated moment. The deployment isn't asked of a refused one.
// verifies: REQ-CAT-3
#[test]
fn session_bound_is_measured_from_cores_clock_not_the_requests_at() {
    let open = |now: &str, at_: &str, until: &str| {
        let mut engine = Engine::with_policy(
            Kept::default(),
            Rules::permitting(),
            Says::of(vec![(
                holds("cust-1", "acct-ref-4"),
                Reply::Given(Said::Yes),
            )]),
        );
        engine.set_clock(Stopped::at(&at(now)));
        let mut request = own_session("req-330");
        request.at = at(at_);
        request.until = at(until);
        let outcome = engine.open_session(&request).unwrap().outcome;
        (outcome, engine.policy.1.ruled)
    };
    let refused = (
        SessionOutcome::Refused {
            class: RefusalClass::NotPermitted,
        },
        0,
    );
    let nine = "2026-11-10T09:00:00Z";
    assert_eq!(
        open(nine, "2027-11-10T09:00:00Z", "2027-11-10T09:10:00Z"),
        refused,
        "a stated moment a year on"
    );
    assert_eq!(
        open(nine, nine, "2026-11-10T09:15:01Z"),
        refused,
        "past the longest"
    );
    assert_eq!(
        open("2026-11-10T09:20:00Z", nine, "2026-11-10T09:15:00Z"),
        refused,
        "already over"
    );
    assert!(matches!(
        open(nine, nine, "2026-11-10T09:15:00Z").0,
        SessionOutcome::Opened { .. }
    ));
    assert!(matches!(
        open("2026-11-10T09:10:00Z", nine, "2026-11-10T09:20:00Z").0,
        SessionOutcome::Opened { .. }
    ));
}

/// A read the log refuses is still recorded: where the order's record says yes wrongly and the log won't take the
/// read as permitted, the engine records it refused, and answers so. A log that can't be reached answers nothing.
// verifies: REQ-AUZ-7
#[test]
fn read_the_log_refuses_is_recorded_as_refused() {
    let lying = || Says::of(vec![(party_of("cust-2", "ord-1"), Reply::Given(Said::Yes))]);
    let mut engine = Engine::with_policy(Kept::world(), Rules::permitting(), lying());
    let other = read(
        "req-490",
        actor("cust-2", "cust-2", Role::Customer),
        json!({"kind": "order", "order": "ord-1"}),
        None,
    );
    assert_eq!(read_recorded(&mut engine, &other), refused());

    struct Down;
    impl PolicyLog for Down {
        fn append_decision(&mut self, _: &DecisionEntry) -> Result<Receipt, AppendFailure> {
            Err(AppendFailure::Unavailable)
        }
    }
    let mut down = Engine::with_policy(Down, Rules::permitting(), lying());
    assert_eq!(down.check_read(&other), Err(AppendFailure::Unavailable));
}

/// Who the checker is is named to customers and providers, so they read it without a permission; any other resource
/// of the deployment rests on a permission, whatever its name, even where the deployment's policy would allow it.
// verifies: REQ-AUZ-7, REQ-AUD-10
#[test]
fn only_the_checkers_name_is_read_without_a_permission() {
    let mut engine = Engine::with_policy(Kept::world(), Rules::permitting(), Says::default());
    let resource = |name: &str| json!({"kind": "resource", "resource": name});
    for (n, role) in [Role::Customer, Role::Provider].into_iter().enumerate() {
        let reader = actor("cust-1", "cust-1", role);
        let checker = read(
            &format!("req-50{n}"),
            reader.clone(),
            resource("checker"),
            None,
        );
        assert_eq!(
            read_recorded(&mut engine, &checker),
            ReadOutcome::Allowed {},
            "{role:?}"
        );
        let keys = read(
            &format!("req-51{n}"),
            reader,
            resource("operator_keys"),
            None,
        );
        assert_eq!(read_recorded(&mut engine, &keys), refused(), "{role:?}");
    }
    assert_eq!(
        engine.policy.1.ruled, 2,
        "only the checker's name reached the deployment"
    );
}

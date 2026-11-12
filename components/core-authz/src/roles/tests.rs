//! `T-021-03`'s tests: only the operator posts a route, each operator permission alone allows its own action, and
//! the operator acts for a user only at a reserve that declared it can't require the user's own withdrawal, each
//! decision recorded with who acted, against a log that holds every entry to core-internal's admission.

use super::*;
use rutent_contracts::core_internal::calls::{
    AppendFailure, DecisionEntry, Decisions, GeneralEntry, Log, PolicyLog,
};
use rutent_contracts::core_internal::{
    ActorKind, Answer, Authorisation, Decision, DecisionRequest, Entry, NotKnownCause, Outcome,
    Permit, RELATIONS, Receipt, Through, TimeSource, VerifiedEntry, VerifiedKind, Violation, What,
    When, admit_entry,
};
use rutent_contracts::router_api::Timestamp;
use serde_json::{Value, json};

use crate::engine::{Engine, Policy, Sources};

/// A valid fixture of the contract, read as whatever type the call site wants.
macro_rules! valid {
    ($name:expr) => {{
        let path = format!(
            "{}/../../contracts/core-internal/fixtures/valid/{}.json",
            env!("CARGO_MANIFEST_DIR"),
            $name
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }};
}

/// A well-formed Ed25519 key for the catalogue changes that name one.
const ADVERTISING_KEY: &str = "PT4_QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1w";

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn actor(party: &str, acts: &str, role: Role, kind: ActorKind) -> Actor {
    Actor {
        party: id(party),
        acts: id(acts),
        role,
        kind,
    }
}

/// The operator's login the fixtures name.
fn operator() -> Actor {
    actor("operator", "op-login-2", Role::Operator, ActorKind::Person)
}

/// A log that holds every entry to the contract's admission on each of its two handles, as `CMP-CORE-AUDIT`'s log
/// must: an effect only on a permitting decision of this log for the same action, a decision only through policy's
/// handle.
#[derive(Default)]
struct Kept(Vec<(Receipt, Entry)>);

impl PolicyLog for Kept {
    fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
        self.admit(Entry::Verified(entry.entry().clone()), Through::Policy)
    }
}

/// What the log reads as permitted (`T-021-04`): the fixtures' authorisation, held before the test began, then each
/// authorisation it took as permitted.
impl crate::funds::Recorded for Kept {
    fn permitted(&mut self) -> Result<Vec<Authorisation>, crate::funds::ReadFailure> {
        let mut held: Vec<Authorisation> = vec![valid!("authorisation-of-one-order-with-a-floor")];
        held.extend(crate::funds::permitted_in(self.0.iter().map(|(_, e)| e)));
        Ok(held)
    }

    fn charged(&mut self) -> Result<Vec<crate::parties::Counted>, crate::funds::ReadFailure> {
        Ok(crate::parties::charged_in(self.0.iter().map(|(_, e)| e)))
    }
}

impl Log for Kept {
    fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        self.admit(entry.entry().clone(), Through::General)
    }
}

impl Kept {
    fn admit(&mut self, entry: Entry, through: Through) -> Result<Receipt, AppendFailure> {
        entry.check().map_err(AppendFailure::Refused)?;
        if let Some((receipt, _)) = self.0.iter().find(|(_, e)| e == &entry) {
            return Ok(receipt.clone());
        }
        // The deployment's operator: the maker it signs its own statements as, as the log's `StatementKeys::operator`
        // answers, and the party its logins act for. These tests append no stated entry, so both are the ones the
        // other test logs of this crate name.
        let operator = rutent_contracts::core_internal::DeploymentOperator {
            maker: id("operator"),
            party: id("operator"),
        };
        admit_entry(&self.0, &entry, through, &operator).map_err(AppendFailure::Refused)?;
        let index = 2_000 + self.0.len();
        let receipt: Receipt = serde_json::from_value(json!({
            "log": "log-roles",
            "index": index,
            "entry": {"algorithm": "sha-256", "value": format!("{index:064x}")},
        }))
        .unwrap();
        self.0.push((receipt.clone(), entry));
        Ok(receipt)
    }

    /// A log holding one coherent world: order `ord-1`, accepted by `cust-1` as its requester, beneficiary and payer.
    fn world() -> Self {
        let path = format!(
            "{}/../../contracts/core-internal/fixtures/admission/world--grant.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let world: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        Kept(
            world["history"]
                .as_array()
                .unwrap()
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

    /// A log holding the decision record the fixtures' posted route cites, so a permitted post is admitted.
    fn with_a_decision_record() -> Self {
        let mut record: Value = valid!("verified-entry-of-a-first-decision-record");
        record["what"]["order"] = json!("ord-18");
        record["what"]["record"]["value"] = json!("2b".repeat(32));
        let mut log = Kept::default();
        log.append(&serde_json::from_value(record).unwrap())
            .unwrap();
        log
    }

    /// The entry `decision`'s receipt names, which must be the last one, recording its answer.
    fn entry_of(&self, decision: &Decision) -> &VerifiedEntry {
        let (receipt, Entry::Verified(entry)) = self.0.last().unwrap() else {
            panic!("a verified entry");
        };
        assert_eq!(
            receipt, &decision.receipt,
            "the answer carries its entry's receipt"
        );
        assert_eq!(entry.outcome, decision.outcome.outcome());
        entry
    }

    /// Appends the effect of `request`'s action on `permit`, as orchestration would before it acts.
    fn effect(
        &mut self,
        request: &DecisionRequest,
        permit: Permit,
    ) -> Result<Receipt, AppendFailure> {
        let entry = Entry::Verified(VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: actor("rutent", "core-orch", Role::Core, ActorKind::Component),
            what: What::Effect {
                action: request.action.clone(),
                decision: permit,
            },
            when: When {
                at: Timestamp::new("2026-11-10T09:00:01Z").unwrap(),
                source: TimeSource::CoreClock,
            },
            outcome: Outcome::ToTakeEffect {},
        });
        self.append(&GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?)
    }
}

/// A permit for `decision`, built by hand whatever it answered: what a path around policy would hold.
fn forged(decision: &Decision) -> Permit {
    serde_json::from_value(json!({
        "request": decision.request,
        "decision": decision.decision,
        "answer": "permit",
        "receipt": decision.receipt,
    }))
    .unwrap()
}

/// The log's refusal of an effect whose permit names no permitting decision of it.
fn no_permitting_decision() -> Result<Receipt, AppendFailure> {
    Err(AppendFailure::Refused(Violation(RELATIONS[0].refusal)))
}

/// The deployment's policy: it needs nothing, permits everything the fixed rules let through, and counts its rulings.
#[derive(Default)]
struct Permits {
    ruled: usize,
}

impl Policy for Permits {
    fn needs(&mut self, _: Asked<'_>) -> Reply<Vec<Need>> {
        Reply::Given(Vec::new())
    }

    fn rule(&mut self, _: Asked<'_>, _: &[Input]) -> Reply<Ruling> {
        self.ruled += 1;
        Reply::Given(Ruling::Permit)
    }
}

/// The deployment's register of permissions, the reserves' declarations and the orders' records, as the assembly
/// answers them: who holds which permission, by actor; what each reserve declared, an answer per declaration; and each
/// order's requester and origin reserve. It keeps what it was asked.
///
/// Who each actor acts for is listed where a test sets it (`acts_for`); an actor no test lists acts for the party its
/// request states, and that word is kept out of `asked`, which counts what the rules ask beyond it. Every agent acts
/// within the scope its party set (`Source::AgentScope`, `T-021-05`), a word kept out of `asked` too.
#[derive(Default)]
struct Register {
    holds: Vec<(Id, Id, Permission)>,
    declarations: Vec<(Id, &'static str, Reply<Said>)>,
    requesters: Vec<(Id, Id)>,
    origins: Vec<(Id, Id)>,
    acts_for: Vec<(Id, Id)>,
    others: Vec<(Need, Reply<Said>)>,
    asked: Vec<Need>,
}

impl Register {
    /// A register where `who` holds `permissions` and nothing else.
    fn holding(who: &Actor, permissions: &[Permission]) -> Self {
        Register::default().and_holding(who, permissions)
    }

    /// The register, with `who` holding `permissions` too.
    fn and_holding(mut self, who: &Actor, permissions: &[Permission]) -> Self {
        self.holds.extend(
            permissions
                .iter()
                .map(|p| (who.acts.clone(), who.party.clone(), *p)),
        );
        self
    }

    /// The register, with `reserve` answering `said` about its declaration that it can't require the user's own
    /// withdrawal.
    fn declaring(self, reserve: &str, said: Reply<Said>) -> Self {
        self.declaring_that(reserve, NO_USER_WITHDRAWAL, said)
    }

    /// The register, with `reserve` answering `said` about `declaration`.
    fn declaring_that(
        mut self,
        reserve: &str,
        declaration: &'static str,
        said: Reply<Said>,
    ) -> Self {
        self.declarations.push((id(reserve), declaration, said));
        self
    }

    /// The register, with `order` requested by `party` and starting at `origin`.
    fn with_order(mut self, order: &str, party: &str, origin: &str) -> Self {
        self.requesters.push((id(order), id(party)));
        self.origins.push((id(order), id(origin)));
        self
    }

    /// The register, with `acts` acting for `party` and for no party it isn't listed for.
    fn acting(mut self, acts: &str, party: &str) -> Self {
        self.acts_for.push((id(acts), id(party)));
        self
    }

    /// The register, with `said` the answer to `need`.
    fn saying(mut self, need: Need, said: Reply<Said>) -> Self {
        self.others.push((need, said));
        self
    }

    /// The fixtures' world: `ord-17` requested by `cust-1` and `ord-19` by `acme-treasury`, both from `us-bank`;
    /// Core's `core-orch` carrying an authorisation onward, and the operator's `op-login-2` acting for a user.
    fn world() -> Self {
        Register::default()
            .with_order("ord-17", "cust-1", "us-bank")
            .with_order("ord-19", "acme-treasury", "us-bank")
            .and_holding(&core(), &[Permission::CarryOnward])
            .and_holding(&operator(), &[Permission::ActForUser])
    }
}

/// Core's own actor that asks for value to move.
fn core() -> Actor {
    actor("rutent", "core-orch", Role::Core, ActorKind::Component)
}

/// Which of `records` names `key`: yes where it names `value` with it, no where it names another, missing where it
/// holds no record of `key`.
fn record_of(records: &[(Id, Id)], key: &Id, value: &Id) -> Reply<Said> {
    match records.iter().find(|(k, _)| k == key) {
        Some((_, v)) => Reply::Given(if v == value { Said::Yes } else { Said::No }),
        None => Reply::Missing,
    }
}

impl Sources for Register {
    fn supply(&mut self, need: &Need) -> Reply<Said> {
        // The kind each actor states is the one its login is listed as: no test here turns on it (`T-021-05`'s do).
        if let Source::ActsFor { acts, party, .. } = &need.from {
            let listed = self.acts_for.iter().any(|(a, _)| a == acts);
            let acts_for = self.acts_for.iter().any(|(a, p)| a == acts && p == party);
            return Reply::Given(if acts_for || !listed {
                Said::Yes
            } else {
                Said::No
            });
        }
        if matches!(need.from, Source::AgentScope { .. }) {
            return Reply::Given(Said::Yes);
        }
        self.asked.push(need.clone());
        if let Some((_, said)) = self.others.iter().find(|(n, _)| n == need) {
            return said.clone();
        }
        match &need.from {
            Source::Permissions { party, permission } => {
                let held = self
                    .holds
                    .iter()
                    .any(|(a, p, h)| a == &need.input && p == party && h == permission);
                Reply::Given(if held { Said::Yes } else { Said::No })
            }
            Source::Declarations(reserve) => self
                .declarations
                .iter()
                .find(|(r, d, _)| r == reserve && *d == need.input.as_str())
                .map_or(Reply::Missing, |(_, _, said)| said.clone()),
            Source::Requester(order) => record_of(&self.requesters, order, &need.input),
            Source::OrderOrigin(order) => record_of(&self.origins, order, &need.input),
            _ => Reply::Missing,
        }
    }
}

/// Every permission of the operator's, each with a request asking for its action by `caller`: posting the fixtures'
/// suggested route, and each catalogue change. The match has no catch-all, so a change a contract bump adds fails this
/// build until it is listed.
fn every_permission(caller: &Actor) -> Vec<(Permission, DecisionRequest)> {
    let mut asked = Vec::new();
    let mut post: DecisionRequest = valid!("decision-request-to-post-a-suggested-route");
    post.caller = caller.clone();
    asked.push((Permission::PostRoute, post));
    for change in [
        CatalogueChange::Grant,
        CatalogueChange::RevokeGrant,
        CatalogueChange::AcceptModule,
        CatalogueChange::RevokeRelease,
        CatalogueChange::MarkStale,
        CatalogueChange::DeclareExploited,
        CatalogueChange::Reinstate,
        CatalogueChange::WithdrawRelease,
        CatalogueChange::SetSlippage,
        CatalogueChange::Promote,
    ] {
        match change {
            CatalogueChange::Grant
            | CatalogueChange::RevokeGrant
            | CatalogueChange::AcceptModule
            | CatalogueChange::RevokeRelease
            | CatalogueChange::MarkStale
            | CatalogueChange::DeclareExploited
            | CatalogueChange::Reinstate
            | CatalogueChange::WithdrawRelease
            | CatalogueChange::SetSlippage
            | CatalogueChange::Promote => {}
        }
        let mut request: DecisionRequest = valid!("decision-request-to-mark-a-release-stale");
        request.request = id(&format!("req-{change:?}").to_lowercase());
        request.caller = caller.clone();
        request.action = Action::ChangeCatalogue {
            change,
            provider: id("bridge-pay"),
            version: change.is_about_a_release().then(|| id("2.2.0")),
            tuple: change.is_about_a_tuple().then(|| id("usd-cop")),
            advertising_keys: if change == CatalogueChange::Grant {
                vec![rutent_contracts::provider_role::PublicKey::new(ADVERTISING_KEY).unwrap()]
            } else {
                vec![]
            },
            statement_key: (change == CatalogueChange::AcceptModule)
                .then(|| rutent_contracts::provider_role::PublicKey::new(ADVERTISING_KEY).unwrap()),
        };
        asked.push((Permission::Change(change), request));
    }
    asked
}

/// The engine of these tests: the fixed rules, a deployment's policy that permits what they let through, and `register`,
/// over a log that held the fixtures' authorisation as permitted before the test began, so the fixtures' move acts
/// inside it (`T-021-04`).
fn engine(log: Kept, register: Register) -> Engine<Kept, Permits, Register> {
    Engine::with_policy(log, Permits::default(), register)
}

/// The refusal of a caller no rule lets ask for its action.
const NOT_PERMITTED: Answer = Answer::Refuse {
    class: RefusalClass::NotPermitted,
};

/// A route posted in any role but the operator's, by a person, an agent or a component, is refused by the fixed rule
/// before the deployment's policy or the register is asked, and recorded refused with who asked; the log admits no
/// effect on it. The operator's login holding the permission posts the same route, and the log admits its effect.
// verifies: REQ-AUZ-2
#[test]
fn provider_posting_a_route_is_refused() {
    let anyone = Register::holding(
        &actor(
            "bridge-pay",
            "bridge-pay",
            Role::Provider,
            ActorKind::Component,
        ),
        &[Permission::PostRoute],
    );
    let mut policy = engine(Kept::with_a_decision_record(), anyone);
    let roles = [
        Role::Customer,
        Role::Provider,
        Role::Verifier,
        Role::Regulator,
        Role::Checker,
        Role::Core,
    ];
    let kinds = [ActorKind::Person, ActorKind::Agent, ActorKind::Component];
    for role in roles {
        for kind in kinds {
            let mut post: DecisionRequest = valid!("decision-request-to-post-a-suggested-route");
            // An agent is never its own party (`T-021-05`), so bridge-pay's agent is one of its own.
            let acts = if kind == ActorKind::Agent {
                "bridge-pay-agent"
            } else {
                "bridge-pay"
            };
            post.caller = actor("bridge-pay", acts, role, kind);
            let decision = policy.decide(&post).unwrap();
            assert_eq!(decision.outcome, NOT_PERMITTED, "{role:?} {kind:?}");
            assert!(decision.permit().is_none());
            assert_eq!(policy.log().entry_of(&decision).who, post.caller);
            assert_eq!(
                policy.log.effect(&post, forged(&decision)),
                no_permitting_decision()
            );
        }
    }
    assert_eq!(
        policy.policy.1.ruled, 0,
        "the deployment's policy not asked"
    );
    assert!(policy.sources.asked.is_empty(), "the register not asked");

    // The register lists the operator's login alone: a provider stating the operator's role holds nothing there.
    let mut policy = engine(
        Kept::with_a_decision_record(),
        Register::holding(&operator(), &[Permission::PostRoute]),
    );
    let mut claimed: DecisionRequest = valid!("decision-request-to-post-a-suggested-route");
    claimed.request = id("req-claimed");
    claimed.caller = actor(
        "bridge-pay",
        "bridge-pay",
        Role::Operator,
        ActorKind::Component,
    );
    let decision = policy.decide(&claimed).unwrap();
    assert_eq!(decision.outcome, NOT_PERMITTED, "a stated operator's role");
    assert_eq!(
        policy.log.effect(&claimed, forged(&decision)),
        no_permitting_decision()
    );
    let post: DecisionRequest = valid!("decision-request-to-post-a-suggested-route");
    let decision = policy.decide(&post).unwrap();
    let permit = decision.permit().expect("the operator posts it");
    assert!(policy.log.effect(&post, permit).is_ok());
}

/// A provider's suggestion has no standing of its own: the provider asking for it to be posted is refused, an operator
/// login that doesn't hold the permission is refused, and no effect rests on either. Posted by the operator's login
/// that holds it, it is permitted, and the decision's entry records the operator as who acted and the provider as who
/// suggested it, each in its own role.
// verifies: REQ-AUZ-2
#[test]
fn suggested_route_has_no_standing_until_posted_and_names_its_suggester() {
    let suggester = actor(
        "bridge-pay",
        "bridge-pay",
        Role::Provider,
        ActorKind::Component,
    );
    let posted: DecisionRequest = valid!("decision-request-to-post-a-suggested-route");
    let Action::PostRoute { suggested_by, .. } = &posted.action else {
        panic!("a posted route");
    };
    assert_eq!(suggested_by.as_ref(), Some(&suggester));
    let mut policy = engine(
        Kept::with_a_decision_record(),
        Register::holding(&operator(), &[Permission::PostRoute]),
    );

    // The suggester asks for its own suggestion to be posted.
    let mut by_itself = posted.clone();
    by_itself.request = id("req-201");
    by_itself.caller = suggester.clone();
    let decision = policy.decide(&by_itself).unwrap();
    assert_eq!(decision.outcome, NOT_PERMITTED);
    assert_eq!(
        policy.log.effect(&by_itself, forged(&decision)),
        no_permitting_decision()
    );

    // An operator login that doesn't hold the permission to post.
    let mut unheld = posted.clone();
    unheld.request = id("req-202");
    unheld.caller.acts = id("op-login-9");
    let decision = policy.decide(&unheld).unwrap();
    assert_eq!(decision.outcome, NOT_PERMITTED);
    assert_eq!(
        policy.log.effect(&unheld, forged(&decision)),
        no_permitting_decision()
    );

    // Posted by the operator's login that holds the permission: standing, and the suggester named apart.
    let decision = policy.decide(&posted).unwrap();
    assert_eq!(decision.outcome, Answer::Permit {});
    let entry = policy.log().entry_of(&decision);
    assert_eq!(entry.who, operator());
    let What::Decide { request, .. } = &entry.what else {
        panic!("a decide entry");
    };
    assert_eq!(
        request.action, posted.action,
        "the entry holds the post whole, its suggester named"
    );
    assert_eq!(request.caller.role, Role::Operator);
    assert_eq!(suggester.role, Role::Provider);
    let permit = decision.permit().unwrap();
    assert!(policy.log.effect(&posted, permit).is_ok());
}

/// Each of the operator's permissions, held alone by one login, allows that one action and refuses every other, each
/// decision recorded with its actor; another login of the same party holds none of them. One party that is both a
/// provider and the operator is refused every change in its provider's role, the register not asked, and allowed the
/// one it holds in its operator's role, the two roles recorded apart.
// verifies: REQ-AUZ-6, REQ-AUD-8
#[test]
fn each_permission_alone_allows_only_its_own_action() {
    let all = every_permission(&operator());
    assert_eq!(all.len(), 11, "posting a route and ten catalogue changes");
    for (held, _) in &all {
        let mut policy = engine(
            Kept::with_a_decision_record(),
            Register::holding(&operator(), &[*held]),
        );
        for (permission, request) in &all {
            let decision = policy.decide(request).unwrap();
            let entry = policy.log().entry_of(&decision).clone();
            assert_eq!(entry.who, operator(), "recorded with its actor");
            assert!(
                matches!(&entry.what, What::Decide { request: r, .. } if r == request),
                "recorded with its action"
            );
            if permission == held {
                assert_eq!(
                    decision.outcome,
                    Answer::Permit {},
                    "{held:?} allows itself"
                );
                assert!(
                    policy
                        .log
                        .effect(request, decision.permit().unwrap())
                        .is_ok()
                );
            } else {
                assert_eq!(
                    decision.outcome, NOT_PERMITTED,
                    "{held:?} alone allows no {permission:?}"
                );
                assert_eq!(
                    policy.log.effect(request, forged(&decision)),
                    no_permitting_decision()
                );
            }
        }
        assert_eq!(
            policy.policy.1.ruled, 1,
            "only its own action reached policy"
        );

        // Another login of the operator's party holds none of them.
        let other = actor("operator", "op-login-9", Role::Operator, ActorKind::Person);
        for (permission, request) in every_permission(&other) {
            let decision = policy.decide(&request).unwrap();
            assert_eq!(decision.outcome, NOT_PERMITTED, "{permission:?}");
        }
    }

    // One party holding two roles: `fx-desk` is a provider, and the operator's party too.
    let as_operator = actor("fx-desk", "fx-ops-1", Role::Operator, ActorKind::Person);
    let as_provider = actor("fx-desk", "fx-ops-1", Role::Provider, ActorKind::Person);
    let stale = Permission::Change(CatalogueChange::MarkStale);
    let mut policy = engine(Kept::default(), Register::holding(&as_operator, &[stale]));
    for (permission, request) in every_permission(&as_provider) {
        if permission == Permission::PostRoute {
            continue;
        }
        let decision = policy.decide(&request).unwrap();
        assert_eq!(
            decision.outcome, NOT_PERMITTED,
            "{permission:?} as a provider"
        );
        assert_eq!(policy.log().entry_of(&decision).who.role, Role::Provider);
    }
    assert!(
        policy.sources.asked.is_empty(),
        "a provider holds no permission"
    );
    let (_, mut request) = every_permission(&as_operator)
        .into_iter()
        .find(|(p, _)| *p == stale)
        .unwrap();
    request.request = id("req-fx-stale");
    let decision = policy.decide(&request).unwrap();
    assert_eq!(decision.outcome, Answer::Permit {});
    assert_eq!(policy.log().entry_of(&decision).who, as_operator);
    let roles: Vec<Role> = policy
        .log
        .0
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(v) if v.who.party.as_str() == "fx-desk" => Some(v.who.role),
            _ => None,
        })
        .collect();
    assert!(roles.contains(&Role::Provider) && roles.contains(&Role::Operator));
}

/// The operator moving value, or giving an authorisation, for a user at an origin reserve that never declared it
/// can't require the user's own withdrawal is refused for the missing declaration; at one that declared it can, it is
/// refused outright; a reserve out of reach gives not known. Only at a reserve that declared it is the operator let
/// through. An authorisation in a role that can't act for the user, a reserve's, a verifier's or Core's, is refused,
/// and so is value moved on a provider's, a verifier's, a regulator's or a checker's request.
// verifies: REQ-AUZ-9
#[test]
fn operator_acting_for_a_user_at_a_reserve_that_never_declared_it_is_refused() {
    let mut moved: DecisionRequest = valid!("decision-request-to-move-value");
    moved.caller = operator();
    let mut authorised: Authorisation = valid!("authorisation-of-one-order-with-a-floor");
    authorised.given_by = operator();
    // A reserve answers both of its declarations at once, as `REQ-CAT-6` has it make both at publishing.
    let declares = |reserve: &str, withdrawal: Reply<Said>| {
        Register::world()
            .declaring(reserve, withdrawal)
            .declaring_that(reserve, AUTOMATIC_DEPOSIT, Reply::Given(Said::No))
    };
    let cases = [
        (
            Register::world(),
            Answer::Refuse {
                class: RefusalClass::MissingInput,
            },
        ),
        (declares("us-bank", Reply::Given(Said::No)), NOT_PERMITTED),
        (
            Register::world().declaring("us-bank", Reply::Unreachable),
            Answer::NotKnown {
                cause: NotKnownCause::VerifierUnreachable,
            },
        ),
        (
            declares("us-bank", Reply::Given(Said::Yes)),
            Answer::Permit {},
        ),
        // Another reserve's declaration says nothing of this one.
        (
            declares("bank-co", Reply::Given(Said::Yes)),
            Answer::Refuse {
                class: RefusalClass::MissingInput,
            },
        ),
    ];
    for (n, (register, answer)) in cases.into_iter().enumerate() {
        let mut policy = engine(Kept::default(), register);
        let decision = policy.decide(&moved).unwrap();
        assert_eq!(decision.outcome, answer, "case {n}: the move");
        assert_eq!(policy.log().entry_of(&decision).who, operator());
        if decision.permit().is_none() {
            assert_eq!(
                policy.log.effect(&moved, forged(&decision)),
                no_permitting_decision()
            );
        }
        assert_eq!(
            policy.consult(Asked::Authorise(&authorised)),
            answer,
            "case {n}: the authorisation"
        );
        assert!(
            policy.sources.asked.iter().all(|n| !matches!(
                &n.from,
                Source::Declarations(r) if r.as_str() != "us-bank"
            )),
            "case {n}: only the origin reserve's declarations asked"
        );
    }

    let mut policy = engine(
        Kept::default(),
        Register::world().declaring("us-bank", Reply::Given(Said::Yes)),
    );
    // A provider other than the origin reserve, a verifier, and Core never give the user's authorisation.
    for role in [Role::Provider, Role::Verifier, Role::Core] {
        let mut by = authorised.clone();
        by.given_by = actor("bridge-pay", "bridge-pay", role, ActorKind::Component);
        assert_eq!(
            policy.consult(Asked::Authorise(&by)),
            NOT_PERMITTED,
            "{role:?}"
        );
    }
    for role in [
        Role::Provider,
        Role::Verifier,
        Role::Regulator,
        Role::Checker,
    ] {
        let mut by = moved.clone();
        by.caller = actor("us-bank", "us-bank", role, ActorKind::Component);
        let decision = policy.decide(&by).unwrap();
        assert_eq!(decision.outcome, NOT_PERMITTED, "{role:?}");
    }
    for given in [
        valid!("authorisation-of-one-order-with-a-floor"),
        valid!("authorisation-of-one-order-given-by-an-agent"),
    ] {
        let given: Authorisation = given;
        assert_eq!(
            policy.consult(Asked::Authorise(&given)),
            Answer::Permit {},
            "the user, or its agent, authorises at its origin"
        );
    }
}

/// Value moved on one order for its user is recorded with who acted: the user, Core carrying the user's authorisation
/// onward, or the operator at a reserve that declared it, each entry naming its actor and role as the request did,
/// and never another's.
// verifies: REQ-AUZ-9
#[test]
fn record_says_who_acted_for_the_user() {
    let mut policy = engine(
        Kept::default(),
        Register::world().declaring("us-bank", Reply::Given(Said::Yes)),
    );
    let customer = actor("cust-1", "cust-1", Role::Customer, ActorKind::Person);
    for (n, who) in [customer, core(), operator()].into_iter().enumerate() {
        let mut moved: DecisionRequest = valid!("decision-request-to-move-value");
        moved.request = id(&format!("req-30{n}"));
        moved.caller = who.clone();
        let decision = policy.decide(&moved).unwrap();
        assert_eq!(decision.outcome, Answer::Permit {}, "{who:?}");
        let entry = policy.log().entry_of(&decision);
        assert_eq!(entry.who, who, "the record says who acted");
        assert!(matches!(&entry.what, What::Decide { request, .. } if request.caller == who));
    }
    let roles: Vec<Role> = policy
        .log
        .0
        .iter()
        .map(|(_, e)| match e {
            Entry::Verified(v) => v.who.role,
            Entry::Stated(_) => panic!("no stated entry"),
        })
        .collect();
    assert_eq!(
        roles,
        [Role::Customer, Role::Core, Role::Operator],
        "one entry for each actor, its role recorded apart"
    );
    assert_eq!(policy.policy.1.ruled, 3);
}

/// A role stated in the request never acts for a user on its own: the operator's login stating Core's role moves no
/// value, stating a customer's gives no authorisation for an order it didn't request, and a customer stating the
/// operator's role moves no value, nor a customer's on another's order. Each actor in its own role is let through.
// verifies: REQ-AUZ-9
#[test]
fn a_stated_role_alone_never_acts_for_a_user() {
    let mut policy = engine(
        Kept::default(),
        Register::world()
            .declaring("us-bank", Reply::Given(Said::Yes))
            .declaring_that("us-bank", AUTOMATIC_DEPOSIT, Reply::Given(Said::Yes)),
    );
    let moved: DecisionRequest = valid!("decision-request-to-move-value");
    let authorised: Authorisation = valid!("authorisation-of-one-order-with-a-floor");
    let move_by = |n: u8, who: Actor| {
        let mut m = moved.clone();
        m.request = id(&format!("req-40{n}"));
        m.caller = who;
        m
    };
    let login_as_core = actor("operator", "op-login-2", Role::Core, ActorKind::Component);
    let login_as_customer = actor("operator", "op-login-2", Role::Customer, ActorKind::Person);
    let customer_as_operator = actor("cust-1", "cust-1", Role::Operator, ActorKind::Person);
    let other_customer = actor(
        "acme-treasury",
        "acme-treasury",
        Role::Customer,
        ActorKind::Person,
    );

    for (n, who) in [login_as_core, customer_as_operator, other_customer]
        .into_iter()
        .enumerate()
    {
        let request = move_by(n as u8, who.clone());
        let decision = policy.decide(&request).unwrap();
        assert_eq!(decision.outcome, NOT_PERMITTED, "{who:?} moves no value");
        assert_eq!(
            policy.log.effect(&request, forged(&decision)),
            no_permitting_decision()
        );
    }
    let mut by_login = authorised.clone();
    by_login.given_by = login_as_customer;
    assert_eq!(
        policy.consult(Asked::Authorise(&by_login)),
        NOT_PERMITTED,
        "the operator's login stating a customer's role authorises nothing"
    );
    assert_eq!(
        policy.policy.1.ruled, 0,
        "none reached the deployment's policy"
    );

    for (n, who) in [
        core(),
        operator(),
        actor("cust-1", "cust-1", Role::Customer, ActorKind::Person),
    ]
    .into_iter()
    .enumerate()
    {
        let decision = policy.decide(&move_by(5 + n as u8, who.clone())).unwrap();
        assert_eq!(
            decision.outcome,
            Answer::Permit {},
            "{who:?} in its own role"
        );
    }
    assert_eq!(
        policy.consult(Asked::Authorise(&authorised)),
        Answer::Permit {}
    );
}

/// The operator naming as the order's origin a reserve that declared it can't require the user's own withdrawal, but
/// that isn't the order's origin, moves no value and gives no authorisation; nor does a reserve naming itself the origin
/// of an order that started elsewhere.
// verifies: REQ-AUZ-9
#[test]
fn operator_naming_a_declaring_reserve_that_is_not_the_orders_origin_is_refused() {
    let mut policy = engine(
        Kept::default(),
        Register::world()
            .declaring("bank-co", Reply::Given(Said::Yes))
            .declaring_that("bank-co", AUTOMATIC_DEPOSIT, Reply::Given(Said::Yes)),
    );
    let mut moved: DecisionRequest = valid!("decision-request-to-move-value");
    moved.caller = operator();
    if let Action::MoveValue { origin_reserve, .. } = &mut moved.action {
        *origin_reserve = id("bank-co");
    }
    let decision = policy.decide(&moved).unwrap();
    assert_eq!(decision.outcome, NOT_PERMITTED);
    assert_eq!(
        policy.log.effect(&moved, forged(&decision)),
        no_permitting_decision()
    );
    let mut authorised: Authorisation = valid!("authorisation-of-one-order-with-a-floor");
    authorised.origin_reserve = id("bank-co");
    authorised.given_by = operator();
    assert_eq!(policy.consult(Asked::Authorise(&authorised)), NOT_PERMITTED);
    authorised.given_by = actor("bank-co", "bank-co", Role::Provider, ActorKind::Component);
    assert_eq!(policy.consult(Asked::Authorise(&authorised)), NOT_PERMITTED);
    assert_eq!(policy.policy.1.ruled, 0);
}

/// A reserve that declared it starts routes on automatic deposits authorises its own order's withdrawal with no user
/// present, and the operator may file one for it on that declaration alone; a reserve that declared it doesn't, never
/// declared it, or isn't the order's origin, authorises nothing, and neither does the operator there. The operator
/// still moves value only where the reserve declared it can't require the user's own withdrawal.
// verifies: REQ-AUZ-9, REQ-ORD-13
#[test]
fn reserve_that_declared_automatic_deposits_authorises_with_no_user_present() {
    let reserve = actor("us-bank", "us-bank", Role::Provider, ActorKind::Component);
    let given_by = |who: &Actor| {
        let mut a: Authorisation = valid!("authorisation-of-one-order-with-a-floor");
        a.given_by = who.clone();
        a
    };
    let declares = |automatic: Reply<Said>| {
        Register::world()
            .declaring("us-bank", Reply::Given(Said::No))
            .declaring_that("us-bank", AUTOMATIC_DEPOSIT, automatic)
    };
    let cases = [
        (declares(Reply::Given(Said::Yes)), Answer::Permit {}),
        (declares(Reply::Given(Said::No)), NOT_PERMITTED),
        (
            Register::world(),
            Answer::Refuse {
                class: RefusalClass::MissingInput,
            },
        ),
    ];
    for (n, (register, answer)) in cases.into_iter().enumerate() {
        let mut policy = engine(Kept::default(), register);
        assert_eq!(
            policy.consult(Asked::Authorise(&given_by(&reserve))),
            answer,
            "case {n}: the reserve"
        );
        assert_eq!(
            policy.consult(Asked::Authorise(&given_by(&operator()))),
            answer,
            "case {n}: the operator filing for it"
        );
    }

    let mut policy = engine(Kept::default(), declares(Reply::Given(Said::Yes)));
    let elsewhere = actor("bank-co", "bank-co", Role::Provider, ActorKind::Component);
    assert_eq!(
        policy.consult(Asked::Authorise(&given_by(&elsewhere))),
        NOT_PERMITTED,
        "a reserve that isn't the order's origin"
    );
    let mut moved: DecisionRequest = valid!("decision-request-to-move-value");
    moved.caller = operator();
    assert_eq!(
        policy.decide(&moved).unwrap().outcome,
        NOT_PERMITTED,
        "automatic deposits alone let the operator move no value for the user"
    );
}

/// The refusal of an actor that doesn't act for the party its request states.
const NO_PARTY: Answer = Answer::Refuse {
    class: RefusalClass::NoAccountableParty,
};

/// An actor stating a party the register doesn't say it acts for acts for no one: `cust-9` stating `cust-1` moves no
/// value on `cust-1`'s order and gives no authorisation for it, and `evil-pay` stating `us-bank` authorises none of
/// `us-bank`'s automatic deposits. The actors a request names besides its caller are held alike: a permission granted
/// to a grantee stating another party, and a route posted naming a suggester stating another party, are refused.
/// Each is refused for having no accountable party, before the deployment's policy is asked, and the log admits no
/// effect on it. The same requests from, or naming, the parties' own actors are let through.
// verifies: REQ-AUZ-8, REQ-AUZ-9
#[test]
fn actor_stating_another_party_acts_for_no_one() {
    let register = || {
        Register::world()
            .acting("cust-9", "cust-9")
            .acting("cust-1", "cust-1")
            .acting("evil-pay", "evil-pay")
            .acting("us-bank", "us-bank")
            .declaring("us-bank", Reply::Given(Said::No))
            .declaring_that("us-bank", AUTOMATIC_DEPOSIT, Reply::Given(Said::Yes))
    };
    let mut policy = engine(Kept::default(), register());
    let forged_customer = actor("cust-1", "cust-9", Role::Customer, ActorKind::Person);
    let forged_reserve = actor("us-bank", "evil-pay", Role::Provider, ActorKind::Component);

    let mut moved: DecisionRequest = valid!("decision-request-to-move-value");
    moved.caller = forged_customer.clone();
    let decision = policy.decide(&moved).unwrap();
    assert_eq!(decision.outcome, NO_PARTY, "a move on another's order");
    assert_eq!(policy.log().entry_of(&decision).who, forged_customer);
    assert_eq!(
        policy.log.effect(&moved, forged(&decision)),
        no_permitting_decision()
    );
    let mut authorised: Authorisation = valid!("authorisation-of-one-order-with-a-floor");
    authorised.given_by = forged_customer;
    assert_eq!(policy.consult(Asked::Authorise(&authorised)), NO_PARTY);
    authorised.given_by = forged_reserve;
    assert_eq!(policy.consult(Asked::Authorise(&authorised)), NO_PARTY);
    assert_eq!(
        policy.policy.1.ruled, 0,
        "none reached the deployment's policy"
    );

    let mut policy = engine(Kept::default(), register());
    moved.caller = actor("cust-1", "cust-1", Role::Customer, ActorKind::Person);
    assert_eq!(policy.decide(&moved).unwrap().outcome, Answer::Permit {});
    authorised.given_by = moved.caller.clone();
    assert_eq!(
        policy.consult(Asked::Authorise(&authorised)),
        Answer::Permit {}
    );
    authorised.given_by = actor("us-bank", "us-bank", Role::Provider, ActorKind::Component);
    assert_eq!(
        policy.consult(Asked::Authorise(&authorised)),
        Answer::Permit {}
    );

    // A permission's grantee: `examiner-3` acts for `fin-regulator`, and states `other-regulator`.
    {
        use rutent_contracts::core_internal::PermissionChange;
        use rutent_contracts::core_internal::calls::Reads as _;

        let register = Register::default()
            .acting("op-login-2", "operator")
            .acting("examiner-3", "fin-regulator");
        let mut policy = engine(Kept::default(), register);
        let granted: PermissionChange = valid!("permission-change-granting-a-regulator-the-trail");
        let mut forged_grantee = granted.clone();
        forged_grantee.request = id("req-g9");
        forged_grantee.grantee.party = id("other-regulator");
        let decision = policy.grant_permission(&forged_grantee).unwrap();
        assert_eq!(
            decision.outcome, NO_PARTY,
            "a grantee stating another party"
        );
        assert!(policy.register().grants.is_empty(), "nothing granted");
        let decision = policy.grant_permission(&granted).unwrap();
        assert_eq!(
            decision.outcome,
            Answer::Permit {},
            "the grantee's own party"
        );
    }

    // A route's suggester: `bridge-pay` acts for `bridge-pay`, and states `fx-desk`.
    {
        let register = Register::holding(&operator(), &[Permission::PostRoute])
            .acting("op-login-2", "operator")
            .acting("bridge-pay", "bridge-pay");
        let mut policy = engine(Kept::with_a_decision_record(), register);
        let posted: DecisionRequest = valid!("decision-request-to-post-a-suggested-route");
        let mut forged_suggester = posted.clone();
        forged_suggester.request = id("req-s9");
        if let Action::PostRoute {
            suggested_by: Some(suggester),
            ..
        } = &mut forged_suggester.action
        {
            suggester.party = id("fx-desk");
        }
        let decision = policy.decide(&forged_suggester).unwrap();
        assert_eq!(
            decision.outcome, NO_PARTY,
            "a suggester stating another party"
        );
        assert_eq!(
            policy.log.effect(&forged_suggester, forged(&decision)),
            no_permitting_decision()
        );
        let decision = policy.decide(&posted).unwrap();
        assert_eq!(
            decision.outcome,
            Answer::Permit {},
            "the suggester's own party"
        );
    }
}

/// An actor stating the account's holder for its party, when the register says it acts for another, opens no session
/// on that account, though the account's record says the party it states holds it; the holder's own actor opens one.
// verifies: REQ-AUZ-8, REQ-CAT-3
#[test]
fn session_opened_by_an_actor_stating_another_party_is_refused() {
    use rutent_contracts::core_internal::calls::Sessions;
    use rutent_contracts::core_internal::{SessionOutcome, SessionRequest};

    let mut request: SessionRequest = valid!("session-request-of-a-delegate");
    let holds = Need {
        input: request.holder.clone(),
        from: Source::Account(request.account.clone()),
    };
    let register = Register::default()
        .acting("agent-7", "acme-treasury")
        .acting("cust-1", "cust-1")
        .saying(holds, Reply::Given(Said::Yes));
    let mut policy = engine(Kept::default(), register);
    policy.set_clock(crate::session::Stopped::at(&request.at));

    // `agent-7` acts for `acme-treasury`, and states the holder, `cust-1`, as its party.
    request.requester.party = request.holder.clone();
    let answer = policy.open_session(&request).unwrap();
    assert!(
        matches!(
            answer.outcome,
            SessionOutcome::Refused {
                class: RefusalClass::NoAccountableParty
            }
        ),
        "{:?}",
        answer.outcome
    );
    assert!(policy.register().sessions().is_empty(), "no session opened");

    request.request = id("req-301");
    request.requester = actor("cust-1", "cust-1", Role::Customer, ActorKind::Person);
    let answer = policy.open_session(&request).unwrap();
    assert!(matches!(answer.outcome, SessionOutcome::Opened { .. }));
}

/// An actor stating a customer's party, when the register says it acts for another, reads none of that customer's
/// orders, though the order's record and the log say that party is its requester; the customer's own actor reads it.
// verifies: REQ-AUZ-8, REQ-AUZ-7
#[test]
fn read_by_an_actor_stating_another_party_is_refused() {
    use rutent_contracts::core_internal::calls::Reads as _;
    use rutent_contracts::core_internal::{ReadCheck, ReadOutcome};

    let party_of = Need {
        input: id("cust-1"),
        from: Source::OrderParties(id("ord-1")),
    };
    let register = Register::default()
        .acting("cust-9", "cust-9")
        .acting("cust-1", "cust-1")
        .saying(party_of, Reply::Given(Said::Yes));
    let mut policy = engine(Kept::world(), register);
    let read = |request: &str, reader: Actor| -> ReadCheck {
        serde_json::from_value(json!({
            "request": request,
            "reader": reader,
            "reads": {"kind": "order", "order": "ord-1"},
            "at": "2026-11-10T09:00:00Z",
        }))
        .unwrap()
    };

    let forged = read(
        "req-450",
        actor("cust-1", "cust-9", Role::Customer, ActorKind::Person),
    );
    let answer = policy.check_read(&forged).unwrap();
    assert_eq!(
        answer.outcome,
        ReadOutcome::Refused {
            class: RefusalClass::NoAccountableParty
        }
    );
    let own = read(
        "req-451",
        actor("cust-1", "cust-1", Role::Customer, ActorKind::Person),
    );
    assert_eq!(
        policy.check_read(&own).unwrap().outcome,
        ReadOutcome::Allowed {}
    );
}

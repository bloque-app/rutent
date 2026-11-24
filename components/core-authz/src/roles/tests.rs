//! `T-021-03`'s tests: only the operator posts a route, each operator permission alone allows its own action, and
//! the operator acts for a user only at a reserve that declared it can't require the user's own withdrawal, each
//! decision recorded with who acted, against a log that holds every entry to core-internal's admission.

use super::*;
use rutent_contracts::core_internal::SettingKind;
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

    fn part_of(
        &mut self,
        order: &Id,
        segment: &Id,
    ) -> Result<Option<Id>, crate::funds::ReadFailure> {
        let plan = crate::funds::plan_of_the_fixtures_move();
        Ok(crate::funds::part_in(
            std::iter::once(&plan).chain(self.0.iter().map(|(_, e)| e)),
            order,
            segment,
        ))
    }
}

/// What the log holds, with the fixtures' orders' acceptances before it.
impl crate::stuck::Receipted for Kept {
    fn receipted(&mut self) -> Result<Vec<(Receipt, Entry)>, crate::funds::ReadFailure> {
        let mut entries = crate::funds::acceptances_of_the_fixtures();
        entries.extend(self.0.iter().cloned());
        Ok(entries)
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
        Kept::of_world("grant").0
    }

    /// A log holding the history of one case of the contract's world, and the request the case's entry records.
    fn of_world(name: &str) -> (Self, Value) {
        let path = format!(
            "{}/../../contracts/core-internal/fixtures/admission/world--{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let world: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let kept = Kept(
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
        );
        (kept, world["entry"]["what"]["request"].clone())
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
    /// The roles each listed login acts in, where a test binds them (`T-021-14`): a login with none listed acts in any
    /// role, as an unlisted one acts for any party.
    roles: Vec<(Id, Role)>,
    /// Each party that acts for another: the links of an `ActsFor` chain past a login's own party (`T-021-06`).
    links: Vec<(Id, Id)>,
    /// Each order's parties, where a test lists them for a stuck order's steps: its requester, beneficiary or payer.
    order_parties: Vec<(Id, Id)>,
    /// What the evidence store keeps, where a test lists it: each commitment, and the order it is about.
    kept: Vec<(rutent_contracts::module_interface::Digest, Option<Id>)>,
    others: Vec<(Need, Reply<Said>)>,
    /// The showings of fault the log holds: the order and segment each is about, how it is named, and when it was
    /// received.
    showings: Vec<(
        Id,
        Id,
        rutent_contracts::core_internal::ShowingNamed,
        Timestamp,
    )>,
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

    /// The register, with `who`'s login acting for its party in its role, and in no role it isn't listed for
    /// (`T-021-14`).
    fn listing(mut self, who: &Actor) -> Self {
        if !self
            .acts_for
            .contains(&(who.acts.clone(), who.party.clone()))
        {
            self.acts_for.push((who.acts.clone(), who.party.clone()));
        }
        self.roles.push((who.acts.clone(), who.role));
        self
    }

    /// The register, with the party `from` acting for the party `to`.
    fn linking(mut self, from: &str, to: &str) -> Self {
        self.links.push((id(from), id(to)));
        self
    }

    /// Everything `start`, a login or a party, reaches: itself, the parties it is listed for, and on along the links.
    fn chain(&self, start: &Id) -> Vec<Id> {
        let mut reached = vec![start.clone()];
        let mut i = 0;
        while i < reached.len() {
            let at = reached[i].clone();
            let next = self
                .acts_for
                .iter()
                .chain(&self.links)
                .filter(|(from, _)| from == &at)
                .map(|(_, to)| to.clone());
            for party in next.collect::<Vec<_>>() {
                if !reached.contains(&party) {
                    reached.push(party);
                }
            }
            i += 1;
        }
        reached
    }

    /// What `start` reaches below the deployment operator's party: its chain, cut where it reaches the operator.
    fn below_operator(&self, start: &Id) -> Vec<Id> {
        let mut reached = vec![start.clone()];
        let mut i = 0;
        while i < reached.len() {
            let at = reached[i].clone();
            let next = self
                .acts_for
                .iter()
                .chain(&self.links)
                .filter(|(from, _)| from == &at)
                .map(|(_, to)| to.clone());
            for party in next.collect::<Vec<_>>() {
                if party.as_str() != "operator" && !reached.contains(&party) {
                    reached.push(party);
                }
            }
            i += 1;
        }
        reached
    }

    /// The register, with the log holding `showing` of `segment` of `order`, received at `received`.
    fn showing(
        mut self,
        order: &str,
        segment: &str,
        showing: rutent_contracts::core_internal::ShowingNamed,
        received: &str,
    ) -> Self {
        self.showings.push((
            id(order),
            id(segment),
            showing,
            Timestamp::new(received).unwrap(),
        ));
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

/// Yes where `yes`, no where not.
fn said(yes: bool) -> Said {
    if yes { Said::Yes } else { Said::No }
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
        // The role it states is one its login is listed in, where a test lists any (`T-021-14`).
        if let Source::ActsFor {
            acts, party, role, ..
        } = &need.from
        {
            let listed = self.acts_for.iter().any(|(a, _)| a == acts);
            let acts_for = self.acts_for.iter().any(|(a, p)| a == acts && p == party);
            let roles: Vec<Role> = self
                .roles
                .iter()
                .filter(|(a, _)| a == acts)
                .map(|(_, r)| *r)
                .collect();
            let in_role = roles.is_empty() || roles.contains(role);
            return Reply::Given(if (acts_for || !listed) && in_role {
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
            // A stuck order's sources, where a test lists their records (`T-021-06`'s, for the sweep).
            Source::OfOperator => {
                Reply::Given(said(self.chain(&need.input).contains(&id("operator"))))
            }
            Source::Reaches { from, to } => Reply::Given(said(self.chain(from).contains(to))),
            Source::MeetsBelowOperator { from, with } => {
                let theirs = self.below_operator(with);
                Reply::Given(said(
                    self.below_operator(from).iter().any(|p| theirs.contains(p)),
                ))
            }
            Source::OrderParties(order) if !self.order_parties.is_empty() => Reply::Given(said(
                self.order_parties
                    .iter()
                    .any(|(o, p)| o == order && p == &need.input),
            )),
            Source::Kept(commitment) if !self.kept.is_empty() => {
                Reply::Given(said(self.kept.iter().any(|(c, about)| {
                    c == commitment && about.as_ref().is_none_or(|o| o == &need.input)
                })))
            }
            Source::KeptAbout(commitment) if !self.kept.is_empty() => {
                Reply::Given(said(self.kept.iter().any(|(c, about)| {
                    c == commitment && about.as_ref() == Some(&need.input)
                })))
            }
            Source::Requester(order) => record_of(&self.requesters, order, &need.input),
            Source::OrderOrigin(order) => record_of(&self.origins, order, &need.input),
            // The assembly's: the log holds the showing, and received it no later than the acceptance is decided.
            Source::Showing {
                order,
                segment,
                showing,
                decided_at,
            } => Reply::Given(
                if self.showings.iter().any(|(o, s, n, received)| {
                    o == order
                        && s == segment
                        && n == showing
                        && received.unix_nanos() <= decided_at.unix_nanos()
                }) {
                    Said::Yes
                } else {
                    Said::No
                },
            ),
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
        CatalogueChange::RollBack,
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
            | CatalogueChange::Promote
            | CatalogueChange::RollBack => {}
            // An operator's setting is a permission of its own for each setting, listed below (`T-021-15`).
            CatalogueChange::SetSetting => unreachable!("one permission for each setting, below"),
        }
        let mut request: DecisionRequest = valid!("decision-request-to-mark-a-release-stale");
        request.request = id(&format!("req-{change:?}").to_lowercase());
        request.caller = caller.clone();
        request.action = Action::ChangeCatalogue {
            change,
            provider: id("bridge-pay"),
            version: change.is_about_a_release().then(|| id("2.2.0")),
            tuple: change.is_about_a_tuple().then(|| id("usd-cop")),
            class: None,
            advertising_keys: if change == CatalogueChange::Grant {
                vec![rutent_contracts::provider_role::PublicKey::new(ADVERTISING_KEY).unwrap()]
            } else {
                vec![]
            },
            statement_key: (change == CatalogueChange::AcceptModule)
                .then(|| rutent_contracts::provider_role::PublicKey::new(ADVERTISING_KEY).unwrap()),
            setting: None,
        };
        asked.push((Permission::Change(change), request));
    }
    // Each of the operator's settings (`T-020-45`), a permission of its own apiece (`T-021-15`).
    for (kind, request) in every_setting_asked(caller) {
        asked.push((Permission::Setting(kind), request));
    }
    // Erasing a statement that later entries cite, the override of `T-021-13`: a permission of its own.
    asked.push((
        Permission::EraseOverridingCitations,
        overriding_erasure_asked(caller),
    ));
    asked
}

/// Each of the operator's settings, by its kind, as `caller` asks to set it: one fixture of the contract for each. The
/// match has no catch-all, so a setting a contract bump adds fails this build until it is listed.
fn every_setting_asked(caller: &Actor) -> Vec<(SettingKind, DecisionRequest)> {
    let fixtures = [
        (
            SettingKind::OperatorFee,
            "decision-request-to-set-the-operators-fee",
        ),
        (
            SettingKind::SlippageLimit,
            "decision-request-to-set-a-tuples-slippage-limit",
        ),
        (
            SettingKind::EvidenceFloor,
            "decision-request-to-set-a-tuples-evidence-floor",
        ),
        (
            SettingKind::PublicNode,
            "decision-request-to-set-a-public-nodes-settings",
        ),
        (
            SettingKind::ProviderCeiling,
            "decision-request-to-set-a-providers-ceiling",
        ),
        (
            SettingKind::PublisherCeiling,
            "decision-request-to-set-a-publishers-ceiling",
        ),
        (
            SettingKind::Cadence,
            "decision-request-to-set-a-jobs-cadence",
        ),
        (
            SettingKind::Cutover,
            "decision-request-to-set-a-protocol-versions-cutover",
        ),
    ];
    fixtures
        .into_iter()
        .map(|(kind, name)| {
            match kind {
                SettingKind::OperatorFee
                | SettingKind::SlippageLimit
                | SettingKind::EvidenceFloor
                | SettingKind::PublicNode
                | SettingKind::ProviderCeiling
                | SettingKind::PublisherCeiling
                | SettingKind::Cadence
                | SettingKind::Cutover => {}
            }
            let path = format!(
                "{}/../../contracts/core-internal/fixtures/valid/{name}.json",
                env!("CARGO_MANIFEST_DIR")
            );
            let mut request: DecisionRequest =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            let Action::ChangeCatalogue {
                setting: Some(setting),
                ..
            } = &request.action
            else {
                panic!("{name} sets a setting");
            };
            assert_eq!(setting.kind(), kind, "{name}");
            request.request = id(&format!("req-set-{kind:?}").to_lowercase());
            request.caller = caller.clone();
            (kind, request)
        })
        .collect()
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

/// Every permission of the register's, each once. The match has no catch-all, so a permission a later row adds fails
/// this build until it is listed, and with it the sweep below.
fn every_permission_held() -> Vec<Permission> {
    let mut all = vec![Permission::PostRoute];
    for (permission, _) in every_permission(&operator()) {
        if let Permission::Change(_) | Permission::Setting(_) = permission {
            all.push(permission);
        }
    }
    all.extend([
        Permission::CarryOnward,
        Permission::ActForUser,
        Permission::ReadOrderState,
        Permission::DecideStuck,
        Permission::MoveStuck,
        Permission::ConfirmOutcome,
        Permission::AskReversal,
        Permission::AskStatus,
        Permission::DeclineOutcome,
        Permission::EraseStatement,
        Permission::EraseOverridingCitations,
        Permission::GrantRead,
        Permission::RevokeRead,
    ]);
    for permission in &all {
        match permission {
            Permission::PostRoute
            | Permission::Change(_)
            | Permission::Setting(_)
            | Permission::CarryOnward
            | Permission::ActForUser
            | Permission::ReadOrderState
            | Permission::DecideStuck
            | Permission::MoveStuck
            | Permission::ConfirmOutcome
            | Permission::AskReversal
            | Permission::AskStatus
            | Permission::DeclineOutcome
            | Permission::EraseStatement
            | Permission::EraseOverridingCitations
            | Permission::GrantRead
            | Permission::RevokeRead => {}
        }
    }
    all
}

/// The operator's desk that takes a stuck order's steps in the sweep: a person on a desk of its own, whose chain meets
/// the world's decider's and reporter's only at the operator (`T-021-06`).
fn desk() -> Actor {
    actor(
        "operator-desk-s",
        "op-login-s",
        Role::Operator,
        ActorKind::Person,
    )
}

/// The world's decider of a stuck order, on the operator's first desk, holding the decide step's permission alone.
fn alice() -> Actor {
    actor(
        "operator-desk-a",
        "alice",
        Role::Operator,
        ActorKind::Person,
    )
}

/// An operator's login that grants a read before the sweep's revocation, holding the permission to grant alone.
fn granter() -> Actor {
    actor("operator", "op-login-g", Role::Operator, ActorKind::Person)
}

/// The register of the sweep: the fixtures' orders and reserve, the operator's logins and desks by their parties, a
/// stuck order's world (`ord-1`, its customer `cust-1`, its holder `bank-co`, the evidence the store keeps about it),
/// the decider and the granter the sweep's setup needs, and each of `holders` holding its permissions.
fn swept(holders: &[(Actor, Vec<Permission>)]) -> Register {
    let mut register = Register::default()
        .with_order("ord-17", "cust-1", "us-bank")
        .with_order("ord-19", "acme-treasury", "us-bank")
        .declaring("us-bank", Reply::Given(Said::Yes))
        .declaring_that("us-bank", AUTOMATIC_DEPOSIT, Reply::Given(Said::No))
        .acting("op-login-2", "operator")
        .acting("op-login-9", "operator")
        .acting("op-login-g", "operator")
        .acting("op-login-s", "operator-desk-s")
        .acting("op-login-t", "operator-desk-t")
        .acting("alice", "operator-desk-a")
        .acting("op-login-5", "operator-desk-b")
        .acting("cust-1", "cust-1")
        .acting("bank-co", "bank-co")
        .acting("core-orch", "rutent")
        .acting("core-other", "rutent")
        .linking("operator-desk-s", "operator")
        .linking("operator-desk-t", "operator")
        .linking("operator-desk-a", "operator")
        .linking("operator-desk-b", "operator")
        .and_holding(&alice(), &[Permission::DecideStuck])
        .and_holding(&granter(), &[Permission::GrantRead]);
    register.order_parties.push((id("ord-1"), id("cust-1")));
    register.kept.push((
        serde_json::from_value(json!({"algorithm": "sha-256", "value": "db".repeat(32)})).unwrap(),
        Some(id("ord-1")),
    ));
    for (who, held) in holders {
        register = register.and_holding(who, held);
    }
    register
}

/// An actor, and the permissions the register lists it for.
type Held = (Actor, Vec<Permission>);

/// How a call of the sweep is made, over an engine whose register lists the actors it is handed.
type Runs = Box<dyn Fn(&[Held]) -> Ran>;

/// What one call of the sweep answered: its answer, who its entry records where it has one, and whether it reached
/// the deployment's policy.
struct Ran {
    answer: Answer,
    who: Option<Actor>,
    ruled: bool,
}

/// One call of the sweep: the permission it is the action of, its caller, and how it is made over an engine whose
/// register lists `holders`.
struct Call {
    name: String,
    owner: Permission,
    caller: Actor,
    run: Runs,
}

/// `request` decided over `log` and the sweep's register: a permit's effect the log admits where `effect` is set, and
/// no effect on a refusal's forged permit.
fn decided_in(
    log: Kept,
    holders: &[(Actor, Vec<Permission>)],
    request: &DecisionRequest,
    effect: bool,
) -> Ran {
    let mut policy = engine(log, swept(holders));
    let decision = policy.decide(request).unwrap();
    let entry = policy.log().entry_of(&decision).clone();
    assert!(
        matches!(&entry.what, What::Decide { request: r, .. } if r == request),
        "recorded with its action"
    );
    match decision.permit() {
        Some(permit) if effect => assert!(policy.log.effect(request, permit).is_ok()),
        Some(_) => {}
        None => assert_eq!(
            policy.log.effect(request, forged(&decision)),
            no_permitting_decision()
        ),
    }
    Ran {
        answer: decision.outcome,
        who: Some(entry.who),
        ruled: policy.policy.1.ruled == 1,
    }
}

/// The verified entry the log took last.
fn last_entry(log: &Kept) -> VerifiedEntry {
    let Some((_, Entry::Verified(entry))) = log.0.last() else {
        panic!("a verified entry");
    };
    entry.clone()
}

/// Every call a permission of the register's lets through, each with the one permission it is the action of: posting
/// a route, each catalogue change, a plain erasure and one overriding its citations; asking a reversal and a late
/// status, by the operator and by Core carrying the order on; moving value by Core and by the operator for a user,
/// and the operator's authorisation for one; the operator's read of an order's state; granting and revoking a read;
/// and a stuck order's four steps, its decision, its move, and confirming and declining a reported outcome.
fn every_call() -> Vec<Call> {
    use rutent_contracts::core_internal::calls::{
        OutcomeDeclines as _, Reads as _, StuckOrders as _,
    };
    use rutent_contracts::core_internal::{
        Decided, OutcomeConfirmation, OutcomeDecline, PermissionChange, ReadCheck, ReadOutcome,
        StuckDecide, StuckMove,
    };

    let mut calls: Vec<Call> = Vec::new();
    let decide = |name: String, owner, request: DecisionRequest, effect| Call {
        name,
        owner,
        caller: request.caller.clone(),
        run: Box::new(move |holders| {
            decided_in(Kept::with_a_decision_record(), holders, &request, effect)
        }),
    };
    for (owner, request) in every_permission(&operator()) {
        calls.push(decide(format!("{owner:?}"), owner, request, true));
    }
    calls.push(decide(
        "erase".into(),
        Permission::EraseStatement,
        erasure_asked(&operator()),
        true,
    ));
    let reversal = |caller: &Actor| {
        let mut request: DecisionRequest = valid!("decision-request-to-mark-a-release-stale");
        request.request = id(&format!("req-reverse-{}", caller.acts.as_str()));
        request.caller = caller.clone();
        request.action = Action::AskReversal {
            order: id("ord-17"),
            segment: id("seg-2"),
            provider: id("bridge-pay"),
        };
        request
    };
    let moved = |caller: &Actor| {
        let mut request: DecisionRequest = valid!("decision-request-to-move-value");
        request.request = id(&format!("req-move-{}", caller.acts.as_str()));
        request.caller = caller.clone();
        request
    };
    for (owner, caller) in [
        (Permission::AskReversal, operator()),
        (Permission::CarryOnward, core()),
    ] {
        calls.push(decide(
            format!("ask reversal by {}", caller.acts.as_str()),
            owner,
            reversal(&caller),
            false,
        ));
        calls.push(decide(
            format!("ask status by {}", caller.acts.as_str()),
            if owner == Permission::AskReversal {
                Permission::AskStatus
            } else {
                owner
            },
            status_asked(&caller),
            false,
        ));
    }
    for (owner, caller) in [
        (Permission::CarryOnward, core()),
        (Permission::ActForUser, operator()),
    ] {
        calls.push(decide(
            format!("move value by {}", caller.acts.as_str()),
            owner,
            moved(&caller),
            false,
        ));
    }

    // The operator's authorisation for a user, at a reserve that declared it can't require the user's own withdrawal.
    calls.push(Call {
        name: "authorise".into(),
        owner: Permission::ActForUser,
        caller: operator(),
        run: Box::new(|holders| {
            let mut given: Authorisation = valid!("authorisation-of-one-order-with-a-floor");
            given.given_by = operator();
            let mut policy = engine(Kept::default(), swept(holders));
            let answer = policy.consult(Asked::Authorise(&given));
            Ran {
                answer,
                who: None,
                ruled: policy.policy.1.ruled == 1,
            }
        }),
    });

    // The operator's read of an order's state and age, with no read permission.
    calls.push(Call {
        name: "read an order's state".into(),
        owner: Permission::ReadOrderState,
        caller: operator(),
        run: Box::new(|holders| {
            let check: ReadCheck = serde_json::from_value(json!({
                "request": "req-state",
                "reader": operator(),
                "reads": {"kind": "order_state", "order": "ord-1"},
                "at": "2026-11-10T09:00:00Z",
            }))
            .unwrap();
            let mut policy = engine(Kept::world(), swept(holders));
            let read = policy.check_read(&check).unwrap();
            Ran {
                answer: match read.outcome {
                    ReadOutcome::Allowed {} => Answer::Permit {},
                    ReadOutcome::Refused { class } => Answer::Refuse { class },
                },
                who: Some(last_entry(&policy.log).who),
                ruled: policy.policy.1.ruled == 1,
            }
        }),
    });

    // Granting a regulator a read of the trail, and revoking one the granter granted first.
    let grant = |by: &Actor, request: &str| {
        let mut change: PermissionChange =
            valid!("permission-change-granting-a-regulator-the-trail");
        change.request = id(request);
        change.by = by.clone();
        change
    };
    calls.push(Call {
        name: "grant a read".into(),
        owner: Permission::GrantRead,
        caller: operator(),
        run: Box::new(move |holders| {
            let mut policy = engine(Kept::world(), swept(holders));
            let decision = policy
                .grant_permission(&grant(&operator(), "req-grant"))
                .unwrap();
            Ran {
                answer: decision.outcome,
                who: Some(policy.log().entry_of(&decision).who.clone()),
                ruled: policy.policy.1.ruled == 1,
            }
        }),
    });
    calls.push(Call {
        name: "revoke a read".into(),
        owner: Permission::RevokeRead,
        caller: operator(),
        run: Box::new(move |holders| {
            let mut policy = engine(Kept::world(), swept(holders));
            let granted = policy
                .grant_permission(&grant(&granter(), "req-grant-first"))
                .unwrap();
            assert!(granted.permit().is_some(), "the granter grants first");
            let ruled = policy.policy.1.ruled;
            let decision = policy
                .revoke_permission(&grant(&operator(), "req-revoke"))
                .unwrap();
            Ran {
                answer: decision.outcome,
                who: Some(policy.log().entry_of(&decision).who.clone()),
                ruled: policy.policy.1.ruled == ruled + 1,
            }
        }),
    });

    // A stuck order's four steps, each by the operator's desk of the sweep, in the contract's world for it.
    calls.push(Call {
        name: "decide a stuck order".into(),
        owner: Permission::DecideStuck,
        caller: desk(),
        run: Box::new(|holders| {
            let (log, request) = Kept::of_world("sd");
            let mut decide: StuckDecide = serde_json::from_value(request).unwrap();
            decide.decider = desk();
            decide.request = id("req-desk-decides");
            let mut policy = engine(log, swept(holders));
            let decision = policy.decide_stuck(&decide).unwrap();
            Ran {
                answer: decision.outcome,
                who: Some(policy.log().entry_of(&decision).who.clone()),
                ruled: policy.policy.1.ruled == 1,
            }
        }),
    });
    calls.push(Call {
        name: "move a stuck order".into(),
        owner: Permission::MoveStuck,
        caller: desk(),
        run: Box::new(|holders| {
            let (log, request) = Kept::of_world("sd");
            let mut decide: StuckDecide = serde_json::from_value(request).unwrap();
            decide.decider = alice();
            decide.request = id("req-alice-decides");
            let mut policy = engine(log, swept(holders));
            let decided = policy.decide_stuck(&decide).unwrap();
            assert!(
                decided.permit().is_some(),
                "alice decides first: {decided:?}"
            );
            let ruled = policy.policy.1.ruled;
            let moved = StuckMove {
                request: id("req-desk-moves"),
                order: id("ord-1"),
                carries_out: Decided {
                    request: decided.request.clone(),
                    decision: decided.decision.clone(),
                },
                mover: desk(),
                at: Timestamp::new("2026-11-10T09:00:00Z").unwrap(),
            };
            let decision = policy.move_stuck(&moved).unwrap();
            Ran {
                answer: decision.outcome,
                who: Some(policy.log().entry_of(&decision).who.clone()),
                ruled: policy.policy.1.ruled == ruled + 1,
            }
        }),
    });
    calls.push(Call {
        name: "confirm an outcome".into(),
        owner: Permission::ConfirmOutcome,
        caller: desk(),
        run: Box::new(|holders| {
            let (log, request) = Kept::of_world("settled_es");
            let mut confirmation: OutcomeConfirmation = serde_json::from_value(request).unwrap();
            confirmation.confirmer = desk();
            confirmation.request = id("req-desk-confirms");
            let mut policy = engine(log, swept(holders));
            let decision = policy.confirm_outcome(&confirmation).unwrap();
            Ran {
                answer: decision.outcome,
                who: Some(policy.log().entry_of(&decision).who.clone()),
                ruled: policy.policy.1.ruled == 1,
            }
        }),
    });
    calls.push(Call {
        name: "decline an outcome".into(),
        owner: Permission::DeclineOutcome,
        caller: desk(),
        run: Box::new(|holders| {
            let (log, request) = Kept::of_world("decline");
            let mut decline: OutcomeDecline = serde_json::from_value(request).unwrap();
            decline.decliner = desk();
            decline.request = id("req-desk-declines");
            let mut policy = engine(log, swept(holders));
            let decision = policy.decline_outcome(&decline).unwrap();
            Ran {
                answer: decision.outcome,
                who: Some(policy.log().entry_of(&decision).who.clone()),
                ruled: policy.policy.1.ruled == 1,
            }
        }),
    });
    calls
}

/// Each of the register's permissions, held alone by each actor that makes a call of the sweep (the operator's login,
/// Core's carrier and the operator's desk), allows its own actions and refuses every other, each decision recorded
/// with its actor and only its own action reaching the deployment's policy: every permission, posting a route, each
/// catalogue change, the plain erasure and the override, a reversal and a late status asked, Core carrying an order on,
/// the operator acting for a user, its read of an order's state, granting and revoking a read, and a stuck order's
/// four steps (`T-021-14`'s widening). The override held alone allows nothing, and beside the erase permission allows
/// the two erasures. Another login or desk of the same parties holding the permission gives the callers none of them.
/// One party that is both a provider and the operator is refused every change in its provider's role, the register not
/// asked, and allowed the one it holds in its operator's role, the two roles recorded apart.
// verifies: REQ-AUZ-6, REQ-AUD-8, REQ-AUZ-7
#[test]
fn each_permission_alone_allows_only_its_own_action() {
    let all = every_permission_held();
    assert_eq!(all.len(), 33, "every permission of the register's");
    let calls = every_call();
    assert_eq!(calls.len(), 36, "every call a permission lets through");
    for permission in &all {
        assert!(
            calls.iter().any(|c| &c.owner == permission),
            "{permission:?} has a call in the sweep"
        );
    }
    let mut held_sets: Vec<Vec<Permission>> = all.iter().map(|p| vec![*p]).collect();
    held_sets.push(vec![
        Permission::EraseStatement,
        Permission::EraseOverridingCitations,
    ]);
    let callers = [operator(), core(), desk()];
    for held in &held_sets {
        let holders: Vec<(Actor, Vec<Permission>)> =
            callers.iter().map(|c| (c.clone(), held.clone())).collect();
        for call in &calls {
            let ran = (call.run)(&holders);
            let allowed = held.contains(&call.owner)
                && (call.owner != Permission::EraseOverridingCitations
                    || held.contains(&Permission::EraseStatement));
            if let Some(who) = &ran.who {
                assert_eq!(who, &call.caller, "{}: recorded with its actor", call.name);
            }
            if allowed {
                assert_eq!(
                    ran.answer,
                    Answer::Permit {},
                    "{held:?} allows {}",
                    call.name
                );
                assert!(ran.ruled, "{}: its own action reached policy", call.name);
            } else {
                assert_eq!(
                    ran.answer, NOT_PERMITTED,
                    "{held:?} alone allows no {}",
                    call.name
                );
                assert!(!ran.ruled, "{held:?}: {} never reached policy", call.name);
            }
        }

        // Another login or desk of the same parties, and another of Core's actors, holding it: the callers hold none.
        let others = [
            actor("operator", "op-login-9", Role::Operator, ActorKind::Person),
            actor("rutent", "core-other", Role::Core, ActorKind::Component),
            actor(
                "operator-desk-t",
                "op-login-t",
                Role::Operator,
                ActorKind::Person,
            ),
        ];
        let holders: Vec<(Actor, Vec<Permission>)> =
            others.iter().map(|o| (o.clone(), held.clone())).collect();
        for call in &calls {
            let ran = (call.run)(&holders);
            assert_eq!(
                ran.answer, NOT_PERMITTED,
                "{held:?} held by another login allows no {}",
                call.name
            );
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

/// A read permission, `permission`, over the trail, granted to or revoked from `grantee` by `by`, as request `request`.
fn read_change(
    request: &str,
    permission: &str,
    grantee: &Actor,
    by: &Actor,
) -> rutent_contracts::core_internal::PermissionChange {
    serde_json::from_value(json!({
        "request": request,
        "permission": permission,
        "grantee": grantee,
        "scope": {"kind": "trail"},
        "by": by,
        "at": "2026-11-10T09:00:00Z",
    }))
    .unwrap()
}

/// A read of the trail by `reader`, naming `permission`, as request `request`.
fn trail_read(
    request: &str,
    reader: &Actor,
    permission: &str,
) -> rutent_contracts::core_internal::ReadCheck {
    serde_json::from_value(json!({
        "request": request,
        "reader": reader,
        "reads": {"kind": "trail"},
        "permission": permission,
        "at": "2026-11-10T09:00:00Z",
    }))
    .unwrap()
}

/// The fixtures' regulator, `examiner-3` of `fin-regulator`.
fn regulator() -> Actor {
    actor(
        "fin-regulator",
        "examiner-3",
        Role::Regulator,
        ActorKind::Person,
    )
}

/// The register of the read grants' tests: the operator's login `op-login-2` listed in the operator's role for both
/// permissions, `op-login-g` for granting alone and `op-login-r` for revoking alone, all of the operator's party; the
/// regulator listed in the regulator's role; the customer's own login listed for its own party in the customer's role
/// alone, and holding nothing; and a provider's login listed, wrongly, in the operator's role and for both
/// permissions, though its party is no part of the operator's.
fn granting() -> Register {
    let customer = actor("cust-1", "cust-1-web", Role::Customer, ActorKind::Person);
    let revoker = actor("operator", "op-login-r", Role::Operator, ActorKind::Person);
    let outsider = actor("bank-co", "bank-ops", Role::Operator, ActorKind::Person);
    Register::default()
        .listing(&operator())
        .listing(&granter())
        .listing(&revoker)
        .listing(&regulator())
        .listing(&customer)
        .listing(&outsider)
        .and_holding(
            &operator(),
            &[Permission::GrantRead, Permission::RevokeRead],
        )
        .and_holding(&granter(), &[Permission::GrantRead])
        .and_holding(&revoker, &[Permission::RevokeRead])
        .and_holding(&outsider, &[Permission::GrantRead, Permission::RevokeRead])
}

/// An operator's login the register lists in the operator's role, of the operator's party, and for the permission to
/// grant a read, grants the regulator the register lists in the regulator's role a read of the trail: permitted and
/// recorded with its actor, so the regulator's read naming it is allowed and recorded. A login listed for the
/// permission to revoke revokes it, permitted and recorded, and the read is refused after. Granting and revoking are
/// each a permission of its own: a login listed for granting alone revokes nothing, and one listed for revoking alone
/// grants nothing.
// verifies: REQ-AUZ-7, REQ-AUD-12, REQ-AUZ-6
#[test]
fn listed_operator_grants_and_revokes_a_regulators_read() {
    use rutent_contracts::core_internal::ReadOutcome;
    use rutent_contracts::core_internal::calls::Reads as _;

    let revoker = actor("operator", "op-login-r", Role::Operator, ActorKind::Person);
    let mut policy = engine(Kept::world(), granting());

    // Revoking alone grants nothing.
    let by_revoker = read_change("req-g0", "perm-reg-1", &regulator(), &revoker);
    let decision = policy.grant_permission(&by_revoker).unwrap();
    assert_eq!(
        decision.outcome, NOT_PERMITTED,
        "revoking alone grants nothing"
    );
    assert!(policy.register().grants.is_empty());

    let grant = read_change("req-g1", "perm-reg-1", &regulator(), &operator());
    let decision = policy.grant_permission(&grant).unwrap();
    assert_eq!(
        decision.outcome,
        Answer::Permit {},
        "the listed operator grants"
    );
    let entry = policy.log().entry_of(&decision);
    assert_eq!(entry.who, operator(), "recorded with its actor");
    assert_eq!(policy.register().grants, std::slice::from_ref(&grant));

    let read = trail_read("req-r1", &regulator(), "perm-reg-1");
    let answer = policy.check_read(&read).unwrap();
    assert_eq!(
        answer.outcome,
        ReadOutcome::Allowed {},
        "the regulator reads"
    );
    assert_eq!(last_entry(&policy.log).who, regulator());

    // Granting alone revokes nothing.
    let by_granter = read_change("req-v0", "perm-reg-1", &regulator(), &granter());
    let decision = policy.revoke_permission(&by_granter).unwrap();
    assert_eq!(
        decision.outcome, NOT_PERMITTED,
        "granting alone revokes nothing"
    );
    assert!(policy.register().revoked.is_empty());
    assert_eq!(
        policy
            .check_read(&trail_read("req-r2", &regulator(), "perm-reg-1"))
            .unwrap()
            .outcome,
        ReadOutcome::Allowed {}
    );

    let revoke = read_change("req-v1", "perm-reg-1", &regulator(), &revoker);
    let decision = policy.revoke_permission(&revoke).unwrap();
    assert_eq!(
        decision.outcome,
        Answer::Permit {},
        "the listed revoker revokes"
    );
    assert_eq!(policy.log().entry_of(&decision).who, revoker);
    assert_eq!(
        policy
            .check_read(&trail_read("req-r3", &regulator(), "perm-reg-1"))
            .unwrap()
            .outcome,
        ReadOutcome::Refused {
            class: RefusalClass::NotPermitted
        },
        "the read is refused once revoked"
    );
    assert_eq!(
        policy.policy.1.ruled, 4,
        "the grant, two reads and the revocation"
    );
}

/// The probe of finding 1 of `M3`'s milestone review: a customer's own login, listed for its own party in the
/// customer's role and for nothing else, states the operator's role and grants itself a read of the trail as a
/// regulator, reads the trail by it, and revokes it. Each is refused, none reaching the deployment's policy, and each
/// refusal is recorded with the roles stated; the trail is read by no one. Stating the operator's party as well, it is
/// refused the same. The customer's login stating the operator's role revokes nothing the operator granted, and the
/// regulator still reads. A login the register lists, wrongly, in the operator's role and for both permissions, whose
/// party is no part of the operator's, grants and revokes nothing; so does the operator's own login in another role
/// it is listed in. And the listed operator's login grants nothing to a grantee stating a regulator's role the
/// register doesn't list it in, the customer's own login among them.
// verifies: REQ-AUZ-7, REQ-AUD-12, REQ-AUZ-6, THR-7
#[test]
fn customers_login_stating_the_operators_role_grants_reads_and_revokes_nothing() {
    use rutent_contracts::core_internal::ReadOutcome;
    use rutent_contracts::core_internal::calls::Reads as _;

    let mut policy = engine(Kept::world(), granting());
    let as_operator = actor("cust-1", "cust-1-web", Role::Operator, ActorKind::Person);
    let as_regulator = actor("cust-1", "cust-1-web", Role::Regulator, ActorKind::Person);
    let of_the_operator = actor("operator", "cust-1-web", Role::Operator, ActorKind::Person);

    for (n, by) in [as_operator.clone(), of_the_operator].iter().enumerate() {
        let probe = read_change(&format!("req-probe-{n}"), "perm-probe", &as_regulator, by);
        let decision = policy.grant_permission(&probe).unwrap();
        assert!(
            matches!(decision.outcome, Answer::Refuse { .. }),
            "{by:?} grants nothing: {decision:?}"
        );
        let entry = policy.log().entry_of(&decision);
        assert_eq!(entry.who, *by, "the refusal recorded with the role stated");
        assert!(matches!(entry.outcome, Outcome::Refused { .. }));
    }
    assert!(policy.register().grants.is_empty(), "nothing granted");

    let read = trail_read("req-probe-read", &as_regulator, "perm-probe");
    let answer = policy.check_read(&read).unwrap();
    assert!(
        matches!(answer.outcome, ReadOutcome::Refused { .. }),
        "the trail is read by no one"
    );
    assert!(matches!(
        last_entry(&policy.log).outcome,
        Outcome::Refused { .. }
    ));
    let revoke = read_change(
        "req-probe-revoke",
        "perm-probe",
        &as_regulator,
        &as_operator,
    );
    let decision = policy.revoke_permission(&revoke).unwrap();
    assert!(matches!(decision.outcome, Answer::Refuse { .. }));

    // The operator grants the regulator its read; the customer's login stating the operator's role can't revoke it.
    let grant = read_change("req-g1", "perm-reg-1", &regulator(), &operator());
    assert_eq!(
        policy.grant_permission(&grant).unwrap().outcome,
        Answer::Permit {}
    );
    let stolen = read_change("req-probe-steal", "perm-reg-1", &regulator(), &as_operator);
    let decision = policy.revoke_permission(&stolen).unwrap();
    assert!(
        matches!(decision.outcome, Answer::Refuse { .. }),
        "{decision:?}"
    );
    assert!(policy.register().revoked.is_empty(), "nothing revoked");
    assert_eq!(
        policy
            .check_read(&trail_read("req-r1", &regulator(), "perm-reg-1"))
            .unwrap()
            .outcome,
        ReadOutcome::Allowed {},
        "the regulator still reads"
    );

    // A login listed in the operator's role and for both permissions, of a party that is no part of the operator's.
    let outsider = actor("bank-co", "bank-ops", Role::Operator, ActorKind::Person);
    let decision = policy
        .grant_permission(&read_change(
            "req-out-g",
            "perm-out",
            &regulator(),
            &outsider,
        ))
        .unwrap();
    assert_eq!(decision.outcome, NOT_PERMITTED, "not the operator's");
    let decision = policy
        .revoke_permission(&read_change(
            "req-out-v",
            "perm-reg-1",
            &regulator(),
            &outsider,
        ))
        .unwrap();
    assert_eq!(decision.outcome, NOT_PERMITTED, "not the operator's");

    // In any role but the operator's, the operator's own login listed for both and in that role grants and revokes
    // nothing, even asked past the request's shape check, which holds a grantor to the operator's role too.
    let as_provider = actor("operator", "op-login-2", Role::Provider, ActorKind::Person);
    let mut policy_as = engine(Kept::world(), granting().listing(&as_provider));
    assert!(
        read_change("req-shape", "p", &regulator(), &operator())
            .check()
            .is_ok()
    );
    let mut change = read_change("req-as-provider", "perm-reg-1", &regulator(), &operator());
    change.by = as_provider;
    assert_eq!(
        policy_as.consult(Asked::GrantPermission(&change)),
        NOT_PERMITTED
    );
    assert_eq!(
        policy_as.consult(Asked::RevokePermission(&change)),
        NOT_PERMITTED
    );
    assert_eq!(policy_as.policy.1.ruled, 0);

    // The listed operator granting a grantee stating a regulator's role the register doesn't list it in.
    for (n, grantee) in [
        as_regulator.clone(),
        actor(
            "fin-regulator",
            "examiner-3",
            Role::Checker,
            ActorKind::Person,
        ),
    ]
    .iter()
    .enumerate()
    {
        let change = read_change(&format!("req-unlisted-{n}"), "perm-x", grantee, &operator());
        let decision = policy.grant_permission(&change).unwrap();
        assert_eq!(decision.outcome, NO_PARTY, "{grantee:?}");
    }
    assert_eq!(policy.register().grants, [grant]);
    assert_eq!(
        policy.policy.1.ruled, 2,
        "only the operator's grant and the regulator's read reached policy"
    );
}

/// Fix round 1 of `T-021-14`'s review, finding 1: a read naming a permission is held to the grantee's own login and
/// role, and only a regulator or a checker reads by one. The listed operator grants `examiner-3`, a regulator of
/// `fin-regulator`, the trail. Other logins of `fin-regulator`, each listed in its own role, read nothing by it: `fr-bot`
/// as a verifier, `fr-ops` in the operator's role, and `examiner-4`, another regulator; nor does `examiner-3` itself,
/// listed as a checker too, reading as one. Each refusal is recorded. Asked past the register of grants, the fixed
/// rule lets no role but a regulator's or a checker's read by a permission. `examiner-3` reads it as the regulator.
// verifies: REQ-AUZ-7, REQ-AUD-12
#[test]
fn read_by_a_permission_is_held_to_its_grantees_login_and_role() {
    use rutent_contracts::core_internal::ReadOutcome;
    use rutent_contracts::core_internal::calls::Reads as _;

    let bot = actor(
        "fin-regulator",
        "fr-bot",
        Role::Verifier,
        ActorKind::Component,
    );
    let ops = actor("fin-regulator", "fr-ops", Role::Operator, ActorKind::Person);
    let other = actor(
        "fin-regulator",
        "examiner-4",
        Role::Regulator,
        ActorKind::Person,
    );
    let as_checker = actor(
        "fin-regulator",
        "examiner-3",
        Role::Checker,
        ActorKind::Person,
    );
    let register = granting()
        .listing(&bot)
        .listing(&ops)
        .listing(&other)
        .listing(&as_checker);
    let mut policy = engine(Kept::world(), register);
    let grant = read_change("req-g1", "perm-reg-1", &regulator(), &operator());
    assert_eq!(
        policy.grant_permission(&grant).unwrap().outcome,
        Answer::Permit {}
    );
    for (n, reader) in [&bot, &ops, &other, &as_checker].into_iter().enumerate() {
        let read = trail_read(&format!("req-borrow-{n}"), reader, "perm-reg-1");
        assert_eq!(
            policy.check_read(&read).unwrap().outcome,
            ReadOutcome::Refused {
                class: RefusalClass::NotPermitted
            },
            "{reader:?} reads nothing by another login's grant"
        );
        assert_eq!(last_entry(&policy.log).who, *reader);
        assert!(matches!(
            last_entry(&policy.log).outcome,
            Outcome::Refused { .. }
        ));
    }
    for (n, reader) in [&bot, &ops].into_iter().enumerate() {
        let read = trail_read(&format!("req-past-{n}"), reader, "perm-reg-1");
        assert_eq!(
            policy.consult(Asked::CheckRead(&read)),
            NOT_PERMITTED,
            "{reader:?}: only a regulator or a checker reads by a permission"
        );
    }
    assert_eq!(
        policy
            .check_read(&trail_read("req-own", &regulator(), "perm-reg-1"))
            .unwrap()
            .outcome,
        ReadOutcome::Allowed {}
    );
    assert_eq!(
        policy.policy.1.ruled, 2,
        "only the grant and the grantee's own read reached policy"
    );
}

/// Fix round 1 of `T-021-14`'s review, finding 2: a read is never granted to the operator's own logins. The listed
/// operator's login, listed in the regulator's role too, grants itself the trail as a regulator, and grants another of
/// the operator's logins listed as a regulator the same: each refused, and recorded refused. A regulator outside the
/// operator is granted it.
// verifies: REQ-AUZ-7, REQ-AUD-10
#[test]
fn read_is_never_granted_to_the_operators_own_login() {
    use rutent_contracts::core_internal::calls::Reads as _;

    let itself = actor("operator", "op-login-2", Role::Regulator, ActorKind::Person);
    let colleague = actor("operator", "op-login-g", Role::Checker, ActorKind::Person);
    let mut policy = engine(
        Kept::world(),
        granting().listing(&itself).listing(&colleague),
    );
    for (n, grantee) in [&itself, &colleague].into_iter().enumerate() {
        let change = read_change(&format!("req-own-{n}"), "perm-own", grantee, &operator());
        let decision = policy.grant_permission(&change).unwrap();
        assert_eq!(decision.outcome, NOT_PERMITTED, "{grantee:?}");
        assert!(matches!(
            policy.log().entry_of(&decision).outcome,
            Outcome::Refused { .. }
        ));
    }
    assert!(policy.register().grants.is_empty(), "nothing granted");
    let grant = read_change("req-g1", "perm-reg-1", &regulator(), &operator());
    assert_eq!(
        policy.grant_permission(&grant).unwrap().outcome,
        Answer::Permit {}
    );
}

/// Fix round 1 of `T-021-14`'s review, finding 3: a revocation is held to the grant it names, its permission, grantee
/// and scope together. The operator grants `examiner-3` the trail, then revokes the same permission naming `ex-9` of
/// `other-reg`, and naming `examiner-3` over another scope: each refused, recorded refused, nothing revoked, and
/// `examiner-3` still reads. Revoking the grant it names revokes it.
// verifies: REQ-AUZ-7, REQ-AUD-1
#[test]
fn revocation_is_held_to_the_grant_it_names() {
    use rutent_contracts::core_internal::ReadOutcome;
    use rutent_contracts::core_internal::calls::Reads as _;

    let elsewhere = actor("other-reg", "ex-9", Role::Regulator, ActorKind::Person);
    let mut policy = engine(Kept::world(), granting().listing(&elsewhere));
    let grant = read_change("req-g1", "perm-reg-1", &regulator(), &operator());
    assert_eq!(
        policy.grant_permission(&grant).unwrap().outcome,
        Answer::Permit {}
    );
    let mut over_an_order = read_change("req-v-order", "perm-reg-1", &regulator(), &operator());
    over_an_order.scope =
        serde_json::from_value(json!({"kind": "order", "order": "ord-1"})).unwrap();
    for revoke in [
        read_change("req-v-other", "perm-reg-1", &elsewhere, &operator()),
        over_an_order,
    ] {
        let decision = policy.revoke_permission(&revoke).unwrap();
        assert_eq!(decision.outcome, NOT_PERMITTED, "{revoke:?}");
        assert!(matches!(
            policy.log().entry_of(&decision).outcome,
            Outcome::Refused { .. }
        ));
        assert!(policy.register().revoked.is_empty(), "nothing revoked");
        assert_eq!(
            policy
                .check_read(&trail_read(
                    &format!("req-r-{}", revoke.request.as_str()),
                    &regulator(),
                    "perm-reg-1"
                ))
                .unwrap()
                .outcome,
            ReadOutcome::Allowed {},
            "examiner-3 still reads"
        );
    }
    let revoke = read_change("req-v1", "perm-reg-1", &regulator(), &operator());
    assert_eq!(
        policy.revoke_permission(&revoke).unwrap().outcome,
        Answer::Permit {}
    );
    assert_eq!(
        policy
            .check_read(&trail_read("req-r-after", &regulator(), "perm-reg-1"))
            .unwrap()
            .outcome,
        ReadOutcome::Refused {
            class: RefusalClass::NotPermitted
        }
    );
}

/// The review's lows on `7865ed6b`: a grant never reuses a permission's id. The log counts an id revoked for good once
/// any revocation of it is permitted, and revokes every grant sharing it, so policy refuses a grant naming an id it
/// granted before, still in force or revoked. A second grant of `perm-reg-1`, to another regulator, is refused; the
/// same request again answers as the first did; after `perm-reg-1` is revoked, granting it again is refused and the
/// read stays refused. Each refusal is recorded. A grant under a fresh id is permitted, and its read allowed.
// verifies: REQ-AUZ-7, REQ-AUD-12
#[test]
fn grant_reusing_a_permission_id_is_refused() {
    use rutent_contracts::core_internal::ReadOutcome;
    use rutent_contracts::core_internal::calls::Reads as _;

    let elsewhere = actor("other-reg", "ex-9", Role::Regulator, ActorKind::Person);
    let mut policy = engine(Kept::world(), granting().listing(&elsewhere));
    let grant = read_change("req-g1", "perm-reg-1", &regulator(), &operator());
    assert_eq!(
        policy.grant_permission(&grant).unwrap().outcome,
        Answer::Permit {}
    );

    // In force: a second grant of the id, to another regulator, is refused.
    let shared = read_change("req-g2", "perm-reg-1", &elsewhere, &operator());
    let decision = policy.grant_permission(&shared).unwrap();
    assert_eq!(decision.outcome, NOT_PERMITTED, "an id in force");
    assert!(matches!(
        policy.log().entry_of(&decision).outcome,
        Outcome::Refused { .. }
    ));
    assert_eq!(policy.register().grants, std::slice::from_ref(&grant));
    assert_eq!(
        policy.grant_permission(&grant).unwrap().outcome,
        Answer::Permit {},
        "the same request again answers as the first did"
    );

    // Revoked: granting the id again is refused, and the read stays refused.
    let revoke = read_change("req-v1", "perm-reg-1", &regulator(), &operator());
    assert_eq!(
        policy.revoke_permission(&revoke).unwrap().outcome,
        Answer::Permit {}
    );
    let again = read_change("req-g3", "perm-reg-1", &regulator(), &operator());
    let decision = policy.grant_permission(&again).unwrap();
    assert_eq!(decision.outcome, NOT_PERMITTED, "a revoked id");
    assert!(matches!(
        policy.log().entry_of(&decision).outcome,
        Outcome::Refused { .. }
    ));
    let replayed = policy.grant_permission(&grant).unwrap();
    assert_eq!(
        replayed.outcome, NOT_PERMITTED,
        "nor the first request again"
    );
    assert_eq!(
        policy
            .check_read(&trail_read("req-r1", &regulator(), "perm-reg-1"))
            .unwrap()
            .outcome,
        ReadOutcome::Refused {
            class: RefusalClass::NotPermitted
        }
    );

    // A fresh id is granted, and read by.
    let fresh = read_change("req-g4", "perm-reg-2", &regulator(), &operator());
    assert_eq!(
        policy.grant_permission(&fresh).unwrap().outcome,
        Answer::Permit {}
    );
    assert_eq!(
        policy
            .check_read(&trail_read("req-r2", &regulator(), "perm-reg-2"))
            .unwrap()
            .outcome,
        ReadOutcome::Allowed {}
    );
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

        let register = Register::holding(&operator(), &[Permission::GrantRead])
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

/// A failed segment's provider is asked to reverse only by an operator's login holding the permission to ask it, or
/// by one of Core's own actors carrying the order on (`CarryOnward`, the coordinator's default for the contract's
/// round 2). The operator holding every other permission, Core without its listing, the customer, the provider itself,
/// a verifier, a regulator and a checker are refused before the deployment's policy is asked.
// verifies: REQ-ORD-3, REQ-AUZ-1
#[test]
fn reversal_is_asked_only_by_the_operator_or_cores_carrier() {
    let ask = |caller: &Actor| {
        let mut request: DecisionRequest = valid!("decision-request-to-mark-a-release-stale");
        request.request = id(&format!("req-reverse-{}", caller.acts.as_str()));
        request.caller = caller.clone();
        request.action = Action::AskReversal {
            order: id("ord-17"),
            segment: id("seg-2"),
            provider: id("bridge-pay"),
        };
        request
    };
    let mut every_other: Vec<Permission> = every_permission(&operator())
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    every_other.push(Permission::ActForUser);
    every_other.push(Permission::CarryOnward);

    let mut policy = engine(
        Kept::with_a_decision_record(),
        Register::holding(&operator(), &[Permission::AskReversal])
            .and_holding(&core(), &[Permission::CarryOnward]),
    );
    for caller in [operator(), core()] {
        let decision = policy.decide(&ask(&caller)).unwrap();
        assert_eq!(decision.outcome, Answer::Permit {}, "{caller:?}");
        assert_eq!(
            policy.log().entry_of(&decision).who,
            caller,
            "recorded with its actor"
        );
    }

    let mut policy = engine(
        Kept::with_a_decision_record(),
        Register::holding(&operator(), &every_other),
    );
    let refused = [
        operator(),
        core(),
        actor("cust-1", "cust-1", Role::Customer, ActorKind::Person),
        actor(
            "bridge-pay",
            "bridge-pay",
            Role::Provider,
            ActorKind::Component,
        ),
        actor("kyc-co", "kyc-co", Role::Verifier, ActorKind::Component),
        actor(
            "fin-regulator",
            "examiner-3",
            Role::Regulator,
            ActorKind::Person,
        ),
        actor("audit-co", "checker-1", Role::Checker, ActorKind::Person),
    ];
    for caller in &refused {
        assert_eq!(
            policy.decide(&ask(caller)).unwrap().outcome,
            NOT_PERMITTED,
            "{caller:?}"
        );
    }
    assert_eq!(
        policy.policy.1.ruled, 0,
        "none reached the deployment's policy"
    );
}

/// A late status asked of a segment's provider, as `caller` asks it.
fn status_asked(caller: &Actor) -> DecisionRequest {
    let mut request: DecisionRequest = valid!("decision-request-to-mark-a-release-stale");
    request.request = id(&format!("req-status-{}", caller.acts.as_str()));
    request.caller = caller.clone();
    request.action = Action::AskStatus {
        order: id("ord-17"),
        segment: id("seg-2"),
        provider: id("bridge-pay"),
        transaction: id("tx-status-1"),
    };
    request
}

/// Core's own actor carrying the order on asks a segment's provider where it stands, and so does an operator's
/// login the register lists for it, each recorded with its actor; Core's other actors and the operator without the
/// permission are not permitted by it (`T-021-09`, as `ask_reversal` is asked).
// verifies: REQ-ORD-1
#[test]
fn late_status_asked_by_core_for_a_segment_is_permitted() {
    let mut policy = engine(
        Kept::with_a_decision_record(),
        Register::holding(&operator(), &[Permission::AskStatus])
            .and_holding(&core(), &[Permission::CarryOnward]),
    );
    for caller in [core(), operator()] {
        let decision = policy.decide(&status_asked(&caller)).unwrap();
        assert_eq!(decision.outcome, Answer::Permit {}, "{caller:?}");
        assert_eq!(
            policy.log().entry_of(&decision).who,
            caller,
            "recorded with its actor"
        );
    }
    assert_eq!(
        policy.policy.1.ruled, 2,
        "each reached the deployment's policy"
    );

    // Unlisted: Core's login without its listing, and the operator's login without the permission to ask it.
    let mut policy = engine(
        Kept::with_a_decision_record(),
        Register::holding(
            &operator(),
            &[Permission::AskReversal, Permission::CarryOnward],
        ),
    );
    for caller in [operator(), core()] {
        assert_eq!(
            policy.decide(&status_asked(&caller)).unwrap().outcome,
            NOT_PERMITTED,
            "{caller:?}"
        );
    }
}

/// Nobody else asks a late status: the customer, the provider itself, a verifier, a regulator and a checker are
/// refused before the deployment's policy is asked, whatever the register lists for them, and the operator holding
/// every other permission is too.
// verifies: REQ-AUZ-1
#[test]
fn late_status_asked_by_a_customer_or_a_provider_is_refused() {
    let mut every_other: Vec<Permission> = every_permission(&operator())
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    every_other.extend([
        Permission::ActForUser,
        Permission::CarryOnward,
        Permission::AskReversal,
        Permission::DeclineOutcome,
    ]);
    let customer = actor("cust-1", "cust-1", Role::Customer, ActorKind::Person);
    let provider = actor(
        "bridge-pay",
        "bridge-pay",
        Role::Provider,
        ActorKind::Component,
    );
    let mut register = Register::holding(&operator(), &every_other)
        .and_holding(&customer, &every_other)
        .and_holding(&provider, &every_other);
    let others = [
        actor("kyc-co", "kyc-co", Role::Verifier, ActorKind::Component),
        actor(
            "fin-regulator",
            "examiner-3",
            Role::Regulator,
            ActorKind::Person,
        ),
        actor("audit-co", "checker-1", Role::Checker, ActorKind::Person),
    ];
    for other in &others {
        register = register.and_holding(other, &[Permission::AskStatus, Permission::CarryOnward]);
    }
    let mut policy = engine(Kept::with_a_decision_record(), register);
    for caller in [operator(), customer, provider].iter().chain(&others) {
        assert_eq!(
            policy.decide(&status_asked(caller)).unwrap().outcome,
            NOT_PERMITTED,
            "{caller:?}"
        );
    }
    assert_eq!(
        policy.policy.1.ruled, 0,
        "none reached the deployment's policy"
    );
}

/// The showing of fault the accepting tests' log holds: fx-desk's failed result of `p1-s2`, under `tx-f2`.
fn the_showing() -> rutent_contracts::core_internal::ShowingNamed {
    serde_json::from_value(json!({
        "transaction": "tx-f2",
        "commitment": {"algorithm": "sha-256", "value": "aa".repeat(32)},
    }))
    .unwrap()
}

/// When the log received that showing.
const SHOWING_RECEIVED: &str = "2026-11-06T15:04:08Z";

/// The customer's acceptance of the showing, asked by `caller` in `session` at `at`.
fn acceptance(caller: &Actor, session: &str, at: &str) -> DecisionRequest {
    let mut request: DecisionRequest =
        valid!("decision-request-to-accept-a-fault-in-the-customers-session");
    request.request = id(&format!("req-accept-{}-{session}", caller.acts.as_str()));
    request.caller = caller.clone();
    request.at = Timestamp::new(at).unwrap();
    request.action = Action::AcceptFault {
        order: id("ord-1"),
        segment: id("p1-s2"),
        showing: the_showing(),
        session: id(session),
    };
    request
}

/// An engine over the world, whose order `ord-1` is `cust-1`'s and holds fx-desk's showing of `p1-s2`, by Core's clock
/// at `now`, with a session open for each of `cust-1` (`sess-1`), `cust-2` (`sess-2`) and the operator (`sess-op`),
/// each until 09:10.
fn accepting(now: &str) -> Engine<Kept, Permits, Register> {
    let register = Register::default()
        .with_order("ord-1", "cust-1", "us-bank")
        .showing("ord-1", "p1-s2", the_showing(), SHOWING_RECEIVED);
    let mut policy = engine(Kept::world(), register);
    policy.set_clock(crate::session::Stopped::at(&Timestamp::new(now).unwrap()));
    for (session, holder) in [
        ("sess-1", "cust-1"),
        ("sess-2", "cust-2"),
        ("sess-op", "operator"),
    ] {
        policy.register.sessions.push(crate::session::OpenSession {
            session: id(session),
            account: id("acct-1"),
            holder: id(holder),
            expires_at: Timestamp::new("2026-11-10T09:10:00Z").unwrap(),
        });
    }
    policy
}

fn customer(party: &str) -> Actor {
    actor(party, party, Role::Customer, ActorKind::Person)
}

/// The order's requester accepts a showing of its fault in its open session, and policy permits it: the decision is
/// recorded with who accepted, and the log admits it. An effect of it is refused, since the acceptance is decided and
/// never carried out (`reversal-carrier-rests-on-its-acceptance`).
// verifies: REQ-ORD-11, REQ-AUZ-1
#[test]
fn the_requester_accepts_a_fault_in_its_open_session() {
    let mut policy = accepting("2026-11-10T09:00:00Z");
    let request = acceptance(&customer("cust-1"), "sess-1", "2026-11-10T09:00:00Z");
    let decision = policy.decide(&request).unwrap();
    assert_eq!(decision.outcome, Answer::Permit {});
    assert_eq!(policy.log().entry_of(&decision).who, request.caller);
    assert_eq!(policy.policy.1.ruled, 1, "the deployment's policy decided");
    let permit = decision.permit().expect("a permit");
    assert!(
        matches!(
            policy.log.effect(&request, permit),
            Err(AppendFailure::Refused(_))
        ),
        "the acceptance has no effect"
    );
}

/// Nobody but the order's requester, in the customer's role, in its own open session, naming a showing the log holds
/// from before the acceptance, accepts a fault: each of another party, the operator's login, every role but the
/// customer's, a session that ended (by Core's clock, or before the acceptance was decided), another party's session,
/// one never opened and a decision made before the showing was received is refused and recorded so.
// verifies: REQ-ORD-11, REQ-AUZ-1
#[test]
fn only_the_requester_in_its_open_session_after_the_showing_accepts_a_fault() {
    let by = |actor: Actor, session: &str, at: &str| (actor, session.to_owned(), at.to_owned());
    let own = customer("cust-1");
    let cases: Vec<(&str, &str, (Actor, String, String))> = vec![
        (
            "another party",
            "2026-11-10T09:00:00Z",
            by(customer("cust-2"), "sess-2", "2026-11-10T09:00:00Z"),
        ),
        (
            "the operator's login",
            "2026-11-10T09:00:00Z",
            by(operator(), "sess-op", "2026-11-10T09:00:00Z"),
        ),
        (
            "a provider in the requester's party",
            "2026-11-10T09:00:00Z",
            by(
                actor("cust-1", "cust-1", Role::Provider, ActorKind::Component),
                "sess-1",
                "2026-11-10T09:00:00Z",
            ),
        ),
        (
            "Core in the requester's party",
            "2026-11-10T09:00:00Z",
            by(
                actor("cust-1", "cust-1", Role::Core, ActorKind::Component),
                "sess-1",
                "2026-11-10T09:00:00Z",
            ),
        ),
        (
            "a verifier in the requester's party",
            "2026-11-10T09:00:00Z",
            by(
                actor("cust-1", "cust-1", Role::Verifier, ActorKind::Component),
                "sess-1",
                "2026-11-10T09:00:00Z",
            ),
        ),
        (
            "a regulator in the requester's party",
            "2026-11-10T09:00:00Z",
            by(
                actor("cust-1", "cust-1", Role::Regulator, ActorKind::Person),
                "sess-1",
                "2026-11-10T09:00:00Z",
            ),
        ),
        (
            "a checker in the requester's party",
            "2026-11-10T09:00:00Z",
            by(
                actor("cust-1", "cust-1", Role::Checker, ActorKind::Person),
                "sess-1",
                "2026-11-10T09:00:00Z",
            ),
        ),
        (
            "a session that ended by Core's clock",
            "2026-11-10T09:20:00Z",
            by(own.clone(), "sess-1", "2026-11-10T09:00:00Z"),
        ),
        (
            "an acceptance decided after its session ended",
            "2026-11-10T09:05:00Z",
            by(own.clone(), "sess-1", "2026-11-10T09:11:00Z"),
        ),
        (
            "another party's session",
            "2026-11-10T09:00:00Z",
            by(own.clone(), "sess-2", "2026-11-10T09:00:00Z"),
        ),
        (
            "a session never opened",
            "2026-11-10T09:00:00Z",
            by(own.clone(), "sess-9", "2026-11-10T09:00:00Z"),
        ),
        (
            "a decision made before the showing was received",
            "2026-11-10T09:00:00Z",
            by(own.clone(), "sess-1", "2026-11-06T15:00:00Z"),
        ),
    ];
    for (name, now, (caller, session, at)) in cases {
        let mut policy = accepting(now);
        let request = acceptance(&caller, &session, &at);
        let decision = policy.decide(&request).unwrap();
        assert_eq!(decision.outcome, NOT_PERMITTED, "{name}");
        assert!(decision.permit().is_none(), "{name}");
        assert_eq!(policy.log().entry_of(&decision).who, caller, "{name}");
    }

    // A showing the log holds none of, or of another transaction, is no showing: the acceptance names what the log
    // holds exactly.
    for other in [
        json!({"transaction": "tx-f9", "commitment": {"algorithm": "sha-256", "value": "aa".repeat(32)}}),
        json!({"transaction": "tx-f2", "commitment": {"algorithm": "sha-256", "value": "bb".repeat(32)}}),
    ] {
        let mut policy = accepting("2026-11-10T09:00:00Z");
        let mut request = acceptance(&own, "sess-1", "2026-11-10T09:00:00Z");
        if let Action::AcceptFault { showing, .. } = &mut request.action {
            *showing = serde_json::from_value(other).unwrap();
        }
        let decision = policy.decide(&request).unwrap();
        assert_eq!(decision.outcome, NOT_PERMITTED, "{request:?}");
    }
}

/// A kept statement's erasure, as `caller` asks it: the fixtures' request, which names the commitment and a reason.
fn erasure_asked(caller: &Actor) -> DecisionRequest {
    let mut request: DecisionRequest = valid!("decision-request-to-erase-a-kept-statement");
    request.request = id(&format!("req-erase-{}", caller.acts.as_str()));
    request.caller = caller.clone();
    request
}

/// An operator's login the register lists for the permission to erase is permitted to erase a kept statement, and the
/// decision's entry holds the request whole, so it names the commitment erased and the actor who asked (`T-021-10`,
/// `DEC-18`); the permit it gives is one the log takes an `erase_statement` effect on.
// verifies: AC-36, DEC-18
#[test]
fn operator_with_the_erase_permission_is_permitted_to_erase_a_statement() {
    let asked = erasure_asked(&operator());
    let Action::EraseStatement { commitment, .. } = &asked.action else {
        panic!("an erasure");
    };
    let mut policy = engine(
        Kept::with_a_decision_record(),
        Register::holding(&operator(), &[Permission::EraseStatement]),
    );
    let decision = policy.decide(&asked).unwrap();
    assert_eq!(decision.outcome, Answer::Permit {});
    let entry = policy.log().entry_of(&decision).clone();
    assert_eq!(entry.who, operator(), "recorded with its actor");
    let What::Decide { request, .. } = &entry.what else {
        panic!("a decide entry");
    };
    assert!(
        matches!(&request.action, Action::EraseStatement { commitment: c, .. } if c == commitment),
        "the decision names the commitment erased"
    );
    assert_eq!(
        policy.policy.1.ruled, 1,
        "it reached the deployment's policy"
    );
    assert!(
        policy
            .log
            .effect(&asked, decision.permit().unwrap())
            .is_ok(),
        "the erasure's effect rests on the permit"
    );
}

/// A caller without the permission to erase is refused: the operator's login the register doesn't list for it, even
/// holding every other permission, and another login of its party; nobody in another role, whatever the register
/// lists for them, before the deployment's policy is asked; and the permission to erase allows no other action.
// verifies: REQ-AUZ-1
#[test]
fn caller_without_the_erase_permission_is_refused() {
    let mut every_other: Vec<Permission> = every_permission(&operator())
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    every_other.extend([
        Permission::ActForUser,
        Permission::CarryOnward,
        Permission::AskReversal,
        Permission::AskStatus,
        Permission::DecideStuck,
        Permission::MoveStuck,
        Permission::ConfirmOutcome,
        Permission::DeclineOutcome,
    ]);
    let others = [
        actor("cust-1", "cust-1", Role::Customer, ActorKind::Person),
        actor(
            "bridge-pay",
            "bridge-pay",
            Role::Provider,
            ActorKind::Component,
        ),
        actor("kyc-co", "kyc-co", Role::Verifier, ActorKind::Component),
        actor(
            "fin-regulator",
            "examiner-3",
            Role::Regulator,
            ActorKind::Person,
        ),
        actor("audit-co", "checker-1", Role::Checker, ActorKind::Person),
        core(),
    ];
    let mut register = Register::holding(&operator(), &every_other);
    for other in &others {
        register = register.and_holding(other, &[Permission::EraseStatement]);
    }
    let mut policy = engine(Kept::with_a_decision_record(), register);
    let other_login = actor("operator", "op-login-9", Role::Operator, ActorKind::Person);
    for caller in [operator(), other_login].iter().chain(&others) {
        let decision = policy.decide(&erasure_asked(caller)).unwrap();
        assert_eq!(decision.outcome, NOT_PERMITTED, "{caller:?}");
        assert_eq!(
            policy.log.effect(&erasure_asked(caller), forged(&decision)),
            no_permitting_decision(),
            "{caller:?}"
        );
    }
    assert_eq!(
        policy.policy.1.ruled, 0,
        "none reached the deployment's policy"
    );

    // The permission to erase allows nothing else.
    let mut policy = engine(
        Kept::with_a_decision_record(),
        Register::holding(&operator(), &[Permission::EraseStatement]),
    );
    for (permission, request) in every_permission(&operator()) {
        assert_eq!(
            policy.decide(&request).unwrap().outcome,
            NOT_PERMITTED,
            "{permission:?}"
        );
    }
}

/// Rolling a release's staging back, core-internal's `roll_back` (`T-020-39`, `T-021-12`), is a permission of its own,
/// `Permission::Change(RollBack)`, held login by login: an operator's login holding every other permission there is,
/// promotion and the erase permission among them, is refused, and so is Core's own actor even holding all of them,
/// the fixed rule refusing before the deployment's policy is asked, and the log takes no effect on it. The login
/// holding the one permission is permitted, recorded with its actor and the action, and the log admits the effect.
/// Holding it permits no promotion or any other change (`REQ-AUZ-1`, `REQ-AUZ-6`, ADR-0014).
// verifies: REQ-AUZ-6
#[test]
fn roll_back_without_its_permission_is_refused() {
    let rolled_back = |caller: &Actor| {
        let mut request: DecisionRequest = valid!("decision-request-to-roll-back-a-staged-release");
        request.request = id(&format!("req-roll-back-{}", caller.acts.as_str()));
        request.caller = caller.clone();
        request
    };
    let own = Permission::Change(CatalogueChange::RollBack);
    let mut every_other: Vec<Permission> = every_permission(&operator())
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| *p != own)
        .collect();
    every_other.push(Permission::EraseStatement);
    every_other.push(Permission::CarryOnward);
    let mut policy = engine(
        Kept::with_a_decision_record(),
        Register::holding(&operator(), &every_other).and_holding(&core(), &every_other),
    );
    for caller in [operator(), core()] {
        let asked = rolled_back(&caller);
        let decision = policy.decide(&asked).unwrap();
        assert_eq!(decision.outcome, NOT_PERMITTED, "{caller:?}");
        assert!(decision.permit().is_none());
        let entry = policy.log().entry_of(&decision).clone();
        assert_eq!(entry.who, caller, "recorded with its actor");
        assert!(
            matches!(&entry.what, What::Decide { request, .. } if request == &asked),
            "recorded with its action"
        );
        assert_eq!(
            policy.log.effect(&asked, forged(&decision)),
            no_permitting_decision(),
            "{caller:?}"
        );
    }
    assert_eq!(
        policy.policy.1.ruled, 0,
        "none reached the deployment's policy"
    );

    // The permission, held alone by the operator's login, permits the roll back, recorded with its actor, and the
    // log admits the effect on that permit; Core's own actor holding it is still refused (an operator's act only).
    let mut policy = engine(
        Kept::with_a_decision_record(),
        Register::holding(&operator(), &[own]).and_holding(&core(), &[own]),
    );
    let asked = rolled_back(&operator());
    let decision = policy.decide(&asked).unwrap();
    assert_eq!(decision.outcome, Answer::Permit {});
    let entry = policy.log().entry_of(&decision).clone();
    assert_eq!(entry.who, operator());
    assert!(matches!(&entry.what, What::Decide { request, .. } if request == &asked));
    assert!(
        policy
            .log
            .effect(&asked, decision.permit().unwrap())
            .is_ok()
    );
    let by_core = rolled_back(&core());
    assert_eq!(
        policy.decide(&by_core).unwrap().outcome,
        NOT_PERMITTED,
        "Core's own actors don't roll a staging back"
    );
    // It permits nothing else: not the promotion it stands beside.
    let (_, promoted) = every_permission(&operator())
        .into_iter()
        .find(|(p, _)| *p == Permission::Change(CatalogueChange::Promote))
        .unwrap();
    assert_eq!(policy.decide(&promoted).unwrap().outcome, NOT_PERMITTED);
}

/// A kept statement's erasure that overrides its citations, as `caller` asks it: the plain erasure with
/// core-internal's `overrides_citations` set (`T-020-39`).
fn overriding_erasure_asked(caller: &Actor) -> DecisionRequest {
    let mut request = erasure_asked(caller);
    request.request = id(&format!("req-erase-over-{}", caller.acts.as_str()));
    let Action::EraseStatement {
        overrides_citations,
        ..
    } = &mut request.action
    else {
        panic!("an erasure");
    };
    *overrides_citations = true;
    request
}

/// An erasure that overrides the statement's citations is permitted only to an operator's login the register lists
/// for both the permission to erase and the permission of its own, `Permission::EraseOverridingCitations`: the
/// decision's entry holds the request whole, so it names the statement and the override, and the permit it gives is
/// one the log takes the effect on. Every other caller is refused: the same login without the override permission,
/// another login of its party, Core's own actors and every other role, even listed for both, before the deployment's
/// policy is asked, and the log takes no effect on a refusal. The pair allows the plain erasure and the override and
/// no other action (`REQ-AUD-13`, `REQ-AUZ-6`, `REQ-AUZ-1`).
// verifies: REQ-AUD-13, REQ-AUZ-6
#[test]
fn erasure_overriding_citations_is_permitted_only_with_its_own_permission() {
    let own = Permission::EraseOverridingCitations;
    let pair = [Permission::EraseStatement, own];
    let asked = overriding_erasure_asked(&operator());
    let mut policy = engine(
        Kept::with_a_decision_record(),
        Register::holding(&operator(), &pair),
    );
    let decision = policy.decide(&asked).unwrap();
    assert_eq!(decision.outcome, Answer::Permit {});
    let entry = policy.log().entry_of(&decision).clone();
    assert_eq!(entry.who, operator(), "recorded with its actor");
    let What::Decide { request, .. } = &entry.what else {
        panic!("a decide entry");
    };
    let (
        Action::EraseStatement { commitment, .. },
        Action::EraseStatement {
            commitment: asked_commitment,
            overrides_citations: true,
            ..
        },
    ) = (&request.action, &asked.action)
    else {
        panic!("the decision holds the erasure, the override set");
    };
    assert_eq!(
        commitment, asked_commitment,
        "the decision names the statement"
    );
    assert_eq!(
        policy.policy.1.ruled, 1,
        "it reached the deployment's policy"
    );
    assert!(
        policy
            .log
            .effect(&asked, decision.permit().unwrap())
            .is_ok(),
        "the override's effect rests on the permit"
    );

    // The pair permits the plain erasure too, and no other action.
    let plain = erasure_asked(&operator());
    assert_eq!(policy.decide(&plain).unwrap().outcome, Answer::Permit {});
    for (permission, request) in every_permission(&operator()) {
        if permission != own {
            assert_eq!(
                policy.decide(&request).unwrap().outcome,
                NOT_PERMITTED,
                "{permission:?}"
            );
        }
    }

    // Without the override permission, and in any other role, even listed for both, an override is refused before
    // the policy is asked.
    let mut every_but: Vec<Permission> = every_permission(&operator())
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| *p != own)
        .collect();
    every_but.extend([Permission::EraseStatement, Permission::CarryOnward]);
    let others = [
        actor("operator", "op-login-7", Role::Core, ActorKind::Component),
        actor("operator", "op-login-7", Role::Customer, ActorKind::Person),
        actor(
            "bridge-pay",
            "bridge-pay",
            Role::Provider,
            ActorKind::Component,
        ),
        actor(
            "operator",
            "op-login-7",
            Role::Checker,
            ActorKind::Component,
        ),
    ];
    let mut register = Register::holding(&operator(), &every_but);
    for other in &others {
        register = register.and_holding(other, &pair);
    }
    let other_login = actor("operator", "op-login-9", Role::Operator, ActorKind::Person);
    let mut policy = engine(Kept::with_a_decision_record(), register);
    for caller in [operator(), other_login].iter().chain(&others) {
        let asked = overriding_erasure_asked(caller);
        let decision = policy.decide(&asked).unwrap();
        assert_eq!(decision.outcome, NOT_PERMITTED, "{caller:?}");
        assert_eq!(
            policy.log.effect(&asked, forged(&decision)),
            no_permitting_decision(),
            "{caller:?}"
        );
    }
    assert_eq!(
        policy.policy.1.ruled, 0,
        "none reached the deployment's policy"
    );
}

/// An operator's login holding the permission to erase alone is refused an erasure that overrides citations, and
/// permitted the same erasure without the override: the default, an erasure waits while a later entry cites its
/// statement, holds (`REQ-AUZ-1`, `REQ-AUD-13`).
// verifies: REQ-AUZ-1
#[test]
fn operator_with_the_erase_permission_alone_is_refused_an_override() {
    let mut every_other: Vec<Permission> = every_permission(&operator())
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| *p != Permission::EraseOverridingCitations)
        .collect();
    every_other.push(Permission::EraseStatement);
    for held in [vec![Permission::EraseStatement], every_other] {
        let mut policy = engine(
            Kept::with_a_decision_record(),
            Register::holding(&operator(), &held),
        );
        let asked = overriding_erasure_asked(&operator());
        let decision = policy.decide(&asked).unwrap();
        assert_eq!(decision.outcome, NOT_PERMITTED, "{} held", held.len());
        assert!(decision.permit().is_none());
        assert_eq!(
            policy.log.effect(&asked, forged(&decision)),
            no_permitting_decision()
        );
        assert_eq!(
            policy.policy.1.ruled, 0,
            "it never reached the deployment's policy"
        );
        // Without the override, the plain permission still answers it.
        let plain = erasure_asked(&operator());
        assert_eq!(policy.decide(&plain).unwrap().outcome, Answer::Permit {});
    }
}

/// An operator's login holding the override permission alone, without the permission to erase, is refused an erasure
/// that overrides citations, and the plain erasure too, before the deployment's policy is asked: the override is a
/// permission to exempt an erasure from the citation check, never one that erases by itself, even holding every other
/// permission there is (`REQ-AUZ-1`, `REQ-AUZ-6`, `REQ-AUD-13`).
// verifies: REQ-AUZ-1
#[test]
fn operator_with_the_override_permission_alone_is_refused() {
    let own = Permission::EraseOverridingCitations;
    let mut every_but_erase: Vec<Permission> = every_permission(&operator())
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    every_but_erase.push(Permission::CarryOnward);
    for held in [vec![own], every_but_erase] {
        assert!(!held.contains(&Permission::EraseStatement));
        let mut policy = engine(
            Kept::with_a_decision_record(),
            Register::holding(&operator(), &held),
        );
        for asked in [
            overriding_erasure_asked(&operator()),
            erasure_asked(&operator()),
        ] {
            let decision = policy.decide(&asked).unwrap();
            assert_eq!(decision.outcome, NOT_PERMITTED, "{} held", held.len());
            assert_eq!(
                policy.log.effect(&asked, forged(&decision)),
                no_permitting_decision()
            );
        }
        assert_eq!(
            policy.policy.1.ruled, 0,
            "it never reached the deployment's policy"
        );
    }
}

/// An operator's setting (core-internal's `set_setting`, `T-020-45`) is a permission of its own for each setting
/// (`Permission::Setting`, `T-021-15`), held login by login: an operator's login holding every other permission there
/// is, every other setting's, `Change(SetSetting)` and the erase permission among them, is refused, and so is Core's
/// own actor even holding all of them, the fixed rule refusing before the deployment's policy is asked, and the log
/// takes no effect on it. The login holding the one permission is permitted, recorded with its actor and the action,
/// the setting and its value held whole, and the log admits the effect; Core's actor holding it is still refused,
/// and a request that names no setting is no request at all (`REQ-AUZ-1`, `REQ-AUZ-6`, `REQ-AUD-8`).
// verifies: REQ-AUZ-6
#[test]
fn operator_setting_without_its_permission_is_refused() {
    for (kind, _) in every_setting_asked(&operator()) {
        let own = Permission::Setting(kind);
        let mut every_other: Vec<Permission> = every_permission(&operator())
            .into_iter()
            .map(|(p, _)| p)
            .filter(|p| *p != own)
            .collect();
        every_other.push(Permission::Change(CatalogueChange::SetSetting));
        every_other.push(Permission::EraseStatement);
        every_other.push(Permission::CarryOnward);
        let setting_of = |caller: &Actor| {
            every_setting_asked(caller)
                .into_iter()
                .find(|(k, _)| *k == kind)
                .unwrap()
                .1
        };
        let mut policy = engine(
            Kept::with_a_decision_record(),
            Register::holding(&operator(), &every_other).and_holding(&core(), &every_other),
        );
        for caller in [operator(), core()] {
            let asked = setting_of(&caller);
            let decision = policy.decide(&asked).unwrap();
            assert_eq!(decision.outcome, NOT_PERMITTED, "{kind:?} by {caller:?}");
            assert!(decision.permit().is_none());
            let entry = policy.log().entry_of(&decision).clone();
            assert_eq!(entry.who, caller, "recorded with its actor");
            assert!(
                matches!(&entry.what, What::Decide { request, .. } if request == &asked),
                "recorded with its action, the setting and its value whole"
            );
            assert_eq!(
                policy.log.effect(&asked, forged(&decision)),
                no_permitting_decision(),
                "{kind:?} by {caller:?}"
            );
        }
        assert_eq!(
            policy.policy.1.ruled, 0,
            "none reached the deployment's policy"
        );

        // Held alone by the operator's login it permits; Core's own actor holding it is still refused.
        let mut policy = engine(
            Kept::with_a_decision_record(),
            Register::holding(&operator(), &[own]).and_holding(&core(), &[own]),
        );
        let asked = setting_of(&operator());
        let decision = policy.decide(&asked).unwrap();
        assert_eq!(decision.outcome, Answer::Permit {}, "{kind:?}");
        let entry = policy.log().entry_of(&decision).clone();
        assert_eq!(entry.who, operator());
        assert!(matches!(&entry.what, What::Decide { request, .. } if request == &asked));
        assert!(
            policy
                .log
                .effect(&asked, decision.permit().unwrap())
                .is_ok()
        );
        assert_eq!(
            policy.decide(&setting_of(&core())).unwrap().outcome,
            NOT_PERMITTED,
            "Core's own actors set no setting"
        );
    }

    // `Change(SetSetting)` names no setting, so it permits none, held with nothing else; and a request that names no
    // setting fails its check, whatever is held.
    let mut every: Vec<Permission> = every_permission(&operator())
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    let mut policy = engine(
        Kept::with_a_decision_record(),
        Register::holding(
            &operator(),
            &[Permission::Change(CatalogueChange::SetSetting)],
        ),
    );
    for (kind, request) in every_setting_asked(&operator()) {
        assert_eq!(
            policy.decide(&request).unwrap().outcome,
            NOT_PERMITTED,
            "{kind:?}"
        );
    }
    every.push(Permission::Change(CatalogueChange::SetSetting));
    let mut policy = engine(
        Kept::with_a_decision_record(),
        Register::holding(&operator(), &every),
    );
    let mut nameless = every_setting_asked(&operator()).remove(0).1;
    let Action::ChangeCatalogue { setting, .. } = &mut nameless.action else {
        panic!("a catalogue change");
    };
    *setting = None;
    assert!(
        policy.decide(&nameless).is_err(),
        "a setting change naming no setting fails its check, so nothing is decided"
    );
    assert_eq!(
        policy.policy.1.ruled, 0,
        "it never reached the deployment's policy"
    );
}

/// The lifecycle actions on a release, `accept`, `withdraw`, `revoke`, `declare_exploited` and `mark_stale`
/// (core-internal's five changes of their own, `LIFECYCLE_ACTIONS`, `T-020-45`), are each a permission of its own,
/// `Permission::Change`, held login by login: an operator's login holding every other permission there is, the
/// other four lifecycle actions, every setting and promotion among them, is refused, and so is Core's own actor
/// holding all of them (a version declared exploited is never carried onward), the fixed rule refusing before the
/// deployment's policy is asked, the attempt recorded with its actor and the version it names, and the log taking no
/// effect on it. The login holding the one permission is permitted, and the log admits the effect on its permit
/// (`REQ-AUZ-1`, `REQ-MDL-20`).
// verifies: REQ-AUZ-1, REQ-MDL-20
#[test]
fn lifecycle_action_without_its_permission_is_refused() {
    for (name, change) in rutent_contracts::core_internal::LIFECYCLE_ACTIONS {
        let own = Permission::Change(change);
        let asked_by = |caller: &Actor| {
            every_permission(caller)
                .into_iter()
                .find(|(p, _)| *p == own)
                .unwrap()
                .1
        };
        let mut every_other: Vec<Permission> = every_permission(&operator())
            .into_iter()
            .map(|(p, _)| p)
            .filter(|p| *p != own)
            .collect();
        every_other.push(Permission::Change(CatalogueChange::SetSetting));
        every_other.push(Permission::CarryOnward);
        let mut policy = engine(
            Kept::with_a_decision_record(),
            Register::holding(&operator(), &every_other).and_holding(&core(), &every_other),
        );
        for caller in [operator(), core()] {
            let asked = asked_by(&caller);
            let Action::ChangeCatalogue { version, .. } = &asked.action else {
                panic!("a catalogue change");
            };
            assert!(version.is_some(), "{name} names its version");
            let decision = policy.decide(&asked).unwrap();
            assert_eq!(decision.outcome, NOT_PERMITTED, "{name} by {caller:?}");
            assert!(decision.permit().is_none());
            let entry = policy.log().entry_of(&decision).clone();
            assert_eq!(entry.who, caller, "recorded with its actor");
            assert!(
                matches!(&entry.what, What::Decide { request, .. } if request == &asked),
                "recorded with the action and the version"
            );
            assert_eq!(
                policy.log.effect(&asked, forged(&decision)),
                no_permitting_decision(),
                "{name} by {caller:?}"
            );
        }
        assert_eq!(
            policy.policy.1.ruled, 0,
            "none reached the deployment's policy"
        );

        let mut policy = engine(
            Kept::with_a_decision_record(),
            Register::holding(&operator(), &[own]).and_holding(&core(), &[own]),
        );
        let asked = asked_by(&operator());
        let decision = policy.decide(&asked).unwrap();
        assert_eq!(decision.outcome, Answer::Permit {}, "{name}");
        assert_eq!(policy.log().entry_of(&decision).who, operator());
        assert!(
            policy
                .log
                .effect(&asked, decision.permit().unwrap())
                .is_ok()
        );
        assert_eq!(
            policy.decide(&asked_by(&core())).unwrap().outcome,
            NOT_PERMITTED,
            "{name} is never carried onward by Core's actors"
        );
    }
}

/// A publisher never marks its own version stale (`REQ-MDL-5`): the provider that published the version, asking in its
/// own role, holding the permission in the register, is refused by the fixed rule before the register or the
/// deployment's policy is asked, and so is its login stating the operator's role, which the register of actors binds
/// to the operator's logins alone; no effect rests on either. The operator's login holding the permission marks it
/// (`REQ-MDL-5`, `REQ-AUZ-6`).
// verifies: REQ-MDL-5
#[test]
fn publisher_is_refused_marking_its_own_version_stale() {
    let own = Permission::Change(CatalogueChange::MarkStale);
    let publisher = actor(
        "bridge-pay",
        "bridge-pay",
        Role::Provider,
        ActorKind::Component,
    );
    let claimed = actor(
        "bridge-pay",
        "bridge-pay",
        Role::Operator,
        ActorKind::Component,
    );
    let stale = |caller: &Actor| {
        let mut request: DecisionRequest = valid!("decision-request-to-mark-a-release-stale");
        request.request = id(&format!("req-stale-{:?}", caller.role).to_lowercase());
        request.caller = caller.clone();
        let Action::ChangeCatalogue { provider, .. } = &request.action else {
            panic!("a catalogue change");
        };
        assert_eq!(
            provider.as_str(),
            "bridge-pay",
            "the publisher's own version"
        );
        request
    };
    let mut policy = engine(
        Kept::with_a_decision_record(),
        Register::holding(&publisher, &[own])
            .and_holding(&claimed, &[own])
            .and_holding(&operator(), &[own])
            .listing(&publisher)
            .listing(&operator()),
    );
    let asked = stale(&publisher);
    let decision = policy.decide(&asked).unwrap();
    assert_eq!(decision.outcome, NOT_PERMITTED);
    assert_eq!(policy.log().entry_of(&decision).who.role, Role::Provider);
    assert_eq!(
        policy.log.effect(&asked, forged(&decision)),
        no_permitting_decision()
    );
    assert!(
        policy.sources.asked.is_empty(),
        "a provider holds no permission"
    );
    assert_eq!(policy.policy.1.ruled, 0);

    // Its login stating the operator's role: the register of actors says that login is not the operator's.
    let asked = stale(&claimed);
    let decision = policy.decide(&asked).unwrap();
    assert!(
        matches!(decision.outcome, Answer::Refuse { .. }),
        "a stated operator's role: {decision:?}"
    );
    assert!(decision.permit().is_none());
    assert_eq!(
        policy.log.effect(&asked, forged(&decision)),
        no_permitting_decision()
    );
    assert_eq!(policy.policy.1.ruled, 0);

    // The operator's login holding the permission marks it.
    let asked = stale(&operator());
    let decision = policy.decide(&asked).unwrap();
    assert_eq!(decision.outcome, Answer::Permit {});
    assert!(
        policy
            .log
            .effect(&asked, decision.permit().unwrap())
            .is_ok()
    );
}

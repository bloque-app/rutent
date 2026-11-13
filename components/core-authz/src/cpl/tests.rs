//! `T-021-07`'s tests: a question about a person asked only by an identified party, bounded at it, on the person's own
//! consent or the order's latest shortlist; the set asked about an order bounded across its recalculations, past the
//! bound no route left.
//!
//! The log these tests append to holds every entry to the contract's admission, as `CMP-CORE-AUDIT`'s log must, where
//! a test sets it to, so a question the log itself would refuse is seen refused there; otherwise it takes what policy
//! answers, so policy's own refusal is seen recorded.

use super::*;
use crate::engine::Source;
use crate::funds::ReadFailure;
use crate::parties::Scope;
use rutent_contracts::core_internal::calls::DecisionEntry;
use rutent_contracts::core_internal::{
    Actor, ActorKind, Authority, Charge, Count, DeploymentOperator, Entry, Outcome, PersonOutcome,
    Receipt, RefusalClass, Role, RoutingRound, Through, TimeSource, VerifiedEntry, VerifiedKind,
    When, Work, admit_entry,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::router_api::{Id, Timestamp};
use serde_json::json;

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn digest(byte: &str) -> Digest {
    serde_json::from_value(json!({"algorithm": "sha-256", "value": byte.repeat(32)})).unwrap()
}

fn at() -> Timestamp {
    Timestamp::new("2026-11-10T09:00:00Z").unwrap()
}

const ORDER: &str = "ord-17";

/// The order's customer's agent, which asks about the order's people, as rounds asks in the order's caller's name.
fn agent() -> Actor {
    Actor {
        party: id("acme-treasury"),
        acts: id("agent-7"),
        role: Role::Customer,
        kind: ActorKind::Agent,
    }
}

/// Core's orchestration, as the contract's fixtures ask.
fn core() -> Actor {
    Actor {
        party: id("rutent"),
        acts: id("core-orch"),
        role: Role::Core,
        kind: ActorKind::Component,
    }
}

/// A verified entry of Core's, timed by its clock.
fn entry(who: &Actor, what: What, outcome: Outcome) -> Entry {
    Entry::Verified(VerifiedEntry {
        kind: VerifiedKind::Verified,
        who: who.clone(),
        what,
        when: When {
            at: at(),
            source: TimeSource::CoreClock,
        },
        outcome,
    })
}

/// A shortlist round's record of `order`, under `record`, naming the providers it shortlisted and the verifiers
/// their tuples need.
fn shortlist(order: &str, record: &str, providers: &[&str], verifiers: &[&str]) -> Entry {
    entry(
        &core(),
        What::DecisionRecord {
            decision_record: id(&format!("dr-{record}")),
            order: id(order),
            round: RoutingRound::Shortlist,
            record: digest(record),
            previous: None,
            shortlisted: Some(providers.iter().map(|p| id(p)).collect()),
            verifiers: verifiers.iter().map(|v| id(v)).collect(),
        },
        Outcome::Observed {},
    )
}

/// A charge of `count` questions about people, of `order`, to `caller`'s party, counted within its bound.
fn charged(request: &str, caller: &Actor, order: &str, work: Work, count: u64) -> Entry {
    entry(
        caller,
        What::Charge {
            charge: Charge {
                request: id(request),
                party: caller.party.clone(),
                caller: caller.clone(),
                work,
                count: Count::new(count).unwrap(),
                order: Some(id(order)),
                at: at(),
            },
            counted_at: Some(at()),
        },
        Outcome::Permitted {},
    )
}

/// A question by `asker` about `person-ref-8` of [`ORDER`], on `authority`, asking `ask` after `asked_before`.
fn question(
    request: &str,
    asker: &Actor,
    authority: Authority,
    ask: &[&str],
    asked_before: &[&str],
) -> PersonQuestion {
    PersonQuestion {
        request: id(request),
        asker: asker.clone(),
        person: id("person-ref-8"),
        order: id(ORDER),
        authority,
        ask: ask.iter().map(|p| id(p)).collect(),
        asked_before: asked_before.iter().map(|p| id(p)).collect(),
        at: at(),
    }
}

/// The person's own consent, by its digest.
fn consent(byte: &str) -> Authority {
    Authority::Person {
        consent: digest(byte),
    }
}

/// A shortlisted route's authority: the shortlist `record`, its providers and the verifiers its tuples need.
fn route(record: &str, providers: &[&str], verifiers: &[&str]) -> Authority {
    Authority::ShortlistedRoute {
        shortlist: digest(record),
        providers: providers.iter().map(|p| id(p)).collect(),
        verifiers: verifiers.iter().map(|v| id(v)).collect(),
    }
}

/// The log policy appends to: where `admits`, every entry held to the contract's admission over what it holds, as
/// the deployment's log is; otherwise every entry policy answers taken as it comes. Its read fails where it is set to.
#[derive(Default)]
struct Log {
    entries: Vec<(Receipt, Entry)>,
    admits: bool,
    unreadable: bool,
}

fn nth_receipt(index: usize) -> Receipt {
    serde_json::from_value(json!({
        "log": "log-main",
        "index": index,
        "entry": {"algorithm": "sha-256", "value": format!("{:064x}", index + 1)},
    }))
    .unwrap()
}

impl Log {
    /// A log that holds every entry to the contract's admission, holding `history` before the test begins.
    fn admitting(history: Vec<Entry>) -> Self {
        let mut log = Log {
            admits: true,
            ..Log::default()
        };
        for e in history {
            log.hold(e);
        }
        log
    }

    /// A log that takes what policy answers, holding `history`.
    fn taking(history: Vec<Entry>) -> Self {
        Log {
            admits: false,
            ..Log::admitting(history)
        }
    }

    fn hold(&mut self, entry: Entry) {
        let receipt = nth_receipt(self.entries.len());
        self.entries.push((receipt, entry));
    }

    /// The outcome of the last entry the log took, and how many it holds.
    fn last(&self) -> (Outcome, usize) {
        match self.entries.last() {
            Some((_, Entry::Verified(entry))) => (entry.outcome, self.entries.len()),
            _ => panic!("a verified entry"),
        }
    }
}

impl PolicyLog for Log {
    fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
        let entry = Entry::Verified(entry.entry().clone());
        entry.check().map_err(AppendFailure::Refused)?;
        if self.admits {
            let operator = DeploymentOperator {
                maker: id("operator"),
                party: id("operator"),
            };
            admit_entry(&self.entries, &entry, Through::Policy, &operator)
                .map_err(AppendFailure::Refused)?;
        }
        self.hold(entry);
        Ok(self.entries.last().unwrap().0.clone())
    }
}

impl Receipted for Log {
    fn receipted(&mut self) -> Result<Vec<(Receipt, Entry)>, ReadFailure> {
        if self.unreadable {
            return Err(ReadFailure::Unavailable);
        }
        Ok(self.entries.clone())
    }
}

/// The deployment's policy: it needs nothing and permits whatever the fixed rules let through.
struct Permits;

impl Policy for Permits {
    fn needs(&mut self, _: Asked<'_>) -> Reply<Vec<Need>> {
        Reply::Given(Vec::new())
    }

    fn rule(&mut self, _: Asked<'_>, _: &[Input]) -> Reply<Ruling> {
        Reply::Given(Ruling::Permit)
    }
}

/// The registers these tests set: who each login acts for, each agent's scope, and each consent a person gave.
#[derive(Default)]
struct Register {
    acts_for: Vec<(Id, Id, ActorKind)>,
    scopes: Vec<(Id, Id, Scope)>,
    /// Each consent: the person, its digest, and the party it lets ask.
    consents: Vec<(Id, Digest, Id)>,
    /// The record of consents answers without the word.
    consents_silent: bool,
    /// The record of consents can't be reached.
    consents_unreachable: bool,
    /// What the rules asked of the record of consents.
    asked: Vec<Need>,
    /// Each order's requester.
    requesters: Vec<(Id, Id)>,
    /// Each order's parties: its requester, beneficiary and payer.
    order_parties: Vec<(Id, Id)>,
    /// Each login's permissions, by its party.
    permissions: Vec<(Id, Id, crate::roles::Permission)>,
}

impl Register {
    /// The agent and Core listed, the agent scoped to ask about people, and the person's consent `51` given to the
    /// agent's party.
    fn usual() -> Self {
        let mut register = Register::default();
        for a in [agent(), core()] {
            register.acts_for.push((a.acts, a.party, a.kind));
        }
        register
            .scopes
            .push((agent().party, agent().acts, Scope::AskAboutPerson));
        register
            .consents
            .push((id("person-ref-8"), digest("51"), agent().party));
        // The order is the agent's party's, and the person asked about is one of its parties.
        register.requesters.push((id(ORDER), agent().party));
        register.order_parties.push((id(ORDER), agent().party));
        register.order_parties.push((id(ORDER), id("person-ref-8")));
        // Core carries the order's authority onward.
        register.permissions.push((
            core().party,
            core().acts,
            crate::roles::Permission::CarryOnward,
        ));
        register
    }
}

impl Sources for Register {
    fn supply(&mut self, need: &Need) -> Reply<crate::engine::Said> {
        use crate::engine::Said;
        let said = |yes: bool| Reply::Given(if yes { Said::Yes } else { Said::No });
        match &need.from {
            Source::ActsFor { acts, party, kind } => {
                if self.acts_for.iter().any(|(a, _, _)| a == acts) {
                    said(
                        self.acts_for
                            .contains(&(acts.clone(), party.clone(), *kind)),
                    )
                } else {
                    Reply::Missing
                }
            }
            Source::AgentScope { party, scope } => said(self.scopes.contains(&(
                party.clone(),
                need.input.clone(),
                *scope,
            ))),
            Source::Consent { person, consent } => {
                self.asked.push(need.clone());
                if self.consents_unreachable {
                    Reply::Unreachable
                } else if self.consents_silent {
                    Reply::Missing
                } else {
                    said(self.consents.contains(&(
                        person.clone(),
                        consent.clone(),
                        need.input.clone(),
                    )))
                }
            }
            Source::Requester(order) => said(
                self.requesters
                    .contains(&(order.clone(), need.input.clone())),
            ),
            Source::OrderParties(order) => said(
                self.order_parties
                    .contains(&(order.clone(), need.input.clone())),
            ),
            Source::Permissions { party, permission } => said(self.permissions.contains(&(
                party.clone(),
                need.input.clone(),
                *permission,
            ))),
            _ => Reply::Missing,
        }
    }
}

type Tested = Engine<Log, Permits, Register>;

fn engine(log: Log) -> Tested {
    Engine::with_policy(log, Permits, Register::usual())
}

fn refused(class: RefusalClass) -> PersonOutcome {
    PersonOutcome::Refused { class }
}

/// The agent's party charged for `count` questions about people of the order.
fn charged_to_the_agent(count: u64) -> Entry {
    charged("chg-people", &agent(), ORDER, Work::PersonQuestion, count)
}

/// A question about a person resting on no authority from that person, nor on a shortlisted route of the order, is
/// refused; so is one by an asker policy can't identify at an accountable party, or outside its party's scope, and
/// one its party wasn't charged for, so no question escapes the party's bound (`REQ-CPL-2`, `REQ-AUZ-8`).
// verifies: REQ-CPL-2
#[test]
fn question_about_a_person_with_no_authority_is_refused() {
    let ask = |e: &mut Tested, request: &str, asker: &Actor, authority: Authority| {
        e.ask_about_person(&question(request, asker, authority, &["kyc-check"], &[]))
            .map(|a| a.outcome)
    };

    // The person's own consent, given to the asker's party, and the question charged: allowed.
    let mut e = engine(Log::admitting(vec![charged_to_the_agent(1)]));
    assert_eq!(
        ask(&mut e, "req-1", &agent(), consent("51")),
        Ok(PersonOutcome::Allowed {})
    );
    assert_eq!(e.log.last().0, Outcome::Permitted {});
    assert_eq!(
        e.sources.asked,
        [Need {
            input: agent().party,
            from: Source::Consent {
                person: id("person-ref-8"),
                consent: digest("51"),
            },
        }],
        "the consent is held to the asker's party"
    );

    // A consent the person never gave, one of another person's, or one given to another party: refused, and
    // recorded so.
    let not_permitted = Ok(refused(RefusalClass::NotPermitted));
    let mut e = engine(Log::admitting(vec![charged_to_the_agent(4)]));
    assert_eq!(ask(&mut e, "req-2", &agent(), consent("99")), not_permitted);
    assert_eq!(
        e.log.last().0,
        Outcome::Refused {
            class: RefusalClass::NotPermitted
        }
    );
    e.sources.consents = vec![(id("person-ref-9"), digest("51"), agent().party)];
    assert_eq!(ask(&mut e, "req-3", &agent(), consent("51")), not_permitted);
    e.sources.consents = vec![(id("person-ref-8"), digest("51"), id("another-party"))];
    assert_eq!(ask(&mut e, "req-4", &agent(), consent("51")), not_permitted);
    // A record of consents that says nothing refuses; one out of reach is not known: neither asks anyone.
    e.sources.consents_silent = true;
    assert_eq!(
        ask(&mut e, "req-5", &agent(), consent("51")),
        Ok(refused(RefusalClass::MissingInput))
    );
    e.sources.consents_unreachable = true;
    assert!(matches!(
        ask(&mut e, "req-6", &agent(), consent("51")),
        Ok(PersonOutcome::NotKnown { .. })
    ));

    // A shortlisted route the log holds no shortlist of for the order is no authority. The deployment's log refuses
    // to record even the refusal (its rows cite every question, a contract item), so the call answers nothing and
    // no one is asked; a log that takes what policy answers records the refusal.
    let none = question(
        "req-7",
        &agent(),
        route("52", &["bank-co"], &[]),
        &["bank-co"],
        &[],
    );
    let mut e = engine(Log::admitting(vec![charged_to_the_agent(1)]));
    let answer = e.ask_about_person(&none);
    assert!(
        matches!(answer, Err(AppendFailure::Refused(ref v)) if v.0.contains("shortlist")),
        "{answer:?}"
    );
    assert_eq!(e.log.entries.len(), 1, "nothing recorded");
    let mut e = engine(Log::taking(vec![
        charged_to_the_agent(1),
        shortlist("ord-other", "52", &["bank-co"], &[]),
    ]));
    assert_eq!(e.ask_about_person(&none).map(|a| a.outcome), not_permitted);

    // An asker policy can't place at an accountable party, one stating a party it doesn't act for, and an agent its
    // party didn't scope to ask about people: refused before any consent is asked.
    let mut e = engine(Log::admitting(vec![charged_to_the_agent(3)]));
    let stranger = Actor {
        acts: id("agent-unlisted"),
        ..agent()
    };
    assert_eq!(
        ask(&mut e, "req-8", &stranger, consent("51")),
        Ok(refused(RefusalClass::MissingInput))
    );
    let elsewhere = Actor {
        party: id("another-party"),
        ..agent()
    };
    assert_eq!(
        ask(&mut e, "req-9", &elsewhere, consent("51")),
        Ok(refused(RefusalClass::NoAccountableParty))
    );
    e.sources.scopes.clear();
    assert_eq!(
        ask(&mut e, "req-10", &agent(), consent("51")),
        not_permitted
    );

    // Bounded: a question its party wasn't charged for, for this order, as questions about people, is over the
    // party's bound. Another party's charge, another order's and a charge of other work cover nothing; a charge of one
    // covers one question, and the same question asked again counts once.
    let over = Ok(refused(RefusalClass::OverBound));
    let mut e = engine(Log::admitting(vec![
        charged("chg-1", &core(), ORDER, Work::PersonQuestion, 5),
        charged("chg-2", &agent(), "ord-other", Work::PersonQuestion, 5),
        charged("chg-3", &agent(), ORDER, Work::FanOut, 5),
    ]));
    assert_eq!(ask(&mut e, "req-11", &agent(), consent("51")), over);
    let mut e = engine(Log::admitting(vec![charged_to_the_agent(1)]));
    assert_eq!(
        ask(&mut e, "req-12", &agent(), consent("51")),
        Ok(PersonOutcome::Allowed {})
    );
    assert_eq!(
        e.ask_about_person(&question(
            "req-12",
            &agent(),
            consent("51"),
            &["kyc-check"],
            &[]
        ))
        .map(|a| a.outcome),
        Ok(PersonOutcome::Allowed {}),
        "the same question again"
    );
    assert_eq!(
        e.ask_about_person(&question(
            "req-13",
            &agent(),
            consent("51"),
            &["kyc-check"],
            &["kyc-check"]
        ))
        .map(|a| a.outcome),
        over
    );
}

/// A log policy can't read asks no one: the shortlist, the charges and the set asked are inputs the decision lacks.
// verifies: REQ-CPL-2, REQ-AUZ-5
#[test]
fn unread_log_asks_no_one_about_a_person() {
    let mut log = Log::taking(vec![charged_to_the_agent(1)]);
    log.unreadable = true;
    let mut e = engine(log);
    assert_eq!(
        e.ask_about_person(&question(
            "req-1",
            &agent(),
            consent("51"),
            &["kyc-check"],
            &[]
        ))
        .map(|a| a.outcome),
        Ok(refused(RefusalClass::MissingInput))
    );
}

/// The set of providers and verifiers asked about one order, across its recalculations, is the log's: past its bound
/// the question isn't asked and the answer is past the bound, which rounds read as no route offered and recalculation
/// as no route left (`REQ-ORD-28`). Asking one already asked again doesn't grow it, and a question naming fewer asked
/// before than the log holds can't undercount it.
// verifies: REQ-CPL-5
#[test]
fn providers_asked_past_the_bound_leave_no_route() {
    let verifiers: Vec<String> = (1..=ASKED_BOUND + 1).map(|n| format!("kyc-{n}")).collect();
    let names: Vec<&str> = verifiers.iter().map(String::as_str).collect();
    let history = || {
        vec![
            charged_to_the_agent(40),
            shortlist(ORDER, "52", &["bank-co"], &names),
        ]
    };
    let mut e = engine(Log::admitting(history()));
    let on_the_route = route("52", &["bank-co"], &names);
    let bound = usize::try_from(ASKED_BOUND).unwrap();
    for (n, verifier) in names.iter().take(bound).enumerate() {
        let answer = e
            .ask_about_person(&question(
                &format!("req-{n}"),
                &agent(),
                on_the_route.clone(),
                &[verifier],
                &names[..n],
            ))
            .unwrap();
        assert_eq!(answer.outcome, PersonOutcome::Allowed {}, "{verifier}");
    }
    // One more is past the bound, recorded as refused over it, and the next asked set stays what it was.
    let past = e
        .ask_about_person(&question(
            "req-past",
            &agent(),
            on_the_route.clone(),
            &[names[bound]],
            &names[..bound],
        ))
        .unwrap();
    assert_eq!(
        past.outcome,
        PersonOutcome::PastBound {
            bound: SafeInteger::try_from(ASKED_BOUND).unwrap()
        }
    );
    assert_eq!(
        e.log.last().0,
        Outcome::Refused {
            class: RefusalClass::OverBound
        }
    );
    let again = e
        .ask_about_person(&question(
            "req-again",
            &agent(),
            on_the_route.clone(),
            &[names[0]],
            &names[..bound],
        ))
        .unwrap();
    assert_eq!(again.outcome, PersonOutcome::Allowed {}, "one asked before");

    // A question claiming fewer asked before than the log holds: the log refuses it, and one that takes what policy
    // answers sees the set the log holds, past the bound all the same.
    let mut log = Log::taking(history());
    for (n, verifier) in names.iter().take(bound).enumerate() {
        log.hold(entry(
            &agent(),
            What::AskAboutPerson {
                question: question(
                    &format!("req-{n}"),
                    &agent(),
                    on_the_route.clone(),
                    &[verifier],
                    &names[..n],
                ),
            },
            Outcome::Permitted {},
        ));
    }
    let mut e = engine(log);
    let stale = e
        .ask_about_person(&question(
            "req-stale",
            &agent(),
            on_the_route,
            &[names[bound]],
            &[],
        ))
        .unwrap();
    assert!(
        matches!(stale.outcome, PersonOutcome::PastBound { .. }),
        "{:?}",
        stale.outcome
    );
}

/// Only the providers and verifiers the order's latest shortlist names are asked on its route: a question citing a
/// shortlist a recalculation replaced, asking a provider it dropped, is refused, and so are one naming a provider or a
/// verifier its shortlist doesn't, and one citing another order's (`REQ-CPL-6`).
// verifies: REQ-CPL-6
#[test]
fn provider_off_the_shortlist_is_never_asked_about_eligibility() {
    let history = || {
        vec![
            charged_to_the_agent(10),
            shortlist(ORDER, "52", &["bank-co", "aml-screen"], &["kyc-verify"]),
            // A recalculation's shortlist: `aml-screen` and the verifier its tuples needed are off the route now.
            shortlist(ORDER, "53", &["bank-co"], &[]),
            shortlist("ord-other", "54", &["aml-screen"], &[]),
        ]
    };
    let ask = |e: &mut Tested, request: &str, authority: Authority, ask: &[&str]| {
        e.ask_about_person(&question(request, &agent(), authority, ask, &[]))
            .map(|a| a.outcome)
    };
    let not_permitted = Ok(refused(RefusalClass::NotPermitted));

    // The replaced shortlist: the log admits its citation, and policy refuses it, on the record.
    let mut e = engine(Log::admitting(history()));
    let before = e.log.entries.len();
    assert_eq!(
        ask(
            &mut e,
            "req-2",
            route("52", &["bank-co", "aml-screen"], &["kyc-verify"]),
            &["aml-screen"]
        ),
        not_permitted
    );
    assert_eq!(e.log.entries.len(), before + 1, "the refusal is recorded");
    assert_eq!(
        ask(
            &mut e,
            "req-3",
            route("52", &["bank-co", "aml-screen"], &["kyc-verify"]),
            &["kyc-verify"]
        ),
        not_permitted
    );
    // The replaced shortlist, cited for a provider still on the route: refused all the same, since the authority is
    // the latest shortlist's alone.
    assert_eq!(
        ask(
            &mut e,
            "req-3b",
            route("52", &["bank-co"], &[]),
            &["bank-co"]
        ),
        not_permitted
    );
    // The latest shortlist's own provider is asked.
    assert_eq!(
        ask(
            &mut e,
            "req-1",
            route("53", &["bank-co"], &[]),
            &["bank-co"]
        ),
        Ok(PersonOutcome::Allowed {})
    );

    // A provider, or a verifier, the latest shortlist doesn't name, and another order's shortlist: the log refuses
    // the question outright, and a log that takes what policy answers records it refused.
    // An authority naming a provider or a verifier off the shortlist is none, even where it asks only one on it.
    let off = [
        route("53", &["bank-co", "aml-screen"], &[]),
        route("53", &["bank-co"], &["kyc-verify"]),
        route("53", &["bank-co", "aml-screen"], &[]),
        route("53", &["bank-co"], &["kyc-verify"]),
        route("54", &["aml-screen"], &[]),
    ];
    let asks = [
        &["aml-screen"][..],
        &["kyc-verify"],
        &["bank-co"],
        &["bank-co"],
        &["aml-screen"],
    ];
    for (authority, asks) in off.iter().zip(asks) {
        let mut e = engine(Log::admitting(history()));
        assert!(
            matches!(
                ask(&mut e, "req-4", authority.clone(), asks),
                Err(AppendFailure::Refused(_))
            ),
            "{authority:?}"
        );
        let mut e = engine(Log::taking(history()));
        assert_eq!(
            ask(&mut e, "req-4", authority.clone(), asks),
            not_permitted,
            "{authority:?}"
        );
    }
}

/// On a shortlisted route the question is the order's own: its asker acts for the order's requester, or is Core's
/// actor the register lists as carrying the order's authority onward, and the person asked about is one of the
/// order's parties. An unrelated party citing the order's latest shortlist, charged and listed, is refused, and so is
/// a question about a person outside the order (`REQ-CPL-2`, `THR-45`).
// verifies: REQ-CPL-2, THR-45
#[test]
fn question_on_a_shortlisted_route_is_the_orders_own_about_one_of_its_parties() {
    let stranger = Actor {
        party: id("other-co"),
        acts: id("other-co-desk"),
        role: Role::Provider,
        kind: ActorKind::Person,
    };
    let history = || {
        vec![
            charged_to_the_agent(3),
            charged("chg-stranger", &stranger, ORDER, Work::PersonQuestion, 3),
            charged("chg-core", &core(), ORDER, Work::PersonQuestion, 3),
            shortlist(ORDER, "53", &["bank-co"], &[]),
        ]
    };
    let on_route = || route("53", &["bank-co"], &[]);
    let ask = |e: &mut Tested, request: &str, asker: &Actor, person: &str| {
        let mut q = question(request, asker, on_route(), &["bank-co"], &[]);
        q.person = id(person);
        e.ask_about_person(&q).map(|a| a.outcome)
    };
    let not_permitted = Ok(refused(RefusalClass::NotPermitted));
    let fresh = |history: Vec<Entry>| {
        let mut e = engine(Log::taking(history));
        e.sources
            .acts_for
            .push((stranger.acts.clone(), stranger.party.clone(), stranger.kind));
        e
    };

    // An unrelated party, listed and charged for the order, citing its latest shortlist: refused.
    let mut e = fresh(history());
    assert_eq!(
        ask(&mut e, "req-1", &stranger, "person-ref-8"),
        not_permitted
    );
    // A person outside the order, asked about by the order's own requester: refused.
    assert_eq!(
        ask(&mut e, "req-2", &agent(), "person-ref-77"),
        not_permitted
    );
    // Core's actor the register doesn't list as carrying the order's authority onward: refused.
    e.sources.permissions.clear();
    assert_eq!(ask(&mut e, "req-3", &core(), "person-ref-8"), not_permitted);

    // The order's requester's agent, and Core's listed actor, about one of the order's parties: allowed.
    let mut e = fresh(history());
    assert_eq!(
        ask(&mut e, "req-4", &agent(), "person-ref-8"),
        Ok(PersonOutcome::Allowed {})
    );
    let mut e = fresh(history());
    assert_eq!(
        ask(&mut e, "req-5", &core(), "person-ref-8"),
        Ok(PersonOutcome::Allowed {})
    );
}

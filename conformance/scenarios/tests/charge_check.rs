//! Core's own charge check (`T-020-14`): each settled segment's charge held to the formula that bound it by Core's
//! segments, a charge above it raised to the operator through Core's own channel and recorded, the terms a provider
//! stated binding from the segment's start and kept only as the evidence store holds them, and each segment's charge
//! checked once. Over the assembled Core against the simulated providers; the log admits everything it takes. The
//! terms are the providers' own, handed back in their answers to Core's dry runs and taken by Core (`T-020-56`): no
//! scenario hands Core terms.

mod routing_support;

use ed25519_dalek::Verifier as _;
use routing_support::*;
use rutent_conformance::sim::{Act, Behaviour, Role, Spec};
use rutent_contracts::core_internal::{
    Action, Attribution, Difference, Encoded, Entry, KeptStatement, Outcome, Raise, Receipt, Salt,
    VerifiedEntry, What,
};
use rutent_contracts::provider_role::PublicKey;
use rutent_contracts::router_api::Id;
use rutent_core_orch::intake::{Status, Told};
use rutent_core_orch::notice::{About, Inbox, Receiver, Taken, Verifier};
use rutent_core_orch::quote::Checked;
use rutent_core_orch::recalc::Recalculation;
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, KeptThrough, Staged};
use rutent_scenarios::harness::{Given, Scenario};
use rutent_scenarios::kit;
use serde_json::json;

const CHARGING: &str = "sim-reserve-charging";

fn alone(provider: &str, accepts: &'static str) -> (Core, Id) {
    let given = Given {
        providers: vec![Spec::of(Role::Reserve, provider)],
        staging: Staged::AtOnce,
        deadline: std::time::Duration::from_secs(2),
        delivery: rutent_scenarios::assembly::machine::Delivery::Once,
    };
    let (mut core, _) = Scenario::new("charge-check", accepts, given).core(1, &mut Reference);
    let session = core.open_session(&kit::customer()).expect("a session");
    (core, session)
}

/// An order of `provider` for the whole amount, quoted, accepted and run: its id.
fn ran(core: &mut Core, session: &Id, provider: &str, name: &str) -> Id {
    let (order, told) = quoted(
        core,
        request_at(provider, name, AMOUNT, session, "lowest_total_cost"),
    );
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("quoted");
    };
    core.accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    core.run(&order).expect("the order runs");
    order
}

struct Ed25519;

impl Verifier for Ed25519 {
    fn verifies(&self, key: &PublicKey, input: &[u8], signature: &[u8; 64]) -> bool {
        ed25519_dalek::VerifyingKey::from_bytes(&key.bytes()).is_ok_and(|k| {
            k.verify(input, &ed25519_dalek::Signature::from_bytes(signature))
                .is_ok()
        })
    }
}

/// What the operator's inbox holds once every notice sent to it is taken: the raises, as it would take them.
fn inbox_raises(core: &Core) -> Vec<Raise> {
    let operator = Receiver {
        party: kit::operator().party,
        role: rutent_contracts::core_internal::Role::Operator,
    };
    let mut inbox = Inbox::new(operator, kit::id("rutent-scenarios"), core.notice_key());
    for sealed in core.operator_notices() {
        let taken = inbox.take(&Ed25519, &sealed).expect("the inbox takes it");
        assert!(matches!(taken, Taken::Event | Taken::Repeat));
    }
    inbox
        .events()
        .filter_map(|about| match about {
            About::Raise { raise } => Some(raise.clone()),
            _ => None,
        })
        .collect()
}

/// The raise entries the log holds, with how each was recorded.
fn raise_entries(core: &Core) -> Vec<(Raise, Outcome)> {
    core.log()
        .entries()
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(VerifiedEntry {
                what: What::Raise { raise },
                outcome,
                ..
            }) => Some((raise.clone(), *outcome)),
            _ => None,
        })
        .collect()
}

// verifies: AC-14, REQ-QTE-5
#[test]
fn charge_above_the_formulas_maximum_is_raised_to_the_operator_by_cores_own_segments() {
    let (mut core, session) = alone(CHARGING, "AC-14");
    // Nothing but `run` is called: the segments check what they settle.
    let order = ran(&mut core, &session, CHARGING, "req-charged");

    // The provider quoted no charge and charged 7 on each movement, which only the movement tells.
    let raised = inbox_raises(&core);
    assert_eq!(
        raised.len(),
        2,
        "both segments' charges reached the operator"
    );
    for raise in &raised {
        let Difference::Charge {
            charged,
            maximum,
            provider,
            terms,
            ..
        } = &raise.difference
        else {
            panic!("a charge difference");
        };
        assert_eq!(charged.as_str(), "7");
        assert_eq!(maximum.as_str(), "0");
        assert_eq!(provider.as_str(), CHARGING);
        assert!(terms.is_none(), "the price bound it: no terms were stated");
    }
    // Recorded as raised and done where it reached the operator, once each, and the log admitted them.
    let entries = raise_entries(&core);
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|(_, o)| *o == Outcome::Done {}));
    assert_eq!(
        core.raised(),
        raised,
        "the channel carried what the log holds"
    );
    // The first segment's charge is raised before the second segment is handed over.
    let places = |wanted: &dyn Fn(&What) -> bool| -> Vec<usize> {
        core.log()
            .entries()
            .iter()
            .enumerate()
            .filter(|(_, (_, e))| matches!(e, Entry::Verified(v) if wanted(&v.what)))
            .map(|(i, _)| i)
            .collect()
    };
    let raises_at = places(&|w| matches!(w, What::Raise { .. }));
    let effects_at = places(&|w| {
        matches!(
            w,
            What::Effect {
                action: Action::MoveValue { .. },
                ..
            }
        )
    });
    assert!(effects_at.len() >= 2);
    assert!(
        raises_at[0] < effects_at[1],
        "the operator is warned before the next segment is handed over"
    );
    // What Core checked, read back per segment.
    let segments: Vec<Id> = raised
        .iter()
        .map(|r| match &r.difference {
            Difference::Charge { segment, .. } => segment.clone(),
            _ => unreachable!("a charge difference"),
        })
        .collect();
    for segment in &segments {
        assert!(matches!(
            core.charge_checked(&order, segment),
            Some(Checked::Flagged(f)) if f.delivered
        ));
    }
}

// verifies: REQ-QTE-5
#[test]
fn charge_within_the_formulas_maximum_raises_nothing() {
    let (mut core, session) = alone(kit::RESERVE, "AC-14");
    let order = ran(&mut core, &session, kit::RESERVE, "req-honest");
    assert!(core.raised().is_empty(), "nothing reached the operator");
    assert!(core.operator_notices().is_empty());
    assert!(raise_entries(&core).is_empty(), "nothing was recorded");
    let checked = core.charged_segments(&order);
    assert_eq!(checked.len(), 2, "both segments' charges were noted");
    for segment in &checked {
        assert!(
            matches!(core.charge_checked(&order, segment), Some(Checked::Within)),
            "{segment:?} was checked and is within"
        );
    }
}

/// The provider's signed terms for each tuple a run of `order` used, with a fee of at most `maximum`, made now: terms
/// a scenario has the evidence store answer in place of the ones the provider handed back, which Core never takes.
fn terms_for(core: &Core, order: &Id, maximum: &str) -> Vec<Vec<u8>> {
    let context = rutent_scenarios::assembly::context();
    core.route_operations(order)
        .iter()
        .map(|op| {
            let mut statement = json!({
                "deployment": context.deployment,
                "protocol_version": context.protocol_version,
                "shape": "urn:rutent:provider-role:terms",
                "maker": op.provider,
                "made_at": core.clock.timestamp(),
                "signature": { "scheme": "ed25519", "key": "", "value": "" },
                "statement": {
                    "tuple": { "provider": op.provider, "from": op.from, "to": op.to },
                    "fee": { "flat": "0", "proportional": { "numerator": "0", "denominator": "1" },
                        "maximum": maximum },
                },
            });
            let key = core
                .statements_signing_key(op.provider.as_str())
                .expect("the provider is installed");
            rutent_conformance::signing::sign_value(&mut statement, &key);
            serde_json::to_vec(&statement).expect("JSON")
        })
        .collect()
}

/// On the next quote, `provider` hands back, in its answers to the dry runs of its withdrawal and its deposit, its
/// terms of a fee of at most `maximum` for the tuple asked: its own statements, which the platform signs with its key
/// (`T-020-56`).
fn states_terms(core: &mut Core, provider: &str, maximum: &str) {
    for kind in ["withdrawal", "deposit"] {
        core.world()
            .script_dry_run(provider, kind, Behaviour::stating_terms_of_at_most(maximum));
    }
}

/// The receipts of the terms' stated entries the log holds, in order, with who the log attributed each to.
fn terms_entries(core: &Core) -> Vec<(Receipt, Attribution)> {
    core.log()
        .entries()
        .iter()
        .filter_map(|(r, e)| match e {
            Entry::Stated(s) if s.what.statement.as_str() == "urn:rutent:provider-role:terms" => {
                Some((r.clone(), s.attribution.clone()))
            }
            _ => None,
        })
        .collect()
}

/// An order of `provider` for the whole amount, quoted: its id and its quote's digest.
fn quoted_at(
    core: &mut Core,
    session: &Id,
    provider: &str,
    name: &str,
) -> (Id, rutent_contracts::module_interface::Digest) {
    let (order, told) = quoted(
        core,
        request_at(provider, name, AMOUNT, session, "lowest_total_cost"),
    );
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("quoted");
    };
    (order, digest)
}

/// The terms the provider hands back in its answers to Core's own dry runs bind each segment of the order they were
/// quoted for, from its start (`REQ-QTE-6`): they are the provider's own statements, as the platform's signer signed
/// them and the log attributed them, taken by Core with no one handing them in; a charge within them raises nothing,
/// and one above them is raised, the raise citing the terms that bound it (`T-020-56`).
// verifies: REQ-QTE-6
#[test]
fn terms_a_module_stated_before_its_segment_started_bind_its_charge() {
    let (mut core, session) = alone(CHARGING, "AC-14");
    // Terms that allow the 7 it charges: within them, nothing is raised, though the price quoted none.
    states_terms(&mut core, CHARGING, "100");
    let within = ran(&mut core, &session, CHARGING, "req-within");
    let stated = terms_entries(&core);
    assert_eq!(
        stated.len(),
        2,
        "one for each tuple the route asked: {stated:?}"
    );
    for (_, attribution) in &stated {
        assert!(
            matches!(attribution, Attribution::Verified { who } if who.acts.as_str() == CHARGING),
            "the provider's own terms, signed by the platform with its key: {attribution:?}"
        );
    }
    assert_eq!(core.terms_held(), 2);
    assert!(core.raised().is_empty(), "within the terms that bound it");
    for segment in core.charged_segments(&within) {
        assert!(matches!(
            core.charge_checked(&within, &segment),
            Some(Checked::Within)
        ));
    }

    // Tighter terms, handed back on the next order's quote, bind its segments: 7 is above 3, and each raise cites
    // the terms that bound it.
    core.clock.advance(60);
    states_terms(&mut core, CHARGING, "3");
    let tight = ran(&mut core, &session, CHARGING, "req-tight");
    let receipts: Vec<Receipt> = terms_entries(&core)
        .into_iter()
        .skip(stated.len())
        .map(|(r, _)| r)
        .collect();
    assert_eq!(receipts.len(), 2);
    let raised = core.raised();
    assert_eq!(raised.len(), 2, "both of the order's charges are above 3");
    for raise in &raised {
        let Difference::Charge {
            order,
            terms,
            maximum,
            ..
        } = &raise.difference
        else {
            panic!("a charge difference");
        };
        assert_eq!(order, &tight);
        assert_eq!(maximum.as_str(), "3");
        assert!(
            terms.as_ref().is_some_and(|t| receipts.contains(t)),
            "it rests on the terms the provider stated before its segment started"
        );
    }
}

/// The terms a provider hands back in a recalculation's dry runs bind the recalculated route's segments, as a quote's
/// bind its route's (`REQ-QTE-6`, `T-020-56`'s review, gap 3): the order quoted on terms that allow the 7 the reserve
/// charges, its deposit refused, then recalculated on the reserve's tighter terms of 3. The new deposit's charge is held
/// to those, the latest stated before it started, and raised citing them; held to the quote's, it would pass.
// verifies: REQ-QTE-6
#[test]
fn terms_a_recalculations_dry_runs_handed_back_bind_the_recalculated_segments() {
    let (mut core, session) = alone(CHARGING, "AC-14");
    states_terms(&mut core, CHARGING, "100");
    let (order, digest) = quoted_at(&mut core, &session, CHARGING, "req-recalculated");
    core.accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    core.world()
        .script_for(CHARGING, "deposit", Behaviour::act(Act::Refuse));
    core.run(&order)
        .expect("the order runs to its refused deposit");
    assert!(core.raised().is_empty(), "the withdrawal's 7 is within 100");
    let quoted = terms_entries(&core).len();

    core.clock.advance(60);
    core.world().script_dry_run(
        CHARGING,
        "deposit",
        Behaviour::stating_terms_of_at_most("3"),
    );
    let Recalculation::Recalculated(recalculated) =
        core.recalculate(&order).expect("the recalculation ran")
    else {
        panic!("a route to the same destination was found");
    };
    let receipts: Vec<Receipt> = terms_entries(&core)
        .into_iter()
        .skip(quoted)
        .map(|(r, _)| r)
        .collect();
    assert_eq!(
        receipts.len(),
        1,
        "the new deposit's terms, taken in the recalculation"
    );
    core.run_recalculated(&order, &recalculated)
        .expect("the recalculated part runs");
    let raised = core.raised();
    assert_eq!(raised.len(), 1, "the new deposit's 7 is above 3");
    let Difference::Charge { terms, maximum, .. } = &raised[0].difference else {
        panic!("a charge difference");
    };
    assert_eq!(maximum.as_str(), "3");
    assert!(
        terms.as_ref().is_some_and(|t| receipts.contains(t)),
        "it rests on the terms the recalculation's dry run handed back"
    );
}

// verifies: REQ-QTE-6
#[test]
fn terms_stated_after_a_segment_started_do_not_bind_it() {
    let (mut core, session) = alone(CHARGING, "AC-14");
    let first = ran(&mut core, &session, CHARGING, "req-first");
    assert_eq!(core.raised().len(), 2, "held to the price: 7 is above 0");

    // The provider then hands back terms that allow 7, on the next order's quote. The segments that ran are not held
    // to them.
    core.clock.advance(60);
    states_terms(&mut core, CHARGING, "100");
    let (second, digest) = quoted_at(&mut core, &session, CHARGING, "req-second");
    assert_eq!(terms_entries(&core).len(), 2, "stated on the quote");
    core.run(&first).expect("runs again");
    assert_eq!(core.raised().len(), 2, "no raise is made or unmade by them");
    for (raise, _) in raise_entries(&core) {
        let Difference::Charge { terms, .. } = raise.difference else {
            panic!("a charge difference");
        };
        assert!(
            terms.is_none(),
            "the price bound it, not terms stated later"
        );
    }

    // A segment that starts after them is held to them: 7 is within 100, and nothing is raised.
    core.clock.advance(60);
    core.accept(&kit::customer(), &second, &digest)
        .expect("accepted");
    core.run(&second).expect("the order runs");
    assert_eq!(core.raised().len(), 2, "within the terms in force");
    let segments = core.charged_segments(&second);
    assert_eq!(segments.len(), 2);
    for segment in &segments {
        assert!(matches!(
            core.charge_checked(&second, segment),
            Some(Checked::Within)
        ));
    }

    // Terms stated tighter bind the segments after them, and the raise cites them.
    core.clock.advance(60);
    states_terms(&mut core, CHARGING, "3");
    let held = terms_entries(&core).len();
    let third = ran(&mut core, &session, CHARGING, "req-third");
    let receipts: Vec<Receipt> = terms_entries(&core)
        .into_iter()
        .skip(held)
        .map(|(r, _)| r)
        .collect();
    let raised = core.raised();
    assert_eq!(raised.len(), 4, "the third order's two charges are above 3");
    for raise in raised.iter().filter(|r| match &r.difference {
        Difference::Charge { order, .. } => order == &third,
        _ => false,
    }) {
        let Difference::Charge { terms, maximum, .. } = &raise.difference else {
            panic!("a charge difference");
        };
        assert_eq!(maximum.as_str(), "3");
        assert!(
            terms.as_ref().is_some_and(|t| receipts.contains(t)),
            "it rests on the terms that bound it"
        );
    }
}

/// A read of the evidence store that keeps what the log's entry doesn't name: the right commitment, `other` bytes.
fn altered(other: Vec<u8>) -> Box<KeptThrough> {
    Box::new(move |found: Option<KeptStatement>| {
        found.map(|kept| KeptStatement {
            commitment: kept.commitment,
            salt: Salt::new("A".repeat(43)).expect("a salt"),
            bytes: Encoded::new(rutent_conformance::signing::base64url(&other)).expect("bytes"),
        })
    })
}

/// A read of the evidence store that answers the statement it kept, byte for byte, under another commitment than the
/// one asked for.
fn recommitted() -> Box<KeptThrough> {
    Box::new(|found: Option<KeptStatement>| {
        found.map(|kept| {
            let mut other = serde_json::to_value(&kept.commitment).expect("a digest is JSON");
            let value = other["value"].as_str().expect("a digest value").to_owned();
            let first = if value.starts_with('a') { 'b' } else { 'a' };
            other["value"] = format!("{first}{}", &value[1..]).into();
            KeptStatement {
                commitment: serde_json::from_value(other).expect("another digest"),
                ..kept
            }
        })
    })
}

/// A read of the evidence store that keeps nothing.
fn nothing() -> Box<KeptThrough> {
    Box::new(|_: Option<KeptStatement>| None)
}

// verifies: REQ-QTE-6, REQ-AUD-2
#[test]
fn terms_whose_kept_statement_does_not_match_the_entrys_commitment_are_not_kept() {
    let (mut core, session) = alone(CHARGING, "AC-14");
    let first = ran(&mut core, &session, CHARGING, "req-first");
    let other = terms_for(&core, &first, "101");

    // The store keeps other bytes behind the commitment, or none: the terms the provider handed back are appended, and
    // not kept.
    let held = core.terms_held();
    states_terms(&mut core, CHARGING, "100");
    core.read_kept_through(Some(altered(other[0].clone())));
    let (second, digest) = quoted_at(&mut core, &session, CHARGING, "req-second");
    states_terms(&mut core, CHARGING, "100");
    core.read_kept_through(Some(nothing()));
    quoted_at(&mut core, &session, CHARGING, "req-third");
    core.read_kept_through(None);
    assert_eq!(terms_entries(&core).len(), 4, "the log took each");
    assert_eq!(core.terms_held(), held, "nothing was kept");

    // So the next order's segments are held to the price, not to terms that rest on nothing the log holds.
    core.clock.advance(60);
    core.accept(&kit::customer(), &second, &digest)
        .expect("accepted");
    core.run(&second).expect("the order runs");
    let flagged: Vec<_> = core
        .raised()
        .into_iter()
        .filter(|r| matches!(&r.difference, Difference::Charge { order, .. } if order == &second))
        .collect();
    assert_eq!(flagged.len(), 2);

    // The store's own bytes behind the same commitment are kept.
    states_terms(&mut core, CHARGING, "100");
    quoted_at(&mut core, &session, CHARGING, "req-fourth");
    assert_eq!(core.terms_held(), held + 2);
}

// verifies: REQ-QTE-5
#[test]
fn each_segments_charge_is_checked_once_also_after_a_restart() {
    let (mut core, session) = alone(CHARGING, "AC-14");
    let order = ran(&mut core, &session, CHARGING, "req-charged");
    assert_eq!(core.raised().len(), 2);

    // Run again, as a caller retrying does: nothing more is raised.
    core.run(&order).expect("runs again");
    assert_eq!(core.raised().len(), 2);
    assert_eq!(raise_entries(&core).len(), 2);

    // The register of checked charges lost and rebuilt from the log: the raises it holds are not made again.
    core.charges_rebuilt_from_log();
    core.run(&order).expect("runs again");
    assert_eq!(core.raised().len(), 2, "the operator is warned once");
    assert_eq!(raise_entries(&core).len(), 2, "and the log holds each once");
    let segment = core.charged_segments(&order)[0].clone();
    assert!(matches!(
        core.charge_checked(&order, &segment),
        Some(Checked::AlreadyRaised)
    ));
}

/// A charge raised and recorded before Core reopens is read back from the log by the reopened Core (each settled
/// segment's charge as its kept result states it, and the raises the log holds): run again, its order raises nothing
/// more, the operator warned once and the log holding each raise once (`NFR-6`, `REQ-QTE-5`).
// verifies: NFR-6
#[test]
fn charge_raise_recorded_before_core_reopens_is_not_raised_again() {
    let given = || Given {
        providers: vec![Spec::of(Role::Reserve, CHARGING)],
        staging: Staged::AtOnce,
        deadline: std::time::Duration::from_secs(2),
        delivery: rutent_scenarios::assembly::machine::Delivery::Once,
    };
    let (core, session) = alone(CHARGING, "AC-14");
    let mut core = core;
    let order = ran(&mut core, &session, CHARGING, "req-charged");
    assert_eq!(raise_entries(&core).len(), 2);
    let mut core = Scenario::new("charge-check", "AC-14", given()).reopen(core, 1, &mut Reference);
    let segment = core.charged_segments(&order)[0].clone();
    assert!(
        matches!(
            core.charge_checked(&order, &segment),
            Some(Checked::AlreadyRaised)
        ),
        "the reopened Core holds the charge raised"
    );
    core.run(&order).expect("runs again");
    assert_eq!(
        raise_entries(&core).len(),
        2,
        "the log holds each raise once"
    );
}

/// A charge above its formula whose raise the log can't take when Core stops: the reopened Core reads each settled
/// segment's charge back from its kept result, so on the next run the raise is made, once (`NFR-6`, `REQ-QTE-5`).
// verifies: NFR-6, REQ-QTE-5
#[test]
fn charge_raise_cut_short_before_core_reopens_is_raised_once() {
    let given = || Given {
        providers: vec![Spec::of(Role::Reserve, CHARGING)],
        staging: Staged::AtOnce,
        deadline: std::time::Duration::from_secs(2),
        delivery: rutent_scenarios::assembly::machine::Delivery::Once,
    };
    let (mut core, session) = alone(CHARGING, "AC-14");
    // Turned away on every try the settling and the run's own re-check make: nothing of it is recorded.
    core.refuse_log_appends("raise", 2 * rutent_core_orch::segments::APPEND_TRIES);
    let (order, told) = quoted(
        &mut core,
        request_at(
            CHARGING,
            "req-charged",
            AMOUNT,
            &session,
            "lowest_total_cost",
        ),
    );
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("quoted");
    };
    core.accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    assert!(core.run(&order).is_err(), "the log couldn't take the raise");
    assert_eq!(raise_entries(&core).len(), 0, "no raise reached the log");
    let mut core = Scenario::new("charge-check", "AC-14", given()).reopen(core, 1, &mut Reference);
    core.run(&order).expect("the part goes on");
    assert_eq!(
        raise_entries(&core).len(),
        2,
        "the settled segment's raise made after the reopening, once, and the second segment's"
    );
    core.run(&order).expect("runs again");
    assert_eq!(raise_entries(&core).len(), 2);
}

/// Terms a provider stated before Core stopped bind the segments that start after the reopening: the reopened Core
/// reads them back from the evidence store, so an order run after it is held to them (7 within 100), and nothing is
/// raised, as the Core that ran on would (`REQ-QTE-6`, `NFR-6`, `T-020-46`).
// verifies: REQ-QTE-6, NFR-6
#[test]
fn terms_stated_before_core_reopens_bind_the_segments_after_it() {
    let given = || Given {
        providers: vec![Spec::of(Role::Reserve, CHARGING)],
        staging: Staged::AtOnce,
        deadline: std::time::Duration::from_secs(2),
        delivery: rutent_scenarios::assembly::machine::Delivery::Once,
    };
    let (mut core, session) = alone(CHARGING, "AC-14");
    ran(&mut core, &session, CHARGING, "req-first");
    assert_eq!(core.raised().len(), 2, "held to the price: 7 is above 0");
    // The provider hands back terms that allow 7 on a quote before Core stops, and none on the quote after it reopens.
    core.clock.advance(60);
    states_terms(&mut core, CHARGING, "100");
    quoted_at(&mut core, &session, CHARGING, "req-quoted");
    assert_eq!(core.terms_held(), 2);
    let mut core = Scenario::new("charge-check", "AC-14", given()).reopen(core, 1, &mut Reference);
    core.clock.advance(60);
    // The session opened before the restart is still open, and the reopened Core holds it, as the Core that ran on
    // would (`T-021-17`): its id opens no other session.
    let second = ran(&mut core, &session, CHARGING, "req-second");
    assert_eq!(raise_entries(&core).len(), 2, "within the terms in force");
    let segments = core.charged_segments(&second);
    assert_eq!(segments.len(), 2);
    for segment in &segments {
        assert!(matches!(
            core.charge_checked(&second, segment),
            Some(Checked::Within)
        ));
    }
}

/// An order's executed price reported through the assembly before Core stopped: the reopened Core holds it as
/// reported from the log's `shown` entry (`Prices::recorded`), so the report asked again after the reopening is
/// answered with the entry the log took, and the log holds one price for the order (`REQ-QTE-7`, `NFR-6`, `T-020-46`).
// verifies: REQ-QTE-7, NFR-6
#[test]
fn executed_price_reported_through_the_assembly_before_core_reopens_is_not_reported_again() {
    let given = || Given {
        providers: vec![Spec::of(Role::Reserve, kit::RESERVE)],
        staging: Staged::AtOnce,
        deadline: std::time::Duration::from_secs(2),
        delivery: rutent_scenarios::assembly::machine::Delivery::Once,
    };
    let (mut core, session) = alone(kit::RESERVE, "AC-14");
    let (order, told) = quoted(
        &mut core,
        request_at(
            kit::RESERVE,
            "req-priced",
            AMOUNT,
            &session,
            "lowest_total_cost",
        ),
    );
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("quoted");
    };
    core.accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    core.run(&order).expect("the order runs");
    let (_, receipt) = core
        .price_reported(&order)
        .expect("Core reported the price when the order completed");
    let prices = |core: &Core| {
        core.verified_whats()
            .iter()
            .filter(|w| {
                matches!(w, What::Shown { order: o, shown_as: rutent_contracts::core_internal::ShownAs::Price, .. }
                    if *o == order)
            })
            .count()
    };
    assert_eq!(prices(&core), 1);
    let mut core = Scenario::new("charge-check", "AC-14", given()).reopen(core, 1, &mut Reference);
    // Later: a price built afresh would be another entry, at another time.
    core.clock.advance(60);
    core.run(&order).expect("runs again");
    let again = core.price_reported(&order);
    assert!(
        matches!(&again, Some((_, r)) if *r == receipt),
        "the price the log took answers again: {again:?}"
    );
    assert_eq!(prices(&core), 1, "one price for the order");
}

/// The measure records a run's segment endings gave, as the log holds them: by provider, role, segment and measure.
fn measures_in_the_log(core: &Core) -> Vec<String> {
    let mut kept: Vec<String> = core
        .verified_whats()
        .iter()
        .filter_map(|w| match w {
            What::Measure { record } => Some(format!(
                "{} {:?} {:?} {:?} citing {}",
                record.provider.as_str(),
                record.role,
                record.segment.as_ref().map(Id::as_str),
                record.measure,
                record.cites.len()
            )),
            _ => None,
        })
        .collect();
    kept.sort();
    kept
}

/// The measure records Core keeps for its writer are rebuilt from the log when it reopens (`REQ-AUD-7`, `NFR-6`,
/// `T-020-46`): an order run and Core reopened before the writer was called writes, after the reopening, every record
/// the Core that ran on writes, once; an order whose records were written before the reopening writes none again.
// verifies: REQ-AUD-7, NFR-6
#[test]
fn measure_records_a_restart_dropped_are_written_once_after_core_reopens() {
    let given = || Given {
        providers: vec![Spec::of(Role::Reserve, kit::RESERVE)],
        staging: Staged::AtOnce,
        deadline: std::time::Duration::from_secs(2),
        delivery: rutent_scenarios::assembly::machine::Delivery::Once,
    };
    let reopen = |core: Core| {
        Scenario::new("charge-check", "AC-14", given()).reopen(core, 1, &mut Reference)
    };

    let (mut ran_on, session) = alone(kit::RESERVE, "AC-14");
    let _ = ran(&mut ran_on, &session, kit::RESERVE, "req-measured");
    let written = ran_on.write_measures();
    assert!(written > 0, "the run's endings give records");
    let expected = measures_in_the_log(&ran_on);

    // Reopened before the writer was called: the records are written after it, each once.
    let (mut core, session) = alone(kit::RESERVE, "AC-14");
    let _ = ran(&mut core, &session, kit::RESERVE, "req-measured");
    let mut core = reopen(core);
    assert_eq!(
        core.write_measures(),
        written,
        "every record the restart dropped"
    );
    assert_eq!(core.write_measures(), 0, "and none twice");
    assert_eq!(measures_in_the_log(&core), expected);

    // Written before the reopening: none again after it.
    let (mut core, session) = alone(kit::RESERVE, "AC-14");
    let _ = ran(&mut core, &session, kit::RESERVE, "req-measured");
    assert_eq!(core.write_measures(), written);
    let mut core = reopen(core);
    assert_eq!(core.write_measures(), 0, "nothing measured twice");
    assert_eq!(measures_in_the_log(&core), expected);
}

// verifies: REQ-QTE-5
#[test]
fn the_assembly_no_longer_checks_charges_itself() {
    // A scan of the assembly's source, not a scenario: it keeps `AC-14`'s scenarios, which call Core's own check, from
    // passing on a stand-in put back, and it verifies `AC-14` by none of its own (the table counts scenarios alone for
    // an acceptance criterion). The assembly holds no check of its own: its charge module only builds the channel and reads it, and nothing
    // of it calls the quote module's check or builds its desk.
    for (file, text) in [
        ("charges.rs", include_str!("../src/assembly/charges.rs")),
        ("mod.rs", include_str!("../src/assembly/mod.rs")),
    ] {
        for forbidden in [
            "ChargeDesk",
            "Charges::new",
            "check_charges(",
            ".check(desk",
        ] {
            let code: String = text
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                !code.contains(forbidden),
                "{file} still checks charges itself: {forbidden}"
            );
        }
    }
}

/// The places in the log of the verified entries `wanted` picks.
fn places(core: &Core, wanted: &dyn Fn(&What) -> bool) -> Vec<usize> {
    core.log()
        .entries()
        .iter()
        .enumerate()
        .filter(|(_, (_, e))| matches!(e, Entry::Verified(v) if wanted(&v.what)))
        .map(|(i, _)| i)
        .collect()
}

fn moves(w: &What) -> bool {
    matches!(
        w,
        What::Effect {
            action: Action::MoveValue { .. },
            ..
        }
    )
}

fn raises(w: &What) -> bool {
    matches!(w, What::Raise { .. })
}

/// An order of `provider`, quoted and accepted, not yet run.
fn accepted(core: &mut Core, session: &Id, provider: &str, name: &str) -> Id {
    let (order, told) = quoted(
        core,
        request_at(provider, name, AMOUNT, session, "lowest_total_cost"),
    );
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("quoted");
    };
    core.accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    order
}

// verifies: REQ-QTE-5
#[test]
fn a_raise_the_log_could_not_take_is_recorded_by_the_same_run_before_the_next_segment() {
    let (mut core, session) = alone(CHARGING, "AC-14");
    let order = accepted(&mut core, &session, CHARGING, "req-charged");
    // The first segment's raise is turned away on each of the three tries its settling makes.
    core.refuse_log_appends("raise", rutent_core_orch::segments::APPEND_TRIES);
    assert!(core.run(&order).is_err(), "the log couldn't take the raise");

    // The run still held the part's settled segment to its check, which recorded the raise the operator was told of
    // once: before the second segment was handed over, which the failed run never did.
    assert_eq!(places(&core, &raises).len(), 1, "the raise was recorded");
    assert_eq!(places(&core, &moves).len(), 1, "no second segment moved");
    assert_eq!(core.raised().len(), 1, "and the operator was warned once");

    // Run again: the part goes on, each charge raised once, the first before the second segment moved.
    core.run(&order).expect("the part goes on");
    assert_eq!(core.raised().len(), 2, "one warning for each segment");
    let (raised, moved) = (places(&core, &raises), places(&core, &moves));
    assert_eq!((raised.len(), moved.len()), (2, 2));
    assert!(raised[0] < moved[1], "warned before the next segment moved");
}

// verifies: REQ-QTE-5
#[test]
fn a_raise_retried_by_a_later_run_is_made_once_and_before_the_next_segment() {
    let (mut core, session) = alone(CHARGING, "AC-14");
    let order = accepted(&mut core, &session, CHARGING, "req-charged");
    // Turned away on every try the settling and the run's own re-check make: nothing of it is recorded.
    core.refuse_log_appends("raise", 2 * rutent_core_orch::segments::APPEND_TRIES);
    assert!(core.run(&order).is_err());
    assert!(places(&core, &raises).is_empty(), "nothing recorded yet");
    assert_eq!(places(&core, &moves).len(), 1, "no second segment moved");
    assert_eq!(core.raised().len(), 1, "the operator was warned, once");

    // The next run records it before it hands the second segment over, and warns no one again.
    core.run(&order).expect("the part goes on");
    let (raised, moved) = (places(&core, &raises), places(&core, &moves));
    assert_eq!((raised.len(), moved.len()), (2, 2));
    assert!(
        raised[0] < moved[1],
        "recorded before the next segment moved"
    );
    assert_eq!(core.raised().len(), 2, "one warning for each segment");
}

// verifies: REQ-QTE-6, REQ-AUD-2
#[test]
fn terms_whose_kept_statement_names_another_commitment_are_not_kept() {
    let (mut core, session) = alone(CHARGING, "AC-14");
    ran(&mut core, &session, CHARGING, "req-first");

    // The store answers the very statement the provider handed back, under a commitment the entry doesn't name: it is
    // not what the entry rests on, so the terms are not kept.
    let held = core.terms_held();
    states_terms(&mut core, CHARGING, "100");
    core.read_kept_through(Some(recommitted()));
    quoted_at(&mut core, &session, CHARGING, "req-second");
    core.read_kept_through(None);
    assert_eq!(terms_entries(&core).len(), 2, "the log took them");
    assert_eq!(core.terms_held(), held, "nothing was kept");

    // The store's own answer, under the commitment asked for, is kept.
    states_terms(&mut core, CHARGING, "100");
    quoted_at(&mut core, &session, CHARGING, "req-third");
    assert_eq!(core.terms_held(), held + 2);
}

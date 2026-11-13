//! Core's own charge check (`T-020-14`): each settled segment's charge held to the formula that bound it by Core's
//! segments, a charge above it raised to the operator through Core's own channel and recorded, the terms a provider
//! stated binding from the segment's start and kept only as the evidence store holds them, and each segment's charge
//! checked once. Over the assembled Core against the simulated providers; the log admits everything it takes.

mod routing_support;

use ed25519_dalek::Verifier as _;
use routing_support::*;
use rutent_conformance::sim::{Role, Spec};
use rutent_contracts::core_internal::calls::{AppendFailure, KeptStatements};
use rutent_contracts::core_internal::{
    Action, Difference, Encoded, Entry, KeptStatement, Outcome, Raise, Salt, VerifiedEntry, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::PublicKey;
use rutent_contracts::router_api::Id;
use rutent_core_orch::intake::{Status, Told};
use rutent_core_orch::notice::{About, Inbox, Receiver, Taken, Verifier};
use rutent_core_orch::quote::{Checked, NotTerms, TakeError};
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, Staged};
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

/// The provider's signed terms for each tuple a run of `order` used, with a fee of at most `maximum`, made now.
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

// verifies: REQ-QTE-6
#[test]
fn terms_stated_after_a_segment_started_do_not_bind_it() {
    let (mut core, session) = alone(CHARGING, "AC-14");
    let first = ran(&mut core, &session, CHARGING, "req-first");
    assert_eq!(core.raised().len(), 2, "held to the price: 7 is above 0");

    // The provider then states terms that allow 7. The segments that ran are not held to them.
    core.clock.advance(60);
    for bytes in terms_for(&core, &first, "100") {
        assert!(core.state_terms(&bytes).expect("kept").new);
    }
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
    let second = ran(&mut core, &session, CHARGING, "req-second");
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
    let tight: Vec<Vec<u8>> = terms_for(&core, &first, "3");
    let receipts: Vec<_> = tight
        .iter()
        .map(|b| core.state_terms(b).expect("kept").receipt)
        .collect();
    core.clock.advance(60);
    let third = ran(&mut core, &session, CHARGING, "req-third");
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

/// A read of the evidence store that keeps what the log's entry doesn't name: the right commitment, other bytes.
struct Altered(Vec<u8>);

impl KeptStatements for Altered {
    fn kept(&mut self, commitment: &Digest) -> Result<Option<KeptStatement>, AppendFailure> {
        Ok(Some(KeptStatement {
            commitment: commitment.clone(),
            salt: Salt::new("A".repeat(43)).expect("a salt"),
            bytes: Encoded::new(rutent_conformance::signing::base64url(&self.0)).expect("bytes"),
        }))
    }
}

/// A read of the evidence store that answers the statement it was handed, byte for byte, under another commitment
/// than the one asked for.
struct Recommitted(Vec<u8>);

impl KeptStatements for Recommitted {
    fn kept(&mut self, commitment: &Digest) -> Result<Option<KeptStatement>, AppendFailure> {
        let mut other = serde_json::to_value(commitment).expect("a digest is JSON");
        let value = other["value"].as_str().expect("a digest value").to_owned();
        let first = if value.starts_with('a') { 'b' } else { 'a' };
        other["value"] = format!("{first}{}", &value[1..]).into();
        Ok(Some(KeptStatement {
            commitment: serde_json::from_value(other).expect("another digest"),
            salt: Salt::new("A".repeat(43)).expect("a salt"),
            bytes: Encoded::new(rutent_conformance::signing::base64url(&self.0)).expect("bytes"),
        }))
    }
}

/// A read of the evidence store that keeps nothing.
struct Nothing;

impl KeptStatements for Nothing {
    fn kept(&mut self, _: &Digest) -> Result<Option<KeptStatement>, AppendFailure> {
        Ok(None)
    }
}

// verifies: REQ-QTE-6, REQ-AUD-2
#[test]
fn terms_whose_kept_statement_does_not_match_the_entrys_commitment_are_not_kept() {
    let (mut core, session) = alone(CHARGING, "AC-14");
    let first = ran(&mut core, &session, CHARGING, "req-first");
    let allowing = terms_for(&core, &first, "100");
    let other = terms_for(&core, &first, "101");

    // The store keeps other bytes behind the commitment, or none: the terms are not kept.
    let held = core.terms_held();
    let refused = core.state_terms_read_by(&mut Altered(other[0].clone()), &allowing[0]);
    assert_eq!(refused, Err(TakeError::NotKept(NotTerms::NotWhatIsKept)));
    let refused = core.state_terms_read_by(&mut Nothing, &allowing[0]);
    assert_eq!(refused, Err(TakeError::NotKept(NotTerms::NotWhatIsKept)));
    assert_eq!(core.terms_held(), held, "nothing was kept");

    // So the next order's segments are held to the price, not to terms that rest on nothing the log holds.
    core.clock.advance(60);
    let second = ran(&mut core, &session, CHARGING, "req-second");
    let flagged: Vec<_> = core
        .raised()
        .into_iter()
        .filter(|r| matches!(&r.difference, Difference::Charge { order, .. } if order == &second))
        .collect();
    assert_eq!(flagged.len(), 2);

    // The store's own bytes behind the same commitment are kept.
    assert!(core.state_terms(&allowing[0]).expect("kept").new);
    assert_eq!(core.terms_held(), held + 1);
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

// verifies: AC-14
#[test]
fn the_assembly_no_longer_checks_charges_itself() {
    // The assembly holds no check of its own: its charge module only builds the channel and reads it, and nothing
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
    let first = ran(&mut core, &session, CHARGING, "req-first");
    let allowing = terms_for(&core, &first, "100");

    // The store answers the very statement handed in, under a commitment the entry doesn't name: it is not what
    // the entry rests on, so the terms are not kept.
    let held = core.terms_held();
    let refused = core.state_terms_read_by(&mut Recommitted(allowing[0].clone()), &allowing[0]);
    assert_eq!(refused, Err(TakeError::NotKept(NotTerms::NotWhatIsKept)));
    assert_eq!(core.terms_held(), held, "nothing was kept");

    // The store's own answer, under the commitment asked for, is kept.
    assert!(core.state_terms(&allowing[0]).expect("kept").new);
    assert_eq!(core.terms_held(), held + 1);
}

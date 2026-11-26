//! A gross, charged segment end to end through the scenarios kit (`T-024-13`): the stand-in reserve `sim-reserve-gross`
//! reports its tuples gross (`REQ-CAT-11`), so a segment's answer states what it gave out with its charge of
//! [`GROSS_FEE`] within it, and what arrives is that less the charge. A route over it, withdrawal into its ledger and
//! deposit back out, is quoted, accepted, run and closed on the assembled Core.
//!
//! The figures, worked by hand for the base amount of 1 000 000 and a charge of 5 000 on each movement:
//!
//! | segment    | took in   | states out | charge | arrives |
//! |------------|-----------|------------|--------|---------|
//! | withdrawal | 1 000 000 | 1 000 000  | 5 000  | 995 000 |
//! | deposit    | 995 000   | 995 000    | 5 000  | 990 000 |
//!
//! `T-024-04`'s `AC-1` and `AC-32` are the net case; these are the gross and charged one.

mod execution_support;

use execution_support::*;
use rutent_conformance::sim::{
    Behaviour, Delivery, GROSS_FEE, InnerSegment, Role, Spec, recorded_segments,
};
use rutent_contracts::core_internal::{Action, Difference, Entry, FeeTo, VerifiedEntry, What};
use rutent_contracts::provider_role::EvidenceGrade;
use rutent_contracts::router_api::{Amount, FeeFormula, Id};
use rutent_contracts::state_machine::OrderState;
use rutent_core_orch::intake::{Quote, Status, Told};
use rutent_core_orch::quote::Checked;
use rutent_core_orch::segments::{Accounted, Ran, SegmentOutcome};
use rutent_scenarios::assembly::Core;
use rutent_scenarios::kit::{self, AMOUNT};
use serde_json::json;

const GROSS: &str = "sim-reserve-gross";
const CHARGING: &str = "sim-reserve-gross-charging";

fn amount(text: &str) -> Amount {
    Amount::new(text.to_owned()).expect("an amount")
}

fn accounted(ran: &[Ran]) -> &[Accounted] {
    let [Ran::Segments(segments)] = ran else {
        panic!("one part ran its segments: {ran:?}");
    };
    segments
}

fn settled(segment: &Accounted) -> bool {
    matches!(segment.outcome, SegmentOutcome::Settled { .. })
}

/// What a segment's settled outcome handed on: what arrived at its destination net of its charge.
fn handed_on(segment: &Accounted) -> Amount {
    let SegmentOutcome::Settled { amount_out, .. } = &segment.outcome else {
        panic!("the segment settled: {segment:?}");
    };
    amount_out.clone()
}

/// The amount the one real operation of kind `kind` in the reserve's journal moved, as the operator stated it.
fn asked(core: &mut Core, reserve: &str, kind: &str) -> String {
    let moved: Vec<String> = core
        .world()
        .journal(reserve)
        .iter()
        .filter_map(|entry| {
            let operation = serde_json::to_value(&entry.operation.statement).expect("JSON");
            (operation["kind"] == kind)
                .then(|| operation["amount"].as_str().unwrap_or("").to_owned())
        })
        .collect();
    let [one] = moved.as_slice() else {
        panic!("exactly one real {kind} reached {reserve}: {moved:?}");
    };
    one.clone()
}

/// What the reserve stated of each segment it served, as its books keep them: the provider's own figures.
fn stated(core: &mut Core, reserve: &str) -> Vec<InnerSegment> {
    let journal = core.world().journal(reserve).to_vec();
    recorded_segments(
        &Spec::of(Role::Reserve, reserve),
        &rutent_scenarios::assembly::context(),
        &journal,
    )
}

/// An order of `reserve` for the base amount, requested, quoted, accepted and run: its id, its quote and what ran.
fn ran(core: &mut Core, session: &Id, reserve: &str, name: &str) -> (Id, Quote, Vec<Ran>) {
    let (order, told) = quoted(core, within(name, session, AMOUNT, reserve, OTHER));
    let Told::Exists {
        status: Status::Quoted { digest, quote, .. },
        ..
    } = told
    else {
        panic!("the order is quoted: {told:?}");
    };
    core.accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    let ran = core.run(&order).expect("the order ran");
    (order, *quote, ran)
}

/// The movement the network `network` holds for `id`, as the network records it.
fn recorded(core: &mut Core, network: &str, id: &str) -> rutent_evidence::Movement {
    let net = core.world().network(network).expect("the network").clone();
    let id = rutent_evidence::MovementId::new(id).expect("a movement id");
    let rutent_evidence::Status::Final(moved) = rutent_evidence::PublicNetwork::status(&net, &id)
    else {
        panic!("the movement is final");
    };
    moved
}

fn verified_of(core: &Core) -> Vec<VerifiedEntry> {
    core.log()
        .entries()
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(v) => Some(v.clone()),
            Entry::Stated(_) => None,
        })
        .collect()
}

// verifies: REQ-ORD-21, REQ-CAT-11
#[test]
fn gross_charged_segment_hands_the_next_segment_what_arrives() {
    let (mut core, session) = core_with("AC-1", &[GROSS]);
    let (order, _, ran) = ran(&mut core, &session, GROSS, "req-gross");
    let segments = accounted(&ran);
    assert_eq!(state(&mut core, &order), OrderState::Complete);

    // What the reserve stated: each segment gives out what it took in, its charge within it. A gross answer.
    let figures = stated(&mut core, GROSS);
    let states: Vec<_> = figures
        .iter()
        .map(|s| (s.amount_in.as_str(), s.amount_out.as_str(), s.fee.as_str()))
        .collect();
    assert_eq!(
        states,
        [("1000000", "1000000", "5000"), ("995000", "995000", "5000")]
    );

    // What Core records of each: what it took in, its charge by value, and what it handed on, net of that charge.
    let charged: Vec<_> = segments
        .iter()
        .map(|s| {
            core.charged_of(&order, &s.segment)
                .expect("a charge was noted")
        })
        .collect();
    assert_eq!(charged[0].amount_in, amount("1000000"));
    assert_eq!(charged[0].charged, amount(&GROSS_FEE.to_string()));
    assert_eq!(charged[1].amount_in, amount("995000"));
    assert_eq!(charged[1].charged, amount("5000"));
    assert_eq!(handed_on(&segments[0]), amount("995000"));
    assert_eq!(handed_on(&segments[1]), amount("990000"));
    assert_ne!(
        handed_on(&segments[0]).as_str(),
        figures[0].amount_out,
        "not what the reserve stated, its charge within it"
    );

    // The next segment moves what arrived: not the 1 000 000 that was quoted, nor the 1 000 000 stated gross.
    assert_eq!(asked(&mut core, GROSS, "deposit"), "995000");
    assert_eq!(asked(&mut core, GROSS, "withdrawal"), "1000000");
    // Each charge is held to the formula that bound it, 5 000 flat, and is within it: a charge read off the gross
    // figures, which differ by nothing, would be none.
    for segment in segments {
        assert!(
            matches!(
                core.charge_checked(&order, &segment.segment),
                Some(Checked::Within)
            ),
            "{segment:?} was checked and is within"
        );
    }
    assert!(
        core.raised().is_empty(),
        "the charge was within its formula"
    );
}

// verifies: REQ-ORD-21, REQ-CAT-11
#[test]
fn a_shortfall_of_a_gross_segment_is_what_the_next_segment_moves_and_not_its_failure() {
    let (mut core, session) = core_with("AC-1", &[GROSS]);
    // The withdrawal states 1% short of what it took in: 990 000, its charge of 5 000 within it.
    core.world().script_for(
        GROSS,
        "withdrawal",
        Behaviour::delivering(Delivery::Short { bps: 100 }),
    );
    let (order, _, ran) = ran(&mut core, &session, GROSS, "req-short");
    let segments = accounted(&ran);
    assert_eq!(handed_on(&segments[0]), amount("985000"), "what arrived");
    assert_eq!(
        asked(&mut core, GROSS, "deposit"),
        "985000",
        "from what is evidenced, not from the 995 000 that was quoted"
    );
    assert_eq!(handed_on(&segments[1]), amount("980000"));
    assert_eq!(state(&mut core, &order), OrderState::Complete);
}

// verifies: REQ-QTE-2
#[test]
fn operators_fee_is_figured_on_what_arrives_of_a_gross_segment() {
    let (mut core, session) = core_with("AC-1", &[GROSS]);
    // 1 000 plus 1% of what arrives: 10 900 on 990 000, and not 10 950 on the 995 000 the last segment states, nor
    // 11 000 on the 1 000 000 that left.
    let formula: FeeFormula = serde_json::from_value(json!({
        "flat": "1000", "proportional": { "numerator": "1", "denominator": "100" }
    }))
    .expect("a fee formula");
    core.set_operator_fee(formula.clone());
    let (order, quote, _) = ran(&mut core, &session, GROSS, "req-fee");

    // The quote shows it apart, on what arrives: the providers' terms end at 995 000, the last charge of 5 000 is a
    // line of its own, and the operator's fee is figured on what is left.
    let shown = &quote.price;
    assert_eq!(shown.providers_out, amount("995000"));
    let within: Vec<_> = shown.within.iter().map(|w| w.charge.clone()).collect();
    assert_eq!(within, [amount("5000")]);
    assert_eq!(shown.arrives, amount("990000"));
    assert_eq!(shown.operator_fee.formula, formula);
    assert_eq!(shown.operator_fee.amount, amount("10900"));
    assert_eq!(shown.shown_out, amount("979100"));
    assert_eq!(
        shown.difference,
        amount("15900"),
        "the charge and the fee, and nothing else"
    );
    assert_eq!(quote.delivers, amount("990000"));

    // The price reported once it ran, on what each segment did, says the same.
    let price = core
        .price_of(&order)
        .expect("Core reported the price when the order completed")
        .clone();
    let fees: Vec<_> = price
        .segments
        .iter()
        .map(|s| s.fee.amount.clone())
        .collect();
    assert_eq!(fees, ["5000", "5000"]);
    assert_eq!(price.arrives, amount("990000"));
    assert_eq!(price.operator_fee.amount, amount("10900"));
    assert_eq!(price.shown_out, amount("979100"));
    assert_eq!(price.difference, amount("15900"));

    // And the log's fee records: each provider's charge as its result states it, and the operator's on what arrived.
    let fees: Vec<(FeeTo, String)> = verified_of(&core)
        .into_iter()
        .filter_map(|v| match v.what {
            What::Fee {
                order: o,
                to,
                amount,
                ..
            } if o == order => Some((to, amount.as_str().to_owned())),
            _ => None,
        })
        .collect();
    assert_eq!(
        fees,
        [
            (FeeTo::Provider, "5000".to_owned()),
            (FeeTo::Provider, "5000".to_owned()),
            (FeeTo::Operator, "10900".to_owned()),
        ]
    );
}

// verifies: REQ-ORD-33
#[test]
fn final_deposit_closes_on_the_public_movement_at_what_arrives_the_charge_staying_with_the_reserve()
{
    let bank = format!("{GROSS}-bank");
    let (mut core, session) = core_with("AC-32", &[GROSS]);
    let (order, _, ran) = ran(&mut core, &session, GROSS, "req-closes");
    let segments = accounted(&ran);
    assert_eq!(state(&mut core, &order), OrderState::Complete);

    // The deposit took in 995 000 and states 995 000 out, its charge of 5 000 within it. What reaches the bank is what
    // arrives, 990 000: the charge stays with the reserve. The order closed on it.
    let deposit = segments[1].segment.clone();
    let closing = evidence_of(&core, &order)
        .into_iter()
        .find(|e| e.segment == deposit && e.movement.is_some())
        .expect("the public record");
    let movement = closing.movement.expect("a movement");
    assert_eq!(movement.network.as_str(), bank);
    let moved = recorded(&mut core, &bank, movement.id.as_str());
    assert_eq!(
        moved.amount, "990000",
        "what arrives, the charge staying with the reserve"
    );
    assert_eq!(moved.destination, OTHER);
    assert_eq!(moved.asset, "COP");
    assert_eq!(
        moved.amount,
        handed_on(&segments[1]).as_str(),
        "the figure Core settled"
    );
}

// verifies: REQ-ORD-33
#[test]
fn final_deposit_that_reaches_the_network_at_its_stated_gross_figure_does_not_close() {
    // A network that holds 995 000, the figure the deposit states, instead of the 990 000 that arrives, is not the
    // record of this segment: the order doesn't close on it.
    let (mut core, session) = core_with("AC-32", &[GROSS]);
    let order = accepted(
        &mut core,
        within("req-gross-movement", &session, AMOUNT, GROSS, OTHER),
    );
    core.world().script_for(
        GROSS,
        "deposit",
        Behaviour {
            charge_misplaced: true,
            ..Behaviour::default()
        },
    );
    let ran = core.run(&order).expect("the order ran");
    let segments = accounted(&ran);
    assert!(settled(&segments[0]));
    assert!(
        !settled(&segments[1]),
        "the deposit doesn't settle on a movement of another figure: {segments:?}"
    );
    assert_ne!(state(&mut core, &order), OrderState::Complete);
}

// verifies: REQ-ORD-33
#[test]
fn withdrawals_public_movement_is_checked_at_what_entered_the_charge_included() {
    let bank = format!("{GROSS}-bank");
    let (mut core, session) = core_with("AC-32", &[GROSS]);
    let (order, _, ran) = ran(&mut core, &session, GROSS, "req-entered");
    let segments = accounted(&ran);

    // The withdrawal took in 1 000 000 from the bank's network, its 5 000 charge included, and the network recorded
    // that, not the 995 000 that reached the ledger.
    let first = evidence_of(&core, &order)
        .into_iter()
        .find(|e| e.segment == segments[0].segment && e.movement.is_some())
        .expect("the withdrawal's public record");
    let movement = first.movement.expect("a movement");
    assert_eq!(movement.network.as_str(), bank);
    let moved = recorded(&mut core, &bank, movement.id.as_str());
    assert_eq!(moved.amount, "1000000", "what entered, the charge included");
    let charged = core
        .charged_of(&order, &segments[0].segment)
        .expect("noted");
    assert_eq!(
        moved.amount,
        charged.amount_in.as_str(),
        "the result's `amount_in`"
    );
    assert_ne!(moved.amount, handed_on(&segments[0]).as_str());
}

// verifies: REQ-ORD-33
#[test]
fn withdrawal_that_reaches_the_network_at_what_arrived_is_no_public_record() {
    // A network that holds 995 000, what the ledger got, is not the record of a withdrawal that took in 1 000 000: Core
    // doesn't rest the withdrawal's evidence on it, whatever the reserve's own statement says.
    let (mut core, session) = core_with("AC-32", &[GROSS]);
    let order = accepted(
        &mut core,
        within("req-entered-less", &session, AMOUNT, GROSS, OTHER),
    );
    core.world().script_for(
        GROSS,
        "withdrawal",
        Behaviour {
            charge_misplaced: true,
            ..Behaviour::default()
        },
    );
    let ran = core.run(&order).expect("the order ran");
    let segments = accounted(&ran);
    let first: Vec<_> = evidence_of(&core, &order)
        .into_iter()
        .filter(|e| e.segment == segments[0].segment)
        .collect();
    assert!(!first.is_empty(), "the reserve's own word is kept");
    assert!(
        first
            .iter()
            .all(|e| e.movement.is_none() && e.grade != EvidenceGrade::PublicRecord),
        "no public movement backs it: {first:?}"
    );
}

// verifies: AC-14
#[test]
fn charge_above_the_formulas_maximum_on_a_gross_segment_is_flagged_by_core() {
    let (mut core, session) = core_with("AC-14", &[CHARGING]);
    let (order, _, ran) = ran(&mut core, &session, CHARGING, "req-over");
    let segments = accounted(&ran);

    // Each movement was quoted at 5 000 and charged 5 007. The reserve states the gross figure, which says nothing
    // of the charge, so only the fee its result states tells.
    let figures = stated(&mut core, CHARGING);
    assert!(
        figures.iter().all(|s| s.amount_in == s.amount_out),
        "a gross answer"
    );
    assert_eq!(
        figures.iter().map(|s| s.fee.as_str()).collect::<Vec<_>>(),
        ["5007", "5007"]
    );

    // Core's own segments checked each charge against the formula that bound it, and flagged it: none passed.
    for segment in segments {
        assert!(
            matches!(
                core.charge_checked(&order, &segment.segment),
                Some(Checked::Flagged(ref f)) if f.delivered
            ),
            "{segment:?} was flagged"
        );
        assert_eq!(
            core.charged_of(&order, &segment.segment)
                .expect("noted")
                .charged,
            amount("5007")
        );
    }
    let raised = core.raised();
    assert_eq!(raised.len(), 2, "both charges reached the operator");
    for raise in &raised {
        let Difference::Charge {
            charged,
            maximum,
            provider,
            terms,
            no_formula,
            ..
        } = &raise.difference
        else {
            panic!("a charge difference");
        };
        assert_eq!(charged.as_str(), "5007");
        assert_eq!(maximum.as_str(), "5000", "the formula's own maximum");
        assert_eq!(provider.as_str(), CHARGING);
        assert!(terms.is_none(), "the price bound it: no terms were stated");
        assert!(no_formula.is_none(), "a formula covers this asset");
    }
    // The charge is raised before the next segment is handed over.
    let at = |wanted: &dyn Fn(&What) -> bool| -> Vec<usize> {
        core.log()
            .entries()
            .iter()
            .enumerate()
            .filter(|(_, (_, e))| matches!(e, Entry::Verified(v) if wanted(&v.what)))
            .map(|(i, _)| i)
            .collect()
    };
    let raises = at(&|w| matches!(w, What::Raise { .. }));
    let effects = at(&|w| {
        matches!(
            w,
            What::Effect {
                action: Action::MoveValue { .. },
                ..
            }
        )
    });
    assert!(
        raises[0] < effects[1],
        "warned before the deposit was handed over"
    );
    // What arrives is the gross figure less the charge actually taken: 1 000 000 less 5 007.
    assert_eq!(handed_on(&segments[0]), amount("994993"));
}

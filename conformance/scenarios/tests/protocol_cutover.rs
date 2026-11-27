//! A provider still on an older protocol version across its cutover, over the assembled Core (`REQ-PRT-2`, `REQ-PRT-4`,
//! `THR-64`, `T-020-54`): its advertisement is taken before the date and survives a reopen, a segment begun before the
//! date resolves by the version it began under whenever its answer lands and however the cutover moves after, and a
//! reopened Core reads the segment's settled result as the live one did.

mod execution_support;
mod failure_support;

use execution_support::*;
use failure_support::*;
use rutent_conformance::sim::{Act, Behaviour};
use rutent_contracts::core_internal::OperatorSetting;
use rutent_contracts::router_api::{ProtocolVersion, Timestamp};
use rutent_core_orch::reversal::{Confirmation, NotCounted};
use rutent_core_orch::segments::SegmentOutcome;
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::harness::Scenario;
use rutent_scenarios::kit::{self, AMOUNT, RESERVE, START};

const OLD: &str = "3.0.0";
const IN: &str = "sim-join-in";
const EXCHANGE: &str = "sim-join-exchange";
const QUICK: &str = "sim-join-ramp-quick";
const OUT: &str = "sim-join-out";
const OUT_UNTRACKED: &str = "sim-join-out-untracked";

fn at(seconds: u64) -> String {
    rutent_prov::catalogue::Instant(START + seconds)
        .rfc3339()
        .expect("a time")
}

fn cutover(date: &str) -> OperatorSetting {
    OperatorSetting::Cutover {
        version: ProtocolVersion::new(OLD).expect("a version"),
        cutover: Timestamp::new(date).expect("a time"),
    }
}

/// An old-version provider's advertisement is taken before its cutover, its order runs, and its answer that lands
/// after the cutover for a segment begun before it is taken, the cutover moved earlier than the segment's start since
/// changing nothing; a reopened Core reads its settled withdrawal as it was read, and lists the provider again.
// verifies: REQ-PRT-2, REQ-PRT-4, THR-64
#[test]
fn old_version_provider_survives_its_cutover_and_a_reopen_by_the_version_its_segment_began_under() {
    // A fleet whose last reserve declares a safe repeat and no status query: the late answer of its deposit comes the
    // way Core takes one, by a decided repeat at the provider's status cadence, the platform holding the answer (`T-020-63`).
    let scenario = Scenario::new(
        "cutover",
        "AC-5",
        given(&[IN, EXCHANGE, QUICK, OUT_UNTRACKED]),
    )
    .holdout(OLD, &at(3_600));
    let (mut core, _) = scenario.core(1, &mut Reference);
    let session = core.open_session(&kit::customer()).expect("a session");
    assert!(
        core.catalogue()
            .accepted_advertisement(OUT_UNTRACKED)
            .is_some(),
        "the old version's advertisement was taken before its cutover"
    );
    core.world()
        .script_for(OUT_UNTRACKED, "deposit", Behaviour::late());
    let (order, ran) = run(
        &mut core,
        request(
            "req-cutover",
            &session,
            AMOUNT,
            IN,
            outer("sim-join-in-bank", "COP"),
            outer("sim-join-out-untracked-bank", "USD"),
            OUT_UNTRACKED,
            OTHER,
            "lowest_total_cost",
        ),
    );
    let segments = segments_of(&ran);
    assert!(
        matches!(segments[0].outcome, SegmentOutcome::Settled { .. }),
        "the withdrawal settled under the old version: {segments:?}"
    );
    assert!(
        matches!(segments[3].outcome, SegmentOutcome::Unknown(_)),
        "{segments:?}"
    );
    // The operator moves the cutover to before the segments began, and the clock passes it.
    core.set_setting(&cutover(&at(0)))
        .expect("the operator moves the cutover");
    core.clock.advance(7_200);
    core.set_status_cadence(OUT_UNTRACKED, 600);
    core.set_cadence(rutent_core_orch::schedule::Job::StuckReports, u64::MAX);
    // The late answer for a segment begun before the date lands after it, and is taken.
    let round = core.tick();
    assert_eq!(round.late.len(), 1, "one late answer: {:?}", round.late);
    assert!(round.late[0].kept.is_ok(), "{:?}", round.late[0]);
    // A reopened Core reads the settled withdrawal as the live one did, though the table now says the version is void.
    let withdrawal = segments[0].segment.clone();
    assert!(core.charged_of(&order, &withdrawal).is_some());
    let core = scenario.reopen(core, 1, &mut Reference);
    assert!(
        core.charged_of(&order, &withdrawal).is_some(),
        "the settled withdrawal's result is judged again by what its segment began under"
    );
    assert!(
        core.catalogue()
            .accepted_advertisement(OUT_UNTRACKED)
            .is_some(),
        "listed again"
    );
}

/// A public node the operator lists is public after a reopen too, and the reopen lists it before it judges the
/// advertisements again, as it stood when each was received (`REQ-CAT-23`, `T-020-49`, #346's review): the settings the
/// log holds are put back ahead of the relist, so a private claim of a node already listed is judged against the list it was
/// judged against live.
// verifies: REQ-AUZ-6
#[test]
fn operator_listed_public_node_stays_public_across_a_reopen() {
    let scenario = Scenario::new("listing", "AC-5", given(&[RESERVE]));
    let (mut core, _) = scenario.core(1, &mut Reference);
    let listing = |node: &str, listed: bool| -> OperatorSetting {
        serde_json::from_value(serde_json::json!({
            "setting": "public_node", "node": { "network": node }, "listed": listed,
        }))
        .expect("a setting")
    };
    core.set_setting(&listing("ledger-z", true))
        .expect("listed");
    core.set_setting(&listing("ledger-y", true))
        .expect("listed");
    core.set_setting(&listing("ledger-y", false))
        .expect("taken off");
    assert!(core.catalogue().nodes().is_public("ledger-z"));
    let core = scenario.reopen(core, 1, &mut Reference);
    assert!(
        core.catalogue().nodes().is_public("ledger-z"),
        "the node the operator listed stays public"
    );
    assert!(
        !core.catalogue().nodes().is_public("ledger-y"),
        "the node the operator took off stays off"
    );
    assert!(core.catalogue().accepted_advertisement(RESERVE).is_some());
}

/// A holdout fleet's exchange fails mid-route and the reversal is asked after the cutover: the failed provider's answer
/// and a reserve's confirmation, under the old version, are taken by the version the segment began under (`REQ-PRT-2`,
/// `REQ-PRT-4`, `T-020-54`). The simulated reserve guest signs under the fleet's version, as the platform's signer
/// requires of the nested confirmation it passes on.
// verifies: REQ-PRT-2, REQ-PRT-4
#[test]
fn reversal_of_a_segment_begun_before_a_cutover_is_taken_when_a_holdouts_answer_lands_after_it() {
    let scenario = Scenario::new("reversal", "AC-4", given(&[IN, EXCHANGE, QUICK, OUT]))
        .holdout(OLD, &at(600));
    let (mut core, _) = scenario.core(1, &mut Reference);
    let session = core.open_session(&kit::customer()).expect("a session");
    core.world()
        .script_for(EXCHANGE, "conversion", Behaviour::act(Act::Fail));
    let (order, ran) = run(
        &mut core,
        request(
            "req-reversal",
            &session,
            AMOUNT,
            IN,
            outer("sim-join-in-bank", "COP"),
            outer("sim-join-out-bank", "USD"),
            OUT,
            OTHER,
            "lowest_total_cost",
        ),
    );
    let segments = segments_of(&ran);
    assert!(
        matches!(segments[1].outcome, SegmentOutcome::Failed),
        "the exchange failed: {segments:?}"
    );
    let exchange = segments[1].segment.clone();
    // The date passes before the reversal is asked.
    core.clock.advance(900);
    core.set_cadence(rutent_core_orch::schedule::Job::StatusAsks, u64::MAX);
    core.set_cadence(rutent_core_orch::schedule::Job::StuckReports, u64::MAX);
    core.world().script(IN, Behaviour::crediting_a_reversal());
    let done = core.confirm_reversal(&order, &exchange).expect("asked");
    assert!(
        matches!(
            done,
            Confirmation::NotCompleted {
                why: NotCounted::SamePublisher,
                ..
            }
        ),
        "the holdout's confirmation is answered and judged, not refused for its version: {done:?}"
    );
    let asked = core.ask_reversal(&order, &exchange).expect("asked");
    assert!(
        asked.completed.is_some(),
        "the failed provider's answer under the old version is taken: {asked:?}"
    );
}

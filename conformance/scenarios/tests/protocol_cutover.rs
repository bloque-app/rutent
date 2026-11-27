//! A provider still on an older protocol version across its cutover, over the assembled Core (`REQ-PRT-2`, `REQ-PRT-4`,
//! `THR-64`, `T-020-54`): its advertisement is taken before the date and survives a reopen, a segment begun before the
//! date resolves by the version it began under whenever its answer lands and however the cutover moves after, and a
//! reopened Core reads the segment's settled result as the live one did.

mod execution_support;
mod failure_support;

use execution_support::*;
use failure_support::*;
use rutent_conformance::sim::Behaviour;
use rutent_contracts::core_internal::OperatorSetting;
use rutent_contracts::router_api::{ProtocolVersion, Timestamp};
use rutent_core_orch::segments::SegmentOutcome;
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::harness::Scenario;
use rutent_scenarios::kit::{self, AMOUNT, RESERVE, START};

const OLD: &str = "3.0.0";

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

/// A holdout fleet of one reserve, its cutover `after` seconds from the scenario's start: Core, the customer's session
/// and the scenario that reopens it.
fn holdout(
    after: u64,
) -> (
    rutent_scenarios::assembly::Core,
    rutent_contracts::router_api::Id,
    Scenario,
) {
    let scenario = Scenario::new("cutover", "AC-5", given(&[RESERVE])).holdout(OLD, &at(after));
    let (mut core, _) = scenario.core(1, &mut Reference);
    let session = core.open_session(&kit::customer()).expect("a session");
    (core, session, scenario)
}

/// An old-version provider's advertisement is taken before its cutover, its order runs, and its answer that lands
/// after the cutover for a segment begun before it is taken, the cutover moved earlier than the segment's start since
/// changing nothing; a reopened Core reads its settled withdrawal as it was read, and lists the provider again.
// verifies: REQ-PRT-2, REQ-PRT-4, THR-64
#[test]
fn old_version_provider_survives_its_cutover_and_a_reopen_by_the_version_its_segment_began_under() {
    let (mut core, session, scenario) = holdout(3_600);
    assert!(
        core.catalogue().accepted_advertisement(RESERVE).is_some(),
        "the old version's advertisement was taken before its cutover"
    );
    let order = accepted(
        &mut core,
        within("req-cutover", &session, AMOUNT, RESERVE, OTHER),
    );
    core.world()
        .script_for(RESERVE, "deposit", Behaviour::late());
    let ran = core.run(&order).expect("the order ran");
    let segments = segments_of(&ran);
    assert!(
        matches!(segments[0].outcome, SegmentOutcome::Settled { .. }),
        "the withdrawal settled under the old version: {segments:?}"
    );
    assert!(
        matches!(segments[1].outcome, SegmentOutcome::Unknown(_)),
        "{segments:?}"
    );
    // The operator moves the cutover to before the segments began, and the clock passes it.
    core.set_setting(&cutover(&at(0)))
        .expect("the operator moves the cutover");
    core.clock.advance(7_200);
    core.set_cadence(rutent_core_orch::schedule::Job::StatusAsks, u64::MAX);
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
        core.catalogue().accepted_advertisement(RESERVE).is_some(),
        "listed again"
    );
}

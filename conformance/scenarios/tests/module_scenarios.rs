//! Module scenarios over the assembled Core (`T-024-07`): one provider's module upgraded while orders run at others
//! (`AC-19`), a release that widens what its module may reach needing a fresh acceptance before it serves an order
//! (`AC-21`), and modules in two languages run sandboxed, reading nothing of another provider's (`AC-20`, `GUA-5`).
//!
//! The releases go to the platform Core's segments run on, admitted as any release is: signed by the provider's
//! advertising key, judged by the catalogue, compiled by the admitting compiler. Which version each segment began
//! and ran on is Core's own record in the log.

use rutent_conformance::sim::{Role, Spec};
use rutent_contracts::core_internal::{Action, CatalogueChange, Entry, Outcome, What};
use rutent_contracts::module_interface::AcceptanceMode;
use rutent_contracts::router_api::Id;
use rutent_contracts::state_machine::OrderState;
use rutent_core_orch::segments::PromoteRefused;
use rutent_prov::module::lifecycle::Staging;
use rutent_prov::module::types::Admitted;
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::releases::SegmentVersion;
use rutent_scenarios::assembly::{Core, Staged};
use rutent_scenarios::harness::{Given, Scenario, Step};
use rutent_scenarios::kit::{self, AMOUNT, RESERVE};
use rutent_scenarios::sim::Release;

const OTHER: &str = "sim-reserve-b";

/// Two simulated reserves, each of its own module and process. The scenarios' orders run at [`RESERVE`].
fn two_reserves() -> Given {
    two_reserves_under(Staged::AtOnce)
}

fn two_reserves_under(staging: Staged) -> Given {
    let mut given = Given::one_reserve(staging);
    given.providers.push(Spec::of(Role::Reserve, OTHER));
    given
}

/// Core with `n` orders accepted at [`RESERVE`], none of them run, and those orders.
fn accepted(given: Given, n: usize) -> (Core, Vec<Id>) {
    let mut scenario = Scenario::new("orders wait to run", "AC-28", given);
    for k in 0..n {
        scenario = scenario.when(Step::Order {
            request: format!("req-{k}"),
            amount: AMOUNT.to_owned(),
        });
    }
    let (core, orders) = scenario.core(1, &mut Reference);
    assert_eq!(orders.len(), n);
    (core, orders)
}

fn complete(core: &mut Core, order: &Id) -> bool {
    core.status(&kit::customer(), order)
        .expect("the customer reads its order")
        .state
        == OrderState::Complete
}

fn artefact(admitted: Admitted) -> Box<rutent_contracts::module_interface::AdmissionArtefact> {
    match admitted {
        Admitted::Artefact(a) => a,
        other => panic!("the release wasn't admitted: {other:?}"),
    }
}

/// The segments Core's log records at `provider`, as the module versions say.
fn at(core: &Core, provider: &str) -> Vec<SegmentVersion> {
    core.segment_versions()
        .into_iter()
        .filter(|s| s.provider == provider)
        .collect()
}

/// What `provider`'s books hold, as JSON.
fn books(core: &mut Core, provider: &str) -> serde_json::Value {
    serde_json::to_value(core.world().journal(provider)).expect("a journal is JSON")
}

// --- AC-19 -----------------------------------------------------------------------------------------------------------

/// A provider's module upgraded while an order waits at another: the upgrade is the provider's alone, the other's
/// process and version stay as they were, and its order runs to the end exactly as it does with no upgrade at all. A
/// release that can't be admitted leaves the version in force serving, in its process, and the next order at that
/// provider runs on it.
fn upgrade_at_once() {
    // The control: the same order, with no upgrade anywhere.
    let (mut control, orders) = accepted(two_reserves(), 2);
    control.run(&orders[0]).expect("the control order runs");
    assert!(complete(&mut control, &orders[0]));
    let control_books = books(&mut control, RESERVE);
    assert!(
        !control_books.as_array().expect("entries").is_empty(),
        "the control order moved value"
    );

    let (mut core, orders) = accepted(two_reserves(), 2);
    let a_before = core.fleet().platform.process(RESERVE).expect("A runs");
    let b_before = core.fleet().platform.process(OTHER).expect("B runs");
    assert_eq!(
        core.fleet().platform.active_version(RESERVE).as_deref(),
        Some("1.0.0")
    );

    // B's module is upgraded, a release that widens nothing: accepted at once, and in force.
    let upgraded = artefact(core.publish_release(OTHER, &Release::at("1.1.0")));
    assert_eq!(upgraded.acceptance.required, AcceptanceMode::Automatic);
    assert!(upgraded.acceptance.accepted.is_some(), "accepted at once");
    assert_eq!(
        core.fleet().platform.active_version(OTHER).as_deref(),
        Some("1.1.0")
    );
    assert_ne!(
        core.fleet().platform.process(OTHER),
        Some(b_before),
        "B's module is another process: its new version"
    );
    // A's module and process are as they were.
    assert_eq!(core.fleet().platform.process(RESERVE), Some(a_before));
    assert_eq!(
        core.fleet().platform.active_version(RESERVE).as_deref(),
        Some("1.0.0")
    );

    // The order at A, accepted before B's upgrade, runs to the end as the control does: the same books, the same
    // end, and every one of its segments on A's version 1.0.0 from its start to its end.
    core.run(&orders[0]).expect("the order at A runs");
    assert!(complete(&mut core, &orders[0]), "the order at A completed");
    assert_eq!(
        books(&mut core, RESERVE),
        control_books,
        "A's books as with no upgrade"
    );
    let ran = at(&core, RESERVE);
    assert!(!ran.is_empty(), "A's segments are recorded");
    assert!(
        ran.iter()
            .all(|s| s.began_on == "1.0.0" && s.runs_on == "1.0.0"),
        "{ran:?}"
    );
    assert!(
        books(&mut core, OTHER)
            .as_array()
            .expect("entries")
            .is_empty()
    );

    // A failed upgrade of A: a release whose sandboxed part is no component is refused, and the version in force
    // stays, in its own process.
    let refused = core.publish_release(RESERVE, &Release::at("1.2.0").corrupt());
    assert!(
        !matches!(refused, Admitted::Artefact(_)),
        "a release that can't be compiled isn't admitted: {refused:?}"
    );
    assert_eq!(
        core.fleet().platform.active_version(RESERVE).as_deref(),
        Some("1.0.0")
    );
    assert_eq!(core.fleet().platform.process(RESERVE), Some(a_before));
    assert_eq!(
        core.fleet().platform.version_process(RESERVE, "1.2.0"),
        None,
        "the failed release never ran"
    );

    // The next order at A, after the failed upgrade, runs on the version that stayed.
    core.run(&orders[1]).expect("the next order at A runs");
    assert!(complete(&mut core, &orders[1]));
    let ran = at(&core, RESERVE);
    assert!(
        ran.iter()
            .all(|s| s.began_on == "1.0.0" && s.runs_on == "1.0.0"),
        "{ran:?}"
    );
}

/// The same under staging (`ADR-0014`): a release is staged beside the version in force, takes no segment of the
/// others and puts none of theirs out; it is put in force by the operator's promotion, which Core's policy decides and
/// the log records, naming the version staged and no other; and a release that can't be admitted is never staged.
fn upgrade_staged() {
    let staging = Staging::new(1, 0, 0).expect("a staging policy");
    let (mut core, orders) = accepted(two_reserves_under(Staged::Under(staging)), 1);
    let a_before = core.fleet().platform.process(RESERVE).expect("A runs");
    let staged = artefact(core.publish_release(OTHER, &Release::at("1.1.0")));
    assert!(staged.acceptance.accepted.is_some(), "accepted, and staged");
    assert_eq!(
        core.fleet().platform.lifecycle().staged(OTHER),
        Some("1.1.0")
    );
    assert_eq!(
        core.fleet().platform.active_version(OTHER).as_deref(),
        Some("1.0.0"),
        "the staged release isn't in force"
    );

    // The order at A runs on, on A's own version, with B's staged release beside B's.
    core.run(&orders[0]).expect("the order at A runs");
    assert!(complete(&mut core, &orders[0]));
    assert!(
        at(&core, RESERVE)
            .iter()
            .all(|s| s.began_on == "1.0.0" && s.runs_on == "1.0.0")
    );
    assert_eq!(core.fleet().platform.process(RESERVE), Some(a_before));

    // A promotion names the version staged: a permit to promote one release promotes no other.
    assert!(matches!(
        core.promote(OTHER, "1.0.5"),
        Err(PromoteRefused::NotTheStagedVersion { .. })
    ));
    assert_eq!(core.promote(OTHER, "1.1.0"), Ok("1.1.0".to_owned()));
    assert_eq!(
        core.fleet().platform.active_version(OTHER).as_deref(),
        Some("1.1.0")
    );
    assert_eq!(core.fleet().platform.process(RESERVE), Some(a_before));
    let promoted: Vec<String> = core
        .log()
        .entries()
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(v) => match &v.what {
                What::Effect {
                    action:
                        Action::ChangeCatalogue {
                            change: CatalogueChange::Promote,
                            provider,
                            ..
                        },
                    ..
                } => Some(provider.as_str().to_owned()),
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .collect();
    assert_eq!(
        promoted,
        vec![OTHER.to_owned()],
        "one promotion, recorded before it was carried out"
    );

    // A release that can't be admitted is never staged at A, and A keeps the version in force.
    let refused = core.publish_release(RESERVE, &Release::at("1.2.0").corrupt());
    assert!(!matches!(refused, Admitted::Artefact(_)), "{refused:?}");
    assert_eq!(core.fleet().platform.lifecycle().staged(RESERVE), None);
    assert_eq!(
        core.fleet().platform.active_version(RESERVE).as_deref(),
        Some("1.0.0")
    );
    assert_eq!(core.fleet().platform.process(RESERVE), Some(a_before));
}

/// A provider's module upgraded while an order waits at another: the upgrade is the provider's alone, the other's
/// process and version stay as they were, and its order runs to the end exactly as it does with no upgrade at all. A
/// release that can't be admitted leaves the version in force serving, in its process, and the next order at that
/// provider runs on it. Both with an upgrade that takes effect at once and with one staged and promoted.
// verifies: AC-19, REQ-MDL-13, REQ-MDL-4
#[test]
fn upgrade_of_one_provider_interrupts_no_order_at_another_and_a_failed_one_leaves_the_previous_version()
 {
    upgrade_at_once();
    upgrade_staged();
}

/// A staged release rolled back (`T-021-12`, `T-020-42`, ADR-0014): the operator's login asks, policy decides, and a
/// permitted roll back ends the provider's staging, keeping the version in force, with its effect recorded before
/// its result. A refused one (a login the register doesn't list for it, or a version that isn't the one staged) moves
/// nothing and records no effect; each is asked of policy first, before the staging is read (`F-025`'s L6,
/// `T-020-49`), so each attempt is a decision of the log's.
// verifies: REQ-AUZ-6
#[test]
fn a_permitted_roll_back_ends_the_staging_and_a_refused_one_moves_nothing() {
    use rutent_core_orch::segments::roll_back::RollBackRefused;

    let staging = Staging::new(1, 0, 0).expect("a staging policy");
    let (mut core, _) = accepted(two_reserves_under(Staged::Under(staging)), 1);
    artefact(core.publish_release(OTHER, &Release::at("1.1.0")));
    assert_eq!(
        core.fleet().platform.lifecycle().staged(OTHER),
        Some("1.1.0")
    );
    let recorded = |core: &Core| -> Vec<String> {
        core.log()
            .entries()
            .iter()
            .filter_map(|(_, e)| match e {
                Entry::Verified(v) => match &v.what {
                    What::Effect {
                        action:
                            Action::ChangeCatalogue {
                                change: CatalogueChange::RollBack,
                                ..
                            },
                        ..
                    } => Some("effect".to_owned()),
                    What::Decide { request, .. }
                        if matches!(
                            request.action,
                            Action::ChangeCatalogue {
                                change: CatalogueChange::RollBack,
                                ..
                            }
                        ) =>
                    {
                        Some("decide".to_owned())
                    }
                    _ => None,
                },
                Entry::Stated(_) => None,
            })
            .collect()
    };

    // Refused: a customer is no operator's login, and 1.0.5 isn't the version staged. Nothing moves.
    assert!(matches!(
        core.roll_back_as(&kit::customer(), OTHER, "1.1.0"),
        Err(RollBackRefused::NotPermitted(_))
    ));
    assert!(matches!(
        core.roll_back(OTHER, "1.0.5"),
        Err(RollBackRefused::NotTheStagedVersion { .. })
    ));
    assert_eq!(
        core.fleet().platform.lifecycle().staged(OTHER),
        Some("1.1.0"),
        "the staging stays"
    );
    assert_eq!(
        recorded(&core),
        ["decide", "decide"],
        "only the two decisions are recorded"
    );

    // Permitted: the staging ends, the version in force stays, and the effect is recorded.
    assert_eq!(core.roll_back(OTHER, "1.1.0"), Ok("1.1.0".to_owned()));
    assert_eq!(core.fleet().platform.lifecycle().staged(OTHER), None);
    assert_eq!(
        core.fleet().platform.active_version(OTHER).as_deref(),
        Some("1.0.0")
    );
    assert_eq!(recorded(&core), ["decide", "decide", "decide", "effect"]);
    let effect_at = core
        .log()
        .entries()
        .iter()
        .find_map(|(r, e)| match e {
            Entry::Verified(v)
                if matches!(
                    &v.what,
                    What::Effect {
                        action: Action::ChangeCatalogue {
                            change: CatalogueChange::RollBack,
                            ..
                        },
                        ..
                    }
                ) =>
            {
                Some(r.clone())
            }
            _ => None,
        })
        .expect("the roll back's effect");
    assert!(
        core.log()
            .entries()
            .iter()
            .any(|(_, e)| matches!(e, Entry::Verified(v)
            if v.outcome == Outcome::Done {}
                && matches!(&v.what, What::Result { effect } if *effect == effect_at))),
        "the roll back effect's result is done"
    );
    assert_eq!(
        core.roll_back(OTHER, "1.1.0"),
        Err(RollBackRefused::NothingStaged),
        "nothing is staged now"
    );
}

// --- AC-21 -----------------------------------------------------------------------------------------------------------

/// A release that widens what its module may reach is not in force until someone accepts it afresh: it is admitted
/// and waits, its widening named; no process of its version runs; the orders that run meanwhile run on the version
/// in force. Once it is accepted it serves the next order.
// verifies: AC-21, REQ-MDL-3
#[test]
fn widened_release_needs_fresh_acceptance_before_it_serves_an_order() {
    let (mut core, orders) = accepted(Given::one_reserve(Staged::AtOnce), 3);
    let widened = artefact(core.publish_release(RESERVE, &Release::at("1.1.0").widened()));
    assert_eq!(widened.acceptance.required, AcceptanceMode::Fresh);
    assert!(widened.acceptance.accepted.is_none(), "it waits");
    assert!(
        !widened.capabilities.widenings.is_empty(),
        "the widening is named"
    );
    assert_eq!(
        widened
            .capabilities
            .compared_with
            .as_ref()
            .map(|v| v.as_str()),
        Some("1.0.0")
    );
    assert_eq!(
        core.fleet().platform.active_version(RESERVE).as_deref(),
        Some("1.0.0"),
        "the version in force stays while the release waits"
    );
    assert_eq!(
        core.fleet().platform.version_process(RESERVE, "1.1.0"),
        None,
        "no process of the waiting release runs"
    );

    // The orders that run while it waits run on the version in force, and complete.
    for order in &orders[..2] {
        core.run(order).expect("the order runs");
        assert!(complete(&mut core, order));
    }
    let ran = at(&core, RESERVE);
    assert!(!ran.is_empty());
    assert!(
        ran.iter()
            .all(|s| s.began_on == "1.0.0" && s.runs_on == "1.0.0"),
        "{ran:?}"
    );

    // A second release can't be admitted over the waiting one: it serves nothing either.
    assert!(!matches!(
        core.publish_release(RESERVE, &Release::at("1.2.0")),
        Admitted::Artefact(_)
    ));

    // Accepted afresh, the release is in force, and the next order runs on it.
    let accepted = core
        .accept_waiting_release(RESERVE, &widened)
        .expect("a waiting release is accepted");
    assert!(accepted.acceptance.accepted.is_some());
    assert_eq!(
        core.fleet().platform.active_version(RESERVE).as_deref(),
        Some("1.1.0")
    );
    let before = at(&core, RESERVE).len();
    core.run(&orders[2]).expect("the order runs");
    assert!(complete(&mut core, &orders[2]));
    let after = at(&core, RESERVE);
    assert!(after.len() > before);
    assert!(
        after[before..]
            .iter()
            .all(|s| s.began_on == "1.1.0" && s.runs_on == "1.1.0"),
        "{after:?}"
    );
}

// --- AC-20 -----------------------------------------------------------------------------------------------------------

use rutent_contracts::module_interface::{PartName, SecretRefusalReason};
use rutent_prov::module::platform::World as _;
use rutent_prov::module::types::{Handled, LookupOutcome};
use rutent_scenarios::sim::probes::{LANGUAGES, Reach, SHARED_SECRET, SHARED_STATE};

/// What reserve B holds as its own: nobody else may read either.
const CANARY_SECRET: &str = "canary-secret-3c1f77d0-held-by-the-other-provider";
const CANARY_STATE: &str = "canary-state-9a42e6b8-held-by-the-other-provider";

/// A module from each of two languages, the Rust stand-in and the C one, admitted beside two simulated reserves and
/// handed an operation that makes each of its two parts try to read what another provider holds: a secret and a state
/// entry, each under a name both providers use. Both run in the platform's sandbox and answer; neither part obtains
/// either canary, by any channel the module has (its answer, the platform's records, the secrets the world handed
/// out, a request that reached an endpoint, Core's log); the sandboxed part's secret lookup never reaches the
/// proxy; the outside part's is refused as not found, since the probe holds none by that name. The order at the
/// simulated reserve runs on to its end meanwhile, and the other provider's secret is there to be had by its own
/// lookup, so what the probes didn't get was there. Each module is a process of its own.
///
/// What is shown on every platform is the WebAssembly sandbox in the platform's engine: no import gives a part
/// another provider's state or secrets, and the platform judges each host call. What only Linux shows is the
/// operating system's jail around the provider's process (Landlock, a `seccomp` filter, no capabilities), which
/// `T-025-02`'s jail probes job covers; nothing here depends on it.
// verifies: AC-20, GUA-5, REQ-MDL-15, REQ-MDL-9
#[test]
fn modules_in_rust_and_in_c_run_sandboxed_and_read_no_other_providers_state_or_secrets() {
    let (mut core, orders) = accepted(two_reserves(), 1);
    core.world()
        .seed_secret(OTHER, SHARED_SECRET, CANARY_SECRET);
    core.world().seed_state(OTHER, SHARED_STATE, CANARY_STATE);
    let reaches: Vec<Reach> = [PartName::Sandboxed, PartName::Outside]
        .into_iter()
        .flat_map(|part| {
            [
                Reach::Secret(part, SHARED_SECRET.to_owned()),
                Reach::State(part, SHARED_STATE.to_owned()),
            ]
        })
        .collect();

    let mut processes = Vec::new();
    for language in LANGUAGES {
        let probe = core.install_probe(language, &reaches);
        processes.push(
            core.fleet()
                .platform
                .process(&probe.provider)
                .expect("it runs"),
        );
        let seen = core.handle_probe(&probe, 1);

        // It ran, in the sandbox, and answered: its result is signed by the platform with the key it declares.
        assert!(
            matches!(seen.handled, Handled::Answered(_)),
            "{language}: the module answered: {:?}",
            seen.handled
        );
        // The sandboxed part's lookup never reached the proxy; the outside part's did, and was told there is no such
        // secret of its own: not that another's is refused, which would say it exists.
        let lookups = &seen.events.lookups;
        assert!(
            lookups
                .iter()
                .any(|l| l.part == PartName::Sandboxed && l.outcome.is_none()),
            "{language}: the sandboxed part's lookup was stopped before the proxy: {lookups:?}"
        );
        assert!(
            lookups
                .iter()
                .all(|l| l.part != PartName::Sandboxed || l.outcome.is_none()),
            "{language}: no sandboxed lookup reached the proxy: {lookups:?}"
        );
        assert!(
            lookups.iter().any(|l| l.part == PartName::Outside
                && l.outcome == Some(LookupOutcome::Refused(SecretRefusalReason::NotFound))),
            "{language}: the outside part's lookup found nothing: {lookups:?}"
        );
        // Each part's read of the shared state name found nothing: the platform asks the world for the probe's own
        // entry, which holds none.
        let reads = &seen.events.state_reads;
        for p in [PartName::Sandboxed, PartName::Outside] {
            assert!(
                reads.iter().any(|r| r.part == p && !r.found),
                "{language}: the {p:?} part's state read found nothing: {reads:?}"
            );
        }
        assert!(reads.iter().all(|r| !r.found), "{language}: {reads:?}");

        // The canaries are in no channel the module has.
        let channels = format!(
            "{:?}{:?}{:?}{:?}",
            seen.handled, seen.events.records, seen.events.calls, seen.events.lookups
        );
        for canary in [CANARY_SECRET, CANARY_STATE] {
            assert!(
                !channels.contains(canary),
                "{language}: {canary} leaked into {channels}"
            );
        }
        assert!(
            core.world()
                .secrets_delivered()
                .iter()
                .all(|(provider, _)| provider != &probe.provider),
            "{language}: the world handed the probe no secret"
        );
    }
    // One process for each module, apart from the simulated providers' two and from each other.
    processes.push(core.fleet().platform.process(RESERVE).expect("A runs"));
    processes.push(core.fleet().platform.process(OTHER).expect("B runs"));
    processes.sort();
    processes.dedup();
    assert_eq!(processes.len(), 4, "a process for each provider");

    // The other provider's secret was there to be had: its own lookup is answered with it. That is the control
    // that makes the probes' empty hands mean something.
    assert_eq!(
        core.world().secret(OTHER, SHARED_SECRET).as_deref(),
        Some(CANARY_SECRET)
    );
    assert!(
        core.world()
            .secrets_delivered()
            .iter()
            .all(|(provider, value)| provider == OTHER && value == CANARY_SECRET),
        "only the owner was ever handed it"
    );

    // Core's order at the simulated reserve ran on meanwhile, and no request, book entry or record of Core's holds a
    // canary.
    core.run(&orders[0]).expect("the order runs");
    assert!(complete(&mut core, &orders[0]));
    let book = books(&mut core, RESERVE);
    let delivered = format!("{:?}", core.world().delivered());
    let held = format!(
        "{delivered}{book:?}{:?}",
        core.log()
            .entries()
            .iter()
            .map(|(_, e)| serde_json::to_string(e).expect("an entry is JSON"))
            .collect::<Vec<_>>()
    );
    for canary in [CANARY_SECRET, CANARY_STATE] {
        assert!(!held.contains(canary), "{canary} reached what Core keeps");
    }
}

// --- T-020-49: a release's lifecycle actions through policy ----------------------------------------------------------

/// The effects and results Core's log holds of `change`, each with its outcome.
fn changes_of(core: &Core, change: CatalogueChange) -> Vec<Outcome> {
    let entries = core.log().entries().to_vec();
    entries
        .iter()
        .filter_map(|(receipt, e)| match e {
            Entry::Verified(v) => match &v.what {
                What::Effect {
                    action: Action::ChangeCatalogue { change: c, .. },
                    ..
                } if *c == change => entries.iter().find_map(|(_, r)| match r {
                    Entry::Verified(r) => match &r.what {
                        What::Result { effect } if effect == receipt => Some(r.outcome),
                        _ => None,
                    },
                    Entry::Stated(_) => None,
                }),
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .collect()
}

/// An operator who must declare a version exploited has a permitted way (`REQ-MDL-20`, `T-020-49`): its login asks
/// Core's decided path, policy permits it, the effect is recorded before the platform is asked and its result after, and
/// the platform marks the version exploited and runs nothing of it: the order waiting at the provider runs no segment
/// on it, and nothing moves at the provider. A login the register doesn't list for it is refused, and nothing is
/// marked.
// verifies: REQ-MDL-20
#[test]
fn operator_declares_a_version_exploited_through_policy_and_its_segments_stop_on_it() {
    use rutent_core_orch::segments::lifecycle::{Changed, ReleaseChange, ReleaseRefused};
    use rutent_prov::module::lifecycle::Mark;
    let (mut core, orders) = accepted(two_reserves(), 1);
    let version = kit::id("1.0.0");
    assert!(matches!(
        core.change_release_as(
            &kit::customer(),
            RESERVE,
            ReleaseChange::DeclareExploited { version: &version }
        ),
        Err(ReleaseRefused::NotPermitted(_))
    ));
    assert_eq!(
        core.fleet().platform.lifecycle().mark(RESERVE, "1.0.0"),
        None
    );
    assert!(changes_of(&core, CatalogueChange::DeclareExploited).is_empty());

    let declared = core
        .change_release(
            RESERVE,
            ReleaseChange::DeclareExploited { version: &version },
        )
        .expect("permitted and carried out");
    assert!(
        matches!(declared, Changed::DeclaredExploited(_)),
        "{declared:?}"
    );
    assert_eq!(
        core.fleet().platform.lifecycle().mark(RESERVE, "1.0.0"),
        Some(Mark::Exploited)
    );
    assert_eq!(
        changes_of(&core, CatalogueChange::DeclareExploited),
        [Outcome::Done {}],
        "one effect, its result done"
    );
    assert_eq!(
        core.fleet().platform.version_process(RESERVE, "1.0.0"),
        None,
        "stopped"
    );

    // Its segments stop on it: the order at the provider moves nothing there.
    let before = books(&mut core, RESERVE);
    let _ = core.run(&orders[0]);
    assert!(!complete(&mut core, &orders[0]));
    assert_eq!(
        books(&mut core, RESERVE),
        before,
        "nothing moved at the exploited version"
    );
    assert!(at(&core, RESERVE).iter().all(|s| s.runs_on != "1.0.0"));
}

/// A publisher can't mark its own version stale (`REQ-MDL-5`, `T-020-49`): its login, in the provider's role or stating
/// the operator's, asks Core's decided path and policy refuses it; nothing is marked, no effect is recorded, and each
/// attempt is the decision's entry, naming the publisher's login. The operator's login is permitted.
// verifies: REQ-MDL-5
#[test]
fn publisher_cannot_mark_its_own_version_stale() {
    use rutent_contracts::core_internal::{Actor, ActorKind, Role};
    use rutent_core_orch::segments::lifecycle::{ReleaseChange, ReleaseRefused};
    let staging = Staging::new(1, 0, 0).expect("a staging policy");
    let (mut core, _) = accepted(two_reserves_under(Staged::Under(staging)), 1);
    let ad = core
        .installed
        .iter()
        .find(|i| i.advertisement.statement().statement.provider == RESERVE)
        .map(|i| i.advertisement.statement().clone())
        .expect("installed");
    let publisher = Actor {
        party: kit::id(&ad.statement.publisher),
        acts: kit::id(&ad.maker),
        role: Role::Provider,
        kind: ActorKind::Component,
    };
    let stating_the_operator = Actor {
        role: Role::Operator,
        ..publisher.clone()
    };
    let version = kit::id("1.0.0");
    for caller in [&publisher, &stating_the_operator] {
        let before = core.log().len();
        assert!(
            matches!(
                core.change_release_as(
                    caller,
                    RESERVE,
                    ReleaseChange::MarkStale { version: &version }
                ),
                Err(ReleaseRefused::NotPermitted(_))
            ),
            "{caller:?}"
        );
        assert_eq!(
            core.log().len(),
            before + 1,
            "the attempt is recorded, and nothing else"
        );
        let entries = core.log().entries().to_vec();
        let Some((_, Entry::Verified(decided))) = entries.last() else {
            panic!("a decision");
        };
        assert!(matches!(&decided.what, What::Decide { request, .. } if &request.caller == caller));
        assert_eq!(&decided.who, caller);
    }
    assert_eq!(
        core.fleet().platform.lifecycle().mark(RESERVE, "1.0.0"),
        None
    );
    assert!(changes_of(&core, CatalogueChange::MarkStale).is_empty());
}

//! A route's calls filled from the router's decision (`T-011-06`, `REQ-MDL-21`, `REQ-MDL-9`, `THR-86`): the
//! platform takes what a route names from a router-api decision record (`RouteCalls::name_segment`), as Core will,
//! and refuses a declared call that no segment of the chosen route names.

#[path = "module_support/mod.rs"]
mod common;

use common::{WasmBinding, binding, declaration, install, languages, withdrawal};
use rutent_conformance::boundary::{ModuleSpec, Output, Step};
use rutent_contracts::module_interface::{
    CallRefusalReason, CrossModuleOutcome, Declaration, Identifier, OperationKind, RouteReference,
    validate_call, validate_call_against,
};
use rutent_contracts::router_api::{DecisionRecord, RecordedDecision};
use rutent_prov::module::platform::Platform;
use rutent_prov::module::types::{Events, Handled};
use rutent_prov::proxy::RouteNotNamed;
use serde_json::json;

fn id(text: &str) -> Identifier {
    Identifier::new(text).expect("an identifier")
}

fn route(record: &str, segment: &str) -> RouteReference {
    RouteReference {
        decision_record: id(record),
        segment: id(segment),
    }
}

/// `provider`'s declaration, handling withdrawals, that may call each of `callees` for withdrawals.
fn calling(provider: &str, callees: &[&str]) -> Declaration {
    let mut value =
        serde_json::to_value(declaration(provider, "1.0.0", &["withdrawal"])).expect("JSON");
    value["outside"]["calls"] = callees
        .iter()
        .map(|c| json!({ "callee": c, "kinds": ["withdrawal"] }))
        .collect();
    serde_json::from_value(value).expect("a valid declaration")
}

fn call(callee: &str) -> Step {
    Step::CallModule {
        callee: callee.to_owned(),
        kind: OperationKind::Withdrawal,
    }
}

/// A module of `declaration`'s provider taking `steps`.
fn module(declaration: Declaration, steps: Vec<Step>) -> ModuleSpec {
    ModuleSpec {
        declaration,
        steps,
        output: Output::Honest,
        starts: true,
    }
}

/// Hands `provider` its withdrawal `n` on the platform itself, and returns how it ended and what it set off.
fn run(binding: &mut WasmBinding, provider: &str, n: usize) -> (Handled, Events) {
    let platform: &mut Platform<_> = binding;
    platform.take_events();
    let handled = platform.handle(provider, &withdrawal(provider, n));
    (handled, platform.take_events())
}

/// A router-api decision record fixture: the choice round over two ramps that `rutent-router`'s own tests show is
/// that router's record over its inputs (`T-011-06`). Its chosen route is `p-r1`'s segment, then `p-r2`'s; `p-r1`'s
/// segment names its module's calls to `p-r2`, and not its declared call to `p-z`, which is on no segment.
fn decision(name: &str) -> DecisionRecord {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/router-api/fixtures/valid")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let recorded: RecordedDecision = serde_json::from_str(&text).expect("a valid decision record");
    recorded.statement
}

// verifies: REQ-MDL-9, REQ-MDL-21
#[test]
fn call_a_route_doesnt_name_is_refused_when_the_route_comes_from_the_router() {
    let routed = decision("decision-record-naming-the-calls-each-segment-makes");
    let shortlist = decision("decision-record-shortlist-round");
    let no_route = decision("decision-record-with-no-route");
    let mut broken = routed.clone();
    broken.chosen = Some(rutent_contracts::router_api::Id::new("c999").expect("an id"));
    for language in languages() {
        let mut binding = binding("reserve");
        for callee in ["p-r2", "p-z"] {
            install(
                &mut binding,
                &module(declaration(callee, "1.0.0", &["withdrawal"]), Vec::new()),
                &language,
                "reserve",
                &[],
            );
        }
        // `p-r1` declares both calls, and makes both: to `p-r2`, on its route, then to `p-z`, on none.
        let declared = calling("p-r1", &["p-r2", "p-z"]);
        install(
            &mut binding,
            &module(declared.clone(), vec![call("p-r2"), call("p-z")]),
            &language,
            "reserve",
            &[],
        );
        let segment = id("segment-1");
        let transaction = id("tx-p-r1-1");
        // Filled from the router's decision: `p-r1`'s segment, the chosen candidate's first operation.
        assert_eq!(
            binding
                .routes
                .name_segment(&transaction, &routed, (0, 0), &segment),
            Ok(1),
            "{language}: the segment's one callee isn't named"
        );
        let before = (
            binding.operations_received("p-r2"),
            binding.operations_received("p-z"),
        );
        let (_, events) = run(&mut binding, "p-r1", 1);
        assert_eq!(events.calls.len(), 2, "{language}: {:?}", events.calls);
        for record in &events.calls {
            assert_eq!(validate_call(record), Ok(()), "{language}: {record:?}");
            assert_eq!(
                validate_call_against(&declared, record),
                Ok(()),
                "{language}: {record:?}"
            );
        }
        let (named, unnamed) = (&events.calls[0], &events.calls[1]);
        assert!(
            matches!(named.outcome, CrossModuleOutcome::Delivered(_)),
            "{language}: the call the route names wasn't delivered: {:?}",
            named.outcome
        );
        assert_eq!(named.route, route("r-calls", "segment-1"), "{language}");
        assert!(
            matches!(
                &unnamed.outcome,
                CrossModuleOutcome::Refused(r) if r.reason == CallRefusalReason::NotNamedByRoute
            ),
            "{language}: a declared call the route doesn't name wasn't refused: {:?}",
            unnamed.outcome
        );
        assert_eq!(
            (
                binding.operations_received("p-r2"),
                binding.operations_received("p-z")
            ),
            (before.0 + 1, before.1),
            "{language}: the callees received other than the named call"
        );
        binding.routes.end(&transaction);
        // `p-r2`'s segment names no call of `p-r1`'s, and a shortlist round, which chooses nothing, names nothing:
        // the route's calls come from the chosen candidate only. Both of `p-r1`'s calls are then refused.
        let transaction = id("tx-p-r1-2");
        assert_eq!(
            binding
                .routes
                .name_segment(&transaction, &routed, (0, 1), &id("segment-2")),
            Ok(0),
            "{language}: p-r2's segment names a call"
        );
        assert_eq!(
            binding
                .routes
                .name_segment(&transaction, &shortlist, (0, 0), &segment),
            Err(RouteNotNamed::NothingChosen),
            "{language}: a shortlist round named a call"
        );
        assert_eq!(
            binding
                .routes
                .name_segment(&transaction, &routed, (0, 2), &segment),
            Err(RouteNotNamed::NoSuchSegment),
            "{language}: a segment past the route named a call"
        );
        // A choice round with no route chooses nothing, and a record that breaks its contract (here, choosing a
        // candidate it doesn't hold) is refused before anything in it is read.
        assert_eq!(
            binding
                .routes
                .name_segment(&transaction, &no_route, (0, 0), &segment),
            Err(RouteNotNamed::NothingChosen),
            "{language}: a choice round with no route named a call"
        );
        assert!(
            matches!(
                binding
                    .routes
                    .name_segment(&transaction, &broken, (0, 0), &segment),
                Err(RouteNotNamed::Invalid(_))
            ),
            "{language}: a record that breaks its contract named a call"
        );
        let (_, events) = run(&mut binding, "p-r1", 2);
        assert_eq!(events.calls.len(), 2, "{language}: {:?}", events.calls);
        for record in &events.calls {
            assert!(
                matches!(
                    &record.outcome,
                    CrossModuleOutcome::Refused(r) if r.reason == CallRefusalReason::NotNamedByRoute
                ),
                "{language}: a call no chosen segment names wasn't refused: {:?}",
                record.outcome
            );
        }
        binding.routes.end(&transaction);
    }
}

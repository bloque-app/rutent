//! What the binding's world says besides a bare call (`T-013-07`): a secret lookup tells a store that is down from a
//! secret that isn't there, and an outbound call carries a whole request, which the egress layer checks hop by hop.

#[path = "module_support/mod.rs"]
mod common;

use std::net::{IpAddr, Ipv4Addr};

use common::{Probe, binding, install, languages, withdrawal};
use rutent_conformance::boundary::{
    BoundaryHarness, Endpoint, Handled, ModuleBinding, ModuleSpec, Output,
};
use rutent_conformance::signing::{public_key, statements_key};
use rutent_contracts::module_interface::{
    CallOutcome, Declaration, RefusalReason, Refused, validate_record,
};
use serde_json::json;

/// A provider with one outside operation, `api.call`, reaching `api` and `other` and holding the secrets `down.key`
/// and `missing.key`.
fn declaration(provider: &str) -> Declaration {
    let destination = |name: &str| {
        json!({
            "name": name, "scheme": "https", "host": format!("{name}.{provider}.example"), "port": 443,
            "addresses": "public",
        })
    };
    serde_json::from_value(json!({
        "provider": provider,
        "version": "1.0.0",
        "interface": rutent_contracts::module_interface::VERSION,
        "sandboxed": { "handles": ["withdrawal"] },
        "outside": {
            "operations": [{
                "name": "api.call", "destinations": ["api", "other"],
                "secrets": ["down.key", "missing.key"],
            }],
            "destinations": [destination("api"), destination("other")],
            "secrets": ["down.key", "missing.key"],
            "calls": [],
            "statement_key": public_key(&statements_key(provider)),
        },
    }))
    .expect("a valid declaration")
}

fn spec(provider: &str) -> ModuleSpec {
    ModuleSpec {
        declaration: declaration(provider),
        steps: Vec::new(),
        output: Output::Honest,
        starts: true,
    }
}

/// The evidence reference of the module's answer, which names each value its outside part obtained.
fn reference(handled: Handled, provider: &str) -> String {
    let Handled::Answered(result) = handled else {
        panic!("{provider} didn't answer")
    };
    serde_json::to_value(&result.statement).expect("JSON")["outcome"]["evidence"][0]["reference"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

// verifies: REQ-MDL-17
#[test]
fn guest_tells_an_unreachable_secret_store_from_a_missing_secret() {
    for language in languages() {
        let mut binding = binding("reserve");
        let provider = format!("store-{language}");
        binding.world.make_store_unavailable(&provider, "down.key");
        let probes = ["down.key", "missing.key", "undeclared.key"].map(|name| Probe::Reason {
            operation: "api.call".to_owned(),
            name: name.to_owned(),
        });
        install(
            &mut binding,
            &spec(&provider),
            &language,
            "reserve",
            &probes,
        );
        let reference = reference(
            binding.handle(&provider, &withdrawal(&provider, 1)),
            &provider,
        );
        assert_eq!(
            reference,
            format!("{provider}-1.0.0-store-unavailable-not-found-undeclared-secret"),
            "{language}: the guest didn't get a reason of its own for each refusal"
        );
    }
}

// verifies: REQ-MDL-6
#[test]
fn outbound_requests_method_path_and_headers_reach_only_its_declared_destination() {
    for language in languages() {
        let mut binding = binding("reserve");
        let provider = format!("fetch-{language}");
        let address = |last| IpAddr::V4(Ipv4Addr::new(185, 12, 64, last));
        let other = Endpoint::https(&format!("other.{provider}.example"));
        let api = Endpoint::https(&format!("api.{provider}.example"));
        let undeclared = Endpoint::https(&format!("evil.{provider}.example"));
        binding.serve(&other, address(41), None);
        binding.serve(&api, address(40), Some(&other));
        binding.serve(&undeclared, address(42), None);
        let headers = vec![
            (
                "Authorization".to_owned(),
                "Bearer-canary".to_owned(),
                false,
            ),
            ("X-Api-Key".to_owned(), "key-canary".to_owned(), true),
            ("X-Trace".to_owned(), "trace-1".to_owned(), false),
        ];
        let fetch = |endpoint: &Endpoint| Probe::Fetch {
            operation: "api.call".to_owned(),
            endpoint: endpoint.clone(),
            method: "PUT".to_owned(),
            path: "/v1/items?page=2".to_owned(),
            headers: headers.clone(),
        };
        let probes = [fetch(&api), fetch(&undeclared)];
        install(
            &mut binding,
            &spec(&provider),
            &language,
            "reserve",
            &probes,
        );
        binding.take_events();
        binding.handle(&provider, &withdrawal(&provider, 1));
        let events = binding.take_events();

        // The declared destination got the whole request, as the guest wrote it.
        let at_api = binding.world.requests_received(&api);
        assert_eq!(at_api.len(), 1, "{language}: api received {at_api:?}");
        assert_eq!(at_api[0].method, "PUT");
        assert_eq!(at_api[0].target, "/v1/items?page=2");
        let names: Vec<_> = at_api[0].headers.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            ["Authorization", "X-Api-Key", "X-Trace"],
            "{language}"
        );
        // The redirect to another declared destination keeps the method and drops every credential, the one named
        // `Authorization` and the one the guest flagged alike.
        let at_other = binding.world.requests_received(&other);
        assert_eq!(at_other.len(), 1, "{language}: other received {at_other:?}");
        assert_eq!(at_other[0].method, "PUT");
        assert_eq!(
            at_other[0].headers,
            [("X-Trace".to_owned(), "trace-1".to_owned())],
            "{language}: a credential followed the redirect"
        );
        // An endpoint the operation doesn't declare received nothing, and the call is recorded as refused.
        assert!(
            binding.world.requests_received(&undeclared).is_empty(),
            "{language}: an undeclared endpoint received the request"
        );
        assert_eq!(events.records.len(), 2, "{language}: a record per call");
        for record in &events.records {
            assert_eq!(
                validate_record(record),
                Ok(()),
                "{language}: invalid record"
            );
        }
        assert!(matches!(
            events.records[0].outcome,
            CallOutcome::Completed(_)
        ));
        assert_eq!(
            events.records[1].outcome,
            CallOutcome::Refused(Refused {
                reason: RefusalReason::UndeclaredDestination
            })
        );
        // The record names the secret-free request's destinations only, never a header's value.
        let text = serde_json::to_string(&events.records).expect("JSON");
        assert!(
            !text.contains("canary"),
            "{language}: a credential reached the record"
        );
    }
}

/// A binding with `api` served, a module of `language` built with `probes`, and what one withdrawal made of it.
fn run(
    language: &str,
    provider: &str,
    spec: &ModuleSpec,
    probes: &[Probe],
) -> (
    common::WasmBinding,
    Endpoint,
    Handled,
    rutent_conformance::boundary::Events,
) {
    let mut binding = binding("reserve");
    let api = Endpoint::https(&format!("api.{provider}.example"));
    binding.serve(&api, IpAddr::V4(Ipv4Addr::new(185, 12, 64, 40)), None);
    install(&mut binding, spec, language, "reserve", probes);
    binding.take_events();
    binding.take_limits();
    let handled = binding.handle(provider, &withdrawal(provider, 1));
    let events = binding.take_events();
    (binding, api, handled, events)
}

fn fetch(api: &Endpoint, path: &str, headers: Vec<(String, String, bool)>) -> Probe {
    Probe::Fetch {
        operation: "api.call".to_owned(),
        endpoint: api.clone(),
        method: "GET".to_owned(),
        path: path.to_owned(),
        headers,
    }
}

fn header(name: &str, value: &str) -> Vec<(String, String, bool)> {
    vec![(name.to_owned(), value.to_owned(), false)]
}

/// Asserts that the calls made were all refused as `reason`, recorded validly and each with one hop, that the
/// endpoint received nothing, and that no record holds a trace of the request.
fn assert_all_refused(
    language: &str,
    binding: &common::WasmBinding,
    api: &Endpoint,
    events: &rutent_conformance::boundary::Events,
    calls: usize,
    reason: RefusalReason,
) {
    assert_eq!(events.records.len(), calls, "{language}: a record per call");
    for record in &events.records {
        assert_eq!(
            validate_record(record),
            Ok(()),
            "{language}: invalid record"
        );
        assert_eq!(record.hops.len(), 1, "{language}");
        assert_eq!(
            record.outcome,
            CallOutcome::Refused(Refused { reason }),
            "{language}"
        );
    }
    assert!(
        binding.world.requests_received(api).is_empty(),
        "{language}: the endpoint received a request"
    );
    let text = serde_json::to_string(&events.records).expect("JSON");
    for trace in ["elsewhere", "Host", "forwarded", "/x"] {
        assert!(
            !text.contains(trace),
            "{language}: {trace} reached a record"
        );
    }
}

// verifies: REQ-MDL-6, REQ-MDL-8
#[test]
fn outbound_request_naming_another_host_in_its_path_or_headers_is_refused_and_recorded() {
    for language in languages() {
        let provider = format!("framing-{language}");
        let api = Endpoint::https(&format!("api.{provider}.example"));
        // An authority in place of a path, a `Host` header and a body-framing header.
        let probes = [
            fetch(&api, "//elsewhere.example/x", Vec::new()),
            fetch(&api, "/x", header("Host", "elsewhere.example")),
            fetch(&api, "/x", header("Content-Length", "0")),
        ];
        let (binding, api, _, events) = run(&language, &provider, &spec(&provider), &probes);
        assert_all_refused(
            &language,
            &binding,
            &api,
            &events,
            3,
            RefusalReason::RequestNotAllowed,
        );
    }
}

// verifies: REQ-MDL-6, REQ-MDL-8
#[test]
fn outbound_request_with_a_forwarding_header_is_refused_and_recorded() {
    for language in languages() {
        let provider = format!("forwarding-{language}");
        let api = Endpoint::https(&format!("api.{provider}.example"));
        let names = [
            "Forwarded",
            "X-Forwarded-Host",
            "X-Forwarded-For",
            "X-Real-IP",
            "X-Original-URL",
        ];
        let probes: Vec<_> = names
            .iter()
            .map(|name| fetch(&api, "/x", header(name, "elsewhere.example")))
            .collect();
        let (binding, api, _, events) = run(&language, &provider, &spec(&provider), &probes);
        assert_all_refused(
            &language,
            &binding,
            &api,
            &events,
            names.len(),
            RefusalReason::RequestNotAllowed,
        );
    }
}

/// Asserts that the operation was stopped by the request's size or header count, the call recorded as refused for a
/// limit, and nothing sent.
fn assert_stopped_by_message_size(
    language: &str,
    binding: &mut common::WasmBinding,
    api: &Endpoint,
    handled: &Handled,
    events: &rutent_conformance::boundary::Events,
) {
    assert_eq!(
        handled,
        &Handled::Refused(rutent_conformance::boundary::PlatformRefusal::LimitReached),
        "{language}"
    );
    let limits = binding.take_limits();
    assert_eq!(limits.len(), 1, "{language}: {limits:?}");
    assert_eq!(
        limits[0].hit.kind,
        rutent_prov::module::limits::LimitKind::MessageSize,
        "{language}"
    );
    assert_eq!(events.records.len(), 1, "{language}");
    assert_eq!(validate_record(&events.records[0]), Ok(()));
    assert_eq!(
        events.records[0].outcome,
        CallOutcome::Refused(Refused {
            reason: RefusalReason::LimitReached
        }),
        "{language}"
    );
    assert!(
        binding.world.requests_received(api).is_empty(),
        "{language}"
    );
}

// verifies: REQ-MDL-11, REQ-MDL-8
#[test]
fn outbound_request_with_more_headers_than_the_bound_is_stopped_and_recorded() {
    use rutent_prov::module::limits::MAX_HEADERS;
    for language in languages() {
        let provider = format!("headers-{language}");
        for (count, stopped) in [(MAX_HEADERS, false), (MAX_HEADERS + 1, true)] {
            let headers = (0..count)
                .map(|i| (format!("x-h{i}"), "v".to_owned(), false))
                .collect();
            let provider = format!("{provider}-{count}");
            let api = Endpoint::https(&format!("api.{provider}.example"));
            let probes = [fetch(&api, "/x", headers)];
            let (mut binding, api, handled, events) =
                run(&language, &provider, &spec(&provider), &probes);
            if stopped {
                assert_stopped_by_message_size(&language, &mut binding, &api, &handled, &events);
            } else {
                // The control: as many headers as the bound allows are sent.
                assert_eq!(binding.world.requests_received(&api).len(), 1, "{language}");
            }
        }
    }
}

// verifies: REQ-MDL-11, REQ-MDL-8
#[test]
fn outbound_request_past_the_byte_bound_in_its_path_headers_or_body_is_stopped_and_recorded() {
    use rutent_conformance::boundary::Step;
    use rutent_contracts::module_interface::PartName;
    use rutent_prov::module::limits::MAX_BODY;
    for language in languages() {
        // Each of the three alone is under the bound; the control sends them whole.
        let cases: [(&str, bool, usize); 4] = [
            ("path", false, MAX_BODY + 1),
            ("headers", false, MAX_BODY + 1),
            ("body and path together", true, 40_000),
            ("control", true, 1_000),
        ];
        for (label, with_body, size) in cases {
            let provider = format!("{}-{language}", label.replace(' ', "-"));
            let api = Endpoint::https(&format!("api.{provider}.example"));
            let probe = match label {
                "headers" => fetch(
                    &api,
                    "/x",
                    vec![("x-big".to_owned(), format!("v*{size}"), false)],
                ),
                _ => fetch(&api, &format!("/x*{size}"), Vec::new()),
            };
            let mut spec = spec(&provider);
            if with_body {
                // The body is what the earlier step obtained: a state entry of 30 000 bytes.
                spec.steps.push(Step::ReadState {
                    part: PartName::Outside,
                    key: "entry".to_owned(),
                });
            }
            let mut binding = binding("reserve");
            binding.serve(&api, IpAddr::V4(Ipv4Addr::new(185, 12, 64, 40)), None);
            binding.seed_state(&provider, "entry", &"s".repeat(30_000));
            install(&mut binding, &spec, &language, "reserve", &[probe]);
            binding.take_events();
            binding.take_limits();
            let handled = binding.handle(&provider, &withdrawal(&provider, 1));
            let events = binding.take_events();
            if label == "control" {
                assert_eq!(binding.world.requests_received(&api).len(), 1, "{language}");
            } else {
                assert_stopped_by_message_size(&language, &mut binding, &api, &handled, &events);
            }
        }
    }
}

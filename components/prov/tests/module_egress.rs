//! The platform binds every secret lookup and outbound call to the outside request in flight, from its own state,
//! never from what the outside part names (`REQ-MDL-6`, `REQ-MDL-7`, `REQ-MDL-8`).

#[path = "module_support/mod.rs"]
mod common;

use std::net::{IpAddr, Ipv4Addr};

use common::Probe;
use common::{binding, install, languages, withdrawal};
use rutent_conformance::boundary::{
    BoundaryHarness, Endpoint, Handled, LookupOutcome, ModuleBinding, ModuleSpec, Output,
};
use rutent_conformance::signing::{public_key, statements_key};
use rutent_contracts::module_interface::{
    CallOutcome, Declaration, RefusalReason, SecretRefusalReason, validate_record,
};
use serde_json::json;

/// A provider with two outside operations: `api.call`, reaching `api` with no secret, and `admin.call`, reaching
/// `admin` with `admin.key`. The first listed is the one its stand-in serves.
fn declaration(provider: &str, first: &str) -> Declaration {
    let api = json!({ "name": "api.call", "destinations": ["api"], "secrets": [] });
    let admin =
        json!({ "name": "admin.call", "destinations": ["admin"], "secrets": ["admin.key"] });
    let operations = if first == "api.call" {
        vec![api, admin]
    } else {
        vec![admin, api]
    };
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
            "operations": operations,
            "destinations": [destination("api"), destination("admin")],
            "secrets": ["admin.key"],
            "calls": [],
            "statement_key": public_key(&statements_key(provider)),
        },
    }))
    .expect("a valid declaration")
}

// verifies: REQ-MDL-6, REQ-MDL-7, REQ-MDL-8, REQ-MDL-3
#[test]
fn an_outside_part_naming_another_operation_than_the_one_in_flight_is_refused() {
    for language in languages() {
        let mut binding = binding("reserve");
        for (provider, serves, forged) in [
            (format!("forger-{language}"), "api.call", true),
            (format!("honest-{language}"), "admin.call", false),
        ] {
            let admin = Endpoint::https(&format!("admin.{provider}.example"));
            binding.serve(&admin, IpAddr::V4(Ipv4Addr::new(185, 12, 64, 40)), None);
            binding.seed_secret(&provider, "admin.key", &format!("canary-admin-{provider}"));
            let spec = ModuleSpec {
                declaration: declaration(&provider, serves),
                steps: Vec::new(),
                output: Output::Honest,
                starts: true,
            };
            let probe = Probe::Forge {
                claimed: "admin.call".to_owned(),
                secret: "admin.key".to_owned(),
                endpoint: admin.clone(),
            };
            install(&mut binding, &spec, &language, "reserve", &[probe]);
            binding.take_events();
            let handled = binding.handle(&provider, &withdrawal(&provider, 1));
            let events = binding.take_events();
            let lookup = events
                .lookups
                .iter()
                .find(|e| e.lookup.name.as_str() == "admin.key")
                .expect("a lookup");
            let record = events.records.first().expect("a record of the call");
            assert_eq!(validate_record(record), Ok(()), "the record is invalid");
            assert_eq!(
                record.operation.as_str(),
                serves,
                "the record names another operation than the one in flight"
            );
            assert_eq!(
                lookup.lookup.operation.as_str(),
                serves,
                "the lookup was judged for another operation"
            );
            let Handled::Answered(result) = handled else {
                panic!("{provider} didn't answer")
            };
            let reference = serde_json::to_value(&result.statement).expect("JSON")["outcome"]["evidence"][0]["reference"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            if forged {
                assert_eq!(
                    lookup.outcome,
                    Some(LookupOutcome::Refused(
                        SecretRefusalReason::UndeclaredSecret
                    )),
                    "a lookup naming another operation was answered"
                );
                assert_eq!(
                    record.outcome,
                    CallOutcome::Refused(rutent_contracts::module_interface::Refused {
                        reason: RefusalReason::UndeclaredOperation
                    }),
                    "a call naming another operation wasn't refused as undeclared_operation"
                );
                assert!(
                    binding.received(&admin).is_empty(),
                    "the endpoint received the forged call"
                );
                assert!(
                    binding.delivered_secrets(&provider).is_empty(),
                    "the proxy delivered the secret"
                );
                assert!(
                    !reference.contains("canary-admin"),
                    "the secret reached the output"
                );
            } else {
                // The control: the same steps, naming the operation in flight, resolve and complete.
                assert_eq!(
                    lookup.outcome,
                    Some(LookupOutcome::Resolved),
                    "the control's lookup was refused"
                );
                assert!(
                    matches!(record.outcome, CallOutcome::Completed(_)),
                    "the control's call didn't complete"
                );
                assert_eq!(
                    binding.received(&admin).len(),
                    1,
                    "the control's endpoint received nothing"
                );
            }
        }
    }
}

/// The module is told each outbound call's outcome so that a refusal is only ever one where nothing was sent: a call
/// the declared destination answers is `completed`; one to an undeclared destination, sent nowhere, is `refused`;
/// and one the declared destination took and redirected to an undeclared one is `unknown`, never `refused`, and is
/// recorded `refused_after_delivery`, since the first destination may have acted on it.
// verifies: REQ-ORD-1, REQ-MDL-7, REQ-MDL-8
#[test]
fn module_is_told_a_refusal_after_a_delivered_hop_as_unknown() {
    for language in languages() {
        let mut binding = binding("reserve");
        let provider = format!("told-{language}");
        let address = |last| IpAddr::V4(Ipv4Addr::new(185, 12, 64, last));
        let api = Endpoint::https(&format!("api.{provider}.example"));
        let undeclared = Endpoint::https(&format!("evil.{provider}.example"));
        binding.serve(&undeclared, address(42), None);
        let tell = |endpoint: &Endpoint| Probe::Tell {
            operation: common::OPERATION.to_owned(),
            endpoint: endpoint.clone(),
        };
        install(
            &mut binding,
            &common::spec(&provider, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[tell(&api), tell(&undeclared)],
        );
        let evidence = |handled: Handled| {
            let Handled::Answered(result) = handled else {
                panic!("{language}: the module didn't answer: {handled:?}")
            };
            serde_json::to_value(&result.statement).expect("JSON")["outcome"]["evidence"][0]
                ["reference"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        };

        // The declared destination answers.
        binding.serve(&api, address(40), None);
        binding.take_events();
        assert_eq!(
            evidence(binding.handle(&provider, &withdrawal(&provider, 1))),
            format!("{provider}-1.0.0-completed-refused"),
            "{language}"
        );

        // The declared destination takes the request and redirects it to the undeclared one.
        binding.serve(&api, address(40), Some(&undeclared));
        binding.take_events();
        assert_eq!(
            evidence(binding.handle(&provider, &withdrawal(&provider, 2))),
            format!("{provider}-1.0.0-unknown-refused"),
            "{language}: a refusal after a delivered hop was told as a refusal"
        );
        let events = binding.take_events();
        let redirected = &events.records[0];
        assert_eq!(
            redirected.outcome,
            CallOutcome::RefusedAfterDelivery(rutent_contracts::module_interface::Refused {
                reason: RefusalReason::RedirectToUndeclaredDestination
            }),
            "{language}: the record says the call was refused before anything was sent"
        );
        assert!(redirected.hops[0].address.is_some(), "{language}");
        assert_eq!(validate_record(redirected), Ok(()), "{language}");
    }
}

//! The sandboxed part of a module that is `M1`'s role stand-in of one provider category (`T-013-01`, `B-01`). The
//! platform hands each operation in as the operator's envelope, as JSON; the stand-in answers it as it does in
//! process, and the part returns the result body's bytes. The stand-in lives in the instance for as long as the
//! provider's process does, so what it holds on its books persists from one operation to the next.
//!
//! A category is chosen by a feature (see `Cargo.toml`).

use std::cell::RefCell;

#[cfg(feature = "exchange")]
use rutent_conformance::role::stand_in::Exchange as Provider;
#[cfg(feature = "ramp")]
use rutent_conformance::role::stand_in::Ramp as Provider;
#[cfg(feature = "reserve")]
use rutent_conformance::role::stand_in::Reserve as Provider;
#[cfg(feature = "verifier")]
use rutent_conformance::role::stand_in::Verifier as Provider;
use rutent_conformance::role::{RoleProvider, SuiteContext};
use rutent_contracts::provider_role::{Operation, Statement};

wit_bindgen::generate!({ world: "sandboxed", path: "../../src/module/wit" });

thread_local! {
    static PROVIDER: RefCell<Option<Provider>> = const { RefCell::new(None) };
}

struct Part;

impl Guest for Part {
    fn start() -> Result<(), String> {
        let context = SuiteContext::default();
        PROVIDER.with(|p| *p.borrow_mut() = Some(Provider::new(&context)));
        Ok(())
    }

    fn handle(operation: String) -> Vec<u8> {
        let Ok(operation) = serde_json::from_str::<Statement<Operation>>(&operation) else {
            return b"not an operation".to_vec();
        };
        PROVIDER.with(|p| match p.borrow_mut().as_mut() {
            Some(provider) => {
                serde_json::to_vec(&provider.handle(&operation).statement().statement)
                    .unwrap_or_default()
            }
            None => b"not started".to_vec(),
        })
    }
}

export!(Part);

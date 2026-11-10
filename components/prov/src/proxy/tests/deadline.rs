//! The store is asked within the operation's deadline (`REQ-MDL-11`, `T-014-05`): a store whose host name never
//! resolves is given up when the deadline passes, and the lookup is refused as `store_unavailable`.

use std::net::IpAddr;
use std::num::NonZeroU16;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use rutent_contracts::module_interface::{Host, SecretRefusalReason};

use super::support::*;
use crate::egress::{NameResolver, ThreadedResolver};
use crate::proxy::{
    Bucket, CredentialProxy, HttpsTransport, InvalidSettings, KvConfig, KvResolver, Refusal,
    StoreAddress, StoreScheme, Token,
};

/// A resolver whose every lookup hangs until it is released, or 20 seconds pass.
#[derive(Clone, Default)]
struct Hanging(Arc<(Mutex<bool>, Condvar)>);

impl Hanging {
    fn release(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}

impl NameResolver for Hanging {
    fn resolve(&self, _: &Host, _: NonZeroU16) -> Vec<IpAddr> {
        let (released, wake) = &*self.0;
        let mut done = released.lock().unwrap();
        let started = Instant::now();
        while !*done && started.elapsed() < Duration::from_secs(20) {
            done = wake
                .wait_timeout(done, Duration::from_millis(100))
                .unwrap()
                .0;
        }
        Vec::new()
    }
}

/// A store reached by a host name whose lookup never answers gives `store_unavailable` at the operation's
/// deadline, and the platform is held no longer than that.
// verifies: REQ-MDL-11, REQ-MDL-17
#[test]
fn store_whose_name_lookup_hangs_is_unavailable_at_the_deadline() {
    let hanging = Hanging::default();
    let ca = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/proxy/tls/ca.pem"),
    )
    .expect("the test CA");
    let transport = HttpsTransport::new(
        &StoreAddress {
            scheme: StoreScheme::Https,
            host: "bao.internal".to_owned(),
            port: 8200,
        },
        Some(&ca),
    )
    .expect("a transport")
    .with_resolver(ThreadedResolver::new(hanging.clone()));
    let resolver = || {
        KvResolver::openbao(
            KvConfig::new(
                "secret",
                "rutent/internal/secrets",
                Token::new("t").unwrap(),
            )
            .unwrap(),
            transport.clone(),
        )
    };
    let mut proxy = CredentialProxy::new(&settings(), |_, _| {
        Ok::<_, InvalidSettings>(Bucket {
            secrets: resolver(),
            keys: resolver(),
        })
    })
    .expect("a proxy");
    proxy
        .admit(
            &release("alpha", "1.0.0", &[SECRET], KEY_B),
            &advertisement("alpha", &[(KEY_A, "advertising"), (KEY_B, "statements")]),
        )
        .expect("admitted");
    let who = caller("alpha");
    let started = Instant::now();
    proxy
        .begin_operation_until(
            &who,
            &id("tx-1"),
            Some(started + Duration::from_millis(400)),
        )
        .expect("the operation starts");
    proxy
        .begin_request(&who, &id("tx-1"), &name(OPERATION))
        .expect("the request is in flight");

    let refused = proxy
        .lookup(&who, &lookup("tx-1", OPERATION, SECRET))
        .expect_err("a store that never resolves answers nothing");

    assert!(matches!(refused, Refusal::Store(_)), "{refused:?}");
    assert_eq!(refused.wire_reason(), SecretRefusalReason::StoreUnavailable);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "held for {:?}",
        started.elapsed()
    );
    hanging.release();
}

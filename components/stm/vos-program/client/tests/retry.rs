//! The client's retry of an open refused while the platform spawns children. It needs no VOS: the open is a
//! closure.

use std::cell::Cell;

use rutent_contracts::state_machine::host::HostError;
use rutent_stm_vos::retry::open_retrying;

fn did_not_start() -> HostError {
    HostError::Unavailable {
        reason: "node 2 did not start; its log ends: [\"the data directory is locked\"]".into(),
    }
}

// verifies: REQ-OPS-3
#[test]
fn the_client_retries_an_open_refused_while_children_spawn() {
    let tries = Cell::new(0);
    let opened = open_retrying(true, || {
        tries.set(tries.get() + 1);
        if tries.get() < 4 {
            Err(did_not_start())
        } else {
            Ok("open")
        }
    });
    assert_eq!(opened.unwrap(), "open");
    assert_eq!(tries.get(), 4);
}

// verifies: REQ-OPS-3
#[test]
fn the_client_opens_a_new_cluster_once_and_does_not_retry_another_refusal() {
    let tries = Cell::new(0);
    let first: Result<(), HostError> = open_retrying(false, || {
        tries.set(tries.get() + 1);
        Err(did_not_start())
    });
    assert!(first.is_err());
    assert_eq!(tries.get(), 1);

    tries.set(0);
    let other: Result<(), HostError> = open_retrying(true, || {
        tries.set(tries.get() + 1);
        Err(HostError::ImportIncomplete)
    });
    assert!(matches!(other, Err(HostError::ImportIncomplete)));
    assert_eq!(tries.get(), 1);
}

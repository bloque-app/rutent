//! The bounded retry of an open refused while the platform spawns children (`F-007`'s notes, `T-010-05`).

use std::cell::Cell;
use std::time::Duration;

use rutent_contracts::state_machine::host::HostError;
use rutent_stm::open_retry::{OpenRetry, is_open_elsewhere, retry_open};

fn quick(attempts: u32) -> OpenRetry {
    OpenRetry {
        attempts,
        pause: Duration::from_millis(1),
    }
}

fn elsewhere() -> HostError {
    HostError::Unavailable {
        reason: rutent_stm::open_retry::STORE_OPEN_ELSEWHERE.into(),
    }
}

// verifies: REQ-OPS-3
#[test]
fn an_open_refused_for_another_reason_is_not_retried() {
    let tries = Cell::new(0);
    let result: Result<(), HostError> = retry_open(quick(5), is_open_elsewhere, || {
        tries.set(tries.get() + 1);
        Err(HostError::ImportIncomplete)
    });
    assert!(matches!(result, Err(HostError::ImportIncomplete)));
    assert_eq!(tries.get(), 1);
}

// verifies: REQ-OPS-3
#[test]
fn an_open_refused_again_and_again_gives_up_after_its_attempts_with_the_last_refusal() {
    let tries = Cell::new(0);
    let result: Result<(), HostError> = retry_open(quick(4), is_open_elsewhere, || {
        tries.set(tries.get() + 1);
        Err(elsewhere())
    });
    assert!(matches!(result, Err(HostError::Unavailable { .. })));
    assert_eq!(tries.get(), 4);
}

// verifies: REQ-OPS-3
#[test]
fn an_open_that_succeeds_on_a_later_attempt_returns_its_host() {
    let tries = Cell::new(0);
    let result = retry_open(quick(5), is_open_elsewhere, || {
        tries.set(tries.get() + 1);
        if tries.get() < 3 {
            Err(elsewhere())
        } else {
            Ok(7)
        }
    });
    assert_eq!(result.unwrap(), 7);
    assert_eq!(tries.get(), 3);
}

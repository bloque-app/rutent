//! The host server's wire carries every call and answer of the host contract unchanged (`T-024-02`): what
//! `rutent-scenarios-vos` sends and reads is what the server took and gave. It needs no node.

use rutent_contracts::state_machine::host::{Delivery, Host, HostError};
use rutent_stm::Stm;
use rutent_stm::reference_host::{MemoryStore, ReferenceHost};
use rutent_stm_vos_wire::{Answer, Error, Request};

// verifies: DEC-18
#[test]
fn host_server_wire_carries_each_call_and_answer_unchanged() {
    let requests = [
        Request::Open,
        Request::Import {
            entries: Vec::new(),
        },
        Request::Orders,
        Request::State {
            order: rutent_contracts::state_machine::Id::new("ord-1").unwrap(),
        },
        Request::Journal {
            order: rutent_contracts::state_machine::Id::new("ord-1").unwrap(),
        },
        Request::Halt,
        Request::Resume,
        Request::Export,
        Request::Stop,
    ];
    for request in requests {
        let line = serde_json::to_string(&request).unwrap();
        assert!(!line.contains('\n'), "one call is one line: {line}");
        assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), request);
    }
    let errors = [
        HostError::Halted,
        HostError::NotHalted,
        HostError::ImportIncomplete,
        HostError::SnapshotRefused {
            reason: "a stream cut short".to_owned(),
        },
        HostError::Unavailable {
            reason: "the store failed".to_owned(),
        },
    ];
    for error in errors {
        let answer = Answer::Failed {
            error: Error::from(error.clone()),
        };
        let line = serde_json::to_string(&answer).unwrap();
        let Answer::Failed { error: back } = serde_json::from_str::<Answer>(&line).unwrap() else {
            panic!("{line}")
        };
        assert_eq!(HostError::from(back), error);
    }
    for answer in [
        Answer::Done,
        Answer::Orders { orders: Vec::new() },
        Answer::State { state: None },
        Answer::Journal {
            records: Vec::new(),
        },
        Answer::Entries {
            entries: Vec::new(),
        },
    ] {
        let line = serde_json::to_string(&answer).unwrap();
        assert_eq!(serde_json::from_str::<Answer>(&line).unwrap(), answer);
    }
}

// verifies: DEC-18
#[test]
fn host_server_wire_carries_a_submit_its_record_and_a_reused_record_id_unchanged() {
    let mut host = ReferenceHost::<Stm>::open(MemoryStore::new()).unwrap();
    let inputs = rutent_conformance::host::probe::order_inputs("ord-w", 0);
    let first = inputs[0].clone();
    let submit = Request::Submit {
        input: Box::new(first.clone()),
    };
    let line = serde_json::to_string(&submit).unwrap();
    assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), submit);
    let Ok(Delivery::Recorded(record)) = host.submit(first.clone()) else {
        panic!("recorded")
    };
    for answer in [
        Answer::Recorded {
            record: record.clone(),
        },
        Answer::AlreadyRecorded { record },
    ] {
        let line = serde_json::to_string(&answer).unwrap();
        assert_eq!(serde_json::from_str::<Answer>(&line).unwrap(), answer);
    }
    // Another input under the same record id is refused, and the refusal's record crosses the wire whole.
    let mut other = inputs[1].clone();
    other.record_id = first.record_id.clone();
    let refused = host.submit(other).unwrap_err();
    assert!(matches!(refused, HostError::RecordIdReused { .. }));
    let answer = Answer::Failed {
        error: Error::from(refused.clone()),
    };
    let line = serde_json::to_string(&answer).unwrap();
    let Answer::Failed { error } = serde_json::from_str::<Answer>(&line).unwrap() else {
        panic!("{line}")
    };
    assert_eq!(HostError::from(error), refused);
}

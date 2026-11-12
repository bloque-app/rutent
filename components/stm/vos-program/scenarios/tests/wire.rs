//! The host server's wire carries every call and answer of the host contract unchanged (`T-024-02`): what
//! `rutent-scenarios-vos` sends and reads is what the server took and gave. It needs no node.

use rutent_contracts::state_machine::host::HostError;
use rutent_stm_vos_serve::{Answer, Error, Request};

// verifies: DEC-26
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

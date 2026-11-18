//! A publisher's record that isn't there when the grading first asks for it is asked for again (`T-020-32`): the
//! segments keep what the grading asks to have kept, each once, but an empty answer caches nothing, so a segment
//! whose grading waited and is graded again is asked again, and the node it binds is bound as soon as a record is.

mod segments_support;

use rutent_conformance::signing::{real, sign_statement};
use rutent_contracts::core_internal::{About, Entry};
use rutent_contracts::provider_role::{
    INNER_SEGMENTS_SHAPE, InnerSegment, InnerSegments, Signature, SignatureScheme, Standing,
    Statement,
};
use rutent_core_orch::segments::{Graded, Grading, NoProxy, ProvidersWord, Short, ToGrade, ToKeep};
use segments_support::*;

/// A grading that waits at its first grading and settles at its second, and whose book of the node is empty at the
/// first ask and holds a record at the second.
struct LateRecord {
    asked: usize,
    graded: usize,
    kept_at_grading: Vec<usize>,
    record: Vec<u8>,
}

impl Grading for LateRecord {
    fn statements(&mut self, settled: &ToGrade<'_>) -> Vec<ToKeep> {
        self.asked += 1;
        if self.asked != 2 {
            return Vec::new();
        }
        vec![ToKeep {
            key: "enter:ledger".to_owned(),
            bytes: self.record.clone(),
            about: About {
                order: Some(settled.order.clone()),
                segment: None,
                transaction: None,
                node: rutent_contracts::router_api::Id::new("ledger"),
            },
        }]
    }

    fn grade(&mut self, settled: &ToGrade<'_>) -> Graded {
        self.graded += 1;
        self.kept_at_grading.push(settled.kept.len());
        if self.graded < 2 {
            return Graded::Waits(Short::Unreadable);
        }
        ProvidersWord.grade(settled)
    }
}

fn a_record(world: &World) -> Vec<u8> {
    let context = world.context();
    let record = Statement {
        deployment: context.deployment.clone(),
        protocol_version: context.protocol_version.clone(),
        shape: INNER_SEGMENTS_SHAPE.to_owned(),
        maker: RESERVE.to_owned(),
        made_at: "2026-11-06T15:04:05Z".to_owned(),
        signature: Signature {
            scheme: SignatureScheme::Ed25519,
            key: rutent_conformance::role::placeholder_key(1),
            value: rutent_conformance::role::placeholder_signature(),
        },
        statement: InnerSegments {
            node: "ledger".to_owned(),
            publisher: "publisher".to_owned(),
            standing: Standing::PublishersWord,
            segments: vec![InnerSegment {
                transaction: "tx-1".to_owned(),
                asset_in: "COP".to_owned(),
                amount_in: "1000".to_owned(),
                asset_out: "COP".to_owned(),
                amount_out: "1000".to_owned(),
                fee: "0".to_owned(),
            }],
        },
    };
    let signed = sign_statement(&record, &real(record.signature.key.as_str()));
    serde_json::to_vec(&signed).expect("a record is JSON")
}

// verifies: REQ-ORD-3
#[test]
fn a_record_missing_at_the_first_ask_is_asked_for_again_when_the_grading_waited() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let session = world.context().session;
    let mut grading = LateRecord {
        asked: 0,
        graded: 0,
        kept_at_grading: Vec::new(),
        record: a_record(&world),
    };
    for _ in 0..2 {
        let _ = core.try_run_with(
            &world.catalogue,
            &mut world.fleet.platform,
            &mut NoProxy,
            &mut grading,
            &session,
        );
    }
    assert!(grading.asked >= 2, "asked again: {}", grading.asked);
    assert_eq!(
        grading.kept_at_grading[..2],
        [0, 1],
        "nothing kept at the first grading, the record at the second"
    );
    let records = core
        .log
        .borrow()
        .entries
        .iter()
        .filter(|(e, _)| {
            matches!(e, Entry::Stated(s) if s.what.statement.as_str() == INNER_SEGMENTS_SHAPE)
        })
        .count();
    assert_eq!(records, 1, "appended once");
}

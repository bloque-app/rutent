//! Core's adapter for each public network's reader service (`T-020-60`, `DEC-28`, ADR-0007 as amended): every public
//! record Core reads, it reads through its own adapter (`rutent_core_orch::closing::Readers`), over an mTLS client of
//! its own to the deployment's reader host, each answer held to the operator's CA and the listed subject name, the
//! reader's listed key and version, the question Core sent, and its deadline on Core's clock.

mod execution_support;
mod readers_support;

use std::cell::Cell;
use std::collections::BTreeSet;
use std::net::TcpListener;
use std::rc::Rc;

use execution_support::*;
use readers_support::{Plan, Read, Readers, TestCa};
use rutent_contracts::core_internal::{PublicNetwork, ReaderNotStarted};
use rutent_contracts::provider_role::NoDetail;
use rutent_contracts::reader::{Answered, Asked, ReaderMismatch};
use rutent_contracts::router_api::Id;
use rutent_contracts::state_machine::OrderState;
use rutent_core_orch::closing::readers::{NotStarted, RandomSource, Standing};
use rutent_core_orch::closing::{PublicRecords as _, Unreadable};
use rutent_core_orch::intake::{OrderRequest, Status, Told};
use rutent_core_orch::snapshot::{Clock, Reading, SystemClock};
use rutent_evidence::MovementId;
use rutent_prov::catalogue::{Catalogue, NodeError};
use rutent_scenarios::assembly::Core;
use rutent_scenarios::kit::{self, AMOUNT, RESERVE};
use rutent_scenarios::sim::readers::{HOST_NAME, HOST_SETTING, READER_VERSION, reader_name};

fn id(text: &str) -> Id {
    Id::new(text).expect("an id")
}

fn bank() -> String {
    format!("{RESERVE}-bank")
}

/// A second of Core's clock, well inside the protocol version's force.
const AT: u64 = 1_900_000_000;

/// Core's clock that reads `first` the first time it is read, and `then` every time after: an answer asked at `first`
/// reaches Core at `then`.
struct Jumping {
    reads: Cell<u32>,
    first: u64,
    then: u64,
}

impl Jumping {
    fn by(seconds: u64) -> Self {
        Jumping {
            reads: Cell::new(0),
            first: AT,
            then: AT + seconds,
        }
    }
}

impl Clock for Jumping {
    fn now(&self) -> Reading {
        let n = self.reads.get();
        self.reads.set(n + 1);
        Reading::at(if n == 0 { self.first } else { self.then })
    }
}

/// What quoting `request` told, where it opened an order at all.
fn quoted_or_not(core: &mut Core, request: OrderRequest) -> (Option<Id>, Option<Told>) {
    let Ok(told) = core.request(request) else {
        return (None, None);
    };
    let Some(order) = Core::order_of(&told) else {
        return (None, Some(told));
    };
    let told = core.quote(&kit::customer(), &order).ok();
    (Some(order), told)
}

/// Whether a quote was shown.
fn is_quoted(told: &Option<Told>) -> bool {
    matches!(
        told,
        Some(Told::Exists {
            status: Status::Quoted { .. },
            ..
        })
    )
}

/// The listed networks of a Core with the base reserve alone: its bank.
fn bank_only() -> (Core, Id, Vec<PublicNetwork>) {
    let (core, session) = core_with("AC-1", &[RESERVE]);
    let listed = core.public_networks();
    assert_eq!(
        listed
            .iter()
            .map(|n| n.network.as_str())
            .collect::<Vec<_>>(),
        [bank()]
    );
    (core, session, listed)
}

/// The order's state, after Core's tick has run it as far as it goes.
fn after_ticks(core: &mut Core, order: &Id, ticks: usize) -> OrderState {
    for _ in 0..ticks {
        core.tick();
    }
    state(core, order)
}

/// A network whose reader doesn't start is raised to the operator in the log, citing the listing its reader was held
/// to, and the log admits the raise (`raise-of-a-reader-not-started-cites-its-listing`); it is raised once in a spell
/// of not starting, so checking again while it still fails raises nothing more. Once its reader passes the spell ends,
/// and a later failure raises it again (`T-020-60` (2), `REQ-CAT-22`).
// verifies: REQ-CAT-22, DEC-28
#[test]
fn network_whose_reader_does_not_start_is_raised_once_in_its_spell_citing_its_listing() {
    let (mut core, _, _) = bank_only();
    let bank = bank();
    let host = core.reader_service().address();
    assert!(core.reader_raises().is_empty(), "every reader started");
    let nowhere = TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("a port");
    core.move_reader_host(nowhere);
    let raises = core.reader_raises();
    let [(network, why, cites)] = raises.as_slice() else {
        panic!("one raise: {raises:?}")
    };
    assert_eq!((network, *why), (&bank, ReaderNotStarted::Unreachable));
    let listing = core
        .listing_receipt(&bank)
        .expect("the log holds the network's listing");
    assert_eq!(cites, &vec![listing], "it cites the network's listing");
    // Checked again while it still fails: the same spell, nothing more raised.
    core.move_reader_host(nowhere);
    assert_eq!(core.reader_raises().len(), 1, "raised once in its spell");
    // Its reader passes again: the spell ends. A later failure raises it again.
    core.move_reader_host(host);
    assert_eq!(core.readers().standing(&bank), Standing::Started);
    assert_eq!(
        core.reader_raises().len(),
        1,
        "a reader that passes isn't raised"
    );
    core.move_reader_host(nowhere);
    assert_eq!(core.reader_raises().len(), 2, "a new spell, raised again");
}

// verifies: REQ-CAT-22, DEC-28
#[test]
fn network_whose_reader_is_unreachable_does_not_start_and_cannot_be_listed() {
    let (mut core, session, listed) = bank_only();
    let bank = bank();
    assert_eq!(core.readers().standing(&bank), Standing::Started);
    assert!(core.catalogue().nodes().is_started_network(&bank));
    // The deployment's host setting now holds an address no reader host listens at.
    let nowhere = TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("a port");
    core.move_reader_host(nowhere);
    assert_eq!(
        core.readers().standing(&bank),
        Standing::NotStarted(NotStarted::Unread(Unreadable::Unreachable))
    );
    let raises = core.reader_raises();
    assert!(
        matches!(raises.as_slice(), [(n, ReaderNotStarted::Unreachable, _)] if *n == bank),
        "the operator is told why, by a raise in the log: {raises:?}"
    );
    // It is off the catalogue's public list, and the snapshot routes nothing to it: no quote is shown.
    assert!(!core.catalogue().nodes().is_started_network(&bank));
    assert!(core.catalogue().nodes().is_withheld_network(&bank));
    let (_, told) = quoted_or_not(&mut core, within("req-1", &session, AMOUNT, RESERVE, OTHER));
    assert!(!is_quoted(&told), "nothing routed to it: {told:?}");
    // A network is taken onto the public list only from a listing naming its reader.
    let mut catalogue = Catalogue::new();
    assert_eq!(
        catalogue.list_public_network(&bank, false, true),
        Err(NodeError::NoListing(bank.clone()))
    );
    assert!(!catalogue.nodes().is_started_network(&bank));
    // The host back where the setting says: the reader passes its check again, and the network starts.
    let back = core.reader_service().address();
    core.move_reader_host(back);
    assert_eq!(core.readers().standing(&bank), Standing::Started);
    assert!(core.catalogue().nodes().is_started_network(&bank));
    let _ = listed;
}

/// A decided listing whose reader fails its start-up check at listing time (the host unreachable then) is carried out,
/// its result `done`, but the network isn't marked started: it is held off the catalogue's public list, and the
/// operator is told why by a raise in the log (`T-020-60` (2), the review of #356's round 2, Low 2; `REQ-CAT-22`).
// verifies: REQ-CAT-22, DEC-28
#[test]
fn listing_whose_reader_fails_its_check_is_done_but_its_network_is_held_off_and_raised() {
    use rutent_contracts::core_internal::{Action, CatalogueChange, Entry, Outcome, What};
    let (mut core, _, listed) = bank_only();
    let bank = bank();
    core.delist_public_network(&id(&bank))
        .expect("delisted: no order uses it");
    assert!(!core.catalogue().nodes().is_started_network(&bank));
    let nowhere = TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("a port");
    core.move_reader_host(nowhere);
    let raised_before = core.reader_raises().len();
    core.list_public_network(listed[0].clone(), "list-bank-again")
        .expect("the listing is carried out");
    let entries = core.log().entries().to_vec();
    let effect = entries
        .iter()
        .rev()
        .find_map(|(r, e)| match e {
            Entry::Verified(v) => match &v.what {
                What::Effect {
                    action:
                        Action::ChangeCatalogue {
                            change: CatalogueChange::ListPublicNetwork,
                            public_network: Some(n),
                            ..
                        },
                    ..
                } if n.network.as_str() == bank => Some(r.clone()),
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .expect("the listing's effect");
    let result = entries.iter().find_map(|(_, e)| match e {
        Entry::Verified(v) => match &v.what {
            What::Result { effect: of } if *of == effect => Some(v.outcome.clone()),
            _ => None,
        },
        Entry::Stated(_) => None,
    });
    assert_eq!(
        result,
        Some(Outcome::Done {}),
        "the listing's result is done"
    );
    assert_eq!(
        core.readers().standing(&bank),
        Standing::NotStarted(NotStarted::Unread(Unreadable::Unreachable)),
        "its reader didn't pass the check"
    );
    assert!(
        !core.catalogue().nodes().is_started_network(&bank),
        "not marked started"
    );
    assert!(
        core.catalogue().nodes().is_withheld_network(&bank),
        "held off the public list's routes"
    );
    let raises = core.reader_raises();
    assert_eq!(raises.len(), raised_before + 1, "raised: {raises:?}");
    assert!(
        matches!(raises.last(), Some((n, ReaderNotStarted::Unreachable, _)) if *n == bank),
        "the operator is told why: {raises:?}"
    );
}

// verifies: DEC-28
#[test]
fn reader_hosts_rotated_certificate_under_the_operators_ca_and_listed_name_is_accepted_without_a_listing_change()
 {
    let (core, _, listed) = bank_only();
    let bank = bank();
    let mut readers = Readers::start(&listed, core.world_endpoints(), &Plan::default());
    let rules = Asked::NetworkRules(NoDetail {});
    assert!(readers.ask(&bank, rules.clone()).1.answered().is_some());
    // The host's leaf rotated: a new key and certificate under the same CA and the listed name, the listing unchanged.
    let rotated = readers.serve_leaf(&TestCa::new(), HOST_NAME);
    readers.core_mut().set_host(HOST_SETTING, rotated);
    assert!(
        matches!(
            readers.ask(&bank, rules.clone()).1,
            Read::Answered(Answered::NetworkRules { .. })
        ),
        "the rotated leaf is taken"
    );
    // Never a leaf of another CA, nor one of the operator's CA under another name: neither handshake completes.
    for (ca, name) in [
        (TestCa::other(), HOST_NAME),
        (TestCa::new(), "another.rutent-scenarios.example"),
    ] {
        let wrong = readers.serve_leaf(&ca, name);
        readers.core_mut().set_host(HOST_SETTING, wrong);
        assert_eq!(
            readers.ask(&bank, rules.clone()).1,
            Read::Unreadable(Unreadable::Unreachable),
            "{name}"
        );
    }
    assert_eq!(
        readers.core().listed()[&bank],
        listed[0].listing.clone().unwrap()
    );
}

// verifies: REQ-ORD-39, DEC-28
#[test]
fn reader_answer_past_its_deadline_by_cores_clock_is_unreadable() {
    let (core, _, listed) = bank_only();
    let bank = bank();
    let readers = Readers::start(&listed, core.world_endpoints(), &Plan::default());
    let window = rutent_core_orch::closing::readers::ANSWER_WINDOW.as_secs();
    let rules = Asked::NetworkRules(NoDetail {});
    // Past the deadline by one second of Core's clock, whatever the host's own clock says: refused.
    let (_, late) = readers.ask_at(&bank, rules.clone(), &Jumping::by(window + 1));
    assert!(
        matches!(&late, Read::Refused(m) if m == &[ReaderMismatch::PastItsDeadline]),
        "{late:?}"
    );
    // Exactly at its deadline: taken.
    let (_, due) = readers.ask_at(&bank, rules.clone(), &Jumping::by(window));
    assert!(
        matches!(due, Read::Answered(Answered::NetworkRules { .. })),
        "{due:?}"
    );
    // And through Core's own start-up check: a reader whose answer comes late doesn't start.
    let mut core_side = readers.core_adapter();
    assert!(matches!(
        core_side.check_at(&bank, &Jumping::by(window + 1)),
        Standing::NotStarted(NotStarted::Unread(Unreadable::Refused(_)))
    ));
    assert_eq!(
        core_side.check_at(&bank, &Jumping::by(1)),
        Standing::Started
    );
}

/// A source that yields a counter, as a counter written as an id would.
struct Counting(Rc<Cell<u64>>);

impl RandomSource for Counting {
    fn fill(&self, bytes: &mut [u8]) -> bool {
        let n = self.0.get() + 1;
        self.0.set(n);
        bytes.fill(0);
        let len = bytes.len();
        bytes[len - 8..].copy_from_slice(&n.to_be_bytes());
        true
    }
}

// verifies: DEC-28
#[test]
fn reader_request_ids_are_drawn_from_cores_random_source_and_never_repeat() {
    let (core, _, listed) = bank_only();
    let bank = bank();
    let readers = Readers::start(&listed, core.world_endpoints(), &Plan::default());
    let mut seen = BTreeSet::new();
    for n in 0..64 {
        let (put, read) = readers.ask(&bank, Asked::NetworkRules(NoDetail {}));
        assert!(read.answered().is_some(), "{n}: {read:?}");
        assert!(!put.request.as_str().starts_with("0000000000000000"));
        assert!(
            seen.insert(put.request.as_str().to_owned()),
            "{n}: repeated"
        );
    }
    // Core's adapter drawing from a counter instead: every draw is refused as a counter's, so no question is put.
    let drawn = Rc::new(Cell::new(0));
    let counting = readers
        .core_adapter()
        .drawing_from(Box::new(Counting(drawn.clone())));
    assert_eq!(
        counting.question(&bank, Asked::NetworkRules(NoDetail {}), &SystemClock),
        Err(Unreadable::NoAnswer)
    );
    assert!(counting.put().is_empty(), "nothing put");
    assert!(drawn.get() > 0, "the draw is the source's");
}

// verifies: REQ-ORD-39, DEC-28
#[test]
fn reader_answer_not_signed_by_the_catalogued_reader_is_refused() {
    let (mut core, _, listed) = bank_only();
    let bank = bank();
    let readers = Readers::start(
        &listed,
        core.world_endpoints(),
        &Plan::everywhere(&listed, "unlisted-key"),
    );
    let (_, read) = readers.ask(&bank, Asked::NetworkRules(NoDetail {}));
    assert_eq!(read, Read::Unreadable(Unreadable::NotSigned));
    // The deployment's own host signing so: Core's start-up check refuses it, and the network doesn't start.
    core.plant_readers(&Plan::everywhere(&listed, "unlisted-key"));
    assert_eq!(
        core.readers().standing(&bank),
        Standing::NotStarted(NotStarted::Unread(Unreadable::NotSigned))
    );
    core.plant_readers(&Plan::default());
    assert_eq!(core.readers().standing(&bank), Standing::Started);
}

/// The base order, run through Core's tick to its end.
fn completed(core: &mut Core, session: &Id) -> Id {
    let (order, _) = run(core, within("req-1", session, AMOUNT, RESERVE, OTHER));
    assert_eq!(after_ticks(core, &order, 2), OrderState::Complete);
    order
}

// verifies: REQ-AUD-4, DEC-28
#[test]
fn evidence_entry_names_the_reader_and_the_version_that_read_its_movement() {
    let (mut core, session, listed) = bank_only();
    let order = completed(&mut core, &session);
    let listing = listed[0].listing.clone().expect("a listing");
    let named: Vec<_> = evidence_of(&core, &order)
        .into_iter()
        .filter_map(|e| e.movement)
        .collect();
    assert!(
        !named.is_empty(),
        "the order's evidence names its movements"
    );
    for movement in named {
        assert_eq!(movement.read_by.reader, listing.endpoint.reader);
        assert_eq!(movement.read_by.version, listing.version);
        assert_eq!(movement.read_by.reader.as_str(), reader_name(&bank()));
        assert_eq!(movement.read_by.version.as_str(), READER_VERSION);
    }
}

// verifies: REQ-ORD-39, INV-3
#[test]
fn unreadable_reader_settles_nothing_and_grades_nothing_down() {
    let (mut core, session, listed) = bank_only();
    // The bank's reader answers its rules, so the route is taken and the withdrawal handed over, but can't stand behind
    // any movement: every movement it is asked of is unreadable.
    core.plant_readers(&Plan::everywhere(&listed, "unread-movements"));
    assert_eq!(core.readers().standing(&bank()), Standing::Started);
    let (order, _) = run(&mut core, within("req-1", &session, AMOUNT, RESERVE, OTHER));
    assert_ne!(after_ticks(&mut core, &order, 3), OrderState::Complete);
    let evidence = evidence_of(&core, &order);
    assert!(
        evidence.is_empty(),
        "no evidence appended, none graded down to a provider's word: {evidence:?}"
    );
    assert!(
        core.readers()
            .put()
            .iter()
            .any(|put| matches!(put.asked, Asked::MovementOf { .. })),
        "the movement was asked, and asked again"
    );
    // Readable again: the next pass reads it, and the order closes on the public record.
    core.plant_readers(&Plan::default());
    assert_eq!(after_ticks(&mut core, &order, 3), OrderState::Complete);
    assert!(
        evidence_of(&core, &order)
            .iter()
            .any(|e| e.grade == rutent_contracts::provider_role::EvidenceGrade::PublicRecord),
        "graded on the public record once read"
    );
}

// verifies: REQ-ORD-33
#[test]
fn final_deposit_to_a_network_whose_reader_is_unreadable_is_not_routed_to() {
    let (mut core, session, listed) = bank_only();
    core.plant_readers(&Plan::everywhere(&listed, "malformed"));
    assert!(matches!(
        core.readers().standing(&bank()),
        Standing::NotStarted(_)
    ));
    let (_, told) = quoted_or_not(&mut core, within("req-1", &session, AMOUNT, RESERVE, OTHER));
    assert!(!is_quoted(&told), "{told:?}");
    // Core's own grading says so too: a deposit to it can't close.
    core.plant_readers(&Plan::default());
    let (_, told) = quoted_or_not(&mut core, within("req-2", &session, AMOUNT, RESERVE, OTHER));
    assert!(is_quoted(&told), "{told:?}");
}

// verifies: REQ-ORD-39
#[test]
fn one_grading_reads_each_movement_once_from_its_reader() {
    let (mut core, session, _) = bank_only();
    let _ = completed(&mut core, &session);
    let bank = bank();
    let movement = MovementId::new("a-movement").unwrap();
    let before = core.readers().put().len();
    {
        let pass = core.public_records();
        for _ in 0..3 {
            pass.rules(&bank).expect("readable");
            pass.status(&bank, &movement).expect("readable");
            pass.movement(&bank, &id("a-transaction"))
                .expect("readable");
        }
    }
    assert_eq!(
        core.readers().put().len() - before,
        3,
        "each question once in a pass"
    );
    {
        let pass = core.public_records();
        pass.status(&bank, &movement).expect("readable");
    }
    assert_eq!(
        core.readers().put().len() - before,
        4,
        "a new pass asks again"
    );
    // In a run, no pass put one question twice: each grading's questions, in its pass, are distinct.
}

// verifies: REQ-MOD-3, DEC-28
#[test]
fn public_records_stay_cores_seam_and_the_deployment_hands_core_no_reader_of_its_own() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut found = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("a directory") {
            let path = entry.expect("an entry").path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.is_dir() {
                if !(name.starts_with('.') || name == "target" || name == "experiments") {
                    stack.push(path);
                }
            } else if name.ends_with(".rs") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                if text.contains(&["impl PublicRecords", " for "].concat())
                    || text.contains(
                        &["impl rutent_core_orch::closing::PublicRecords", " for"].concat(),
                    )
                {
                    found.push(path.strip_prefix(&root).unwrap().to_path_buf());
                }
            }
        }
    }
    for path in &found {
        assert!(
            path.starts_with("components/core-orch"),
            "an impl PublicRecords outside Core: {}",
            path.display()
        );
    }
    assert!(
        found
            .iter()
            .any(|p| p.starts_with("components/core-orch/src/closing/readers")),
        "Core's adapter implements it"
    );
    // The grading and the reversals read through Core's own pass of its adapter (`Public::of`): the type the ports
    // carry is Core's, and a stand-in in its place is a test hook no deployment's build has (the review of round 1,
    // M1). The assembly builds each from the readers Core holds, and lists, checks and raises nothing itself.
    let schedule =
        std::fs::read_to_string(root.join("conformance/scenarios/src/assembly/schedule.rs"))
            .unwrap();
    assert_eq!(
        schedule
            .matches("records: rutent_core_orch::closing::Public::of(&core.readers),")
            .count(),
        2
    );
    let assembly =
        std::fs::read_to_string(root.join("conformance/scenarios/src/assembly/mod.rs")).unwrap();
    for done_by_core in [
        "check_all",
        "readers.list(",
        "Raised",
        ".list_public_network(",
        ".drop_public_network(",
        "may_delist",
    ] {
        assert!(
            !assembly.contains(done_by_core),
            "the assembly does Core's `{done_by_core}`"
        );
    }
    let hooks =
        std::fs::read_to_string(root.join("components/core-orch/src/closing/readers/mod.rs"))
            .unwrap();
    for hook in [
        "pub fn drawing_from",
        "pub fn set_host",
        "pub fn question",
        "pub fn list(",
        "pub fn check_at",
    ] {
        let at = hooks.find(hook).expect("the hook");
        assert!(
            hooks[..at].rfind("#[cfg(").is_some_and(|cfg| hooks[cfg..at]
                .contains("feature = \"reader-test-hooks\"")
                && !hooks[cfg..at].contains("\n    pub fn ")),
            "`{hook}` is a test hook only"
        );
    }
}

// verifies: REQ-ORD-33, AC-1
#[test]
fn order_closes_on_its_final_deposit_as_the_networks_reader_service_reads_it() {
    let (mut core, session, _) = bank_only();
    let order = completed(&mut core, &session);
    let bank = bank();
    let closing = evidence_of(&core, &order)
        .into_iter()
        .rfind(|e| e.grade == rutent_contracts::provider_role::EvidenceGrade::PublicRecord)
        .expect("the close's public record");
    assert_eq!(closing.whose.as_str(), bank);
    let movement = closing.movement.expect("its movement");
    assert_eq!(movement.read_by.reader.as_str(), reader_name(&bank));
    // Its reader was asked of that movement, under a request id of Core's.
    assert!(core.readers().put().iter().any(|put| put.network == bank
        && matches!(&put.asked, Asked::MovementStatus { movement: m } if m == &movement.id)));
}

// verifies: REQ-MDL-11, INV-3
#[test]
fn hostile_reader_leaves_its_segment_unsettled_and_core_running() {
    let (mut core, session, listed) = bank_only();
    // The order's withdrawal handed over while its reader reads rules alone, then the reader turned hostile in turn.
    core.plant_readers(&Plan::everywhere(&listed, "unread-movements"));
    let (order, _) = run(&mut core, within("req-1", &session, AMOUNT, RESERVE, OTHER));
    for act in ["malformed", "other-movement", "crash", "unlisted-key"] {
        core.plant_readers(&Plan::everywhere(&listed, act));
        assert!(
            matches!(core.readers().standing(&bank()), Standing::NotStarted(_)),
            "{act}"
        );
        // Core runs on: its tick returns, the order reads, and nothing settles on what the reader said.
        assert_ne!(
            after_ticks(&mut core, &order, 2),
            OrderState::Complete,
            "{act}"
        );
        assert!(evidence_of(&core, &order).is_empty(), "{act}");
    }
    // The network's standing doesn't change under the order that uses it: its delisting is refused (#346's review, L3).
    let refused = core
        .delist_public_network(&id(&bank()))
        .expect_err("refused while in use");
    assert!(format!("{refused:?}").contains("InUse"), "{refused:?}");
    core.plant_readers(&Plan::default());
    assert_eq!(after_ticks(&mut core, &order, 3), OrderState::Complete);
}

/// A reader that starts and then hangs on a movement question: its host gives it up at its hang bound and answers for it
/// that it can't stand behind an answer (`reader_hung`, `T-015-04`), well before Core's own socket bound, so Core reads
/// it as unreadable, the segment settles nothing and waits, and Core runs on. Once the reader answers again the order
/// completes (the reader timeout path, `T-024-25`'s Low 3).
// verifies: REQ-MDL-11, INV-3
#[test]
fn reader_that_hangs_is_unreadable_once_its_host_gives_it_up_and_its_segment_waits() {
    let (mut core, session, listed) = bank_only();
    let bank = bank();
    core.plant_readers(&Plan::everywhere(&listed, "unread-movements"));
    let (order, _) = run(&mut core, within("req-1", &session, AMOUNT, RESERVE, OTHER));
    core.plant_readers(&Plan::everywhere(&listed, "hang"));
    assert_eq!(
        core.readers().standing(&bank),
        Standing::Started,
        "it answers its rules, so it starts"
    );
    let asked = std::time::Instant::now();
    let read = core
        .public_records()
        .status(&bank, &MovementId::new("any-movement").expect("an id"));
    let waited = asked.elapsed();
    assert!(
        matches!(&read, Err(Unreadable::Reader(why)) if why.as_str() == "reader_hung"),
        "the host answers for the reader it gave up: {read:?}"
    );
    assert!(
        waited < rutent_core_orch::closing::readers::ANSWER_WINDOW,
        "within Core's own bound: {waited:?}"
    );
    assert_ne!(after_ticks(&mut core, &order, 1), OrderState::Complete);
    assert!(evidence_of(&core, &order).is_empty(), "nothing settles");
    core.plant_readers(&Plan::default());
    assert_eq!(after_ticks(&mut core, &order, 3), OrderState::Complete);
}

// verifies: NFR-6, REQ-AUD-4
#[test]
fn listed_reader_is_checked_again_after_core_reopens_and_its_evidence_still_reads() {
    let (mut core, session, listed) = bank_only();
    let order = completed(&mut core, &session);
    let before: Vec<_> = evidence_of(&core, &order);
    let core = rutent_scenarios::harness::Scenario::new("execution", "AC-1", given(&[RESERVE]))
        .reopen(core, 1, &mut rutent_scenarios::assembly::machine::Reference);
    // A reopened Core's adapter is new: its first questions are each listed reader's start-up check.
    let put = core.readers().put();
    assert_eq!(put.len(), listed.len(), "{put:?}");
    for (put, network) in put.iter().zip(&listed) {
        assert_eq!(put.network, network.network.as_str());
        assert!(matches!(put.asked, Asked::NetworkRules(_)));
    }
    assert_eq!(core.readers().standing(&bank()), Standing::Started);
    // The evidence recorded before still reads, with its reader and version.
    assert_eq!(evidence_of(&core, &order), before);
    assert!(
        before
            .iter()
            .filter_map(|e| e.movement.as_ref())
            .all(|m| m.read_by.reader.as_str() == reader_name(&bank()))
    );
}

/// A network's standing doesn't change under a quoted order that uses it (#346's review, L3; the review of round 1, L6):
/// its delisting is refused while the order is quoted, before anything is handed over, as while value is in flight.
// verifies: REQ-CAT-22, DEC-28
#[test]
fn delisting_is_refused_while_a_quoted_order_uses_the_network() {
    let (mut core, session, _) = bank_only();
    let (order, told) = quoted_or_not(&mut core, within("req-1", &session, AMOUNT, RESERVE, OTHER));
    assert!(order.is_some() && is_quoted(&told), "{told:?}");
    let refused = core
        .delist_public_network(&id(&bank()))
        .expect_err("refused while a quoted order uses it");
    assert!(format!("{refused:?}").contains("InUse"), "{refused:?}");
    assert_eq!(core.readers().standing(&bank()), Standing::Started);
}

/// A reopened Core lists and checks every network itself (the review of round 1, M1): Core's own reopening
/// (`schedule::reopen`) takes each listing the log holds into the adapter the deployment built from its certificate,
/// the CA, its clock and its host setting, asks each reader its rules, and puts each network that passes on the
/// catalogue's public list. The assembly has no start-up step of its own to do it (the seam test holds that).
// verifies: NFR-6, REQ-CAT-22
#[test]
fn reopened_core_lists_and_checks_every_network_through_its_own_reopening() {
    let (core, _) = core_with("AC-1", &[RESERVE, "sim-exchange"]);
    let listed = core.public_networks();
    assert!(listed.len() > 1, "{listed:?}");
    let core = rutent_scenarios::harness::Scenario::new(
        "execution",
        "AC-1",
        given(&[RESERVE, "sim-exchange"]),
    )
    .reopen(core, 1, &mut rutent_scenarios::assembly::machine::Reference);
    let mut names: Vec<String> = listed
        .iter()
        .map(|n| n.network.as_str().to_owned())
        .collect();
    names.sort();
    assert_eq!(
        core.readers().listed().keys().cloned().collect::<Vec<_>>(),
        names,
        "every listing the log holds, taken in by Core's reopening"
    );
    for network in &listed {
        let name = network.network.as_str();
        assert_eq!(core.readers().standing(name), Standing::Started, "{name}");
        assert!(core.catalogue().nodes().is_started_network(name), "{name}");
        assert!(
            core.readers()
                .put()
                .iter()
                .any(|put| put.network == name && matches!(put.asked, Asked::NetworkRules(_))),
            "{name}'s reader checked on reopening"
        );
    }
}

/// A leaf certificate handed to Core as the operator's CA is refused as no CA (#343's review, L3, and the CA
/// basic-constraints check), and a listing whose `ca` names another digest than the CA the deployment handed Core,
/// such as a leaf's, reads as unreadable and never starts: a leaf can't stand in for the CA either way.
// verifies: DEC-28
#[test]
fn leaf_handed_as_the_ca_or_listed_by_its_digest_fails_closed() {
    use rcgen::ExtendedKeyUsagePurpose;
    let (core, _, listed) = bank_only();
    let bank = bank();
    let readers = Readers::start(&listed, core.world_endpoints(), &Plan::default());
    let leaf = TestCa::new().issue(HOST_NAME, ExtendedKeyUsagePurpose::ServerAuth);
    assert_eq!(
        rutent_core_orch::closing::readers::ReaderTls::new(
            leaf.chain_pem.as_bytes(),
            readers.core_chain_pem.as_bytes(),
            readers.core_key_pem.as_bytes(),
        )
        .map(|_| ()),
        Err(rutent_core_orch::closing::readers::TlsRefusal::NotACa)
    );
    let mut adapter = readers.core_adapter();
    let mut listing = listed[0].clone();
    if let Some(l) = listing.listing.as_mut() {
        l.reader.ca.value = rutent_contracts::module_interface::DigestValue::new(
            rutent_prov::module::wire::sha256(&leaf.chain[0]),
        )
        .expect("a digest");
    }
    adapter.list(&listing);
    assert_eq!(
        adapter.question(&bank, Asked::NetworkRules(NoDetail {}), &SystemClock),
        Err(Unreadable::OtherCa)
    );
    assert_eq!(
        adapter.check_at(&bank, &SystemClock),
        Standing::NotStarted(NotStarted::Unread(Unreadable::OtherCa))
    );
}

/// Core's start-up check holds the reader's own `network_rules` to the listing, not to the reader host's configuration:
/// a host configured for the network as it is, under a listing that names another identity or another trust rule (an
/// operator's listing that doesn't match the host it names), doesn't start, and the operator is told which.
// verifies: DEC-28, REQ-CAT-22
#[test]
fn reader_whose_rules_differ_from_its_listing_does_not_start() {
    let (core, _, listed) = bank_only();
    let bank = bank();
    let readers = Readers::start(&listed, core.world_endpoints(), &Plan::default());
    let mut adapter = readers.core_adapter();
    assert_eq!(adapter.check_at(&bank, &SystemClock), Standing::Started);
    let other_identity = {
        let mut network = listed[0].clone();
        if let Some(l) = network.listing.as_mut() {
            l.identity = rutent_scenarios::sim::readers::identity_of("another-substrate-network");
        }
        network
    };
    adapter.list(&other_identity);
    assert_eq!(
        adapter.check_at(&bank, &SystemClock),
        Standing::NotStarted(NotStarted::OtherIdentity)
    );
    // A network that failed only its start-up check reads as unreadable in a pass, though its reader answers every
    // question honestly: the gate alone holds it (the review of round 1, L1).
    {
        let pass = adapter.pass();
        let movement = MovementId::new("any-movement").expect("an id");
        assert_eq!(pass.status(&bank, &movement), Err(Unreadable::NotStarted));
        assert_eq!(
            pass.movement(&bank, &id("any-transaction")).map(|_| ()),
            Err(Unreadable::NotStarted)
        );
        assert_eq!(pass.rules(&bank), Err(Unreadable::NotStarted));
    }
    assert!(
        adapter
            .question(&bank, Asked::NetworkRules(NoDetail {}), &SystemClock)
            .is_ok(),
        "its reader answers: only the start-up check's standing makes it unreadable"
    );
    // An EVM network's listing naming its chain id with another pool than the host's: its identity holds, its trust
    // rule doesn't.
    let (evm_core, _) = core_with("AC-1", &[RESERVE, "sim-exchange"]);
    let evm: Vec<PublicNetwork> = evm_core
        .public_networks()
        .into_iter()
        .filter(|n| n.network.as_str() == "polygon")
        .collect();
    let evm_readers = Readers::start(&evm, evm_core.world_endpoints(), &Plan::default());
    let mut evm_adapter = evm_readers.core_adapter();
    assert_eq!(
        evm_adapter.check_at("polygon", &SystemClock),
        Standing::Started
    );
    let mut other_trust = evm[0].clone();
    if let Some(l) = other_trust.listing.as_mut()
        && let rutent_contracts::reader::TrustRule::RpcPool { quorum, .. } = &mut l.trust
    {
        *quorum = 3;
    }
    evm_adapter.list(&other_trust);
    assert_eq!(
        evm_adapter.check_at("polygon", &SystemClock),
        Standing::NotStarted(NotStarted::OtherTrust)
    );
    adapter.list(&listed[0]);
    assert_eq!(adapter.check_at(&bank, &SystemClock), Standing::Started);
}

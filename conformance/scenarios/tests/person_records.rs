//! v0's part of `REQ-CPL` that rests on records rather than on a policy call (`T-021-07`, `DEC-16`): a reserve's proof
//! attributed to it, and one in another's name counting for nothing; a reserve's demand that a person be present
//! reported as the reserve's own outcome; a third-party payer named by reference only; and the record of whose
//! judgement was relied on, and by whom.

mod segments_support;

use rutent_conformance::sim::{Act, Behaviour};
use rutent_contracts::core_internal::{
    Attribution, DeploymentOperator, Entry, Through, What, admit_entry,
};
use rutent_contracts::state_machine::Observed;
use rutent_core_orch::intake::{Status as IntakeStatus, Told};
use rutent_core_orch::segments::{NotCounted, Ran, SegmentOutcome};
use rutent_scenarios::assembly::Staged;
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::harness::{Given, Scenario, base_request};
use rutent_scenarios::kit::{self, AMOUNT};
use segments_support::*;
use serde_json::{Value, json};

fn outcomes(ran: &Ran) -> Vec<SegmentOutcome> {
    match ran {
        Ran::Segments(done) => done.iter().map(|a| a.outcome.clone()).collect(),
        Ran::Refused { refused, .. } => panic!("the part was refused: {refused:?}"),
    }
}

/// A reserve's proof that its user may enact the withdrawal is its own signed answer, which the log attributes to
/// it. The same proof presented in another's name, its maker swapped for another party's, is no one's: Core counts
/// nothing on it and the segment moves no further (`REQ-CPL-8`).
// verifies: REQ-CPL-8
#[test]
fn reserves_proof_in_anothers_name_is_refused() {
    // The reserve's own proof: attributed to it.
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let ran = core.run(&mut world);
    assert!(
        matches!(outcomes(&ran)[0], SegmentOutcome::Settled { .. }),
        "{:?}",
        outcomes(&ran)
    );
    let admitted = core
        .admitted(&world.catalogue)
        .expect("the log admits every entry");
    assert!(
        admitted.iter().any(|e| matches!(e, Entry::Stated(s)
            if matches!(&s.attribution, Attribution::Verified { who } if who.acts.as_str() == RESERVE))),
        "the reserve's answer is attributed to it"
    );

    // In another's name: the maker is another reserve's, the signature still the first's.
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.alter = Some((
        0,
        Box::new(|bytes| {
            let mut value: Value = serde_json::from_slice(&bytes).unwrap();
            value["maker"] = json!("another-reserve");
            serde_json::to_vec(&value).unwrap()
        }),
    ));
    let ran = core.run_over(&catalogue, &mut watched);
    drop(watched);
    world.catalogue = catalogue;
    assert_eq!(
        outcomes(&ran)[0],
        SegmentOutcome::NoResult(NotCounted::Unattributable)
    );
    assert!(
        !core
            .log
            .borrow()
            .inputs()
            .iter()
            .any(|i| matches!(i.observed, Observed::SegmentSettled { .. })),
        "nothing settles on it"
    );
}

/// Where the reserve demands the person be present, its demand is the reserve's: Core reports it as the reserve's own
/// statement, attributed to it, and decides nothing in its place, neither failing the segment nor moving it on, so
/// what the order's actor, an agent or its person, may do there is the reserve's to say (`REQ-CPL-9`). Core passes the
/// reserve nothing of the actor's kind on this path, so the outcome is the same whoever acts for the customer.
// verifies: REQ-CPL-9
#[test]
fn presence_demanded_of_an_agent_is_reported_as_the_reserves_outcome() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world.fleet.world().script_for(
        RESERVE,
        "withdrawal",
        Behaviour::act(Act::RefuseWaitingOnThePerson),
    );
    let decisions_before = decisions(&core);
    let ran = core.run(&mut world);
    let done = outcomes(&ran);
    let [SegmentOutcome::WaitingOnThePerson(statement)] = done.as_slice() else {
        panic!("{done:?}");
    };
    assert_eq!(statement.maker.as_str(), RESERVE, "the reserve's outcome");
    let admitted = core
        .admitted(&world.catalogue)
        .expect("the log admits every entry");
    let index = usize::try_from(u64::from(statement.statement.index)).unwrap();
    assert!(matches!(&admitted[index], Entry::Stated(s)
        if matches!(&s.attribution, Attribution::Verified { who } if who.acts.as_str() == RESERVE)));
    // Core decided nothing of its own on it: the only input is the segment's own start at the reserve, none failed,
    // settled or moved it on, and no decision followed the segment's own move.
    let inputs: Vec<Observed> = core
        .log
        .borrow()
        .inputs()
        .into_iter()
        .map(|i| i.observed.clone())
        .filter(|o| !matches!(o, Observed::RouteChosen { .. }))
        .collect();
    assert_eq!(inputs, vec![Observed::ProviderBound {}], "{inputs:?}");
    assert_eq!(
        decisions(&core),
        decisions_before + 1,
        "only the move's own decision"
    );
}

/// The verified decisions Core's log holds.
fn decisions(core: &Core) -> usize {
    core.log
        .borrow()
        .entries
        .iter()
        .filter(|(e, _)| {
            matches!(e, Entry::Verified(v)
            if rutent_contracts::core_internal::calls::is_decision(&v.what))
        })
        .count()
}

/// After an order paid by a third party, not its customer, every record Core keeps names the payer by reference and
/// nothing else of it: the acceptance holds its reference apart from the requester's and the beneficiary's, and no
/// other entry names it (`REQ-CPL-11`). Its details are the provider's, which received them.
// verifies: REQ-CPL-11
#[test]
fn records_after_a_third_party_payment_hold_a_reference_at_most() {
    const PAYER: &str = "payer-ref-9";
    let (mut core, _) = Scenario::new(
        "a third party pays",
        "AC-28",
        Given::one_reserve(Staged::AtOnce),
    )
    .core(1, &mut Reference);
    let customer = kit::customer();
    let session = core
        .open_session(&customer)
        .expect("the customer's session");
    let mut request = base_request("req-third-party", AMOUNT, &session);
    request.parties.payer = kit::id(PAYER);
    let told = core.request(request).expect("the order is asked for");
    let order = rutent_scenarios::assembly::Core::order_of(&told).expect("the order opens");
    let Told::Exists {
        status: IntakeStatus::Quoted { digest, .. },
        ..
    } = core.quote(&customer, &order).expect("the order is quoted")
    else {
        panic!("the order is quoted");
    };
    core.accept(&customer, &order, &digest)
        .expect("the customer accepts");
    let _ = core.run(&order);

    let entries: Vec<Value> = core
        .log()
        .entries()
        .iter()
        .map(|(_, e)| serde_json::to_value(e).unwrap())
        .collect();
    let named: Vec<&Value> = entries
        .iter()
        .filter(|e| e.to_string().contains(PAYER))
        .collect();
    assert!(!named.is_empty(), "the order's acceptance names its payer");
    for entry in named {
        let what = &entry["what"];
        assert_eq!(
            what["call"], "acceptance",
            "only the acceptance names the payer: {entry}"
        );
        assert_eq!(what["parties"]["payer"], PAYER, "by reference");
        let mut rest = entry.clone();
        rest["what"]["parties"]["payer"] = json!("");
        assert!(
            !rest.to_string().contains(PAYER),
            "nowhere else in it: {entry}"
        );
    }
    // Nothing keyed as a person's detail reaches the log: only references.
    for entry in &entries {
        let text = entry.to_string();
        for detail in [
            "payer.name",
            "payer.address",
            "payer.document",
            "payer.email",
        ] {
            assert!(!text.contains(detail), "{detail} in {entry}");
        }
    }
}

/// The record of a judgement relied on names whose judgement it was and the provider that relied on it, which carries
/// the loss of a false one (`REQ-CPL-12`); the log admits it only where the judgement is that judge's own verified
/// statement, so it can't name another's.
// verifies: REQ-CPL-12
#[test]
fn record_names_whose_judgement_was_relied_on_and_by_whom() {
    let path = format!(
        "{}/../../contracts/core-internal/fixtures/admission/world--reliance.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let case: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let history: Vec<_> = case["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                serde_json::from_value(item["receipt"].clone()).unwrap(),
                serde_json::from_value::<Entry>(item["entry"].clone()).unwrap(),
            )
        })
        .collect();
    let entry: Entry = serde_json::from_value(case["entry"].clone()).unwrap();
    let operator = DeploymentOperator {
        maker: kit::id("operator"),
        party: kit::id("operator"),
    };
    let Entry::Verified(v) = &entry else {
        panic!("a verified entry");
    };
    let What::Reliance {
        judge,
        relied_on_by,
        ..
    } = &v.what
    else {
        panic!("a reliance");
    };
    assert_ne!(
        judge, relied_on_by,
        "the judge and the provider relying on it, apart"
    );
    admit_entry(&history, &entry, Through::General, &operator)
        .expect("the record of whose judgement was relied on, and by whom");

    // Naming a judge other than the statement's maker: refused.
    let mut other = case["entry"].clone();
    other["what"]["judge"] = json!("someone-else");
    let other: Entry = serde_json::from_value(other).unwrap();
    assert!(admit_entry(&history, &other, Through::General, &operator).is_err());
}

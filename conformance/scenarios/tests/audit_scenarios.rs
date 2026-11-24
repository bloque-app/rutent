//! Audit scenarios over the assembled Core (`T-024-07`): the outside checker against a rewrite made with the
//! operator's full access (`AC-22`), checkpoints that look the same whether the period was busy or idle (`AC-37`),
//! and erasure (`AC-36`, `T-020-37`).
//!
//! A scenario runs Core, and a checkpointer reads its log on the schedule the assembly fixes
//! ([`rutent_scenarios::assembly::audit`]). The checker is handed bytes, as an outside checker is: the history's
//! records, the checkpoints as they were received at publication, and their openings. It holds the log's public key,
//! the log's name and the schedule, and no handle on Core.

use rutent_contracts::core_internal::Entry;
use rutent_contracts::module_interface::Digest;
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_contracts::state_machine::OrderState;
use rutent_core_audit::checkpoint::{Finding, Kept, tick_of};
use rutent_core_audit::log::record;
use rutent_core_authz::roles::Permission;
use rutent_core_orch::intake::{Status, Told};
use rutent_scenarios::assembly::checking::{
    self, CHECKPOINT_EVERY, CoreCheckpointer, checker, history_of, openings_of,
};
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, Staged};
use rutent_scenarios::harness::{Given, Scenario, base_request};
use rutent_scenarios::kit::{self, AMOUNT};

/// Core, put together, with a checkpointer over its log and what it published, as an outside checker received it.
struct Watched {
    core: Core,
    checkpointer: CoreCheckpointer,
    /// Every checkpoint published, in the order, as the checker received it.
    received: Vec<Vec<u8>>,
}

impl Watched {
    fn new(name: &'static str) -> Self {
        let (core, _) = Scenario::new(name, "AC-28", Given::one_reserve(Staged::AtOnce))
            .core(1, &mut Reference);
        let checkpointer = core.checkpointer().expect("a checkpointer over the log");
        Watched {
            core,
            checkpointer,
            received: Vec::new(),
        }
    }

    /// The clock moves `seconds`; every checkpoint now due is issued and published, and acknowledged as published.
    fn period(&mut self, seconds: u64) {
        self.core.clock.advance(seconds);
        self.publish();
    }

    fn publish(&mut self) {
        let ran = self.checkpointer.run_due();
        assert!(ran.stopped.is_none(), "{:?}", ran.stopped);
        self.received.extend(ran.published.iter().cloned());
        self.checkpointer.acknowledge(&ran).expect("acknowledged");
    }

    /// The customer's order, requested, quoted, accepted and run, in the session the first one opens.
    fn busy(&mut self, orders: &[(&str, &str)]) {
        let customer = kit::customer();
        let session = self
            .core
            .open_session(&customer)
            .expect("the customer's session");
        for (request, amount) in orders {
            let told = self
                .core
                .request(base_request(request, amount, &session))
                .expect("the request opens an order");
            let order = Core::order_of(&told).expect("an order");
            let Told::Exists {
                status: Status::Quoted { digest, .. },
                ..
            } = self.core.quote(&customer, &order).expect("quoted")
            else {
                panic!("a quote is shown");
            };
            self.core
                .accept(&customer, &order, &digest)
                .expect("accepted");
            self.core.run(&order).expect("the order runs");
            let state = self
                .core
                .status(&customer, &order)
                .expect("the customer reads its order")
                .state;
            assert_eq!(state, OrderState::Complete, "{request} completed");
        }
    }

    fn history(&self) -> Vec<Vec<u8>> {
        history_of(&self.core.log())
    }

    fn openings(&self) -> Vec<Vec<u8>> {
        openings_of(self.checkpointer.kept())
    }

    /// The tick the last checkpoint is of.
    fn through(&self) -> Option<u64> {
        self.checkpointer
            .kept()
            .last()
            .and_then(|k| tick_of(k.opening.tick.as_str()))
    }

    /// What the outside checker finds, handed `history` and what it received at publication, and the operator's
    /// openings.
    fn findings(
        &self,
        history: &[Vec<u8>],
        received: &[Vec<u8>],
        openings: &[Vec<u8>],
    ) -> Vec<Finding> {
        checker()
            .check(history, received, openings, self.through())
            .findings
    }
}

/// A run: a tick before anything runs, an order in the next period, and two ticks more after it. Its checkpoints,
/// as received, commit to the history as it stood at each.
fn run_of(amount: &str) -> Watched {
    let mut watched = Watched::new("a log watched from outside");
    watched.publish();
    watched.period(CHECKPOINT_EVERY);
    watched.busy(&[("req-1", amount)]);
    watched.period(CHECKPOINT_EVERY);
    watched.period(CHECKPOINT_EVERY);
    watched
}

/// A copy of `history` with the record at `index` rewritten by `change`, canonical as a record is, so that it is
/// still an entry: what whoever holds the file and the operator's full access can write.
fn rewritten(history: &[Vec<u8>], index: usize, change: impl FnOnce(&mut Entry)) -> Vec<Vec<u8>> {
    let mut entry: Entry = serde_json::from_slice(&history[index]).expect("a record is an entry");
    change(&mut entry);
    let mut out = history.to_vec();
    out[index] = record(&entry).expect("a rewritten entry is still a record");
    assert_ne!(out[index], history[index], "the rewrite changed the record");
    out
}

// --- AC-22 -----------------------------------------------------------------------------------------------------------

/// The operator, with full access to Core's log, its stores and the log's own signing key, rewrites past records.
/// The outside checker, which held each checkpoint as it was published, finds it, and says where: one record
/// altered fails the checkpoints that committed to a history holding it and none before; records removed fail
/// every checkpoint that committed to more; and a whole history rewritten and checkpointed again from its start,
/// which checks out alone, fails against the copies the checker kept.
// verifies: AC-22, REQ-AUD-9, INV-6
#[test]
fn rewrite_with_full_operator_access_is_detected_by_the_outside_checker() {
    let watched = run_of(AMOUNT);
    let history = watched.history();
    let openings = watched.openings();
    let ticks = watched.checkpointer.kept().len();
    assert!(ticks >= 4, "four ticks fell: {ticks}");
    assert_eq!(watched.received.len(), ticks, "each was published");

    // Held against what was published, the history the operator kept checks out.
    assert_eq!(
        watched.findings(&history, &watched.received, &openings),
        Vec::<Finding>::new(),
        "an untouched history holds"
    );

    // One record altered, late in the history: it is a stated entry received after the order ran. The checkpoints of
    // the ticks that committed to a history holding it fail; those of ticks before it was written hold.
    let sizes: Vec<usize> = watched
        .checkpointer
        .kept()
        .iter()
        .map(|k| usize::try_from(u64::from(k.opening.size)).expect("a size"))
        .collect();
    let late = history
        .iter()
        .rposition(|r| matches!(serde_json::from_slice::<Entry>(r), Ok(Entry::Stated(_))))
        .expect("a stated entry written during the order");
    assert!(
        sizes[0] <= late && late < sizes[2],
        "the entry falls after tick 0 and before the order's tick: {sizes:?}, {late}"
    );
    let altered = rewritten(&history, late, |entry| {
        let Entry::Stated(s) = entry else {
            unreachable!("picked a stated entry")
        };
        s.received_at = Timestamp::new("2026-11-09T00:00:00Z").expect("a time");
    });
    let found = watched.findings(&altered, &watched.received, &openings);
    let failed: Vec<u64> = found
        .iter()
        .filter_map(|f| match f {
            Finding::DoesNotHold { tick } => Some(*tick),
            _ => None,
        })
        .collect();
    let expected: Vec<u64> = (0..ticks as u64)
        .filter(|t| sizes[*t as usize] > late)
        .collect();
    assert!(!expected.is_empty(), "some tick committed to the record");
    assert_eq!(failed, expected, "{found:?}");
    assert_eq!(
        found.len(),
        expected.len(),
        "nothing else is found: {found:?}"
    );

    // Records removed from the end: the checkpoints that committed to them name more entries than the history holds.
    let shortened = history[..history.len() - 3].to_vec();
    let found = watched.findings(&shortened, &watched.received, &openings);
    assert!(
        found
            .iter()
            .any(|f| matches!(f, Finding::HistoryShorter { .. })),
        "{found:?}"
    );

    // The whole history rewritten, as an operator who holds the log's key can: the same run with another amount, its
    // schedule replayed over it with the log's own key. Alone, with its own checkpoints, it checks out: a checker
    // that kept nothing it was sent at publication can't see a rewrite.
    let other = run_of("7000000");
    let their_history = other.history();
    assert_ne!(their_history, history, "the rewrite is another history");
    assert_eq!(
        other.findings(&their_history, &other.received, &other.openings()),
        Vec::<Finding>::new(),
        "a rewrite checkpointed again checks out alone"
    );
    // Against the copies the checker received as each checkpoint was published, it fails at every tick that
    // committed to the history before it was rewritten, and the new checkpoints sit beside them as a second word
    // for each tick.
    let found = watched.findings(&their_history, &watched.received, &openings);
    let mut failed: Vec<u64> = found
        .iter()
        .filter_map(|f| match f {
            Finding::DoesNotHold { tick } => Some(*tick),
            _ => None,
        })
        .collect();
    failed.dedup();
    assert_eq!(
        failed,
        (0..ticks as u64).collect::<Vec<_>>(),
        "every checkpoint received fails the rewrite: {found:?}"
    );
    let both: Vec<Vec<u8>> = watched
        .received
        .iter()
        .chain(&other.received)
        .cloned()
        .collect();
    let both_openings: Vec<Vec<u8>> = openings.iter().chain(&other.openings()).cloned().collect();
    let found = watched.findings(&their_history, &both, &both_openings);
    assert!(
        found
            .iter()
            .any(|f| matches!(f, Finding::Equivocation { .. })),
        "two words for one tick: the operator said two things: {found:?}"
    );

    // An operator who withholds the openings is itself a finding.
    let found = watched.findings(&history, &watched.received, &[]);
    assert!(
        found.iter().any(|f| matches!(f, Finding::NoOpening { .. })),
        "{found:?}"
    );

    // The checker never held Core: its key is the log's, not the operator's.
    assert_ne!(
        checking::log_public_key(),
        rutent_scenarios::assembly::context()
            .deployment
            .operator_key
    );
}

// --- AC-37 -----------------------------------------------------------------------------------------------------------

fn bytes_of(kept: &[Kept]) -> Vec<Vec<u8>> {
    kept.iter()
        .map(|k| serde_json::to_vec(&k.checkpoint).expect("a checkpoint is JSON"))
        .collect()
}

/// Two idle periods, a busy one (two orders), and two idle again: one checkpoint for each tick of the schedule, every
/// one of them the same length and made of the same members, on the schedule's own times, none repeating the one
/// before it where the log didn't move, and none a commitment an observer who knows the log's size and root can
/// reproduce without the opening. Altered history fails against any of them.
// verifies: AC-37, REQ-AUD-14
#[test]
fn busy_and_idle_periods_give_indistinguishable_checkpoints() {
    let mut watched = Watched::new("idle and busy periods");
    watched.publish();
    watched.period(CHECKPOINT_EVERY);
    let before_the_busy_period = watched.checkpointer.kept().len();
    watched.busy(&[("req-1", AMOUNT), ("req-2", "2000000")]);
    watched.period(CHECKPOINT_EVERY);
    watched.period(CHECKPOINT_EVERY);
    watched.period(CHECKPOINT_EVERY);

    let kept = watched.checkpointer.kept();
    let sizes: Vec<u64> = kept.iter().map(|k| u64::from(k.opening.size)).collect();
    // The log moved a great deal in the busy period and not at all in the idle ones around it.
    let busy_tick = before_the_busy_period;
    assert!(
        sizes[busy_tick] > sizes[busy_tick - 1] + 20,
        "the busy period wrote many records: {sizes:?}"
    );
    assert_eq!(
        sizes[busy_tick],
        sizes[busy_tick + 1],
        "an idle period wrote none: {sizes:?}"
    );
    assert_eq!(sizes[busy_tick + 1], sizes[busy_tick + 2], "{sizes:?}");
    assert_eq!(sizes[0], sizes[1], "{sizes:?}");

    // One checkpoint for each tick, in order, none skipped and none added, on the schedule's own times.
    let schedule = checking::schedule();
    let published = bytes_of(kept);
    assert_eq!(
        watched.received, published,
        "what was published is what was kept"
    );
    for (tick, k) in kept.iter().enumerate() {
        assert_eq!(tick_of(k.checkpoint.tick.as_str()), Some(tick as u64));
        assert_eq!(
            Some(k.checkpoint.at.clone()),
            schedule.at(tick as u64),
            "tick {tick}'s time"
        );
    }

    // They can't be told apart by what they are: one length, one set of members, whatever the period held.
    let lengths: std::collections::BTreeSet<usize> = published.iter().map(Vec::len).collect();
    assert_eq!(
        lengths.len(),
        1,
        "every checkpoint has the same length: {lengths:?}"
    );
    let members: std::collections::BTreeSet<Vec<String>> = published
        .iter()
        .map(|p| {
            let v: serde_json::Value = serde_json::from_slice(p).expect("JSON");
            v.as_object().expect("an object").keys().cloned().collect()
        })
        .collect();
    assert_eq!(members.len(), 1, "one set of members: {members:?}");
    for member in members.iter().next().expect("a set") {
        assert!(
            !["size", "root", "count", "entries", "volume"].contains(&member.as_str()),
            "a checkpoint publishes no {member}"
        );
    }

    // Where the log didn't move, the checkpoint isn't the same as the one before: no repeat shows the idleness.
    let commitments: Vec<_> = kept
        .iter()
        .map(|k| k.checkpoint.commitment.clone())
        .collect();
    let distinct: std::collections::BTreeSet<String> =
        commitments.iter().map(|c| format!("{c:?}")).collect();
    assert_eq!(
        distinct.len(),
        commitments.len(),
        "no two commitments are alike"
    );

    // An observer who holds the history, and so knows each tick's size and root, can't make the commitment without
    // the opening's salt: the guesses a person would try don't give it.
    let log = Id::new(rutent_scenarios::assembly::LOG_NAME).expect("a name");
    let history = watched.history();
    let leaves: Vec<_> = history
        .iter()
        .map(|r| rutent_core_audit::log::merkle::leaf_hash(r))
        .collect();
    for (tick, k) in kept.iter().enumerate() {
        let size = usize::try_from(u64::from(k.opening.size)).expect("a size");
        let root = rutent_core_audit::log::merkle::root(&leaves[..size]);
        for salt in [vec![], vec![0u8; 32]] {
            assert_ne!(
                rutent_core_audit::checkpoint::commitment(
                    &log,
                    tick as u64,
                    &salt,
                    size as u64,
                    &root
                ),
                k.checkpoint.commitment,
                "tick {tick}: an unsalted commitment to the known root"
            );
        }
    }

    // Altered history fails against one: a record of the busy period altered, the checkpoint of an idle tick after
    // it no longer holds, though nothing was written in its own period.
    let openings = watched.openings();
    let target = history.len() - 5;
    let altered = rewritten(&history, target, |entry| match entry {
        Entry::Stated(s) => s.received_at = Timestamp::new("2026-11-09T00:00:00Z").expect("a time"),
        Entry::Verified(v) => v.when.at = Timestamp::new("2026-11-09T00:00:00Z").expect("a time"),
    });
    let found = watched.findings(&altered, &watched.received, &openings);
    let last_idle = (kept.len() - 1) as u64;
    assert!(
        found.contains(&Finding::DoesNotHold { tick: last_idle }),
        "the last, idle tick's checkpoint fails against altered history: {found:?}"
    );
    assert_eq!(
        watched.findings(&history, &watched.received, &openings),
        Vec::<Finding>::new(),
        "and the history as it was holds"
    );
}

// --- AC-36 -----------------------------------------------------------------------------------------------------------

/// A statement of the run's history the store keeps and nothing rests on, which the store itself would let go.
fn erasable_statement(watched: &Watched) -> Digest {
    watched
        .history()
        .iter()
        .filter_map(|r| match serde_json::from_slice::<Entry>(r) {
            Ok(Entry::Stated(s)) => Some(s.commitment),
            _ => None,
        })
        .find(|c| watched.core.bound().erasable(c).is_ok())
        .expect("a statement nothing rests on")
}

/// The kinds of the verified entries of the history from `from` on, in order.
fn calls_from(watched: &Watched, from: usize) -> Vec<String> {
    watched.history()[from..]
        .iter()
        .filter_map(|r| match serde_json::from_slice::<Entry>(r) {
            Ok(Entry::Verified(v)) => Some(
                serde_json::to_value(&v.what).expect("a what")["call"]
                    .as_str()
                    .expect("a call")
                    .to_owned(),
            ),
            _ => None,
        })
        .collect()
}

/// The operator's login asks Core to erase a statement the store keeps, nothing rests on and policy lets it erase:
/// Core records the decision, the effect, its done result and the `erasure` entry, the store drops the bytes, and the
/// reference stays. The statement's entry is in the history as it was, so are the checkpoints published before, and
/// the outside checker, handed bytes alone, still finds nothing wrong, then and after a later checkpoint. This is
/// the assembled Core's scenario for `AC-36` (`T-020-37`), in an `F-024` file; the row's other tests are in
/// `components/core-orch/tests/erasure.rs`.
// verifies: AC-36, REQ-AUD-13
#[test]
fn erasure_leaves_the_reference_and_history_checks() {
    use rutent_contracts::core_internal::What;

    let mut watched = Watched::new("an erasure asked of Core");
    watched.publish();
    watched.busy(&[("req-1", AMOUNT)]);
    watched.period(CHECKPOINT_EVERY);

    let commitment = erasable_statement(&watched);
    let before = watched.history();
    let received = watched.received.clone();
    assert_eq!(
        watched.findings(&before, &received, &watched.openings()),
        Vec::<Finding>::new(),
        "the history checks before"
    );
    assert!(!watched.core.bound().is_erased(&commitment));

    let erased = watched
        .core
        .erase_statement(&commitment, "retention")
        .expect("the operator's login erases a statement nothing rests on");

    // Erased outside the log: the store answers erased, and finds nothing missing or left unsettled.
    assert!(watched.core.bound().is_erased(&commitment));
    assert!(watched.core.bound().missing().is_empty());
    assert!(watched.core.bound().refused_erasures().is_empty());
    assert!(watched.core.bound().unsettled_erasures().is_empty());

    // The record: the decision, the effect before the result, and the erasure naming the commitment that stays.
    let history = watched.history();
    assert_eq!(
        calls_from(&watched, before.len()),
        ["decide", "effect", "result", "erasure"]
    );
    assert_eq!(history.len(), before.len() + 4);
    let named: Vec<_> = history[before.len()..]
        .iter()
        .filter_map(|r| match serde_json::from_slice::<Entry>(r) {
            Ok(Entry::Verified(v)) => match v.what {
                What::Erasure { evidence } => Some(evidence),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(
        named,
        std::slice::from_ref(&commitment),
        "the erasure names the reference"
    );
    assert!(erased.effect.index < erased.result.index && erased.result.index < erased.entry.index);

    // The reference stays: everything that was in the history is, byte for byte, including the statement's own entry.
    assert_eq!(&history[..before.len()], &before[..]);
    assert!(history.iter().any(|r| matches!(
        serde_json::from_slice::<Entry>(r),
        Ok(Entry::Stated(s)) if s.commitment == commitment
    )));

    // And history still checks, against what was published before the erasure and after it.
    let openings = watched.openings();
    assert_eq!(
        watched.findings(&history, &received, &openings),
        Vec::<Finding>::new(),
        "history checks against the checkpoints published before the erasure"
    );
    watched.period(CHECKPOINT_EVERY);
    assert_eq!(
        watched.findings(&watched.history(), &watched.received, &watched.openings()),
        Vec::<Finding>::new(),
        "and against one published after it"
    );
}

/// Policy doesn't permit the erasure (a login the register doesn't list for it, here the customer's own): nothing is
/// decided to take effect, nothing is erased, the reference stays, the store refuses to erase on the refusal's own
/// receipt, and the history still checks. A statement an entry rests on is refused before policy is asked.
// verifies: DEC-18
#[test]
fn erasure_without_a_permitting_decision_is_refused_in_the_assembled_core() {
    use rutent_contracts::core_internal::calls::AppendFailure;
    use rutent_contracts::core_internal::{Violation, What};
    use rutent_core_audit::receipts::store::{EVIDENCE_RESTED_ON, NO_DECIDED_ERASURE};
    use rutent_core_orch::erasure::NotErased;

    let mut watched = Watched::new("an erasure asked of Core");
    watched.publish();
    watched.busy(&[("req-1", AMOUNT)]);
    let commitment = erasable_statement(&watched);
    let before = watched.history();

    let refused = watched
        .core
        .erase_statement_as(&kit::customer(), &commitment, "retention");
    let Err(NotErased::NotPermitted(answer)) = refused.map_err(|e| *e) else {
        panic!("a customer is let erase")
    };
    assert!(matches!(
        answer,
        rutent_contracts::core_internal::Answer::Refuse { .. }
    ));
    assert!(!watched.core.bound().is_erased(&commitment));
    assert_eq!(
        calls_from(&watched, before.len()),
        ["decide"],
        "only the refusal is recorded: no effect, no result, no erasure"
    );

    // The store, handed the refusal's receipt as if it were a decided erasure, refuses.
    let receipt = watched
        .core
        .log()
        .entries()
        .iter()
        .rev()
        .find_map(|(r, e)| match e {
            Entry::Verified(v) if matches!(v.what, What::Decide { .. }) => Some(r.clone()),
            _ => None,
        })
        .expect("the refusal's entry");
    assert_eq!(
        watched.core.bound().erase(&receipt),
        Err(AppendFailure::Refused(Violation(NO_DECIDED_ERASURE)))
    );

    // A statement an entry rests on is refused before anything is asked of policy.
    let cited = watched
        .history()
        .iter()
        .filter_map(|r| match serde_json::from_slice::<Entry>(r) {
            Ok(Entry::Stated(s)) => Some(s.commitment),
            _ => None,
        })
        .find(|c| {
            watched.core.bound().erasable(c)
                == Err(AppendFailure::Refused(Violation(EVIDENCE_RESTED_ON)))
        })
        .expect("a statement an entry rests on");
    let length = watched.history().len();
    assert!(matches!(
        watched
            .core
            .erase_statement(&cited, "retention")
            .map_err(|e| *e),
        Err(NotErased::NotErasable(_))
    ));
    assert_eq!(watched.history().len(), length, "nothing was recorded");

    watched.period(CHECKPOINT_EVERY);
    assert_eq!(
        watched.findings(&watched.history(), &watched.received, &watched.openings()),
        Vec::<Finding>::new()
    );
}

// --- Erasure that overrides citations (T-020-42, item erasure-of-cited-evidence) ---------------------------------------

/// A statement of the run's history the store keeps and a later entry rests on: refused by default.
fn cited_statement(watched: &Watched) -> Digest {
    use rutent_contracts::core_internal::Violation;
    use rutent_contracts::core_internal::calls::AppendFailure;
    use rutent_core_audit::receipts::store::EVIDENCE_RESTED_ON;

    watched
        .history()
        .iter()
        .filter_map(|r| match serde_json::from_slice::<Entry>(r) {
            Ok(Entry::Stated(s)) => Some(s.commitment),
            _ => None,
        })
        .find(|c| {
            watched.core.bound().erasable(c)
                == Err(AppendFailure::Refused(Violation(EVIDENCE_RESTED_ON)))
        })
        .expect("a statement an entry rests on")
}

/// A run with one order done and checkpoints published, and a statement a later entry rests on.
fn watched_with_a_cited_statement() -> (Watched, Digest) {
    let mut watched = Watched::new("an override asked of Core");
    watched.publish();
    watched.busy(&[("req-1", AMOUNT)]);
    watched.period(CHECKPOINT_EVERY);
    let cited = cited_statement(&watched);
    (watched, cited)
}

/// Asked for no override, Core keeps a statement a later entry rests on, and nothing is recorded.
// verifies: REQ-AUD-13
#[test]
fn an_erasure_asked_without_the_override_keeps_a_cited_statement() {
    use rutent_core_orch::erasure::NotErased;

    let (mut watched, cited) = watched_with_a_cited_statement();
    watched
        .core
        .registers()
        .permit(&kit::operator_login(), Permission::EraseOverridingCitations);
    let length = watched.history().len();
    assert!(matches!(
        watched
            .core
            .erase_statement(&cited, "retention")
            .map_err(|e| *e),
        Err(NotErased::NotErasable(_))
    ));
    assert_eq!(watched.history().len(), length, "nothing was recorded");
    assert!(!watched.core.bound().is_erased(&cited));
}

/// An operator's login holding both the erase permission and the override's asks Core to erase a statement a later
/// entry rests on, as an override: policy permits it, the statement's bytes go, the entries citing it stay as they
/// are, the store finds nothing wrong, and the outside checker, handed bytes alone, finds history intact, against
/// checkpoints published before and after.
// verifies: REQ-AUD-13, AC-36
#[test]
fn an_operator_holding_both_permissions_erases_a_cited_statement_and_history_checks() {
    let (mut watched, cited) = watched_with_a_cited_statement();
    watched
        .core
        .registers()
        .permit(&kit::operator_login(), Permission::EraseOverridingCitations);
    let before = watched.history();
    let received = watched.received.clone();
    assert_eq!(
        watched.findings(&before, &received, &watched.openings()),
        Vec::<Finding>::new()
    );

    let erased = watched
        .core
        .erase_statement_overriding(&cited, "retention")
        .expect("an operator holding both permissions overrides");

    assert!(watched.core.bound().is_erased(&cited));
    assert!(watched.core.bound().refused_erasures().is_empty());
    assert!(watched.core.bound().unsettled_erasures().is_empty());
    assert_eq!(
        calls_from(&watched, before.len()),
        ["decide", "effect", "result", "erasure"]
    );
    assert!(erased.effect.index < erased.result.index && erased.result.index < erased.entry.index);
    // The entries that cite it stay as they were, byte for byte.
    let history = watched.history();
    assert_eq!(&history[..before.len()], &before[..]);
    let openings = watched.openings();
    assert_eq!(
        watched.findings(&history, &received, &openings),
        Vec::<Finding>::new(),
        "history checks against the checkpoints published before the override"
    );
    watched.period(CHECKPOINT_EVERY);
    assert_eq!(
        watched.findings(&watched.history(), &watched.received, &watched.openings()),
        Vec::<Finding>::new(),
        "and against one published after it"
    );
}

/// An operator's login holding the erase permission alone is refused the override: the refusal is recorded, no effect,
/// nothing is erased, and history checks.
// verifies: REQ-AUZ-1, REQ-AUD-13
#[test]
fn an_operator_holding_only_the_erase_permission_is_refused_the_override() {
    use rutent_contracts::core_internal::Answer;
    use rutent_core_orch::erasure::NotErased;

    let (mut watched, cited) = watched_with_a_cited_statement();
    let before = watched.history().len();
    let refused = watched
        .core
        .erase_statement_overriding(&cited, "retention")
        .map_err(|e| *e);
    assert!(
        matches!(refused, Err(NotErased::NotPermitted(Answer::Refuse { .. }))),
        "{refused:?}"
    );
    assert!(!watched.core.bound().is_erased(&cited));
    assert_eq!(
        calls_from(&watched, before),
        ["decide"],
        "only the refusal is recorded"
    );
    watched.period(CHECKPOINT_EVERY);
    assert_eq!(
        watched.findings(&watched.history(), &watched.received, &watched.openings()),
        Vec::<Finding>::new()
    );
}

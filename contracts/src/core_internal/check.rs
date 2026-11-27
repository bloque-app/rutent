//! The rules JSON Schema can't state, and every list rule the schemas state, checked by each shape's `check`.
//!
//! Every `check` recurses into the shapes it holds, so a value built in memory is held to the same rules as one
//! read from the wire. Deserialising a shape with rules of its own runs its `check` too. The callee of every call in
//! [`calls`](super::calls) runs `check` on what it is handed and refuses a value that fails it.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::Violation;
use super::types::*;

/// True when no item of `items` repeats.
fn distinct<T: PartialEq>(items: &[T]) -> bool {
    items
        .iter()
        .enumerate()
        .all(|(i, item)| !items[..i].contains(item))
}

/// `Ok` when `items` holds at least one item and none twice, else the violation.
fn listed<T: PartialEq>(items: &[T], rule: &'static str) -> Result<(), Violation> {
    if items.is_empty() || !distinct(items) {
        broke(rule)?;
    }
    Ok(())
}

/// The refusal of `rule`, broken: `Err` unless a contract test switched the rule off on this thread
/// (`without_rule`), so it can see that a value it reads breaks that rule alone (`T-020-45`).
fn broke(rule: &'static str) -> Result<(), Violation> {
    if switched_off(rule) {
        Ok(())
    } else {
        Err(Violation(rule))
    }
}

#[cfg(feature = "rule-switches")]
std::thread_local! {
    static OFF: core::cell::RefCell<alloc::vec::Vec<&'static str>> = const { core::cell::RefCell::new(alloc::vec::Vec::new()) };
}

#[cfg(feature = "rule-switches")]
fn switched_off(rule: &str) -> bool {
    OFF.with(|off| off.borrow().contains(&rule))
}

#[cfg(not(feature = "rule-switches"))]
fn switched_off(_: &str) -> bool {
    false
}

/// Runs `f` with the shape rule `rule`, as its refusal names it, switched off on this thread, so a contract test sees
/// whether an invalid fixture breaks that rule alone: read with it off, the fixture is accepted exactly when no other
/// rule refuses it (`T-020-45`, the milestone review's Low on fixtures pinned by their first refusal). Built only with
/// the `rule-switches` feature, which the contract's own tests turn on and nothing shipped does.
#[cfg(feature = "rule-switches")]
pub fn without_rule<T>(rule: &'static str, f: impl FnOnce() -> T) -> T {
    OFF.with(|off| off.borrow_mut().push(rule));
    let out = f();
    OFF.with(|off| {
        off.borrow_mut().retain(|r| *r != rule);
    });
    out
}

impl Action {
    /// Checks the rules of an action the schema can't state.
    pub fn check(&self) -> Result<(), Violation> {
        // A read of a private node's record names the transactions it is about, at least one, each once (`T-020-58`).
        if let Action::ReadNodeRecord { about, .. } = self
            && (about.is_empty() || !distinct(about))
        {
            broke(
                "a read of a private node's record names the transactions it is about, at least one, each once",
            )?;
        }
        if let Action::ChangeCatalogue {
            change,
            version,
            tuple,
            class,
            advertising_keys,
            statement_key,
            setting,
            provider,
            versions,
        } = self
        {
            if change.is_about_a_release() != version.is_some() {
                broke(
                    "a catalogue change names a version exactly when it is about a module release",
                )?;
            }
            // A verifier's grant, and its revocation, name its class in place of a tuple (`T-020-30`).
            if class.is_some()
                && (!matches!(
                    change,
                    CatalogueChange::Grant | CatalogueChange::RevokeGrant
                ) || tuple.is_some())
            {
                broke(
                    "a catalogue change names a verifier's class only as a grant or its revocation, in place of a tuple",
                )?;
            }
            if change.is_about_a_tuple() != (tuple.is_some() || class.is_some()) {
                broke("a catalogue change names a tuple exactly when it is about a tuple")?;
            }
            let grant = *change == CatalogueChange::Grant;
            if grant != !advertising_keys.is_empty() || !distinct(advertising_keys) {
                broke(
                    "a catalogue change names advertising keys, at least one, each once, exactly when it is a grant",
                )?;
            }
            if (*change == CatalogueChange::AcceptModule) != statement_key.is_some() {
                broke(
                    "a catalogue change names a statement key exactly when it accepts a module release",
                )?;
            }
            // An operator's setting names itself and its value, and nothing of a release or a tuple (`T-020-45`).
            if change.is_a_setting() != setting.is_some() {
                broke("a catalogue change names an operator's setting exactly when it sets one")?;
            }
            // A confirmation of a provider's releases names the versions it puts back, and nothing else does
            // (`T-020-49`).
            if change.confirms_releases() == versions.is_empty() || !distinct(versions) {
                broke(
                    "a catalogue change names the versions it confirms, at least one, each once, exactly when it confirms a provider's releases",
                )?;
            }
            // A provider's own cadence is the status question's alone (`T-020-49`).
            if let Some(OperatorSetting::Cadence {
                job,
                provider: Some(_),
                ..
            }) = setting.as_deref()
                && job.as_str() != STATUS_ASKS_JOB
            {
                broke("a cadence names a provider only for the status question's job")?;
            }
            // A setting about a provider (a tuple's, or a provider's ceiling) names that provider, so policy and the
            // log hold it against the provider whose value it sets (the review of `1bb5f4cb`).
            if setting
                .as_ref()
                .and_then(|s| s.provider())
                .is_some_and(|p| p != provider)
            {
                broke("an operator's setting about a provider names that provider")?;
            }
        }
        Ok(())
    }
}

impl DecisionRequest {
    /// Checks the action it asks for, which is never a stuck order's move: that travels only through
    /// `StuckOrders::move_stuck`, after its decide step (ADR-0011).
    pub fn check(&self) -> Result<(), Violation> {
        if matches!(self.action, Action::ResolveStuck { .. }) {
            broke(
                "a decision request never asks to move a stuck order's value: only a stuck move does",
            )?;
        }
        self.action.check()
    }
}

impl Effect {
    /// Checks the action it takes, which is never a customer's acceptance of fault: that is a decision alone, and
    /// takes no effect.
    pub fn check(&self) -> Result<(), Violation> {
        effect_action(&self.action)
    }
}

/// An effect's action: one that takes effect, never a customer's acceptance of fault.
fn effect_action(action: &Action) -> Result<(), Violation> {
    if matches!(action, Action::AcceptFault { .. }) {
        broke("a customer's acceptance of fault is a decision, and takes no effect")?;
    }
    if matches!(action, Action::RefileArrival { .. }) {
        broke(
            "filing an arrival anew is a decision, and takes no effect: the refiling entry records it",
        )?;
    }
    action.check()
}

impl StuckDecide {
    /// Checks that the decision names its evidence.
    pub fn check(&self) -> Result<(), Violation> {
        listed(
            &self.evidence,
            "a stuck order's decision names its evidence, at least one item, each once",
        )
    }
}

impl Authorisation {
    /// Checks that the parts it names, where it names any, are at least one, each once.
    pub fn check(&self) -> Result<(), Violation> {
        match &self.parts {
            Some(parts) => listed(
                parts,
                "an authorisation names the parts it covers, at least one, each once",
            ),
            None => Ok(()),
        }
    }
}

impl SessionRequest {
    /// Checks that the session it asks for ends after it is asked for.
    pub fn check(&self) -> Result<(), Violation> {
        if self.until.unix_nanos() <= self.at.unix_nanos() {
            broke("a session ends after the time it is asked for")?;
        }
        Ok(())
    }
}

impl Charge {
    /// Checks that the work is charged to the accountable party the caller acts for (`REQ-AUZ-8`).
    pub fn check(&self) -> Result<(), Violation> {
        if self.party != self.caller.party {
            broke("work is charged to the accountable party its caller acts for")?;
        }
        Ok(())
    }
}

impl PersonQuestion {
    /// Checks the lists of a question about a person, its authority's included.
    pub fn check(&self) -> Result<(), Violation> {
        listed(
            &self.ask,
            "a question names who it asks, at least one, each once",
        )?;
        if !distinct(&self.asked_before) {
            broke("a question names each provider already asked about its order once")?;
        }
        if let Authority::ShortlistedRoute {
            providers,
            verifiers,
            ..
        } = &self.authority
        {
            listed(
                providers,
                "a shortlisted route names its providers, at least one, each once",
            )?;
            if !distinct(verifiers) {
                broke("a shortlisted route names each verifier its tuples need once")?;
            }
            // The verifiers that receive the subject are asked too, and recorded and bounded with the providers
            // (`REQ-CPL-5`); anyone else is off the route.
            if !self
                .ask
                .iter()
                .all(|p| providers.contains(p) || verifiers.contains(p))
            {
                broke(
                    "a question on a shortlisted route asks only the shortlisted providers and the verifiers its tuples need",
                )?;
            }
        }
        Ok(())
    }
}

impl PermissionChange {
    /// Checks that the grantee is a regulator or a checker, and that the operator grants or revokes it
    /// (`REQ-AUZ-7`).
    pub fn check(&self) -> Result<(), Violation> {
        if !matches!(self.grantee.role, Role::Regulator | Role::Checker) {
            broke("a read permission is a regulator's or a checker's")?;
        }
        if self.by.role != Role::Operator {
            broke("a read permission is granted or revoked by the operator")?;
        }
        Ok(())
    }
}

impl ReportPair {
    /// Checks that the pair's two reports are two parties' (`REQ-ORD-9`), each about its own segment, the pair's
    /// the sent one's and the received one's another (`T-020-38`), and that a gross figure holds no more charge than
    /// it states (`REQ-CAT-11`), so the comparison can take the charge off it.
    pub fn check(&self) -> Result<(), Violation> {
        if self.sent.party == self.received.party {
            broke("a segment's two reports are by two different parties")?;
        }
        if self.segment != self.sent.segment {
            broke("a report pair's segment is the one its sent report is about")?;
        }
        if self.sent.segment == self.received.segment {
            broke("a report pair's received report is about another segment than its sent report")?;
        }
        if let ReportedAs::Gross { charge } = &self.terms.reporting
            && self.sent.amount.checked_sub(charge).is_none()
        {
            broke("a gross report's charge is no more than the amount sent")?;
        }
        Ok(())
    }
}

impl HoldingReading {
    /// Checks that each order is named once.
    pub fn check(&self) -> Result<(), Violation> {
        let orders: alloc::vec::Vec<_> = self.held_for.iter().map(|h| &h.order).collect();
        if !distinct(&orders) {
            broke("a holding account reading names each order once")?;
        }
        Ok(())
    }
}

impl OrderInputs {
    /// Checks that the inputs name at least one input, as the schema does: an order with none states none.
    pub fn check(&self) -> Result<(), Violation> {
        if self.inputs.is_empty() {
            broke("an order's inputs name at least one input")?;
        }
        Ok(())
    }
}

impl MeasureRecord {
    /// Checks that the record cites what it rests on.
    pub fn check(&self) -> Result<(), Violation> {
        listed(
            &self.cites,
            "a measure record cites the entries it rests on, at least one, each once",
        )
    }
}

impl HandedOver {
    /// Checks a segment record's moves and a proxy use's operation.
    pub fn check(&self) -> Result<(), Violation> {
        match self {
            HandedOver::SegmentRecord {
                began_on,
                runs_on,
                moves,
                ..
            } => {
                let broken = "a segment record's moves lead from the version it began on to the one it runs on";
                let mut at = began_on;
                let mut led = true;
                for step in moves {
                    if &step.from != at || step.from == step.to {
                        led = false;
                    }
                    at = &step.to;
                }
                if !led || at != runs_on {
                    broke(broken)?;
                }
            }
            HandedOver::SecretUse { operation, of, .. } => {
                let for_a_secret = matches!(of, UseOf::Secret { .. });
                if for_a_secret != operation.is_some() {
                    broke("a proxy use names its operation exactly when it asked for a secret")?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Raise {
    /// Checks that a raise rests on cited entries and carries a real difference.
    pub fn check(&self) -> Result<(), Violation> {
        // A pair never built has no entry to cite where a restart dropped it, nor an account never read: those kinds alone
        // may cite none, each once.
        if matches!(
            self.difference,
            Difference::PairNotBuilt { .. } | Difference::HoldingNotRead { .. }
        ) {
            if !distinct(&self.cites) {
                broke("a raise cites the entries it rests on, each once")?;
            }
        } else {
            listed(
                &self.cites,
                "a raise cites the entries it rests on, at least one, each once",
            )?;
        }
        if let Difference::HoldingNotComparable {
            word,
            asset,
            reason,
            ..
        } = &self.difference
            && (word.is_some() != (*reason != HoldingIncomparable::NoReading)
                || asset.is_some() != (*reason != HoldingIncomparable::NoReading))
        {
            broke(
                "a holding that can't be compared names its balance's word and asset exactly when a balance was read",
            )?;
        }
        if let Difference::Charge {
            no_formula: Some(_),
            maximum,
            ..
        } = &self.difference
            && maximum.as_str() != "0"
        {
            broke("a charge marked as having no formula names a maximum of nothing")?;
        }
        let same = match &self.difference {
            Difference::Reports { sent, received, .. } => sent == received,
            Difference::ReportsNotComparable { sent, received, .. } => sent.asset == received.asset,
            Difference::HoldingNotComparable { .. } => false,
            Difference::SettlementTime {
                before_seconds,
                now_seconds,
                ..
            } => before_seconds == now_seconds,
            Difference::Charge {
                charged, maximum, ..
            } => charged <= maximum,
            Difference::Holding { .. }
            | Difference::PairNotBuilt { .. }
            | Difference::HoldingNotRead { .. } => false,
            Difference::PublicEnd {
                found,
                moved,
                stated,
                ..
            } => !found.differs(moved, stated),
        };
        if same {
            broke(
                "a raise's difference is a real one: two amounts or times that differ, a charge above its maximum, two reports in two assets or units, a public end's figure its mismatch names",
            )?;
        }
        Ok(())
    }
}

impl What {
    /// Whether `outcome` is one this call can have.
    pub fn allows(&self, outcome: &Outcome) -> bool {
        match self {
            What::Decide { .. }
            | What::Authorise { .. }
            | What::StuckDecide { .. }
            | What::StuckMove { .. }
            | What::ReportOutcome { .. }
            | What::ConfirmOutcome { .. }
            | What::DeclineOutcome { .. }
            | What::OpenSession { .. }
            | What::Charge { .. }
            | What::AskAboutPerson { .. }
            | What::CheckRead { .. }
            | What::GrantPermission { .. }
            | What::RevokePermission { .. } => matches!(
                outcome,
                Outcome::Permitted {} | Outcome::Refused { .. } | Outcome::NotKnown { .. }
            ),
            What::Effect { .. } => matches!(outcome, Outcome::ToTakeEffect {}),
            What::Result { .. } => matches!(
                outcome,
                Outcome::Done {} | Outcome::Failed {} | Outcome::Unanswered {}
            ),
            What::Raise { .. } | What::Command { .. } | What::Erasure { .. } => {
                matches!(outcome, Outcome::Done {} | Outcome::Failed {})
            }
            What::HandOver { .. }
            | What::ReportPair { .. }
            | What::HoldingCheck { .. }
            | What::Measure { .. }
            | What::Input { .. }
            | What::Acceptance { .. }
            | What::OrderRefusal { .. }
            | What::DestinationCheck { .. }
            | What::Arrival { .. }
            | What::DecisionRecord { .. }
            | What::Shown { .. }
            | What::Settlement { .. }
            | What::Evidence { .. }
            | What::Fee { .. }
            | What::Replay { .. }
            | What::StuckCall { .. }
            | What::Afterwards { .. }
            | What::Reliance { .. }
            | What::Refiling { .. }
            | What::HoldingWithdrawal { .. }
            | What::ReversalCarrier { .. }
            | What::InputNotTaken { .. }
            | What::NoticeSummary { .. }
            | What::PublicMismatch { .. }
            | What::PartEvidence { .. }
            | What::WaitEnded { .. }
            | What::OrderOpened { .. } => matches!(outcome, Outcome::Observed {}),
        }
    }

    /// Checks every shape it holds.
    pub fn check(&self) -> Result<(), Violation> {
        match self {
            What::Decide { request, .. } => request.check(),
            What::Effect { action, .. } => effect_action(action),
            What::StuckDecide { request, .. } => request.check(),
            What::OpenSession { request, .. } => request.check(),
            What::Charge { charge, .. } => charge.check(),
            What::AskAboutPerson { question } => question.check(),
            What::HandOver { item, .. } => item.check(),
            What::ReportPair { pair } => pair.check(),
            What::Measure { record } => record.check(),
            What::Raise { raise } => raise.check(),
            What::Input { input } => input
                .check()
                .or_else(|_| broke("a recorded input breaks the state-machine contract")),
            What::HoldingCheck { orders, .. } => {
                if distinct(orders) {
                    Ok(())
                } else {
                    broke("a holding check names each order once")
                }
            }
            What::GrantPermission { change, .. } | What::RevokePermission { change, .. } => {
                change.check()
            }
            What::Acceptance {
                basis,
                yeses,
                session,
                ..
            } => {
                let given = *basis == AcceptanceBasis::Given;
                // Given on a deposit (`REQ-QTE-10`, `T-020-39`): the customer's session, as given, and no yes, as an
                // automatic deposit: it rests on the deposit's arrival.
                if *basis == AcceptanceBasis::GivenOnDeposit {
                    if !yeses.is_empty() || session.is_none() {
                        broke(
                            "an acceptance given on a deposit names the session it was asked in and no reserve's yes",
                        )?;
                    }
                } else if given != !yeses.is_empty() || given != session.is_some() {
                    broke(
                        "an acceptance given names the reserve's yes and the session it was asked in, and an automatic deposit neither",
                    )?;
                }
                let parts: alloc::vec::Vec<_> = yeses.iter().map(|y| &y.part).collect();
                if !distinct(&parts) {
                    broke("an acceptance names each part's yes once")?;
                }
                // Each part rests on a yes of its own: two parts never cite one receipt.
                let receipts: alloc::vec::Vec<_> = yeses.iter().map(|y| &y.reserve_yes).collect();
                if !distinct(&receipts) {
                    broke("an acceptance cites a yes of its own for each part")?;
                }
                Ok(())
            }
            What::DecisionRecord {
                round, verifiers, ..
            } if !distinct(verifiers)
                || (*round == RoutingRound::Choice && !verifiers.is_empty()) =>
            {
                broke(
                    "a decision record names the verifiers its tuples need only on a shortlist's, each once",
                )
            }
            What::DecisionRecord {
                round, shortlisted, ..
            } => match (round, shortlisted) {
                // A shortlist may keep no provider: its record is what a `no_route` input cites (`REQ-AUD-4`).
                (RoutingRound::Shortlist, Some(providers)) if !distinct(providers) => broke(
                    "a shortlist's decision record names its providers, each once, and none where it kept none",
                ),
                (RoutingRound::Shortlist, Some(_)) => Ok(()),
                (RoutingRound::Choice, None) => Ok(()),
                _ => broke(
                    "a decision record names shortlisted providers exactly when it is a shortlist's",
                ),
            },
            What::Evidence { rests_on, .. } => listed(
                rests_on,
                "graded evidence rests on stated entries, at least one, each once",
            ),
            What::PartEvidence { rests_on, .. } => listed(
                rests_on,
                "a stuck part's evidence rests on stated entries, at least one, each once",
            ),
            What::PublicMismatch {
                node,
                movement,
                found,
                moved,
                stated,
                ..
            } => {
                if node.network != movement.network.as_str() {
                    broke("a public end's mismatch is at the network its movement is on")?;
                }
                if found.differs(moved, stated) {
                    Ok(())
                } else {
                    broke(
                        "a public end's mismatch names a figure the network and the provider differ on",
                    )
                }
            }
            What::Arrival {
                order: Some(_),
                notice: None,
                ..
            } => broke("an arrival naming an order names the notice of the deposit it is of"),
            What::Replay {
                nodes,
                found,
                rests_on,
                ..
            } => {
                listed(
                    nodes,
                    "a replay names the nodes it replayed, at least one, each once",
                )?;
                listed(
                    rests_on,
                    "a replay rests on its exit's result and the records it read, at least one, each once",
                )?;
                // Past the exit's result, one record at most for each node it replayed that recorded something: a
                // receipt beyond names no record for a node it replayed (`REQ-AUD-9`).
                let silent = match found {
                    ReplayFound::NothingRecorded { nodes: silent } => silent.len(),
                    _ => 0,
                };
                if rests_on.len() > 1 + nodes.len().saturating_sub(silent) {
                    broke(
                        "a replay names at most one record for each node it replayed that recorded something",
                    )?;
                }
                match found {
                    ReplayFound::Mismatch { mismatches } => {
                        listed(
                            mismatches,
                            "a mismatch names how the records disagree, at least one way, each once",
                        )?;
                        if mismatches.iter().all(|m| nodes.contains(&m.node)) {
                            Ok(())
                        } else {
                            broke("a replay's mismatch is against a node it replayed")
                        }
                    }
                    ReplayFound::NothingRecorded { nodes: silent } => {
                        listed(
                            silent,
                            "a replay names the nodes that recorded nothing, at least one, each once",
                        )?;
                        if silent.iter().all(|n| nodes.contains(n)) {
                            Ok(())
                        } else {
                            broke("a replay's node that recorded nothing is one it replayed")
                        }
                    }
                    ReplayFound::ConsistentWithTheExit {} => Ok(()),
                }
            }
            What::Authorise { authorisation, .. } => authorisation.check(),
            // A quote an order waits on its customer to accept is one shown without a firm rate, naming the slow
            // segments, each once (`REQ-QTE-10`).
            What::Shown {
                awaiting: Some(awaiting),
                shown_as,
                ..
            } => {
                // A quote asked of the customer once stays one the customer accepts when it is asked again, firm now
                // or not: a firm one names no slow segment, a slow one names at least one.
                let slow = !awaiting.slow.is_empty();
                if !matches!(
                    (shown_as, slow),
                    (ShownAs::QuoteWithoutFirmRate, true) | (ShownAs::Quote, false)
                ) {
                    broke(
                        "a quote an order waits on its customer to accept names its slow segments exactly when it is shown without a firm rate",
                    )?;
                }
                let places: alloc::vec::Vec<_> = awaiting.slow.iter().map(|s| s.position).collect();
                if !distinct(&places) {
                    broke("a quote awaiting its customer names its slow segments, each once")?;
                }
                Ok(())
            }
            What::Result { .. }
            | What::StuckMove { .. }
            | What::CheckRead { .. }
            | What::Refiling { .. }
            | What::OrderRefusal { .. }
            | What::DestinationCheck { .. }
            | What::Arrival { .. }
            | What::Shown { .. }
            | What::Settlement { .. }
            | What::Fee { .. }
            | What::StuckCall { .. }
            | What::Afterwards { .. }
            | What::Reliance { .. }
            | What::Command { .. }
            | What::Erasure { .. }
            | What::ReportOutcome { .. }
            | What::ConfirmOutcome { .. }
            | What::DeclineOutcome { .. }
            | What::NoticeSummary { .. }
            | What::HoldingWithdrawal { .. }
            | What::InputNotTaken { .. }
            | What::WaitEnded { .. }
            | What::OrderOpened { .. } => Ok(()),
            What::ReversalCarrier {
                carrier,
                showing,
                accepted_by,
                accepted_with,
                ..
            } => {
                let accepted =
                    showing.is_some() && accepted_by.is_some() && accepted_with.is_some();
                let shaped = match carrier {
                    Carrier::Provider => {
                        showing.is_none() && accepted_by.is_none() && accepted_with.is_none()
                    }
                    Carrier::Customer | Carrier::Operator => accepted,
                };
                if !shaped {
                    broke(
                        "a reversal's carrier other than its provider rests on a showing and an act of acceptance, and the provider on neither",
                    )?;
                }
                Ok(())
            }
        }
    }
}

impl VerifiedEntry {
    /// Checks the rules of a verified entry the schema can't state, and every shape it holds.
    pub fn check(&self) -> Result<(), Violation> {
        self.what.check()?;
        if !self.what.allows(&self.outcome) {
            broke("an entry's outcome is one its call can have")?;
        }
        if let What::Charge { counted_at, .. } = &self.what
            && counted_at.is_some() != (self.outcome == (Outcome::Permitted {}))
        {
            broke(
                "a charge says when its engine counted it exactly where it was counted, permitted within its bound",
            )?;
        }
        if let What::CheckRead { check } = &self.what
            && self.outcome == (Outcome::Permitted {})
            && matches!(check.reader.role, Role::Regulator | Role::Checker)
            && check.permission.is_none()
        {
            broke("a regulator's or a checker's permitted read names its permission")?;
        }
        if let What::CheckRead { check } = &self.what
            && self.outcome == (Outcome::Permitted {})
        {
            let broad = matches!(
                check.reads,
                Reads::Trail {}
                    | Reads::Order { .. }
                    | Reads::OrderState { .. }
                    | Reads::FiledOrder { .. }
            );
            if check.reader.role == Role::Provider && broad {
                broke(
                    "a provider's permitted read is of a segment, never an order, its state or the trail",
                )?;
            }
            // The operator reads an order's state and age without a permission (`REQ-ORD-5`), and its own
            // evidence; anything else it reads rests on a permission.
            if check.reader.role == Role::Operator
                && check.permission.is_none()
                && !matches!(
                    check.reads,
                    Reads::OrderState { .. } | Reads::FiledOrder { .. } | Reads::Evidence { .. }
                )
            {
                broke(
                    "an operator's permitted read without a permission is of an order's state, a filed order's or its own evidence",
                )?;
            }
            if check.reader.role == Role::Customer && matches!(check.reads, Reads::Trail {}) {
                broke("a customer's permitted read is of its own orders, never the trail")?;
            }
        }
        if let What::OpenSession { request, opened } = &self.what {
            let permitted = matches!(self.outcome, Outcome::Permitted {});
            if permitted != opened.is_some() {
                broke("a session entry names the session exactly when it was opened")?;
            }
            if let Some(opened) = opened {
                let ends = opened.expires_at.unix_nanos();
                if ends <= request.at.unix_nanos() || ends > request.until.unix_nanos() {
                    broke(
                        "an opened session ends after it was asked for and no later than its until",
                    )?;
                }
            }
        }
        Ok(())
    }
}

impl About {
    /// Checks that it names something.
    pub fn check(&self) -> Result<(), Violation> {
        if self.order.is_none()
            && self.segment.is_none()
            && self.transaction.is_none()
            && self.node.is_none()
        {
            broke("a stated entry's about names an order, a segment, a transaction or a node")?;
        }
        Ok(())
    }
}

impl StatedEntry {
    /// When the statement binds its maker from: the later of its `made_at` (`when.at`) and Core's `received_at`, never
    /// `made_at` alone, so a statement dated before an instant but received after it doesn't bind from before it. A
    /// provider's signed terms bind a segment only where they bind from no later than its start (`REQ-QTE-6`).
    pub fn binds_from(&self) -> &crate::router_api::Timestamp {
        if self.when.at.unix_nanos() >= self.received_at.unix_nanos() {
            &self.when.at
        } else {
            &self.received_at
        }
    }

    /// Checks that whoever relayed the statement is not its maker (`REQ-AUD-2`), and what it is about.
    pub fn check(&self) -> Result<(), Violation> {
        if let Some(about) = &self.about {
            about.check()?;
            // A node is what a publisher's record of the segments inside it is of: no other statement names one, and
            // such a record is about an order through its segments' transactions, never one of its own (`T-020-36`).
            if about.node.is_some()
                && (self.what.statement.as_str() != crate::provider_role::INNER_SEGMENTS_SHAPE
                    || about.segment.is_some()
                    || about.transaction.is_some())
            {
                broke(
                    "a stated entry names a node only as a publisher's record of the segments inside it, about no segment or transaction",
                )?;
            }
        }
        // The relay is another actor than the maker; a sibling under the same publisher may relay its statement.
        let maker = &self.attribution.maker().acts;
        if self
            .relayed_by
            .as_ref()
            .is_some_and(|relay| &relay.acts == maker)
        {
            broke("a stated entry's relay is a party other than its maker")?;
        }
        Ok(())
    }
}

impl Entry {
    /// Checks the entry by its own shape's rules, and every shape it holds.
    pub fn check(&self) -> Result<(), Violation> {
        match self {
            Entry::Stated(entry) => entry.check(),
            Entry::Verified(entry) => entry.check(),
        }
    }
}

macro_rules! checked_serde {
    ($($name:ident),*) => {$(
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                $name::serialize(self, serializer)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = $name::deserialize(deserializer)?;
                value.check().map_err(D::Error::custom)?;
                Ok(value)
            }
        }
    )*};
}

checked_serde!(
    Action,
    Authorisation,
    DecisionRequest,
    PermissionChange,
    StuckDecide,
    SessionRequest,
    Charge,
    PersonQuestion,
    ReportPair,
    HoldingReading,
    OrderInputs,
    MeasureRecord,
    HandedOver,
    Raise,
    VerifiedEntry,
    About,
    StatedEntry
);

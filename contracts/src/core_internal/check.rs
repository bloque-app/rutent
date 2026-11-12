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
        return Err(Violation(rule));
    }
    Ok(())
}

impl Action {
    /// Checks the rules of an action the schema can't state.
    pub fn check(&self) -> Result<(), Violation> {
        if let Action::ChangeCatalogue {
            change,
            version,
            tuple,
            advertising_keys,
            statement_key,
            ..
        } = self
        {
            if change.is_about_a_release() != version.is_some() {
                return Err(Violation(
                    "a catalogue change names a version exactly when it is about a module release",
                ));
            }
            if change.is_about_a_tuple() != tuple.is_some() {
                return Err(Violation(
                    "a catalogue change names a tuple exactly when it is about a tuple",
                ));
            }
            let grant = *change == CatalogueChange::Grant;
            if grant != !advertising_keys.is_empty() || !distinct(advertising_keys) {
                return Err(Violation(
                    "a catalogue change names advertising keys, at least one, each once, exactly when it is a grant",
                ));
            }
            if (*change == CatalogueChange::AcceptModule) != statement_key.is_some() {
                return Err(Violation(
                    "a catalogue change names a statement key exactly when it accepts a module release",
                ));
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
            return Err(Violation(
                "a decision request never asks to move a stuck order's value: only a stuck move does",
            ));
        }
        self.action.check()
    }
}

impl Effect {
    /// Checks the action it takes.
    pub fn check(&self) -> Result<(), Violation> {
        self.action.check()
    }
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

impl SessionRequest {
    /// Checks that the session it asks for ends after it is asked for.
    pub fn check(&self) -> Result<(), Violation> {
        if self.until.unix_nanos() <= self.at.unix_nanos() {
            return Err(Violation("a session ends after the time it is asked for"));
        }
        Ok(())
    }
}

impl Charge {
    /// Checks that the work is charged to the accountable party the caller acts for (`REQ-AUZ-8`).
    pub fn check(&self) -> Result<(), Violation> {
        if self.party != self.caller.party {
            return Err(Violation(
                "work is charged to the accountable party its caller acts for",
            ));
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
            return Err(Violation(
                "a question names each provider already asked about its order once",
            ));
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
                return Err(Violation(
                    "a shortlisted route names each verifier its tuples need once",
                ));
            }
            // The verifiers that receive the subject are asked too, and recorded and bounded with the providers
            // (`REQ-CPL-5`); anyone else is off the route.
            if !self
                .ask
                .iter()
                .all(|p| providers.contains(p) || verifiers.contains(p))
            {
                return Err(Violation(
                    "a question on a shortlisted route asks only the shortlisted providers and the verifiers its tuples need",
                ));
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
            return Err(Violation(
                "a read permission is a regulator's or a checker's",
            ));
        }
        if self.by.role != Role::Operator {
            return Err(Violation(
                "a read permission is granted or revoked by the operator",
            ));
        }
        Ok(())
    }
}

impl ReportPair {
    /// Checks that the pair's two reports are two parties' (`REQ-ORD-9`).
    pub fn check(&self) -> Result<(), Violation> {
        if self.sent.party == self.received.party {
            return Err(Violation(
                "a segment's two reports are by two different parties",
            ));
        }
        Ok(())
    }
}

impl HoldingReading {
    /// Checks that each order is named once.
    pub fn check(&self) -> Result<(), Violation> {
        let orders: alloc::vec::Vec<_> = self.held_for.iter().map(|h| &h.order).collect();
        if !distinct(&orders) {
            return Err(Violation("a holding account reading names each order once"));
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
                let broken = Violation(
                    "a segment record's moves lead from the version it began on to the one it runs on",
                );
                let mut at = began_on;
                for step in moves {
                    if &step.from != at || step.from == step.to {
                        return Err(broken);
                    }
                    at = &step.to;
                }
                if at != runs_on {
                    return Err(broken);
                }
            }
            HandedOver::SecretUse { operation, of, .. } => {
                let for_a_secret = matches!(of, UseOf::Secret { .. });
                if for_a_secret != operation.is_some() {
                    return Err(Violation(
                        "a proxy use names its operation exactly when it asked for a secret",
                    ));
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
        listed(
            &self.cites,
            "a raise cites the entries it rests on, at least one, each once",
        )?;
        let same = match &self.difference {
            Difference::Reports { sent, received, .. } => sent == received,
            Difference::SettlementTime {
                before_seconds,
                now_seconds,
                ..
            } => before_seconds == now_seconds,
            Difference::Charge {
                charged, maximum, ..
            } => charged <= maximum,
            Difference::Holding { .. } => false,
        };
        if same {
            return Err(Violation(
                "a raise's difference is a real one: two amounts or times that differ, a charge above its maximum",
            ));
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
            | What::StuckCall { .. }
            | What::Afterwards { .. }
            | What::Reliance { .. }
            | What::Refiling { .. } => matches!(outcome, Outcome::Observed {}),
        }
    }

    /// Checks every shape it holds.
    pub fn check(&self) -> Result<(), Violation> {
        match self {
            What::Decide { request, .. } => request.check(),
            What::Effect { action, .. } => action.check(),
            What::StuckDecide { request, .. } => request.check(),
            What::OpenSession { request, .. } => request.check(),
            What::Charge { charge } => charge.check(),
            What::AskAboutPerson { question } => question.check(),
            What::HandOver { item, .. } => item.check(),
            What::ReportPair { pair } => pair.check(),
            What::Measure { record } => record.check(),
            What::Raise { raise } => raise.check(),
            What::Input { input } => input
                .check()
                .map_err(|_| Violation("a recorded input breaks the state-machine contract")),
            What::HoldingCheck { orders, .. } => {
                if distinct(orders) {
                    Ok(())
                } else {
                    Err(Violation("a holding check names each order once"))
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
                if given != !yeses.is_empty() || given != session.is_some() {
                    return Err(Violation(
                        "an acceptance given names the reserve's yes and the session it was asked in, and an automatic deposit neither",
                    ));
                }
                let parts: alloc::vec::Vec<_> = yeses.iter().map(|y| &y.part).collect();
                if !distinct(&parts) {
                    return Err(Violation("an acceptance names each part's yes once"));
                }
                // Each part rests on a yes of its own: two parts never cite one receipt.
                let receipts: alloc::vec::Vec<_> = yeses.iter().map(|y| &y.reserve_yes).collect();
                if !distinct(&receipts) {
                    return Err(Violation(
                        "an acceptance cites a yes of its own for each part",
                    ));
                }
                Ok(())
            }
            What::DecisionRecord {
                round, verifiers, ..
            } if !distinct(verifiers)
                || (*round == RoutingRound::Choice && !verifiers.is_empty()) =>
            {
                Err(Violation(
                    "a decision record names the verifiers its tuples need only on a shortlist's, each once",
                ))
            }
            What::DecisionRecord {
                round, shortlisted, ..
            } => match (round, shortlisted) {
                // A shortlist may keep no provider: its record is what a `no_route` input cites (`REQ-AUD-4`).
                (RoutingRound::Shortlist, Some(providers)) if !distinct(providers) => {
                    Err(Violation(
                        "a shortlist's decision record names its providers, each once, and none where it kept none",
                    ))
                }
                (RoutingRound::Shortlist, Some(_)) => Ok(()),
                (RoutingRound::Choice, None) => Ok(()),
                _ => Err(Violation(
                    "a decision record names shortlisted providers exactly when it is a shortlist's",
                )),
            },
            What::Evidence { rests_on, .. } => listed(
                rests_on,
                "graded evidence rests on stated entries, at least one, each once",
            ),
            What::Authorise { .. }
            | What::Result { .. }
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
            | What::Erasure { .. } => Ok(()),
        }
    }
}

impl VerifiedEntry {
    /// Checks the rules of a verified entry the schema can't state, and every shape it holds.
    pub fn check(&self) -> Result<(), Violation> {
        self.what.check()?;
        if !self.what.allows(&self.outcome) {
            return Err(Violation("an entry's outcome is one its call can have"));
        }
        if matches!(self.what, What::Command { .. }) && self.who.role != Role::Operator {
            return Err(Violation("an operator command is the operator's"));
        }
        if let What::CheckRead { check } = &self.what
            && self.outcome == (Outcome::Permitted {})
            && matches!(check.reader.role, Role::Regulator | Role::Checker)
            && check.permission.is_none()
        {
            return Err(Violation(
                "a regulator's or a checker's permitted read names its permission",
            ));
        }
        if let What::CheckRead { check } = &self.what
            && self.outcome == (Outcome::Permitted {})
        {
            let broad = matches!(check.reads, Reads::Trail {} | Reads::Order { .. });
            if check.reader.role == Role::Provider && broad {
                return Err(Violation(
                    "a provider's permitted read is of a segment, never an order or the trail",
                ));
            }
            if check.reader.role == Role::Customer && matches!(check.reads, Reads::Trail {}) {
                return Err(Violation(
                    "a customer's permitted read is of its own orders, never the trail",
                ));
            }
        }
        if let What::OpenSession { request, opened } = &self.what {
            let permitted = matches!(self.outcome, Outcome::Permitted {});
            if permitted != opened.is_some() {
                return Err(Violation(
                    "a session entry names the session exactly when it was opened",
                ));
            }
            if let Some(opened) = opened {
                let ends = opened.expires_at.unix_nanos();
                if ends <= request.at.unix_nanos() || ends > request.until.unix_nanos() {
                    return Err(Violation(
                        "an opened session ends after it was asked for and no later than its until",
                    ));
                }
            }
        }
        Ok(())
    }
}

impl About {
    /// Checks that it names something.
    pub fn check(&self) -> Result<(), Violation> {
        if self.order.is_none() && self.segment.is_none() && self.transaction.is_none() {
            return Err(Violation(
                "a stated entry's about names an order, a segment or a transaction",
            ));
        }
        Ok(())
    }
}

impl StatedEntry {
    /// Checks that whoever relayed the statement is not its maker (`REQ-AUD-2`), and what it is about.
    pub fn check(&self) -> Result<(), Violation> {
        if let Some(about) = &self.about {
            about.check()?;
        }
        // The relay is another actor than the maker; a sibling under the same publisher may relay its statement.
        let maker = &self.attribution.maker().acts;
        if self
            .relayed_by
            .as_ref()
            .is_some_and(|relay| &relay.acts == maker)
        {
            return Err(Violation(
                "a stated entry's relay is a party other than its maker",
            ));
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
    DecisionRequest,
    PermissionChange,
    StuckDecide,
    SessionRequest,
    Charge,
    PersonQuestion,
    ReportPair,
    HoldingReading,
    MeasureRecord,
    HandedOver,
    Raise,
    VerifiedEntry,
    About,
    StatedEntry
);

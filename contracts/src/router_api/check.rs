//! The rules a field type can't carry, and the deserialisers that run them, so that a deserialised value always
//! serialises to a valid instance. Every `check` recurses through what it holds, so a value built in Rust is
//! checked whole by one call.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use super::atoms::{seconds_down, seconds_up};
use super::types::{
    Candidate, CatalogueSnapshot, CeilingScope, DecisionRecord, DropReason, Endpoint, Figure,
    FigureValue, Operation, OperationKind, Origin, Part, PolicyBounds, Pricing, Round,
};
use super::{Amount, Id, Timestamp, Violation};
use crate::provider_role::{
    ConversionOutcome, Denomination, Fee, JudgementAnswer, NodeRef, ReserveDirection,
    ReserveMovementOutcome, ResultBody, Statement, StatementShape, TransferOutcome, VoidedBy,
};

impl Operation {
    /// Checks the rules a field type can't carry.
    ///
    /// - A withdrawal goes from an outer node to an inner one, a deposit from an inner node to an outer one, and
    ///   both keep the asset.
    /// - A route keeps the asset between two different inner nodes.
    /// - An exchange stays at one inner node.
    /// - The fee is in the from asset and at most the amount in.
    /// - It cites each figure once.
    pub fn check(&self) -> Result<(), Violation> {
        let outer = |n: &NodeRef| matches!(n, NodeRef::Outer(_));
        let (from, to) = (&self.from.node, &self.to.node);
        let same_asset = self.from.asset.id == self.to.asset.id;
        match self.kind {
            OperationKind::Withdrawal => {
                if !outer(from) || outer(to) {
                    return Err(Violation(
                        "a withdrawal goes from an outer node to an inner one",
                    ));
                }
                if !same_asset {
                    return Err(Violation("a withdrawal keeps its asset"));
                }
            }
            OperationKind::Deposit => {
                if outer(from) || !outer(to) {
                    return Err(Violation(
                        "a deposit goes from an inner node to an outer one",
                    ));
                }
                if !same_asset {
                    return Err(Violation("a deposit keeps its asset"));
                }
            }
            OperationKind::Route => {
                if outer(from) || outer(to) {
                    return Err(Violation("a route runs between inner nodes"));
                }
                if from == to {
                    return Err(Violation("a route runs between two different nodes"));
                }
                if !same_asset {
                    return Err(Violation("a route keeps its asset"));
                }
            }
            OperationKind::Exchange => {
                if outer(from) || outer(to) {
                    return Err(Violation("an exchange runs at an inner node"));
                }
                if from != to {
                    return Err(Violation("an exchange stays at one node"));
                }
            }
        }
        if repeats(&self.figures) {
            return Err(Violation(
                "duplicate figure: an operation cites each figure once",
            ));
        }
        if self.fee.asset != self.from.asset.id {
            return Err(Violation("an operation's fee is in its from asset"));
        }
        match Amount::new(self.fee.amount.as_str()) {
            Some(fee) if &fee <= self.amount_in.amount() => {}
            _ => return Err(Violation("an operation's fee is at most its amount in")),
        }
        Ok(())
    }
}

impl Part {
    /// Checks every operation, and that each starts where the one before it ended, in the same unit, with what
    /// it delivered.
    pub fn check(&self) -> Result<(), Violation> {
        for operation in self.operations.iter() {
            operation.check()?;
        }
        for pair in self.operations.windows(2) {
            if pair[0].to != pair[1].from {
                return Err(Violation(
                    "each operation of a part starts where the one before it ended",
                ));
            }
            if &pair[0].amount_out != pair[1].amount_in.amount() {
                return Err(Violation(
                    "each operation of a part takes in what the one before it delivered",
                ));
            }
        }
        Ok(())
    }
}

impl Candidate {
    /// Checks every part, that a drop reason's indices name an operation of the candidate, and that a candidate
    /// isn't outranked by itself.
    pub fn check(&self) -> Result<(), Violation> {
        for part in self.parts.iter() {
            part.check()?;
        }
        match &self.dropped {
            Some(
                DropReason::OutsideBand { part, operation }
                | DropReason::Unconfirmed { part, operation },
            ) => {
                let found = self
                    .parts
                    .get(*part as usize)
                    .is_some_and(|p| (*operation as usize) < p.operations.len());
                if !found {
                    return Err(Violation(
                        "an outside-band or unconfirmed reason names an operation of its candidate",
                    ));
                }
            }
            Some(DropReason::Outranked { by }) if by == &self.candidate_id => {
                return Err(Violation("a candidate isn't outranked by itself"));
            }
            _ => {}
        }
        Ok(())
    }
}

impl PolicyBounds {
    /// Checks that there is at most one ceiling per provider and asset, and per publisher and asset, an asset
    /// being its id and decimals.
    pub fn check(&self) -> Result<(), Violation> {
        let providers: Vec<_> = self
            .provider_ceilings
            .iter()
            .map(|c| (&c.provider, &c.asset))
            .collect();
        let publishers: Vec<_> = self
            .publisher_ceilings
            .iter()
            .map(|c| (&c.publisher, &c.asset))
            .collect();
        if repeats(&providers) || repeats(&publishers) {
            return Err(Violation(
                "there is at most one ceiling per provider or publisher and asset",
            ));
        }
        Ok(())
    }

    /// Whether an operation uses a provider or tuple these bounds exclude.
    fn excludes(&self, operation: &Operation) -> bool {
        self.excluded_providers.contains(&operation.provider)
            || self.excluded_tuples.iter().any(|t| {
                t.provider == operation.provider && t.from == operation.from && t.to == operation.to
            })
    }

    fn has_ceiling(&self, scope: CeilingScope, party: &Id, asset: &Denomination) -> bool {
        match scope {
            CeilingScope::Provider => self
                .provider_ceilings
                .iter()
                .any(|c| &c.provider == party && &c.asset == asset),
            CeilingScope::Publisher => self
                .publisher_ceilings
                .iter()
                .any(|c| &c.publisher == party && &c.asset == asset),
        }
    }
}

fn repeats<T: PartialEq>(items: &[T]) -> bool {
    items
        .iter()
        .enumerate()
        .any(|(i, item)| items[..i].contains(item))
}

impl CatalogueSnapshot {
    /// Checks the rules a field type can't carry.
    ///
    /// - Figure ids are unique in the snapshot.
    /// - An exposure figure comes from the operator, every other figure from a provider; exposures are unique per
    ///   provider and asset.
    /// - A price, fresh quote, dry run or eligibility judgement names a tuple figure of the snapshot; all but the
    ///   judgement, a verifier's, are from the tuple's provider and publisher.
    /// - A price states a rate exactly when its tuple is an exchange's; levels' bounds strictly increase.
    /// - A statement's envelope names its shape, and its maker is its figure's provider.
    /// - A fresh quote is a transfer quote for a ramp's tuple or a conversion quote for an exchange's; its fee is
    ///   in the tuple's input asset and at most its amount in, and something voids it, each condition once.
    /// - A dry run is a dry-run answer or refusal for its tuple's kind of operation; an accepted one states the
    ///   amount the figure records.
    /// - Every provider has one publisher across the snapshot, its exposures included (`REQ-ORD-8`).
    /// - One tuple figure per provider and ends, and at most one price per tuple figure.
    /// - All judgements about one relying provider's tuples are about one subject: the subject reference is the
    ///   relying provider's, the one given to it (ADR-0012; ruled by the orchestrator, 2026-11-06). The relying
    ///   provider is the provider of the tuple a judgement names, not the verifier that made it.
    /// - Answers agree: no two dry-run answers (fresh quotes included) for one tuple and amount, one accepted and
    ///   one refused; no two judgements of one subject for one tuple, one yes and one no (ruled by the
    ///   orchestrator, 2026-11-06).
    /// - A statement figure's age runs from its `made_at` to `taken_at`, rounded up, and its validity from
    ///   `made_at` to its `valid_until`, rounded down (`REQ-QTE-4`).
    pub fn check(&self) -> Result<(), Violation> {
        for (i, figure) in self.figures.iter().enumerate() {
            if self.figures[..i]
                .iter()
                .any(|f| f.figure_id == figure.figure_id)
            {
                return Err(Violation("figure ids are unique in a snapshot"));
            }
        }
        let exposures: Vec<_> = self
            .figures
            .iter()
            .filter_map(|f| match &f.value {
                FigureValue::Exposure {
                    provider, asset, ..
                } => Some((provider, asset)),
                _ => None,
            })
            .collect();
        if repeats(&exposures) {
            return Err(Violation(
                "there is at most one exposure per provider and asset",
            ));
        }
        let publishers: Vec<(&Id, &Id)> = self
            .figures
            .iter()
            .filter_map(|f| match (&f.origin, &f.value) {
                (
                    _,
                    FigureValue::Exposure {
                        provider,
                        publisher,
                        ..
                    },
                ) => Some((provider, publisher)),
                (
                    Origin::Provider {
                        provider,
                        publisher,
                        ..
                    },
                    _,
                ) => Some((provider, publisher)),
                _ => None,
            })
            .collect();
        if publishers.iter().any(|(provider, publisher)| {
            publishers
                .iter()
                .any(|(p, q)| p == provider && q != publisher)
        }) {
            return Err(Violation(
                "every provider has one publisher across the snapshot",
            ));
        }
        let tuples: Vec<(Option<&Id>, (Endpoint, Endpoint))> = self
            .figures
            .iter()
            .filter_map(|f| tuple_ends(f).map(|ends| (f.origin.provider(), ends)))
            .collect();
        if repeats(&tuples) {
            return Err(Violation(
                "a snapshot holds one tuple figure per provider and ends",
            ));
        }
        let priced: Vec<&Id> = self
            .figures
            .iter()
            .filter_map(|f| match &f.value {
                FigureValue::Price { tuple, .. } => Some(tuple),
                _ => None,
            })
            .collect();
        if repeats(&priced) {
            return Err(Violation("a snapshot holds at most one price per tuple"));
        }
        // The subject reference belongs to the relying provider (ADR-0012): the provider of the tuple a
        // judgement names, not the verifier that made it.
        let judged: Vec<(Option<&Id>, &str)> = self
            .figures
            .iter()
            .filter_map(|f| match &f.value {
                FigureValue::Eligibility { tuple, judgement } => {
                    let relying = self
                        .figures
                        .iter()
                        .find(|t| &t.figure_id == tuple)
                        .and_then(|t| t.origin.provider());
                    Some((relying, judgement.statement.subject.as_str()))
                }
                _ => None,
            })
            .collect();
        if judged
            .iter()
            .any(|(p, subject)| judged.iter().any(|(q, other)| p == q && subject != other))
        {
            return Err(Violation(
                "all judgements about one relying provider's tuples are about one subject",
            ));
        }
        for figure in &self.figures {
            self.check_figure(figure)?;
        }
        self.check_answers_agree()
    }

    /// No two dry-run answers for one tuple and amount disagree, nor two judgements of one subject for one
    /// tuple.
    fn check_answers_agree(&self) -> Result<(), Violation> {
        let answers: Vec<(&Id, &str, bool)> = self
            .figures
            .iter()
            .filter_map(|f| match &f.value {
                FigureValue::FreshQuote { tuple, quote } => match dry_run_view(&quote.statement) {
                    Some((_, DryRun::Accepted { amount_in, .. })) => Some((tuple, amount_in, true)),
                    _ => None,
                },
                FigureValue::DryRun {
                    tuple,
                    amount_in,
                    result,
                } => match dry_run_view(&result.statement) {
                    Some((_, DryRun::Accepted { .. })) => {
                        Some((tuple, amount_in.amount().as_str(), true))
                    }
                    Some((_, DryRun::Refused)) => Some((tuple, amount_in.amount().as_str(), false)),
                    None => None,
                },
                _ => None,
            })
            .collect();
        if answers.iter().any(|(t, a, yes)| {
            answers
                .iter()
                .any(|(t2, a2, yes2)| t == t2 && a == a2 && yes != yes2)
        }) {
            return Err(Violation("dry-run answers for one tuple and amount agree"));
        }
        let judgements: Vec<(&Id, &str, bool)> = self
            .figures
            .iter()
            .filter_map(|f| match &f.value {
                FigureValue::Eligibility { tuple, judgement } => Some((
                    tuple,
                    judgement.statement.subject.as_str(),
                    judgement.statement.answer == JudgementAnswer::Yes,
                )),
                _ => None,
            })
            .collect();
        if judgements.iter().any(|(t, s, yes)| {
            judgements
                .iter()
                .any(|(t2, s2, yes2)| t == t2 && s == s2 && yes != yes2)
        }) {
            return Err(Violation(
                "eligibility judgements of one subject for one tuple agree",
            ));
        }
        Ok(())
    }

    fn check_figure(&self, figure: &Figure) -> Result<(), Violation> {
        let is_exposure = matches!(figure.value, FigureValue::Exposure { .. });
        if is_exposure != matches!(figure.origin, Origin::Operator { .. }) {
            return Err(Violation(
                "an exposure figure comes from the operator, every other figure from a provider",
            ));
        }
        match &figure.value {
            FigureValue::Price { tuple, rate, .. } => {
                if let Some(Pricing::Levels { levels }) = rate
                    && levels.windows(2).any(|w| w[0].up_to >= w[1].up_to)
                {
                    return Err(Violation("levels' bounds strictly increase"));
                }
                let (_, side) = self.tuple_of(tuple, figure, true)?;
                if rate.is_some() != (side == Side::Exchange) {
                    return Err(Violation(
                        "a price states a rate exactly when its tuple is an exchange's",
                    ));
                }
                Ok(())
            }
            FigureValue::FreshQuote { tuple, quote } => {
                let (target, side) = self.tuple_of(tuple, figure, true)?;
                self.check_envelope(figure, quote)?;
                let Some((kind, view)) = dry_run_view(&quote.statement) else {
                    return Err(Violation("a fresh quote is a provider's dry-run quote"));
                };
                let DryRun::Accepted {
                    fee,
                    amount_in,
                    voided_by,
                    valid_until,
                    ..
                } = view
                else {
                    return Err(Violation("a fresh quote is a provider's dry-run quote"));
                };
                if !matches!(kind, Side::Ramp | Side::Exchange) || kind != side {
                    return Err(Violation(
                        "a fresh quote is a transfer quote for a ramp's tuple or a conversion quote for an exchange's",
                    ));
                }
                check_fee(fee, amount_in, target)?;
                if voided_by.is_empty() {
                    return Err(Violation("something voids a quote"));
                }
                if repeats(voided_by) {
                    return Err(Violation("a quote names each void condition once"));
                }
                self.check_timing(figure, &quote.made_at, Some(valid_until))
            }
            FigureValue::DryRun {
                tuple,
                amount_in: asked,
                result,
            } => {
                let (_, side) = self.tuple_of(tuple, figure, true)?;
                self.check_envelope(figure, result)?;
                let Some((kind, view)) = dry_run_view(&result.statement) else {
                    return Err(Violation(
                        "a dry run is a provider's dry-run answer or refusal",
                    ));
                };
                if kind != side {
                    return Err(Violation(
                        "a dry run answers for its tuple's kind of operation",
                    ));
                }
                let valid_until = match view {
                    DryRun::Accepted {
                        valid_until,
                        amount_in,
                        ..
                    } => {
                        if amount_in != asked.amount().as_str() {
                            return Err(Violation(
                                "an accepted dry run states the amount its figure records",
                            ));
                        }
                        Some(valid_until)
                    }
                    DryRun::Refused => None,
                };
                self.check_timing(figure, &result.made_at, valid_until)
            }
            FigureValue::Eligibility { tuple, judgement } => {
                self.tuple_of(tuple, figure, false)?;
                self.check_envelope(figure, judgement)?;
                self.check_timing(
                    figure,
                    &judgement.made_at,
                    Some(&judgement.statement.valid_until),
                )
            }
            _ => Ok(()),
        }
    }

    /// The tuple figure a figure names, and which kind of operation it serves. With `same_provider`, the figure
    /// must be from the tuple's provider and publisher.
    fn tuple_of(
        &self,
        tuple: &Id,
        figure: &Figure,
        same_provider: bool,
    ) -> Result<(&Figure, Side), Violation> {
        let target = self
            .figures
            .iter()
            .find(|f| &f.figure_id == tuple)
            .ok_or(Violation("a figure names a tuple figure of its snapshot"))?;
        let side = match &target.value {
            FigureValue::ReserveTuple { tuple } => match tuple.direction() {
                Some(ReserveDirection::Withdrawal) => Side::Withdrawal,
                Some(ReserveDirection::Deposit) => Side::Deposit,
                None => return Err(Violation("a figure names a tuple figure of its snapshot")),
            },
            FigureValue::RampTuple { .. } => Side::Ramp,
            FigureValue::ExchangeTuple { .. } => Side::Exchange,
            _ => return Err(Violation("a figure names a tuple figure of its snapshot")),
        };
        if same_provider
            && (target.origin.provider() != figure.origin.provider()
                || target.origin.publisher() != figure.origin.publisher())
        {
            return Err(Violation(
                "a figure about a tuple is from its tuple's provider and publisher",
            ));
        }
        Ok((target, side))
    }

    /// A statement figure's envelope names its shape, and its maker is the figure's provider.
    fn check_envelope<T: StatementShape>(
        &self,
        figure: &Figure,
        statement: &Statement<T>,
    ) -> Result<(), Violation> {
        if statement.check_shape().is_err() {
            return Err(Violation("a statement's envelope names its own shape"));
        }
        if Some(statement.maker.as_str()) != figure.origin.provider().map(Id::as_str) {
            return Err(Violation("a statement's maker is its figure's provider"));
        }
        Ok(())
    }

    /// A statement figure's age runs from its `made_at` to `taken_at`, rounded up, and its validity from
    /// `made_at` to its `valid_until`, rounded down; a statement with no `valid_until` states no validity.
    fn check_timing(
        &self,
        figure: &Figure,
        made_at: &str,
        valid_until: Option<&str>,
    ) -> Result<(), Violation> {
        if Timestamp::new(made_at).is_none() {
            return Err(Violation("a statement's made_at is a timestamp"));
        }
        let age = seconds_up(made_at, self.taken_at.as_str());
        if age != Some(figure.age_seconds.get()) {
            return Err(Violation(
                "a statement figure's age runs from its making to the snapshot, rounded up",
            ));
        }
        let stated = figure.valid_for_seconds.map(|s| s.get());
        let validity = match valid_until {
            Some(until) => {
                let validity = seconds_down(made_at, until);
                validity.is_some() && validity == stated
            }
            None => stated.is_none(),
        };
        if !validity {
            return Err(Violation(
                "a statement figure's validity runs from its making to its valid_until, rounded down",
            ));
        }
        Ok(())
    }

    fn has_figure(&self, id: &Id) -> bool {
        self.figures.iter().any(|f| &f.figure_id == id)
    }
}

impl DecisionRecord {
    /// Checks the whole record: its snapshot, bounds and candidates, with their parts and operations, and the
    /// rules that tie them together.
    ///
    /// - Candidate ids are unique in the record.
    /// - A shortlist round chooses nothing. In a choice round, `chosen`, when present, names a candidate, and
    ///   that candidate alone has no drop reason; with no `chosen`, every candidate has one.
    /// - Every part of every candidate starts at the request's origin and ends at its destination, in their
    ///   units, and the parts' amounts in add up to the request's amount (`REQ-ORD-42`).
    /// - Every operation cites a tuple figure of its own provider, with its two ends in their units.
    /// - The chosen candidate uses no statement void at the bounds' deployment and protocol version, by the
    ///   provider-role `check_context` (`REQ-PRT-1`), and no figure past its validity (`REQ-QTE-4`).
    /// - The choice round chooses only a candidate whose every operation is confirmed: it cites an accepted
    ///   dry-run answer of its provider for its tuple, at its terms, within its validity and not void. That the
    ///   answer was made after the shortlist round can't be checked from the record. For a
    ///   route or exchange that is the fresh quote; for a withdrawal or deposit, the reserve's accepted dry run. A
    ///   candidate dropped as `unconfirmed` names an operation that isn't.
    /// - Every operation cites one tuple figure, and every other figure it cites names that tuple.
    /// - A kept candidate (the chosen one, or one on the shortlist) cites no void statement, no figure past its
    ///   validity, no refused dry run and no judgement of no; arrives at or above the floor and with something;
    ///   passes no ceiling; uses no excluded provider or tuple; keeps within the part and hop limits; and has every
    ///   operation within its tuple's bands and its price's levels.
    /// - A drop reason that names a figure names one its candidate cites, and the figure supports the drop: a
    ///   stale figure is past its validity, a void statement is void, a voided quote names the condition, a
    ///   refused dry run was refused, and an ineligible judgement answered no. `below_floor`,
    ///   `no_positive_outcome`, `over_ceiling`, `excluded_by_policy` and `outside_band` are recomputed from the
    ///   candidate's stated amounts and fees, the floor, the bounds and the exposures: they show the stated
    ///   amounts consistent with the drops, not with the prices cited, which `T-004-02` recomputes.
    ///   `outranked` and `suspect_rate_cycle` are the router's judgement: only that the outranking candidate
    ///   exists is checked, and recomputing the ranking is `T-004-02`'s.
    /// - A confirming answer equals its operation's amount in, fee and amount out.
    /// - Every figure an operation or a drop reason names is in the snapshot, every ceiling a drop reason names
    ///   is in the bounds, and an outranking candidate is in the record (`REQ-AUD-4`).
    pub fn check(&self) -> Result<(), Violation> {
        self.snapshot.check()?;
        self.bounds.check()?;
        for (i, candidate) in self.candidates.iter().enumerate() {
            candidate.check()?;
            if self.candidates[..i]
                .iter()
                .any(|c| c.candidate_id == candidate.candidate_id)
            {
                return Err(Violation("candidate ids are unique in a decision record"));
            }
        }
        match (&self.request.round, &self.chosen) {
            (Round::Shortlist {}, Some(_)) => {
                return Err(Violation("a shortlist round chooses nothing"));
            }
            (Round::Choice { .. }, chosen) => {
                if let Some(chosen) = chosen
                    && !self.candidates.iter().any(|c| &c.candidate_id == chosen)
                {
                    return Err(Violation("the chosen candidate is in the record"));
                }
                for candidate in &self.candidates {
                    let is_chosen = chosen.as_ref() == Some(&candidate.candidate_id);
                    if is_chosen == candidate.dropped.is_some() {
                        return Err(Violation(
                            "every candidate but the chosen one has a drop reason, and the chosen one has none",
                        ));
                    }
                }
            }
            (Round::Shortlist {}, None) => {}
        }
        for candidate in &self.candidates {
            self.check_candidate(candidate)?;
        }
        Ok(())
    }

    /// Whether a figure's statement is void at the bounds' deployment and protocol version (`REQ-PRT-1`), by
    /// the provider-role `check_context`; `None` for a figure that isn't a statement.
    fn is_void(&self, id: &Id) -> Option<bool> {
        let figure = self.snapshot.figures.iter().find(|f| &f.figure_id == id)?;
        let (deployment, version) = (
            &self.bounds.deployment,
            self.bounds.protocol_version.as_str(),
        );
        let context = match &figure.value {
            FigureValue::FreshQuote { quote, .. } => quote.check_context(deployment, version),
            FigureValue::DryRun { result, .. } => result.check_context(deployment, version),
            FigureValue::Eligibility { judgement, .. } => {
                judgement.check_context(deployment, version)
            }
            _ => return None,
        };
        Some(context.is_err())
    }

    fn figure(&self, id: &Id) -> Option<&Figure> {
        self.snapshot.figures.iter().find(|f| &f.figure_id == id)
    }

    /// The one tuple figure an operation cites. It must be its own provider's, with the operation's two ends in
    /// their units, and every other figure the operation cites must be about that tuple: its one price, a fresh
    /// quote, a dry run for the operation's amount in, or an eligibility judgement that names it.
    fn tuple_of(&self, operation: &Operation) -> Result<&Figure, Violation> {
        let cited: Vec<&Figure> = operation
            .figures
            .iter()
            .filter_map(|id| self.figure(id))
            .collect();
        let tuples: Vec<&Figure> = cited
            .iter()
            .copied()
            .filter(|f| tuple_ends(f).is_some())
            .collect();
        let [tuple] = tuples[..] else {
            return Err(Violation("every operation cites one tuple figure"));
        };
        if tuple.origin.provider() != Some(&operation.provider) {
            return Err(Violation(
                "every operation cites a tuple of its own provider",
            ));
        }
        if tuple_ends(tuple) != Some((operation.from.clone(), operation.to.clone())) {
            return Err(Violation(
                "every operation cites a tuple with its own two ends",
            ));
        }
        for figure in &cited {
            let names = match &figure.value {
                FigureValue::Price { tuple: t, .. }
                | FigureValue::FreshQuote { tuple: t, .. }
                | FigureValue::DryRun { tuple: t, .. }
                | FigureValue::Eligibility { tuple: t, .. } => Some(t),
                _ => None,
            };
            if !(figure.figure_id == tuple.figure_id || names == Some(&tuple.figure_id)) {
                return Err(Violation(
                    "every other figure an operation cites names the operation's own tuple",
                ));
            }
            if let FigureValue::DryRun { amount_in, .. } = &figure.value
                && amount_in != &operation.amount_in
            {
                return Err(Violation(
                    "every dry run an operation cites is for its amount in",
                ));
            }
        }
        if cited
            .iter()
            .filter(|f| matches!(f.value, FigureValue::Price { .. }))
            .count()
            != 1
        {
            return Err(Violation(
                "every operation cites one price, for its own tuple",
            ));
        }
        Ok(tuple)
    }

    /// Whether an operation is confirmed for the choice round: it cites an accepted dry-run answer of its own
    /// provider for its tuple, at its amount in, with its fee and its amount out, within its validity and not void.
    /// For a route or exchange that is the provider's fresh quote (`transfer_quote`, `conversion_quote`); for a
    /// withdrawal or deposit, the reserve's accepted dry run.
    fn is_confirmed(&self, operation: &Operation, tuple: &Id) -> bool {
        operation
            .figures
            .iter()
            .filter_map(|id| self.figure(id))
            .any(|f| {
                let result = match &f.value {
                    FigureValue::FreshQuote { tuple: t, quote } if t == tuple => quote,
                    FigureValue::DryRun {
                        tuple: t, result, ..
                    } if t == tuple => result,
                    _ => return false,
                };
                let same_terms = matches!(
                    dry_run_view(&result.statement),
                    Some((_, DryRun::Accepted { amount_in, amount_out, fee, .. }))
                        if amount_in == operation.amount_in.amount().as_str()
                            && amount_out == operation.amount_out.as_str()
                            && fee == &operation.fee
                );
                same_terms && f.is_within_validity() && self.is_void(&f.figure_id) == Some(false)
            })
    }

    fn check_candidate(&self, candidate: &Candidate) -> Result<(), Violation> {
        let request = &self.request;
        let choice = matches!(request.round, Round::Choice { .. });
        let mut total = super::Amount::zero();
        let mut arriving = super::Amount::zero();
        let mut unconfirmed = Vec::new();
        let mut outside_band = Vec::new();
        let mut placed = Vec::new();
        for (p, part) in candidate.parts.iter().enumerate() {
            let (first, last) = (part.operations.first(), part.operations.last());
            if first.from != request.from {
                return Err(Violation("every part starts at the request's origin"));
            }
            if last.to != request.to {
                return Err(Violation("every part ends at the request's destination"));
            }
            total = total
                .checked_add(first.amount_in.amount())
                .ok_or(Violation("a candidate's amounts add up within 78 digits"))?;
            arriving = arriving
                .checked_add(&last.amount_out)
                .ok_or(Violation("a candidate's amounts add up within 78 digits"))?;
            for (o, operation) in part.operations.iter().enumerate() {
                if !operation
                    .figures
                    .iter()
                    .all(|f| self.snapshot.has_figure(f))
                {
                    return Err(Violation(
                        "every figure an operation names is in the snapshot",
                    ));
                }
                let tuple = self.tuple_of(operation)?;
                if !self.is_confirmed(operation, &tuple.figure_id) {
                    unconfirmed.push((p, o));
                }
                if !self.within_band(operation, tuple) {
                    outside_band.push((p, o));
                }
                placed.push((operation, tuple.origin.publisher()));
            }
        }
        if &total != request.amount.amount() {
            return Err(Violation(
                "a candidate's parts add up to the request's amount",
            ));
        }
        let cited: Vec<&Figure> = candidate
            .parts
            .iter()
            .flat_map(|p| p.operations.iter())
            .flat_map(|o| o.figures.iter())
            .filter_map(|id| self.figure(id))
            .collect();
        let below_floor = request
            .floor
            .as_ref()
            .is_some_and(|floor| &arriving < floor);
        let over: Vec<(CeilingScope, &Id, &Denomination)> = self.ceilings_passed(&placed);
        let excluded = placed
            .iter()
            .any(|(operation, _)| self.bounds.excludes(operation));

        let Some(dropped) = &candidate.dropped else {
            // Kept: chosen, or on the shortlist. Nothing that would drop it may hold.
            if cited
                .iter()
                .any(|f| self.is_void(&f.figure_id) == Some(true))
            {
                return Err(Violation("a kept candidate uses no void statement"));
            }
            if cited.iter().any(|f| !f.is_within_validity()) {
                return Err(Violation(
                    "a kept candidate uses no figure past its validity",
                ));
            }
            if cited.iter().any(|f| refuses(f)) {
                return Err(Violation(
                    "a kept candidate cites no refused dry run and no judgement of no",
                ));
            }
            if below_floor {
                return Err(Violation("a kept candidate arrives at or above the floor"));
            }
            if arriving.is_zero() {
                return Err(Violation("a kept candidate arrives with something"));
            }
            if !over.is_empty() {
                return Err(Violation("a kept candidate passes no ceiling"));
            }
            if excluded {
                return Err(Violation(
                    "a kept candidate uses no excluded provider or tuple",
                ));
            }
            let hops = usize::from(self.bounds.max_hops.get());
            if candidate.parts.len() > usize::from(self.bounds.max_parts.get())
                || candidate.parts.iter().any(|p| p.operations.len() > hops)
            {
                return Err(Violation(
                    "a kept candidate keeps within the part and hop limits",
                ));
            }
            if !outside_band.is_empty() {
                return Err(Violation(
                    "a kept candidate's every operation is within its tuple's bands and levels",
                ));
            }
            if choice && !unconfirmed.is_empty() {
                return Err(Violation(
                    "the choice round chooses only a candidate whose every operation is confirmed",
                ));
            }
            return Ok(());
        };
        let cites = |id: &Id| cited.iter().any(|f| &f.figure_id == id);
        match dropped {
            DropReason::StaleFigure { figure }
            | DropReason::VoidStatement { figure }
            | DropReason::QuoteVoided { figure, .. }
            | DropReason::DryRunRefused { figure }
            | DropReason::Ineligible { figure } => {
                let Some(found) = self.figure(figure) else {
                    return Err(Violation(
                        "every figure a drop reason names is in the snapshot",
                    ));
                };
                if !cites(figure) {
                    return Err(Violation(
                        "a drop reason names a figure its candidate cites",
                    ));
                }
                if !self.supports(candidate, dropped, found) {
                    return Err(Violation("a drop reason's figure supports the drop"));
                }
                Ok(())
            }
            DropReason::Unconfirmed { part, operation } => {
                if !choice || !unconfirmed.contains(&(*part as usize, *operation as usize)) {
                    return Err(Violation(
                        "an unconfirmed reason names an operation the choice round finds unconfirmed",
                    ));
                }
                Ok(())
            }
            DropReason::OutsideBand { part, operation } => {
                if !outside_band.contains(&(*part as usize, *operation as usize)) {
                    return Err(Violation(
                        "an outside-band reason names an operation outside its tuple's bands or levels",
                    ));
                }
                Ok(())
            }
            DropReason::OverCeiling {
                scope,
                party,
                asset,
            } => {
                if !self.bounds.has_ceiling(*scope, party, asset) {
                    return Err(Violation(
                        "every ceiling a drop reason names is in the bounds",
                    ));
                }
                if !over.contains(&(*scope, party, asset)) {
                    return Err(Violation(
                        "an over-ceiling reason names a ceiling the candidate passes",
                    ));
                }
                Ok(())
            }
            DropReason::BelowFloor {} if !below_floor => Err(Violation(
                "a below-floor reason names a candidate that arrives below the floor",
            )),
            DropReason::NoPositiveOutcome {} if !arriving.is_zero() => Err(Violation(
                "a no-positive-outcome reason names a candidate that arrives with nothing",
            )),
            DropReason::ExcludedByPolicy {} if !excluded => Err(Violation(
                "an excluded-by-policy reason names a candidate that uses an excluded provider or tuple",
            )),
            DropReason::Outranked { by }
                if !self.candidates.iter().any(|c| &c.candidate_id == by) =>
            {
                Err(Violation("an outranking candidate is in the record"))
            }
            _ => Ok(()),
        }
    }

    /// Whether an operation's amount in falls within one of its tuple's bands, and, where it cites a price with
    /// levels, its net amount within the last level (`REQ-QTE-12`).
    fn within_band(&self, operation: &Operation, tuple: &Figure) -> bool {
        let terms = match &tuple.value {
            FigureValue::ReserveTuple { tuple } => &tuple.terms,
            FigureValue::RampTuple { tuple } => &tuple.terms,
            FigureValue::ExchangeTuple { tuple } => &tuple.terms,
            _ => return false,
        };
        let amount = operation.amount_in.amount();
        let in_band = terms.bands.iter().any(|band| {
            match (
                Amount::new(band.min.as_str()),
                Amount::new(band.max.as_str()),
            ) {
                (Some(min), Some(max)) => &min <= amount && amount <= &max,
                _ => false,
            }
        });
        let fee = Amount::new(operation.fee.amount.as_str()).unwrap_or_else(Amount::zero);
        let in_levels = operation
            .figures
            .iter()
            .filter_map(|id| self.figure(id))
            .all(|f| match &f.value {
                FigureValue::Price {
                    rate: Some(Pricing::Levels { levels }),
                    ..
                } => levels
                    .last()
                    .up_to
                    .amount()
                    .checked_add(&fee)
                    .is_some_and(|bound| amount <= &bound),
                _ => true,
            });
        in_band && in_levels
    }

    /// The ceilings a candidate passes: each where it puts value, in the ceiling's asset, at the provider or across
    /// the publisher's providers, its parts counted together, and that value plus the snapshot's exposure there is
    /// above the ceiling (`REQ-ORD-8`).
    fn ceilings_passed<'a>(
        &'a self,
        placed: &[(&Operation, Option<&Id>)],
    ) -> Vec<(CeilingScope, &'a Id, &'a Denomination)> {
        let sum = |pick: &dyn Fn(&Operation, Option<&Id>) -> bool| {
            placed
                .iter()
                .filter(|(o, publisher)| pick(o, *publisher))
                .try_fold(Amount::zero(), |acc, (o, _)| {
                    acc.checked_add(o.amount_in.amount())
                })
        };
        let exposure = |pick: &dyn Fn(&Id, &Id, &Denomination) -> bool| {
            self.snapshot
                .figures
                .iter()
                .filter_map(|f| match &f.value {
                    FigureValue::Exposure {
                        provider,
                        publisher,
                        asset,
                        in_flight,
                    } if pick(provider, publisher, asset) => Some(in_flight),
                    _ => None,
                })
                .try_fold(Amount::zero(), |acc, a| acc.checked_add(a))
        };
        let passes =
            |value: Option<Amount>, held: Option<Amount>, ceiling: &Amount| match (value, held) {
                (Some(value), Some(held)) => {
                    !value.is_zero()
                        && value
                            .checked_add(&held)
                            .is_none_or(|total| &total > ceiling)
                }
                _ => true,
            };
        let mut out = Vec::new();
        for c in &self.bounds.provider_ceilings {
            let value = sum(&|o, _| o.provider == c.provider && o.from.asset == c.asset);
            let held = exposure(&|provider, _, asset| provider == &c.provider && asset == &c.asset);
            if passes(value, held, &c.amount) {
                out.push((CeilingScope::Provider, &c.provider, &c.asset));
            }
        }
        for c in &self.bounds.publisher_ceilings {
            let value =
                sum(&|o, publisher| publisher == Some(&c.publisher) && o.from.asset == c.asset);
            let held =
                exposure(&|_, publisher, asset| publisher == &c.publisher && asset == &c.asset);
            if passes(value, held, &c.amount) {
                out.push((CeilingScope::Publisher, &c.publisher, &c.asset));
            }
        }
        out
    }

    /// Whether a figure the candidate cites supports the drop that names it.
    ///
    /// - `stale_figure`: the figure is past its validity.
    /// - `void_statement`: its statement is void at the bounds' deployment and protocol version.
    /// - `quote_voided`: it is a fresh quote that names the condition. For `validity_passes` the quote is past its
    ///   validity; for `amount_changes`, an operation citing it takes in another amount than the quote's. The
    ///   other conditions depend on facts outside the record, so only their being named is checked.
    /// - `dry_run_refused`: it is a dry run the provider refused.
    /// - `ineligible`: it is a judgement whose answer is no.
    fn supports(&self, candidate: &Candidate, dropped: &DropReason, figure: &Figure) -> bool {
        match (dropped, &figure.value) {
            (DropReason::StaleFigure { .. }, _) => !figure.is_within_validity(),
            (DropReason::VoidStatement { .. }, _) => self.is_void(&figure.figure_id) == Some(true),
            (DropReason::QuoteVoided { condition, .. }, FigureValue::FreshQuote { quote, .. }) => {
                let Some((
                    _,
                    DryRun::Accepted {
                        amount_in,
                        voided_by,
                        ..
                    },
                )) = dry_run_view(&quote.statement)
                else {
                    return false;
                };
                if !voided_by.contains(condition) {
                    return false;
                }
                match condition {
                    VoidedBy::ValidityPasses => !figure.is_within_validity(),
                    VoidedBy::AmountChanges => candidate
                        .parts
                        .iter()
                        .flat_map(|p| p.operations.iter())
                        .any(|o| {
                            o.figures.contains(&figure.figure_id)
                                && o.amount_in.amount().as_str() != amount_in
                        }),
                    _ => true,
                }
            }
            (DropReason::DryRunRefused { .. }, FigureValue::DryRun { .. })
            | (DropReason::Ineligible { .. }, FigureValue::Eligibility { .. }) => refuses(figure),
            _ => false,
        }
    }
}

/// Whether a figure refuses: a dry run the provider refused, or a judgement whose answer is no.
fn refuses(figure: &Figure) -> bool {
    match &figure.value {
        FigureValue::DryRun { result, .. } => {
            matches!(dry_run_view(&result.statement), Some((_, DryRun::Refused)))
        }
        FigureValue::Eligibility { judgement, .. } => {
            judgement.statement.answer == JudgementAnswer::No
        }
        _ => false,
    }
}

/// A tuple figure's two ends, as endpoints with their units; `None` for a figure that isn't a tuple.
fn tuple_ends(figure: &Figure) -> Option<(Endpoint, Endpoint)> {
    let denom = |id: &str, decimals| Denomination {
        id: id.to_owned(),
        decimals,
    };
    let inner = |node: &str| NodeRef::Inner(node.to_owned());
    let pair = |node_a, asset_a, node_b, asset_b| {
        (
            Endpoint {
                node: node_a,
                asset: asset_a,
            },
            Endpoint {
                node: node_b,
                asset: asset_b,
            },
        )
    };
    match &figure.value {
        FigureValue::ReserveTuple { tuple } => Some(pair(
            tuple.origin.clone(),
            denom(&tuple.asset, tuple.decimals.origin),
            tuple.destination.clone(),
            denom(&tuple.asset, tuple.decimals.destination),
        )),
        FigureValue::RampTuple { tuple } => Some(pair(
            inner(&tuple.origin),
            denom(&tuple.asset, tuple.decimals.origin),
            inner(&tuple.destination),
            denom(&tuple.asset, tuple.decimals.destination),
        )),
        FigureValue::ExchangeTuple { tuple } => Some(pair(
            inner(&tuple.node),
            tuple.from.clone(),
            inner(&tuple.node),
            tuple.to.clone(),
        )),
        _ => None,
    }
}

/// Which kind of operation a tuple serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Withdrawal,
    Deposit,
    Ramp,
    Exchange,
}

/// A provider's dry-run answer, as the router reads it.
enum DryRun<'a> {
    Accepted {
        fee: &'a Fee,
        amount_in: &'a str,
        amount_out: &'a str,
        voided_by: &'a [VoidedBy],
        valid_until: &'a str,
    },
    Refused,
}

/// A provider result's dry-run answer and the kind of operation it answers, or `None` for any other result.
fn dry_run_view(body: &ResultBody) -> Option<(Side, DryRun<'_>)> {
    let refused = |side| Some((side, DryRun::Refused));
    match body {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => {
            let side = if matches!(body, ResultBody::Withdrawal(_)) {
                Side::Withdrawal
            } else {
                Side::Deposit
            };
            match &r.outcome {
                ReserveMovementOutcome::DryRunAccepted(a) => Some((
                    side,
                    DryRun::Accepted {
                        fee: &a.fee,
                        amount_in: &a.amount_in,
                        amount_out: &a.amount_out,
                        voided_by: &a.voided_by,
                        valid_until: &a.valid_until,
                    },
                )),
                ReserveMovementOutcome::Refused(_) => refused(side),
                _ => None,
            }
        }
        ResultBody::Transfer(r) => match &r.outcome {
            TransferOutcome::DryRunAccepted(q) => Some((
                Side::Ramp,
                DryRun::Accepted {
                    fee: &q.fee,
                    amount_in: &q.amount_in,
                    amount_out: &q.amount_out,
                    voided_by: &q.voided_by,
                    valid_until: &q.valid_until,
                },
            )),
            TransferOutcome::Refused(_) => refused(Side::Ramp),
            _ => None,
        },
        ResultBody::Conversion(r) => match &r.outcome {
            ConversionOutcome::DryRunAccepted(q) => Some((
                Side::Exchange,
                DryRun::Accepted {
                    fee: &q.fee,
                    amount_in: &q.amount_in,
                    amount_out: &q.amount_out,
                    voided_by: &q.voided_by,
                    valid_until: &q.valid_until,
                },
            )),
            ConversionOutcome::Refused(_) => refused(Side::Exchange),
            _ => None,
        },
        _ => None,
    }
}

/// A quote's fee is in its tuple's input asset and at most its amount in.
fn check_fee(fee: &Fee, amount_in: &str, tuple: &Figure) -> Result<(), Violation> {
    let input = match &tuple.value {
        FigureValue::ReserveTuple { tuple } => tuple.asset.as_str(),
        FigureValue::RampTuple { tuple } => tuple.asset.as_str(),
        FigureValue::ExchangeTuple { tuple } => tuple.from.id.as_str(),
        _ => return Err(Violation("a figure names a tuple figure of its snapshot")),
    };
    if fee.asset != input {
        return Err(Violation("a quote's fee is in its tuple's input asset"));
    }
    match (Amount::new(fee.amount.as_str()), Amount::new(amount_in)) {
        (Some(fee), Some(amount_in)) if fee <= amount_in => Ok(()),
        _ => Err(Violation("a quote's fee is at most its amount in")),
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
    Operation,
    Part,
    Candidate,
    PolicyBounds,
    CatalogueSnapshot,
    DecisionRecord
);

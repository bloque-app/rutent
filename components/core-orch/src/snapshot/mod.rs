//! The snapshot Core hands the router: the catalogue read at Core's own clock, exposures from Core's records, the
//! `calls` figure, and each statement tied to its maker's key before a record is formed.
//!
//! Declared by `T-020-01`; `T-020-05` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! [`Snapshots::form`] forms one snapshot at one reading of Core's [`Clock`]: no caller passes a time, so a stale
//! one can't honour an expired grant (`REQ-CAT-2`). In order, it holds:
//!
//! 1. the catalogue's tuple figures at that reading ([`Catalogue::snapshot`]), with what stayed out and why;
//! 2. the prices Core holds for those tuples ([`Price`]), each with its provider's origin;
//! 3. each provider's `calls` figure, from the accepted release its tuples name ([`Releases`]);
//! 4. the exposures from Core's records ([`Exposures`]), every provider with value in flight, whether or not it has
//!    a tuple in this snapshot, so that a publisher's ceiling counts all of its providers (`REQ-ORD-8`);
//! 5. the statements Core received for this round, fresh quotes, dry runs and eligibility judgements, each kept
//!    only once it and every statement nested in it is tied to its maker's key ([`bind_statement`]), its figure is
//!    stated as the contract times it, and the snapshot with it still passes the contract and the policy bounds'
//!    check against it. Every other is in [`Formed::refused`], to record as its claimed maker's unverified word
//!    ([`record_refused`]).
//!
//! The result passes [`CatalogueSnapshot::check`] and [`PolicyBounds::check_against`] (router-api 3.0.0, as
//! `T-020-02` left it), the router's precondition. The route Core takes in is checked by [`check_route`].

mod clock;
mod exposure;
mod journal;
mod releases;
mod route;
mod statements;

pub use clock::{Clock, SystemClock};
pub use exposure::{Exposures, Held, HoldError};
pub use journal::OrchestrationLog;
pub use releases::{Release, ReleaseError, Releases};
pub use route::{RouteRefusal, Step, check_part, check_route, record_route_refusal};
pub use statements::{
    RecordError, Refusal, RefusedStatement, bind_operator_statement, bind_statement, record_refused,
};

use rutent_contracts::provider_role::{
    Category, Judgement, NodeRef, ResultBody, SafeInteger, Statement,
};
use rutent_contracts::provider_role::{ConversionOutcome, ReserveMovementOutcome, TransferOutcome};
use rutent_contracts::router_api::{
    CatalogueSnapshot, Endpoint, FeeFormula, Figure, FigureValue, Id, Origin, PolicyBounds,
    PositiveAmount, Pricing, Timestamp, TupleKey, Version, Violation,
};
use rutent_prov::catalogue::{Catalogue, Instant, LeftOut, SnapshotError};

/// The source an exposure figure names: Core's own record of value in flight.
pub const EXPOSURE_SOURCE: &str = "core-exposures";

/// A tuple's advertised price, as Core holds it: its provider's fee formula, its rate where it is an exchange's, and
/// its declared settlement time, with when the provider stated them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Price {
    /// The tuple, by its provider and its two ends.
    pub tuple: TupleKey,
    /// When the provider stated it: the figure's age counts from here.
    pub stated_at: Timestamp,
    /// The fee formula.
    pub fee: FeeFormula,
    /// The rate; present exactly when the tuple is an exchange's.
    pub rate: Option<Pricing>,
    /// The declared settlement time, in seconds from segment start.
    pub settlement_seconds: SafeInteger,
}

/// Why a price stayed out of the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PriceLeftOut {
    /// The snapshot holds no figure of its tuple: the tuple is out, or was never advertised.
    NoTuple,
    /// It is dated after the snapshot's `taken_at`.
    DatedAfterSnapshot,
    /// The figure would break a rule of the router-api contract or the bounds' check against the snapshot.
    Contract(Violation),
}

/// What a statement Core received was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// A ramp's or exchange's fresh quote: its signed dry-run transfer or conversion quote.
    FreshQuote,
    /// A dry run, at this amount in the tuple's input unit.
    DryRun {
        /// The amount asked about.
        amount_in: PositiveAmount,
    },
    /// A verifier's judgement of the customer for a class the tuple needs.
    Eligibility,
}

/// A statement Core received for a round, exactly as received, with the tuple it is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    /// The tuple it is about, by its provider and its two ends.
    pub tuple: TupleKey,
    /// What it answers.
    pub asked: Asked,
    /// Its bytes as received: what its signature covers.
    pub bytes: Vec<u8>,
}

/// One snapshot to form.
#[derive(Debug, Clone, Copy)]
pub struct Ask<'a> {
    /// The snapshot's id.
    pub snapshot_id: &'a str,
    /// The bounds the router will run under: the snapshot must pass their check against it.
    pub bounds: &'a PolicyBounds,
    /// The prices Core holds.
    pub prices: &'a [Price],
    /// The statements received for this round: none in a shortlist round.
    pub statements: &'a [Received],
}

/// A snapshot, the reading of Core's clock it was formed at, and what stayed out of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formed {
    /// What the router reads.
    pub snapshot: CatalogueSnapshot,
    /// The reading of Core's clock it was formed at: its `taken_at`.
    pub at: Instant,
    /// The catalogue's tuples that stayed out, with why.
    pub left_out: Vec<LeftOut>,
    /// The prices that stayed out, with why.
    pub prices_left_out: Vec<(TupleKey, PriceLeftOut)>,
    /// The statements kept out, to record as unverified.
    pub refused: Vec<RefusedStatement>,
}

/// Why no snapshot was formed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormError {
    /// The catalogue gave none.
    Catalogue(SnapshotError),
    /// Value is in flight at a provider whose publisher the catalogue doesn't know, so its exposure can't be
    /// stated, and leaving it out would let a route past a ceiling.
    ExposureWithoutPublisher(Id),
    /// A figure Core wrote can't be carried: a bug in Core, never silent.
    Contract(Violation),
    /// The snapshot fails the bounds' check against it.
    Bounds(Violation),
}

/// What Core forms snapshots from.
#[derive(Clone, Copy)]
pub struct Snapshots<'a> {
    /// The catalogue.
    pub catalogue: &'a Catalogue,
    /// Core's clock, read once per snapshot.
    pub clock: &'a dyn Clock,
    /// Core's record of value in flight.
    pub exposures: &'a Exposures,
    /// The releases accepted.
    pub releases: &'a Releases,
}

impl Snapshots<'_> {
    /// Forms one snapshot at one reading of Core's clock (see the module's docs).
    pub fn form(&self, ask: &Ask<'_>) -> Result<Formed, FormError> {
        let at = self.clock.now();
        let built = self
            .catalogue
            .snapshot(ask.snapshot_id, at)
            .map_err(FormError::Catalogue)?;
        let mut snapshot = built.snapshot;
        let tuples: Vec<(Id, TupleKey, Origin)> = snapshot
            .figures
            .iter()
            .filter_map(|f| {
                let (from, to) = ends(&f.value)?;
                let provider = f.origin.provider()?.clone();
                Some((
                    f.figure_id.clone(),
                    TupleKey { provider, from, to },
                    f.origin.clone(),
                ))
            })
            .collect();

        let mut prices_left_out = Vec::new();
        for price in ask.prices {
            match self.price_figure(price, &tuples, &snapshot) {
                Ok(figure) => {
                    if let Err(v) = try_add(&mut snapshot, figure, ask.bounds) {
                        prices_left_out.push((price.tuple.clone(), PriceLeftOut::Contract(v)));
                    }
                }
                Err(reason) => prices_left_out.push((price.tuple.clone(), reason)),
            }
        }

        let mut declaring: Vec<(&Id, &Version, &Origin)> = Vec::new();
        for (_, key, origin) in &tuples {
            if let Origin::Provider { module_version, .. } = origin
                && !declaring.iter().any(|(p, _, _)| *p == &key.provider)
            {
                declaring.push((&key.provider, module_version, origin));
            }
        }
        for (provider, version, origin) in declaring {
            let Some(release) = self.releases.release(provider, version) else {
                continue;
            };
            let Some(calls) = &release.calls else {
                continue;
            };
            let age = seconds_up(&release.made_at, &snapshot.taken_at).unwrap_or(0);
            let figure = Figure {
                figure_id: next_id(&snapshot),
                origin: origin.clone(),
                age_seconds: safe(age)?,
                valid_for_seconds: None,
                value: FigureValue::Calls {
                    calls: calls.clone(),
                },
            };
            snapshot.figures.push(figure);
        }

        let source = Id::new(EXPOSURE_SOURCE).expect("an identifier");
        for (provider, asset, in_flight) in self.exposures.totals() {
            let publisher = self
                .catalogue
                .publishers()
                .publisher_of_provider(provider.as_str())
                .and_then(Id::new)
                .ok_or_else(|| FormError::ExposureWithoutPublisher(provider.clone()))?;
            let figure = Figure {
                figure_id: next_id(&snapshot),
                origin: Origin::Operator {
                    source: source.clone(),
                },
                age_seconds: safe(0)?,
                valid_for_seconds: None,
                value: FigureValue::Exposure {
                    provider,
                    publisher,
                    asset,
                    in_flight,
                },
            };
            snapshot.figures.push(figure);
        }
        snapshot.check().map_err(FormError::Contract)?;
        ask.bounds
            .check_against(&snapshot)
            .map_err(FormError::Bounds)?;

        let mut refused = Vec::new();
        for received in ask.statements {
            let outcome = self
                .statement_figure(received, &tuples, &snapshot, at)
                .and_then(|figure| {
                    try_add(&mut snapshot, figure, ask.bounds)
                        .map_err(statements::Refusal::Contract)
                });
            if let Err(reason) = outcome {
                refused.push(RefusedStatement {
                    tuple: received.tuple.clone(),
                    bytes: received.bytes.clone(),
                    reason,
                });
            }
        }
        snapshot.check().map_err(FormError::Contract)?;
        ask.bounds
            .check_against(&snapshot)
            .map_err(FormError::Bounds)?;
        Ok(Formed {
            snapshot,
            at,
            left_out: built.left_out,
            prices_left_out,
            refused,
        })
    }

    fn price_figure(
        &self,
        price: &Price,
        tuples: &[(Id, TupleKey, Origin)],
        snapshot: &CatalogueSnapshot,
    ) -> Result<Figure, PriceLeftOut> {
        let (tuple, _, origin) = tuples
            .iter()
            .find(|(_, key, _)| key == &price.tuple)
            .ok_or(PriceLeftOut::NoTuple)?;
        let age = seconds_up(&price.stated_at, &snapshot.taken_at)
            .ok_or(PriceLeftOut::DatedAfterSnapshot)?;
        Ok(Figure {
            figure_id: next_id(snapshot),
            origin: origin.clone(),
            age_seconds: safe(age).map_err(|_| PriceLeftOut::DatedAfterSnapshot)?,
            valid_for_seconds: None,
            value: FigureValue::Price {
                tuple: tuple.clone(),
                fee: price.fee.clone(),
                rate: price.rate.clone(),
                settlement_seconds: price.settlement_seconds,
            },
        })
    }

    fn statement_figure(
        &self,
        received: &Received,
        tuples: &[(Id, TupleKey, Origin)],
        snapshot: &CatalogueSnapshot,
        at: Instant,
    ) -> Result<Figure, statements::Refusal> {
        use statements::Refusal;
        bind_statement(self.catalogue, &received.bytes, at)?;
        let (tuple, key, tuple_origin) = tuples
            .iter()
            .find(|(_, key, _)| key == &received.tuple)
            .ok_or(Refusal::NoTuple)?;
        let unreadable = |e: String| Refusal::Unreadable(e);
        let (made_at, valid_until, origin, value) = match &received.asked {
            Asked::FreshQuote | Asked::DryRun { .. } => {
                let result: Statement<ResultBody> = serde_json::from_slice(&received.bytes)
                    .map_err(|e| unreadable(e.to_string()))?;
                result
                    .check_shape()
                    .map_err(|e| unreadable(format!("{e:?}")))?;
                if result.maker != key.provider.as_str() {
                    return Err(Refusal::NotItsProvider);
                }
                let valid_until = valid_until_of(&result.statement);
                let made_at = result.made_at.clone();
                let value = match &received.asked {
                    Asked::DryRun { amount_in } => FigureValue::DryRun {
                        tuple: tuple.clone(),
                        amount_in: amount_in.clone(),
                        result: Box::new(result),
                    },
                    _ => FigureValue::FreshQuote {
                        tuple: tuple.clone(),
                        quote: Box::new(result),
                    },
                };
                (made_at, valid_until, tuple_origin.clone(), value)
            }
            Asked::Eligibility => {
                let judgement: Statement<Judgement> = serde_json::from_slice(&received.bytes)
                    .map_err(|e| unreadable(e.to_string()))?;
                judgement
                    .check_shape()
                    .map_err(|e| unreadable(format!("{e:?}")))?;
                self.catalogue
                    .category_usable(&judgement.maker, Category::Verifier, at)
                    .map_err(Refusal::MakerNotUsable)?;
                let origin = self.origin_of(&judgement.maker).ok_or(Refusal::NoOrigin)?;
                let valid_until = Some(judgement.statement.valid_until.clone());
                let made_at = judgement.made_at.clone();
                let value = FigureValue::Eligibility {
                    tuple: tuple.clone(),
                    judgement: Box::new(judgement),
                };
                (made_at, valid_until, origin, value)
            }
        };
        let made_at = Timestamp::new(made_at).ok_or(Refusal::Contract(Violation(
            "a statement's made_at is a timestamp",
        )))?;
        let age = seconds_up(&made_at, &snapshot.taken_at).ok_or(Refusal::DatedAfterSnapshot)?;
        let validity =
            match valid_until {
                Some(until) => {
                    let until = Timestamp::new(until).ok_or(Refusal::Contract(Violation(
                        "a statement's valid_until is a timestamp",
                    )))?;
                    let seconds = seconds_down(&made_at, &until).ok_or(Refusal::Contract(
                        Violation("a statement's valid_until is at or after its made_at"),
                    ))?;
                    Some(safe(seconds).map_err(|_| {
                        Refusal::Contract(Violation("a validity is a safe integer"))
                    })?)
                }
                None => None,
            };
        Ok(Figure {
            figure_id: next_id(snapshot),
            origin,
            age_seconds: safe(age)
                .map_err(|_| Refusal::Contract(Violation("an age is a safe integer")))?,
            valid_for_seconds: validity,
            value,
        })
    }

    /// The origin of a provider with no tuple in the snapshot, such as a verifier: its accepted advertisement's
    /// publisher and version, and its release accepted last.
    fn origin_of(&self, provider: &str) -> Option<Origin> {
        let ad = self.catalogue.accepted_advertisement(provider)?;
        let id = Id::new(provider)?;
        let release = self.releases.latest(&id)?;
        Some(Origin::Provider {
            provider: id,
            publisher: Id::new(ad.statement.publisher.as_str())?,
            provider_version: Version::new(ad.statement.version.as_str())?,
            module_version: release.version.clone(),
        })
    }
}

/// Adds `figure` if the snapshot with it still passes the contract and the bounds' check against it.
fn try_add(
    snapshot: &mut CatalogueSnapshot,
    figure: Figure,
    bounds: &PolicyBounds,
) -> Result<(), Violation> {
    snapshot.figures.push(figure);
    let verdict = snapshot
        .check()
        .and_then(|()| bounds.check_against(snapshot));
    if verdict.is_err() {
        snapshot.figures.pop();
    }
    verdict
}

/// A figure id no figure of the snapshot has yet.
fn next_id(snapshot: &CatalogueSnapshot) -> Id {
    let mut n = snapshot.figures.len() + 1;
    loop {
        let id = Id::new(format!("fig-{n:04}")).expect("an identifier");
        if !snapshot.figures.iter().any(|f| f.figure_id == id) {
            return id;
        }
        n += 1;
    }
}

fn safe(value: u64) -> Result<SafeInteger, FormError> {
    SafeInteger::try_from(value)
        .map_err(|_| FormError::Contract(Violation("an age or validity is a safe integer")))
}

/// The time from `earlier` to `later`, rounded up to whole seconds, as the contract counts an age; `None` if
/// `later` is earlier.
fn seconds_up(earlier: &Timestamp, later: &Timestamp) -> Option<u64> {
    let nanos = later.unix_nanos().checked_sub(earlier.unix_nanos())?;
    if nanos < 0 {
        return None;
    }
    u64::try_from((nanos + 999_999_999) / 1_000_000_000).ok()
}

/// The time from `earlier` to `later`, rounded down to whole seconds, as the contract counts a validity; `None`
/// if `later` is earlier.
fn seconds_down(earlier: &Timestamp, later: &Timestamp) -> Option<u64> {
    let nanos = later.unix_nanos().checked_sub(earlier.unix_nanos())?;
    if nanos < 0 {
        return None;
    }
    u64::try_from(nanos / 1_000_000_000).ok()
}

/// A dry-run answer's `valid_until`: an accepted one's, and none for a refusal.
fn valid_until_of(body: &ResultBody) -> Option<String> {
    match body {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => match &r.outcome {
            ReserveMovementOutcome::DryRunAccepted(a) => Some(a.valid_until.clone()),
            _ => None,
        },
        ResultBody::Transfer(r) => match &r.outcome {
            TransferOutcome::DryRunAccepted(q) => Some(q.valid_until.clone()),
            _ => None,
        },
        ResultBody::Conversion(r) => match &r.outcome {
            ConversionOutcome::DryRunAccepted(q) => Some(q.valid_until.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// A tuple figure's two ends, in their units.
fn ends(value: &FigureValue) -> Option<(Endpoint, Endpoint)> {
    use rutent_contracts::provider_role::Denomination;
    let unit = |id: &str, decimals: u8| Denomination {
        id: id.to_owned(),
        decimals,
    };
    match value {
        FigureValue::ReserveTuple { tuple } => Some((
            Endpoint {
                node: tuple.origin.clone(),
                asset: unit(&tuple.asset, tuple.decimals.origin),
            },
            Endpoint {
                node: tuple.destination.clone(),
                asset: unit(&tuple.asset, tuple.decimals.destination),
            },
        )),
        FigureValue::RampTuple { tuple } => Some((
            Endpoint {
                node: NodeRef::Inner(tuple.origin.clone()),
                asset: unit(&tuple.asset, tuple.decimals.origin),
            },
            Endpoint {
                node: NodeRef::Inner(tuple.destination.clone()),
                asset: unit(&tuple.asset, tuple.decimals.destination),
            },
        )),
        FigureValue::ExchangeTuple { tuple } => Some((
            Endpoint {
                node: NodeRef::Inner(tuple.node.clone()),
                asset: tuple.from.clone(),
            },
            Endpoint {
                node: NodeRef::Inner(tuple.node.clone()),
                asset: tuple.to.clone(),
            },
        )),
        _ => None,
    }
}

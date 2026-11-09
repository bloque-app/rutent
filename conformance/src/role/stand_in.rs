//! In-memory stand-ins, one per category (`T-002-03`). Each implements [`RoleProvider`] the way the provider-role
//! contract asks. Each also has builders that make it break exactly one role rule, so each of the suite's checks
//! can be shown to catch what it names. The ramp has a variant with internal steps (`REQ-CAT-21`, `AC-30`).
//!
//! Stand-ins sign nothing. Their signature values are well-formed placeholders, which no Ed25519 verifier accepts;
//! the suite's own tests verify with stand-in verifiers (see [`SignatureVerifier`](super::SignatureVerifier)).
//! Each sends its statements as `serde_json` writes them ([`Received::sent`]).

use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::provider_role::{
    ADVERTISEMENT_SHAPE, Advertisement, Availability, BalanceAnswer, BalanceOutcome,
    BalanceRefusal, BalanceRefused, BalanceVerificationResult, Band, BoundedAvailability,
    BoundedSlippage, Confirmed, ConversionOutcome, ConversionQuote, ConversionResult,
    ConversionSettlement, Declarations, Denomination, EndUnits, Evidence, EvidenceGrade,
    EvidencePiece, ExchangeOffer, ExchangeTuple, Failure, Fee, Inputs, JUDGEMENT_SHAPE, Judged,
    Judgement, JudgementAnswer, JudgementOutcome, JudgementResult, KeyPurpose, LiftsAt,
    MovementSettlement, NoDetail, NodeRef, Offers, Operation, OuterNode, PublicKey, RESULT_SHAPE,
    REVERSAL_CONFIRMATION_SHAPE, RampOffer, RampTuple, Refusal, Refused, Reporting,
    ReserveDeclaration, ReserveDryRunAnswer, ReserveMovementOutcome, ReserveMovementResult,
    ReserveOffer, ReserveTuple, ResultBody, Reversal, ReversalConfirmation,
    ReversalConfirmationOutcome, ReversalConfirmationResult, ReversalOutcome, ReversalResult,
    ReversalSettlement, SafeInteger, Signature, SignatureScheme, SigningKey, Slippage, Statement,
    Terms, Tracking, TransferOutcome, TransferQuote, TransferResult, Unbounded, Unknown,
    VerifierOffer, VoidedBy,
};

use super::{
    Received, RoleHarness, RoleProvider, SuiteContext, placeholder_key, placeholder_signature,
};

/// A stand-in's answers as the contract's types. Each stand-in sends them as `serde_json` writes them
/// ([`Received::sent`]), as an adapter built on the contract's types would.
trait Typed {
    fn advertisement(&self) -> Statement<Advertisement>;
    fn handle(&mut self, operation: &Statement<Operation>) -> Statement<ResultBody>;
    fn state(&self) -> String;
    fn status(&mut self, _transaction: &str) -> Option<Statement<ResultBody>> {
        None
    }
}

impl<T: Typed> RoleProvider for T {
    fn advertisement(&self) -> Received<Advertisement> {
        Received::sent(&Typed::advertisement(self))
    }

    fn handle(&mut self, operation: &Statement<Operation>) -> Received<ResultBody> {
        Received::sent(&Typed::handle(self, operation))
    }

    fn state(&self) -> String {
        Typed::state(self)
    }

    fn status(&mut self, transaction: &str) -> Option<Received<ResultBody>> {
        Typed::status(self, transaction).map(|s| Received::sent(&s))
    }
}

/// When stand-ins say they made a statement.
const MADE_AT: &str = "2026-11-06T15:04:05Z";
/// Until when stand-ins' quotes and judgements hold, and when a lifting refusal lifts.
const VALID_UNTIL: &str = "2026-11-13T15:04:05Z";
/// The publisher behind every stand-in.
const PUBLISHER: &str = "stand-in-publisher";

/// A role rule a stand-in breaks, set by one of its builders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Fault {
    MovesValueOutsideItsRole,
    DeclarationOfAnotherMaker,
    IgnoresTheSession,
    ConfirmsAnyCredit,
    ConfirmsWithoutTheSegment,
    IgnoresTheOperationEnvelope,
    ChecksTheEnvelopeOnlyOnDryRuns,
    SkipsTheEnvelopeOnConfirmations,
    PaysLessThroughItsStatus,
    RepeatsBeforeTheEnvelope,
    ComparesOnlyTheMajorVersion,
    ChecksTheNameAndItsOwnKey,
    FailsOnlyOnce,
    ChecksTheEnvelopeOnItsFirstClassOnly,
    FailsAfterMoving,
    RefusesToUndoFailures,
    DryRunMoves,
    SignsWithAnUnpublishedKey,
    QuotesAnotherAmount,
    AnswersDryRunsWithOutcomeUnknown,
    SucceedsWithoutMoving,
    RefusesWithALiftingTime,
    PaysLessThanQuoted,
    RefusesEverything,
    ServesOutsideItsTuplesAndBands,
    LeavesItsStatusUnknown,
    GivesWeakerEvidence,
    MovesAgainOnARepeat,
    ReversesWithoutUndoing,
    IgnoresMissingInputs,
    JudgesAnotherClass,
    DatesItsJudgementBadly,
    ChargesItsFeeInAnotherAsset,
    TakesItsFeeOnTopOfTheAmount,
}

/// Signs statements as one maker, with placeholder signatures: its advertisements with its advertising key, and
/// every other statement with its statements key (provider-role 2.0.0's key purposes).
#[derive(Debug, Clone)]
struct Signer {
    context: SuiteContext,
    maker: String,
    advertising_key: PublicKey,
    key: PublicKey,
}

impl Signer {
    fn new(context: &SuiteContext, maker: &str, seed: u8) -> Self {
        Self {
            context: context.clone(),
            maker: maker.to_owned(),
            advertising_key: placeholder_key(seed + 30),
            key: placeholder_key(seed),
        }
    }

    fn sign<T>(&self, shape: &str, statement: T) -> Statement<T> {
        Statement {
            deployment: self.context.deployment.clone(),
            protocol_version: self.context.protocol_version.clone(),
            shape: shape.to_owned(),
            maker: self.maker.clone(),
            made_at: MADE_AT.to_owned(),
            signature: Signature {
                scheme: SignatureScheme::Ed25519,
                key: if shape == ADVERTISEMENT_SHAPE {
                    self.advertising_key.clone()
                } else {
                    self.key.clone()
                },
                value: placeholder_signature(),
            },
            statement,
        }
    }

    fn keys(&self) -> Vec<SigningKey> {
        vec![
            SigningKey {
                scheme: SignatureScheme::Ed25519,
                public_key: self.advertising_key.clone(),
                purpose: KeyPurpose::Advertising,
            },
            SigningKey {
                scheme: SignatureScheme::Ed25519,
                public_key: self.key.clone(),
                purpose: KeyPurpose::Statements,
            },
        ]
    }

    fn result(&self, body: ResultBody) -> Statement<ResultBody> {
        self.sign(RESULT_SHAPE, body)
    }

    /// Whether an operation is the operator's statement for this deployment and protocol version, signed with the
    /// deployment's operator key (`REQ-PRT-1`).
    fn accepts(&self, operation: &Statement<Operation>) -> bool {
        operation
            .check_context(&self.context.deployment, &self.context.protocol_version)
            .is_ok()
            && operation.check_signed_by_operator().is_ok()
    }

    /// The final refusal of an operation from elsewhere.
    fn refuse_foreign(&self, operation: &Statement<Operation>) -> Statement<ResultBody> {
        self.result(refuse_outside_role(&operation.statement, final_refusal()))
    }
}

/// A segment a stand-in served, as a reversal must undo it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Served {
    node: String,
    asset: String,
    amount: String,
    /// Whether the segment settled, rather than failed.
    settled: bool,
}

impl Served {
    fn is_undone_by(&self, reversal: &Reversal) -> bool {
        self.node == reversal.node && self.asset == reversal.asset && self.amount == reversal.amount
    }
}

fn amount(text: &str) -> Option<u128> {
    text.parse().ok()
}

fn within(bands: &[Band], value: u128) -> bool {
    bands.iter().any(|band| {
        matches!((amount(&band.min), amount(&band.max)), (Some(min), Some(max)) if min <= value && value <= max)
    })
}

fn lacks_inputs(required: &[String], given: &Inputs) -> bool {
    required.iter().any(|name| !given.contains_key(name))
}

fn seconds(value: u64) -> SafeInteger {
    SafeInteger::try_from(value).expect("a safe integer")
}

fn final_refusal() -> Refused {
    Refused {
        refusal: Refusal::Final(NoDetail {}),
    }
}

fn waiting_on_the_person() -> Refused {
    Refused {
        refusal: Refusal::WaitsOnPerson(NoDetail {}),
    }
}

fn lifting_refusal() -> Refused {
    Refused {
        refusal: Refusal::LiftsAtKnownTime(LiftsAt {
            lifts_at: VALID_UNTIL.to_owned(),
        }),
    }
}

fn evidence(grade: EvidenceGrade, by: &str, transaction: &str) -> Vec<EvidencePiece> {
    vec![EvidencePiece {
        grade,
        reference: format!("{by}:{transaction}"),
        by: by.to_owned(),
    }]
}

fn terms(min: &str, max: &str, reporting: Reporting, inputs: &[&str]) -> Terms {
    Terms {
        bands: vec![Band {
            min: min.to_owned(),
            max: max.to_owned(),
        }],
        increment: None,
        reporting,
        required_inputs: inputs.iter().map(|s| (*s).to_owned()).collect(),
    }
}

/// A real value-moving operation's transaction: the one a repeat would name.
fn real_transaction(operation: &Operation) -> Option<&str> {
    match operation {
        Operation::Withdrawal(o) if !o.dry_run => Some(&o.transaction),
        Operation::Deposit(o) if !o.dry_run => Some(&o.transaction),
        Operation::Transfer(o) if !o.dry_run => Some(&o.transaction),
        Operation::Conversion(o) if !o.dry_run => Some(&o.transaction),
        _ => None,
    }
}

/// The answer a stand-in gives to an operation outside its role: a refusal, of the operation's kind. A balance
/// refusal is always final, since it can't say when it lifts.
fn refuse_outside_role(operation: &Operation, refused: Refused) -> ResultBody {
    match operation {
        Operation::Withdrawal(w) => ResultBody::Withdrawal(ReserveMovementResult {
            transaction: w.transaction.clone(),
            outcome: ReserveMovementOutcome::Refused(refused),
        }),
        Operation::Deposit(d) => ResultBody::Deposit(ReserveMovementResult {
            transaction: d.transaction.clone(),
            outcome: ReserveMovementOutcome::Refused(refused),
        }),
        Operation::Transfer(t) => ResultBody::Transfer(TransferResult {
            transaction: t.transaction.clone(),
            outcome: TransferOutcome::Refused(refused),
        }),
        Operation::Conversion(c) => ResultBody::Conversion(ConversionResult {
            transaction: c.transaction.clone(),
            outcome: ConversionOutcome::Refused(refused),
        }),
        Operation::BalanceVerification(b) => {
            ResultBody::BalanceVerification(BalanceVerificationResult {
                transaction: b.transaction.clone(),
                outcome: BalanceOutcome::Refused(BalanceRefused {
                    refusal: BalanceRefusal::Final(NoDetail {}),
                }),
            })
        }
        Operation::Judgement(_) => ResultBody::Judgement(JudgementResult {
            outcome: JudgementOutcome::Refused(refused),
        }),
        Operation::Reversal(r) => ResultBody::Reversal(ReversalResult {
            transaction: r.transaction.clone(),
            outcome: ReversalOutcome::Refused(refused),
        }),
        Operation::ReversalConfirmation(c) => {
            ResultBody::ReversalConfirmation(ReversalConfirmationResult {
                transaction: c.transaction.clone(),
                outcome: ReversalConfirmationOutcome::Refused(refused),
            })
        }
    }
}

// Reserve ------------------------------------------------------------------------------------------------

/// A reserve over a book, between a bank network and its ledger, in COP.
#[derive(Debug, Clone)]
pub struct Reserve {
    signer: Signer,
    advertisement: Statement<Advertisement>,
    session: String,
    account: String,
    balances: BTreeMap<String, u128>,
    movements: Vec<String>,
    credits: BTreeMap<String, Served>,
    results: BTreeMap<String, Statement<ResultBody>>,
    pending: BTreeMap<String, ReserveMovementOutcome>,
    unknown_next: bool,
    faults: BTreeSet<Fault>,
}

/// One reserve movement asked of the stand-in.
struct Movement<'a> {
    transaction: &'a str,
    origin: NodeRef,
    destination: NodeRef,
    asset: &'a str,
    stated: &'a str,
    dry_run: bool,
    inputs: &'a Inputs,
}

impl Reserve {
    /// The reserve's provider identifier.
    pub const PROVIDER: &'static str = "stand-in-reserve";

    /// A reserve that keeps its role. `context.account` holds funds, and `context.session` is its holder's
    /// session.
    pub fn new(context: &SuiteContext) -> Self {
        let signer = Signer::new(context, Self::PROVIDER, 1);
        let advertisement = Advertisement {
            provider: Self::PROVIDER.to_owned(),
            publisher: PUBLISHER.to_owned(),
            version: "0.1.0".to_owned(),
            keys: signer.keys(),
            offers: Offers {
                reserve: Some(ReserveOffer {
                    tuples: vec![
                        ReserveTuple {
                            origin: NodeRef::Outer(OuterNode {
                                network: "stand-in-bank".to_owned(),
                            }),
                            destination: NodeRef::Inner("stand-in-ledger".to_owned()),
                            asset: "COP".to_owned(),
                            decimals: EndUnits {
                                origin: 2,
                                destination: 2,
                            },
                            terms: terms(
                                "100",
                                "100000000",
                                Reporting::Net,
                                &["payer.bank_account"],
                            ),
                        },
                        ReserveTuple {
                            origin: NodeRef::Inner("stand-in-ledger".to_owned()),
                            destination: NodeRef::Outer(OuterNode {
                                network: "stand-in-bank".to_owned(),
                            }),
                            asset: "COP".to_owned(),
                            decimals: EndUnits {
                                origin: 2,
                                destination: 2,
                            },
                            terms: terms(
                                "100",
                                "100000000",
                                Reporting::Net,
                                &["beneficiary.bank_account"],
                            ),
                        },
                    ],
                    declaration: ReserveDeclaration {
                        can_require_user_initiated_withdrawal: true,
                        starts_routes_on_automatic_deposits: false,
                        maker: Self::PROVIDER.to_owned(),
                        declared_in_version: "0.1.0".to_owned(),
                    },
                }),
                ..Offers::default()
            },
            declarations: Declarations {
                tracking: vec![Tracking::Identifier, Tracking::StatusQuery],
                evidence: Some(Evidence {
                    grade: EvidenceGrade::SignedByProvider,
                    availability: Availability::Bounded(BoundedAvailability {
                        seconds: seconds(2_592_000),
                    }),
                }),
                safe_retry: Some(true),
            },
            private_nodes: vec!["stand-in-ledger".to_owned()],
        };
        let mut balances = BTreeMap::new();
        balances.insert(context.account.clone(), 50_000_000);
        Self {
            advertisement: signer.sign(ADVERTISEMENT_SHAPE, advertisement),
            signer,
            session: context.session.clone(),
            account: context.account.clone(),
            balances,
            movements: Vec::new(),
            credits: BTreeMap::new(),
            results: BTreeMap::new(),
            pending: BTreeMap::new(),
            unknown_next: false,
            faults: BTreeSet::new(),
        }
    }

    /// Breaks `REQ-CAT-9`: the reserve executes transfers, moving value outside its role.
    pub fn moving_value_outside_its_role(mut self) -> Self {
        self.faults.insert(Fault::MovesValueOutsideItsRole);
        self
    }

    /// Breaks `REQ-CAT-6`: the reserve's declaration names another maker than the advertisement's.
    pub fn with_a_declaration_of_another_maker(mut self) -> Self {
        let mut advertisement = self.advertisement.statement.clone();
        if let Some(offer) = advertisement.offers.reserve.as_mut() {
            offer.declaration.maker = "another-party".to_owned();
        }
        self.advertisement = self.signer.sign(ADVERTISEMENT_SHAPE, advertisement);
        self.faults.insert(Fault::DeclarationOfAnotherMaker);
        self
    }

    /// Breaks `REQ-CAT-3`: the reserve answers balance checks and withdrawal dry runs in any session.
    pub fn ignoring_the_session(mut self) -> Self {
        self.faults.insert(Fault::IgnoresTheSession);
        self
    }

    /// Breaks ADR-0008: the reserve confirms any credit it is asked about, arrived or not.
    pub fn confirming_any_credit(mut self) -> Self {
        self.faults.insert(Fault::ConfirmsAnyCredit);
        self
    }

    /// Breaks `REQ-PRT-1`: the reserve answers a reversal confirmation from another deployment, or signed with
    /// another key, as its own.
    pub fn skipping_the_envelope_on_confirmations(mut self) -> Self {
        self.faults.insert(Fault::SkipsTheEnvelopeOnConfirmations);
        self
    }

    /// Breaks ADR-0008: the reserve confirms a credit by its asset and amount, whatever segment it was for.
    pub fn confirming_without_the_segment(mut self) -> Self {
        self.faults.insert(Fault::ConfirmsWithoutTheSegment);
        self
    }

    fn breaks(&self, fault: Fault) -> bool {
        self.faults.contains(&fault)
    }

    fn inner_nodes(&self) -> Vec<String> {
        self.advertisement
            .statement
            .offers
            .reserve
            .iter()
            .flat_map(|offer| &offer.tuples)
            .flat_map(|t| [&t.origin, &t.destination])
            .filter_map(|end| match end {
                NodeRef::Inner(node) => Some(node.clone()),
                NodeRef::Outer(_) => None,
            })
            .collect()
    }

    fn terms_of(&self, origin: &NodeRef, destination: &NodeRef, asset: &str) -> Option<Terms> {
        self.advertisement
            .statement
            .offers
            .reserve
            .iter()
            .flat_map(|offer| &offer.tuples)
            .find(|t| &t.origin == origin && &t.destination == destination && t.asset == asset)
            .map(|t| t.terms.clone())
    }

    fn movement(&mut self, movement: Movement<'_>) -> ReserveMovementOutcome {
        let Movement {
            transaction,
            origin,
            destination,
            asset,
            stated,
            dry_run,
            inputs,
        } = movement;
        let (Some(value), Some(terms)) =
            (amount(stated), self.terms_of(&origin, &destination, asset))
        else {
            return ReserveMovementOutcome::Refused(final_refusal());
        };
        if !within(&terms.bands, value) {
            return ReserveMovementOutcome::Refused(final_refusal());
        }
        if lacks_inputs(&terms.required_inputs, inputs) {
            return ReserveMovementOutcome::Refused(waiting_on_the_person());
        }
        let fee = Fee {
            asset: asset.to_owned(),
            amount: "0".to_owned(),
        };
        if dry_run {
            return ReserveMovementOutcome::DryRunAccepted(ReserveDryRunAnswer {
                amount_in: stated.to_owned(),
                amount_out: stated.to_owned(),
                fee,
                valid_until: VALID_UNTIL.to_owned(),
                voided_by: vec![VoidedBy::ValidityPasses, VoidedBy::AmountChanges],
                settlement_seconds: seconds(86_400),
            });
        }
        // A withdrawal takes the value out of the holder's account; a deposit puts it in.
        let balance = self.balances.entry(self.account.clone()).or_insert(0);
        match origin {
            NodeRef::Outer(_) if *balance < value => {
                return ReserveMovementOutcome::Refused(waiting_on_the_person());
            }
            NodeRef::Outer(_) => *balance -= value,
            NodeRef::Inner(_) => *balance += value,
        }
        self.movements.push(transaction.to_owned());
        let settled = ReserveMovementOutcome::Succeeded(MovementSettlement {
            amount_in: stated.to_owned(),
            amount_out: stated.to_owned(),
            fee,
            evidence: evidence(EvidenceGrade::SignedByProvider, Self::PROVIDER, transaction),
        });
        if std::mem::take(&mut self.unknown_next) {
            // The movement happened, but the reserve can't tell yet; its status query tells later.
            self.pending.insert(transaction.to_owned(), settled);
            return ReserveMovementOutcome::Unknown(Unknown {});
        }
        settled
    }

    fn in_session(&self, session: Option<&str>) -> bool {
        self.breaks(Fault::IgnoresTheSession) || session == Some(self.session.as_str())
    }
}

impl Typed for Reserve {
    fn advertisement(&self) -> Statement<Advertisement> {
        self.advertisement.clone()
    }

    fn handle(&mut self, operation: &Statement<Operation>) -> Statement<ResultBody> {
        let skips = self.breaks(Fault::SkipsTheEnvelopeOnConfirmations)
            && matches!(operation.statement, Operation::ReversalConfirmation(_));
        if !skips && !self.signer.accepts(operation) {
            return self.signer.refuse_foreign(operation);
        }
        // A repeat of a real movement is answered as before and moves nothing (`REQ-CAT-10`).
        if let Some(transaction) = real_transaction(&operation.statement)
            && let Some(recorded) = self.results.get(transaction)
        {
            return recorded.clone();
        }
        let body = match &operation.statement {
            Operation::Withdrawal(w) => ResultBody::Withdrawal(ReserveMovementResult {
                transaction: w.transaction.clone(),
                // A dry run reveals sufficiency, so it is answered only in the holder's session (`REQ-CAT-4`).
                outcome: if w.dry_run && !self.in_session(w.session.as_deref()) {
                    ReserveMovementOutcome::Refused(final_refusal())
                } else {
                    self.movement(Movement {
                        transaction: &w.transaction,
                        origin: NodeRef::Outer(w.origin.clone()),
                        destination: NodeRef::Inner(w.destination.clone()),
                        asset: &w.asset,
                        stated: &w.amount,
                        dry_run: w.dry_run,
                        inputs: &w.inputs,
                    })
                },
            }),
            Operation::Deposit(d) => ResultBody::Deposit(ReserveMovementResult {
                transaction: d.transaction.clone(),
                outcome: self.movement(Movement {
                    transaction: &d.transaction,
                    origin: NodeRef::Inner(d.origin.clone()),
                    destination: NodeRef::Outer(d.destination.clone()),
                    asset: &d.asset,
                    stated: &d.amount,
                    dry_run: d.dry_run,
                    inputs: &d.inputs,
                }),
            }),
            Operation::BalanceVerification(b) => {
                let outcome = match amount(&b.amount) {
                    Some(stated) if self.in_session(Some(&b.session)) => {
                        BalanceOutcome::Answered(BalanceAnswer {
                            sufficient: self.balances.get(&b.account).copied().unwrap_or(0)
                                >= stated,
                        })
                    }
                    _ => BalanceOutcome::Refused(BalanceRefused {
                        refusal: BalanceRefusal::Final(NoDetail {}),
                    }),
                };
                ResultBody::BalanceVerification(BalanceVerificationResult {
                    transaction: b.transaction.clone(),
                    outcome,
                })
            }
            Operation::ReversalConfirmation(c) => {
                let matches =
                    |credit: &Served| credit.asset == c.asset && credit.amount == c.amount;
                let arrived = if self.breaks(Fault::ConfirmsWithoutTheSegment) {
                    self.credits.values().any(matches)
                } else {
                    self.credits.get(&c.segment).is_some_and(matches)
                };
                let outcome = if arrived || self.breaks(Fault::ConfirmsAnyCredit) {
                    ReversalConfirmationOutcome::Confirmed(Confirmed {
                        confirmation: Box::new(self.signer.sign(
                            REVERSAL_CONFIRMATION_SHAPE,
                            ReversalConfirmation {
                                segment: c.segment.clone(),
                                asset: c.asset.clone(),
                                amount: c.amount.clone(),
                            },
                        )),
                    })
                } else {
                    ReversalConfirmationOutcome::NotConfirmed(NoDetail {})
                };
                ResultBody::ReversalConfirmation(ReversalConfirmationResult {
                    transaction: c.transaction.clone(),
                    outcome,
                })
            }
            Operation::Transfer(t) if self.breaks(Fault::MovesValueOutsideItsRole) => {
                self.movements.push(t.transaction.clone());
                ResultBody::Transfer(TransferResult {
                    transaction: t.transaction.clone(),
                    outcome: TransferOutcome::Succeeded(MovementSettlement {
                        amount_in: t.amount.clone(),
                        amount_out: t.amount.clone(),
                        fee: Fee {
                            asset: t.asset.clone(),
                            amount: "0".to_owned(),
                        },
                        evidence: evidence(
                            EvidenceGrade::SignedByProvider,
                            Self::PROVIDER,
                            &t.transaction,
                        ),
                    }),
                })
            }
            other => refuse_outside_role(other, final_refusal()),
        };
        let result = self.signer.result(body);
        if let Some(transaction) = transaction_of(&result.statement) {
            self.results.insert(transaction, result.clone());
        }
        result
    }

    fn state(&self) -> String {
        format!(
            "{:?} {:?} {:?}",
            self.balances, self.movements, self.credits
        )
    }

    fn status(&mut self, transaction: &str) -> Option<Statement<ResultBody>> {
        let recorded = self.results.get(transaction)?.clone();
        let Some(settled) = self.pending.get(transaction).cloned() else {
            return Some(recorded);
        };
        let body = match recorded.statement {
            ResultBody::Withdrawal(r) => ResultBody::Withdrawal(ReserveMovementResult {
                transaction: r.transaction,
                outcome: settled,
            }),
            ResultBody::Deposit(r) => ResultBody::Deposit(ReserveMovementResult {
                transaction: r.transaction,
                outcome: settled,
            }),
            other => other,
        };
        Some(self.signer.result(body))
    }
}

impl RoleHarness for Reserve {
    fn force_outcome_unknown(&mut self) -> bool {
        self.unknown_next = true;
        true
    }

    fn credit_reversal(&mut self, segment: &str, node: &str, asset: &str, amount: &str) -> bool {
        if !self.inner_nodes().iter().any(|n| n == node) {
            return false;
        }
        self.credits.insert(
            segment.to_owned(),
            Served {
                node: node.to_owned(),
                asset: asset.to_owned(),
                amount: amount.to_owned(),
                settled: true,
            },
        );
        true
    }
}

// Exchange -----------------------------------------------------------------------------------------------

/// An exchange converting USDC to USDT at one public node, at a rate of one.
#[derive(Debug, Clone)]
pub struct Exchange {
    signer: Signer,
    rogue: Signer,
    advertisement: Statement<Advertisement>,
    conversions: Vec<String>,
    served: BTreeMap<String, Served>,
    unknown_next: bool,
    fail_next: bool,
    failed_once: bool,
    faults: BTreeSet<Fault>,
}

impl Exchange {
    /// The exchange's provider identifier.
    pub const PROVIDER: &'static str = "stand-in-exchange";

    /// An exchange that keeps its role.
    pub fn new(context: &SuiteContext) -> Self {
        let signer = Signer::new(context, Self::PROVIDER, 2);
        let advertisement = Advertisement {
            provider: Self::PROVIDER.to_owned(),
            publisher: PUBLISHER.to_owned(),
            version: "0.1.0".to_owned(),
            keys: signer.keys(),
            offers: Offers {
                exchange: Some(ExchangeOffer {
                    tuples: vec![ExchangeTuple {
                        node: "polygon".to_owned(),
                        from: Denomination {
                            id: "USDC".to_owned(),
                            decimals: 6,
                        },
                        to: Denomination {
                            id: "USDT".to_owned(),
                            decimals: 6,
                        },
                        terms: terms("1000000", "100000000000", Reporting::Gross, &[]),
                    }],
                }),
                ..Offers::default()
            },
            declarations: Declarations {
                tracking: vec![Tracking::Identifier],
                evidence: Some(Evidence {
                    grade: EvidenceGrade::PublicRecord,
                    availability: Availability::Unbounded(Unbounded::Unbounded),
                }),
                safe_retry: Some(false),
            },
            private_nodes: Vec::new(),
        };
        Self {
            advertisement: signer.sign(ADVERTISEMENT_SHAPE, advertisement),
            rogue: Signer::new(context, Self::PROVIDER, 12),
            signer,
            conversions: Vec::new(),
            served: BTreeMap::new(),
            unknown_next: false,
            fail_next: false,
            failed_once: false,
            faults: BTreeSet::new(),
        }
    }

    /// Breaks `REQ-CAT-14`: a dry run changes the exchange's state.
    pub fn with_a_dry_run_that_moves(mut self) -> Self {
        self.faults.insert(Fault::DryRunMoves);
        self
    }

    /// Breaks `REQ-CAT-2`: the exchange signs its results with a key it doesn't publish.
    pub fn signing_with_an_unpublished_key(mut self) -> Self {
        self.faults.insert(Fault::SignsWithAnUnpublishedKey);
        self
    }

    /// Breaks `REQ-ORD-1`: the exchange's quote takes in more than the amount stated.
    pub fn quoting_another_amount(mut self) -> Self {
        self.faults.insert(Fault::QuotesAnotherAmount);
        self
    }

    /// Breaks `REQ-QTE-5`: the exchange quotes and charges its fee in the asset it gives out, not the one it
    /// takes in, so the fee can't be counted against its formula. Quote and charge agree, so only the contract's
    /// amount rule sees it.
    pub fn charging_its_fee_in_another_asset(mut self) -> Self {
        self.faults.insert(Fault::ChargesItsFeeInAnotherAsset);
        self
    }

    /// Breaks `REQ-CAT-14`: the exchange answers a dry run with outcome unknown.
    pub fn answering_dry_runs_with_outcome_unknown(mut self) -> Self {
        self.faults.insert(Fault::AnswersDryRunsWithOutcomeUnknown);
        self
    }

    /// Breaks `REQ-ORD-1`: the exchange reports a conversion succeeded without making it.
    pub fn succeeding_without_moving(mut self) -> Self {
        self.faults.insert(Fault::SucceedsWithoutMoving);
        self
    }

    /// Breaks `REQ-CAT-18`: the exchange's refusals say they lift at a known time, even for what it never serves.
    pub fn refusing_with_a_lifting_time(mut self) -> Self {
        self.faults.insert(Fault::RefusesWithALiftingTime);
        self
    }

    /// Breaks `REQ-QTE-1`: the exchange gives two percent less than it quoted, beyond its bounded slippage.
    pub fn paying_less_than_it_quoted(mut self) -> Self {
        self.faults.insert(Fault::PaysLessThanQuoted);
        self
    }

    /// Breaks `REQ-ORD-1`: a conversion the exchange reports failed has moved value on.
    pub fn failing_after_moving(mut self) -> Self {
        self.faults.insert(Fault::FailsAfterMoving);
        self
    }

    /// Breaks `REQ-PRT-1`: the exchange serves operations replayed from another deployment or signed with another
    /// key.
    pub fn ignoring_the_operation_envelope(mut self) -> Self {
        self.faults.insert(Fault::IgnoresTheOperationEnvelope);
        self
    }

    /// Breaks the harness's word: after one failure, `force_failure` says yes but the next conversion succeeds.
    pub fn failing_only_once(mut self) -> Self {
        self.faults.insert(Fault::FailsOnlyOnce);
        self
    }

    /// Breaks no rule, and answers nothing: the exchange refuses everything, finally, and can't be driven to
    /// outcome unknown. It fails the suite on coverage alone.
    pub fn refusing_everything(mut self) -> Self {
        self.faults.insert(Fault::RefusesEverything);
        self
    }

    fn breaks(&self, fault: Fault) -> bool {
        self.faults.contains(&fault)
    }

    fn refusal(&self) -> Refused {
        if self.breaks(Fault::RefusesWithALiftingTime) {
            lifting_refusal()
        } else {
            final_refusal()
        }
    }

    fn advertises(&self, node: &str, from: &str, to: &str, value: u128) -> bool {
        self.advertisement
            .statement
            .offers
            .exchange
            .iter()
            .flat_map(|offer| &offer.tuples)
            .any(|t| {
                t.node == node
                    && t.from.id == from
                    && t.to.id == to
                    && within(&t.terms.bands, value)
            })
    }

    fn convert(&mut self, c: &rutent_contracts::provider_role::Conversion) -> ConversionOutcome {
        let fee = Fee {
            asset: if self.breaks(Fault::ChargesItsFeeInAnotherAsset) {
                c.to.clone()
            } else {
                c.from.clone()
            },
            amount: "0".to_owned(),
        };
        let Some(value) =
            amount(&c.amount).filter(|v| self.advertises(&c.node, &c.from, &c.to, *v))
        else {
            return ConversionOutcome::Refused(self.refusal());
        };
        if c.dry_run {
            if self.breaks(Fault::AnswersDryRunsWithOutcomeUnknown) {
                return ConversionOutcome::Unknown(Unknown {});
            }
            if self.breaks(Fault::DryRunMoves) {
                self.conversions.push(c.transaction.clone());
            }
            let amount_in = if self.breaks(Fault::QuotesAnotherAmount) {
                (value + 1).to_string()
            } else {
                c.amount.clone()
            };
            return ConversionOutcome::DryRunAccepted(ConversionQuote {
                amount_in,
                amount_out: c.amount.clone(),
                rate: "1".to_owned(),
                slippage: Slippage::Bounded(BoundedSlippage {
                    max_bps: 50.try_into().expect("basis points"),
                }),
                fee,
                direction: c.direction,
                valid_until: VALID_UNTIL.to_owned(),
                voided_by: vec![VoidedBy::ValidityPasses, VoidedBy::RateMovesBeyondSlippage],
                settlement_seconds: seconds(60),
            });
        }
        let failing = std::mem::take(&mut self.fail_next);
        let moves = if failing {
            self.breaks(Fault::FailsAfterMoving)
        } else {
            !self.breaks(Fault::SucceedsWithoutMoving)
        };
        if moves {
            self.conversions.push(c.transaction.clone());
        }
        self.served.insert(
            c.transaction.clone(),
            Served {
                node: c.node.clone(),
                asset: c.from.clone(),
                amount: c.amount.clone(),
                settled: !failing,
            },
        );
        if failing {
            return ConversionOutcome::Failed(Failure {
                evidence: evidence(EvidenceGrade::PublicRecord, "polygon", &c.transaction),
            });
        }
        if std::mem::take(&mut self.unknown_next) {
            return ConversionOutcome::Unknown(Unknown {});
        }
        let (amount_out, rate) = if self.breaks(Fault::PaysLessThanQuoted) {
            ((value * 98 / 100).to_string(), "0.98".to_owned())
        } else {
            (c.amount.clone(), "1".to_owned())
        };
        ConversionOutcome::Succeeded(ConversionSettlement {
            amount_in: c.amount.clone(),
            amount_out,
            rate,
            fee,
            evidence: evidence(EvidenceGrade::PublicRecord, "polygon", &c.transaction),
        })
    }

    /// A failed conversion is undone at once. A settled one only after the next settlement window, so its
    /// reversal is refused until a known time (`REQ-CAT-18`).
    fn reverse(&mut self, reversal: &Reversal) -> ReversalOutcome {
        match self.served.get(&reversal.reverses) {
            Some(served) if served.is_undone_by(reversal) && served.settled => {
                ReversalOutcome::Refused(lifting_refusal())
            }
            Some(served) if served.is_undone_by(reversal) => {
                self.conversions.retain(|t| t != &reversal.reverses);
                self.served.remove(&reversal.reverses);
                ReversalOutcome::Succeeded(ReversalSettlement {
                    node: reversal.node.clone(),
                    asset: reversal.asset.clone(),
                    amount: reversal.amount.clone(),
                    evidence: evidence(
                        EvidenceGrade::PublicRecord,
                        "polygon",
                        &reversal.transaction,
                    ),
                })
            }
            _ => ReversalOutcome::Refused(self.refusal()),
        }
    }
}

impl Typed for Exchange {
    fn advertisement(&self) -> Statement<Advertisement> {
        self.advertisement.clone()
    }

    fn handle(&mut self, operation: &Statement<Operation>) -> Statement<ResultBody> {
        if !self.breaks(Fault::IgnoresTheOperationEnvelope) && !self.signer.accepts(operation) {
            return self.signer.refuse_foreign(operation);
        }
        let body = match &operation.statement {
            other if self.breaks(Fault::RefusesEverything) => {
                refuse_outside_role(other, final_refusal())
            }
            Operation::Conversion(c) => ResultBody::Conversion(ConversionResult {
                transaction: c.transaction.clone(),
                outcome: self.convert(c),
            }),
            Operation::Reversal(r) => ResultBody::Reversal(ReversalResult {
                transaction: r.transaction.clone(),
                outcome: self.reverse(r),
            }),
            other => refuse_outside_role(other, self.refusal()),
        };
        if self.breaks(Fault::SignsWithAnUnpublishedKey) {
            self.rogue.result(body)
        } else {
            self.signer.result(body)
        }
    }

    fn state(&self) -> String {
        format!("{:?}", self.conversions)
    }
}

impl RoleHarness for Exchange {
    fn force_outcome_unknown(&mut self) -> bool {
        if self.breaks(Fault::RefusesEverything) {
            return false;
        }
        self.unknown_next = true;
        true
    }

    fn force_failure(&mut self) -> bool {
        if self.breaks(Fault::RefusesEverything) {
            return false;
        }
        if !(self.breaks(Fault::FailsOnlyOnce) && self.failed_once) {
            self.fail_next = true;
            self.failed_once = true;
        }
        true
    }
}

// Ramp ---------------------------------------------------------------------------------------------------

/// A ramp moving USDC between two public nodes.
#[derive(Debug, Clone)]
pub struct Ramp {
    signer: Signer,
    advertisement: Statement<Advertisement>,
    transfers: Vec<String>,
    served: BTreeMap<String, Served>,
    results: BTreeMap<String, Statement<ResultBody>>,
    pending: BTreeMap<String, TransferOutcome>,
    internal_steps: Vec<String>,
    unknown_next: bool,
    fail_next: bool,
    converts_internally: bool,
    faults: BTreeSet<Fault>,
}

impl Ramp {
    /// The ramp's provider identifier.
    pub const PROVIDER: &'static str = "stand-in-ramp";

    /// A ramp that keeps its role.
    pub fn new(context: &SuiteContext) -> Self {
        let signer = Signer::new(context, Self::PROVIDER, 3);
        let advertisement = Advertisement {
            provider: Self::PROVIDER.to_owned(),
            publisher: PUBLISHER.to_owned(),
            version: "0.1.0".to_owned(),
            keys: signer.keys(),
            offers: Offers {
                ramp: Some(RampOffer {
                    tuples: vec![RampTuple {
                        origin: "polygon".to_owned(),
                        destination: "bsc".to_owned(),
                        asset: "USDC".to_owned(),
                        decimals: EndUnits {
                            origin: 6,
                            destination: 6,
                        },
                        terms: terms(
                            "1000000",
                            "100000000000",
                            Reporting::Net,
                            &["beneficiary.address"],
                        ),
                    }],
                }),
                ..Offers::default()
            },
            declarations: Declarations {
                tracking: vec![Tracking::StatusQuery],
                evidence: Some(Evidence {
                    grade: EvidenceGrade::SignedByAnotherParty,
                    availability: Availability::Bounded(BoundedAvailability {
                        seconds: seconds(2_592_000),
                    }),
                }),
                safe_retry: Some(true),
            },
            private_nodes: Vec::new(),
        };
        Self {
            advertisement: signer.sign(ADVERTISEMENT_SHAPE, advertisement),
            signer,
            transfers: Vec::new(),
            served: BTreeMap::new(),
            results: BTreeMap::new(),
            pending: BTreeMap::new(),
            internal_steps: Vec::new(),
            unknown_next: false,
            fail_next: false,
            converts_internally: false,
            faults: BTreeSet::new(),
        }
    }

    /// Breaks `REQ-CAT-13`: the ramp serves amounts and nodes it doesn't advertise.
    pub fn serving_outside_its_tuples_and_bands(mut self) -> Self {
        self.faults.insert(Fault::ServesOutsideItsTuplesAndBands);
        self
    }

    /// Breaks `REQ-ORD-1`: the ramp's status query keeps answering outcome unknown.
    pub fn leaving_its_status_unknown(mut self) -> Self {
        self.faults.insert(Fault::LeavesItsStatusUnknown);
        self
    }

    /// Breaks `REQ-ORD-39`: the ramp's evidence is weaker than the grade it declares.
    pub fn with_weaker_evidence(mut self) -> Self {
        self.faults.insert(Fault::GivesWeakerEvidence);
        self
    }

    /// Breaks `REQ-CAT-10`: the ramp declares a repeat safe, then moves value again on one.
    pub fn moving_again_on_a_repeat(mut self) -> Self {
        self.faults.insert(Fault::MovesAgainOnARepeat);
        self
    }

    /// Breaks `REQ-CAT-15`: the ramp says it reversed a transfer without undoing it.
    pub fn reversing_without_undoing(mut self) -> Self {
        self.faults.insert(Fault::ReversesWithoutUndoing);
        self
    }

    /// Breaks `REQ-PRT-1`: the ramp checks an operation's envelope on dry runs only, so it executes and reverses
    /// for another deployment.
    pub fn checking_the_envelope_only_on_dry_runs(mut self) -> Self {
        self.faults.insert(Fault::ChecksTheEnvelopeOnlyOnDryRuns);
        self
    }

    /// Breaks `REQ-PRT-1`: the ramp answers a repeat from what it recorded before it checks the envelope, so a
    /// replay from elsewhere gets the recorded success.
    pub fn checking_its_repeats_before_the_envelope(mut self) -> Self {
        self.faults.insert(Fault::RepeatsBeforeTheEnvelope);
        self
    }

    /// Breaks `REQ-PRT-1`: the ramp compares only the protocol version's major number.
    pub fn comparing_only_the_major_version(mut self) -> Self {
        self.faults.insert(Fault::ComparesOnlyTheMajorVersion);
        self
    }

    /// Breaks `REQ-PRT-1`: the ramp checks the deployment's name and that the signing key is its own operator's,
    /// but not the operator key the envelope names.
    pub fn checking_the_name_and_its_own_operator_key(mut self) -> Self {
        self.faults.insert(Fault::ChecksTheNameAndItsOwnKey);
        self
    }

    /// Whether an operation's envelope is the operator's, as this ramp checks it.
    fn envelope_is_the_operators(&self, operation: &Statement<Operation>) -> bool {
        let context = &self.signer.context;
        if self.breaks(Fault::ComparesOnlyTheMajorVersion) {
            let major = |version: &str| version.split('.').next().map(str::to_owned);
            return operation.deployment == context.deployment
                && operation.check_signed_by_operator().is_ok()
                && major(&operation.protocol_version) == major(&context.protocol_version);
        }
        if self.breaks(Fault::ChecksTheNameAndItsOwnKey) {
            return operation.deployment.name == context.deployment.name
                && operation.protocol_version == context.protocol_version
                && operation.signature.key == context.deployment.operator_key;
        }
        self.signer.accepts(operation)
    }

    /// Breaks `THR-108`: the ramp takes its fee on top of the amount stated, so more leaves the origin than the
    /// `exact_in` operation asked. Its quote says so too, so only the contract's amount rule sees it.
    pub fn taking_its_fee_on_top_of_the_amount(mut self) -> Self {
        self.faults.insert(Fault::TakesItsFeeOnTopOfTheAmount);
        self
    }

    /// Breaks `REQ-QTE-1`: a transfer whose outcome arrives through the status query pays half what was quoted.
    pub fn paying_less_through_its_status(mut self) -> Self {
        self.faults.insert(Fault::PaysLessThroughItsStatus);
        self
    }

    /// Breaks `REQ-CAT-15`: the ramp refuses, finally, to undo a transfer that failed.
    pub fn refusing_to_undo_failures(mut self) -> Self {
        self.faults.insert(Fault::RefusesToUndoFailures);
        self
    }

    /// Breaks `REQ-CAT-12`: the ramp serves a transfer that lacks the inputs its tuple declares.
    pub fn ignoring_missing_inputs(mut self) -> Self {
        self.faults.insert(Fault::IgnoresMissingInputs);
        self
    }

    /// A ramp that takes internal steps: it converts USDC to an internal asset, moves that, and converts back
    /// (`REQ-CAT-21`). The contract sees only the segment.
    pub fn with_internal_steps(mut self) -> Self {
        self.converts_internally = true;
        self
    }

    /// The internal steps taken so far. They are a log of how the ramp served its segments, kept apart from its
    /// state: a reversal returns the value, not the history.
    pub fn internal_steps(&self) -> &[String] {
        &self.internal_steps
    }

    fn breaks(&self, fault: Fault) -> bool {
        self.faults.contains(&fault)
    }

    fn grade(&self) -> EvidenceGrade {
        if self.breaks(Fault::GivesWeakerEvidence) {
            EvidenceGrade::SignedByProvider
        } else {
            EvidenceGrade::SignedByAnotherParty
        }
    }

    fn transfer(&mut self, t: &rutent_contracts::provider_role::Transfer) -> TransferOutcome {
        let terms = self
            .advertisement
            .statement
            .offers
            .ramp
            .iter()
            .flat_map(|offer| &offer.tuples)
            .find(|tuple| {
                tuple.origin == t.origin
                    && tuple.destination == t.destination
                    && tuple.asset == t.asset
            })
            .map(|tuple| tuple.terms.clone());
        let advertised = amount(&t.amount).is_some_and(|value| {
            self.breaks(Fault::ServesOutsideItsTuplesAndBands)
                || terms
                    .as_ref()
                    .is_some_and(|terms| within(&terms.bands, value))
        });
        if !advertised {
            return TransferOutcome::Refused(final_refusal());
        }
        if !self.breaks(Fault::IgnoresMissingInputs)
            && terms
                .as_ref()
                .is_some_and(|terms| lacks_inputs(&terms.required_inputs, &t.inputs))
        {
            return TransferOutcome::Refused(waiting_on_the_person());
        }
        // The ramp charges nothing, unless it takes a fee of one unit on top of the amount stated.
        let on_top = self.breaks(Fault::TakesItsFeeOnTopOfTheAmount);
        let taken = if on_top {
            amount(&t.amount).map_or(t.amount.clone(), |v| (v + 1).to_string())
        } else {
            t.amount.clone()
        };
        let fee = Fee {
            asset: t.asset.clone(),
            amount: if on_top { "1" } else { "0" }.to_owned(),
        };
        if t.dry_run {
            return TransferOutcome::DryRunAccepted(TransferQuote {
                amount_in: taken.clone(),
                amount_out: t.amount.clone(),
                fee,
                direction: t.direction,
                valid_until: VALID_UNTIL.to_owned(),
                voided_by: vec![VoidedBy::ValidityPasses],
                settlement_seconds: seconds(600),
            });
        }
        if std::mem::take(&mut self.fail_next) {
            // The transfer failed before anything moved on; the ramp still owes its reversal.
            self.served.insert(
                t.transaction.clone(),
                Served {
                    node: t.origin.clone(),
                    asset: t.asset.clone(),
                    amount: t.amount.clone(),
                    settled: false,
                },
            );
            return TransferOutcome::Failed(Failure {
                evidence: evidence(self.grade(), &t.destination, &t.transaction),
            });
        }
        if self.converts_internally {
            for step in [
                "convert USDC to internal units",
                "move internal units",
                "convert internal units to USDC",
            ] {
                self.internal_steps
                    .push(format!("{}: {step}", t.transaction));
            }
        }
        self.transfers.push(t.transaction.clone());
        self.served.insert(
            t.transaction.clone(),
            Served {
                node: t.origin.clone(),
                asset: t.asset.clone(),
                amount: t.amount.clone(),
                settled: true,
            },
        );
        let settled = TransferOutcome::Succeeded(MovementSettlement {
            amount_in: taken,
            amount_out: t.amount.clone(),
            fee,
            evidence: evidence(self.grade(), &t.destination, &t.transaction),
        });
        if std::mem::take(&mut self.unknown_next) {
            let settled = match settled {
                TransferOutcome::Succeeded(mut s)
                    if self.breaks(Fault::PaysLessThroughItsStatus) =>
                {
                    s.amount_out = amount(&t.amount).map_or(s.amount_out, |v| (v / 2).to_string());
                    TransferOutcome::Succeeded(s)
                }
                other => other,
            };
            self.pending.insert(t.transaction.clone(), settled);
            return TransferOutcome::Unknown(Unknown {});
        }
        settled
    }

    fn reverse(&mut self, reversal: &Reversal) -> ReversalOutcome {
        match self.served.get(&reversal.reverses) {
            Some(served)
                if served.is_undone_by(reversal)
                    && !(self.breaks(Fault::RefusesToUndoFailures) && !served.settled) =>
            {
                if !self.breaks(Fault::ReversesWithoutUndoing) {
                    self.transfers.retain(|t| t != &reversal.reverses);
                }
                self.served.remove(&reversal.reverses);
                ReversalOutcome::Succeeded(ReversalSettlement {
                    node: reversal.node.clone(),
                    asset: reversal.asset.clone(),
                    amount: reversal.amount.clone(),
                    evidence: evidence(self.grade(), &reversal.node, &reversal.transaction),
                })
            }
            _ => ReversalOutcome::Refused(final_refusal()),
        }
    }
}

impl Typed for Ramp {
    fn advertisement(&self) -> Statement<Advertisement> {
        self.advertisement.clone()
    }

    fn handle(&mut self, operation: &Statement<Operation>) -> Statement<ResultBody> {
        // A repeat of a real transfer is answered as before and moves nothing (`REQ-CAT-10`), but only once the
        // envelope is the operator's.
        let repeat = if self.breaks(Fault::MovesAgainOnARepeat) {
            None
        } else {
            real_transaction(&operation.statement).and_then(|tx| self.results.get(tx).cloned())
        };
        if self.breaks(Fault::RepeatsBeforeTheEnvelope)
            && let Some(recorded) = repeat.clone()
        {
            return recorded;
        }
        let dry_run = matches!(&operation.statement, Operation::Transfer(t) if t.dry_run);
        let checks = !self.breaks(Fault::ChecksTheEnvelopeOnlyOnDryRuns) || dry_run;
        if checks && !self.envelope_is_the_operators(operation) {
            return self.signer.refuse_foreign(operation);
        }
        if let Some(recorded) = repeat {
            return recorded;
        }
        let body = match &operation.statement {
            Operation::Transfer(t) => ResultBody::Transfer(TransferResult {
                transaction: t.transaction.clone(),
                outcome: self.transfer(t),
            }),
            Operation::Reversal(r) => ResultBody::Reversal(ReversalResult {
                transaction: r.transaction.clone(),
                outcome: self.reverse(r),
            }),
            other => refuse_outside_role(other, final_refusal()),
        };
        let result = self.signer.result(body);
        if let Some(transaction) = transaction_of(&result.statement) {
            self.results.insert(transaction, result.clone());
        }
        result
    }

    fn state(&self) -> String {
        format!("{:?}", self.transfers)
    }

    fn status(&mut self, transaction: &str) -> Option<Statement<ResultBody>> {
        let recorded = self.results.get(transaction)?.clone();
        if self.breaks(Fault::LeavesItsStatusUnknown) {
            return Some(recorded);
        }
        match self.pending.get(transaction).cloned() {
            Some(settled) => Some(self.signer.result(ResultBody::Transfer(TransferResult {
                transaction: transaction.to_owned(),
                outcome: settled,
            }))),
            None => Some(recorded),
        }
    }
}

impl RoleHarness for Ramp {
    fn force_outcome_unknown(&mut self) -> bool {
        self.unknown_next = true;
        true
    }

    fn force_failure(&mut self) -> bool {
        self.fail_next = true;
        true
    }
}

// Verifier -----------------------------------------------------------------------------------------------

/// A verifier judging one class.
#[derive(Debug, Clone)]
pub struct Verifier {
    signer: Signer,
    advertisement: Statement<Advertisement>,
    faults: BTreeSet<Fault>,
}

impl Verifier {
    /// The verifier's provider identifier.
    pub const PROVIDER: &'static str = "stand-in-verifier";

    /// A verifier that keeps its role.
    pub fn new(context: &SuiteContext) -> Self {
        let signer = Signer::new(context, Self::PROVIDER, 4);
        let advertisement = Advertisement {
            provider: Self::PROVIDER.to_owned(),
            publisher: PUBLISHER.to_owned(),
            version: "0.1.0".to_owned(),
            keys: signer.keys(),
            offers: Offers {
                verifier: Some(VerifierOffer {
                    classes: vec!["kyc.natural_person".to_owned()],
                }),
                ..Offers::default()
            },
            declarations: Declarations {
                tracking: vec![Tracking::Identifier],
                evidence: None,
                safe_retry: None,
            },
            private_nodes: Vec::new(),
        };
        Self {
            advertisement: signer.sign(ADVERTISEMENT_SHAPE, advertisement),
            signer,
            faults: BTreeSet::new(),
        }
    }

    /// Breaks `REQ-CPL-1`: the verifier answers about a class other than the one asked.
    pub fn judging_another_class(mut self) -> Self {
        self.faults.insert(Fault::JudgesAnotherClass);
        self
    }

    /// Breaks `REQ-PRT-1`: the verifier judges a second class, and checks the envelope only for its first.
    pub fn checking_the_envelope_on_its_first_class_only(mut self) -> Self {
        let mut advertisement = self.advertisement.statement.clone();
        if let Some(offer) = advertisement.offers.verifier.as_mut() {
            offer.classes.push("kyc.legal_entity".to_owned());
        }
        self.advertisement = self.signer.sign(ADVERTISEMENT_SHAPE, advertisement);
        self.faults
            .insert(Fault::ChecksTheEnvelopeOnItsFirstClassOnly);
        self
    }

    /// Breaks `REQ-PRT-1`: the verifier's nested judgement states its time made in a malformed way.
    pub fn dating_its_judgement_badly(mut self) -> Self {
        self.faults.insert(Fault::DatesItsJudgementBadly);
        self
    }

    fn breaks(&self, fault: Fault) -> bool {
        self.faults.contains(&fault)
    }
}

impl Typed for Verifier {
    fn advertisement(&self) -> Statement<Advertisement> {
        self.advertisement.clone()
    }

    fn handle(&mut self, operation: &Statement<Operation>) -> Statement<ResultBody> {
        let first_class = self
            .advertisement
            .statement
            .offers
            .verifier
            .as_ref()
            .and_then(|offer| offer.classes.first());
        let skips = self.breaks(Fault::ChecksTheEnvelopeOnItsFirstClassOnly)
            && matches!(&operation.statement, Operation::Judgement(q) if Some(&q.class) != first_class);
        if !skips && !self.signer.accepts(operation) {
            return self.signer.refuse_foreign(operation);
        }
        let body = match &operation.statement {
            Operation::Judgement(question) => {
                let advertised = self
                    .advertisement
                    .statement
                    .offers
                    .verifier
                    .as_ref()
                    .is_some_and(|offer| offer.classes.contains(&question.class));
                let outcome = if advertised {
                    let class = if self.breaks(Fault::JudgesAnotherClass) {
                        "kyc.another_class".to_owned()
                    } else {
                        question.class.clone()
                    };
                    let mut judgement = self.signer.sign(
                        JUDGEMENT_SHAPE,
                        Judgement {
                            subject: question.subject.clone(),
                            class,
                            answer: JudgementAnswer::Yes,
                            valid_until: VALID_UNTIL.to_owned(),
                            holder_key: None,
                        },
                    );
                    if self.breaks(Fault::DatesItsJudgementBadly) {
                        judgement.made_at = "2026-11-06 15:04".to_owned();
                    }
                    JudgementOutcome::Judged(Judged {
                        judgement: Box::new(judgement),
                    })
                } else {
                    JudgementOutcome::Refused(final_refusal())
                };
                ResultBody::Judgement(JudgementResult { outcome })
            }
            other => refuse_outside_role(other, final_refusal()),
        };
        self.signer.result(body)
    }

    fn state(&self) -> String {
        String::new()
    }
}

impl RoleHarness for Verifier {}

/// The transaction a result answers, if its kind names one.
fn transaction_of(body: &ResultBody) -> Option<String> {
    match body {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => Some(r.transaction.clone()),
        ResultBody::Transfer(r) => Some(r.transaction.clone()),
        ResultBody::Conversion(r) => Some(r.transaction.clone()),
        ResultBody::BalanceVerification(r) => Some(r.transaction.clone()),
        ResultBody::Judgement(_) => None,
        ResultBody::Reversal(r) => Some(r.transaction.clone()),
        ResultBody::ReversalConfirmation(r) => Some(r.transaction.clone()),
    }
}

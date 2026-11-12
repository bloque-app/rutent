//! The suite's probes and checks.

use std::collections::BTreeSet;

use rutent_contracts::provider_role::{
    Advertisement, BalanceOutcome, BalanceRefusal, BalanceVerification, Category, Conversion,
    ConversionOutcome, Deposit, Direction, EvidenceGrade, EvidencePiece, Fee,
    HoldingBalanceOutcome, Inputs, JudgementOutcome, JudgementRequest, JudgementResult, KeyPurpose,
    NodeRef, OPERATION_SHAPE, Operation, OuterNode, PublicKey, Refusal, ReserveMovementOutcome,
    ResultBody, Reversal, ReversalConfirmationOutcome, ReversalConfirmationRequest,
    ReversalConfirmationResult, ReversalOutcome, Signature, SignatureScheme, Slippage, Statement,
    StatementShape, Terms, Tracking, Transfer, TransferOutcome, Withdrawal, check_amounts, member,
    signing_input, validate_statement,
};

/// An operation, the result that answers it, and the terms of the tuple it runs through.
type Answered<'a> = (&'a Operation, &'a ResultBody, &'a Terms);

use super::encoding::{is_canonical_signature, is_protocol_version, is_utc_timestamp};
use super::{
    Check, Exercised, Finding, Received, Report, RoleHarness, SignatureVerifier, SuiteContext,
    Uncovered, Verification, placeholder_key, placeholder_signature,
};

/// A node no provider advertises, for probing the edge of a provider's tuples.
const UNADVERTISED_NODE: &str = "conformance-unadvertised-node";
/// An asset no provider advertises.
const UNADVERTISED_ASSET: &str = "conformance-unadvertised-asset";
/// A class no verifier advertises.
const UNADVERTISED_CLASS: &str = "conformance.unadvertised_class";
/// What the provider-wide answers are required of.
const THE_PROVIDER: &str = "the provider";
/// The amount a reversal confirmation probe asks about.
const CREDIT: u128 = 1000;

/// Runs the role suite on a provider and reports what it found and exercised.
pub fn run_role_suite<P: RoleHarness + ?Sized>(
    provider: &mut P,
    context: &SuiteContext,
    verifier: &dyn SignatureVerifier,
) -> Report {
    let advertisement = provider.advertisement();
    let mut run = Run {
        context,
        verifier,
        advertisement: advertisement.statement().statement.clone(),
        report: Report::default(),
        next_transaction: 0,
        required: BTreeSet::new(),
        seen: BTreeSet::new(),
    };
    run.check_advertisement(&advertisement);
    run.probe(provider);
    run.report.uncovered = run
        .required
        .difference(&run.seen)
        .map(|(subject, answer)| Uncovered {
            subject: subject.clone(),
            answer: *answer,
        })
        .collect();
    run.report
}

/// The kind of an operation or result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Withdrawal,
    Deposit,
    Transfer,
    Conversion,
    BalanceVerification,
    Judgement,
    Reversal,
    ReversalConfirmation,
    HoldingBalance,
}

/// A refusal's class, whichever kind of refusal it is (`REQ-CAT-18`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefusalClass {
    LiftsAtKnownTime,
    WaitsOnPerson,
    Final,
}

impl From<&Refusal> for RefusalClass {
    fn from(refusal: &Refusal) -> Self {
        match refusal {
            Refusal::LiftsAtKnownTime(_) => Self::LiftsAtKnownTime,
            Refusal::WaitsOnPerson(_) => Self::WaitsOnPerson,
            Refusal::Final(_) => Self::Final,
        }
    }
}

impl From<&BalanceRefusal> for RefusalClass {
    fn from(refusal: &BalanceRefusal) -> Self {
        match refusal {
            BalanceRefusal::WaitsOnPerson(_) => Self::WaitsOnPerson,
            BalanceRefusal::Final(_) => Self::Final,
        }
    }
}

/// A quote, or a reserve's dry-run answer, as far as the checks need.
#[derive(Debug, Clone)]
struct Quote {
    amount_out: String,
    fee: Fee,
    rate: Option<String>,
    slippage: Option<Slippage>,
    direction: Option<Direction>,
}

/// A settled segment, as far as the checks need.
#[derive(Debug, Clone)]
struct Settled {
    amount_in: String,
    amount_out: String,
    fee: Fee,
    rate: Option<String>,
    evidence: Vec<EvidencePiece>,
}

/// What a result says, as far as the checks need.
#[derive(Debug, Clone)]
enum Status {
    Succeeded(Settled),
    Failed {
        evidence: Vec<EvidencePiece>,
    },
    Unknown,
    Refused(RefusalClass),
    DryRunAccepted(Quote),
    Answered,
    Judged {
        subject: String,
        class: String,
    },
    Reversed {
        node: String,
        asset: String,
        amount: String,
        evidence: Vec<EvidencePiece>,
    },
    Confirmed {
        segment: String,
        asset: String,
        amount: String,
    },
    NotConfirmed,
}

/// A result, seen through the checks.
#[derive(Debug, Clone)]
struct View {
    kind: Kind,
    transaction: Option<String>,
    status: Status,
}

/// The answer to one probe.
struct Answer {
    view: View,
    result: Statement<ResultBody>,
    changed_state: bool,
}

fn kind_of(operation: &Operation) -> (Kind, Option<&str>) {
    match operation {
        Operation::Withdrawal(o) => (Kind::Withdrawal, Some(&o.transaction)),
        Operation::Deposit(o) => (Kind::Deposit, Some(&o.transaction)),
        Operation::Transfer(o) => (Kind::Transfer, Some(&o.transaction)),
        Operation::Conversion(o) => (Kind::Conversion, Some(&o.transaction)),
        Operation::BalanceVerification(o) => (Kind::BalanceVerification, Some(&o.transaction)),
        Operation::Judgement(_) => (Kind::Judgement, None),
        Operation::Reversal(o) => (Kind::Reversal, Some(&o.transaction)),
        Operation::ReversalConfirmation(o) => (Kind::ReversalConfirmation, Some(&o.transaction)),
        Operation::HoldingBalance(o) => (Kind::HoldingBalance, Some(&o.transaction)),
    }
}

fn view_of(body: &ResultBody) -> View {
    fn reserve(outcome: &ReserveMovementOutcome) -> Status {
        match outcome {
            ReserveMovementOutcome::Succeeded(s) => Status::Succeeded(Settled {
                amount_in: s.amount_in.clone(),
                amount_out: s.amount_out.clone(),
                fee: s.fee.clone(),
                rate: None,
                evidence: s.evidence.clone(),
            }),
            ReserveMovementOutcome::Failed(f) => Status::Failed {
                evidence: f.evidence.clone(),
            },
            ReserveMovementOutcome::Unknown(_) => Status::Unknown,
            ReserveMovementOutcome::Refused(r) => Status::Refused((&r.refusal).into()),
            ReserveMovementOutcome::DryRunAccepted(q) => Status::DryRunAccepted(Quote {
                amount_out: q.amount_out.clone(),
                fee: q.fee.clone(),
                rate: None,
                slippage: None,
                direction: None,
            }),
        }
    }
    let (kind, transaction, status) = match body {
        ResultBody::Withdrawal(r) => (Kind::Withdrawal, Some(&r.transaction), reserve(&r.outcome)),
        ResultBody::Deposit(r) => (Kind::Deposit, Some(&r.transaction), reserve(&r.outcome)),
        ResultBody::Transfer(r) => (
            Kind::Transfer,
            Some(&r.transaction),
            match &r.outcome {
                TransferOutcome::Succeeded(s) => Status::Succeeded(Settled {
                    amount_in: s.amount_in.clone(),
                    amount_out: s.amount_out.clone(),
                    fee: s.fee.clone(),
                    rate: None,
                    evidence: s.evidence.clone(),
                }),
                TransferOutcome::Failed(f) => Status::Failed {
                    evidence: f.evidence.clone(),
                },
                TransferOutcome::Unknown(_) => Status::Unknown,
                TransferOutcome::Refused(r) => Status::Refused((&r.refusal).into()),
                TransferOutcome::DryRunAccepted(q) => Status::DryRunAccepted(Quote {
                    amount_out: q.amount_out.clone(),
                    fee: q.fee.clone(),
                    rate: None,
                    slippage: None,
                    direction: Some(q.direction),
                }),
            },
        ),
        ResultBody::Conversion(r) => (
            Kind::Conversion,
            Some(&r.transaction),
            match &r.outcome {
                ConversionOutcome::Succeeded(s) => Status::Succeeded(Settled {
                    amount_in: s.amount_in.clone(),
                    amount_out: s.amount_out.clone(),
                    fee: s.fee.clone(),
                    rate: Some(s.rate.clone()),
                    evidence: s.evidence.clone(),
                }),
                ConversionOutcome::Failed(f) => Status::Failed {
                    evidence: f.evidence.clone(),
                },
                ConversionOutcome::Unknown(_) => Status::Unknown,
                ConversionOutcome::Refused(r) => Status::Refused((&r.refusal).into()),
                ConversionOutcome::DryRunAccepted(q) => Status::DryRunAccepted(Quote {
                    amount_out: q.amount_out.clone(),
                    fee: q.fee.clone(),
                    rate: Some(q.rate.clone()),
                    slippage: Some(q.slippage.clone()),
                    direction: Some(q.direction),
                }),
            },
        ),
        ResultBody::BalanceVerification(r) => (
            Kind::BalanceVerification,
            Some(&r.transaction),
            match &r.outcome {
                BalanceOutcome::Answered(_) => Status::Answered,
                BalanceOutcome::Refused(r) => Status::Refused((&r.refusal).into()),
            },
        ),
        ResultBody::HoldingBalance(r) => (
            Kind::HoldingBalance,
            Some(&r.transaction),
            match &r.outcome {
                HoldingBalanceOutcome::Answered(_) => Status::Answered,
                HoldingBalanceOutcome::Refused(r) => Status::Refused((&r.refusal).into()),
            },
        ),
        ResultBody::Judgement(r) => (
            Kind::Judgement,
            None,
            match &r.outcome {
                JudgementOutcome::Judged(j) => Status::Judged {
                    subject: j.judgement.statement.subject.clone(),
                    class: j.judgement.statement.class.clone(),
                },
                JudgementOutcome::Refused(r) => Status::Refused((&r.refusal).into()),
            },
        ),
        ResultBody::Reversal(r) => (
            Kind::Reversal,
            Some(&r.transaction),
            match &r.outcome {
                ReversalOutcome::Succeeded(s) => Status::Reversed {
                    node: s.node.clone(),
                    asset: s.asset.clone(),
                    amount: s.amount.clone(),
                    evidence: s.evidence.clone(),
                },
                ReversalOutcome::Unknown(_) => Status::Unknown,
                ReversalOutcome::Refused(r) => Status::Refused((&r.refusal).into()),
            },
        ),
        ResultBody::ReversalConfirmation(r) => (
            Kind::ReversalConfirmation,
            Some(&r.transaction),
            match &r.outcome {
                ReversalConfirmationOutcome::Confirmed(c) => Status::Confirmed {
                    segment: c.confirmation.statement.segment.clone(),
                    asset: c.confirmation.statement.asset.clone(),
                    amount: c.confirmation.statement.amount.clone(),
                },
                ReversalConfirmationOutcome::NotConfirmed(_) => Status::NotConfirmed,
                ReversalConfirmationOutcome::Refused(r) => Status::Refused((&r.refusal).into()),
            },
        ),
    };
    View {
        kind,
        transaction: transaction.cloned(),
        status,
    }
}

/// Which part of an advertised tuple a probe replaces with something unadvertised.
#[derive(Debug, Clone, Copy)]
enum Off {
    Node,
    Asset,
}

/// What a probe of one tuple varies.
struct Probe {
    transaction: String,
    amount: String,
    dry_run: bool,
    inputs: Inputs,
    off: Option<Off>,
    session: String,
}

/// How to probe one advertised value-moving tuple.
struct Segment {
    /// What the probe is about, for findings and coverage.
    label: String,
    /// The tuple's operation, as a probe varies it.
    build: Box<dyn Fn(&Probe) -> Operation>,
    /// The tuple's terms.
    terms: Terms,
    /// For an exchange's or ramp's tuple, where a reversal puts the value back: the origin node and asset.
    origin: Option<(String, String)>,
    /// Whether the operation carries the holder's session: a withdrawal (`REQ-CAT-3`).
    in_session: bool,
}

struct Run<'a> {
    context: &'a SuiteContext,
    verifier: &'a dyn SignatureVerifier,
    advertisement: Advertisement,
    report: Report,
    next_transaction: u64,
    required: BTreeSet<(String, Exercised)>,
    seen: BTreeSet<(String, Exercised)>,
}

impl Run<'_> {
    fn flag(&mut self, check: Check, detail: impl Into<String>) {
        self.report.findings.push(Finding {
            check,
            detail: detail.into(),
        });
    }

    fn require(&mut self, subject: &str, answer: Exercised) {
        self.required.insert((subject.to_owned(), answer));
    }

    fn saw(&mut self, subject: &str, answer: Exercised) {
        self.report.exercised.insert(answer);
        self.seen.insert((subject.to_owned(), answer));
    }

    fn transaction(&mut self) -> String {
        self.next_transaction += 1;
        format!("conformance-tx-{}", self.next_transaction)
    }

    fn other_session(&self) -> String {
        format!("{}-another", self.context.session)
    }

    fn operation(&self, statement: Operation) -> Statement<Operation> {
        Statement {
            deployment: self.context.deployment.clone(),
            protocol_version: self.context.protocol_version.clone(),
            shape: OPERATION_SHAPE.to_owned(),
            maker: self.context.operator.clone(),
            made_at: "2026-11-06T15:04:05Z".to_owned(),
            signature: Signature {
                scheme: SignatureScheme::Ed25519,
                key: self.context.deployment.operator_key.clone(),
                value: placeholder_signature(),
            },
            statement,
        }
    }

    /// Whether the advertisement lists `key` for `statements`, the purpose a result is signed under.
    fn published_key(&self, key: &PublicKey) -> bool {
        self.advertisement.keys.iter().any(|k| {
            k.scheme == SignatureScheme::Ed25519
                && &k.public_key == key
                && k.purpose == KeyPurpose::Statements
        })
    }

    /// The envelope checks every statement gets: shape, context, encodings, and the signature hook.
    fn check_envelope<T: StatementShape>(
        &mut self,
        statement: &Statement<T>,
        received: &[u8],
        what: &str,
    ) {
        if let Err(e) = statement.check_shape() {
            self.flag(Check::EnvelopeWellFormed, format!("{what}: {e:?}"));
        }
        if let Err(e) =
            statement.check_context(&self.context.deployment, &self.context.protocol_version)
        {
            self.flag(Check::EnvelopeWellFormed, format!("{what}: {e:?}"));
        }
        // The operator key and the signing key need no check here: they are `PublicKey`s, which only their
        // canonical encoding builds, so a statement with another doesn't deserialise.
        let encodings = [
            (
                "signature value",
                is_canonical_signature(&statement.signature.value),
            ),
            (
                "protocol version",
                is_protocol_version(&statement.protocol_version),
            ),
            ("made_at", is_utc_timestamp(&statement.made_at)),
        ];
        for (field, ok) in encodings {
            if !ok {
                self.flag(
                    Check::EnvelopeWellFormed,
                    format!("{what}: malformed {field}"),
                );
            }
        }
        // The text itself must have a signing input: I-JSON with no member named twice, an envelope with a
        // `signature.value`. Typed reading alone can miss a repeat inside a free-form map.
        if let Err(e) = signing_input(received) {
            self.flag(
                Check::EnvelopeWellFormed,
                format!("{what}: no signing input under the contract's rule: {e:?}"),
            );
        }
        match self
            .verifier
            .verify(&statement.maker, &statement.signature, received)
        {
            Verification::Verified => {}
            Verification::Deferred => {
                self.report.signatures_deferred += 1;
                self.report.deferred.insert(Check::SignatureVerifies);
            }
            Verification::Failed(why) => {
                self.flag(Check::SignatureVerifies, format!("{what}: {why}"));
            }
        }
    }

    /// The maker and key checks for a result or a statement nested in one.
    fn check_maker_and_key<T>(&mut self, statement: &Statement<T>, what: &str) {
        if statement.maker != self.advertisement.provider {
            self.flag(
                Check::MakerAndKey,
                format!(
                    "{what}: made by {}, not {}",
                    statement.maker, self.advertisement.provider
                ),
            );
        }
        if !self.published_key(&statement.signature.key) {
            self.flag(
                Check::MakerAndKey,
                format!("{what}: signed with a key not published for statements"),
            );
        }
    }

    fn check_advertisement(&mut self, received: &Received<Advertisement>) {
        let advertisement = received.statement();
        self.check_envelope(advertisement, received.bytes(), "the advertisement");
        if let Err(violations) = validate_statement(advertisement) {
            self.flag(Check::AdvertisementValid, format!("{violations:?}"));
        }
    }

    /// Sends one operation, checks the result's envelope, maker and match, and says whether the provider's
    /// state changed.
    fn ask<P: RoleHarness + ?Sized>(
        &mut self,
        provider: &mut P,
        operation: Operation,
        what: &str,
    ) -> Answer {
        let statement = self.operation(operation);
        self.ask_statement(provider, statement, what)
    }

    /// Sends one operation statement as given, envelope included, and checks the answer as `ask` does.
    fn ask_statement<P: RoleHarness + ?Sized>(
        &mut self,
        provider: &mut P,
        statement: Statement<Operation>,
        what: &str,
    ) -> Answer {
        let (kind, transaction) = kind_of(&statement.statement);
        let transaction = transaction.map(str::to_owned);
        let before = provider.state();
        let result = provider.handle(&statement);
        let changed_state = provider.state() != before;
        self.check_result(&result, kind, transaction.as_deref(), what);
        let result = result.into_statement();
        Answer {
            view: view_of(&result.statement),
            result,
            changed_state,
        }
    }

    /// The text of a statement nested in a result's outcome under `name`, as it sat in the result received.
    fn nested(&mut self, received: &Received<ResultBody>, name: &str, what: &str) -> Vec<u8> {
        match member(received.bytes(), &["statement", "outcome", name]) {
            Ok(Some(text)) => text.to_vec(),
            Ok(None) | Err(_) => {
                self.flag(
                    Check::EnvelopeWellFormed,
                    format!("{what}: not found as its own text in the result received"),
                );
                Vec::new()
            }
        }
    }

    fn check_result(
        &mut self,
        received: &Received<ResultBody>,
        kind: Kind,
        transaction: Option<&str>,
        what: &str,
    ) {
        let result = received.statement();
        self.check_envelope(result, received.bytes(), what);
        self.check_maker_and_key(result, what);
        match &result.statement {
            ResultBody::Judgement(JudgementResult {
                outcome: JudgementOutcome::Judged(judged),
            }) => {
                let what = format!("{what}, its judgement");
                let nested = self.nested(received, "judgement", &what);
                self.check_envelope(&judged.judgement, &nested, &what);
                self.check_maker_and_key(&judged.judgement, &what);
            }
            ResultBody::ReversalConfirmation(ReversalConfirmationResult {
                outcome: ReversalConfirmationOutcome::Confirmed(confirmed),
                ..
            }) => {
                let what = format!("{what}, its confirmation");
                let nested = self.nested(received, "confirmation", &what);
                self.check_envelope(&confirmed.confirmation, &nested, &what);
                self.check_maker_and_key(&confirmed.confirmation, &what);
            }
            _ => {}
        }
        let view = view_of(&result.statement);
        if view.kind != kind {
            self.flag(
                Check::AnswersItsOperation,
                format!(
                    "{what}: a {:?} answered with a {:?} result",
                    kind, view.kind
                ),
            );
        }
        if view.transaction.as_deref() != transaction {
            self.flag(
                Check::AnswersItsOperation,
                format!(
                    "{what}: answers transaction {:?}, not {transaction:?}",
                    view.transaction
                ),
            );
        }
    }

    fn check_evidence(&mut self, evidence: &[EvidencePiece], what: &str) {
        let declared = self
            .advertisement
            .declarations
            .evidence
            .as_ref()
            .map(|e| e.grade);
        let best: Option<EvidenceGrade> = evidence.iter().map(|p| p.grade).min();
        match (declared, best) {
            (Some(declared), Some(best)) if best > declared => self.flag(
                Check::EvidenceAtDeclaredGrade,
                format!("{what}: best evidence {best:?} is weaker than the declared {declared:?}"),
            ),
            (Some(_), None) => self.flag(
                Check::EvidenceAtDeclaredGrade,
                format!("{what}: no evidence"),
            ),
            _ => {}
        }
    }

    /// A refusal of something the provider never serves must be final (`REQ-CAT-18`).
    fn check_final(&mut self, class: RefusalClass, what: &str) {
        if class != RefusalClass::Final {
            self.flag(
                Check::RefusalClass,
                format!("{what}: refused as {class:?}, not final"),
            );
        }
    }

    fn probe<P: RoleHarness + ?Sized>(&mut self, provider: &mut P) {
        let categories = self.advertisement.offers.categories();
        if categories.iter().any(|c| c.moves_value()) {
            self.require(THE_PROVIDER, Exercised::OutcomeUnknown);
            if self
                .advertisement
                .declarations
                .tracking
                .contains(&Tracking::StatusQuery)
            {
                self.require(THE_PROVIDER, Exercised::StatusQuery);
            }
        }
        let mut forced_unknown = false;
        let segments = self.segments();
        for segment in &segments {
            self.probe_segment(provider, segment, &mut forced_unknown);
        }
        if categories.contains(&Category::Reserve) {
            self.probe_balance(provider);
            self.probe_reversal_confirmations(provider);
        }
        if categories.contains(&Category::Verifier) {
            self.probe_judgements(provider);
        }
        self.probe_outside_role(provider, &categories);
    }

    /// An operation from elsewhere must be refused finally and change nothing (`REQ-PRT-1`). Each is sent several
    /// ways, so a provider that checks one field of the envelope but not another is caught:
    /// - under this deployment's name with another operator key, signed with that key;
    /// - under another deployment's name, signed with the operator's key;
    /// - naming another operator key in the deployment, signed with the operator's key;
    /// - under a higher major version, a higher patch version and a lower version;
    /// - in this deployment, signed with a key other than the operator's.
    ///
    /// `build` gets a fresh transaction for each; a replay of a known transaction ignores it.
    ///
    /// Anything but a final refusal is flagged under [`Check::RefusesForeignOperations`] alone.
    fn probe_foreign<P: RoleHarness + ?Sized>(
        &mut self,
        provider: &mut P,
        what: &str,
        build: &dyn Fn(String) -> Operation,
    ) {
        let other_key = [61, 62]
            .into_iter()
            .map(placeholder_key)
            .find(|key| key != &self.context.deployment.operator_key)
            .expect("two distinct placeholder keys");
        let mut variants = Vec::new();
        let tx = self.transaction();
        let mut statement = self.operation(build(tx));
        statement.deployment.operator_key = other_key.clone();
        statement.signature.key = other_key.clone();
        variants.push((
            "under this deployment's name with another operator key",
            statement,
        ));
        let tx = self.transaction();
        let mut statement = self.operation(build(tx));
        statement.deployment.name = format!("{}-elsewhere", self.context.deployment.name);
        variants.push(("under another deployment's name", statement));
        let tx = self.transaction();
        let mut statement = self.operation(build(tx));
        statement.deployment.operator_key = other_key.clone();
        variants.push((
            "naming another operator key, signed with the operator's",
            statement,
        ));
        // `check_context` asks for the exact protocol version, so each of these is foreign.
        for (how, version) in other_protocol_versions(&self.context.protocol_version) {
            let tx = self.transaction();
            let mut statement = self.operation(build(tx));
            statement.protocol_version = version;
            variants.push((how, statement));
        }
        let tx = self.transaction();
        let mut statement = self.operation(build(tx));
        statement.signature.key = other_key;
        variants.push(("signed with a key other than the operator's", statement));
        for (how, statement) in variants {
            let what = format!("{what}, {how}");
            let answer = self.ask_statement(provider, statement, &what);
            if answer.changed_state {
                self.flag(
                    Check::RefusesForeignOperations,
                    format!("{what}: the provider's state changed"),
                );
            }
            if !matches!(answer.view.status, Status::Refused(RefusalClass::Final)) {
                self.flag(
                    Check::RefusesForeignOperations,
                    format!(
                        "{what}: answered {:?}, not a final refusal",
                        answer.view.status
                    ),
                );
            }
        }
    }

    fn probe_segment<P: RoleHarness + ?Sized>(
        &mut self,
        provider: &mut P,
        segment: &Segment,
        forced_unknown: &mut bool,
    ) {
        let label = segment.label.clone();
        let Some((min, max)) = band_edges(&segment.terms) else {
            self.flag(
                Check::AdvertisementValid,
                format!("{label}: no amount band"),
            );
            return;
        };
        let inputs = inputs_for(&segment.terms);
        let session = self.context.session.clone();
        let probe = |transaction: String, amount: u128, dry_run: bool| Probe {
            transaction,
            amount: amount.to_string(),
            dry_run,
            inputs: inputs.clone(),
            off: None,
            session: session.clone(),
        };
        self.require(&label, Exercised::DryRunAccepted);
        self.require(&label, Exercised::Execution);
        self.require(&label, Exercised::RefusalOutsideTerms);
        if segment.origin.is_some() {
            self.require(&label, Exercised::Reversal);
        }
        if !segment.terms.required_inputs.is_empty() {
            self.require(&label, Exercised::RefusalForMissingInputs);
        }

        // A dry run at the band's minimum: its quote.
        let what = format!("{label}, dry run");
        let operation = (segment.build)(&probe(self.transaction(), min, true));
        let answer = self.ask(provider, operation.clone(), &what);
        if answer.changed_state {
            self.flag(
                Check::DryRunHasNoEffect,
                format!("{what}: the provider's state changed"),
            );
        }
        let quote = match answer.view.status {
            Status::DryRunAccepted(quote) => {
                self.saw(&label, Exercised::DryRunAccepted);
                self.check_amounts(
                    (&operation, &answer.result.statement, &segment.terms),
                    &what,
                );
                if let Some(direction) = quote.direction
                    && direction != Direction::ExactIn
                {
                    self.flag(
                        Check::QuoteHonoured,
                        format!("{what}: quotes {direction:?}, not the exact_in asked"),
                    );
                }
                Some(quote)
            }
            Status::Refused(_) => None,
            other => {
                self.flag(Check::DryRunAnswered, format!("{what}: answered {other:?}"));
                None
            }
        };

        // A real operation at the band's minimum, against the quote.
        let tx = self.transaction();
        let what = format!("{label}, execution");
        let operation = (segment.build)(&probe(tx.clone(), min, false));
        let before = provider.state();
        let answer = self.ask(provider, operation.clone(), &what);
        self.check_execution(
            &label,
            &answer.view,
            (
                (&operation, &answer.result.statement, &segment.terms),
                quote.as_ref(),
            ),
            &what,
        );
        if matches!(answer.view.status, Status::Succeeded(_)) && !answer.changed_state {
            self.flag(
                Check::ExecutionReported,
                format!("{what}: succeeded, but the provider's state didn't change"),
            );
        }
        if matches!(answer.view.status, Status::Failed { .. }) && answer.changed_state {
            self.flag(
                Check::ExecutionReported,
                format!("{what}: failed, but the provider's state changed"),
            );
        }

        // The same transaction again, where the provider declares a repeat safe, and never otherwise
        // (`REQ-CAT-10`).
        let executed = matches!(
            answer.view.status,
            Status::Succeeded(_) | Status::Failed { .. }
        );
        if executed && self.advertisement.declarations.safe_retry == Some(true) {
            let what = format!("{label}, repeated");
            let again = self.ask(provider, operation.clone(), &what);
            if again.changed_state {
                self.flag(
                    Check::SafeRetry,
                    format!("{what}: a repeat of {tx} changed the provider's state"),
                );
            }
            if again.result.statement != answer.result.statement {
                self.flag(
                    Check::SafeRetry,
                    format!("{what}: a repeat of {tx} was answered differently"),
                );
            }
        }

        // The executed transaction replayed from elsewhere (`REQ-PRT-1`): a provider must check the envelope
        // before it answers from what it has recorded.
        if executed {
            self.probe_foreign(
                provider,
                &format!("{label}, a replay of the executed {tx}"),
                &|_| operation.clone(),
            );
        }

        // An exchange's or ramp's reversals: of the settled segment, which nothing obliges it to undo; of a
        // failed one, which it owes (`AC-4`, `INV-3`); and of one it never served, which it refuses.
        if let Some((node, asset)) = &segment.origin {
            let provider_id = self.advertisement.provider.clone();
            if let Status::Succeeded(settled) = &answer.view.status {
                let amount = settled.amount_in.clone();
                self.probe_foreign(
                    provider,
                    &format!("{label}, a reversal of the settled segment"),
                    &|t| reversal_operation(&provider_id, t, &tx, node, asset, &amount),
                );
                self.probe_reversal(
                    provider,
                    &label,
                    (&tx, node, asset, &settled.amount_in),
                    &before,
                    false,
                );
            }
            self.require(&label, Exercised::Failure);
            if provider.force_failure() {
                self.report.hooks_used = true;
                let tx = self.transaction();
                let what = format!("{label}, driven to fail");
                let before = provider.state();
                let operation = (segment.build)(&probe(tx.clone(), min, false));
                let answer = self.ask(provider, operation, &what);
                match &answer.view.status {
                    Status::Failed { evidence } => {
                        self.saw(&label, Exercised::Failure);
                        self.check_evidence(evidence, &what);
                        if answer.changed_state {
                            self.flag(
                                Check::ExecutionReported,
                                format!("{what}: failed, but the provider's state changed"),
                            );
                        }
                        self.probe_reversal(
                            provider,
                            &label,
                            (&tx, node, asset, &min.to_string()),
                            &before,
                            true,
                        );
                    }
                    other => self.flag(
                        Check::ExecutionReported,
                        format!("{what}: reported {other:?}, not a failure"),
                    ),
                }
            }
            // A second failed segment, whose reversal is owed, asked for only from elsewhere: a provider that
            // served those would really undo it. The suite leaves it unreversed.
            let second = format!("{label}, a second failure");
            self.require(&second, Exercised::Failure);
            if provider.force_failure() {
                self.report.hooks_used = true;
                let failed = self.transaction();
                let what = format!("{label}, driven to fail again");
                let operation = (segment.build)(&probe(failed.clone(), min, false));
                let answer = self.ask(provider, operation, &what);
                match &answer.view.status {
                    Status::Failed { evidence } => {
                        self.saw(&second, Exercised::Failure);
                        self.check_evidence(evidence, &what);
                        if answer.changed_state {
                            self.flag(
                                Check::ExecutionReported,
                                format!("{what}: failed, but the provider's state changed"),
                            );
                        }
                        let amount = min.to_string();
                        self.probe_foreign(
                            provider,
                            &format!("{label}, a reversal of a failed segment"),
                            &|t| reversal_operation(&provider_id, t, &failed, node, asset, &amount),
                        );
                    }
                    other => self.flag(
                        Check::ExecutionReported,
                        format!("{what}: reported {other:?}, not a failure"),
                    ),
                }
            }
            let what = format!("{label}, reversal of a segment never served");
            let never = self.transaction();
            let operation = self.reversal(&never, node, asset, &min.to_string());
            let answer = self.ask(provider, operation, &what);
            if answer.changed_state {
                self.flag(
                    Check::ReversalIsTrueUndo,
                    format!("{what}: the provider's state changed"),
                );
            }
            match answer.view.status {
                Status::Refused(class) => self.check_final(class, &what),
                other => self.flag(
                    Check::ReversalIsTrueUndo,
                    format!("{what}: answered {other:?}, not a refusal"),
                ),
            }
        }

        // The same dry run without the inputs the tuple declares (`REQ-CAT-12`).
        if !segment.terms.required_inputs.is_empty() {
            let what = format!("{label}, without its required inputs");
            let mut bare = probe(self.transaction(), min, true);
            bare.inputs = Inputs::new();
            let answer = self.ask(provider, (segment.build)(&bare), &what);
            if answer.changed_state {
                self.flag(
                    Check::DryRunHasNoEffect,
                    format!("{what}: the provider's state changed"),
                );
            }
            match answer.view.status {
                Status::Refused(RefusalClass::WaitsOnPerson) => {
                    self.saw(&label, Exercised::RefusalForMissingInputs);
                }
                Status::Refused(class) => self.flag(
                    Check::RefusalClass,
                    format!("{what}: refused as {class:?}, not waits_on_person"),
                ),
                other => self.flag(
                    Check::RequiresDeclaredInputs,
                    format!("{what}: answered {other:?}, not a refusal"),
                ),
            }
        }

        // A withdrawal dry run outside the holder's session reveals sufficiency (`REQ-CAT-3`, `REQ-CAT-4`).
        if segment.in_session {
            let what = format!("{label}, dry run outside the holder's session");
            let mut elsewhere = probe(self.transaction(), min, true);
            elsewhere.session = self.other_session();
            let answer = self.ask(provider, (segment.build)(&elsewhere), &what);
            if answer.changed_state {
                self.flag(
                    Check::DryRunHasNoEffect,
                    format!("{what}: the provider's state changed"),
                );
            }
            if !matches!(answer.view.status, Status::Refused(_)) {
                self.flag(
                    Check::BalanceRevealsOnlySufficiency,
                    format!("{what}: answered {:?}, not a refusal", answer.view.status),
                );
            }
        }

        // The band's edges, a real operation above them, and a tuple with an unadvertised node or asset.
        let mut outside = vec![(
            format!("{label}, above its band"),
            probe(self.transaction(), max + 1, true),
        )];
        if min > 1 {
            outside.push((
                format!("{label}, below its band"),
                probe(self.transaction(), min - 1, true),
            ));
        }
        for (off, name) in [(Off::Node, "node"), (Off::Asset, "asset")] {
            let mut unadvertised = probe(self.transaction(), min, true);
            unadvertised.off = Some(off);
            outside.push((format!("{label}, on an unadvertised {name}"), unadvertised));
        }
        outside.push((
            format!("{label}, executed above its band"),
            probe(self.transaction(), max + 1, false),
        ));
        for (what, probe) in outside {
            let answer = self.ask(provider, (segment.build)(&probe), &what);
            if answer.changed_state {
                let check = if probe.dry_run {
                    Check::DryRunHasNoEffect
                } else {
                    Check::StaysWithinTuplesAndBands
                };
                self.flag(check, format!("{what}: the provider's state changed"));
            }
            match answer.view.status {
                Status::Refused(class) => {
                    self.saw(&label, Exercised::RefusalOutsideTerms);
                    self.check_final(class, &what);
                }
                other => self.flag(
                    Check::StaysWithinTuplesAndBands,
                    format!("{what}: answered {other:?}, not a refusal"),
                ),
            }
        }

        // Operations from elsewhere, as dry runs and as real ones (`REQ-PRT-1`).
        self.probe_foreign(provider, &format!("{label}, a dry run"), &|t| {
            (segment.build)(&probe(t, min, true))
        });
        self.probe_foreign(provider, &format!("{label}, an execution"), &|t| {
            (segment.build)(&probe(t, min, false))
        });

        // Outcome unknown, once per provider, where the provider can be driven to it.
        if !*forced_unknown && provider.force_outcome_unknown() {
            *forced_unknown = true;
            self.report.hooks_used = true;
            let tx = self.transaction();
            let what = format!("{label}, outcome unknown");
            let operation = (segment.build)(&probe(tx.clone(), min, false));
            let (kind, _) = kind_of(&operation);
            let answer = self.ask(provider, operation.clone(), &what);
            match answer.view.status {
                Status::Unknown => self.saw(THE_PROVIDER, Exercised::OutcomeUnknown),
                other => self.flag(
                    Check::OutcomeUnknownDistinct,
                    format!("{what}: reported {other:?}, not outcome unknown"),
                ),
            }
            if self
                .advertisement
                .declarations
                .tracking
                .contains(&Tracking::StatusQuery)
            {
                self.probe_status(
                    provider,
                    (kind, &tx),
                    (&label, &what),
                    (&operation, &segment.terms),
                    quote.as_ref(),
                );
            }
        }
    }

    /// A status query for a transaction left at outcome unknown must resolve it (`REQ-CAT-16`, `REQ-ORD-1`). A
    /// success it reports is held to the tuple's quote like any other.
    fn probe_status<P: RoleHarness + ?Sized>(
        &mut self,
        provider: &mut P,
        (kind, tx): (Kind, &str),
        (label, what): (&str, &str),
        (operation, terms): (&Operation, &Terms),
        quote: Option<&Quote>,
    ) {
        let what = format!("{what}, status query");
        let Some(result) = provider.status(tx) else {
            self.flag(
                Check::OutcomeUnknownDistinct,
                format!("{what}: the declared status query gave no answer"),
            );
            return;
        };
        self.check_result(&result, kind, Some(tx), &what);
        let result = result.into_statement();
        let view = view_of(&result.statement);
        match &view.status {
            Status::Succeeded(_) | Status::Failed { .. } => {
                self.saw(THE_PROVIDER, Exercised::StatusQuery);
                self.check_execution(
                    label,
                    &view,
                    ((operation, &result.statement, terms), quote),
                    &what,
                );
            }
            Status::Unknown => self.flag(
                Check::OutcomeUnknownDistinct,
                format!("{what}: still unknown, which doesn't resolve it"),
            ),
            other => self.flag(
                Check::OutcomeUnknownDistinct,
                format!("{what}: answered {other:?}"),
            ),
        }
    }

    fn reversal(&self, reverses: &str, node: &str, asset: &str, amount: &str) -> Operation {
        reversal_operation(
            &self.advertisement.provider,
            format!("{reverses}-reversal"),
            reverses,
            node,
            asset,
            amount,
        )
    }

    /// The reversal of a segment the provider served. If it succeeds, it is a true undo: back at the origin, in
    /// full and in the asset it left, and the provider's state as before the segment (`REQ-CAT-15`, `REQ-ORD-3`).
    /// A failed segment's reversal is `owed`: a final refusal is flagged, and anything else but a success leaves it
    /// owed (`REQ-CAT-7`) and uncovered. A settled segment's reversal may be refused in any class.
    fn probe_reversal<P: RoleHarness + ?Sized>(
        &mut self,
        provider: &mut P,
        label: &str,
        (tx, node, asset, amount): (&str, &str, &str, &str),
        before: &str,
        owed: bool,
    ) {
        let what = if owed {
            format!("{label}, reversal of the failed segment")
        } else {
            format!("{label}, reversal of the settled segment")
        };
        let operation = self.reversal(tx, node, asset, amount);
        let answer = self.ask(provider, operation, &what);
        match answer.view.status {
            Status::Reversed {
                node: back_at,
                asset: back_in,
                amount: back,
                evidence,
            } => {
                if owed {
                    self.saw(label, Exercised::Reversal);
                }
                if provider.state() != before {
                    self.flag(
                        Check::ReversalIsTrueUndo,
                        format!("{what}: the provider's state isn't as before the segment"),
                    );
                }
                if back_at != node || back_in != asset || back != amount {
                    self.flag(
                        Check::ReversalIsTrueUndo,
                        format!(
                            "{what}: {back} {back_in} back at {back_at}, not {amount} {asset} at {node}"
                        ),
                    );
                }
                self.check_evidence(&evidence, &what);
            }
            Status::Refused(RefusalClass::Final) if owed => self.flag(
                Check::ReversalIsTrueUndo,
                format!(
                    "{what}: refused finally to undo a failed segment, which it should have declined"
                ),
            ),
            Status::Refused(RefusalClass::LiftsAtKnownTime) => {
                self.saw(label, Exercised::RefusalLiftingAtKnownTime);
            }
            // Owed and not discharged (`REQ-CAT-7`), or not owed at all.
            _ => {}
        }
    }

    /// A real operation's outcome. A success counts as the tuple's execution only once it is compared with the
    /// tuple's quote, so every quote meets an execution.
    fn check_execution(
        &mut self,
        label: &str,
        view: &View,
        (answered, quote): (Answered<'_>, Option<&Quote>),
        what: &str,
    ) {
        match &view.status {
            Status::Succeeded(settled) => {
                if let Some(quote) = quote {
                    self.check_quote(quote, settled, what);
                    self.saw(label, Exercised::Execution);
                }
                self.check_amounts(answered, what);
                self.check_evidence(&settled.evidence, what);
            }
            Status::Failed { evidence } => self.check_evidence(evidence, what),
            Status::Unknown | Status::Refused(_) => {}
            other => self.flag(
                Check::ExecutionReported,
                format!("{what}: answered {other:?}"),
            ),
        }
    }

    /// A result's amounts against its operation and the tuple's terms, by the contract's own rule
    /// (`check_amounts`): its fee in the asset the segment takes in and within what it took, and for `exact_in`
    /// no more taken in than stated and no remainder beyond the tuple's fill increment (`REQ-QTE-5`,
    /// `REQ-CAT-11`, `THR-108`).
    fn check_amounts(&mut self, (operation, result, terms): Answered<'_>, what: &str) {
        if let Err(mismatches) = check_amounts(operation, result, terms) {
            for mismatch in mismatches {
                self.flag(Check::AnswersItsOperation, format!("{what}: {mismatch:?}"));
            }
        }
    }

    /// An execution against its quote: no higher fee, and no less out, nor a lower rate, than quoted beyond the
    /// quote's slippage regime. A firm quote, a ramp's and a reserve's allow none; a floating one bounds nothing.
    fn check_quote(&mut self, quote: &Quote, settled: &Settled, what: &str) {
        let fee_higher = settled.fee.asset != quote.fee.asset
            || match (decimal(&settled.fee.amount), decimal(&quote.fee.amount)) {
                (Some(charged), Some(quoted)) => short_by_more_than(quoted, charged, 0),
                _ => true,
            };
        if fee_higher {
            self.flag(
                Check::QuoteHonoured,
                format!(
                    "{what}: charged {} {}, quoted {} {}",
                    settled.fee.amount, settled.fee.asset, quote.fee.amount, quote.fee.asset
                ),
            );
        }
        let tolerance = match &quote.slippage {
            None | Some(Slippage::Firm(_)) => Some(0),
            Some(Slippage::Bounded(bounded)) => Some(bounded.max_bps.get()),
            Some(Slippage::Floating(_)) => None,
        };
        let Some(bps) = tolerance else {
            return;
        };
        let mut compared = vec![("out", &settled.amount_out, &quote.amount_out)];
        if let (Some(rate), Some(quoted)) = (&settled.rate, &quote.rate) {
            compared.push(("the rate", rate, quoted));
        }
        for (name, actual, quoted) in compared {
            let short = match (decimal(actual), decimal(quoted)) {
                (Some(a), Some(q)) => short_by_more_than(a, q, bps),
                _ => true,
            };
            if short {
                self.flag(
                    Check::QuoteHonoured,
                    format!("{what}: {name} {actual}, quoted {quoted} within {bps} basis points"),
                );
            }
        }
    }

    fn probe_balance<P: RoleHarness + ?Sized>(&mut self, provider: &mut P) {
        let assets: BTreeSet<String> = self
            .advertisement
            .offers
            .reserve
            .iter()
            .flat_map(|o| &o.tuples)
            .map(|t| t.asset.clone())
            .collect();
        for asset in assets {
            let subject = format!("balance checks of {asset}");
            self.require(&subject, Exercised::BalanceAnswered);
            let asks = [
                (self.context.session.clone(), "1"),
                (
                    self.context.session.clone(),
                    "1000000000000000000000000000000",
                ),
                (self.other_session(), "1"),
            ];
            for (session, stated) in asks {
                let in_session = session == self.context.session;
                let what = if in_session {
                    format!("balance check of {stated} {asset}")
                } else {
                    format!("balance check of {stated} {asset} outside the holder's session")
                };
                let operation = Operation::BalanceVerification(BalanceVerification {
                    transaction: self.transaction(),
                    provider: self.advertisement.provider.clone(),
                    session,
                    account: self.context.account.clone(),
                    asset: asset.clone(),
                    amount: stated.to_owned(),
                });
                let answer = self.ask(provider, operation, &what);
                if answer.changed_state {
                    self.flag(
                        Check::BalanceRevealsOnlySufficiency,
                        format!("{what}: the provider's state changed"),
                    );
                }
                match answer.view.status {
                    Status::Answered if in_session => {
                        self.saw(&subject, Exercised::BalanceAnswered);
                    }
                    Status::Refused(_) => {}
                    other => self.flag(
                        Check::BalanceRevealsOnlySufficiency,
                        format!("{what}: answered {other:?}"),
                    ),
                }
            }
            // From elsewhere, an in-session check the holder's funds would answer yes (`INV-9`).
            let (provider_id, session, account) = (
                self.advertisement.provider.clone(),
                self.context.session.clone(),
                self.context.account.clone(),
            );
            self.probe_foreign(provider, &format!("a balance check of 1 {asset}"), &|t| {
                Operation::BalanceVerification(BalanceVerification {
                    transaction: t,
                    provider: provider_id.clone(),
                    session: session.clone(),
                    account: account.clone(),
                    asset: asset.clone(),
                    amount: "1".to_owned(),
                })
            });
        }
    }

    /// A reserve's confirmations: one naming the segment, asset and amount for the credit that arrived whole, and
    /// none for a credit that never arrived, arrived short, or is asked about under another segment or asset
    /// (ADR-0008, `REQ-ORD-3`).
    fn probe_reversal_confirmations<P: RoleHarness + ?Sized>(&mut self, provider: &mut P) {
        let mut assets = BTreeSet::new();
        let credits: Vec<(String, String)> = self
            .advertisement
            .offers
            .reserve
            .iter()
            .flat_map(|o| &o.tuples)
            .filter_map(|t| {
                let node = match (&t.origin, &t.destination) {
                    (NodeRef::Inner(node), _) | (_, NodeRef::Inner(node)) => node.clone(),
                    _ => return None,
                };
                assets
                    .insert(t.asset.clone())
                    .then(|| (t.asset.clone(), node))
            })
            .collect();
        let advertised: Vec<String> = credits.iter().map(|(asset, _)| asset.clone()).collect();
        for (asset, node) in credits {
            let subject = format!("reversal confirmations of {asset}");
            self.require(&subject, Exercised::ReversalNotConfirmed);
            self.require(&subject, Exercised::ReversalConfirmed);

            let never = self.transaction();
            self.ask_confirmation(provider, &subject, (&never, &asset, CREDIT), false);

            let segment = self.transaction();
            if provider.credit_reversal(&segment, &node, &asset, &CREDIT.to_string()) {
                self.report.hooks_used = true;
                self.ask_confirmation(provider, &subject, (&segment, &asset, CREDIT), true);
                self.ask_confirmation(provider, &subject, (&segment, &asset, CREDIT + 1), false);
                let fresh = self.transaction();
                self.ask_confirmation(provider, &subject, (&fresh, &asset, CREDIT), false);
                // Another asset the reserve advertises, where it has one, so the probe isn't an obvious refusal.
                let other_asset = advertised
                    .iter()
                    .find(|a| **a != asset)
                    .map_or(UNADVERTISED_ASSET, String::as_str);
                self.ask_confirmation(provider, &subject, (&segment, other_asset, CREDIT), false);
                // From elsewhere, the question the credit would answer yes (ADR-0008).
                let provider_id = self.advertisement.provider.clone();
                self.probe_foreign(
                    provider,
                    &format!("a confirmation of {CREDIT} {asset} for {segment}, which arrived"),
                    &|t| {
                        Operation::ReversalConfirmation(ReversalConfirmationRequest {
                            transaction: t,
                            provider: provider_id.clone(),
                            segment: segment.clone(),
                            asset: asset.clone(),
                            amount: CREDIT.to_string(),
                        })
                    },
                );
            }
        }
    }

    fn ask_confirmation<P: RoleHarness + ?Sized>(
        &mut self,
        provider: &mut P,
        subject: &str,
        (segment, asset, amount): (&str, &str, u128),
        arrived: bool,
    ) {
        let what = if arrived {
            format!("confirmation of {amount} {asset} for {segment}, which arrived")
        } else {
            format!("confirmation of {amount} {asset} for {segment}, which didn't arrive as asked")
        };
        let operation = Operation::ReversalConfirmation(ReversalConfirmationRequest {
            transaction: self.transaction(),
            provider: self.advertisement.provider.clone(),
            segment: segment.to_owned(),
            asset: asset.to_owned(),
            amount: amount.to_string(),
        });
        let answer = self.ask(provider, operation, &what);
        if answer.changed_state {
            self.flag(
                Check::ReversalConfirmationMatches,
                format!("{what}: the provider's state changed"),
            );
        }
        match answer.view.status {
            Status::Confirmed {
                segment: confirmed,
                asset: confirmed_asset,
                amount: confirmed_amount,
            } => {
                if !arrived {
                    self.flag(
                        Check::ReversalConfirmationMatches,
                        format!("{what}: confirmed"),
                    );
                } else if confirmed != segment
                    || confirmed_asset != asset
                    || confirmed_amount != amount.to_string()
                {
                    self.flag(
                        Check::ReversalConfirmationMatches,
                        format!(
                            "{what}: confirms {confirmed_amount} {confirmed_asset} for {confirmed}"
                        ),
                    );
                } else {
                    self.saw(subject, Exercised::ReversalConfirmed);
                }
            }
            Status::NotConfirmed if !arrived => self.saw(subject, Exercised::ReversalNotConfirmed),
            // A credit that arrived but isn't confirmed, or a refusal, leaves the confirmation uncovered.
            _ => {}
        }
    }

    fn probe_judgements<P: RoleHarness + ?Sized>(&mut self, provider: &mut P) {
        let classes: Vec<String> = self
            .advertisement
            .offers
            .verifier
            .iter()
            .flat_map(|o| o.classes.clone())
            .collect();
        for class in classes {
            let what = format!("judgement of {class}");
            self.require(&what, Exercised::Judged);
            let operation = self.judgement_question(&class);
            self.probe_foreign(
                provider,
                &format!("a judgement question of {class}"),
                &|_| operation.clone(),
            );
            let answer = self.ask(provider, operation, &what);
            match answer.view.status {
                Status::Judged {
                    subject,
                    class: judged,
                } => {
                    self.saw(&what, Exercised::Judged);
                    if subject != self.context.subject || judged != class {
                        self.flag(
                            Check::JudgementMatchesQuestion,
                            format!("{what}: judged {subject} for {judged}"),
                        );
                    }
                }
                Status::Refused(_) => {}
                other => self.flag(
                    Check::JudgementMatchesQuestion,
                    format!("{what}: answered {other:?}"),
                ),
            }
        }
        let what = "judgement of an unadvertised class".to_owned();
        self.require(&what, Exercised::RefusalOutsideTerms);
        let operation = self.judgement_question(UNADVERTISED_CLASS);
        let answer = self.ask(provider, operation, &what);
        match answer.view.status {
            Status::Refused(class) => {
                self.saw(&what, Exercised::RefusalOutsideTerms);
                self.check_final(class, &what);
            }
            other => self.flag(
                Check::StaysWithinTuplesAndBands,
                format!("{what}: answered {other:?}, not a refusal"),
            ),
        }
    }

    fn judgement_question(&self, class: &str) -> Operation {
        Operation::Judgement(JudgementRequest {
            provider: self.advertisement.provider.clone(),
            subject: self.context.subject.clone(),
            class: class.to_owned(),
        })
    }

    fn probe_outside_role<P: RoleHarness + ?Sized>(
        &mut self,
        provider: &mut P,
        categories: &[Category],
    ) {
        let provider_id = self.advertisement.provider.clone();
        let mut probes: Vec<(&str, Operation)> = Vec::new();
        if !categories.contains(&Category::Reserve) {
            probes.push((
                "a withdrawal",
                Operation::Withdrawal(Withdrawal {
                    transaction: self.transaction(),
                    provider: provider_id.clone(),
                    session: Some(self.context.session.clone()),
                    origin: OuterNode {
                        network: UNADVERTISED_NODE.to_owned(),
                    },
                    destination: UNADVERTISED_NODE.to_owned(),
                    asset: "USDC".to_owned(),
                    amount: "1000000".to_owned(),
                    dry_run: false,
                    inputs: Inputs::new(),
                }),
            ));
            probes.push((
                "a deposit",
                Operation::Deposit(Deposit {
                    transaction: self.transaction(),
                    provider: provider_id.clone(),
                    origin: UNADVERTISED_NODE.to_owned(),
                    destination: OuterNode {
                        network: UNADVERTISED_NODE.to_owned(),
                    },
                    asset: "USDC".to_owned(),
                    amount: "1000000".to_owned(),
                    dry_run: false,
                    inputs: Inputs::new(),
                }),
            ));
            probes.push((
                "a balance check",
                Operation::BalanceVerification(BalanceVerification {
                    transaction: self.transaction(),
                    provider: provider_id.clone(),
                    session: self.context.session.clone(),
                    account: self.context.account.clone(),
                    asset: "USDC".to_owned(),
                    amount: "1".to_owned(),
                }),
            ));
            probes.push((
                "a reversal confirmation",
                Operation::ReversalConfirmation(ReversalConfirmationRequest {
                    transaction: self.transaction(),
                    provider: provider_id.clone(),
                    segment: self.transaction(),
                    asset: "USDC".to_owned(),
                    amount: "1000000".to_owned(),
                }),
            ));
        }
        if !categories.contains(&Category::Ramp) {
            probes.push((
                "a transfer",
                Operation::Transfer(Transfer {
                    transaction: self.transaction(),
                    provider: provider_id.clone(),
                    origin: "polygon".to_owned(),
                    destination: "bsc".to_owned(),
                    asset: "USDC".to_owned(),
                    direction: Direction::ExactIn,
                    amount: "1000000".to_owned(),
                    dry_run: false,
                    inputs: Inputs::new(),
                }),
            ));
        }
        if !categories.contains(&Category::Exchange) {
            probes.push((
                "a conversion",
                Operation::Conversion(Conversion {
                    transaction: self.transaction(),
                    provider: provider_id.clone(),
                    node: "polygon".to_owned(),
                    from: "USDC".to_owned(),
                    to: "USDT".to_owned(),
                    direction: Direction::ExactIn,
                    amount: "1000000".to_owned(),
                    dry_run: false,
                    inputs: Inputs::new(),
                }),
            ));
        }
        if !categories.contains(&Category::Exchange) && !categories.contains(&Category::Ramp) {
            let reverses = self.transaction();
            probes.push((
                "a reversal",
                self.reversal(&reverses, "polygon", "USDC", "1000000"),
            ));
        }
        if !categories.contains(&Category::Verifier) {
            probes.push((
                "a judgement question",
                self.judgement_question("kyc.natural_person"),
            ));
        }
        if !probes.is_empty() {
            self.require(THE_PROVIDER, Exercised::RefusalOutsideRole);
        }
        for (name, operation) in probes {
            let what = format!("{name}, outside the provider's role");
            let answer = self.ask(provider, operation, &what);
            if answer.changed_state {
                self.flag(
                    Check::StaysWithinRole,
                    format!("{what}: the provider's state changed"),
                );
            }
            match answer.view.status {
                Status::Refused(class) => {
                    self.saw(THE_PROVIDER, Exercised::RefusalOutsideRole);
                    self.check_final(class, &what);
                }
                other => self.flag(
                    Check::StaysWithinRole,
                    format!("{what}: answered {other:?}, not a refusal"),
                ),
            }
        }
    }

    /// One probe per advertised value-moving tuple.
    fn segments(&self) -> Vec<Segment> {
        let provider = self.advertisement.provider.clone();
        let mut segments = Vec::new();
        let node_or = |off: Option<Off>, node: &str| match off {
            Some(Off::Node) => UNADVERTISED_NODE.to_owned(),
            _ => node.to_owned(),
        };
        let asset_or = |off: Option<Off>, asset: &str| match off {
            Some(Off::Asset) => UNADVERTISED_ASSET.to_owned(),
            _ => asset.to_owned(),
        };
        for tuple in self
            .advertisement
            .offers
            .reserve
            .iter()
            .flat_map(|o| o.tuples.clone())
        {
            let (p, asset) = (provider.clone(), tuple.asset.clone());
            match (&tuple.origin, &tuple.destination) {
                (NodeRef::Outer(origin), NodeRef::Inner(destination)) => {
                    let (origin, destination) = (origin.clone(), destination.clone());
                    segments.push(Segment {
                        label: format!(
                            "withdrawal of {asset} from {} into {destination}",
                            origin.network
                        ),
                        build: Box::new(move |probe: &Probe| {
                            Operation::Withdrawal(Withdrawal {
                                transaction: probe.transaction.clone(),
                                provider: p.clone(),
                                session: Some(probe.session.clone()),
                                origin: origin.clone(),
                                destination: node_or(probe.off, &destination),
                                asset: asset_or(probe.off, &asset),
                                amount: probe.amount.clone(),
                                dry_run: probe.dry_run,
                                inputs: probe.inputs.clone(),
                            })
                        }),
                        terms: tuple.terms.clone(),
                        origin: None,
                        in_session: true,
                    });
                }
                (NodeRef::Inner(origin), NodeRef::Outer(destination)) => {
                    let (origin, destination) = (origin.clone(), destination.clone());
                    segments.push(Segment {
                        label: format!(
                            "deposit of {asset} from {origin} out to {}",
                            destination.network
                        ),
                        build: Box::new(move |probe: &Probe| {
                            Operation::Deposit(Deposit {
                                transaction: probe.transaction.clone(),
                                provider: p.clone(),
                                origin: node_or(probe.off, &origin),
                                destination: destination.clone(),
                                asset: asset_or(probe.off, &asset),
                                amount: probe.amount.clone(),
                                dry_run: probe.dry_run,
                                inputs: probe.inputs.clone(),
                            })
                        }),
                        terms: tuple.terms.clone(),
                        origin: None,
                        in_session: false,
                    });
                }
                _ => {}
            }
        }
        for tuple in self
            .advertisement
            .offers
            .exchange
            .iter()
            .flat_map(|o| o.tuples.clone())
        {
            let (p, node, from, to) = (
                provider.clone(),
                tuple.node.clone(),
                tuple.from.id.clone(),
                tuple.to.id.clone(),
            );
            segments.push(Segment {
                label: format!("conversion of {from} to {to} at {node}"),
                origin: Some((node.clone(), from.clone())),
                build: Box::new(move |probe: &Probe| {
                    Operation::Conversion(Conversion {
                        transaction: probe.transaction.clone(),
                        provider: p.clone(),
                        node: node_or(probe.off, &node),
                        from: from.clone(),
                        to: asset_or(probe.off, &to),
                        direction: Direction::ExactIn,
                        amount: probe.amount.clone(),
                        dry_run: probe.dry_run,
                        inputs: probe.inputs.clone(),
                    })
                }),
                terms: tuple.terms.clone(),
                in_session: false,
            });
        }
        for tuple in self
            .advertisement
            .offers
            .ramp
            .iter()
            .flat_map(|o| o.tuples.clone())
        {
            let (p, origin, destination, asset) = (
                provider.clone(),
                tuple.origin.clone(),
                tuple.destination.clone(),
                tuple.asset.clone(),
            );
            segments.push(Segment {
                label: format!("transfer of {asset} from {origin} to {destination}"),
                origin: Some((origin.clone(), asset.clone())),
                build: Box::new(move |probe: &Probe| {
                    Operation::Transfer(Transfer {
                        transaction: probe.transaction.clone(),
                        provider: p.clone(),
                        origin: origin.clone(),
                        destination: node_or(probe.off, &destination),
                        asset: asset_or(probe.off, &asset),
                        direction: Direction::ExactIn,
                        amount: probe.amount.clone(),
                        dry_run: probe.dry_run,
                        inputs: probe.inputs.clone(),
                    })
                }),
                terms: tuple.terms.clone(),
                in_session: false,
            });
        }
        segments
    }
}

/// Protocol versions other than `ours`, each with how it differs: a higher major, a higher patch, and a lower
/// version where there is one.
fn other_protocol_versions(ours: &str) -> Vec<(&'static str, String)> {
    let parts: Vec<u64> = ours.split('.').filter_map(|p| p.parse().ok()).collect();
    let [major, minor, patch] = parts[..] else {
        return vec![("under another protocol version", "999.0.0".to_owned())];
    };
    let mut out = vec![
        (
            "under a higher major protocol version",
            format!("{}.0.0", major + 1),
        ),
        (
            "under a higher patch protocol version",
            format!("{major}.{minor}.{}", patch + 1),
        ),
    ];
    let lower = if patch > 0 {
        Some(format!("{major}.{minor}.{}", patch - 1))
    } else if minor > 0 {
        Some(format!("{major}.{}.0", minor - 1))
    } else if major > 0 {
        Some(format!("{}.0.0", major - 1))
    } else {
        None
    };
    if let Some(lower) = lower {
        out.push(("under a lower protocol version", lower));
    }
    out
}

/// A reversal operation with its own transaction.
fn reversal_operation(
    provider: &str,
    transaction: String,
    reverses: &str,
    node: &str,
    asset: &str,
    amount: &str,
) -> Operation {
    Operation::Reversal(Reversal {
        transaction,
        provider: provider.to_owned(),
        reverses: reverses.to_owned(),
        node: node.to_owned(),
        asset: asset.to_owned(),
        amount: amount.to_owned(),
    })
}

/// The inputs a tuple declares, each with a placeholder value.
fn inputs_for(terms: &Terms) -> Inputs {
    terms
        .required_inputs
        .iter()
        .map(|name| (name.clone(), format!("conformance-{name}")))
        .collect()
}

/// The lowest minimum and highest maximum across a tuple's bands.
fn band_edges(terms: &Terms) -> Option<(u128, u128)> {
    let min = terms
        .bands
        .iter()
        .filter_map(|b| b.min.parse::<u128>().ok())
        .min()?;
    let max = terms
        .bands
        .iter()
        .filter_map(|b| b.max.parse::<u128>().ok())
        .max()?;
    Some((min.max(1), max))
}

/// A non-negative decimal string, such as an amount or a rate, as digits and a scale.
fn decimal(text: &str) -> Option<(u128, u32)> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let digits: u128 = format!("{whole}{fraction}").parse().ok()?;
    Some((digits, u32::try_from(fraction.len()).ok()?))
}

/// Whether `actual` falls short of `quoted` by more than `bps` basis points. Values too large to compare count
/// as short, so the check fails closed.
fn short_by_more_than(actual: (u128, u32), quoted: (u128, u32), bps: u16) -> bool {
    let scale = actual.1.max(quoted.1);
    let widen = |(digits, own): (u128, u32)| {
        10u128
            .checked_pow(scale - own)
            .and_then(|f| digits.checked_mul(f))
    };
    let (Some(actual), Some(quoted)) = (widen(actual), widen(quoted)) else {
        return true;
    };
    match (
        actual.checked_mul(10_000),
        quoted.checked_mul(u128::from(10_000 - bps.min(10_000))),
    ) {
        (Some(actual), Some(floor)) => actual < floor,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{decimal, is_protocol_version, other_protocol_versions, short_by_more_than};
    use rutent_contracts::provider_role::VERSION;

    // verifies: REQ-QTE-1
    #[test]
    fn quote_tolerance_follows_the_basis_points() {
        let d = |s| decimal(s).expect("a decimal");
        assert!(!short_by_more_than(d("1"), d("1"), 0));
        assert!(short_by_more_than(d("0.9999"), d("1"), 0));
        assert!(!short_by_more_than(d("0.995"), d("1"), 50));
        assert!(short_by_more_than(d("0.98"), d("1"), 50));
        assert!(!short_by_more_than(d("1.2"), d("1"), 0));
        assert_eq!(decimal("1e3"), None);
        assert_eq!(decimal(".5"), None);
    }

    // verifies: REQ-PRT-1
    #[test]
    fn the_other_protocol_versions_are_valid_and_differ_from_the_current_one() {
        assert_eq!(
            VERSION, "3.1.0",
            "re-pin these values when the protocol version moves"
        );
        let others = other_protocol_versions(VERSION);
        let versions: Vec<&str> = others.iter().map(|(_, v)| v.as_str()).collect();
        assert_eq!(versions, ["4.0.0", "3.1.1", "3.0.0"]);
        for version in versions {
            assert!(
                is_protocol_version(version),
                "{version} is a protocol version"
            );
            assert_ne!(version, VERSION);
        }
    }
}

//! Whether a statement completes a reversal (`REQ-ORD-3`, ADR-0008): judged from its bytes as received, in this
//! order, and the first rule it breaks is why it doesn't.
//!
//! 1. **Attributable**: its signature verifies under a `statements` key its maker listed (`Catalogue::verify_statement`),
//!    or, for a module the operator publishes (a public node's holding account, which advertises nothing), under a
//!    statement key its accepted release names in the log (`T-020-57`); and it is a result in its envelope.
//! 2. **Not the failed provider's own word**, unless the failed provider is the origin reserve, the reserve at the
//!    node the value is back at, whose own statement counts, labelled its word.
//! 3. **The origin reserve's**: ADR-0008 takes the origin node's own reserve as the party other than the failed
//!    provider; any other maker's statement is kept and completes nothing.
//! 4. **Not its publisher's**: a maker the catalogue names no publisher for is refused, and so is the origin reserve
//!    published by the failed provider's publisher, that publisher's word about its own provider's work (`THR-13`,
//!    `THR-124`). Inside the publisher's own private node its word counts, labelled, but the log takes the label only
//!    on a statement, the failed provider's own answer when asked to reverse ([`judge_reversal`]).
//! 5. **This question's, about this credit**: the reserve's confirmation answers Core's question, names the reversed
//!    segment's transaction, and says the credit arrived, signed by the reserve itself, in full (the amount the
//!    segment took in) and in the asset it left in, the confirmation inside of its own shape. No credit, a refusal,
//!    another segment, another asset or a short or larger credit completes nothing.
//!
//! What counts is graded: the reserve's own word about its own segment as a statement (4), its signed confirmation of
//! another provider's reversal as signed by its maker (3); the evidence says whose it is.

use rutent_contracts::provider_role::PublicKey;
use rutent_contracts::provider_role::{
    EvidenceGrade, RESULT_SHAPE, REVERSAL_CONFIRMATION_SHAPE, ResultBody,
    ReversalConfirmationOutcome, Statement,
};
use rutent_contracts::provider_role::{ReversalOutcome, ReversalResult};
use rutent_contracts::router_api::Id;
use rutent_prov::catalogue::{Catalogue, HoldingSource, Instant, Publisher, verify_envelope};

use super::Word;
use crate::segments::Began;

/// Why a statement doesn't complete a reversal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotCounted {
    /// Its signature doesn't verify under a key its maker published for statements: it is no one's.
    Unattributable,
    /// It isn't a result Core can read as a reversal's confirmation.
    Unreadable,
    /// It is the failed provider's own word about its own reversal (`REQ-ORD-3`).
    FailedProvidersOwn,
    /// Its maker isn't the origin reserve, the one party ADR-0008 takes as confirming a reversal.
    NotTheOriginReserve,
    /// Its maker shares the failed provider's publisher (`REQ-ORD-3`), outside that publisher's own private node.
    SamePublisher,
    /// The catalogue names no publisher for its maker.
    NoPublisher,
    /// It answers another question than Core's.
    AnotherQuestion,
    /// The reserve says no credit matching the segment, amount and asset has arrived.
    NotConfirmed,
    /// The reserve refused the question.
    Refused,
    /// The confirmation inside it isn't the reserve's own signed statement.
    NotItsOwn,
    /// It names another segment's transaction.
    AnotherSegment,
    /// The credit is in another asset than the one the segment took in.
    AnotherAsset,
    /// The credit isn't what the segment took in, in full: `stated` arrived.
    NotInFull {
        /// What the confirmation says arrived.
        stated: String,
    },
    /// Nothing came back to Core's question.
    Unanswered,
    /// It would count, but it answers no question of Core's: the log verifies only an answer under the transaction
    /// Core asked it under, so Core asks the origin reserve ([`super::Reversals::confirm`]).
    Unasked,
}

/// The reversed segment, as Core's records hold it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Reversed<'a> {
    /// The segment's provider: the one that failed, or, for an undone start, the origin reserve.
    pub provider: &'a Id,
    /// The inner node the segment starts at, if it starts at one: where the value is to be back.
    pub node: Option<&'a str>,
    /// The party that confirms the reversal: the origin reserve, the provider of the part's withdrawal, at the node
    /// its withdrawal took the value to; the node's holding account at a later inner node; none where no party
    /// confirms, and then nothing counts.
    pub origin: Option<&'a Id>,
    /// The transaction the segment ran under.
    pub transaction: &'a Id,
    /// The asset it took in.
    pub asset: &'a str,
    /// What it took in.
    pub amount: &'a str,
}

/// Whose statement completes a reversal, and at what grade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Whose {
    pub maker: Id,
    pub grade: EvidenceGrade,
    pub word: Word,
    /// What the statement says came back: what is landed again where the value is back at a held node (`T-020-57`).
    pub back: String,
}

/// The statement keys the log names for each module the operator publishes, by module: the keys its accepted
/// releases name in force (core-internal's `release_statement_keys`). Such a module, a public node's holding account,
/// advertises nothing, so its statements are attributed by these, as the log attributes them (`T-020-57`).
pub(super) type ReleaseKeys = std::collections::BTreeMap<String, Vec<PublicKey>>;

/// [`Catalogue::verify_statement`] by the versions in force when the segment began, where Core holds it, else by the
/// catalogue's table at `at` (`T-020-54`).
fn verify(
    catalogue: &Catalogue,
    received: &[u8],
    at: Instant,
    began: Option<&Began>,
) -> Result<(), rutent_prov::catalogue::StatementRefusal> {
    match began {
        Some(b) => catalogue.verify_statement_under(received, at, &b.versions, b.at),
        None => catalogue.verify_statement(received, at),
    }
}

/// Whether `received` is attributable to its maker: its signature verifies under a `statements` key its maker
/// listed in its advertisement (`Catalogue::verify_statement`), or, for a module the operator publishes, which
/// advertises nothing, under a statement key one of its accepted releases names in the log, in the catalogue's
/// deployment and protocol version (`T-020-57`).
fn attributable(
    catalogue: &Catalogue,
    keys: &ReleaseKeys,
    received: &[u8],
    at: Instant,
    began: Option<&Began>,
) -> bool {
    if verify(catalogue, received, at, began).is_ok() {
        return true;
    }
    let Ok(envelope) = serde_json::from_slice::<serde_json::Value>(received) else {
        return false;
    };
    let text = |path: &[&str]| {
        path.iter()
            .try_fold(&envelope, |v, k| v.get(*k))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let (Some(maker), Some(key), Some(value), Some((deployment, _)), Some(versions)) = (
        text(&["maker"]),
        text(&["signature", "key"]).and_then(PublicKey::new),
        text(&["signature", "value"]),
        catalogue.context(),
        // The versions in force as the segment began under them, where Core holds the segment (`T-020-54`).
        began
            .map(|b| b.versions.clone())
            .or_else(|| catalogue.protocol_versions()),
    ) else {
        return false;
    };
    let at = began.map_or(at, |b| b.at);
    // The protocol versions in force, each older one until its cutover, judged at the caller's clock (`REQ-PRT-4`).
    let in_context = serde_json::to_value(deployment).ok().as_ref() == envelope.get("deployment")
        && at.rfc3339().is_some_and(|now| {
            text(&["protocol_version"]).is_some_and(|sent| versions.in_force(&sent, &now).is_ok())
        });
    in_context
        && catalogue.publishers().published_by_operator(&maker)
        && keys.get(&maker).is_some_and(|listed| listed.contains(&key))
        && verify_envelope(&key, &value, received).is_ok()
}

/// Judges `received` about `reversed`, as the answer to Core's question under `question` where Core asked one.
pub(super) fn judge(
    catalogue: &Catalogue,
    keys: &ReleaseKeys,
    at: Instant,
    began: Option<&Began>,
    reversed: &Reversed<'_>,
    question: Option<&Id>,
    received: &[u8],
) -> Result<Whose, NotCounted> {
    if !attributable(catalogue, keys, received, at, began) {
        return Err(NotCounted::Unattributable);
    }
    let result: Statement<ResultBody> =
        serde_json::from_slice(received).map_err(|_| NotCounted::Unreadable)?;
    if result.shape != RESULT_SHAPE {
        return Err(NotCounted::Unreadable);
    }
    let maker = Id::new(result.maker.as_str()).ok_or(NotCounted::Unreadable)?;
    let own = maker == *reversed.provider;
    if own && reversed.origin != Some(&maker) {
        return Err(NotCounted::FailedProvidersOwn);
    }
    if reversed.origin != Some(&maker) {
        return Err(NotCounted::NotTheOriginReserve);
    }
    let publishers = catalogue.publishers();
    let (theirs, failed) = (
        publishers.publisher(maker.as_str()),
        publishers.publisher(reversed.provider.as_str()),
    );
    let Some(theirs) = theirs else {
        return Err(NotCounted::NoPublisher);
    };
    // Of the failed provider's publisher, the reserve's word is that publisher's own about its own provider's work.
    // Inside the publisher's own private node the publisher's word counts, labelled, but the log takes the label only
    // on a statement graded a statement, which a reserve's signed confirmation isn't: the publisher's word there is
    // the failed provider's own answer when Core asks it to reverse ([`judge_reversal`]).
    if !own && failed.as_ref() == Some(&theirs) {
        return Err(NotCounted::SamePublisher);
    }
    let word = Word::Reserves;
    let ResultBody::ReversalConfirmation(answer) = &result.statement else {
        return Err(NotCounted::Unreadable);
    };
    let confirmed = match &answer.outcome {
        ReversalConfirmationOutcome::Confirmed(confirmed) => &confirmed.confirmation,
        ReversalConfirmationOutcome::NotConfirmed(_) => return Err(NotCounted::NotConfirmed),
        ReversalConfirmationOutcome::Refused(_) => return Err(NotCounted::Refused),
    };
    if question.is_some_and(|q| q.as_str() != answer.transaction) {
        return Err(NotCounted::AnotherQuestion);
    }
    let inner = serde_json::to_vec(&**confirmed).map_err(|_| NotCounted::Unreadable)?;
    if confirmed.maker != result.maker
        || confirmed.shape != REVERSAL_CONFIRMATION_SHAPE
        || !attributable(catalogue, keys, &inner, at, began)
    {
        return Err(NotCounted::NotItsOwn);
    }
    let credit = &confirmed.statement;
    if credit.segment != reversed.transaction.as_str() {
        return Err(NotCounted::AnotherSegment);
    }
    if credit.asset != reversed.asset {
        return Err(NotCounted::AnotherAsset);
    }
    if credit.amount != reversed.amount {
        return Err(NotCounted::NotInFull {
            stated: credit.amount.clone(),
        });
    }
    if question.is_none() {
        return Err(NotCounted::Unasked);
    }
    let grade = if own {
        EvidenceGrade::Statement
    } else {
        EvidenceGrade::SignedByProvider
    };
    Ok(Whose {
        maker,
        grade,
        word,
        back: credit.amount.clone(),
    })
}

/// Judges `received`, the failed provider's answer to Core's reversal under `question`, about `reversed`: it counts
/// only inside a private node, the segment's start, that the provider's own publisher holds (it gave the node's
/// holding account, `register_private_holding`) and that the log binds as that publisher's (`bound`, read from the
/// log, [`super::Reversals::follow`]): there its word is its publisher's, labelled, graded a statement
/// (`REQ-ORD-3`'s third sentence). Anywhere else it is the failed provider's own word, which completes nothing. It
/// must say the value is back at that node, in full and in the asset it left in.
pub(super) fn judge_reversal(
    catalogue: &Catalogue,
    at: Instant,
    began: Option<&Began>,
    reversed: &Reversed<'_>,
    question: &Id,
    bound: Option<&str>,
    received: &[u8],
) -> Result<Whose, NotCounted> {
    verify(catalogue, received, at, began).map_err(|_| NotCounted::Unattributable)?;
    let result: Statement<ResultBody> =
        serde_json::from_slice(received).map_err(|_| NotCounted::Unreadable)?;
    if result.shape != RESULT_SHAPE || result.maker != reversed.provider.as_str() {
        return Err(NotCounted::Unreadable);
    }
    let ResultBody::Reversal(ReversalResult {
        transaction,
        outcome,
    }) = &result.statement
    else {
        return Err(NotCounted::Unreadable);
    };
    if transaction != question.as_str() {
        return Err(NotCounted::AnotherQuestion);
    }
    let back = match outcome {
        ReversalOutcome::Succeeded(back) => back,
        ReversalOutcome::Refused(_) => return Err(NotCounted::Refused),
        ReversalOutcome::Unknown(_) => return Err(NotCounted::NotConfirmed),
    };
    if Some(back.node.as_str()) != reversed.node {
        return Err(NotCounted::AnotherSegment);
    }
    if back.asset != reversed.asset {
        return Err(NotCounted::AnotherAsset);
    }
    if back.amount != reversed.amount {
        return Err(NotCounted::NotInFull {
            stated: back.amount.clone(),
        });
    }
    let publisher = match catalogue.publishers().publisher(reversed.provider.as_str()) {
        Some(Publisher::Party(publisher)) => publisher,
        _ => return Err(NotCounted::FailedProvidersOwn),
    };
    let holds = reversed
        .node
        .and_then(|node| catalogue.nodes().holding(node))
        .is_some_and(|h| matches!(&h.source, HoldingSource::Publisher(p) if *p == publisher));
    if !holds || bound != Some(publisher.as_str()) {
        return Err(NotCounted::FailedProvidersOwn);
    }
    Ok(Whose {
        maker: reversed.provider.clone(),
        grade: EvidenceGrade::Statement,
        word: Word::Publishers(Id::new(publisher.as_str()).ok_or(NotCounted::NoPublisher)?),
        back: back.amount.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rutent_contracts::provider_role::{Deployment, RESULT_SHAPE, VERSION, signing_input};

    fn base64url(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
            for i in 0..=chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
        }
        out
    }

    /// A statement of the operator's own module `wallet`, under protocol `version`, signed with `key`.
    fn statement(deployment: &Deployment, version: &str, key: &SigningKey) -> Vec<u8> {
        let public = base64url(key.verifying_key().as_bytes());
        let mut value = serde_json::json!({
            "deployment": deployment,
            "protocol_version": version,
            "shape": RESULT_SHAPE,
            "maker": "wallet",
            "made_at": "2026-11-10T00:00:00Z",
            "signature": {"scheme": "ed25519", "key": public, "value": base64url(&[0u8; 64])},
            "statement": {}
        });
        let input = signing_input(&serde_json::to_vec(&value).unwrap()).unwrap();
        value["signature"]["value"] = base64url(&key.sign(&input).to_bytes()).into();
        serde_json::to_vec(&value).unwrap()
    }

    /// A statement of a module the operator publishes is attributed by the protocol versions in force: under an older
    /// version before its cutover, and not from it, judged at the caller's clock (`REQ-PRT-4`, `THR-64`, `T-020-54`);
    /// the current version always, and a version no cutover names never.
    // verifies: REQ-PRT-4, THR-64
    #[test]
    fn operator_modules_statement_under_an_older_version_is_attributed_only_before_its_cutover() {
        let key = SigningKey::from_bytes(&[5u8; 32]);
        let operator = SigningKey::from_bytes(&[6u8; 32]);
        let deployment = Deployment {
            name: "rutent-test".to_owned(),
            operator_key: PublicKey::new(base64url(operator.verifying_key().as_bytes())).unwrap(),
        };
        let mut catalogue = Catalogue::new();
        catalogue.set_context(deployment.clone(), VERSION);
        catalogue.publish_operator_module("wallet").unwrap();
        let keys: ReleaseKeys = [(
            "wallet".to_owned(),
            vec![PublicKey::new(base64url(key.verifying_key().as_bytes())).unwrap()],
        )]
        .into();
        let old = statement(&deployment, "3.0.0", &key);
        let current = statement(&deployment, VERSION, &key);
        let (before, after) = (Instant(4_999), Instant(5_000));
        assert!(
            !attributable(&catalogue, &keys, &old, before, None),
            "no cutover names it"
        );
        catalogue.set_cutover("3.0.0", &Instant(5_000).rfc3339().unwrap());
        assert!(attributable(&catalogue, &keys, &old, before, None));
        assert!(!attributable(&catalogue, &keys, &old, after, None));
        assert!(attributable(&catalogue, &keys, &current, after, None));
    }

    /// The confirmation judge reads the version the segment began under, not the date its answer lands on
    /// (`REQ-PRT-2`, `REQ-PRT-4`, `T-020-54`): a holdout's statement landing after the cutover, for a segment begun before
    /// it, passes the attribution (and fails later, for being no result, which is how the test tells), and one for a
    /// segment begun after it is unattributable. The failed provider's answer (`judge_reversal`) is the assembly's test.
    // verifies: REQ-PRT-2, REQ-PRT-4
    #[test]
    fn confirmation_judge_reads_the_version_its_segment_began_under() {
        let key = SigningKey::from_bytes(&[5u8; 32]);
        let operator = SigningKey::from_bytes(&[6u8; 32]);
        let deployment = Deployment {
            name: "rutent-test".to_owned(),
            operator_key: PublicKey::new(base64url(operator.verifying_key().as_bytes())).unwrap(),
        };
        let mut catalogue = Catalogue::new();
        catalogue.set_context(deployment.clone(), VERSION);
        catalogue.publish_operator_module("wallet").unwrap();
        catalogue.set_cutover("3.0.0", &Instant(5_000).rfc3339().unwrap());
        let keys: ReleaseKeys = [(
            "wallet".to_owned(),
            vec![PublicKey::new(base64url(key.verifying_key().as_bytes())).unwrap()],
        )]
        .into();
        let old = statement(&deployment, "3.0.0", &key);
        let (provider, origin, transaction) = (
            Id::new("wallet").unwrap(),
            Id::new("wallet").unwrap(),
            Id::new("tx-1").unwrap(),
        );
        let reversed = Reversed {
            provider: &provider,
            node: None,
            origin: Some(&origin),
            transaction: &transaction,
            asset: "COP",
            amount: "1",
        };
        let versions = catalogue.protocol_versions().unwrap();
        let before = Began {
            at: Instant(4_000),
            versions: versions.clone(),
        };
        let after = Began {
            at: Instant(5_000),
            versions,
        };
        let lands = Instant(9_000);
        let judged =
            |began: Option<&Began>| judge(&catalogue, &keys, lands, began, &reversed, None, &old);
        assert_eq!(judged(Some(&before)), Err(NotCounted::Unreadable));
        assert_eq!(judged(Some(&after)), Err(NotCounted::Unattributable));
        assert_eq!(judged(None), Err(NotCounted::Unattributable));
    }
}

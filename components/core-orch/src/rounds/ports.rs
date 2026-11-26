//! What the rounds reach outside Core's own records: the operator's key, the providers' modules, and a source of
//! fresh identifiers. The assembly gives each; the tests give fakes.

use rutent_contracts::provider_role::PublicKey;
use rutent_contracts::router_api::Id;

/// The deployment's operator key, which signs what Core states itself: each question it asks a provider and each
/// recorded decision (router-api's `RecordedDecision`). Core holds no private key; the assembly holds this.
pub trait OperatorKey {
    /// The public half: the deployment's `operator_key`.
    fn key(&self) -> PublicKey;

    /// Signs `input`, provider-role's signing input of a statement Core makes (`signing_input`), and answers the
    /// 64-byte Ed25519 signature, or `None` where it can't sign now. Nothing is asked or recorded unsigned.
    fn sign(&mut self, input: &[u8]) -> Option<[u8; 64]>;
}

/// The providers' modules, as Core asks them one question each. The assembly hands the question to the platform
/// (`Platform::handle`) and the answer's bytes back.
pub trait Providers {
    /// Hands `question`, a provider-role operation in its envelope signed with the operator key, exactly as signed,
    /// to `provider`, and answers what came back, as received: `None` where nothing did.
    fn ask(&mut self, provider: &Id, question: &[u8]) -> Option<Vec<u8>>;
}

/// A source of identifiers no one can read anything from: each transaction a question is asked under, and each
/// request, snapshot and charge id the rounds write. An identifier carries no order reference and no count a
/// provider could line up with another question's (`REQ-CAT-5`); the assembly draws them at random.
pub trait Fresh {
    /// An identifier never drawn before.
    fn id(&mut self) -> Id;
}

//! The operator's own signed statements (`T-016-04`; `REQ-MDL-1`, `REQ-CAT-23`): the release of a module the
//! operator publishes, such as a public node's holding account, which no provider advertisement vouches for.
//!
//! The key such a statement is checked against is the `operator_key` of the deployment the catalogue serves, the
//! one its deployment settings give it ([`Catalogue::set_context`]), never one a statement or an advertisement
//! names. This is signature checking for module admission, a `DEC-20` path.

use rutent_contracts::provider_role::{Deployment, Statement};

use super::advertisements::StatementRefusal;
use super::signature::verify_envelope;
use super::time::Stamp;
use super::{Catalogue, Instant, PublisherError};

impl Catalogue {
    /// Records `provider` as a module the operator publishes, such as a public node's holding account
    /// (`REQ-CAT-23`): from then on the catalogue names the operator as its publisher, admission takes its releases
    /// only on the operator's own signature, and no advertisement may name it. A repeat changes nothing; a provider
    /// a publisher's advertisement stands behind is refused (`REQ-MDL-1`).
    pub fn publish_operator_module(&mut self, provider: &str) -> Result<(), PublisherError> {
        self.publishers.register_operator(provider)
    }

    /// The deployment and protocol version the catalogue serves, as its settings gave them, if they did.
    pub fn context(&self) -> Option<(&Deployment, &str)> {
        self.context
            .as_ref()
            .map(|(deployment, version)| (deployment, version.as_str()))
    }

    /// Verifies a statement the operator made, given as received. It must name the catalogue's deployment and
    /// protocol version (`THR-8`), be signed with that deployment's `operator_key`, the one the catalogue's settings
    /// give, and not be dated more than the skew ahead of the caller's clock `at`. Then the signature must verify,
    /// strictly, over the statement's signing input (`REQ-PRT-1`).
    pub fn verify_operator_statement(
        &self,
        received: &[u8],
        at: Instant,
    ) -> Result<(), StatementRefusal> {
        let (deployment, protocol_version) = self.context().ok_or(StatementRefusal::NoContext)?;
        let statement: Statement<serde_json::Value> = serde_json::from_slice(received)
            .map_err(|e| StatementRefusal::Unreadable(e.to_string()))?;
        if &statement.deployment != deployment || statement.protocol_version != protocol_version {
            return Err(StatementRefusal::WrongContext);
        }
        if statement.signature.key != deployment.operator_key {
            return Err(StatementRefusal::NotSignedByOperator);
        }
        let made_at = Stamp::parse(&statement.made_at).ok_or(StatementRefusal::BadTime)?;
        if made_at.ceil_secs() > at.0.saturating_add(self.clock_skew()) {
            return Err(StatementRefusal::DatedTooFarAhead);
        }
        verify_envelope(
            &deployment.operator_key,
            &statement.signature.value,
            received,
        )
        .map_err(StatementRefusal::BadSignature)
    }
}

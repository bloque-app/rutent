//! The operator's channel as the assembly builds it, and what a scenario reads off it (`REQ-QTE-5`, `AC-14`;
//! `T-020-14`).
//!
//! Core's segments check each settled segment's charge themselves (`rutent_core_orch::segments`), and raise one
//! above its formula through the channel the assembly hands them: the notices' own ([`rutent_core_orch::notice::Channel`]), which seals
//! the raise for the operator alone, signs it with a notice key and sends it on a transport. The assembly's
//! transport takes every notice and keeps what it took, so a scenario reads the operator's inbox as the operator
//! would: the sealed notices, checked and taken by the reference receiver ([`rutent_core_orch::notice::Inbox`]).
//! This module holds no check of its own.

use ed25519_dalek::Signer as _;
use rutent_contracts::core_internal::{Entry, Raise, Receipt, VerifiedEntry, What};
use rutent_contracts::provider_role::PublicKey;
use rutent_contracts::router_api::Id;
use rutent_core_orch::notice::{About, Notice, NoticeKey, Receiver, Transport};
use rutent_core_orch::quote::{Checked, Recorded, TakeError, Taken, TermsDesk};
use serde_json::Value;

use super::Core;

/// The deployment's key for notices in the assembly: a key of its own, apart from the operator's.
pub struct NoticeSigner(pub ed25519_dalek::SigningKey);

impl NoticeSigner {
    /// The key the assembly's notices are signed with.
    pub fn assembly() -> Self {
        Self(rutent_conformance::signing::signing_key(
            "the notices of rutent-scenarios",
            0,
        ))
    }

    /// Its public half, which the operator's inbox checks a notice against.
    pub fn public(&self) -> PublicKey {
        rutent_conformance::signing::public_key(&self.0)
    }
}

impl NoticeKey for NoticeSigner {
    fn sign(&mut self, input: &[u8]) -> Option<[u8; 64]> {
        Some(self.0.sign(input).to_bytes())
    }
}

/// The assembly's transport: it takes every notice, and keeps each one with its receiver, in order.
#[derive(Debug, Clone, Default)]
pub struct Wire {
    /// Every notice sent, sealed.
    pub sent: Vec<(Receiver, Vec<u8>)>,
}

impl Transport for Wire {
    fn send(&mut self, to: &Receiver, notice: &[u8]) -> bool {
        self.sent.push((to.clone(), notice.to_vec()));
        true
    }
}

/// The log's recorded entries, read by receipt.
struct Log<'a>(&'a [(Receipt, Entry)]);

impl Recorded for Log<'_> {
    fn entry(&self, receipt: &Receipt) -> Option<Entry> {
        let place = usize::try_from(u64::from(receipt.index)).ok()?;
        self.0
            .get(place)
            .filter(|(r, _)| r == receipt)
            .map(|(_, e)| e.clone())
    }
}

impl Core {
    /// The notices sent to the operator, sealed, in order.
    pub fn operator_notices(&self) -> Vec<Vec<u8>> {
        self.wire
            .sent
            .iter()
            .filter(|(to, _)| to == self.notices.operator())
            .map(|(_, sealed)| sealed.clone())
            .collect()
    }

    /// Every raise the operator was sent, in order, as its inbox would take it: only a notice whose signature the
    /// notice key's verifies, addressed to the operator, is read.
    pub fn raised(&self) -> Vec<Raise> {
        self.operator_notices()
            .iter()
            .filter_map(|sealed| notice_of(sealed))
            .filter_map(|notice| match notice.about {
                About::Raise { raise } => Some(raise),
                _ => None,
            })
            .collect()
    }

    /// The public half of the key notices are signed with.
    pub fn notice_key(&self) -> PublicKey {
        NoticeSigner::assembly().public()
    }

    /// What the check of `segment` of `order` came to, once Core made and recorded it.
    pub fn charge_checked(&self, order: &Id, segment: &Id) -> Option<Checked> {
        self.segments.charges().result(order, segment)
    }

    /// Core's register of checked charges lost and rebuilt from the log, as a restart that kept the segments would
    /// leave it (`Charges::recorded`): a raise the log holds isn't raised again.
    pub fn charges_rebuilt_from_log(&mut self) {
        let entries = self.log.borrow().entries().to_vec();
        self.segments.recorded_charges(&entries);
    }

    /// From now on, the log's store turns away the next `times` appends of a record whose `call` is `call`: a log that
    /// can't take them now, which a caller tries again.
    pub fn refuse_log_appends(&self, call: &'static str, times: usize) {
        self.disk.refuse(call, times);
    }

    /// The segments of `order` whose settled charge Core noted for its check, in the plan's order.
    pub fn charged_segments(&self, order: &Id) -> Vec<Id> {
        let Some(started) = self.started.get(order.as_str()) else {
            return Vec::new();
        };
        started
            .plan
            .iter()
            .flat_map(|part| part.segments.iter())
            .map(|s| super::kit::id(s.segment_id.as_str()))
            .filter(|segment| self.segments.charged(order, segment).is_some())
            .collect()
    }

    /// The operations of `order`'s chosen route, in order.
    pub fn route_operations(&self, order: &Id) -> Vec<rutent_contracts::router_api::Operation> {
        let Some(started) = self.started.get(order.as_str()) else {
            return Vec::new();
        };
        started
            .record
            .chosen()
            .map(|c| {
                c.parts
                    .iter()
                    .flat_map(|p| p.operations.iter().cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The private half of the key the simulated `provider`'s module signs its statements with: the key its release
    /// declares, which the world holds, so a scenario can state terms as the provider would sign them.
    pub fn statements_signing_key(&self, provider: &str) -> Option<ed25519_dalek::SigningKey> {
        use rutent_conformance::role::RoleProvider as _;
        let installed = self
            .installed
            .iter()
            .find(|i| i.spec.provider == provider)?;
        let placeholders = crate::sim::fleet::InProcess(installed.spec.make(&super::context()))
            .advertisement()
            .into_statement()
            .statement;
        let key = placeholders
            .keys
            .iter()
            .find(|k| k.purpose == rutent_contracts::provider_role::KeyPurpose::Statements)?;
        Some(rutent_conformance::signing::real(key.public_key.as_str()))
    }

    /// How many terms Core holds, across tuples.
    pub fn terms_held(&self) -> usize {
        self.segments.terms().len()
    }

    /// Takes a provider's signed terms, `bytes` as they arrived, for the segments that start after them
    /// (`REQ-QTE-6`). Nothing delivers a provider's terms to Core yet; a scenario hands them here.
    ///
    /// # Errors
    ///
    /// If the bytes aren't terms, the log refused them, or what the evidence store kept isn't what was handed in.
    pub fn state_terms(&mut self, bytes: &[u8]) -> Result<Taken, TakeError> {
        self.take_terms_with(None, bytes)
    }

    /// [`Core::state_terms`], with the statement behind the entry's commitment read through `kept`: how a scenario
    /// has the evidence store keep something else than the bytes it was handed.
    ///
    /// # Errors
    ///
    /// As [`Core::state_terms`].
    pub fn state_terms_read_by(
        &mut self,
        kept: &mut dyn rutent_contracts::core_internal::calls::KeptStatements,
        bytes: &[u8],
    ) -> Result<Taken, TakeError> {
        self.take_terms_with(Some(kept), bytes)
    }

    fn take_terms_with(
        &mut self,
        kept: Option<&mut dyn rutent_contracts::core_internal::calls::KeptStatements>,
        bytes: &[u8],
    ) -> Result<Taken, TakeError> {
        let kept: &mut dyn rutent_contracts::core_internal::calls::KeptStatements = match kept {
            Some(kept) => kept,
            None => &mut self.kept,
        };
        // The log's own record of the entry the call appended is read back from the log as it stands after.
        struct Now<'a>(&'a std::cell::RefCell<super::CoreLog>);
        impl Recorded for Now<'_> {
            fn entry(&self, receipt: &Receipt) -> Option<Entry> {
                Log(self.0.borrow().entries()).entry(receipt)
            }
        }
        let log = std::rc::Rc::clone(&self.log);
        let recorded = Now(&log);
        let desk = TermsDesk {
            evidence: &mut self.keeper,
            log: &mut self.general,
            kept,
            recorded: &recorded,
            clock: &self.clock,
            catalogue: &self.catalogue,
        };
        self.segments.take_terms(desk, bytes)
    }
}

/// A sealed notice, read without checking its signature: the operator's inbox does that.
fn notice_of(sealed: &[u8]) -> Option<Notice> {
    let value: Value = serde_json::from_slice(sealed).ok()?;
    serde_json::from_value(value.get("notice")?.clone()).ok()
}

/// The raises the log holds, by their entries.
pub fn raises_in(entries: &[(Receipt, Entry)]) -> Vec<Raise> {
    entries
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(VerifiedEntry {
                what: What::Raise { raise },
                ..
            }) => Some(raise.clone()),
            _ => None,
        })
        .collect()
}

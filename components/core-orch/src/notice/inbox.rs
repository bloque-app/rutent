//! What a receiver does with a notice: the reference receiver, the one `INTEGRATORS.md`'s advice describes and the
//! tests hold the notices to (`REQ-ORD-20`, `THR-103`).
//!
//! A notice is taken only when its signature is the deployment's notice key's over what it says, it comes from this
//! deployment, and it is addressed to this receiver: its party and the role it receives in. An order's notice
//! carries the order's whole state after its change, at its sequence, so the receiver keeps the latest sequence it
//! has and takes a notice only if it is later: a repeated or reordered one changes nothing, and the view is always
//! the latest state delivered. A missing sequence shows as a gap ([`Inbox::missing`]), which only asking for the
//! current state recovers (`super::Notices::current`). The operator's events (raises, charges counted ahead) carry
//! no sequence: each is kept once, by what it is.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::provider_role::PublicKey;
use rutent_contracts::router_api::Id;

use super::wire::{About, Receiver, Sealed, View, signing_input, unhex};

/// A signature check: whether `signature` is the Ed25519 signature of `input` under `key`.
pub trait Verifier {
    /// Whether it verifies.
    fn verifies(&self, key: &PublicKey, input: &[u8], signature: &[u8; 64]) -> bool;
}

/// Why a notice wasn't taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotTaken {
    /// It isn't a sealed notice.
    Malformed,
    /// Its signature isn't the deployment's notice key's over what it says.
    Forged,
    /// It comes from another deployment.
    OtherDeployment,
    /// It is addressed to another receiver.
    ForAnother,
}

/// What taking a notice did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    /// A later state of its order: the view is now this one.
    Later,
    /// An earlier or the same sequence of its order: the view stays as it was.
    Earlier,
    /// An operator's event not held before.
    Event,
    /// An operator's event held already.
    Repeat,
}

/// One order as a receiver holds it.
#[derive(Debug, Clone)]
struct Held {
    latest: u64,
    view: View,
    /// The sequences taken above `covered`.
    sequences: BTreeSet<u64>,
    /// The latest sequence an answer of the current state covered: every change up to it is held.
    covered: u64,
}

/// A receiver's notices: the latest state of each order, and the operator's events.
#[derive(Debug, Clone)]
pub struct Inbox {
    me: Receiver,
    deployment: Id,
    key: PublicKey,
    orders: BTreeMap<String, Held>,
    events: BTreeMap<String, About>,
}

impl Inbox {
    /// `me`'s inbox, for notices of `deployment` signed with its notice key `key`.
    pub fn new(me: Receiver, deployment: Id, key: PublicKey) -> Self {
        Self {
            me,
            deployment,
            key,
            orders: BTreeMap::new(),
            events: BTreeMap::new(),
        }
    }

    /// Takes `bytes`, a sealed notice as received, once it is checked.
    pub fn take(&mut self, verifier: &dyn Verifier, bytes: &[u8]) -> Result<Taken, NotTaken> {
        let sealed: Sealed = serde_json::from_slice(bytes).map_err(|_| NotTaken::Malformed)?;
        let signature = unhex(&sealed.signature).ok_or(NotTaken::Malformed)?;
        let input = signing_input(&sealed.notice).ok_or(NotTaken::Malformed)?;
        if !verifier.verifies(&self.key, &input, &signature) {
            return Err(NotTaken::Forged);
        }
        if sealed.notice.deployment != self.deployment {
            return Err(NotTaken::OtherDeployment);
        }
        if sealed.notice.to != self.me {
            return Err(NotTaken::ForAnother);
        }
        Ok(match sealed.notice.about {
            About::Order {
                order,
                sequence,
                view,
                ..
            } => self.order(order.as_str(), sequence, view, sealed.notice.current),
            about @ (About::Raise { .. } | About::Ahead { .. }) => {
                match self.events.entry(event_key(&about)) {
                    Entry::Occupied(_) => Taken::Repeat,
                    Entry::Vacant(slot) => {
                        slot.insert(about);
                        Taken::Event
                    }
                }
            }
        })
    }

    fn order(&mut self, order: &str, sequence: u64, view: View, current: bool) -> Taken {
        let held = self.orders.entry(order.to_owned()).or_insert_with(|| Held {
            latest: 0,
            view: view.clone(),
            sequences: BTreeSet::new(),
            covered: 0,
        });
        if current {
            held.covered = held.covered.max(sequence);
        }
        let covered = held.covered;
        held.sequences.insert(sequence);
        held.sequences.retain(|s| *s > covered);
        if sequence > held.latest {
            held.latest = sequence;
            held.view = view;
            Taken::Later
        } else {
            Taken::Earlier
        }
    }

    /// The latest state held of `order`, and its sequence.
    pub fn view(&self, order: &Id) -> Option<(u64, &View)> {
        self.orders
            .get(order.as_str())
            .map(|held| (held.latest, &held.view))
    }

    /// Whether a change of `order` below the latest held never arrived, and no answer of the current state since
    /// covers it. It changes nothing: the latest notice already holds the order's whole state. A lost latest notice
    /// shows in no sequence, so a receiver that must be sure before acting asks for the current state (`THR-103`),
    /// whatever this says.
    pub fn missing(&self, order: &Id) -> bool {
        self.orders.get(order.as_str()).is_some_and(|held| {
            (held.sequences.len() as u64) < held.latest.saturating_sub(held.covered)
        })
    }

    /// The operator's events held, each once, in the order of what they are.
    pub fn events(&self) -> impl Iterator<Item = &About> {
        self.events.values()
    }

    /// The orders held.
    pub fn orders(&self) -> impl Iterator<Item = &str> {
        self.orders.keys().map(String::as_str)
    }
}

/// What an operator's event is, so it is held once however often it comes.
pub(super) fn event_key(about: &About) -> String {
    match about {
        About::Raise { raise } => {
            serde_json::to_string(&("raise", raise.raise.as_str())).unwrap_or_default()
        }
        About::Ahead { charge } => {
            serde_json::to_string(&("ahead", charge.party.as_str(), charge.request.as_str()))
                .unwrap_or_default()
        }
        About::Order {
            order, sequence, ..
        } => serde_json::to_string(&("order", order.as_str(), sequence)).unwrap_or_default(),
    }
}

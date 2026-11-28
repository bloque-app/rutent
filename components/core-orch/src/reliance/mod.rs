//! Whose judgement an input rested on, and who relied on it (`REQ-CPL-12`, `T-020-23`).
//!
//! Where Core hands a provider an input that rests on a reserve's judgement, a reserve's yes, its proof that its users
//! may enact a set of transactions (`REQ-CPL-8`) or its answer to a check, Core first appends a `reliance` entry: the
//! order, the receipt of the stated entry holding the judgement, the judge and the provider that relies on it. The
//! record then shows whose judgement it was and who relied on it, so the loss of a false one falls on the relying
//! provider and on no one else.
//!
//! - **Declared, not guessed.** Which of an order's inputs rest on which judgement is the caller's to say
//!   ([`Reliances::rests_on`]): Core reads a name and a value, never where the value came from. The judgement is named
//!   by the receipt of its stated entry, which the caller holds.
//! - **Written by the log's say-so.** Core never decides that a judgement is its judge's: the log does, on append,
//!   refusing a `reliance` whose judgement is no stated entry of the log, isn't verified, was made by another than the
//!   judge named, or is about another order (`reliance-names-its-judges-statement`). A refusal is the answer, and
//!   nothing relying on it is sent (`REQ-AUD-2`).
//! - **Before the operation, once.** [`Reliances::write`] is called before the operation that carries the input is
//!   sent ([`crate::segments::Segments`] does, before a segment's effect). The entry is built once for an order, a
//!   judgement and a relying provider and appended unchanged however often it is tried, so one judgement relied on by
//!   one provider is one entry whatever the retries, and a segment's retry never sends without it (`INV-7`, `INV-8`).
//! - **No loss path.** Nothing here names the operator or a customer as bearing a judgement's loss: the entry names the
//!   judge and the provider that relied on it, and Core writes nothing else about a false judgement.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use rutent_contracts::core_internal::calls::{AppendFailure, GeneralEntry, Log};
use rutent_contracts::core_internal::{Actor, Attribution, Outcome, Receipt, What};
use rutent_contracts::provider_role::{self, JUDGEMENT_SHAPE, Statement};
use rutent_contracts::router_api::{Id, Timestamp};

use crate::segments::journal::{append_general, verified};

/// A judgement an order's input rests on: the receipt of the stated entry holding it, and whose it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judgement {
    /// The receipt of the stated entry holding the judgement.
    pub receipt: Receipt,
    /// Whose judgement it is: a reserve or a verifier. The log holds the statement to this maker.
    pub judge: Id,
}

/// Why an input couldn't be declared as resting on a judgement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rests {
    /// The input rests on another judgement already: one input rests on one judgement.
    Another,
    /// A segment carrying the input has been handed over already: the reliance would come after the send.
    TooLate,
}

/// The reliance entries Core writes, and the judgements each order's inputs rest on.
#[derive(Debug, Clone, Default)]
pub struct Reliances {
    rests: BTreeMap<(String, String), Judgement>,
    entries: BTreeMap<String, Written>,
}

#[derive(Debug, Clone)]
struct Written {
    entry: GeneralEntry,
    receipt: Option<Receipt>,
}

impl Reliances {
    /// Nothing declared, nothing written.
    pub fn new() -> Self {
        Self::default()
    }

    /// The register rebuilt from `history`, the log's own entries, as a Core that reopens reads it (`T-020-46`): each
    /// `reliance` entry Core wrote is held as written, its receipt the log's, so it is never appended again, and each
    /// input it names rests on its judgement again, so a segment after the reopening that carries one writes its own
    /// reliance (`REQ-CPL-12`). A declaration no segment relied on yet left no entry: it is the caller's word, which
    /// the caller gives again.
    pub fn recorded(history: &[(Receipt, rutent_contracts::core_internal::Entry)]) -> Self {
        let mut reliances = Self::new();
        for (receipt, entry) in history {
            let rutent_contracts::core_internal::Entry::Verified(v) = entry else {
                continue;
            };
            let What::Reliance {
                order,
                judgement,
                judge,
                relied_on_by,
                inputs,
            } = &v.what
            else {
                continue;
            };
            let held = Judgement {
                receipt: judgement.clone(),
                judge: judge.clone(),
            };
            for input in inputs {
                let _ = reliances.rests_on(order, input.as_str(), held.clone());
            }
            let key = format!(
                "{}\u{0}{}\u{0}{}",
                order.as_str(),
                relied_on_by.as_str(),
                serde_json::to_string(judgement).unwrap_or_default()
            );
            if let Ok(general) = GeneralEntry::try_from(entry.clone()) {
                reliances.entries.entry(key).or_insert(Written {
                    entry: general,
                    receipt: Some(receipt.clone()),
                });
            }
        }
        reliances
    }

    /// Declares that the order's input `input` rests on `judgement`. Declared again for the same judgement it is a
    /// retry and changes nothing; for another judgement it is refused, not replaced: which judgement a value rests on
    /// is not for a later caller to change under a provider that may have relied on the first (`REQ-CPL-12`).
    pub fn rests_on(&mut self, order: &Id, input: &str, judgement: Judgement) -> Result<(), Rests> {
        match self
            .rests
            .entry((order.as_str().to_owned(), input.to_owned()))
        {
            Entry::Occupied(held) if *held.get() == judgement => Ok(()),
            Entry::Occupied(_) => Err(Rests::Another),
            Entry::Vacant(slot) => {
                slot.insert(judgement);
                Ok(())
            }
        }
    }

    /// The judgements the order's `inputs` rest on, each once, in the order the inputs name them.
    pub fn resting<'a>(
        &self,
        order: &Id,
        inputs: impl IntoIterator<Item = &'a str>,
    ) -> Vec<Judgement> {
        let mut found: Vec<Judgement> = Vec::new();
        for input in inputs {
            let Some(judgement) = self
                .rests
                .get(&(order.as_str().to_owned(), input.to_owned()))
            else {
                continue;
            };
            if !found.contains(judgement) {
                found.push(judgement.clone());
            }
        }
        found
    }

    /// Appends, once each, the reliance of `provider` on each of `judgements` for `order`, before an operation that
    /// carries what rests on them is sent. The receipts, in `judgements`' order.
    ///
    /// The entry is built the first time and appended unchanged however often this is called, so a retry after a
    /// failed or unanswered append gives the receipt the log already holds, and one judgement relied on by one
    /// provider is one entry. The first failure stops the rest, and the caller sends nothing: a refusal is the log
    /// saying the judgement isn't its judge's (`reliance-names-its-judges-statement`), and it is asked the same on
    /// every try (`REQ-CPL-12`, `INV-7`, `INV-8`).
    pub fn write(
        &mut self,
        log: &mut dyn Log,
        core: &Actor,
        at: &Timestamp,
        order: &Id,
        provider: &Id,
        judgements: &[Judgement],
    ) -> Result<Vec<Receipt>, AppendFailure> {
        let mut receipts = Vec::new();
        for judgement in judgements {
            let key = format!(
                "{}\u{0}{}\u{0}{}",
                order.as_str(),
                provider.as_str(),
                serde_json::to_string(&judgement.receipt).unwrap_or_default()
            );
            let held = match self.entries.entry(key) {
                Entry::Occupied(held) => held.into_mut(),
                Entry::Vacant(slot) => {
                    // Every input of the order declared on this judgement, by name: what a Core that reopens
                    // rebuilds the declarations from (`T-020-46`).
                    let inputs: Vec<Id> = self
                        .rests
                        .iter()
                        .filter(|((o, _), held)| o == order.as_str() && *held == judgement)
                        .filter_map(|((_, input), _)| Id::new(input.as_str()))
                        .collect();
                    let entry = verified(
                        core,
                        What::Reliance {
                            order: order.clone(),
                            judgement: judgement.receipt.clone(),
                            judge: judgement.judge.clone(),
                            relied_on_by: provider.clone(),
                            inputs,
                        },
                        at,
                        Outcome::Observed {},
                    );
                    entry.check().map_err(AppendFailure::Refused)?;
                    let entry = GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?;
                    slot.insert(Written {
                        entry,
                        receipt: None,
                    })
                }
            };
            let receipt = match &held.receipt {
                Some(receipt) => receipt.clone(),
                None => {
                    let receipt = append_general(log, &held.entry)?;
                    held.receipt = Some(receipt.clone());
                    receipt
                }
            };
            receipts.push(receipt);
        }
        Ok(receipts)
    }
}

/// The judgement whose stated entry the log holds as `statement`, the verifier's judgement an operation's eligibility
/// figure carries whole (`REQ-CPL-12`, `T-020-61`): the entry of the judgement's shape, attributed to the maker the
/// statement names, whose bytes the evidence store kept under its commitment are that statement. Core finds the entry
/// itself, so the receipt a reliance cites is never a caller's word. `None` where the log holds none: the input then
/// rests on nothing Core can cite, and no reliance is declared for it.
pub fn stated_judgement(
    history: &[(Receipt, rutent_contracts::core_internal::Entry)],
    kept: &mut dyn FnMut(&rutent_contracts::module_interface::Digest) -> Option<Vec<u8>>,
    statement: &Statement<provider_role::Judgement>,
) -> Option<Judgement> {
    history.iter().find_map(|(receipt, entry)| {
        let rutent_contracts::core_internal::Entry::Stated(stated) = entry else {
            return None;
        };
        let Attribution::Verified { who } = &stated.attribution else {
            return None;
        };
        if stated.what.statement.as_str() != JUDGEMENT_SHAPE || who.acts.as_str() != statement.maker
        {
            return None;
        }
        let bytes = kept(&stated.commitment)?;
        let held = serde_json::from_slice::<Statement<provider_role::Judgement>>(&bytes).ok()?;
        (held == *statement).then(|| Judgement {
            receipt: receipt.clone(),
            judge: who.acts.clone(),
        })
    })
}

#[cfg(test)]
mod tests {
    use rutent_contracts::core_internal::{ActorKind, Role};

    use super::*;

    fn id(text: &str) -> Id {
        Id::new(text).unwrap()
    }

    fn receipt(index: u64) -> Receipt {
        serde_json::from_value(serde_json::json!({
            "log": "log-1",
            "index": index,
            "entry": { "algorithm": "sha-256", "value": format!("{index:064x}") },
        }))
        .unwrap()
    }

    #[derive(Default)]
    struct Kept(Vec<GeneralEntry>);

    impl Log for Kept {
        fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
            let at = match self.0.iter().position(|e| e == entry) {
                Some(at) => at,
                None => {
                    self.0.push(entry.clone());
                    self.0.len() - 1
                }
            };
            Ok(receipt(at as u64))
        }
    }

    fn core() -> Actor {
        Actor {
            party: id("rutent"),
            acts: id("core-orch"),
            role: Role::Core,
            kind: ActorKind::Component,
        }
    }

    fn judged(by: &str) -> Judgement {
        Judgement {
            receipt: receipt(7),
            judge: id(by),
        }
    }

    fn at() -> Timestamp {
        Timestamp::new("2026-11-10T09:00:00Z".to_owned()).unwrap()
    }

    /// The verifier's judgement as an eligibility figure carries it: by `maker`, about `subject`.
    fn statement(maker: &str, subject: &str) -> Statement<provider_role::Judgement> {
        serde_json::from_value(serde_json::json!({
            "deployment": {
                "name": "rutent-test",
                "operator_key": "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA"
            },
            "protocol_version": "2.0.1",
            "shape": JUDGEMENT_SHAPE,
            "maker": maker,
            "made_at": "2026-11-06T15:04:05Z",
            "signature": {
                "scheme": "ed25519",
                "key": "oKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr8",
                "value": "BwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyAhIiMkJSYnKCkqKywtLi8wMTIzNDU2Nzg5Ojs8PT4_QEFCQ0RFRg"
            },
            "statement": {
                "subject": subject,
                "class": "kyc.natural_person",
                "answer": "yes",
                "valid_until": "2026-11-13T15:04:05Z"
            }
        }))
        .unwrap()
    }

    /// A stated entry of the judgement's shape by `maker`, kept under the commitment `kept`, attributed or not: read
    /// as the log hands it back, so no stated entry is built here but through `relay`'s helper.
    fn stated(maker: &str, kept: u8, attributed: bool) -> rutent_contracts::core_internal::Entry {
        let who = serde_json::json!({
            "party": "acme-checks", "acts": maker, "role": "verifier", "kind": "component",
        });
        let attribution = if attributed {
            serde_json::json!({ "signature": "verified", "who": who })
        } else {
            serde_json::json!({ "signature": "unverified", "claimed_by": who })
        };
        serde_json::from_value(serde_json::json!({
            "kind": "stated",
            "attribution": attribution,
            "what": { "statement": JUDGEMENT_SHAPE },
            "when": { "at": "2026-11-06T15:04:05Z", "source": "party_stated" },
            "received_at": "2026-11-10T09:00:00Z",
            "outcome": { "status": "stated" },
            "signed_with": { "scheme": "ed25519", "key": "oKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr8" },
            "commitment": commitment(kept),
        }))
        .unwrap()
    }

    fn commitment(at: u8) -> rutent_contracts::module_interface::Digest {
        serde_json::from_value(serde_json::json!({
            "algorithm": "sha-256", "value": format!("{at:064x}"),
        }))
        .unwrap()
    }

    /// Core finds the judgement an eligibility figure carries in the log itself (`T-020-61`): the stated entry of its
    /// maker whose kept bytes are that statement, never another judgement of the same verifier, and never one the log
    /// didn't attribute to it.
    // verifies: REQ-CPL-12
    #[test]
    fn the_judgement_a_figure_carries_is_found_by_its_kept_bytes_and_its_attribution() {
        let wanted = statement("id-check", "payer-ref");
        let other = statement("id-check", "beneficiary-ref");
        let bytes = |s: &Statement<provider_role::Judgement>| serde_json::to_vec(s).unwrap();
        let kept_bytes = [
            (1u8, bytes(&other)),
            (2, bytes(&wanted)),
            (3, bytes(&wanted)),
        ];
        let mut kept = |c: &rutent_contracts::module_interface::Digest| {
            kept_bytes
                .iter()
                .find(|(at, _)| commitment(*at) == *c)
                .map(|(_, b)| b.clone())
        };
        let history = vec![
            (receipt(0), stated("id-check", 3, false)),
            (receipt(1), stated("id-check", 1, true)),
            (receipt(2), stated("id-check", 2, true)),
        ];
        assert_eq!(
            stated_judgement(&history, &mut kept, &wanted),
            Some(Judgement {
                receipt: receipt(2),
                judge: id("id-check"),
            }),
            "the attributed entry whose bytes are the statement"
        );
        assert_eq!(
            stated_judgement(&history[..2], &mut kept, &wanted),
            None,
            "an unattributed copy, or another judgement of the same maker, is never it"
        );
    }

    /// One entry for each provider that relies on a judgement, and one only for a provider that relies on it again.
    // verifies: REQ-CPL-12
    #[test]
    fn a_judgement_is_relied_on_once_by_each_provider_that_relies_on_it() {
        let mut reliances = Reliances::new();
        let mut log = Kept::default();
        let judgement = judged("reserve-a");
        let order = id("ord-1");
        for provider in ["bank-co", "bank-co", "ledger-co", "bank-co"] {
            reliances
                .write(
                    &mut log,
                    &core(),
                    &at(),
                    &order,
                    &id(provider),
                    std::slice::from_ref(&judgement),
                )
                .unwrap();
        }
        assert_eq!(log.0.len(), 2, "bank-co once, ledger-co once");
        // Another order's reliance on the same judgement is its own.
        reliances
            .write(
                &mut log,
                &core(),
                &at(),
                &id("ord-2"),
                &id("bank-co"),
                std::slice::from_ref(&judgement),
            )
            .unwrap();
        assert_eq!(log.0.len(), 3);
    }

    /// A retry gives the receipt of the first try, though Core's clock has moved on.
    // verifies: REQ-CPL-12
    #[test]
    fn a_retry_is_the_same_entry_at_the_same_time() {
        let mut reliances = Reliances::new();
        let mut log = Kept::default();
        let judgement = judged("reserve-a");
        let write = |reliances: &mut Reliances, log: &mut Kept, when: &str| {
            reliances
                .write(
                    log,
                    &core(),
                    &Timestamp::new(when.to_owned()).unwrap(),
                    &id("ord-1"),
                    &id("bank-co"),
                    std::slice::from_ref(&judgement),
                )
                .unwrap()
        };
        let first = write(&mut reliances, &mut log, "2026-11-10T09:00:00Z");
        let second = write(&mut reliances, &mut log, "2026-11-10T09:05:00Z");
        assert_eq!(first, second);
        assert_eq!(log.0.len(), 1);
    }

    /// The inputs that rest on one judgement name it once, and those resting on nothing name none.
    // verifies: REQ-CPL-12
    #[test]
    fn inputs_name_each_judgement_they_rest_on_once() {
        let mut reliances = Reliances::new();
        let order = id("ord-1");
        reliances
            .rests_on(&order, "a", judged("reserve-a"))
            .unwrap();
        reliances
            .rests_on(&order, "b", judged("reserve-a"))
            .unwrap();
        reliances
            .rests_on(&order, "c", judged("reserve-b"))
            .unwrap();
        assert_eq!(
            reliances.resting(&order, ["a", "b", "x", "c"]),
            vec![judged("reserve-a"), judged("reserve-b")]
        );
        assert!(reliances.resting(&id("ord-2"), ["a"]).is_empty());
        assert_eq!(
            reliances.rests_on(&order, "a", judged("reserve-b")),
            Err(Rests::Another)
        );
        assert_eq!(reliances.rests_on(&order, "a", judged("reserve-a")), Ok(()));
    }
}

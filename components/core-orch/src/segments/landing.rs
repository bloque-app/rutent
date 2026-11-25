//! Value that settles at a public inner node held by a holding account lands in it (`T-020-33`; `REQ-ORD-40`,
//! `INV-5`): the operator's plain wallet is asked to place it in the account, and the host attributes it to the order
//! (`Holdings::land`). Until this, nothing in Core called it, so where the value rested at a public node was the
//! scenario's word; now it is Core's own record, made where the segment that brought the value settled.
//!
//! **Only value still resting at the node is held there.** A segment whose value a later segment moved on (the one
//! out of the node settled) before it was landed lands nothing, and a landing is released, in the account, once the
//! segment out of the node settles or the segment that brought the value is reversed (`REQ-AUD-15`: the account's
//! attribution is what the node holds, and `INV-5`'s per-order guard reads it). A release is asked as a landing is.
//!
//! **The wallet is asked once for a segment.** The landing is called by the assembly after each entry point that can
//! settle a segment ([`Segments::land_settled`](super::Segments::land_settled)), like the measure writer, and nothing
//! of a segment, its outcome, its entries or what it moved depends on it: a value that couldn't be placed leaves the
//! segment as it was. A landing the wallet couldn't take *now* (nothing passed out, so nothing is attributed) is asked
//! again on the next call; one whose outcome is unknown, or that the account or the wallet refused, is never asked
//! again: asking twice could attribute the value twice, and the account's own rules then refuse any withdrawal of
//! more than it holds, so nothing leaves the account that was never confirmed in it. It is counted
//! ([`Segments::landings_unconfirmed`](super::Segments::landings_unconfirmed)) for the operator's reading of the
//! account (`REQ-AUD-15`).

use std::collections::BTreeSet;

use rutent_contracts::router_api::Amount;

/// The operator's holding accounts, as the segments land value in them and release it. The assembly gives it, over
/// `Holdings` and the platform that runs their wallets.
pub trait Landings {
    /// Whether `node` has a holding account, which value that settles there lands in. A node with none holds nothing
    /// of Core's.
    fn holds(&self, node: &str) -> bool;

    /// Places `amount` of `asset`, the value of `order` settled at `node`, in the node's holding account.
    fn land(&mut self, node: &str, asset: &str, order: &str, amount: &Amount) -> Landed;

    /// Releases `amount` of `asset` the account holds for `order` at `node`: the value moved on or came back.
    /// [`Landed::Placed`] says it was released.
    fn release(&mut self, node: &str, asset: &str, order: &str, amount: &Amount) -> Landed;

    /// Whether the module of `node`'s holding account restarted since it last confirmed a movement, as the platform
    /// reports its process: the wallet's own record of what it holds for each order went with it (`REQ-ORD-43`,
    /// `T-020-46`). False by default.
    fn restarted(&self, node: &str) -> bool {
        let _ = node;
        false
    }

    /// Restores in `node`'s holding account `amount` of `asset`, what the log holds for `order` there: what the
    /// wallet itself holds for the order is asked first, per order, and only what it lacks is landed again, under a
    /// transaction of Core's own, so nothing is attributed twice (`INV-1`, `T-020-46`). [`Landed::Placed`] says the
    /// account holds it now. Unconfirmed by default: nothing is landed blind.
    fn restore(&mut self, node: &str, asset: &str, order: &str, amount: &Amount) -> Landed {
        let _ = (node, asset, order, amount);
        Landed::Unconfirmed
    }
}

/// What landing, or releasing, a value answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Landed {
    /// The wallet confirmed it and the account holds, or no longer holds, the value for the order.
    Placed,
    /// The wallet could not take it now, and nothing passed out: it is asked again on the next call.
    NotNow,
    /// The wallet didn't confirm it, or its outcome is unknown: never asked again for this segment.
    Unconfirmed,
}

/// A segment, by order and segment.
pub(super) type At = (String, String);

/// A segment the log settled at a node a holding account holds: the segment, the node, the asset, what it gave out
/// there, and whether a later segment moved it on.
pub(super) type Found = (At, String, String, Amount, bool);

/// The sum of `amounts`, or `None` past what an amount holds.
pub(super) fn sum<'a>(amounts: impl IntoIterator<Item = &'a Amount>) -> Option<Amount> {
    amounts
        .into_iter()
        .try_fold(Amount::zero(), |total, amount| total.checked_add(amount))
}

/// What a Core that reopened finds of the landings its segments owed (`T-020-46`): the wallet and the host count per
/// order, so the values of one order still resting at one node are restored together, as their sum, once, and each
/// reads placed where the account then holds it, never landed blind; one that moved on is read as passed. Answers how
/// many it found placed.
/// What `reconcile` restores at once: a node, asset and order, the places the log holds there for it, and their values.
type Group = ((String, String, String), Vec<At>, Vec<Amount>);

pub(super) fn reconcile(
    landing: &mut Landing,
    landings: &mut dyn Landings,
    found: Vec<Found>,
) -> usize {
    let mut groups: Vec<Group> = Vec::new();
    for (at, node, asset, amount, moved_on) in found {
        if moved_on {
            landing.found(at, Some(false));
            continue;
        }
        let key = (node, asset, at.0.clone());
        match groups.iter_mut().find(|(k, _, _)| *k == key) {
            Some((_, ats, amounts)) => {
                ats.push(at);
                amounts.push(amount);
            }
            None => groups.push((key, vec![at], vec![amount])),
        }
    }
    let mut placed = 0;
    for ((node, asset, order), ats, amounts) in groups {
        let answered = match sum(&amounts) {
            Some(total) => landings.restore(&node, &asset, &order, &total),
            None => Landed::Unconfirmed,
        };
        let found = matches!(answered, Landed::Placed).then_some(true);
        for at in ats {
            if found.is_some() {
                placed += 1;
            }
            landing.found(at, found);
        }
    }
    placed
}

/// What became of each segment's landing.
#[derive(Debug, Clone, Default)]
pub(super) struct Landing {
    placed: BTreeSet<At>,
    released: BTreeSet<At>,
    /// The value moved on, or came back, before it was landed: nothing was.
    passed: BTreeSet<At>,
    reversed: BTreeSet<At>,
    unconfirmed: BTreeSet<At>,
    unreleased: BTreeSet<At>,
}

/// What a call owes one segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Owes {
    Nothing,
    Land,
    Release,
}

impl Landing {
    /// What is owed for the segment `at`, whose value `moved_on` says a later segment took out of the node: a landing
    /// while it rests there, a release once it landed and has since left (or the segment was reversed), nothing else.
    /// A value that left before it was landed is never landed.
    pub(super) fn owes(&mut self, at: &At, moved_on: bool) -> Owes {
        let gone = moved_on || self.reversed.contains(at);
        if self.placed.contains(at) {
            if gone && !self.released.contains(at) && !self.unreleased.contains(at) {
                return Owes::Release;
            }
            return Owes::Nothing;
        }
        if self.passed.contains(at) || self.unconfirmed.contains(at) {
            return Owes::Nothing;
        }
        if gone {
            self.passed.insert(at.clone());
            return Owes::Nothing;
        }
        Owes::Land
    }

    /// Takes what the port answered for a landing, and whether it placed it.
    pub(super) fn landed(&mut self, at: At, answered: Landed) -> bool {
        match answered {
            Landed::Placed => {
                self.placed.insert(at);
                true
            }
            Landed::Unconfirmed => {
                self.unconfirmed.insert(at);
                false
            }
            Landed::NotNow => false,
        }
    }

    /// Takes what the port answered for a release.
    pub(super) fn released(&mut self, at: At, answered: Landed) {
        match answered {
            Landed::Placed => {
                self.released.insert(at);
            }
            Landed::Unconfirmed => {
                self.unreleased.insert(at);
            }
            Landed::NotNow => {}
        }
    }

    /// The segment's reversal completed: its value is back where it came from.
    pub(super) fn reverse(&mut self, at: At) {
        self.reversed.insert(at);
    }

    pub(super) fn unconfirmed(&self) -> usize {
        self.unconfirmed.len() + self.unreleased.len()
    }

    /// What a Core that reopened found of a segment's landing (`T-020-46`): placed where the account holds it now,
    /// passed where its value moved on, and otherwise unconfirmed, never landed blind.
    pub(super) fn found(&mut self, at: At, placed: Option<bool>) {
        match placed {
            Some(true) => {
                self.placed.insert(at);
            }
            Some(false) => {
                self.passed.insert(at);
            }
            None => {
                self.unconfirmed.insert(at);
            }
        }
    }

    pub(super) fn is_placed(&self, at: &At) -> bool {
        self.placed.contains(at) && !self.released.contains(at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A holding account that answers every restore placed and keeps what it was asked.
    #[derive(Default)]
    struct Asked(Vec<(String, String)>);

    impl Landings for Asked {
        fn holds(&self, _: &str) -> bool {
            true
        }
        fn land(&mut self, _: &str, _: &str, _: &str, _: &Amount) -> Landed {
            Landed::Placed
        }
        fn release(&mut self, _: &str, _: &str, _: &str, _: &Amount) -> Landed {
            Landed::Placed
        }
        fn restore(&mut self, _: &str, _: &str, order: &str, amount: &Amount) -> Landed {
            self.0.push((order.to_owned(), amount.as_str().to_owned()));
            Landed::Placed
        }
    }

    /// Two values of one order resting at one node, the account's module restarted: the wallet and the host count per
    /// order, so the account is asked to restore the order's whole, once, and both values read placed (`INV-1`,
    /// `T-020-46`'s review, should-fix 1).
    // verifies: INV-1, REQ-ORD-43
    #[test]
    fn two_values_of_one_order_at_one_node_are_restored_as_their_sum() {
        let found = vec![
            (
                ("ord-1".to_owned(), "p1-s1".to_owned()),
                "ledger".to_owned(),
                "USDC".to_owned(),
                Amount::new("300".to_owned()).unwrap(),
                false,
            ),
            (
                ("ord-1".to_owned(), "p2-s1".to_owned()),
                "ledger".to_owned(),
                "USDC".to_owned(),
                Amount::new("200".to_owned()).unwrap(),
                false,
            ),
        ];
        let (mut landing, mut asked) = (Landing::default(), Asked::default());
        assert_eq!(reconcile(&mut landing, &mut asked, found), 2);
        assert_eq!(asked.0, [("ord-1".to_owned(), "500".to_owned())]);
        assert!(landing.is_placed(&("ord-1".to_owned(), "p1-s1".to_owned())));
        assert!(landing.is_placed(&("ord-1".to_owned(), "p2-s1".to_owned())));
    }

    fn at() -> At {
        ("ord-1".to_owned(), "seg-1".to_owned())
    }

    /// A value that rests at the node lands once; one the wallet couldn't take now is asked again, and one it didn't
    /// confirm never is (asking twice could attribute it twice).
    // verifies: REQ-AUD-15, INV-5
    #[test]
    fn landing_is_asked_again_only_where_nothing_took_effect() {
        let mut landing = Landing::default();
        assert_eq!(landing.owes(&at(), false), Owes::Land);
        assert!(!landing.landed(at(), Landed::NotNow));
        assert_eq!(
            landing.owes(&at(), false),
            Owes::Land,
            "nothing took effect"
        );
        assert!(!landing.landed(at(), Landed::Unconfirmed));
        assert_eq!(
            landing.owes(&at(), false),
            Owes::Nothing,
            "never asked again"
        );
        assert_eq!(landing.unconfirmed(), 1);
        assert!(!landing.is_placed(&at()));

        let mut landing = Landing::default();
        assert!(landing.landed(at(), Landed::Placed));
        assert_eq!(landing.owes(&at(), false), Owes::Nothing, "landed once");
        assert!(landing.is_placed(&at()));
    }

    /// A value that moved on before it was landed lands nothing; one landed and since moved on, or reversed, is
    /// released, as a landing is asked: again where nothing took effect, never where unconfirmed.
    // verifies: REQ-AUD-15, INV-5
    #[test]
    fn only_value_still_resting_is_held_and_what_left_is_released() {
        let mut passing = Landing::default();
        assert_eq!(passing.owes(&at(), true), Owes::Nothing, "passed through");
        assert_eq!(passing.owes(&at(), false), Owes::Nothing, "and stays so");

        let mut moved = Landing::default();
        assert!(moved.landed(at(), Landed::Placed));
        assert_eq!(moved.owes(&at(), true), Owes::Release);
        moved.released(at(), Landed::NotNow);
        assert_eq!(
            moved.owes(&at(), true),
            Owes::Release,
            "nothing took effect"
        );
        moved.released(at(), Landed::Unconfirmed);
        assert_eq!(moved.owes(&at(), true), Owes::Nothing, "never asked again");
        assert_eq!(moved.unconfirmed(), 1);

        let mut released = Landing::default();
        assert!(released.landed(at(), Landed::Placed));
        assert_eq!(released.owes(&at(), true), Owes::Release);
        released.released(at(), Landed::Placed);
        assert_eq!(released.owes(&at(), true), Owes::Nothing, "released once");
        assert!(!released.is_placed(&at()));

        let mut reversed = Landing::default();
        assert!(reversed.landed(at(), Landed::Placed));
        assert_eq!(reversed.owes(&at(), false), Owes::Nothing, "resting");
        reversed.reverse(at());
        assert_eq!(reversed.owes(&at(), false), Owes::Release, "reversed");

        let mut never = Landing::default();
        never.reverse(at());
        assert_eq!(
            never.owes(&at(), false),
            Owes::Nothing,
            "reversed before landed"
        );
    }
}

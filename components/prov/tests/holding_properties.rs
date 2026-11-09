//! Property tests of the holding account's invariant (`DEC-18`): seeded, so a failure names the seed that replays it.

use std::collections::BTreeMap;

use rutent_prov::holding::{
    BalanceSource, Difference, HeldForOrder, HoldingAccount, HoldingReading, OrderLedger,
    Reconciliation, reconcile,
};

const CASES: u64 = 400;

/// A deterministic generator (xorshift64*), seeded per case.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n
    }
}

// verifies: INV-1, REQ-CAT-23
#[test]
fn holding_account_balance_equals_the_sum_held_for_its_orders() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let mut ledger = OrderLedger::new("node-a", "USDC");
        let mut model: BTreeMap<String, u128> = BTreeMap::new();

        for step in 0..60 {
            let order = format!("o-{}", rng.below(6));
            let amount = u128::from(rng.below(1_000)) + 1;
            if rng.below(2) == 0 {
                ledger.hold(&order, amount).unwrap();
                *model.entry(order).or_default() += amount;
            } else {
                let held = model.get(&order).copied().unwrap_or(0);
                let result = ledger.release(&order, amount);
                if amount <= held {
                    result.unwrap();
                    if amount == held {
                        model.remove(&order);
                    } else {
                        model.insert(order, held - amount);
                    }
                } else {
                    assert!(result.is_err(), "seed {seed}, step {step}");
                }
            }

            let reading = ledger.reading().unwrap();
            let sum: u128 = reading.held.iter().map(|h| h.amount).sum();
            assert_eq!(reading.balance, sum, "seed {seed}, step {step}");
            let expected: u128 = model.values().sum();
            assert_eq!(reading.balance, expected, "seed {seed}, step {step}");
            for h in &reading.held {
                assert_eq!(
                    model.get(&h.order),
                    Some(&h.amount),
                    "seed {seed}, step {step}"
                );
            }
            assert_eq!(reconcile(&reading), Ok(Reconciliation::Matches));
        }
    }
}

// verifies: INV-1, REQ-AUD-15
#[test]
fn any_difference_between_balance_and_sum_is_raised_with_its_size_and_direction() {
    let mut matched = 0;
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let held: Vec<HeldForOrder> = (0..rng.below(6))
            .map(|i| HeldForOrder {
                order: format!("o-{i}"),
                amount: u128::from(rng.below(1_000)) + 1,
            })
            .collect();
        let sum: u128 = held.iter().map(|h| h.amount).sum();
        let balance = if rng.below(3) == 0 {
            sum
        } else {
            u128::from(rng.below(6_000))
        };
        let reading = HoldingReading {
            node: "n".into(),
            asset: "USDC".into(),
            balance,
            source: BalanceSource::Publisher("p".into()),
            held,
        };
        match reconcile(&reading).unwrap() {
            Reconciliation::Matches => {
                assert_eq!(balance, sum, "seed {seed}");
                matched += 1;
            }
            Reconciliation::Differs(Difference::Short { by, .. }) => {
                assert!(balance < sum && by == sum - balance, "seed {seed}");
            }
            Reconciliation::Differs(Difference::Over { by, .. }) => {
                assert!(balance > sum && by == balance - sum, "seed {seed}");
            }
        }
    }
    assert!(matched > 50, "Matches was hit only {matched} times");
}

//! A segment's advertised price, evaluated exactly at the amount that enters it (`REQ-QTE-5`, `REQ-QTE-8`,
//! `REQ-QTE-12`, `REQ-EXT-2`), as `contracts/router-api/README.md` ("Computing, exactly") states it.
//!
//! Whole numbers only, never floating point: a fee's proportion rounds up, a conversion rounds down once, at the
//! end, so a route is never evaluated as better than its providers may make it. Amounts are up to 78 digits and
//! a conversion multiplies them by powers of ten and by a rate's digits, so everything is an arbitrary-precision
//! [`BigUint`].

use num_bigint::BigUint;
use rutent_contracts::provider_role::Denomination;
use rutent_contracts::router_api::{Amount, Decimal, FeeFormula, Pricing};

/// A whole number from decimal digits, or `None` if the text isn't one.
pub(crate) fn whole(text: &str) -> Option<BigUint> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    BigUint::parse_bytes(text.as_bytes(), 10)
}

/// A checked amount as a whole number. The contract's [`Amount`] holds decimal digits only.
pub(crate) fn number(amount: &Amount) -> BigUint {
    whole(amount.as_str()).unwrap_or_default()
}

fn ten_to(power: u32) -> BigUint {
    BigUint::from(10u8).pow(power)
}

/// A rate as `digits / 10^places`, exactly: `0.00025` is `25 / 10^5`.
fn fraction(rate: &Decimal) -> (BigUint, u32) {
    let text = rate.as_str();
    let (integer, decimals) = text.split_once('.').unwrap_or((text, ""));
    let digits = whole(&format!("{integer}{decimals}")).unwrap_or_default();
    (digits, decimals.len() as u32)
}

/// What a price gives on one amount entering its segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Priced {
    /// The fee, in the segment's input unit.
    pub(crate) fee: BigUint,
    /// What arrives, in the segment's output unit; zero where the price's levels don't reach the net amount.
    pub(crate) amount_out: BigUint,
    /// Whether the net amount is above the price's last level, which the price doesn't serve.
    pub(crate) beyond_levels: bool,
}

/// The fee a formula charges on `amount_in`: `flat + ceil(amount_in × numerator / denominator)`, or its
/// `maximum` where one is given, since the charge may come to it and route evaluation counts it there.
pub(crate) fn fee(formula: &FeeFormula, amount_in: &BigUint) -> BigUint {
    if let Some(maximum) = &formula.maximum {
        return number(maximum);
    }
    let numerator = number(&formula.proportional.numerator);
    let denominator = number(formula.proportional.denominator.amount());
    let proportion = (amount_in * numerator + &denominator - 1u8) / denominator;
    number(&formula.flat) + proportion
}

/// The last level's bound, for a price by levels.
pub(crate) fn deepest(rate: Option<&Pricing>) -> Option<BigUint> {
    match rate {
        Some(Pricing::Levels { levels }) => Some(number(levels.last().up_to.amount())),
        _ => None,
    }
}

/// What a net amount converts to, from `from`'s unit to `to`'s.
///
/// With no rate, only the unit changes: `floor(net × 10^to / 10^from)`. With a rate, the net fills the levels in
/// order, an order book's depth, and a fixed rate is one level with no bound; the output is
/// `floor(Σ take_i × rate_i × 10^to / 10^from)`, rounded once. A net above the last level gives nothing.
pub(crate) fn convert(
    rate: Option<&Pricing>,
    net: &BigUint,
    from: &Denomination,
    to: &Denomination,
) -> BigUint {
    let up = ten_to(u32::from(to.decimals));
    let down = ten_to(u32::from(from.decimals));
    let levels: Vec<(Option<BigUint>, (BigUint, u32))> = match rate {
        None => return net * up / down,
        Some(Pricing::Fixed { rate }) => vec![(None, fraction(rate))],
        Some(Pricing::Levels { levels }) => levels
            .iter()
            .map(|level| (Some(number(level.up_to.amount())), fraction(&level.rate)))
            .collect(),
    };
    if deepest(rate).is_some_and(|last| net > &last) {
        return BigUint::ZERO;
    }
    // Every level's rate over one power of ten, so the sum stays whole until the one division.
    let places = levels.iter().map(|(_, (_, p))| *p).max().unwrap_or(0);
    let mut sum = BigUint::ZERO;
    let mut filled = BigUint::ZERO;
    for (bound, (digits, p)) in &levels {
        if net <= &filled {
            break;
        }
        let top = match bound {
            Some(bound) if bound < net => bound.clone(),
            _ => net.clone(),
        };
        sum += (&top - &filled) * digits * ten_to(places - p);
        filled = top;
    }
    sum * up / (ten_to(places) * down)
}

/// What a price gives on `amount_in`: its fee, then the conversion of what is left. `None` where the fee is above
/// the amount, which no operation can state.
pub(crate) fn evaluate(
    formula: &FeeFormula,
    rate: Option<&Pricing>,
    amount_in: &BigUint,
    from: &Denomination,
    to: &Denomination,
) -> Option<Priced> {
    let fee = fee(formula, amount_in);
    if &fee > amount_in {
        return None;
    }
    let net = amount_in - &fee;
    let beyond_levels = deepest(rate).is_some_and(|last| net > last);
    let amount_out = convert(rate, &net, from, to);
    Some(Priced {
        fee,
        amount_out,
        beyond_levels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rutent_contracts::router_api::{Level, NonEmpty, PositiveAmount, Ratio};

    fn n(text: &str) -> BigUint {
        whole(text).unwrap()
    }

    fn unit(id: &str, decimals: u8) -> Denomination {
        Denomination {
            id: id.to_owned(),
            decimals,
        }
    }

    fn formula(
        flat: &str,
        numerator: &str,
        denominator: &str,
        maximum: Option<&str>,
    ) -> FeeFormula {
        FeeFormula {
            flat: Amount::new(flat).unwrap(),
            proportional: Ratio {
                numerator: Amount::new(numerator).unwrap(),
                denominator: PositiveAmount::new(denominator).unwrap(),
            },
            maximum: maximum.map(|m| Amount::new(m).unwrap()),
        }
    }

    // verifies: REQ-QTE-5
    #[test]
    fn a_fee_is_its_flat_part_plus_its_proportion_rounded_up_or_else_its_maximum() {
        assert_eq!(
            fee(&formula("1000000", "1", "3", None), &n("10")),
            n("1000004")
        );
        assert_eq!(
            fee(&formula("0", "3", "1000", None), &n("99900000")),
            n("299700")
        );
        assert_eq!(
            fee(&formula("0", "1", "100", Some("2000000")), &n("249000750")),
            n("2000000")
        );
    }

    // verifies: REQ-QTE-12, REQ-EXT-2
    #[test]
    fn an_order_books_levels_fill_in_order_and_round_down_once() {
        let book = Pricing::Levels {
            levels: NonEmpty::new(vec![
                Level {
                    up_to: PositiveAmount::new("33333333").unwrap(),
                    rate: Decimal::new("0.000263").unwrap(),
                },
                Level {
                    up_to: PositiveAmount::new("1000000000").unwrap(),
                    rate: Decimal::new("0.0002517").unwrap(),
                },
            ])
            .unwrap(),
        };
        // 33,333,333 × 2.63 + 66,466,767 × 2.517 = 254,963,518.329: once rounded, not 254,963,517.
        let out = convert(
            Some(&book),
            &n("99800100"),
            &unit("COP", 2),
            &unit("USDC", 6),
        );
        assert_eq!(out, n("254963518"));
        let past = evaluate(
            &formula("0", "0", "1", None),
            Some(&book),
            &n("1000000001"),
            &unit("COP", 2),
            &unit("USDC", 6),
        )
        .unwrap();
        assert!(past.beyond_levels);
        assert_eq!(past.amount_out, BigUint::ZERO);
    }

    // verifies: REQ-QTE-12
    #[test]
    fn with_no_rate_only_the_unit_changes_rounding_down() {
        let down = convert(
            None,
            &n("247876749625000000000"),
            &unit("USDC", 18),
            &unit("USDC", 6),
        );
        assert_eq!(down, n("247876749"));
        let up = convert(None, &n("248000750"), &unit("USDC", 6), &unit("USDC", 18));
        assert_eq!(up, n("248000750000000000000"));
    }

    // verifies: REQ-QTE-5
    #[test]
    fn a_fee_above_the_amount_can_not_be_stated() {
        let greedy = formula("5", "0", "1", None);
        assert!(evaluate(&greedy, None, &n("4"), &unit("COP", 2), &unit("COP", 2)).is_none());
    }
}

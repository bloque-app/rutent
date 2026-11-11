//! Exact pricing, as `contracts/router-api/README.md` ("Computing, exactly") states it: whole numbers only, no
//! floating point, a fee's proportion rounded up and one rounding down at the end of a conversion (`REQ-QTE-5`,
//! `REQ-QTE-12`, `THR-10`).
//!
//! Amounts are up to 78 decimal digits, so products and powers of ten are carried as arbitrary-precision whole
//! numbers. Nothing here is serialised, so the 2^53 − 1 limit on safe integers doesn't bind these values.

use num_bigint::BigUint;
use rutent_contracts::provider_role::Denomination;
use rutent_contracts::router_api::{Decimal, FeeFormula, Pricing};

/// A whole number from its decimal digits, as an `Amount` or a fee amount carries it; `None` if it isn't one.
pub(crate) fn whole(text: &str) -> Option<BigUint> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    BigUint::parse_bytes(text.as_bytes(), 10)
}

fn ten_to(power: u32) -> BigUint {
    BigUint::from(10u8).pow(power)
}

/// A rate as `digits / 10^places`, exactly: `0.00025` is `25 / 10^5`.
fn rate(decimal: &Decimal) -> (BigUint, u32) {
    let text = decimal.as_str();
    let (whole_part, fraction) = text.split_once('.').unwrap_or((text, ""));
    let digits = format!("{whole_part}{fraction}");
    let places = u32::try_from(fraction.len()).expect("a decimal has at most 96 characters");
    (
        whole(&digits).expect("a decimal is digits with an optional fraction"),
        places,
    )
}

/// What a price gives on an amount entering its tuple: its fee, and what arrives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Terms {
    /// The fee, in the tuple's input unit.
    pub(crate) fee: BigUint,
    /// What arrives, in the tuple's output unit. Zero where the price's levels don't serve the net amount: the
    /// price gives no output there.
    pub(crate) amount_out: BigUint,
}

/// The fee a formula charges on `amount_in`: `flat + ceil(amount_in × numerator / denominator)`, or its
/// `maximum` where one is given, since route evaluation counts the fee at the maximum (`REQ-QTE-5`). The fee
/// rounds up and the output down, so a route is never evaluated as better than its providers may make it.
pub(crate) fn fee(formula: &FeeFormula, amount_in: &BigUint) -> BigUint {
    if let Some(maximum) = &formula.maximum {
        return whole(maximum.as_str()).expect("an amount");
    }
    let flat = whole(formula.flat.as_str()).expect("an amount");
    let numerator = whole(formula.proportional.numerator.as_str()).expect("an amount");
    let denominator = whole(formula.proportional.denominator.amount().as_str()).expect("an amount");
    // Rounded up: a remainder, however small, costs one more unit.
    flat + (amount_in * numerator + &denominator - 1u8) / denominator
}

/// The operator's own fee on what arrives at the destination (`REQ-QTE-2`, router-api's
/// `PolicyBounds::operator_fee`): `flat + ceil(arrives × numerator / denominator)`, capped by its `maximum` where
/// one is given and never raised to it, since the operator's fee is known before execution.
pub(crate) fn operator_fee(formula: &FeeFormula, arrives: &BigUint) -> BigUint {
    let flat = whole(formula.flat.as_str()).expect("an amount");
    let numerator = whole(formula.proportional.numerator.as_str()).expect("an amount");
    let denominator = whole(formula.proportional.denominator.amount().as_str()).expect("an amount");
    let fee = flat + (arrives * numerator + &denominator - 1u8) / denominator;
    match &formula.maximum {
        Some(maximum) => fee.min(whole(maximum.as_str()).expect("an amount")),
        None => fee,
    }
}

/// What a net amount converts to, from `from`'s unit to `to`'s.
///
/// With no rate, `floor(net × 10^to / 10^from)`. With a rate, the net fills the levels in order (a fixed rate is
/// one unbounded level), and the output is `floor(Σ take_i × rate_i × 10^to / 10^from)`, rounded once, at the
/// end. A net above the last level isn't served, and gives zero.
pub(crate) fn convert(
    rate_of: Option<&Pricing>,
    net: &BigUint,
    from: &Denomination,
    to: &Denomination,
) -> BigUint {
    let scale_up = ten_to(u32::from(to.decimals));
    let scale_down = ten_to(u32::from(from.decimals));
    let levels: Vec<(Option<BigUint>, (BigUint, u32))> = match rate_of {
        None => return net * scale_up / scale_down,
        Some(Pricing::Fixed { rate: r }) => vec![(None, rate(r))],
        Some(Pricing::Levels { levels }) => levels
            .iter()
            .map(|level| {
                (
                    Some(whole(level.up_to.amount().as_str()).expect("an amount")),
                    rate(&level.rate),
                )
            })
            .collect(),
    };
    if let Some((Some(last), _)) = levels.last()
        && net > last
    {
        return BigUint::ZERO;
    }
    // Every rate over one common power of ten, so the sum stays whole until the single division.
    let places = levels.iter().map(|(_, (_, p))| *p).max().unwrap_or(0);
    let mut sum = BigUint::ZERO;
    let mut filled = BigUint::ZERO;
    for (up_to, (digits, p)) in &levels {
        if net <= &filled {
            break;
        }
        let top = match up_to {
            Some(bound) if bound < net => bound.clone(),
            _ => net.clone(),
        };
        let take = &top - &filled;
        sum += take * digits * ten_to(places - p);
        filled = top;
    }
    sum * scale_up / (ten_to(places) * scale_down)
}

/// What a price gives on `amount_in` between two units: its fee, then the conversion of what is left. `None` when
/// the fee is above the amount, which no operation can state.
pub(crate) fn evaluate(
    formula: &FeeFormula,
    rate_of: Option<&Pricing>,
    amount_in: &BigUint,
    from: &Denomination,
    to: &Denomination,
) -> Option<Terms> {
    let fee = fee(formula, amount_in);
    if &fee > amount_in {
        return None;
    }
    let net = amount_in - &fee;
    let amount_out = convert(rate_of, &net, from, to);
    Some(Terms { fee, amount_out })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rutent_contracts::router_api::{Amount, Level, NonEmpty, PositiveAmount, Ratio};

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

    fn levels(levels: &[(&str, &str)]) -> Pricing {
        Pricing::Levels {
            levels: NonEmpty::new(
                levels
                    .iter()
                    .map(|(up_to, r)| Level {
                        up_to: PositiveAmount::new(*up_to).unwrap(),
                        rate: Decimal::new(*r).unwrap(),
                    })
                    .collect(),
            )
            .unwrap(),
        }
    }

    // verifies: REQ-QTE-5
    #[test]
    fn a_fee_is_flat_plus_the_proportion_rounded_up_or_its_maximum() {
        // 1,000,000.00 COP at 3/1000: ceil(99,900,000 × 3 / 1000) = 299,700, which divides evenly.
        assert_eq!(
            fee(&formula("0", "3", "1000", None), &n("99900000")),
            n("299700")
        );
        // A flat 1 USDC plus 1/3 of 10 units: 1,000,000 + ceil(10/3) = 1,000,004.
        assert_eq!(
            fee(&formula("1000000", "1", "3", None), &n("10")),
            n("1000004")
        );
        // With a maximum, route evaluation counts the fee at the maximum, not at the formula's 2,490,008.
        assert_eq!(
            fee(&formula("0", "1", "100", Some("2000000")), &n("249000750")),
            n("2000000")
        );
    }

    // verifies: REQ-QTE-5
    #[test]
    fn a_fee_that_does_not_divide_evenly_rounds_up_so_a_charge_rounded_either_way_is_within_it() {
        // 401,204 COP units at 1/1000 is 401.204: the formula gives 402, so a provider charging 401 or 402 is
        // within it, and a route is never ranked on a fee lower than its provider may charge.
        assert_eq!(
            fee(&formula("0", "1", "1000", None), &n("401204")),
            n("402")
        );
        // Where the proportion divides evenly, nothing is rounded.
        assert_eq!(
            fee(&formula("0", "1", "1000", None), &n("401000")),
            n("401")
        );
    }

    // verifies: REQ-QTE-5, REQ-QTE-12
    #[test]
    fn levels_are_filled_in_order_and_rounded_down_once() {
        // Net 99,800,100 COP units (2 decimals) into USDC (6 decimals): 33,333,333 at 0.000263, then 66,466,767
        // at 0.0002517. Exactly, 33,333,333 × 2.63 + 66,466,767 × 2.517 = 87,666,665.79 + 167,296,852.539 =
        // 254,963,518.329, so 254,963,518. Rounding each level first would give 254,963,517.
        let price = levels(&[("33333333", "0.000263"), ("1000000000", "0.0002517")]);
        let out = convert(
            Some(&price),
            &n("99800100"),
            &unit("COP", 2),
            &unit("USDC", 6),
        );
        assert_eq!(out, n("254963518"));
    }

    // verifies: REQ-QTE-12
    #[test]
    fn a_fixed_rate_scales_by_the_units_and_a_net_above_the_last_level_gives_nothing() {
        let fixed = Pricing::Fixed {
            rate: Decimal::new("0.00025").unwrap(),
        };
        // 99,600,300 × 0.00025 × 10^6 / 10^2 = 249,000,750 exactly.
        let out = convert(
            Some(&fixed),
            &n("99600300"),
            &unit("COP", 2),
            &unit("USDC", 6),
        );
        assert_eq!(out, n("249000750"));
        let shallow = levels(&[("50000000", "0.00025")]);
        assert_eq!(
            convert(
                Some(&shallow),
                &n("50000001"),
                &unit("COP", 2),
                &unit("USDC", 6)
            ),
            BigUint::ZERO
        );
        assert_eq!(
            convert(
                Some(&shallow),
                &n("50000000"),
                &unit("COP", 2),
                &unit("USDC", 6)
            ),
            n("125000000")
        );
    }

    // verifies: REQ-QTE-12
    #[test]
    fn with_no_rate_only_the_units_change_rounding_down() {
        // 247,876,749.625 USDC units of 6 decimals, from 18: floor(247,876,749,625,000,000,000 / 10^12).
        let out = convert(
            None,
            &n("247876749625000000000"),
            &unit("USDC", 18),
            &unit("USDC", 6),
        );
        assert_eq!(out, n("247876749"));
        let up = convert(None, &n("248000750"), &unit("USDC", 6), &unit("USDC", 18));
        assert_eq!(up, n("248000750000000000000"));
    }

    // verifies: REQ-QTE-5
    #[test]
    fn a_fee_above_the_amount_can_not_be_stated() {
        let greedy = formula("5", "0", "1", None);
        assert!(evaluate(&greedy, None, &n("4"), &unit("COP", 2), &unit("COP", 2)).is_none());
        let all = formula("0", "1", "1", None);
        let terms = evaluate(&all, None, &n("4"), &unit("COP", 2), &unit("COP", 2)).unwrap();
        assert_eq!((terms.fee, terms.amount_out), (n("4"), BigUint::ZERO));
    }
}

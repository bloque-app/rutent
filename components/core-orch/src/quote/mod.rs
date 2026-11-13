//! The quote shown: the operator's fee by its formula and shown apart, and Core's records over router-api's types.
//!
//! Declared by `T-020-01`; `T-020-07` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! The rules are the router's, as pure functions over a decision record (`rutent_router::quote`, `T-011-03`), and the
//! arguments a record doesn't hold are router-api's types (`T-020-02`), so Core reaches them through the contract
//! (`DEC-2`). This module is Core's use of them: what the customer is shown, what Core keeps, and what it raises.
//!
//! - **[`show`]: the quote shown** (`REQ-QTE-2`, `REQ-QTE-7`, `REQ-QTE-9`, `REQ-QTE-10`). From the choice round's
//!   record and its chosen candidate: each provider's fee, what the providers' terms deliver (the sum of the quotes,
//!   composed, as the owner ruled on `spec-route-price-composition`), every difference between that and the price
//!   shown named line by line (the operator's fee by its formula, from the bounds the record ran under, and any
//!   charge a provider reports within what it gives out), what the customer receives, each segment's slippage
//!   limit or its freedom past three days, and each segment over three days, which makes the route a quote without
//!   a firm rate. A route whose price can't be delivered as composed is never shown ([`NotShown`]). Intake
//!   (`T-020-03`) shows it, digests it and records the `shown` entry.
//! - **[`executed`] and [`Prices`]: the price** (`REQ-QTE-7`). Once every segment has executed, the same lines from
//!   what each did, labelled the price, recorded as a `shown` entry of `price`. Until then nothing is the price.
//! - **[`Terms`]: stated terms with their times** (`REQ-QTE-6`). Each tuple's history of `StatedTerms`; the terms
//!   that bind a segment are the last stated at or before it started, and later ones never apply to it.
//! - **[`Holds`]: a reserve's held quote** (`REQ-QTE-15`). A choice round's quote held with a stated validity,
//!   labelled a quote, binding no provider; past its validity it starts nothing.
//! - **[`Charges`]: a charge above its formula** (`REQ-QTE-5`, `AC-14`). A segment's charge, checked against the
//!   formula that bound it at its start, raised to the operator where it is above it, and the raise recorded.
//!
//! Ranking stays before the operator's fee (router-api's README; the owner's form, `m3-rank-before-fee`): this module
//! shows the fee on the route the router chose and ranks nothing.

mod charge;
mod held;
mod journal;
mod shown;
mod terms;

pub use charge::{ChargeDesk, ChargeError, Charged, Charges, Checked, Flagged};
pub use held::{Held, HoldError, Holds};
pub use journal::APPEND_TRIES;
pub use shown::{
    HeldTo, NotShown, OperatorFee, PriceError, Priced, Prices, ReportedWithin, SegmentShown, Shown,
    SlowSegment, digest, executed, show,
};
pub use terms::Terms;

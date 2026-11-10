//! The bounds on what a module's parts may ask of the platform (`REQ-MDL-11`, `THR-57`; `B-14` of `EXP-2`'s scoring
//! sheet): how many outbound calls, state reads, outside requests and cross-module calls one operation of one
//! module makes, how many bytes they carry, how long a name or a state key may be, and how deep a chain of
//! cross-module calls goes.
//!
//! Two scopes bound each count and size. A **module's operation** is one operation handed to one provider's
//! module, which a module reached by a cross-module call has of its own. A **provider** is bounded across every
//! operation it handles inside one top-level operation, the one the platform handed to the first provider and the
//! cross-module calls it set off: a provider called eight times in one tree has its operations' sum bounded, not
//! only each one. Both end with the top-level operation, so nothing here is carried from one operation to the next.
//!
//! Nothing here runs a part: [`charge`] and the length checks are pure, so the invariants hold for every sequence
//! of requests (`DEC-18`). The platform asks them before it copies a request's strings, asks the world or writes a
//! record, and turns a refusal into the operation's end: [`PlatformRefusal::LimitReached`], its module still
//! admitted and its process still running.
//!
//! [`PlatformRefusal::LimitReached`]: crate::module::types::PlatformRefusal::LimitReached

/// The longest state key a part may read, in bytes.
pub const MAX_KEY: usize = 256;

/// The longest name a part may give: an operation, a secret, a callee or a kind, in bytes.
pub const MAX_NAME: usize = 128;

/// The longest host name a part may call, in bytes.
pub const MAX_HOST: usize = 253;

/// The longest outside request or outside response the platform takes, in bytes of JSON.
pub const MAX_OUTSIDE_MESSAGE: usize = 64 * 1024;

/// The largest outbound request, in bytes: its body, its path and every header's name and value together.
pub const MAX_BODY: usize = 64 * 1024;

/// The most headers one outbound request may carry.
pub const MAX_HEADERS: usize = 64;

/// The largest frame the platform reads from a provider's process, in bytes. A provider's output, hex-encoded, is
/// the largest message it sends. The platform's reader refuses a longer frame from its length prefix, before
/// it allocates room for it or reads a byte of it.
pub const MAX_PROVIDER_FRAME: usize = 4 * 1024 * 1024;

/// What is counted against a budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Metered {
    /// Outbound calls the egress layer was asked for, refused ones included.
    OutboundCalls,
    /// The bytes of the requests those calls carried: bodies, paths and headers.
    OutboundBytes,
    /// State reads, a denied read of a sandboxed part included.
    StateReads,
    /// The bytes of the values those reads returned.
    StateBytes,
    /// Outside requests: the work a sandboxed part leaves pending with its outside part.
    OutsideRequests,
    /// Cross-module calls, refused ones included.
    ModuleCalls,
    /// Secret lookups, of the outside part and the denied ones of the sandboxed part.
    SecretLookups,
}

/// Every metered thing.
pub const METERED: [Metered; 7] = [
    Metered::OutboundCalls,
    Metered::OutboundBytes,
    Metered::StateReads,
    Metered::StateBytes,
    Metered::OutsideRequests,
    Metered::ModuleCalls,
    Metered::SecretLookups,
];

/// Which bound was hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LimitKind {
    /// A count or a byte total of a budget.
    Metered(Metered),
    /// A chain of cross-module calls too deep.
    CallDepth,
    /// A state key too long.
    KeyLength,
    /// A name or a host too long.
    NameLength,
    /// A message, or one body, too large.
    MessageSize,
}

/// Which budget a count or a total ran out of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// One module's operation.
    Operation,
    /// One provider, across its operations inside one top-level operation.
    Provider,
}

/// A bound that was hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LimitHit {
    /// What was asked for too much of.
    pub kind: LimitKind,
    /// The budget it ran out of, where it was a count or a total.
    pub scope: Option<Scope>,
}

/// The most of each metered thing a scope allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Outbound calls.
    pub outbound_calls: u64,
    /// Bytes of outbound bodies.
    pub outbound_bytes: u64,
    /// State reads.
    pub state_reads: u64,
    /// Bytes of state values read.
    pub state_bytes: u64,
    /// Outside requests.
    pub outside_requests: u64,
    /// Cross-module calls.
    pub module_calls: u64,
    /// Secret lookups.
    pub secret_lookups: u64,
}

impl Budget {
    /// The most of `what` this budget allows.
    pub fn allows(&self, what: Metered) -> u64 {
        match what {
            Metered::OutboundCalls => self.outbound_calls,
            Metered::OutboundBytes => self.outbound_bytes,
            Metered::StateReads => self.state_reads,
            Metered::StateBytes => self.state_bytes,
            Metered::OutsideRequests => self.outside_requests,
            Metered::ModuleCalls => self.module_calls,
            Metered::SecretLookups => self.secret_lookups,
        }
    }
}

/// How much of each metered thing a scope has used.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// Outbound calls.
    pub outbound_calls: u64,
    /// Bytes of outbound bodies.
    pub outbound_bytes: u64,
    /// State reads.
    pub state_reads: u64,
    /// Bytes of state values read.
    pub state_bytes: u64,
    /// Outside requests.
    pub outside_requests: u64,
    /// Cross-module calls.
    pub module_calls: u64,
    /// Secret lookups.
    pub secret_lookups: u64,
}

impl Usage {
    /// How much of `what` has been used.
    pub fn of(&self, what: Metered) -> u64 {
        match what {
            Metered::OutboundCalls => self.outbound_calls,
            Metered::OutboundBytes => self.outbound_bytes,
            Metered::StateReads => self.state_reads,
            Metered::StateBytes => self.state_bytes,
            Metered::OutsideRequests => self.outside_requests,
            Metered::ModuleCalls => self.module_calls,
            Metered::SecretLookups => self.secret_lookups,
        }
    }

    fn slot(&mut self, what: Metered) -> &mut u64 {
        match what {
            Metered::OutboundCalls => &mut self.outbound_calls,
            Metered::OutboundBytes => &mut self.outbound_bytes,
            Metered::StateReads => &mut self.state_reads,
            Metered::StateBytes => &mut self.state_bytes,
            Metered::OutsideRequests => &mut self.outside_requests,
            Metered::ModuleCalls => &mut self.module_calls,
            Metered::SecretLookups => &mut self.secret_lookups,
        }
    }

    /// Whether `amount` more of `what` fits `budget`.
    pub fn fits(&self, what: Metered, amount: u64, budget: &Budget) -> bool {
        self.of(what)
            .checked_add(amount)
            .is_some_and(|total| total <= budget.allows(what))
    }
}

/// Every bound the platform enforces on a module's requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// What one module's operation may use.
    pub per_operation: Budget,
    /// What one provider may use across its operations inside one top-level operation.
    pub per_provider: Budget,
    /// The most operations in flight at once, the top-level one and every cross-module call it set off and that
    /// hasn't answered.
    pub depth: usize,
}

impl Bounds {
    /// The bounds the platform starts with. A stand-in's program takes at most 64 steps, so a test reaches any of
    /// them by repeating a step; the stand-ins' own tests use a small fraction.
    pub const DEFAULT: Self = Self {
        per_operation: Budget {
            outbound_calls: 32,
            outbound_bytes: 1024 * 1024,
            state_reads: 64,
            state_bytes: 1024 * 1024,
            outside_requests: 64,
            module_calls: 8,
            secret_lookups: 32,
        },
        per_provider: Budget {
            outbound_calls: 64,
            outbound_bytes: 2 * 1024 * 1024,
            state_reads: 128,
            state_bytes: 2 * 1024 * 1024,
            outside_requests: 128,
            module_calls: 16,
            secret_lookups: 64,
        },
        depth: 4,
    };
}

impl Default for Bounds {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Charges `amount` of `what` to a module's operation and to its provider, together: both fit and both are
/// charged, or neither is and the bound that was hit is given. The operation's own budget is judged first.
pub fn charge(
    bounds: &Bounds,
    operation: &mut Usage,
    provider: &mut Usage,
    what: Metered,
    amount: u64,
) -> Result<(), LimitHit> {
    let hit = |scope| LimitHit {
        kind: LimitKind::Metered(what),
        scope: Some(scope),
    };
    if !operation.fits(what, amount, &bounds.per_operation) {
        return Err(hit(Scope::Operation));
    }
    if !provider.fits(what, amount, &bounds.per_provider) {
        return Err(hit(Scope::Provider));
    }
    *operation.slot(what) += amount;
    *provider.slot(what) += amount;
    Ok(())
}

/// Whether something of `length` bytes is within `max`, else the bound it breaks.
pub fn within(kind: LimitKind, length: usize, max: usize) -> Result<(), LimitHit> {
    if length <= max {
        Ok(())
    } else {
        Err(LimitHit { kind, scope: None })
    }
}

/// Whether a chain of `in_flight` operations may take one more.
pub fn depth_allows(bounds: &Bounds, in_flight: usize) -> Result<(), LimitHit> {
    if in_flight < bounds.depth {
        Ok(())
    } else {
        Err(LimitHit {
            kind: LimitKind::CallDepth,
            scope: None,
        })
    }
}

/// Appends `item` to `list`, dropping the oldest items first so the list never holds more than `cap`: what the
/// platform keeps of its measurements, so that a platform running for long holds a bounded amount of them.
pub fn keep_last<T>(list: &mut Vec<T>, item: T, cap: usize) {
    list.push(item);
    if list.len() > cap {
        let surplus = list.len() - cap;
        list.drain(..surplus);
    }
}

/// How many of each measurement the platform keeps.
pub const MAX_MEASUREMENTS: usize = 256;

#[cfg(test)]
mod tests {
    use super::*;

    /// A small seeded generator, so a failure names the seed that replays it.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    /// Bounds where each provider budget is at least the operation's, as the platform's are.
    fn small(rng: &mut Rng) -> Bounds {
        let operation = Budget {
            outbound_calls: 1 + rng.below(8),
            outbound_bytes: 1 + rng.below(200),
            state_reads: 1 + rng.below(8),
            state_bytes: 1 + rng.below(200),
            outside_requests: 1 + rng.below(8),
            module_calls: 1 + rng.below(4),
            secret_lookups: 1 + rng.below(8),
        };
        let provider = Budget {
            outbound_calls: operation.outbound_calls + rng.below(16),
            outbound_bytes: operation.outbound_bytes + rng.below(400),
            state_reads: operation.state_reads + rng.below(16),
            state_bytes: operation.state_bytes + rng.below(400),
            outside_requests: operation.outside_requests + rng.below(16),
            module_calls: operation.module_calls + rng.below(8),
            secret_lookups: operation.secret_lookups + rng.below(16),
        };
        Bounds {
            per_operation: operation,
            per_provider: provider,
            depth: 1 + rng.below(5) as usize,
        }
    }

    // verifies: REQ-MDL-11, THR-57
    #[test]
    fn no_sequence_of_charges_takes_more_than_either_budget_allows() {
        for seed in 1..=200u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let bounds = small(&mut rng);
            let (mut operation, mut provider) = (Usage::default(), Usage::default());
            for _ in 0..200 {
                let what = METERED[rng.below(METERED.len() as u64) as usize];
                let amount = rng.below(60);
                let before = (operation, provider);
                match charge(&bounds, &mut operation, &mut provider, what, amount) {
                    Ok(()) => {
                        assert_eq!(
                            operation.of(what),
                            before.0.of(what) + amount,
                            "seed {seed}"
                        );
                        assert_eq!(provider.of(what), before.1.of(what) + amount, "seed {seed}");
                    }
                    Err(hit) => {
                        assert_eq!(hit.kind, LimitKind::Metered(what), "seed {seed}");
                        assert_eq!(
                            (operation, provider),
                            before,
                            "seed {seed}: a refused charge changed a count"
                        );
                    }
                }
                for what in METERED {
                    assert!(
                        operation.of(what) <= bounds.per_operation.allows(what),
                        "seed {seed}: the operation's {what:?} passed its budget"
                    );
                    assert!(
                        provider.of(what) <= bounds.per_provider.allows(what),
                        "seed {seed}: the provider's {what:?} passed its budget"
                    );
                }
            }
        }
    }

    // verifies: REQ-MDL-11, THR-57
    #[test]
    fn a_charge_that_would_pass_a_budget_is_refused_whole_whatever_its_size() {
        for seed in 1..=200u64 {
            let mut rng = Rng(seed.wrapping_mul(0xD1B5_4A32_D192_ED03));
            let bounds = small(&mut rng);
            for what in METERED {
                let (mut operation, mut provider) = (Usage::default(), Usage::default());
                let first = rng.below(bounds.per_operation.allows(what) + 1);
                charge(&bounds, &mut operation, &mut provider, what, first)
                    .expect("a charge within the budget");
                // The next, of any size up to the largest a counter holds, fits only if the total does.
                let next = match rng.below(3) {
                    0 => u64::MAX - rng.below(4),
                    1 => bounds.per_operation.allows(what) - first + 1,
                    _ => rng.below(bounds.per_provider.allows(what) * 2),
                };
                let fits = first.checked_add(next).is_some_and(|total| {
                    total <= bounds.per_operation.allows(what)
                        && total <= bounds.per_provider.allows(what)
                });
                let result = charge(&bounds, &mut operation, &mut provider, what, next);
                assert_eq!(
                    result.is_ok(),
                    fits,
                    "seed {seed}: {what:?} {first} + {next}"
                );
            }
        }
    }

    // verifies: REQ-MDL-11, THR-57
    #[test]
    fn a_provider_budget_bounds_what_several_operations_of_one_provider_use_together() {
        let bounds = Bounds::DEFAULT;
        let mut provider = Usage::default();
        let mut taken = 0;
        loop {
            // Each operation, a fresh one, uses calls well within its own budget.
            let mut operation = Usage::default();
            match charge(
                &bounds,
                &mut operation,
                &mut provider,
                Metered::OutboundCalls,
                bounds.per_operation.outbound_calls,
            ) {
                Ok(()) => taken += bounds.per_operation.outbound_calls,
                Err(hit) => {
                    assert_eq!(hit.scope, Some(Scope::Provider));
                    break;
                }
            }
        }
        assert!(taken <= bounds.per_provider.outbound_calls);
        assert!(taken >= bounds.per_operation.outbound_calls);
    }

    // verifies: REQ-MDL-11, THR-57
    #[test]
    fn a_length_is_within_its_bound_exactly_when_it_does_not_pass_it() {
        for length in [0, 1, MAX_KEY - 1, MAX_KEY, MAX_KEY + 1, usize::MAX] {
            assert_eq!(
                within(LimitKind::KeyLength, length, MAX_KEY).is_ok(),
                length <= MAX_KEY
            );
        }
        let bounds = Bounds::DEFAULT;
        for in_flight in 0..8 {
            assert_eq!(
                depth_allows(&bounds, in_flight).is_ok(),
                in_flight < bounds.depth
            );
        }
    }

    // verifies: REQ-MDL-11
    #[test]
    fn the_measurements_kept_never_pass_their_cap_and_are_the_newest() {
        for cap in [1usize, 3, 10] {
            let mut list = Vec::new();
            for n in 0..50 {
                keep_last(&mut list, n, cap);
                assert!(list.len() <= cap);
                assert_eq!(list.last(), Some(&n));
            }
            assert_eq!(list, ((50 - cap)..50).collect::<Vec<_>>());
        }
    }
}

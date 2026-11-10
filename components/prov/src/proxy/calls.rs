//! Cross-module calls on each module's own credentials (`T-014-04`, `REQ-MDL-21`, `REQ-MDL-9`, `THR-86`).
//!
//! A module may call another provider's module only where its own release declares the call and the route names
//! it. [`judge_call`] is that rule, in the contract's order of checks, and the module platform applies it to every
//! call it routes. What the route names, the platform holds in a [`RouteCalls`]: whoever hands the platform a
//! route segment's operation names the calls that segment makes, for its transaction, and ends them with the route.
//!
//! The call carries no credentials. The callee answers in its own process, and every lookup it makes is
//! attributed to that process ([`Caller`](super::Caller)), so it resolves only its own provider's secrets, for its
//! own outside request in flight; the caller's request in flight gives it nothing.

use std::collections::BTreeMap;

use rutent_contracts::module_interface::{
    CallRefusalReason, Declaration, Identifier, OperationKind, RouteReference,
};

/// One call a route segment names: in its transaction, `caller`'s module may call `callee`'s for `kinds`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedCall {
    /// The route decision record and the segment that name the call.
    pub route: RouteReference,
    /// The calling module's provider.
    pub caller: Identifier,
    /// The provider whose module may be called.
    pub callee: Identifier,
    /// The operation kinds that may be asked of it.
    pub kinds: Vec<OperationKind>,
}

/// The calls the routes in flight name, by transaction. A transaction's calls end with its route
/// ([`RouteCalls::end`]); none is carried from one transaction to another.
#[derive(Debug, Clone, Default)]
pub struct RouteCalls {
    named: BTreeMap<String, Vec<NamedCall>>,
}

impl RouteCalls {
    /// Names `call` in `transaction`'s route.
    pub fn name(&mut self, transaction: &Identifier, call: NamedCall) {
        self.named
            .entry(transaction.as_str().to_owned())
            .or_default()
            .push(call);
    }

    /// Ends `transaction`'s route: no call is named in it any more.
    pub fn end(&mut self, transaction: &Identifier) {
        self.named.remove(transaction.as_str());
    }

    /// The route segment that names `caller`'s call to `callee` for `kind` in `transaction`, the first named if
    /// several do.
    pub fn naming(
        &self,
        transaction: &str,
        caller: &str,
        callee: &str,
        kind: OperationKind,
    ) -> Option<&RouteReference> {
        self.calls(transaction)
            .iter()
            .find(|c| {
                c.caller.as_str() == caller
                    && c.callee.as_str() == callee
                    && c.kinds.contains(&kind)
            })
            .map(|c| &c.route)
    }

    /// The first segment of `transaction`'s route that names any call of `caller`'s.
    fn segment_of(&self, transaction: &str, caller: &str) -> Option<&RouteReference> {
        self.calls(transaction)
            .iter()
            .find(|c| c.caller.as_str() == caller)
            .map(|c| &c.route)
    }

    fn calls(&self, transaction: &str) -> &[NamedCall] {
        self.named.get(transaction).map_or(&[], Vec::as_slice)
    }
}

/// The decision record id of [`no_route`]. It is reserved: no router may issue a decision record by this id, so a
/// call record naming it never names a real route. A decision record's id is its request's (router-api), and the
/// `rutent:` prefix keeps it apart from the ids requests carry.
pub const NO_ROUTE_RECORD: &str = "rutent:no-route";

/// The segment id of [`no_route`], reserved with [`NO_ROUTE_RECORD`].
pub const NO_ROUTE_SEGMENT: &str = "rutent:no-segment";

/// The placeholder a call is recorded against when the route check fails, or when no segment names anything of
/// its caller's: the contract's record names a route for every call, and this one names none. Its ids are
/// reserved ([`NO_ROUTE_RECORD`], [`NO_ROUTE_SEGMENT`]).
pub fn no_route() -> RouteReference {
    RouteReference {
        decision_record: Identifier::new(NO_ROUTE_RECORD).expect("an identifier"),
        segment: Identifier::new(NO_ROUTE_SEGMENT).expect("an identifier"),
    }
}

/// A cross-module call as the platform asks it, all from its own state: the caller is the provider whose process
/// asked, the transaction the one its module is handling, and the kind the kind of the operation forwarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallAsked<'a> {
    /// The provider whose process asked.
    pub caller: &'a str,
    /// The transaction the caller's module is handling.
    pub transaction: &'a str,
    /// The provider called.
    pub callee: &'a str,
    /// The kind of the operation the platform would forward: the one the caller is handling.
    pub kind: OperationKind,
    /// The kind the caller's code named, if it named one the contract knows.
    pub claimed: Option<OperationKind>,
    /// Whether the caller's outside part is serving an outside request: only then can a route segment name the
    /// call.
    pub in_flight: bool,
}

/// How [`judge_call`] judged a call: the route it is recorded against, and why it is refused, if it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallJudgement {
    /// [`no_route`] when the call fails the route check (`not_named_by_route`), since no segment names it.
    /// Otherwise the segment that names the call, else the segment the caller is on in that transaction, else
    /// [`no_route`].
    pub route: RouteReference,
    /// The first check the call fails, or `None` if the platform may pass it on.
    pub refusal: Option<CallRefusalReason>,
}

/// Judges a cross-module call as the platform makes it (`REQ-MDL-21`, `THR-86`), in the contract's order:
///
/// - `undeclared_callee`: `release`, the caller's own running release, doesn't declare the callee. A release of
///   another provider than the caller declares nothing for it;
/// - `undeclared_kind`: it declares the callee, but not the kind forwarded, or the caller's code named another
///   kind than the one forwarded, or the kind is `holding_balance`, which is never a call;
/// - `not_named_by_route`: no outside request of the caller's is in flight, or no segment of the transaction's
///   route names the caller's call to the callee for that kind.
///
/// Whether the callee runs (`callee_unavailable`) is the platform's to judge after this, at run time.
pub fn judge_call(
    release: &Declaration,
    routes: &RouteCalls,
    asked: &CallAsked<'_>,
) -> CallJudgement {
    // `holding_balance` is the operator's own question to a holding account, so that only the operator's audit reads
    // an account's balance through the host: a module may not declare it as a call, and a route may not name it. It
    // is never a call that is declared, and never one a route names (`REQ-AUD-15`).
    let barred = asked.kind == OperationKind::HoldingBalance;
    let naming = routes
        .naming(asked.transaction, asked.caller, asked.callee, asked.kind)
        .filter(|_| !barred);
    let declared = release
        .outside
        .calls
        .iter()
        .filter(|_| release.provider.as_str() == asked.caller)
        .find(|c| c.callee.as_str() == asked.callee);
    let refusal = match declared {
        None => Some(CallRefusalReason::UndeclaredCallee),
        Some(c)
            if barred || !c.kinds.contains(&asked.kind) || asked.claimed != Some(asked.kind) =>
        {
            Some(CallRefusalReason::UndeclaredKind)
        }
        Some(_) if !asked.in_flight || naming.is_none() => Some(CallRefusalReason::NotNamedByRoute),
        Some(_) => None,
    };
    // A call refused by the route check is recorded against no route: no segment names it, and with no request
    // in flight one that names it doesn't apply.
    let route = if refusal == Some(CallRefusalReason::NotNamedByRoute) {
        no_route()
    } else {
        naming
            .or_else(|| routes.segment_of(asked.transaction, asked.caller))
            .cloned()
            .unwrap_or_else(no_route)
    };
    CallJudgement { route, refusal }
}

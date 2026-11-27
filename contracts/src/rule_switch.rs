//! The rule switch the contract's own tests use (`T-020-45`, `T-020-54`): one rule's refusal switched off at a time on a
//! thread, so a test sees whether an invalid fixture breaks that rule alone. Shared by every contract's checks, and
//! owned by none: a contract names no other contract to reach it (`DEC-2`). Built only with the `rule-switches`
//! feature, which the contract's own tests turn on and nothing shipped does.

#[cfg(feature = "rule-switches")]
std::thread_local! {
    static OFF: core::cell::RefCell<alloc::vec::Vec<&'static str>> = const { core::cell::RefCell::new(alloc::vec::Vec::new()) };
}

#[cfg(feature = "rule-switches")]
pub(crate) fn switched_off(rule: &str) -> bool {
    OFF.with(|off| off.borrow().contains(&rule))
}

#[cfg(not(feature = "rule-switches"))]
pub(crate) fn switched_off(_: &str) -> bool {
    false
}

/// Runs `f` with the shape rule `rule`, as its refusal names it, switched off on this thread, so a contract test sees
/// whether an invalid fixture breaks that rule alone: read with it off, the fixture is accepted exactly when no other
/// rule refuses it (`T-020-45`, the milestone review's Low on fixtures pinned by their first refusal). Built only with
/// the `rule-switches` feature, which the contract's own tests turn on and nothing shipped does.
#[cfg(feature = "rule-switches")]
pub fn without_rule<T>(rule: &'static str, f: impl FnOnce() -> T) -> T {
    OFF.with(|off| off.borrow_mut().push(rule));
    let out = f();
    OFF.with(|off| {
        off.borrow_mut().retain(|r| *r != rule);
    });
    out
}

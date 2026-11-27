//! The scan behind `no_public_entry_point_of_core_reaches_an_effect_without_a_decision` (`REQ-AUZ-1`).
//!
//! **It is a tripwire, not the guard.** It catches the plain ways a later edit could go around a decision, and says
//! where. The guard is the log: its append surface is split so that only policy's handle, `PolicyLog`, takes a
//! decision's entry, and orchestration's, `Log`, takes none, held by the crate graph (`T-020-01`). Anything the scan
//! can't see, such as a value read through serde, is that handle's to refuse.
//!
//! It reads the source of Core's three crates, `core-orch`, `core-authz` and `core-audit`, as files, so it imports
//! none of them (`DEC-2`). It walks each crate's module tree from `lib.rs`, leaves test code aside (`#[cfg(test)]`
//! and `#[test]` items, and the files only they declare), and visits every function, closures and macro arguments
//! included. Three rules:
//!
//! 1. **One permit site.** A permitting answer, `Answer::Permit`, is written in exactly one function of Core: the
//!    engine's consultation. The other permitting answers, `SessionOutcome::Opened`, `ReadOutcome::Allowed`,
//!    `ChargeOutcome::Within` and `PersonOutcome::Allowed`, are written only in the engine, each in the one function
//!    that takes it from the consultation's verdict (`session_outcome`, `read_outcome`, `charge_outcome`,
//!    `person_outcome`). No code of Core writes
//!    `Outcome::Permitted` or `PermitAnswer::Permit`, and none but the engine builds a `Decision`.
//! 2. **Effects are orchestration's.** Only `core-orch` records an effect (`What::Effect`, an `Effect`) or calls an
//!    outer library's method that carries out a decided action ([`CARRY_OUT`]).
//! 3. **Only on a decision.** In `core-orch`, a function that records an effect holds a `Permit` or an `Effect` among
//!    its parameters, or takes one from a decision (`.permit()`); a function that carries out an action holds the
//!    `Effect` it carries out among its parameters, whose entry the log admitted only on a permit.
//! 4. **Lifecycle actions through the decided path.** In `core-orch`, a function that carries out one of a release's
//!    lifecycle actions on the platform (`accept`, `withdraw`, `revoke`, `declare_exploited`, `mark_stale`,
//!    `promote`, `roll_back`, `confirm`) is visible in the crate at most, or a method of a port's trait impl; and one
//!    visible outside the crate that reaches one of the crate's carriers ([`CARRIERS`]: `carry_out_roll_back` and its
//!    siblings) asks policy a decision itself: so no caller outside `core-orch` reaches one but through the decided
//!    path, which asks policy, records the effect and carries it out (`F-025`'s note on `carry_out_roll_back`,
//!    `T-020-49`, L1 of #337's review).
//!
//! The names the rules watch can't be renamed out of their sight: no `use .. as`, no type alias of a watched type, no
//! import of a watched enum's variants (`use Answer::*`, `use Answer::Permit`). Nor can a file be mounted from
//! elsewhere: an attribute with a `path` key, `cfg_attr` included, and `include!` are findings wherever they sit. This
//! is boundary-check's rule on mounts, applied here on syn's parse, since Core's crates may not depend on
//! boundary-check.
//!
//! Names are matched without types: a Core method that shares a name in [`CARRY_OUT`] is held to rule 3 as well. What
//! no syntax shows is not checked here: whether an effect's receipt came from the log's append is the row's that
//! carries effects out (`T-020-08`) and the security review's (`T-025-05`); the log guards every effect appended to
//! it, which the test drives too. A macro whose arguments don't read as expressions is searched word by word, and one naming anything the rules
//! watch is a finding.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use syn::parse::ParseStream;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Attribute, Expr, Token};

/// The outer libraries' methods that carry out an action policy decides: each name, the file of ours that defines
/// it, and the action it carries out. `reinstate`, `set_slippage` and `post_route` have no such method yet.
pub(super) const CARRY_OUT: &[(&str, &str, &str)] = &[
    (
        "handle",
        "prov/src/module/platform.rs",
        "move_value, resolve_stuck",
    ),
    (
        "accept",
        "prov/src/module/platform.rs",
        "change_catalogue: accept_module",
    ),
    (
        "withdraw",
        "prov/src/module/platform.rs",
        "change_catalogue: withdraw_release",
    ),
    (
        "revoke",
        "prov/src/module/platform.rs",
        "change_catalogue: revoke_release",
    ),
    (
        "declare_exploited",
        "prov/src/module/platform.rs",
        "change_catalogue: declare_exploited",
    ),
    (
        "mark_stale",
        "prov/src/module/platform.rs",
        "change_catalogue: mark_stale",
    ),
    (
        "promote",
        "prov/src/module/platform.rs",
        "change_catalogue: promote",
    ),
    (
        "roll_back",
        "prov/src/module/platform.rs",
        "change_catalogue: roll_back",
    ),
    // Puts a provider's releases back in force after a restart whose record couldn't vouch for them (`T-013-11`): a
    // sixth lifecycle action, core-internal's `confirm_releases` (`T-020-49`).
    (
        "confirm",
        "prov/src/module/platform.rs",
        "change_catalogue: confirm_releases",
    ),
    (
        "grant",
        "prov/src/catalogue/mod.rs",
        "change_catalogue: grant",
    ),
    (
        "revoke",
        "prov/src/catalogue/mod.rs",
        "change_catalogue: revoke_grant",
    ),
];

/// Every other method of prov's platform and catalogue that changes them (`&mut self`), with why it carries out no
/// action policy decides: none is one of `REQ-AUZ-6`'s permissions or a value-moving operation. A method added there
/// fails [`carry_out_is_pinned`] until it is listed here or in [`CARRY_OUT`].
pub(super) const CARRIES_OUT_NOTHING: &[(&str, &str, &str)] = &[
    // A read: asks a provider's module where a transaction it was handed stands, and moves nothing (`REQ-ORD-1`,
    // `INV-4`, `T-013-11`). Asking status is itself decided (`ask_status`), but it is listed here and not in
    // [`CARRY_OUT`], which would give the scan four false findings. The guard is the `effect` parameter of core-orch's
    // `Modules::status`: only a permitted `ask_status` effect lets Core ask.
    (
        "status",
        "prov/src/module/platform.rs",
        "a read that moves nothing",
    ),
    (
        "set_bounds",
        "prov/src/module/platform.rs",
        "a platform setting",
    ),
    (
        "set_call_budget",
        "prov/src/module/platform.rs",
        "a platform setting",
    ),
    (
        "set_compile_deadline",
        "prov/src/module/platform.rs",
        "a platform setting",
    ),
    (
        "set_measurement_cap",
        "prov/src/module/platform.rs",
        "a platform setting",
    ),
    (
        "set_stderr_capacity",
        "prov/src/module/platform.rs",
        "a platform setting",
    ),
    (
        "set_deadline",
        "prov/src/module/platform.rs",
        "a platform setting",
    ),
    (
        "set_operation_clock",
        "prov/src/module/platform.rs",
        "a platform setting: the clock its deadlines run on, a test's to step",
    ),
    (
        "set_output_limit",
        "prov/src/module/platform.rs",
        "a platform setting",
    ),
    (
        "set_staging",
        "prov/src/module/platform.rs",
        "a platform setting (ADR-0014)",
    ),
    (
        "set_start_deadline",
        "prov/src/module/platform.rs",
        "a platform setting: the most a process may take to start, a test's to widen",
    ),
    (
        "kill",
        "prov/src/module/platform.rs",
        "a process's life, not a release's standing",
    ),
    (
        "restart",
        "prov/src/module/platform.rs",
        "a process's life, not a release's standing",
    ),
    (
        "admit",
        "prov/src/module/platform.rs",
        "admission; it starts only a release that needs no fresh acceptance (`REQ-MDL-3`), a fresh one is `accept`",
    ),
    (
        "begin_segment",
        "prov/src/module/platform.rs",
        "a segment's bounds; its value moves through `handle`",
    ),
    (
        "end_segment",
        "prov/src/module/platform.rs",
        "a segment's bounds; its value moves through `handle`",
    ),
    (
        "take_events",
        "prov/src/module/platform.rs",
        "drains the platform's records for the log",
    ),
    (
        "set_context",
        "prov/src/catalogue/advertisements.rs",
        "a catalogue setting",
    ),
    (
        "set_clock_skew",
        "prov/src/catalogue/advertisements.rs",
        "a catalogue setting",
    ),
    (
        "set_cutover",
        "prov/src/catalogue/advertisements.rs",
        "an older protocol version's cutover date: only the operator's decided setting calls it (`T-020-49`, `T-020-54`), as `set_evidence_floor` is",
    ),
    (
        "trust_advertising_key",
        "prov/src/catalogue/advertisements.rs",
        "a key an advertisement is checked by",
    ),
    (
        "record_statement_key",
        "prov/src/catalogue/advertisements.rs",
        "a key a statement is checked by",
    ),
    (
        "join",
        "prov/src/catalogue/advertisements.rs",
        "an advertisement joining under a grant in force",
    ),
    (
        "set_check_bound",
        "prov/src/catalogue/balance.rs",
        "the balance checks' register (REQ-CAT-3)",
    ),
    (
        "set_answer_within",
        "prov/src/catalogue/balance.rs",
        "the balance checks' register (REQ-CAT-3)",
    ),
    (
        "open_session",
        "prov/src/catalogue/balance.rs",
        "the balance checks' register (REQ-CAT-3)",
    ),
    (
        "begin_check",
        "prov/src/catalogue/balance.rs",
        "the balance checks' register (REQ-CAT-3)",
    ),
    (
        "answer_check",
        "prov/src/catalogue/balance.rs",
        "the balance checks' register (REQ-CAT-3)",
    ),
    (
        "sweep_silent",
        "prov/src/catalogue/balance.rs",
        "the balance checks' register (REQ-CAT-3)",
    ),
    (
        "record",
        "prov/src/catalogue/declaration.rs",
        "a declaration recorded (REQ-CAT-6)",
    ),
    (
        "show_to",
        "prov/src/catalogue/declaration.rs",
        "a declaration shown (REQ-CAT-6)",
    ),
    (
        "bound",
        "prov/src/catalogue/mod.rs",
        "an operator bound (REQ-CAT-1), not a REQ-AUZ-6 permission",
    ),
    (
        "set_ceiling",
        "prov/src/catalogue/mod.rs",
        "a reversal ceiling (DEC-11), not a REQ-AUZ-6 permission",
    ),
    (
        "rebond",
        "prov/src/catalogue/mod.rs",
        "a reversal bond re-checked (DEC-11)",
    ),
    (
        "add_public_node",
        "prov/src/catalogue/mod.rs",
        "the catalogue's nodes",
    ),
    (
        "remove_public_node",
        "prov/src/catalogue/mod.rs",
        "the catalogue's nodes: the operator's decided setting unlisting one (`T-020-49`)",
    ),
    (
        "register_public_holding",
        "prov/src/catalogue/mod.rs",
        "a holding account's register",
    ),
    (
        "register_private_holding",
        "prov/src/catalogue/mod.rs",
        "a holding account's register",
    ),
    (
        "set_evidence_floor",
        "prov/src/catalogue/mod.rs",
        "a tuple's evidence floor, not a REQ-AUZ-6 permission",
    ),
    (
        "set_module_version",
        "prov/src/catalogue/mod.rs",
        "the version in force, as the platform reports it",
    ),
    (
        "declarations_mut",
        "prov/src/catalogue/mod.rs",
        "hands over the declarations, whose methods are listed",
    ),
    (
        "owed_mut",
        "prov/src/catalogue/mod.rs",
        "hands over the owed reversals, whose methods are listed",
    ),
    (
        "publish_operator_module",
        "prov/src/catalogue/operator.rs",
        "the operator's own module published",
    ),
    (
        "register",
        "prov/src/catalogue/publisher.rs",
        "a publisher's register (REQ-CAT-20)",
    ),
    (
        "register_operator",
        "prov/src/catalogue/publisher.rs",
        "a publisher's register (REQ-CAT-20)",
    ),
    (
        "set_ceiling",
        "prov/src/catalogue/reversal.rs",
        "a reversal ceiling (DEC-11)",
    ),
    (
        "rebond",
        "prov/src/catalogue/reversal.rs",
        "a reversal bond re-checked (DEC-11)",
    ),
    (
        "owe",
        "prov/src/catalogue/reversal.rs",
        "a reversal owed (REQ-CAT-7)",
    ),
    (
        "observe",
        "prov/src/catalogue/reversal.rs",
        "an owed reversal observed (REQ-CAT-7)",
    ),
];

/// Which of Core's crates a source belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Crate {
    /// `CMP-CORE-ORCH`.
    Orch,
    /// `CMP-CORE-AUTHZ`.
    Authz,
    /// `CMP-CORE-AUDIT`.
    Audit,
}

/// One place that breaks a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Finding {
    /// The file.
    pub file: String,
    /// The function, or `<module>` outside any.
    pub function: String,
    /// What it does.
    pub what: String,
}

/// What a scan found.
#[derive(Debug, Default)]
pub(super) struct Scan {
    /// The public entry points: public functions and methods, and every method of a trait impl.
    pub entry_points: Vec<String>,
    /// Each function that writes `Answer::Permit`.
    pub permit_sites: Vec<String>,
    /// The functions visited.
    pub functions: usize,
    /// What breaks a rule.
    pub findings: Vec<Finding>,
}

/// The function being visited, and what it holds and does.
#[derive(Default)]
struct Context {
    name: String,
    /// Visible outside its crate, and no method of a trait impl.
    exposed: bool,
    holds_effect: bool,
    holds_permit: bool,
    takes_permit: bool,
    carries_out: Vec<String>,
    records_effect: usize,
    /// The crate's own lifecycle carriers it reaches (rule 4's [`CARRIERS`]).
    reaches_carrier: Vec<String>,
    /// Whether it asks policy a decision (`.decide(..)`).
    decides: bool,
}

/// core-orch's own functions that carry a lifecycle action out on the effect they are handed, each visible in the crate
/// alone (rule 4, `T-020-49`): a function visible outside the crate reaches one only where it asks policy itself, as the
/// decided paths (`Segments::promote`, `roll_back`, `change_release`) do, so no public wrapper carries a lifecycle
/// action out on an effect it was handed (L1 of #337's review).
pub(super) const CARRIERS: &[&str] = &[
    "carry_out_roll_back",
    "carry_promotion",
    "carry_out_release_change",
    "retake_restriction",
];

struct Visitor<'a> {
    krate: Crate,
    file: &'a str,
    engine: bool,
    in_trait_impl: bool,
    stack: Vec<Context>,
    scan: &'a mut Scan,
}

/// Whether `attrs` mark an item as test code only.
fn is_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("test")
            || (a.path().is_ident("cfg")
                && a.meta
                    .require_list()
                    .is_ok_and(|l| l.tokens.to_string().trim() == "test"))
    })
}

fn segments(path: &syn::Path) -> Vec<String> {
    path.segments.iter().map(|s| s.ident.to_string()).collect()
}

fn ends(segs: &[String], tail: &[&str]) -> bool {
    segs.len() >= tail.len()
        && segs[segs.len() - tail.len()..]
            .iter()
            .zip(tail)
            .all(|(a, b)| a == b)
}

/// The type names a type mentions.
fn names(ty: &syn::Type) -> BTreeSet<String> {
    struct Names(BTreeSet<String>);
    impl<'ast> Visit<'ast> for Names {
        fn visit_path(&mut self, path: &'ast syn::Path) {
            if let Some(last) = path.segments.last() {
                self.0.insert(last.ident.to_string());
            }
            visit::visit_path(self, path);
        }
    }
    let mut found = Names(BTreeSet::new());
    found.visit_type(ty);
    found.0
}

/// The words the rules watch for in a macro they can't read.
const WATCHED: &[&str] = &[
    "Permit",
    "Permitted",
    "Effect",
    "Decision",
    "Opened",
    "Allowed",
    "Within",
];

/// The permitting answers, by enum and variant: `Answer::Permit` first, which only the consultation writes.
const PERMITTING: &[(&str, &str)] = &[
    ("Answer", "Permit"),
    ("SessionOutcome", "Opened"),
    ("ReadOutcome", "Allowed"),
    ("ChargeOutcome", "Within"),
    ("PersonOutcome", "Allowed"),
];

/// The enums whose variants the rules watch, which may not be imported bare.
const WATCHED_ENUMS: &[&str] = &[
    "Answer",
    "SessionOutcome",
    "ReadOutcome",
    "ChargeOutcome",
    "PersonOutcome",
    "Outcome",
    "PermitAnswer",
    "What",
];

/// The types the rules watch by name, which may not be renamed.
const WATCHED_TYPES: &[&str] = &["Decision", "Effect", "Permit"];

/// Whether `name` is one of the platform's lifecycle actions on a release, rule 4's: a [`CARRY_OUT`] method of the
/// platform that changes the catalogue.
fn is_a_lifecycle_action(name: &str) -> bool {
    CARRY_OUT.iter().any(|(n, file, action)| {
        *n == name
            && *file == "prov/src/module/platform.rs"
            && action.starts_with("change_catalogue: ")
    })
}

/// Whether `name` is one the rules watch, so renaming it would take it out of their sight.
fn watched(name: &str) -> bool {
    WATCHED_ENUMS.contains(&name)
        || WATCHED_TYPES.contains(&name)
        || CARRY_OUT.iter().any(|(n, _, _)| *n == name)
}

impl Visitor<'_> {
    fn context(&mut self) -> &mut Context {
        self.stack.last_mut().expect("a context")
    }

    fn find(&mut self, what: String) {
        let function = self
            .stack
            .last()
            .map_or_else(String::new, |c| c.name.clone());
        self.scan.findings.push(Finding {
            file: self.file.to_string(),
            function,
            what,
        });
    }

    fn function(
        &mut self,
        name: String,
        sig: &syn::Signature,
        public: bool,
        body: impl FnOnce(&mut Self),
    ) {
        if public {
            self.scan
                .entry_points
                .push(format!("{}::{name}", self.file));
        }
        self.scan.functions += 1;
        let mut context = Context {
            name,
            exposed: public && !self.in_trait_impl,
            ..Context::default()
        };
        for input in &sig.inputs {
            if let syn::FnArg::Typed(typed) = input {
                let names = names(&typed.ty);
                context.holds_effect |= names.contains("Effect");
                context.holds_permit |= names.contains("Permit");
            }
        }
        self.stack.push(context);
        body(self);
        let context = self.stack.pop().expect("its context");
        self.judge(context);
    }

    /// Rules 2 and 3, once a function's body is visited.
    fn judge(&mut self, context: Context) {
        let mut found = Vec::new();
        for call in &context.carries_out {
            if self.krate != Crate::Orch {
                found.push(format!("carries out `{call}` outside orchestration"));
            } else if !context.holds_effect {
                found.push(format!(
                    "carries out `{call}` without the effect it rests on"
                ));
            } else if context.exposed && is_a_lifecycle_action(call) {
                found.push(format!(
                    "carries out `{call}` from a public function, outside the decided path"
                ));
            }
        }
        if self.krate == Crate::Orch && context.exposed && !context.decides {
            for carrier in &context.reaches_carrier {
                found.push(format!(
                    "reaches `{carrier}` from a public function that asks no decision"
                ));
            }
        }
        if context.records_effect > 0 {
            if self.krate != Crate::Orch {
                found.push("records an effect outside orchestration".to_string());
            } else if !(context.holds_effect || context.holds_permit || context.takes_permit) {
                found.push("records an effect without a permit".to_string());
            }
        }
        for what in found {
            self.scan.findings.push(Finding {
                file: self.file.to_string(),
                function: context.name.clone(),
                what,
            });
        }
    }

    fn carry_out(&mut self, name: &str) {
        if CARRY_OUT.iter().any(|(n, _, _)| *n == name) {
            self.context().carries_out.push(name.to_string());
        }
    }

    /// Rule 1, and what rule 3 counts, for a value written at `path`.
    fn written(&mut self, path: &syn::Path) {
        let segs = segments(path);
        if let Some((kind, variant)) = PERMITTING.iter().find(|(e, v)| ends(&segs, &[e, v])) {
            let function = self.stack.last().map_or("", |c| c.name.as_str());
            let site = format!("{}::{function}: {kind}::{variant}", self.file);
            self.scan.permit_sites.push(site);
            if !self.engine {
                self.find(format!("writes `{kind}::{variant}` outside the engine"));
            }
        } else if ends(&segs, &["Outcome", "Permitted"]) || ends(&segs, &["PermitAnswer", "Permit"])
        {
            self.find(format!("writes `{}`", segs.join("::")));
        } else if segs.last().is_some_and(|l| l == "Decision") && !self.engine {
            self.find("builds a decision outside the engine".to_string());
        } else if ends(&segs, &["What", "Effect"]) || segs.last().is_some_and(|l| l == "Effect") {
            self.context().records_effect += 1;
        }
    }
}

/// What a `use` tree does that would take a watched name out of the rules' sight.
fn hidden_by(tree: &syn::UseTree, last: Option<&str>, out: &mut Vec<String>) {
    let under_an_enum = last.filter(|l| WATCHED_ENUMS.contains(l));
    match tree {
        syn::UseTree::Path(path) => {
            let name = path.ident.to_string();
            hidden_by(&path.tree, Some(&name), out);
        }
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                hidden_by(tree, last, out);
            }
        }
        syn::UseTree::Name(name) => {
            if let Some(e) = under_an_enum {
                out.push(format!("imports `{e}::{}` bare", name.ident));
            }
        }
        syn::UseTree::Rename(rename) => {
            let name = rename.ident.to_string();
            if let Some(e) = under_an_enum {
                out.push(format!("imports `{e}::{name}` bare"));
            } else if watched(&name) {
                out.push(format!("renames `{name}` as `{}`", rename.rename));
            }
        }
        syn::UseTree::Glob(_) => {
            if let Some(e) = under_an_enum {
                out.push(format!("glob-imports the variants of `{e}`"));
            }
        }
    }
}

impl<'ast> Visit<'ast> for Visitor<'_> {
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        if is_test(&item.attrs) {
            return;
        }
        let mut found = Vec::new();
        hidden_by(&item.tree, None, &mut found);
        for what in found {
            self.find(what);
        }
        visit::visit_item_use(self, item);
    }

    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        if is_test(&item.attrs) {
            return;
        }
        let aliased: Vec<String> = names(&item.ty).into_iter().filter(|n| watched(n)).collect();
        for name in aliased {
            self.find(format!("aliases `{name}` as `{}`", item.ident));
        }
        visit::visit_item_type(self, item);
    }

    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        let mounts = attr.path().is_ident("path")
            || (attr.path().is_ident("cfg_attr")
                && attr.meta.require_list().is_ok_and(|l| {
                    let text = l.tokens.to_string();
                    let words: Vec<&str> = text.split_whitespace().collect();
                    words
                        .windows(2)
                        .any(|w| w[0].trim_start_matches(',') == "path" && w[1] == "=")
                        || text.contains("path=")
                }));
        if mounts {
            self.find("mounts a file with a `path` attribute".to_string());
        }
        visit::visit_attribute(self, attr);
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !is_test(&item.attrs) {
            visit::visit_item_mod(self, item);
        }
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if is_test(&item.attrs) {
            return;
        }
        let outer = self.in_trait_impl;
        self.in_trait_impl = item.trait_.is_some();
        visit::visit_item_impl(self, item);
        self.in_trait_impl = outer;
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if is_test(&item.attrs) {
            return;
        }
        let public = matches!(item.vis, syn::Visibility::Public(_));
        self.function(item.sig.ident.to_string(), &item.sig, public, |v| {
            visit::visit_item_fn(v, item)
        });
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if is_test(&item.attrs) {
            return;
        }
        let public = self.in_trait_impl || matches!(item.vis, syn::Visibility::Public(_));
        self.function(item.sig.ident.to_string(), &item.sig, public, |v| {
            visit::visit_impl_item_fn(v, item)
        });
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        if is_test(&item.attrs) {
            return;
        }
        self.function(item.sig.ident.to_string(), &item.sig, false, |v| {
            visit::visit_trait_item_fn(v, item)
        });
    }

    fn visit_expr_struct(&mut self, expr: &'ast syn::ExprStruct) {
        self.written(&expr.path);
        visit::visit_expr_struct(self, expr);
    }

    fn visit_expr_path(&mut self, expr: &'ast syn::ExprPath) {
        self.written(&expr.path);
        // A carrier of the crate's own, called or taken as a value, by any path (rule 4).
        if let Some(last) = expr.path.segments.last() {
            let name = last.ident.to_string();
            if CARRIERS.contains(&name.as_str()) && !self.stack.is_empty() {
                self.context().reaches_carrier.push(name);
            }
        }
        // `Platform::handle`, called or taken as a value to call later.
        if expr.path.segments.len() > 1
            && let Some(last) = expr.path.segments.last()
        {
            self.carry_out(&last.ident.to_string());
        }
        visit::visit_expr_path(self, expr);
    }

    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        let method = expr.method.to_string();
        if method == "permit" {
            self.context().takes_permit = true;
        }
        if method == "decide" {
            self.context().decides = true;
        }
        self.carry_out(&method);
        visit::visit_expr_method_call(self, expr);
    }

    fn visit_expr_call(&mut self, expr: &'ast syn::ExprCall) {
        // A plain `handle(..)`; a path of more than one segment counts where it is visited.
        if let Expr::Path(path) = &*expr.func
            && path.path.segments.len() == 1
            && let Some(last) = path.path.segments.last()
        {
            self.carry_out(&last.ident.to_string());
        }
        visit::visit_expr_call(self, expr);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac.path.is_ident("include") {
            self.find("mounts a file with `include!`".to_string());
            return;
        }
        let matches = mac.path.is_ident("matches");
        let read = if matches {
            // Its pattern is a pattern, not a value written: the scrutinee and any guard are read.
            mac.parse_body_with(|input: ParseStream| {
                let scrutinee: Expr = input.parse()?;
                input.parse::<Token![,]>()?;
                syn::Pat::parse_multi_with_leading_vert(input)?;
                let mut read = vec![scrutinee];
                if input.peek(Token![if]) {
                    input.parse::<Token![if]>()?;
                    read.push(input.parse::<Expr>()?);
                }
                if input.peek(Token![,]) {
                    input.parse::<Token![,]>()?;
                }
                Ok(read)
            })
        } else {
            mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated)
                .map(|exprs| exprs.into_iter().collect())
        };
        match read {
            Ok(exprs) => {
                for expr in &exprs {
                    self.visit_expr(expr);
                }
            }
            Err(_) => {
                let text = mac.tokens.to_string();
                let words: BTreeSet<&str> = text
                    .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .filter(|w| !w.is_empty())
                    .collect();
                let named: BTreeSet<&str> = WATCHED
                    .iter()
                    .copied()
                    .chain(CARRY_OUT.iter().map(|(n, _, _)| *n))
                    .filter(|w| words.contains(w))
                    .collect();
                if !named.is_empty() {
                    let named: Vec<&str> = named.into_iter().collect();
                    self.find(format!(
                        "a macro the scan can't read names {}",
                        named.join(", ")
                    ));
                }
            }
        }
    }
}

/// Scans one source file of `krate`, as it would be compiled outside tests.
pub(super) fn scan_source(krate: Crate, file: &str, engine: bool, source: &str, scan: &mut Scan) {
    let parsed = syn::parse_file(source).unwrap_or_else(|e| panic!("{file}: {e}"));
    let mut visitor = Visitor {
        krate,
        file,
        engine,
        in_trait_impl: false,
        stack: vec![Context {
            name: "<module>".to_string(),
            ..Context::default()
        }],
        scan,
    };
    visitor.visit_file(&parsed);
    let module = visitor.stack.pop().expect("the module's context");
    visitor.judge(module);
}

/// The files of a module tree, from `root`, that compile outside tests: each `mod name;` not marked test-only, and
/// those inside inline modules.
fn module_files(root: &Path) -> Vec<PathBuf> {
    fn declared(items: &[syn::Item], dir: &Path, out: &mut Vec<PathBuf>) {
        for item in items {
            let syn::Item::Mod(module) = item else {
                continue;
            };
            // A module mounted with a `path` attribute is a finding of its own, where its declaration is visited.
            if is_test(&module.attrs) || module.attrs.iter().any(|a| a.path().is_ident("path")) {
                continue;
            }
            let name = module.ident.to_string();
            match &module.content {
                Some((_, inner)) => declared(inner, &dir.join(&name), out),
                None => {
                    let flat = dir.join(format!("{name}.rs"));
                    let nested = dir.join(&name).join("mod.rs");
                    let file = if flat.exists() { flat } else { nested };
                    assert!(file.exists(), "module {name} under {}", dir.display());
                    walk(&file, out);
                }
            }
        }
    }
    fn walk(file: &Path, out: &mut Vec<PathBuf>) {
        let source = std::fs::read_to_string(file).unwrap();
        let parsed = syn::parse_file(&source).unwrap();
        let stem = file.file_stem().unwrap().to_str().unwrap();
        let parent = file.parent().unwrap();
        let dir = if stem == "lib" || stem == "mod" {
            parent.to_path_buf()
        } else {
            parent.join(stem)
        };
        out.push(file.to_path_buf());
        declared(&parsed.items, &dir, out);
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

fn components() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Rules 1 to 3 hold over every public entry point of Core's three crates and every function under them, and the
/// outer methods they watch are still where [`CARRY_OUT`] says.
pub(super) fn every_entry_point_of_core_holds() {
    for (name, file, action) in CARRY_OUT {
        let source = std::fs::read_to_string(components().join(file)).unwrap();
        assert!(
            source.contains(&format!("pub fn {name}(")),
            "`{name}`, which carries out {action}, is no longer in {file}"
        );
    }
    let mut scan = Scan::default();
    let engine = components().join("core-authz/src/engine/mod.rs");
    for (krate, dir) in [
        (Crate::Orch, "core-orch"),
        (Crate::Authz, "core-authz"),
        (Crate::Audit, "core-audit"),
    ] {
        let entry_points = scan.entry_points.len();
        let files = module_files(&components().join(dir).join("src/lib.rs"));
        for file in &files {
            let source = std::fs::read_to_string(file).unwrap();
            let shown = file
                .strip_prefix(components())
                .unwrap()
                .display()
                .to_string();
            scan_source(krate, &shown, *file == engine, &source, &mut scan);
        }
        let found = scan.entry_points.len() - entry_points;
        eprintln!("{dir}: {} files, {found} public entry points", files.len());
        assert!(found > 0, "{dir} has entry points to scan");
    }
    assert_eq!(scan.findings, Vec::new());
    assert_eq!(
        scan.permit_sites,
        vec![
            "core-authz/src/engine/mod.rs::consult: Answer::Permit".to_string(),
            "core-authz/src/engine/mod.rs::session_outcome: SessionOutcome::Opened".to_string(),
            "core-authz/src/engine/mod.rs::read_outcome: ReadOutcome::Allowed".to_string(),
            "core-authz/src/engine/mod.rs::charge_outcome: ChargeOutcome::Within".to_string(),
            "core-authz/src/engine/mod.rs::person_outcome: PersonOutcome::Allowed".to_string(),
        ],
        "one permit site, and the open session, the allowed read, the charge within its bound and the allowed \
         question taken from its verdict"
    );
}

/// Every method of prov's platform (`module/platform.rs`) and catalogue (`catalogue/`) that changes them is listed,
/// either as carrying out a decided action ([`CARRY_OUT`]) or as carrying out none ([`CARRIES_OUT_NOTHING`]), and
/// nothing listed is gone.
pub(super) fn carry_out_is_pinned() {
    let prov = components().join("prov/src");
    let mut files = vec![prov.join("module/platform.rs")];
    let mut catalogue: Vec<PathBuf> = std::fs::read_dir(prov.join("catalogue"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    catalogue.sort();
    files.extend(catalogue);
    let mut found = BTreeSet::new();
    for file in &files {
        let parsed = syn::parse_file(&std::fs::read_to_string(file).unwrap()).unwrap();
        let shown = file
            .strip_prefix(components())
            .unwrap()
            .display()
            .to_string();
        for item in &parsed.items {
            let syn::Item::Impl(block) = item else {
                continue;
            };
            for item in &block.items {
                let syn::ImplItem::Fn(f) = item else { continue };
                let changes = matches!(
                    f.sig.inputs.first(),
                    Some(syn::FnArg::Receiver(r)) if r.reference.is_some() && r.mutability.is_some()
                );
                if changes && matches!(f.vis, syn::Visibility::Public(_)) {
                    found.insert((f.sig.ident.to_string(), shown.clone()));
                }
            }
        }
    }
    let listed: BTreeSet<(String, String)> = CARRY_OUT
        .iter()
        .chain(CARRIES_OUT_NOTHING)
        .map(|(n, f, _)| (n.to_string(), f.to_string()))
        .collect();
    let unlisted: Vec<_> = found.difference(&listed).collect();
    let gone: Vec<_> = listed.difference(&found).collect();
    assert!(
        unlisted.is_empty(),
        "prov methods that change state, unlisted: {unlisted:?}"
    );
    assert!(gone.is_empty(), "listed but no longer there: {gone:?}");
}

/// Each rule finds the bypass planted for it, and passes the same code written on a decision.
pub(super) fn each_rule_catches_its_planted_bypass() {
    let found = |krate, engine, source: &str| {
        let mut scan = Scan::default();
        scan_source(krate, "planted.rs", engine, source, &mut scan);
        scan.findings
            .into_iter()
            .map(|f| format!("{}: {}", f.function, f.what))
            .collect::<Vec<_>>()
    };
    let orch = |source: &str| found(Crate::Orch, false, source);

    // Rule 1: a permit minted outside the engine, a decision built there, an entry's outcome written permitted.
    assert_eq!(
        orch("pub fn mint() -> Answer { Answer::Permit {} }"),
        ["mint: writes `Answer::Permit` outside the engine"]
    );
    assert_eq!(
        found(
            Crate::Authz,
            false,
            "fn stub(r: Id) -> Decision { Decision { request: r.clone(), decision: r, outcome, receipt } }"
        ),
        ["stub: builds a decision outside the engine"]
    );
    assert_eq!(
        orch("fn f() { let o = Outcome::Permitted {}; }"),
        ["f: writes `Outcome::Permitted`"]
    );
    assert_eq!(
        found(
            Crate::Authz,
            true,
            "fn consult() -> Answer { Answer::Permit {} }"
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        orch("fn f(d: &Decision) -> bool { matches!(d.outcome, Answer::Permit {}) }"),
        Vec::<String>::new(),
        "a pattern writes nothing"
    );
    assert_eq!(
        orch(
            "fn f(d: &Decision) -> bool { matches!(d.outcome, Answer::Refuse { .. } if keep(Answer::Permit {})) }"
        ),
        ["f: writes `Answer::Permit` outside the engine"],
        "a guard is read"
    );
    assert_eq!(
        orch(
            "fn f(&mut self, o: &Op) -> bool { matches!(o, Op::Move | Op::Hold if self.platform.handle(\"p\", o).ok()) }"
        ),
        ["f: carries out `handle` without the effect it rests on"],
        "a guard is read"
    );

    // The other permitting answers, outside the engine.
    for (written, finding) in [
        (
            "SessionOutcome::Opened { session, expires_at }",
            "SessionOutcome::Opened",
        ),
        ("ReadOutcome::Allowed {}", "ReadOutcome::Allowed"),
        ("ChargeOutcome::Within { left }", "ChargeOutcome::Within"),
        ("PersonOutcome::Allowed {}", "PersonOutcome::Allowed"),
    ] {
        assert_eq!(
            found(
                Crate::Authz,
                false,
                &format!("fn f() {{ let a = {written}; }}")
            ),
            [format!("f: writes `{finding}` outside the engine")]
        );
    }

    // Watched names kept in sight: no rename, alias or bare import of a watched enum's variants.
    assert_eq!(
        orch("use rutent_contracts::core_internal::Answer as A;"),
        ["<module>: renames `Answer` as `A`"]
    );
    assert_eq!(
        orch("use rutent_contracts::core_internal::{Decision as D, Permit};"),
        ["<module>: renames `Decision` as `D`"]
    );
    assert_eq!(
        orch(
            "fn f() { use rutent_prov::module::Platform; let h = Platform::handle; } use x::handle as h;"
        ),
        [
            "f: carries out `handle` without the effect it rests on",
            "<module>: renames `handle` as `h`"
        ]
    );
    assert_eq!(
        orch("use rutent_contracts::core_internal::Answer::*;"),
        ["<module>: glob-imports the variants of `Answer`"]
    );
    assert_eq!(
        orch("use rutent_contracts::core_internal::{Answer::Permit, SessionOutcome::{Opened}};"),
        [
            "<module>: imports `Answer::Permit` bare",
            "<module>: imports `SessionOutcome::Opened` bare"
        ]
    );
    assert_eq!(
        orch("type D = Decision; type Many = Vec<Effect>;"),
        [
            "<module>: aliases `Decision` as `D`",
            "<module>: aliases `Effect` as `Many`"
        ]
    );
    assert_eq!(
        orch(
            "use rutent_contracts::core_internal::*; use rutent_contracts::core_internal::{Answer, Decision};"
        ),
        Vec::<String>::new(),
        "names kept as they are"
    );

    // No file mounted from elsewhere.
    assert_eq!(
        orch("#[path = \"elsewhere.rs\"] mod m;"),
        ["<module>: mounts a file with a `path` attribute"]
    );
    assert_eq!(
        orch("#[cfg_attr(unix, path = \"elsewhere.rs\")] mod m;"),
        ["<module>: mounts a file with a `path` attribute"]
    );
    assert_eq!(
        orch("fn f() { include!(\"elsewhere.rs\"); }"),
        ["f: mounts a file with `include!`"]
    );

    // Rule 2: an effect or a carried-out action outside orchestration.
    assert_eq!(
        found(
            Crate::Audit,
            false,
            "fn f(&mut self, g: Grant) { self.catalogue.grant(g); }"
        ),
        ["f: carries out `grant` outside orchestration"]
    );
    assert_eq!(
        found(
            Crate::Authz,
            false,
            "fn f(p: Permit, a: Action) -> What { What::Effect { action: a, decision: p } }"
        ),
        ["f: records an effect outside orchestration"]
    );

    // Rule 3: in orchestration, only on a decision.
    assert_eq!(
        orch(
            "pub fn go(&mut self, op: &Statement<Operation>) { self.platform.handle(\"p\", op); }"
        ),
        ["go: carries out `handle` without the effect it rests on"]
    );
    assert_eq!(
        orch("fn go(p: &mut Platform, op: &Op) { Platform::handle(p, \"p\", op); }"),
        ["go: carries out `handle` without the effect it rests on"]
    );
    assert_eq!(
        orch(
            "fn go(&mut self, effect: &Effect) { self.platform.handle(\"p\", &self.op(effect)); }"
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        orch(
            "fn rec(a: Action, p: Permit) -> What { What::Effect { action: a, decision: forge() } }"
        ),
        Vec::<String>::new(),
        "a permit in hand"
    );
    assert_eq!(
        orch("fn rec(a: Action) -> What { What::Effect { action: a, decision: forge() } }"),
        ["rec: records an effect without a permit"]
    );
    assert_eq!(
        orch(
            "fn rec(a: Action, d: Decision) -> Option<What> { Some(What::Effect { action: a, decision: d.permit()? }) }"
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        orch(
            "fn go(&mut self) { let run = || self.platform.mark_stale(\"p\", \"v\", \"op\"); run(); }"
        ),
        ["go: carries out `mark_stale` without the effect it rests on"],
        "a closure is its function's"
    );
    assert_eq!(
        orch("const E: fn() = || { platform().handle(\"p\", op()) };"),
        ["<module>: carries out `handle` without the effect it rests on"]
    );

    // Macros: read where their arguments are expressions, searched word by word where not.
    assert_eq!(
        orch("fn go(&mut self) { log!(self.platform.withdraw(\"p\", d, by)); }"),
        ["go: carries out `withdraw` without the effect it rests on"]
    );
    assert_eq!(
        orch("fn go(&mut self) { odd!(=> self.platform.revoke(\"p\")); }"),
        ["go: a macro the scan can't read names revoke"]
    );

    // Test code is left aside, in an inline module or a test function.
    assert_eq!(
        orch(
            "#[cfg(test)] mod tests { fn f() { Answer::Permit {}; } } #[test] fn t() { p.handle(x); }"
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        orch("#[cfg(not(test))] fn f() { p.handle(x); }"),
        ["f: carries out `handle` without the effect it rests on"],
        "only test code is left aside"
    );
}

/// `roll_back`, which ends a release's staging and keeps the version in force (ADR-0014), is listed as carrying out a
/// decided action (`change_catalogue: roll_back`), not as carrying out none, so in orchestration it is called only
/// where the effect it rests on is in hand (`T-021-12`); and the rule finds a call of it planted without one.
pub(super) fn staging_is_rolled_back_only_on_an_effect() {
    let listed = |list: &[(&str, &str, &str)]| {
        list.iter()
            .any(|(n, f, _)| *n == "roll_back" && *f == "prov/src/module/platform.rs")
    };
    assert!(listed(CARRY_OUT), "`roll_back` is a carried out action");
    assert!(
        !listed(CARRIES_OUT_NOTHING),
        "`roll_back` is no longer a method that carries out nothing"
    );
    let orch = |source: &str| {
        let mut scan = Scan::default();
        scan_source(Crate::Orch, "planted.rs", false, source, &mut scan);
        scan.findings
            .into_iter()
            .map(|f| format!("{}: {}", f.function, f.what))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        orch("pub fn go(&mut self) { self.platform.roll_back(\"p\", \"op\", at); }"),
        ["go: carries out `roll_back` without the effect it rests on"]
    );
    assert_eq!(
        orch(
            "fn go(&mut self, effect: &Effect) { self.platform.roll_back(\"p\", \"op\", self.at(effect)); }"
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        found_outside("fn f(&mut self) { self.platform.roll_back(\"p\", \"op\", at); }"),
        ["f: carries out `roll_back` outside orchestration"]
    );
}

/// The five lifecycle actions on a release (`accept`, `withdraw`, `revoke`, `declare_exploited`, `mark_stale`, core-internal's
/// `LIFECYCLE_ACTIONS`) are each listed as carrying out the change of its own on the platform, not as carrying out
/// none, so in orchestration each is called only where the effect it rests on is in hand; and the rule finds a call
/// of each planted without one, in orchestration and outside it (`T-021-15`).
pub(super) fn lifecycle_actions_are_carried_out_only_on_an_effect() {
    use rutent_contracts::core_internal::LIFECYCLE_ACTIONS;

    let orch = |source: &str| {
        let mut scan = Scan::default();
        scan_source(Crate::Orch, "planted.rs", false, source, &mut scan);
        scan.findings
            .into_iter()
            .map(|f| format!("{}: {}", f.function, f.what))
            .collect::<Vec<_>>()
    };
    for (name, change) in LIFECYCLE_ACTIONS {
        let change = serde_json::to_value(change).unwrap();
        let action = format!("change_catalogue: {}", change.as_str().unwrap());
        let on_the_platform = |list: &[(&str, &str, &str)]| {
            list.iter()
                .filter(|(n, f, a)| {
                    *n == name && *f == "prov/src/module/platform.rs" && *a == action
                })
                .count()
        };
        assert_eq!(
            on_the_platform(CARRY_OUT),
            1,
            "`{name}` is a carried out action, `{action}`"
        );
        assert!(
            CARRIES_OUT_NOTHING
                .iter()
                .all(|(n, f, _)| !(*n == name && *f == "prov/src/module/platform.rs")),
            "`{name}` is no method that carries out nothing"
        );
        assert_eq!(
            orch(&format!(
                "pub fn go(&mut self) {{ self.platform.{name}(\"p\", \"v\", \"op\"); }}"
            )),
            [format!(
                "go: carries out `{name}` without the effect it rests on"
            )]
        );
        assert_eq!(
            orch(&format!(
                "fn go(&mut self, effect: &Effect) {{ self.platform.{name}(\"p\", \"v\", self.by(effect)); }}"
            )),
            Vec::<String>::new()
        );
        assert_eq!(
            found_outside(&format!(
                "fn f(&mut self) {{ self.platform.{name}(\"p\", \"v\", \"op\"); }}"
            )),
            [format!("f: carries out `{name}` outside orchestration")]
        );
    }
}

fn found_outside(source: &str) -> Vec<String> {
    let mut scan = Scan::default();
    scan_source(Crate::Authz, "planted.rs", false, source, &mut scan);
    scan.findings
        .into_iter()
        .map(|f| format!("{}: {}", f.function, f.what))
        .collect()
}

/// Rule 4 (`T-020-49`, `F-025`'s note on `carry_out_roll_back`): roll back and its siblings, each of the platform's
/// lifecycle actions on a release, are carried out in `core-orch` only by a function visible in the crate at most, or
/// by a port's trait impl, so a caller outside it reaches them only through the decided path (`Segments::promote`,
/// `roll_back`, `change_release`), which asks policy first. Planted: a public carrier is found, a crate-visible one and
/// a port's impl are not; and Core's own tree holds none.
pub(super) fn lifecycle_actions_are_reached_only_through_the_decided_path() {
    let orch = |source: &str| {
        let mut scan = Scan::default();
        scan_source(Crate::Orch, "planted.rs", false, source, &mut scan);
        scan.findings
            .into_iter()
            .map(|f| format!("{}: {}", f.function, f.what))
            .collect::<Vec<_>>()
    };
    let lifecycle: Vec<&str> = CARRY_OUT
        .iter()
        .filter(|(_, file, action)| {
            *file == "prov/src/module/platform.rs" && action.starts_with("change_catalogue: ")
        })
        .map(|(n, ..)| *n)
        .collect();
    assert_eq!(
        lifecycle,
        [
            "accept",
            "withdraw",
            "revoke",
            "declare_exploited",
            "mark_stale",
            "promote",
            "roll_back",
            "confirm"
        ]
    );
    for name in lifecycle {
        assert_eq!(
            orch(&format!(
                "pub fn carry_out_{name}(m: &mut M, effect: &Effect) {{ m.{name}(effect, \"p\"); }}"
            )),
            [format!(
                "carry_out_{name}: carries out `{name}` from a public function, outside the decided path"
            )]
        );
        assert_eq!(
            orch(&format!(
                "impl S {{ pub fn carry(&mut self, effect: &Effect) {{ self.m.{name}(effect, \"p\"); }} }}"
            )),
            [format!(
                "carry: carries out `{name}` from a public function, outside the decided path"
            )]
        );
        for kept in [
            format!(
                "pub(crate) fn carry_out_{name}(m: &mut M, effect: &Effect) {{ m.{name}(effect, \"p\"); }}"
            ),
            format!("fn carry(m: &mut M, effect: &Effect) {{ m.{name}(effect, \"p\"); }}"),
            format!(
                "impl Port for Platform {{ fn {name}(&mut self, effect: &Effect) {{ Platform::{name}(self, \"p\"); }} }}"
            ),
        ] {
            assert_eq!(orch(&kept), Vec::<String>::new(), "{kept}");
        }
    }
    // A public wrapper of a carrier is found unless it asks policy itself (L1 of #337's review); a crate-visible one isn't.
    for carrier in CARRIERS {
        assert_eq!(
            orch(&format!(
                "pub fn wrap(m: &mut M, effect: &Effect) {{ {carrier}(m, effect); }}"
            )),
            [format!(
                "wrap: reaches `{carrier}` from a public function that asks no decision"
            )]
        );
        assert_eq!(
            orch(&format!(
                "impl S {{ pub fn wrap(&mut self, effect: &Effect) {{ super::lifecycle::{carrier}(&mut self.m, effect); }} }}"
            )),
            [format!(
                "wrap: reaches `{carrier}` from a public function that asks no decision"
            )]
        );
        for kept in [
            format!("pub(crate) fn wrap(m: &mut M, effect: &Effect) {{ {carrier}(m, effect); }}"),
            format!(
                "pub fn decided(&mut self, d: &mut dyn Decisions, r: &DecisionRequest) {{ let x = d.decide(r); {carrier}(self.m, x); }}"
            ),
        ] {
            assert_eq!(orch(&kept), Vec::<String>::new(), "{kept}");
        }
    }
    // `handle` moves value and is no lifecycle action: a public carrier holding its effect is rule 3's alone.
    assert_eq!(
        orch("pub fn go(m: &mut M, effect: &Effect) { m.handle(\"p\", effect); }"),
        Vec::<String>::new()
    );
}

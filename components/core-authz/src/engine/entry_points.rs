//! The scan behind `no_public_entry_point_of_core_reaches_an_effect_without_a_decision` (`REQ-AUZ-1`).
//!
//! It reads the source of Core's three crates, `core-orch`, `core-authz` and `core-audit`, as files, so it imports
//! none of them (`DEC-2`). It walks each crate's module tree from `lib.rs`, leaves test code aside (`#[cfg(test)]`
//! and `#[test]` items, and the files only they declare), and visits every function, closures and macro arguments
//! included. Three rules:
//!
//! 1. **One permit site.** A permitting answer, `Answer::Permit`, is written in exactly one function of Core: the
//!    engine's consultation. No code of Core writes `Outcome::Permitted` or `PermitAnswer::Permit`, and none but the
//!    engine builds a `Decision`.
//! 2. **Effects are orchestration's.** Only `core-orch` records an effect (`What::Effect`, an `Effect`) or calls an
//!    outer library's method that carries out a decided action ([`CARRY_OUT`]).
//! 3. **Only on a decision.** In `core-orch`, a function that records an effect holds a `Permit` or an `Effect` among
//!    its parameters, or takes one from a decision (`.permit()`); a function that carries out an action holds the
//!    `Effect` it carries out among its parameters, whose entry the log admitted only on a permit.
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
    holds_effect: bool,
    holds_permit: bool,
    takes_permit: bool,
    carries_out: Vec<String>,
    records_effect: usize,
}

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
const WATCHED: &[&str] = &["Permit", "Permitted", "Effect", "Decision"];

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
        if ends(&segs, &["Answer", "Permit"]) {
            let function = self.stack.last().map_or("", |c| c.name.as_str());
            let site = format!("{}::{function}", self.file);
            self.scan.permit_sites.push(site);
            if !self.engine {
                self.find("writes a permitting answer outside the engine".to_string());
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

impl<'ast> Visit<'ast> for Visitor<'_> {
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
        visit::visit_expr_path(self, expr);
    }

    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        let method = expr.method.to_string();
        if method == "permit" {
            self.context().takes_permit = true;
        }
        self.carry_out(&method);
        visit::visit_expr_method_call(self, expr);
    }

    fn visit_expr_call(&mut self, expr: &'ast syn::ExprCall) {
        if let Expr::Path(path) = &*expr.func
            && let Some(last) = path.path.segments.last()
        {
            self.carry_out(&last.ident.to_string());
        }
        visit::visit_expr_call(self, expr);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let matches = mac.path.is_ident("matches");
        let read = if matches {
            // Its pattern is a pattern, not a value written: only the scrutinee is read.
            mac.parse_body_with(|input: ParseStream| {
                let scrutinee: Expr = input.parse()?;
                input.step(|cursor| {
                    let mut rest = *cursor;
                    while let Some((_, next)) = rest.token_tree() {
                        rest = next;
                    }
                    Ok(((), rest))
                })?;
                Ok(vec![scrutinee])
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
            if is_test(&module.attrs) {
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
        vec!["core-authz/src/engine/mod.rs::consult".to_string()],
        "one permit site"
    );
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
        ["mint: writes a permitting answer outside the engine"]
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

//! The role suite (`T-002-03`) run through the platform (`B-01`): `M1`'s stand-in of each provider category is the
//! sandboxed part of a module, admitted and run in a process of its own, and the suite's operations reach it as the
//! operator's envelope through the platform.

#[path = "module_support/mod.rs"]
mod common;

use common::{CATEGORIES, WasmBinding, assertion_signed_by, declaration, paths, standins};
use rutent_conformance::boundary::{Admitted, Handled, ModuleBinding, ModuleSpec, Output};
use rutent_conformance::role::stand_in::{Exchange, Ramp, Reserve, Verifier};
use rutent_conformance::role::{
    Check, DeferredVerifier, Exercised, Received, RoleHarness, RoleProvider, SuiteContext,
    WithoutHooks, run_role_suite,
};
use rutent_conformance::signing::{Signing, real, signed_inside};
use rutent_contracts::module_interface::Parts;
use rutent_contracts::provider_role::{
    Advertisement, KeyPurpose, Operation, ResultBody, Statement,
};

/// Every kind of operation a module may be handed: the stand-in itself refuses what is outside its role.
const KINDS: [&str; 8] = [
    "withdrawal",
    "deposit",
    "transfer",
    "conversion",
    "balance_verification",
    "judgement",
    "reversal",
    "reversal_confirmation",
];

/// A stand-in running as a module behind the platform. Operations, and nothing else, go through the platform: the
/// advertisement, the books' fingerprint and the status query are not operations, so they are read from a twin of
/// the stand-in that is asked every operation the module is, and each answer the module gives is compared with the
/// twin's.
struct Through {
    binding: WasmBinding,
    provider: String,
    twin: Box<dyn RoleProvider>,
    /// The advertisement the catalogue holds: the twin's, signed for real. Its statements key is the one the
    /// module's declaration names, and the one the platform's results name.
    signed_advertisement: Received<Advertisement>,
}

impl RoleProvider for Through {
    fn advertisement(&self) -> Received<Advertisement> {
        self.signed_advertisement.clone()
    }

    fn handle(&mut self, operation: &Statement<Operation>) -> Received<ResultBody> {
        let expected = self.twin.handle(operation);
        let Handled::Answered(answered) = self.binding.handle(&self.provider, operation) else {
            panic!("the platform refused an operation of the role suite");
        };
        assert_eq!(
            serde_json::to_value(&answered.statement).expect("JSON"),
            serde_json::to_value(&expected.statement().statement).expect("JSON"),
            "the module answered otherwise than the stand-in does in process"
        );
        // As the platform sends it: the JSON of the statement it signs. A judgement the module nested in its answer
        // is the stand-in's own statement, and carries the signature of the key its placeholder stands for.
        let sent = Received::sent(&answered);
        Received::from_bytes(signed_inside(sent.bytes())).expect("reads back")
    }

    fn state(&self) -> String {
        self.twin.state()
    }

    fn status(&mut self, transaction: &str) -> Option<Received<ResultBody>> {
        self.twin.status(transaction)
    }
}

impl RoleHarness for Through {}

/// A boxed stand-in, as a provider the suite can run.
struct InProcess(Box<dyn RoleProvider>);

impl RoleProvider for InProcess {
    fn advertisement(&self) -> Received<Advertisement> {
        self.0.advertisement()
    }

    fn handle(&mut self, operation: &Statement<Operation>) -> Received<ResultBody> {
        self.0.handle(operation)
    }

    fn state(&self) -> String {
        self.0.state()
    }

    fn status(&mut self, transaction: &str) -> Option<Received<ResultBody>> {
        self.0.status(transaction)
    }
}

/// The category's stand-in, in process.
fn in_process(category: &str) -> Box<dyn RoleProvider> {
    let context = SuiteContext::default();
    match category {
        "reserve" => Box::new(Reserve::new(&context)),
        "exchange" => Box::new(Exchange::new(&context)),
        "ramp" => Box::new(Ramp::new(&context)),
        _ => Box::new(Verifier::new(&context)),
    }
}

/// The category's stand-in as a module of the platform: its role stand-in as the sandboxed part, and the module
/// stand-in's outside part, with a declaration that names the advertisement's own provider and statements key.
fn through(category: &str) -> Through {
    let twin = in_process(category);
    let placeholders = twin.advertisement().into_statement().statement;
    // The stand-in signs with placeholders; its keys, signed for real, are the ones its derived keys stand for.
    let signed_advertisement = Signing(InProcess(in_process(category))).advertisement();
    let advertisement = signed_advertisement.statement().statement.clone();
    let statements_key = advertisement
        .keys
        .iter()
        .find(|k| k.purpose == KeyPurpose::Statements)
        .expect("a statements key")
        .public_key
        .clone();
    let mut declaration = declaration(&advertisement.provider, "1.0.0", &KINDS);
    declaration.outside.statement_key = statements_key;
    let mut binding = common::WasmBinding::new(paths(), &["rs"], category).expect("a binding");
    binding.set_output_limit(1 << 20);
    // The outside part is the module stand-in's, as `M1`'s stand-ins have none of their own.
    let spec = ModuleSpec {
        declaration: declaration.clone(),
        steps: Vec::new(),
        output: Output::Honest,
        starts: true,
    };
    let module = binding
        .build_with(&spec, "rs", category, &[])
        .expect("the module stand-in builds");
    let binaries = Parts {
        sandboxed: std::fs::read(standins().join(format!("role-{category}-sandboxed.wasm")))
            .expect("the role stand-in"),
        outside: module.outside,
    };
    // The catalogue holds the stand-in's advertisement, and its release is signed with the advertising key that
    // advertisement lists.
    assert!(
        binding.list(signed_advertisement.bytes()),
        "{category}: the catalogue wouldn't take the stand-in's advertisement"
    );
    let advertising = placeholders
        .keys
        .iter()
        .find(|k| k.purpose == KeyPurpose::Advertising)
        .expect("an advertising key");
    let admitted = binding.admit(
        &assertion_signed_by(
            &declaration,
            &binaries,
            &real(advertising.public_key.as_str()),
        ),
        &binaries,
    );
    assert!(
        matches!(&admitted, Admitted::Artefact(a) if a.acceptance.accepted.is_none()),
        "{category}: first release wasn't left waiting for its acceptance: {admitted:?}"
    );
    binding.accept(
        &advertisement.provider,
        declaration.version.as_str(),
        "test-reviewer",
    );
    Through {
        binding,
        provider: advertisement.provider,
        twin,
        signed_advertisement,
    }
}

// verifies: REQ-CAT-21, AC-20, DEC-7
#[test]
fn stand_in_of_each_category_passes_its_role_suite_through_the_binding() {
    for category in CATEGORIES {
        let mut provider = WithoutHooks(through(category));
        let report = run_role_suite(&mut provider, &SuiteContext::default(), &DeferredVerifier);
        // The first report of a certification, but for signatures: the platform's are placeholders until `M2`
        // brings the verifier, so `SignatureVerifies` is the one check left deferred.
        let only_signatures_deferred = report
            .deferred
            .iter()
            .all(|c| *c == Check::SignatureVerifies);
        assert!(
            !report.hooks_used
                && report.findings.is_empty()
                && only_signatures_deferred
                && report.uncovered.iter().all(|u| u.answer.needs_a_hook()),
            "{category}: findings {:#?}\nuncovered {:#?}\ndeferred {:#?}",
            report.findings,
            report.uncovered,
            report.deferred
        );
        // The platform hid nothing and added nothing: the run is the stand-in's own run, in process.
        let bare = run_role_suite(
            &mut WithoutHooks(InProcess(in_process(category))),
            &SuiteContext::default(),
            &DeferredVerifier,
        );
        assert_eq!(report.findings, bare.findings, "{category}: findings");
        assert_eq!(report.exercised, bare.exercised, "{category}: exercised");
        assert_eq!(report.uncovered, bare.uncovered, "{category}: uncovered");
        assert_eq!(report.deferred, bare.deferred, "{category}: deferred");
        // What the advertisement requires was seen (`uncovered`), and the operations outside the role reached the
        // module and were refused by it.
        assert!(
            report.exercised.contains(&Exercised::RefusalOutsideRole),
            "{category}: the suite saw {:?}",
            report.exercised
        );
    }
}

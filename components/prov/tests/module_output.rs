//! What the platform passes on of a module's output (`REQ-MDL-12`, `REQ-MDL-9`, `B-19`): each output checked against
//! its schema's patterns and lengths, not only its shape, and no string a guest wrote reaching a refusal, a log or a
//! report. These tests use the Rust stand-ins: the C stand-ins have no mode that replaces one value of the result.

#[path = "module_support/mod.rs"]
mod common;

use common::Probe;
use common::{answers_as, binding, install, spec, withdrawal};
use rutent_conformance::boundary::{Handled, ModuleBinding, PlatformRefusal};
use rutent_prov::module::children::{CompileError, ProviderProcess, Stderrs, compile};
use rutent_prov::module::platform::{COMPILE_DEADLINE, OPERATION_DEADLINE};
use rutent_prov::module::wire::sha256;

const RUST: &str = "rs";
const CANARY: &str = "canary-8f3a1c9e";

fn replaced(pointer: &str, value: &str) -> Vec<Probe> {
    vec![Probe::Output {
        pointer: pointer.to_owned(),
        value: value.to_owned(),
    }]
}

// verifies: REQ-MDL-12
#[test]
fn output_breaking_its_schemas_pattern_or_length_is_refused() {
    let long_reference = format!("\"{}\"", "r".repeat(513));
    let long_amount = format!("\"{}\"", "9".repeat(79));
    let cases = [
        ("amount with a leading zero", "/outcome/amount_in", "\"01\""),
        ("amount with a sign", "/outcome/amount_out", "\"-5\""),
        (
            "amount past its length",
            "/outcome/amount_out",
            long_amount.as_str(),
        ),
        (
            "asset outside its pattern",
            "/outcome/fee/asset",
            "\"co p!\"",
        ),
        (
            "evidence maker outside its pattern",
            "/outcome/evidence/0/by",
            "\"not valid!\"",
        ),
        (
            "reference past its length",
            "/outcome/evidence/0/reference",
            long_reference.as_str(),
        ),
        ("empty reference", "/outcome/evidence/0/reference", "\"\""),
        ("grade outside its range", "/outcome/evidence/0/grade", "9"),
        ("amount of another type", "/outcome/amount_in", "5"),
    ];
    let mut binding = binding("reserve");
    let control = "output-control";
    install(
        &mut binding,
        &spec(control, "1.0.0", Vec::new(), true),
        RUST,
        "reserve",
        &[],
    );
    assert!(
        answers_as(&mut binding, control, "1.0.0", 0),
        "the honest output was refused"
    );
    for (n, (what, pointer, value)) in cases.into_iter().enumerate() {
        let provider = format!("output-case-{n}");
        install(
            &mut binding,
            &spec(&provider, "1.0.0", Vec::new(), true),
            RUST,
            "reserve",
            &replaced(pointer, value),
        );
        let handled = binding.handle(&provider, &withdrawal(&provider, 1));
        assert_eq!(
            handled,
            Handled::Refused(PlatformRefusal::MalformedOutput),
            "{what} was passed on"
        );
    }
    assert!(
        answers_as(&mut binding, control, "1.0.0", 1),
        "another provider stopped answering"
    );
}

// verifies: REQ-MDL-12, REQ-MDL-9
#[test]
fn guest_string_never_appears_in_a_refusal_log() {
    let mut binding = binding("reserve");
    for (n, pointer) in [
        "/outcome/amount_in",
        "/outcome/fee/asset",
        "/outcome/evidence/0/by",
    ]
    .into_iter()
    .enumerate()
    {
        let provider = format!("canary-{n}");
        install(
            &mut binding,
            &spec(&provider, "1.0.0", Vec::new(), true),
            RUST,
            "reserve",
            &replaced(pointer, &format!("\"{CANARY} !\"")),
        );
        let handled = binding.handle(&provider, &withdrawal(&provider, 1));
        assert_eq!(
            handled,
            Handled::Refused(PlatformRefusal::MalformedOutput),
            "{pointer}: the output was passed on"
        );
        let mut seen = vec![format!("{handled:?}")];
        seen.push(format!("{:?}", binding.measurements));
        seen.push(format!("{:?}", binding.take_events()));
        for stderr in binding.stderrs() {
            seen.push(format!(
                "{} {}",
                stderr.label,
                String::from_utf8_lossy(&stderr.bytes)
            ));
        }
        for text in seen {
            assert!(
                !text.contains(CANARY),
                "{pointer}: the guest's string is in {text}"
            );
        }
    }
}

// verifies: REQ-MDL-12
#[test]
fn refused_output_leaves_the_provider_answering() {
    let mut binding = binding("reserve");
    let provider = "output-then-honest";
    install(
        &mut binding,
        &spec(provider, "1.0.0", Vec::new(), true),
        RUST,
        "reserve",
        &replaced("/outcome/amount_in", "\"01\""),
    );
    let pid = binding.process(provider);
    for n in 0..2 {
        assert_eq!(
            binding.handle(provider, &withdrawal(provider, n)),
            Handled::Refused(PlatformRefusal::MalformedOutput)
        );
    }
    assert_eq!(binding.process(provider), pid, "the process was replaced");
}

const NAMED: &str = "canary-zq7kx4";

// verifies: REQ-MDL-12, REQ-MDL-9
#[test]
fn an_imports_name_never_appears_in_the_reason_admission_gives() {
    let dir = std::env::temp_dir().join(format!("rutent-output-canary-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a work directory");
    let read = |name: &str| std::fs::read(common::standins().join(name)).expect("a stand-in");
    let stderrs = Stderrs::default();
    let refused = compile(
        &common::paths(),
        &dir,
        &read("hostile-canary.wasm"),
        &read("rs-reserve-outside.wasm"),
        &stderrs,
        COMPILE_DEADLINE,
    );
    let Err(CompileError::Refused {
        disallowed_import,
        why,
        ..
    }) = refused
    else {
        panic!("a part importing a canary-named interface was not refused");
    };
    assert!(disallowed_import);
    assert!(!why.contains(NAMED), "the import's name is in: {why}");
    // The operator can still find which import it was, by its place among the part's imports.
    assert!(why.contains("import #"), "no index in: {why}");
    for stderr in stderrs.lock().expect("a lock").entries().iter() {
        let text = String::from_utf8_lossy(&stderr.bytes);
        assert!(
            !text.contains(NAMED),
            "the import's name is in stderr: {text}"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

// verifies: REQ-MDL-12, REQ-MDL-9
#[test]
fn a_start_error_the_guest_wrote_never_appears_in_why_the_release_did_not_start() {
    let mut binding = binding("reserve");
    let provider = "start-canary";
    install(
        &mut binding,
        &spec(provider, "1.0.0", Vec::new(), false),
        RUST,
        "reserve",
        &[Probe::StartError(CANARY.to_owned())],
    );
    let machine = binding
        .machine(provider, "1.0.0")
        .expect("its machine code");
    let dir = std::env::temp_dir().join(format!("rutent-start-canary-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a work directory");
    let stderrs = Stderrs::default();
    let started = ProviderProcess::spawn(
        &common::paths(),
        &dir,
        provider,
        &machine,
        [&sha256(&machine.sandboxed), &sha256(&machine.outside)],
        &stderrs,
        OPERATION_DEADLINE,
    );
    let Err(why) = started else {
        panic!("a release whose start fails was started");
    };
    assert!(
        why.contains("didn't start"),
        "refused for another reason: {why}"
    );
    let mut seen = vec![why, format!("{:?}", binding.take_events())];
    for stderr in stderrs.lock().expect("a lock").entries().iter() {
        seen.push(String::from_utf8_lossy(&stderr.bytes).into_owned());
    }
    for stderr in binding.stderrs() {
        seen.push(String::from_utf8_lossy(&stderr.bytes).into_owned());
    }
    for text in seen {
        assert!(!text.contains(CANARY), "the guest's string is in {text}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

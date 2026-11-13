//! The simulated providers' guests (`T-024-01`): one crate, two parts chosen by a feature, and for the sandboxed part
//! a role (`reserve`, `exchange`, `ramp` or `verifier`) chosen by one too.
//!
//! The sandboxed part is `rutent-conformance`'s [`Simulated`] stand-in. For an operation of a provider that moves
//! value it asks its outside part, first, what the script says to do (`sim.begin`: the next behaviour queued for the
//! operation's kind, and the length of the journal the platform's world keeps), reads the journal back when its own
//! books are shorter or longer than it (`sim.book`), answers as the behaviour says, and writes the entry the
//! operation made to the provider's rail (`sim.settle`) and each movement it made to the public network it is on
//! (`sim.chain`). Spinning and trapping, which the behaviours silent, late and crash ask for, are done here, since
//! only a module can.
//!
//! The outside part serves those four outside operations: the state reads, and the two outbound calls, each of them
//! recorded by the platform (`REQ-MDL-8`). A verifier has no script and no book, and asks it nothing (`DEC-16`).

#[cfg(all(feature = "sandboxed", feature = "outside"))]
compile_error!("choose one part: `sandboxed` or `outside`");

#[cfg(feature = "sandboxed")]
mod sandboxed {
    use std::cell::RefCell;

    use rutent_conformance::role::SuiteContext;
    use rutent_conformance::sim::{
        BEGIN, BOOK, Behaviour, CHAIN, Crash, Entry, Role, SETTLE, Simulated, Spec, kind_of,
        transaction_of,
    };
    use rutent_contracts::provider_role::{Operation, Statement};
    use serde_json::{Value, json};

    wit_bindgen::generate!({ world: "sandboxed", path: "../../src/module/wit" });

    use rutent::binding::outside_requests;

    #[cfg(feature = "exchange")]
    const ROLE: Role = Role::Exchange;
    #[cfg(feature = "ramp")]
    const ROLE: Role = Role::Ramp;
    #[cfg(feature = "reserve")]
    const ROLE: Role = Role::Reserve;
    #[cfg(feature = "verifier")]
    const ROLE: Role = Role::Verifier;

    thread_local! {
        /// The provider, built when its first operation names it, and kept for as long as its process lives.
        static PROVIDER: RefCell<Option<Simulated>> = const { RefCell::new(None) };
    }

    struct Part;

    impl Guest for Part {
        fn start() -> Result<(), String> {
            Ok(())
        }

        fn handle(operation: String) -> Vec<u8> {
            let Ok(statement) = serde_json::from_str::<Statement<Operation>>(&operation) else {
                return b"not an operation".to_vec();
            };
            let provider = serde_json::to_value(&statement.statement)
                .ok()
                .and_then(|v| v["provider"].as_str().map(str::to_owned))
                .unwrap_or_default();
            PROVIDER.with(|cell| serve(&mut cell.borrow_mut(), &provider, &statement))
        }
    }

    /// One outside request, and the body of its completed outcome.
    fn ask(transaction: &str, operation: &str, arguments: Value) -> Option<Value> {
        let request = json!({
            "transaction": transaction,
            "operation": operation,
            "arguments": arguments,
        });
        let response: Value =
            serde_json::from_str(&outside_requests::request(&request.to_string())).ok()?;
        (response["outcome"]["status"] == "completed").then(|| response["outcome"]["body"].clone())
    }

    /// The provider, built the first time it is needed, in the deployment the world names (`deployment`, the
    /// state key `DEPLOYMENT_KEY` the outside part reads back with the script), or the suite's default where it
    /// names none: it holds every operation to that deployment.
    fn provider<'a>(
        held: &'a mut Option<Simulated>,
        provider: &str,
        deployment: Option<&Value>,
    ) -> &'a mut Simulated {
        held.get_or_insert_with(|| {
            let mut context = SuiteContext::default();
            if let Some(deployment) = deployment
                .and_then(Value::as_str)
                .and_then(|text| serde_json::from_str(text).ok())
            {
                context.deployment = deployment;
            }
            Simulated::new(Spec::of(ROLE, provider), &context)
        })
    }

    fn serve(
        held: &mut Option<Simulated>,
        provider: &str,
        operation: &Statement<Operation>,
    ) -> Vec<u8> {
        let Some(transaction) = transaction_of(&operation.statement).filter(|_| ROLE.moves_value())
        else {
            // A verifier, and anything with no transaction to ask under, answers as its role asks.
            return self::provider(held, provider, None)
                .answer(operation, &Behaviour::default())
                .body;
        };
        let (kind, dry_run) = kind_of(&operation.statement);
        let begun = ask(
            &transaction,
            BEGIN,
            json!({ "kind": kind, "dry_run": dry_run }),
        );
        let sim = self::provider(held, provider, begun.as_ref().map(|b| &b["deployment"]));
        let provider = sim.spec().provider.clone();
        let behaviour = begun
            .as_ref()
            .and_then(|b| b["script"].as_str())
            .and_then(|script| serde_json::from_str::<Behaviour>(script).ok())
            .unwrap_or_default();
        // The books of this process and the journal's differ: it was started after one that took entries, or an
        // entry it made was never written. The journal is the books.
        let length = begun
            .as_ref()
            .and_then(|b| b["book_length"].as_u64())
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0);
        if sim.applied() != length
            && let Some(book) = ask(&transaction, BOOK, json!({}))
            && let Some(entries) = book["book"]
                .as_str()
                .and_then(|text| serde_json::from_str::<Vec<Entry>>(text).ok())
        {
            sim.restore(&entries);
        }
        if behaviour.silent {
            spin();
        }
        let answer = sim.answer(operation, &behaviour);
        if behaviour.crash == Some(Crash::BeforeSettle) {
            crash();
        }
        if let Some(entry) = &answer.entry {
            match ask(
                &transaction,
                SETTLE,
                json!({ "provider": provider, "entry": entry }),
            ) {
                Some(body) if body["egress"] == "completed" => sim.applied_one(),
                _ => sim.diverged(),
            }
        }
        for movement in &answer.movements {
            ask(&transaction, CHAIN, json!({ "movement": movement }));
        }
        if behaviour.crash == Some(Crash::AfterSettle) {
            crash();
        }
        if behaviour.late {
            spin();
        }
        answer.body
    }

    /// Runs until the platform stops it: at its fuel or its deadline. It never answers.
    fn spin() {
        let mut n: u64 = 0;
        loop {
            n = core::hint::black_box(n.wrapping_add(1));
        }
    }

    fn crash() {
        core::arch::wasm32::unreachable()
    }

    export!(Part);
}

#[cfg(feature = "outside")]
mod outside {
    use rutent_conformance::sim::{
        BEGIN, BOOK, BOOK_KEY, BOOK_LENGTH_KEY, CHAIN, DEPLOYMENT_KEY, SETTLE, chain_host,
        rail_host, script_key,
    };
    use serde_json::{Value, json};

    wit_bindgen::generate!({ world: "outside", path: "../../src/module/wit" });

    use rutent::binding::egress::{self, Scheme};
    use rutent::binding::state;

    struct Part;

    impl Guest for Part {
        fn start() -> Result<(), String> {
            Ok(())
        }

        fn perform(outside_request: String) -> String {
            let request: Value = serde_json::from_str(&outside_request).unwrap_or(Value::Null);
            let transaction = request["transaction"].as_str().unwrap_or_default();
            let operation = request["operation"].as_str().unwrap_or_default();
            let arguments = &request["arguments"];
            let body = match operation {
                BEGIN => {
                    let key = script_key(
                        arguments["kind"].as_str().unwrap_or_default(),
                        arguments["dry_run"].as_bool().unwrap_or(false),
                    );
                    json!({
                        "script": state::read(&key),
                        "book_length": state::read(BOOK_LENGTH_KEY)
                            .and_then(|n| n.parse::<u64>().ok())
                            .unwrap_or(0),
                        "deployment": state::read(DEPLOYMENT_KEY),
                    })
                }
                BOOK => json!({ "book": state::read(BOOK_KEY) }),
                SETTLE => {
                    let provider = arguments["provider"].as_str().unwrap_or_default();
                    json!({ "egress": send(operation, &rail_host(provider), &arguments["entry"]) })
                }
                CHAIN => {
                    let movement = &arguments["movement"];
                    let host = chain_host(movement["network"].as_str().unwrap_or_default());
                    json!({ "egress": send(operation, &host, movement) })
                }
                _ => json!({}),
            };
            json!({
                "transaction": transaction,
                "operation": operation,
                "outcome": { "status": "completed", "body": body },
            })
            .to_string()
        }
    }

    /// One outbound call to `host`, the destination the operation declares, carrying `body`; how it ended.
    fn send(operation: &str, host: &str, body: &Value) -> &'static str {
        match egress::call(
            operation,
            Scheme::Https,
            host,
            443,
            &egress::Request {
                method: egress::Method::Post,
                path: "/".to_owned(),
                headers: Vec::new(),
                body: body.to_string(),
            },
        ) {
            egress::Outcome::Completed => "completed",
            egress::Outcome::Failed => "failed",
            egress::Outcome::Unknown => "unknown",
            egress::Outcome::Refused => "refused",
        }
    }

    export!(Part);
}

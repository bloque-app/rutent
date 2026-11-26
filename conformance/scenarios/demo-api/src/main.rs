//! `rutent-demo-api`: the demo's network over HTTP, with a page that draws it and replays the demo's acts through the
//! API. Nothing in it holds or moves real value, and it makes no outbound call: every provider is one of
//! `rutent-scenarios`'s simulated providers, admitted to the real module platform and signing for real, and Core is
//! the scenario assembly's, the same one `rutent-demo` and `M3`'s scenarios run.
//!
//! ```text
//! cargo run --manifest-path conformance/scenarios/demo-api/Cargo.toml -- [--addr HOST:PORT] [--seed N] [--out DIR]
//!     [--lazy] [--warm-up] [--database-url postgres://…] [--database-ca FILE]
//! ```
//!
//! Then open the address it prints (`http://127.0.0.1:7040/` by default). `--lazy` puts Core together on the first
//! call that needs it instead of at start; `--out` is where `POST /audit` writes the folder an outside checker is
//! handed (`<target>/rutent-demo-api` by default). `conformance/demo/README.md` lists the endpoints.
//!
//! Core is single-threaded, and so is the server: one connection at a time, read, answered and closed.

// Each binary uses part of what the two share.
#[allow(dead_code)]
#[path = "../../src/bin/demo-common/mod.rs"]
mod common;

mod api;
mod http;
mod world;

use std::net::TcpListener;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::{Value, json};

use api::{Api, Failure, Reply, fail, panicked};
use common::relaunch;
use http::{Request, Response};

/// The page.
const INDEX: &str = include_str!("web/index.html");
/// Its script.
const SCRIPT: &str = include_str!("web/app.js");
/// Its style.
const STYLE: &str = include_str!("web/style.css");

/// What the command line set.
struct Options {
    addr: String,
    seed: u64,
    out: PathBuf,
    lazy: bool,
    warm_up: bool,
    database_url: Option<String>,
    database_ca: Option<String>,
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        addr: "127.0.0.1:7040".to_owned(),
        seed: 1,
        out: relaunch::default_out("rutent-demo-api"),
        lazy: false,
        warm_up: false,
        database_url: std::env::var("DATABASE_URL").ok(),
        database_ca: std::env::var("DATABASE_CA_FILE").ok(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--addr" => options.addr = args.next().ok_or("--addr names HOST:PORT")?,
            "--out" => options.out = args.next().ok_or("--out names a folder")?.into(),
            "--seed" => {
                options.seed = args
                    .next()
                    .and_then(|s| s.parse().ok())
                    .ok_or("--seed is a whole number")?;
            }
            "--lazy" => options.lazy = true,
            "--warm-up" => options.warm_up = true,
            "--database-url" => {
                options.database_url = Some(args.next().ok_or("--database-url names a database")?)
            }
            "--database-ca" => {
                options.database_ca = Some(args.next().ok_or("--database-ca names a PEM file")?)
            }
            "-h" | "--help" => {
                return Err("usage: rutent-demo-api [--addr HOST:PORT] [--seed N] [--out DIR] [--lazy] [--warm-up]\n  \
                     [--database-url postgres://…] [--database-ca FILE]\n  see conformance/demo/README.md"
                    .to_owned());
            }
            other => return Err(format!("unknown argument {other}; see --help")),
        }
    }
    Ok(options)
}

fn main() -> ExitCode {
    relaunch::from_deps("rutent_demo_api");
    let options = match options() {
        Ok(options) => options,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::from(2);
        }
    };
    let store = match world::Store::open(
        options.database_url.as_deref(),
        options.database_ca.as_deref(),
    ) {
        Ok(store) => store,
        Err(why) => {
            eprintln!("the transitions store: {why}");
            return ExitCode::FAILURE;
        }
    };
    if options.warm_up {
        // Builds what Core needs on disk (the platform's executables, the simulated providers' modules) and puts
        // Core together once, then stops: a container image runs this at build time.
        let mut api = Api::new(options.seed, options.out, store);
        return match api.start() {
            Ok(()) => {
                println!("warmed up");
                ExitCode::SUCCESS
            }
            Err(failure) => {
                eprintln!(
                    "{}",
                    failure.body["error"]
                        .as_str()
                        .unwrap_or("Core couldn't start")
                );
                ExitCode::FAILURE
            }
        };
    }
    eprintln!("transitions are kept in {}", store.engine());
    let listener = match TcpListener::bind(&options.addr) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("couldn't listen on {}: {e}", options.addr);
            return ExitCode::FAILURE;
        }
    };
    let address = listener
        .local_addr()
        .map_or_else(|_| options.addr.clone(), |a| a.to_string());
    // The smoke test reads this line for the port.
    println!("rutent-demo-api listening on http://{address}/");
    let mut api = Api::new(options.seed, options.out, store);
    if !options.lazy
        && let Err(failure) = api.start()
    {
        eprintln!(
            "{}",
            failure.body["error"]
                .as_str()
                .unwrap_or("Core couldn't start")
        );
        return ExitCode::FAILURE;
    }
    println!("ready: open http://{address}/ (Ctrl-C stops it)");
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let request = match http::read(&stream) {
            Ok(Some(request)) => request,
            Ok(None) => continue,
            Err(e) => {
                let _ = http::write(
                    &stream,
                    &Response::json(400, &json!({ "error": e.to_string() })),
                );
                continue;
            }
        };
        let response = respond(&mut api, &request);
        if response.status >= 400 {
            eprintln!("{} {} → {}", request.method, request.path, response.status);
        }
        let _ = http::write(&stream, &response);
    }
    ExitCode::SUCCESS
}

/// Answers `request`: the page and its files, or an endpoint of the API. A panic in Core is answered as a 500 that
/// says so, and the server goes on; `POST /reset` puts a new Core together.
fn respond(api: &mut Api, request: &Request) -> Response {
    let segments = request.segments();
    match (request.method.as_str(), segments.as_slice()) {
        ("GET", [] | ["index.html"]) => page("text/html; charset=utf-8", INDEX),
        ("GET", ["app.js"]) => page("text/javascript; charset=utf-8", SCRIPT),
        ("GET", ["style.css"]) => page("text/css; charset=utf-8", STYLE),
        ("GET", ["favicon.ico"]) => Response {
            status: 404,
            content_type: "text/plain",
            body: Vec::new(),
        },
        ("GET", ["health"]) => {
            let (status, body) = api.health();
            Response::json(status, &body)
        }
        _ => {
            let answered = std::panic::catch_unwind(AssertUnwindSafe(|| {
                let reply = endpoint(api, request);
                // What the call made the simulated network do is recorded before the answer goes back.
                api.sync();
                reply
            }));
            let reply = answered.unwrap_or_else(|p| {
                Err(fail(
                    500,
                    format!(
                        "Core panicked: {}. Reset Core (POST /reset) to go on from a clean state.",
                        panicked(&p)
                    ),
                ))
            });
            match reply {
                Ok(value) => Response::json(200, &value),
                Err(Failure { status, body }) => Response::json(status, &body),
            }
        }
    }
}

fn page(content_type: &'static str, text: &str) -> Response {
    Response {
        status: 200,
        content_type,
        body: text.as_bytes().to_vec(),
    }
}

/// The API's endpoints.
fn endpoint(api: &mut Api, request: &Request) -> Reply {
    let body: Value = if request.body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&request.body)
            .map_err(|e| fail(400, format!("the body isn't JSON: {e}")))?
    };
    let segments = request.segments();
    match (request.method.as_str(), segments.as_slice()) {
        ("GET", ["network"]) => Ok(Api::network()),
        ("GET", ["status"]) => Ok(api.status()),
        ("GET", ["log"]) => {
            let since = request
                .query
                .get("since")
                .map(|s| s.parse().map_err(|_| fail(400, "since is a whole number")))
                .transpose()?
                .unwrap_or(0);
            api.log(since)
        }
        ("POST", ["reset"]) => api.reset(&body),
        ("POST", ["faults"]) => api.faults(&body),
        ("POST", ["quotes"]) => api.quote(&body),
        ("POST", ["holding"]) => api.holding(),
        ("POST", ["notices"]) => api.notices(),
        ("POST", ["audit"]) => api.audit(),
        ("GET", ["world", "runs"]) => api.runs(),
        ("GET", ["world", "transitions"]) => {
            let number = |key: &str, default: i64| {
                request
                    .query
                    .get(key)
                    .map(|s| {
                        s.parse()
                            .map_err(|_| fail(400, format!("{key} is a whole number")))
                    })
                    .transpose()
                    .map(|n| n.unwrap_or(default))
            };
            let since = number("since", 0)?;
            let limit = number("limit", 500)?;
            api.transitions(since, request.query.get("run").map(String::as_str), limit)
        }
        ("POST", ["world", "transitions"]) => api.post_transitions(&body),
        ("GET", ["orders", order]) => api.order(order),
        ("POST", ["orders", order, action]) => match *action {
            "accept" => api.accept(order),
            "run" => api.run(order),
            "reversal" => api.reversal(order),
            "recalculate" => api.recalculate(order),
            "stuck" => api.stuck(order),
            "linked" => api.linked(order),
            "decide" => api.decide(order, &body),
            "move" => api.move_stuck(order, &body),
            other => Err(fail(404, format!("no action {other} on an order"))),
        },
        (
            _,
            [
                "network" | "status" | "log" | "reset" | "faults" | "quotes" | "holding"
                | "notices" | "audit",
            ],
        ) => Err(fail(
            405,
            format!("{} isn't allowed on {}", request.method, request.path),
        )),
        _ => Err(fail(
            404,
            format!("no endpoint {} {}", request.method, request.path),
        )),
    }
}

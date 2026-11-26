//! A smoke test of the demo's API (`rutent-demo-api`, `conformance/demo/README.md`): the server starts, serves its
//! page, and `GET /network` answers with the graph the providers' shapes make. Core is left lazy (`--lazy`), so the
//! test needs neither the simulated providers' build nor the minute it takes to admit them.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};

use serde_json::Value;

/// Stops the server however the test ends.
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `GET path` on `address`: the status code and the body.
fn get(address: &str, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(address).expect("the server accepts a connection");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .expect("the request is sent");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("the response is read");
    let (head, body) = response.split_once("\r\n\r\n").expect("a head and a body");
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("a status code");
    (status, body.to_owned())
}

#[test]
fn the_server_starts_and_answers_with_the_network() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rutent-demo-api"))
        .args(["--addr", "127.0.0.1:0", "--lazy"])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the server starts");
    let stdout = child.stdout.take().expect("its output");
    let server = Server(child);
    let mut lines = BufReader::new(stdout).lines();
    let address = lines
        .by_ref()
        .map_while(Result::ok)
        .find_map(|l| {
            l.strip_prefix("rutent-demo-api listening on http://")
                .map(|a| a.trim_end_matches('/').to_owned())
        })
        .expect("the server says where it listens");
    // The rest of its output is drained, so it never blocks on a full pipe.
    std::thread::spawn(move || lines.for_each(drop));

    let (status, body) = get(&address, "/network");
    assert_eq!(status, 200, "{body}");
    let network: Value = serde_json::from_str(&body).expect("the network is JSON");
    let nodes = network["nodes"].as_array().expect("nodes");
    let edges = network["edges"].as_array().expect("edges");
    assert_eq!(network["providers"], 18);
    assert_eq!(nodes.len(), 10, "{body}");
    assert_eq!(edges.len(), 23, "{body}");
    let named = |n: &Value| nodes.iter().any(|x| x["id"] == *n);
    assert!(
        edges.iter().all(|e| named(&e["from"]) && named(&e["to"])),
        "every edge joins two of the nodes"
    );
    let loops = edges.iter().filter(|e| e["from"] == e["to"]).count();
    assert!(
        loops >= 1 && edges.iter().filter(|e| e["does"] == "exchange").count() == loops,
        "every exchange, and only an exchange, is a loop"
    );
    let bridges = edges
        .iter()
        .filter(|e| e["from"] == "sim-polygon" && e["to"] == "sim-base")
        .count();
    assert!(
        bridges >= 2,
        "two bridges run in parallel from sim-polygon to sim-base"
    );

    let (status, page) = get(&address, "/");
    assert_eq!(status, 200);
    assert!(page.contains("cytoscape"), "the page draws the graph");
    let (status, script) = get(&address, "/app.js");
    assert_eq!(status, 200);
    assert!(
        script.contains("/network"),
        "the page reads the network from the API"
    );

    let (status, body) = get(&address, "/status");
    assert_eq!(status, 200);
    let status_body: Value = serde_json::from_str(&body).expect("the status is JSON");
    assert_eq!(status_body["ready"], false, "Core is still lazy");

    let (status, _) = get(&address, "/nowhere");
    assert_eq!(status, 404);
    drop(server);
}

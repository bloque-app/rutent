//! The demo's world, recorded: every transition of the network, a request an adapter (a provider) made of an outer
//! piece (its book at a bank or a ledger, a public chain) and how it came out, kept in a database the API owns.
//!
//! Every transition reaches the store through [`Store::append`], whoever sends it: the simulated network the API
//! runs in-process, read off its world after each call ([`from_delivery`]), or an adapter outside the process,
//! through `POST /world/transitions`. Nothing here knows the simulator's types: a transition is plain fields and the
//! request as JSON, so a real adapter can send the same.
//!
//! The store is PostgreSQL when the server is given a database URL (`DATABASE_URL`, `--database-url`), else memory.
//! `schema.sql`, beside this file, is the schema, applied at start. The pieces a transition reaches: `book` (a provider's
//! book), `chain` (a public network), `ledger` (a publisher's private one), `rail` (an outer network).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The schema, applied at start; idempotent.
pub const SCHEMA: &str = include_str!("schema.sql");

/// One transition, as the store keeps it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Transition {
    /// The run it belongs to; the store sets it.
    pub run: String,
    /// Its place in the run, from 0; the store sets it.
    pub seq: i64,
    /// When the store took it, RFC 3339; the store sets it.
    pub recorded_at: Option<String>,
    /// Core's clock when the API recorded it, RFC 3339.
    pub world_at: Option<String>,
    /// Who sent it: `simulator`, or the name an adapter gives itself.
    pub source: String,
    /// The outer piece it reached: `book`, `chain`, or what an adapter names.
    pub piece: String,
    /// What it was: withdrawal, deposit, transfer, conversion, …, or `movement`.
    pub kind: String,
    /// The provider that made it.
    pub provider: Option<String>,
    /// The public network it was on, for a movement.
    pub network: Option<String>,
    /// The order it served, where the API knows it.
    pub order_id: Option<String>,
    /// The segment it served, where the API knows it.
    pub segment_id: Option<String>,
    /// The transaction it ran under.
    pub transaction_id: Option<String>,
    /// The node value left.
    pub from_node: Option<String>,
    /// The node value reached.
    pub to_node: Option<String>,
    /// The asset taken in.
    pub asset_in: Option<String>,
    /// The asset given out.
    pub asset_out: Option<String>,
    /// The amount taken in, in the asset's smallest unit.
    pub amount_in: Option<String>,
    /// The amount given out, where it is known.
    pub amount_out: Option<String>,
    /// Whether it was a dry run, which moves nothing.
    pub dry_run: bool,
    /// How it came out.
    pub outcome: String,
    /// The request as the piece received it.
    pub detail: Value,
}

impl Transition {
    /// Why it can't be stored, if it can't: the fields every transition names.
    pub fn missing(&self) -> Option<&'static str> {
        if self.source.is_empty() {
            Some("source")
        } else if self.piece.is_empty() {
            Some("piece")
        } else if self.kind.is_empty() {
            Some("kind")
        } else if self.outcome.is_empty() {
            Some("outcome")
        } else {
            None
        }
    }
}

/// Where transitions are kept.
pub enum Store {
    /// In this process, gone when it stops.
    Memory(Vec<Transition>, Vec<Value>),
    /// A PostgreSQL database.
    Postgres(Box<postgres::Client>),
}

impl Store {
    /// The store `url` names, the schema applied: memory without one. `ca_file` is a PEM bundle of the
    /// certificate authorities the database's certificate is checked against; with it the connection is TLS (rustls),
    /// without it plain, which `sslmode=require` in the URL then refuses.
    ///
    /// # Errors
    ///
    /// If the URL doesn't read, the database can't be reached, or the schema can't be applied.
    pub fn open(url: Option<&str>, ca_file: Option<&str>) -> Result<Self, String> {
        let Some(url) = url.filter(|u| !u.is_empty()) else {
            return Ok(Self::Memory(Vec::new(), Vec::new()));
        };
        let config: postgres::Config = url
            .parse()
            .map_err(|e| format!("the database URL doesn't read: {e}"))?;
        let mut client = match ca_file.filter(|f| !f.is_empty()) {
            Some(path) => {
                let tls = tls(path)?;
                config.connect(tls)
            }
            None => config.connect(postgres::NoTls),
        }
        .map_err(|e| format!("the database can't be reached: {e}"))?;
        client
            .batch_execute(SCHEMA)
            .map_err(|e| format!("the schema wasn't applied: {e}"))?;
        Ok(Self::Postgres(Box::new(client)))
    }

    /// `memory` or `postgres`.
    pub fn engine(&self) -> &'static str {
        match self {
            Self::Memory(..) => "memory",
            Self::Postgres(_) => "postgres",
        }
    }

    /// Whether the store answers.
    ///
    /// # Errors
    ///
    /// If the database doesn't.
    pub fn check(&mut self) -> Result<(), String> {
        match self {
            Self::Memory(..) => Ok(()),
            Self::Postgres(client) => client
                .simple_query("SELECT 1")
                .map(drop)
                .map_err(|e| e.to_string()),
        }
    }

    /// Starts a run: one Core, from `seed`, on `network`.
    ///
    /// # Errors
    ///
    /// If the database refuses it.
    pub fn begin(&mut self, run: &str, seed: u64, network: &Value) -> Result<(), String> {
        match self {
            Self::Memory(_, runs) => {
                runs.push(json!({ "run": run, "seed": seed, "started_at": now() }));
                Ok(())
            }
            Self::Postgres(client) => client
                .execute(
                    "INSERT INTO demo_runs (run, seed, network) VALUES ($1, $2, $3) ON CONFLICT (run) DO NOTHING",
                    &[&run, &i64::try_from(seed).unwrap_or(i64::MAX), network],
                )
                .map(drop)
                .map_err(|e| e.to_string()),
        }
    }

    /// Appends `transitions` to `run` from place `first` on, in one transaction: all of them or none.
    ///
    /// # Errors
    ///
    /// If the database refuses them; then none is kept.
    pub fn append(
        &mut self,
        run: &str,
        first: i64,
        transitions: &mut [Transition],
    ) -> Result<(), String> {
        for (n, t) in transitions.iter_mut().enumerate() {
            t.run = run.to_owned();
            t.seq = first + i64::try_from(n).unwrap_or(i64::MAX);
        }
        match self {
            Self::Memory(kept, _) => {
                for t in transitions.iter_mut() {
                    t.recorded_at = Some(now());
                }
                kept.extend(transitions.iter().cloned());
                Ok(())
            }
            Self::Postgres(client) => {
                let mut tx = client.transaction().map_err(|e| e.to_string())?;
                for t in transitions.iter() {
                    tx.execute(
                        "INSERT INTO demo_transitions (run, seq, world_at, source, piece, kind, provider, network, \
                         order_id, segment_id, transaction_id, from_node, to_node, asset_in, asset_out, amount_in, \
                         amount_out, dry_run, outcome, detail) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20)",
                        &[
                            &t.run, &t.seq, &t.world_at, &t.source, &t.piece, &t.kind, &t.provider, &t.network,
                            &t.order_id, &t.segment_id, &t.transaction_id, &t.from_node, &t.to_node, &t.asset_in,
                            &t.asset_out, &t.amount_in, &t.amount_out, &t.dry_run, &t.outcome, &t.detail,
                        ],
                    )
                    .map_err(|e| e.to_string())?;
                }
                tx.commit().map_err(|e| e.to_string())
            }
        }
    }

    /// `run`'s transitions from place `since` on, at most `limit`.
    ///
    /// # Errors
    ///
    /// If the database refuses the read.
    pub fn since(&mut self, run: &str, since: i64, limit: i64) -> Result<Vec<Transition>, String> {
        match self {
            Self::Memory(kept, _) => Ok(kept
                .iter()
                .filter(|t| t.run == run && t.seq >= since)
                .take(usize::try_from(limit).unwrap_or(usize::MAX))
                .cloned()
                .collect()),
            Self::Postgres(client) => {
                let rows = client
                    .query(
                        "SELECT run, seq, recorded_at::text, world_at, source, piece, kind, provider, network, \
                         order_id, segment_id, transaction_id, from_node, to_node, asset_in, asset_out, amount_in, \
                         amount_out, dry_run, outcome, detail \
                         FROM demo_transitions WHERE run = $1 AND seq >= $2 ORDER BY seq LIMIT $3",
                        &[&run, &since, &limit],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(rows
                    .iter()
                    .map(|r| Transition {
                        run: r.get(0),
                        seq: r.get(1),
                        recorded_at: r.get(2),
                        world_at: r.get(3),
                        source: r.get(4),
                        piece: r.get(5),
                        kind: r.get(6),
                        provider: r.get(7),
                        network: r.get(8),
                        order_id: r.get(9),
                        segment_id: r.get(10),
                        transaction_id: r.get(11),
                        from_node: r.get(12),
                        to_node: r.get(13),
                        asset_in: r.get(14),
                        asset_out: r.get(15),
                        amount_in: r.get(16),
                        amount_out: r.get(17),
                        dry_run: r.get(18),
                        outcome: r.get(19),
                        detail: r.get(20),
                    })
                    .collect())
            }
        }
    }

    /// The runs the store holds, newest first, with how many transitions each has.
    ///
    /// # Errors
    ///
    /// If the database refuses the read.
    pub fn runs(&mut self) -> Result<Vec<Value>, String> {
        match self {
            Self::Memory(kept, runs) => Ok(runs
                .iter()
                .rev()
                .map(|r| {
                    let run = r["run"].as_str().unwrap_or_default();
                    let mut r = r.clone();
                    r["transitions"] = json!(kept.iter().filter(|t| t.run == run).count());
                    r
                })
                .collect()),
            Self::Postgres(client) => {
                let rows = client
                    .query(
                        "SELECT r.run, r.seed, r.started_at::text, \
                         (SELECT count(*) FROM demo_transitions t WHERE t.run = r.run) \
                         FROM demo_runs r ORDER BY r.started_at DESC LIMIT 100",
                        &[],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(rows
                    .iter()
                    .map(|r| {
                        json!({
                            "run": r.get::<_, String>(0),
                            "seed": r.get::<_, i64>(1),
                            "started_at": r.get::<_, String>(2),
                            "transitions": r.get::<_, i64>(3),
                        })
                    })
                    .collect())
            }
        }
    }
}

/// A TLS connector that checks the database's certificate against the authorities in the PEM file at `path`.
fn tls(path: &str) -> Result<tokio_postgres_rustls::MakeRustlsConnect, String> {
    use rustls_pki_types::CertificateDer;
    use rustls_pki_types::pem::PemObject as _;
    let mut roots = rustls::RootCertStore::empty();
    for cert in
        CertificateDer::pem_file_iter(path).map_err(|e| format!("the CA bundle {path}: {e}"))?
    {
        let cert = cert.map_err(|e| format!("the CA bundle {path}: {e}"))?;
        roots
            .add(cert)
            .map_err(|e| format!("the CA bundle {path}: {e}"))?;
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(tokio_postgres_rustls::MakeRustlsConnect::new(config))
}

/// Now, by the wall clock, RFC 3339.
pub fn now() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    rutent_prov::catalogue::Instant(seconds)
        .rfc3339()
        .unwrap_or_default()
}

/// The name of a node as an operation names it: an outer node's network, or an inner node's identifier.
fn node(value: &Value) -> Option<String> {
    value["network"]
        .as_str()
        .or_else(|| value.as_str())
        .map(str::to_owned)
}

/// The first of `keys` `value` names as text.
fn text(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| value[*k].as_str())
        .map(str::to_owned)
}

/// What the simulated network's world knows about who is at a host.
pub struct Hosts {
    /// Each book's host, and the provider whose book it is.
    pub books: BTreeMap<String, String>,
    /// Each network's host: the network, and the piece it is: `chain` for a public network, `ledger` for a
    /// publisher's private one, `rail` for an outer one (a bank's rail, a chain's addresses).
    pub chains: BTreeMap<String, (String, &'static str)>,
}

/// The transition one request to an outer piece makes: `host` received `body`. `refused` names the movements the
/// chains refused, by network and id, and `served` the order and segment each transaction served.
pub fn from_delivery(
    host: &str,
    body: &str,
    hosts: &Hosts,
    refused: &[(String, String, String)],
    served: &BTreeMap<String, (String, String)>,
) -> Transition {
    let detail: Value = serde_json::from_str(body).unwrap_or_else(|_| Value::from(body));
    let mut t = Transition {
        source: "simulator".to_owned(),
        detail: detail.clone(),
        ..Transition::default()
    };
    if let Some(provider) = hosts.books.get(host) {
        // A provider's book entry: the operation it was asked, as Core signed it, and how it answered.
        let op = &detail["operation"]["statement"];
        t.piece = "book".to_owned();
        t.kind = text(op, &["kind"]).unwrap_or_else(|| "entry".to_owned());
        t.provider = text(op, &["provider"]).or_else(|| Some(provider.clone()));
        t.transaction_id = text(op, &["transaction"]);
        if t.kind == "conversion" {
            // A conversion stays at its node and names the assets it converts between as `from` and `to`.
            t.from_node = node(&op["node"]);
            t.to_node = t.from_node.clone();
            t.asset_in = text(op, &["from"]);
            t.asset_out = text(op, &["to"]);
        } else {
            t.from_node = ["origin", "from", "node"]
                .iter()
                .find_map(|k| node(&op[*k]));
            t.to_node = ["destination", "to", "node"]
                .iter()
                .find_map(|k| node(&op[*k]));
            t.asset_in = text(op, &["asset"]);
            t.asset_out = t.asset_in.clone();
        }
        t.amount_in = text(op, &["amount"]);
        t.dry_run = op["dry_run"].as_bool().unwrap_or(false);
        let act = detail["behaviour"]["act"].as_str().unwrap_or("answer");
        t.outcome = match act {
            "answer" => "answered".to_owned(),
            "fail" => "failed".to_owned(),
            other => other.to_owned(),
        };
    } else if let Some((network, piece)) = hosts.chains.get(host) {
        // A movement on a network: a public chain, or an outer network such as a bank's rail.
        t.piece = (*piece).to_owned();
        t.kind = "movement".to_owned();
        t.network = Some(network.clone());
        t.transaction_id = text(&detail, &["id"]);
        t.from_node = Some(network.clone());
        t.to_node = text(&detail, &["destination"]);
        t.asset_in = text(&detail, &["asset"]);
        t.asset_out = t.asset_in.clone();
        t.amount_in = text(&detail, &["amount"]);
        t.amount_out = t.amount_in.clone();
        let id = t.transaction_id.clone().unwrap_or_default();
        t.outcome = refused
            .iter()
            .find(|(n, m, _)| n == network && *m == id)
            .map_or_else(
                || "recorded".to_owned(),
                |(_, _, why)| format!("refused: {why}"),
            );
    } else {
        t.piece = "other".to_owned();
        t.kind = "request".to_owned();
        t.to_node = Some(host.to_owned());
        t.outcome = "delivered".to_owned();
    }
    // A reversal serves the order of the transaction it reverses.
    let reverses = text(&detail["operation"]["statement"], &["reverses"]);
    if let Some(tx) = t.transaction_id.as_deref() {
        // A movement's id is its operation's transaction, or that and the piece's number.
        let found = served
            .get(tx)
            .or_else(|| {
                served
                    .iter()
                    .find(|(known, _)| tx.starts_with(known.as_str()))
                    .map(|(_, v)| v)
            })
            .or_else(|| reverses.as_ref().and_then(|r| served.get(r)));
        if let Some((order, segment)) = found {
            t.order_id = Some(order.clone());
            t.segment_id = Some(segment.clone());
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts() -> Hosts {
        Hosts {
            books: BTreeMap::from([("rail.sim-a".to_owned(), "sim-a".to_owned())]),
            chains: BTreeMap::from([(
                "chain.sim-polygon".to_owned(),
                ("sim-polygon".to_owned(), "chain"),
            )]),
        }
    }

    #[test]
    fn a_book_entry_is_an_operation_and_its_outcome() {
        let body = r#"{"operation":{"statement":{"kind":"withdrawal","transaction":"tx-1","provider":"sim-a",
            "origin":{"network":"sim-co-bank"},"destination":"sim-a-ledger","asset":"COP","amount":"500",
            "dry_run":false}},"behaviour":{"act":"fail"}}"#;
        let served = BTreeMap::from([("tx-1".to_owned(), ("o-1".to_owned(), "s-1".to_owned()))]);
        let t = from_delivery("rail.sim-a", body, &hosts(), &[], &served);
        assert_eq!(
            (t.piece.as_str(), t.kind.as_str(), t.outcome.as_str()),
            ("book", "withdrawal", "failed")
        );
        assert_eq!(t.from_node.as_deref(), Some("sim-co-bank"));
        assert_eq!(t.to_node.as_deref(), Some("sim-a-ledger"));
        assert_eq!(t.order_id.as_deref(), Some("o-1"));
        assert_eq!(t.missing(), None);
    }

    #[test]
    fn a_conversion_stays_at_its_node_and_a_reversal_serves_the_order_it_reverses() {
        let served = BTreeMap::from([("tx-1".to_owned(), ("o-1".to_owned(), "s-2".to_owned()))]);
        let body = r#"{"operation":{"statement":{"kind":"conversion","node":"sim-a-ledger","from":"COP","to":"USDC",
            "amount":"500","transaction":"tx-1","provider":"sim-a"}},"behaviour":{"act":"answer"}}"#;
        let t = from_delivery("rail.sim-a", body, &hosts(), &[], &served);
        assert_eq!(
            (t.from_node.as_deref(), t.to_node.as_deref()),
            (Some("sim-a-ledger"), Some("sim-a-ledger"))
        );
        assert_eq!(
            (t.asset_in.as_deref(), t.asset_out.as_deref()),
            (Some("COP"), Some("USDC"))
        );
        let body = r#"{"operation":{"statement":{"kind":"reversal","node":"sim-a-ledger","asset":"COP","amount":"500",
            "reverses":"tx-1","transaction":"tx-9","provider":"sim-a"}},"behaviour":{"act":"answer"}}"#;
        let t = from_delivery("rail.sim-a", body, &hosts(), &[], &served);
        assert_eq!(t.order_id.as_deref(), Some("o-1"));
    }

    #[test]
    fn a_movement_is_recorded_or_refused_on_its_chain() {
        let body = r#"{"network":"sim-polygon","id":"tx-1-0","destination":"0xabc","asset":"USDC","amount":"9"}"#;
        let refused = vec![(
            "sim-polygon".to_owned(),
            "tx-1-0".to_owned(),
            "Duplicate".to_owned(),
        )];
        let t = from_delivery(
            "chain.sim-polygon",
            body,
            &hosts(),
            &refused,
            &BTreeMap::new(),
        );
        assert_eq!(t.piece, "chain");
        assert_eq!(t.outcome, "refused: Duplicate");
        let t = from_delivery("chain.sim-polygon", body, &hosts(), &[], &BTreeMap::new());
        assert_eq!(t.outcome, "recorded");
    }

    #[test]
    fn the_memory_store_numbers_and_reads_back() {
        let mut store = Store::open(None, None).expect("memory");
        store.begin("r", 1, &json!({})).expect("a run");
        let mut batch = vec![
            Transition {
                source: "a".into(),
                piece: "book".into(),
                kind: "deposit".into(),
                outcome: "answered".into(),
                ..Transition::default()
            };
            3
        ];
        store.append("r", 0, &mut batch).expect("kept");
        let read = store.since("r", 1, 10).expect("read");
        assert_eq!(read.iter().map(|t| t.seq).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(store.runs().expect("runs")[0]["transitions"], 3);
    }
}

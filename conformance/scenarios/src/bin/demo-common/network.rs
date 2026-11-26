//! The demo's network: the simulated providers, and the graph they make, read from their own shapes
//! (`rutent_conformance::sim::shape_of`) so the graph drawn is the network simulated.
//!
//! Every place value can rest is a node: an outer network (a bank rail, a chain's addresses), a private node (one
//! publisher's ledger) or a public node (a chain on the operator's list). Every tuple a provider serves is a directed
//! edge: a reserve's withdrawal from its outer network into its node and its deposit back, an exchange's conversion
//! inside its node (an edge from the node to itself), a ramp's transfer from one node to another.

use std::collections::BTreeSet;

use rutent_conformance::sim::{Role, Spec, shape_of};

/// The providers, by role and name: three publishers' banks, exchanges and ramps, two chains' address reserves, and
/// three bridges between the chains. Their names hash to distinct key seeds (`Spec::seed`).
pub const PROVIDERS: &[(Role, &str)] = &[
    (Role::Reserve, "sim-andina-cop"),
    (Role::Exchange, "sim-andina-exchange"),
    (Role::Exchange, "sim-andina-exchange-fast"),
    (Role::Ramp, "sim-andina-ramp-polygon"),
    (Role::Ramp, "sim-andina-base"),
    (Role::Reserve, "sim-azteca-mxn-desk"),
    (Role::Exchange, "sim-azteca-exchange"),
    (Role::Exchange, "sim-azteca-fx-in"),
    (Role::Ramp, "sim-azteca-base"),
    (Role::Ramp, "sim-azteca-in-polygon"),
    (Role::Reserve, "sim-liberty-usd"),
    (Role::Exchange, "sim-liberty-fx-in"),
    (Role::Ramp, "sim-liberty-in-base"),
    (Role::Ramp, "sim-bridge"),
    (Role::Ramp, "sim-bridge-fast"),
    (Role::Ramp, "sim-bridge-back"),
    (Role::Reserve, "sim-polygon-wallets"),
    (Role::Reserve, "sim-base-wallets"),
];

/// The providers' specs.
pub fn specs() -> Vec<Spec> {
    PROVIDERS
        .iter()
        .map(|(role, name)| Spec::of(*role, name))
        .collect()
}

/// What kind of place a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A network outside rutent where money starts or ends: a bank rail, a chain's addresses.
    Outer,
    /// One publisher's own ledger, seen only by its providers.
    Private,
    /// A shared ledger on the operator's list, where anyone can see what moved.
    Public,
}

impl Kind {
    /// Its name, as the demo prints it.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Outer => "outer",
            Kind::Private => "private",
            Kind::Public => "public",
        }
    }
}

/// One edge: a tuple a provider serves.
#[derive(Debug, Clone)]
pub struct Edge {
    /// The provider.
    pub provider: String,
    /// What it does: withdrawal, deposit, exchange or ramp.
    pub does: &'static str,
    /// The node it takes value from.
    pub from: String,
    /// The node it gives value to.
    pub to: String,
    /// The asset it takes in.
    pub asset_in: String,
    /// The asset it gives out.
    pub asset_out: String,
    /// Its fee in the asset it takes in, in that asset's smallest unit.
    pub fee: u128,
    /// The seconds it declares to settle, where its shape sets them.
    pub seconds: Option<u64>,
}

impl Edge {
    /// Its asset, or its conversion.
    pub fn assets(&self) -> String {
        if self.asset_in == self.asset_out {
            self.asset_in.clone()
        } else {
            format!("{} → {}", self.asset_in, self.asset_out)
        }
    }
}

/// Every edge of the network, provider by provider.
pub fn edges() -> Vec<Edge> {
    let mut edges = Vec::new();
    for spec in specs() {
        let shape = shape_of(&spec.provider);
        let asset = shape.asset.clone().unwrap_or_else(|| "?".to_owned());
        let edge = |does, from: String, to: String, asset_out: String| Edge {
            provider: spec.provider.clone(),
            does,
            from,
            to,
            asset_in: asset.clone(),
            asset_out,
            fee: shape.fee,
            seconds: shape.settles_in,
        };
        match spec.role {
            Role::Reserve => {
                edges.push(edge(
                    "withdrawal",
                    spec.bank(),
                    spec.ledger(),
                    asset.clone(),
                ));
                edges.push(edge("deposit", spec.ledger(), spec.bank(), asset.clone()));
            }
            Role::Exchange => {
                let node = shape.node.clone().unwrap_or_else(|| spec.ledger());
                let out = shape.to_asset.clone().unwrap_or_else(|| "?".to_owned());
                edges.push(edge("exchange", node.clone(), node, out));
            }
            Role::Ramp => {
                let from = shape.node.clone().unwrap_or_else(|| spec.ledger());
                let to = shape.outer.clone().unwrap_or_default();
                edges.push(edge("ramp", from, to, asset.clone()));
            }
            Role::Verifier => {}
        }
    }
    edges
}

/// The public nodes the providers reach, on the operator's list.
pub fn public_nodes() -> BTreeSet<String> {
    specs().iter().flat_map(Spec::public_nodes).collect()
}

/// The private nodes the providers' publishers declare.
pub fn private_nodes() -> BTreeSet<String> {
    specs().iter().flat_map(Spec::private_nodes).collect()
}

/// Every node of the network, in name order, with its kind.
pub fn nodes() -> Vec<(String, Kind)> {
    let public = public_nodes();
    let private = private_nodes();
    let mut all: BTreeSet<String> = BTreeSet::new();
    for e in edges() {
        all.insert(e.from);
        all.insert(e.to);
    }
    all.into_iter()
        .map(|n| {
            let kind = if public.contains(&n) {
                Kind::Public
            } else if private.contains(&n) {
                Kind::Private
            } else {
                Kind::Outer
            };
            (n, kind)
        })
        .collect()
}

/// The network as a Graphviz graph: each node a vertex, shaped and coloured by its kind, and each tuple a directed
/// edge labelled with its provider and what it moves; `holder` is drawn holding the public node `held`.
pub fn dot(holder: &str, held: &str) -> String {
    let mut dot = String::from(
        "// The rutent demo's network (conformance/demo/README.md). Render it with `dot -Tsvg network.dot > network.svg`.\n\
         digraph network {\n  layout=neato; overlap=false; splines=true; sep=\"+12\";\n  \
         node [fontname=\"Helvetica\", fontsize=11, style=filled];\n  edge [fontname=\"Helvetica\", fontsize=9];\n",
    );
    for (node, kind) in nodes() {
        let (shape, fill, style) = match kind {
            Kind::Public => ("ellipse", "#E1F5EE", "filled"),
            Kind::Private => ("ellipse", "#EEEDFE", "\"filled,dashed\""),
            Kind::Outer => ("box", "#F1EFE8", "\"filled,rounded\""),
        };
        dot.push_str(&format!(
            "  \"{node}\" [label=\"{node}\\n({})\", shape={shape}, style={style}, fillcolor=\"{fill}\"];\n",
            kind.name()
        ));
    }
    for e in edges() {
        dot.push_str(&format!(
            "  \"{}\" -> \"{}\" [label=\"{}\\n{} · {}\"];\n",
            e.from,
            e.to,
            e.provider,
            e.does,
            e.assets()
        ));
    }
    dot.push_str(&format!(
        "  \"{holder}\" [label=\"{holder}\\n(holding account)\", shape=note, fillcolor=\"#F1EFE8\"];\n  \
         \"{holder}\" -> \"{held}\" [style=dashed, arrowhead=none, label=\"holds\"];\n}}\n"
    ));
    dot
}

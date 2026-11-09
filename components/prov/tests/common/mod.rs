//! Shared helpers for `CMP-PROV`'s tests: the contract's valid advertisements as the catalogue meets them, and a
//! small seeded generator for the property tests (`DEC-18`), so a failing case replays from its seed.

#![allow(dead_code)]

use std::path::Path;

use rutent_contracts::provider_role::{Advertisement, Statement};
use rutent_prov::catalogue::{
    Bond, Category, Grant, GrantTerms, Instant, ReversalCapacity, TupleKey,
};

/// The statement inside one of provider-role's valid advertisement fixtures.
pub fn advertisement(fixture: &str) -> Advertisement {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/provider-role/fixtures/valid")
        .join(format!("{fixture}.json"));
    let text = std::fs::read_to_string(&path).expect("fixture is readable");
    let statement: Statement<Advertisement> = serde_json::from_str(&text).expect("fixture parses");
    statement.statement
}

/// The ramp provider with a public and a private node: publisher `acme`, provider `bridge-pay`.
pub fn ramp_advertisement() -> Advertisement {
    advertisement("advertisement-ramp-between-a-public-and-a-private-node")
}

/// The reserve provider that starts routes on automatic deposits.
pub fn reserve_advertisement() -> Advertisement {
    advertisement("advertisement-reserve-starting-routes-on-automatic-deposits")
}

/// A bond in `USDC` held by a third party.
pub fn bond(amount: u64) -> Bond {
    Bond {
        asset: "USDC".into(),
        amount,
        held_by: "escrow-co".into(),
    }
}

/// A recorded reversal-capacity check, for a ceiling at most the bond.
pub fn capacity(bond_amount: u64, ceiling: u64) -> ReversalCapacity {
    ReversalCapacity::check(bond(bond_amount), ceiling, "ops-lead", Instant(1_000))
        .expect("ceiling within bond")
}

/// A grant of `provider` to `party` for `tuples`, in force from 1 000 to `expires_at`.
pub fn grant(id: &str, provider: &str, tuples: Vec<TupleKey>, expires_at: u64) -> Grant {
    let mut categories: Vec<Category> = Vec::new();
    for t in &tuples {
        if !categories.contains(&t.category) {
            categories.push(t.category);
        }
    }
    Grant::new(GrantTerms {
        id: id.into(),
        provider: provider.into(),
        party: "Acme Payments Ltd".into(),
        granted_by: "operator-admin".into(),
        granted_at: Instant(1_000),
        expires_at: Instant(expires_at),
        categories,
        tuples,
        reversal_capacity: capacity(1_000, 500),
    })
    .expect("grant is well formed")
}

/// A deterministic generator (xorshift64*), seeded per case.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value below `n`.
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    pub fn chance(&mut self, one_in: u64) -> bool {
        self.below(one_in) == 0
    }
}

/// How many seeded cases each property runs.
pub const CASES: u64 = 400;

use rutent_contracts::provider_role::EvidenceGrade;
use rutent_contracts::router_api::Version;
use rutent_prov::catalogue::{Catalogue, tuples_of};

/// The exchange provider `dex-pool`, publisher `acme`.
pub fn exchange_advertisement() -> Advertisement {
    advertisement("advertisement-exchange-within-one-node")
}

/// The ramp between two private nodes of `acme`.
pub fn internal_rail_advertisement() -> Advertisement {
    advertisement("advertisement-ramp-between-two-private-nodes-of-one-publisher")
}

/// When the fixtures' catalogues join their advertisements.
pub const JOINED_AT: Instant = Instant(1_500);

/// A catalogue of `acme`'s advertisements, each with a grant, a module version, an evidence floor of grade 4 (the
/// weakest) on every tuple, `polygon` on the public list with a holding account, and `acme-ledger` with the
/// publisher's. Only what the caller leaves out of this is missing.
pub fn acme_catalogue(ads: &[Advertisement]) -> Catalogue {
    let mut catalogue = Catalogue::new();
    catalogue.add_public_node("polygon").unwrap();
    catalogue
        .register_public_holding("polygon", "wallet-module")
        .unwrap();
    for (i, ad) in ads.iter().enumerate() {
        let keys = tuples_of(ad);
        catalogue
            .grant(grant(
                &format!("g-{i}"),
                &ad.provider,
                keys.clone(),
                100_000,
            ))
            .unwrap();
        catalogue.join(ad, JOINED_AT).unwrap();
        catalogue.set_module_version(&ad.provider, Version::new("1.0.0").unwrap());
        for key in keys {
            catalogue.set_evidence_floor(key, EvidenceGrade::Statement);
        }
    }
    for node in ["acme-ledger", "acme-card-book"] {
        if catalogue.publishers().publisher_of_node(node).is_some() {
            catalogue
                .register_private_holding(node, "acme", "acme-record-interface")
                .unwrap();
        }
    }
    catalogue
}

/// The snapshot's figure origins as (provider, publisher).
pub fn origins(
    snapshot: &rutent_contracts::router_api::CatalogueSnapshot,
) -> Vec<(String, String)> {
    use rutent_contracts::router_api::Origin;
    snapshot
        .figures
        .iter()
        .filter_map(|f| match &f.origin {
            Origin::Provider {
                provider,
                publisher,
                ..
            } => Some((provider.to_string(), publisher.to_string())),
            Origin::Operator { .. } => None,
        })
        .collect()
}

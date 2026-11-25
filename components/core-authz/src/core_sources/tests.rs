//! Core's own answers fail closed, and the deployment is asked only what is its own (`T-020-55`).

use super::*;
use crate::parties::Scope;
use crate::roles::NO_USER_WITHDRAWAL;
use crate::roles::Permission;
use rutent_contracts::core_internal::{ActorKind, Role, ShowingNamed};

fn id(text: &str) -> Id {
    Id::new(text).expect("an identifier")
}

fn digest(byte: char) -> Digest {
    serde_json::from_value(serde_json::json!({
        "algorithm": "sha-256",
        "value": byte.to_string().repeat(64),
    }))
    .expect("a digest")
}

/// A log that holds nothing, or can't be read.
#[derive(Clone)]
struct Log(Option<Vec<(Receipt, Entry)>>);

impl LogEntries for Log {
    fn read<T>(&mut self, read: impl FnOnce(&[(Receipt, Entry)]) -> T) -> Result<T, ReadFailure> {
        self.0.as_deref().map(read).ok_or(ReadFailure::Unavailable)
    }
}

/// A key register that accepted nothing, or one that accepted `bank-co`'s advertisement under `commitment`, or one that
/// can't be read.
#[derive(Clone)]
enum Store {
    Accepted(Option<Digest>),
    Unreadable,
}

/// The test's readable register that accepted nothing.
#[allow(non_snake_case)]
fn Store(readable: bool) -> Store {
    if readable {
        Store::Accepted(None)
    } else {
        Store::Unreadable
    }
}

impl AcceptedAdvertisements for Store {
    fn accepted(&mut self, provider: &Id) -> Result<Option<AcceptedAdvertisement>, ReadFailure> {
        match self {
            Store::Unreadable => Err(ReadFailure::Unavailable),
            Store::Accepted(None) => Ok(None),
            Store::Accepted(Some(commitment)) => {
                Ok(
                    (provider.as_str() == "bank-co").then(|| AcceptedAdvertisement {
                        commitment: commitment.clone(),
                        declaration: Some(ReserveDeclaration {
                            can_require_user_initiated_withdrawal: false,
                            starts_routes_on_automatic_deposits: true,
                            maker: "bank-co".to_owned(),
                            declared_in_version: "1.0.0".to_owned(),
                        }),
                    }),
                )
            }
        }
    }
}

/// A deployment that says yes to whatever it is asked, and keeps what it was asked.
#[derive(Clone, Default)]
struct YesToAll(Rc<RefCell<Vec<Source>>>);

impl DeploymentRegisters for YesToAll {
    fn registered(&mut self, asked: &Registered<'_>) -> Reply<Said> {
        self.0.borrow_mut().push(asked.source().clone());
        Reply::Given(Said::Yes)
    }
}

/// Every source of Core's own, each once.
fn cores() -> Vec<Source> {
    vec![
        Source::Requester(id("ord-1")),
        Source::OrderOrigin(id("ord-1")),
        Source::OrderParties(id("ord-1")),
        Source::SegmentOrder(id("seg-1")),
        Source::SegmentProvider {
            order: id("ord-1"),
            segment: id("seg-1"),
        },
        Source::Statement(digest('a')),
        Source::Showing {
            order: id("ord-1"),
            segment: id("seg-1"),
            showing: ShowingNamed {
                transaction: id("tx-1"),
                commitment: digest('b'),
            },
            decided_at: Timestamp::new("2026-01-01T00:00:00Z").expect("a time"),
        },
        Source::Declarations(id("bank-co")),
        Source::Kept(digest('c')),
        Source::KeptAbout(digest('d')),
    ]
}

/// Every source of the deployment's, each once.
fn deployments() -> Vec<Source> {
    vec![
        Source::ActsFor {
            acts: id("user-1"),
            party: id("cust-1"),
            kind: ActorKind::Person,
            role: Role::Customer,
        },
        Source::AgentScope {
            party: id("cust-1"),
            scope: Scope::CheckRead,
        },
        Source::Permissions {
            party: id("operator"),
            permission: Permission::ReadOrderState,
        },
        Source::Account(id("acct-1")),
        Source::Holder {
            account: id("acct-1"),
            holder: id("cust-1"),
        },
        Source::Reaches {
            from: id("user-1"),
            to: id("cust-1"),
        },
        Source::MeetsBelowOperator {
            from: id("user-1"),
            with: id("user-2"),
        },
        Source::OfOperator,
        Source::Verifier(id("verifier")),
        Source::Provider(id("bank-co")),
        Source::Consent {
            person: id("person-1"),
            consent: digest('e'),
        },
        Source::HoldingAccount {
            node: "ledger-in".to_owned(),
        },
    ]
}

fn need(from: Source) -> Need {
    Need {
        input: id("cust-1"),
        from,
    }
}

/// The deployment answers every source of its own, as it is, and never one of Core's: a deployment saying yes to
/// everything moves none of Core's answers, which an empty log and store leave as no or nothing, never yes.
// verifies: REQ-AUZ-5
#[test]
fn the_deployment_is_asked_only_its_own_registers() {
    let asked = YesToAll::default();
    let mut sources = CoreSources::new(Log(Some(Vec::new())), Store(true), asked.clone());
    for source in cores() {
        assert!(!is_the_deployments(&source), "{source:?}");
        let answer = sources.supply(&need(source.clone()));
        assert!(
            matches!(answer, Reply::Missing | Reply::Given(Said::No)),
            "{source:?}: {answer:?}"
        );
    }
    assert!(asked.0.borrow().is_empty(), "{:?}", asked.0.borrow());
    for source in deployments() {
        assert!(is_the_deployments(&source), "{source:?}");
        assert_eq!(sources.supply(&need(source)), Reply::Given(Said::Yes));
    }
    assert_eq!(*asked.0.borrow(), deployments());
    assert_eq!(sources.order_record(&id("ord-1")), Reply::Missing);
}

/// A log or a store that can't be read leaves each of Core's answers not known, never yes; an order register held
/// elsewhere at the moment does too.
// verifies: REQ-AUZ-5
#[test]
fn an_unreadable_log_or_store_gives_not_known() {
    let mut blind = CoreSources::new(Log(None), Store(true), YesToAll::default());
    for source in cores() {
        assert_eq!(
            blind.supply(&need(source.clone())),
            Reply::Unreachable,
            "{source:?}"
        );
    }
    assert_eq!(blind.order_record(&id("ord-1")), Reply::Unreachable);

    // A key register that can't be read leaves the declarations not known.
    let mut unregistered =
        CoreSources::new(Log(Some(Vec::new())), Store(false), YesToAll::default());
    assert_eq!(
        unregistered.supply(&need(Source::Declarations(id("bank-co")))),
        Reply::Unreachable
    );
    // An advertisement the register accepted, whose stated entry the log doesn't hold, declares nothing.
    let mut unlogged = CoreSources::new(
        Log(Some(Vec::new())),
        Store::Accepted(Some(digest('f'))),
        YesToAll::default(),
    );
    assert_eq!(
        unlogged.supply(&Need {
            input: id(NO_USER_WITHDRAWAL),
            from: Source::Declarations(id("bank-co")),
        }),
        Reply::Missing
    );

    let sources = CoreSources::new(Log(Some(Vec::new())), Store(true), YesToAll::default());
    let opened = sources.opened_orders();
    let parties = Parties {
        requester: id("cust-1"),
        beneficiary: id("cust-1"),
        settlement_account_at: None,
        payer: id("cust-1"),
    };
    opened.opened(&id("ord-1"), &parties, &id("bank-co"));
    let mut sources = sources;
    assert_eq!(
        sources.supply(&need(Source::Requester(id("ord-1")))),
        Reply::Given(Said::Yes)
    );
    {
        let _held = opened.0.borrow_mut();
        assert_eq!(
            sources.supply(&need(Source::Requester(id("ord-1")))),
            Reply::Unreachable
        );
        assert_eq!(sources.order_record(&id("ord-1")), Reply::Unreachable);
    }
    opened.withdrawn(&id("ord-1"));
    assert_eq!(
        sources.supply(&need(Source::Requester(id("ord-1")))),
        Reply::Missing
    );
}

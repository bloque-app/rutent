//! Sessions and reads: the session register the catalogue reads, and who may read which order or segment.
//!
//! Declared by `T-020-01`; `T-021-02` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! # Sessions
//!
//! A session speaks for one account, and the catalogue's balance checks happen only inside one (`REQ-CAT-3`). The
//! engine opens it only for the account's holder or its delegate, and only for a bounded time: the fixed rule refuses
//! a session asked by anyone else, or asked to last longer than [`LONGEST_SESSION_SECS`], before the deployment's
//! policy is asked, and the deployment's policy still decides the rest. The holder is the party the request names;
//! a delegate is another party the holder names as its delegate on that account, an input asked of the holder
//! (`Source::Holder`) like any other, so a holder that can't be reached gives not known and one that says nothing a
//! refusal (`REQ-AUZ-5`). An open session ends when it was asked to, and its id is its request's. An id already
//! issued opens no other session: the catalogue holds one session per id, so a second would take the first one's
//! place. That rule reads what the engine opened, so, like the permissions' below, it runs before the consultation.
//!
//! The engine keeps every session it opened in its [`Register`], which hands them to the catalogue
//! ([`Register::open_in`]). The catalogue holds only which account each speaks for and when it ends, and answers no
//! balance check in one that has ended (`F-012`'s notes).
//!
//! # Reads
//!
//! Each read is decided and recorded, allowed or refused (`REQ-AUZ-7`, `REQ-AUD-12`). The fixed rule holds a read to
//! what the log admits of a permitted one (core-internal's read rules and its relation rows), so policy never answers
//! a read the log would refuse:
//!
//! - a customer reads only an order, or a segment of an order, whose requester, beneficiary or payer it is, which the
//!   order's record says (`Source::OrderParties`), and never the trail;
//! - a provider reads only a segment it names itself the provider of, which the segment's record says
//!   (`Source::SegmentProvider`), and never an order or the trail;
//! - a regulator's or a checker's read names its permission.
//!
//! A read naming a permission is held to the register: the permission was granted, through
//! [`Reads::grant_permission`], to the reader's party, its scope covers what the read is of, and it hasn't been
//! revoked. That rule reads what the engine itself decided, which a fixed rule, a plain function of the request,
//! can't hold, so it is asked first, beside the fixed rules: its refusal is the answer and the deployment's policy
//! isn't asked. A revocation is held to it the same way: only a permission granted can be revoked.
//!
//! A read is allowed or refused and nothing else, so one policy can't decide is refused as not permitted.

use rutent_contracts::core_internal::calls::{AppendFailure, PolicyLog, Reads, Sessions};
use rutent_contracts::core_internal::{
    Answer, Decision, Opened, PermissionChange, ReadAnswer, ReadCheck, Reads as Read, RefusalClass,
    Role, SessionAnswer, SessionOutcome, SessionRequest, What, covers,
};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_prov::catalogue::{Catalogue, Instant};

use crate::engine::{
    Asked, Engine, Input, Need, Policy, Reply, Ruling, Said, Source, Sources, read_outcome,
    session_outcome,
};

/// The longest a session may last, in seconds, from when it is asked for to when it ends. A session asked to last
/// longer is refused, whoever asks (`REQ-CAT-3`: checks happen only inside a session, and a session that never ends
/// would bound nothing).
pub const LONGEST_SESSION_SECS: u64 = 15 * 60;

/// A session the engine opened: the account it speaks for, its holder, and when it ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSession {
    /// The session's id, its request's.
    pub session: Id,
    /// The account it speaks for, by reference.
    pub account: Id,
    /// The account's holder.
    pub holder: Id,
    /// When it ends, by Core's clock.
    pub expires_at: Timestamp,
}

impl OpenSession {
    /// Hands the session to the catalogue's register, which holds which account it speaks for until it ends. The
    /// end is taken down to the whole second, so the catalogue never holds a session past its end.
    pub fn open_in(&self, catalogue: &mut Catalogue) {
        catalogue.open_session(
            self.session.as_str(),
            self.account.as_str(),
            instant(&self.expires_at),
        );
    }
}

/// `at` on the catalogue's clock: whole seconds since the epoch, rounded down.
pub fn instant(at: &Timestamp) -> Instant {
    Instant(u64::try_from(at.unix_nanos().div_euclid(1_000_000_000)).unwrap_or(0))
}

/// What the engine opened, granted and revoked: the sessions it opened and the read permissions in force.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Register {
    pub(crate) sessions: Vec<OpenSession>,
    pub(crate) grants: Vec<PermissionChange>,
    pub(crate) revoked: Vec<Id>,
}

impl Register {
    /// Every session opened, oldest first.
    pub fn sessions(&self) -> &[OpenSession] {
        &self.sessions
    }

    /// Hands every session opened to the catalogue's register.
    pub fn open_in(&self, catalogue: &mut Catalogue) {
        for session in &self.sessions {
            session.open_in(catalogue);
        }
    }

    /// Whether `request` would open, under an id already issued, a session other than the one issued: for another
    /// account, holder or end. The catalogue holds one session per id, so a second would take the first one's place
    /// and speak for an account its holder never opened it on. The same request again opens the same session.
    fn reissues(&self, request: &SessionRequest) -> bool {
        self.sessions.iter().any(|s| {
            s.session == request.request
                && (s.account != request.account
                    || s.holder != request.holder
                    || s.expires_at != request.until)
        })
    }

    /// Whether `permission` was granted.
    fn granted(&self, permission: &Id) -> bool {
        self.grants.iter().any(|g| &g.permission == permission)
    }

    /// Why a read naming a permission is refused, where it is: the permission wasn't granted to the reader's party
    /// over a scope covering what it reads, or it was revoked (core-internal's rows
    /// `permitted-read-rests-on-its-grant` and `permitted-read-is-not-revoked`).
    fn refuses(&self, check: &ReadCheck) -> Option<RefusalClass> {
        let permission = check.permission.as_ref()?;
        let granted = self.grants.iter().any(|g| {
            &g.permission == permission
                && g.grantee.party == check.reader.party
                && covers(&g.scope, &check.reads)
        });
        (!granted || self.revoked.contains(permission)).then_some(RefusalClass::NotPermitted)
    }
}

/// The fixed rule's refusal of `asked`, where it needs no input to refuse.
fn refused_outright(asked: Asked<'_>) -> Option<RefusalClass> {
    let refused = match asked {
        Asked::OpenSession(r) => {
            let longest = i128::from(LONGEST_SESSION_SECS) * 1_000_000_000;
            r.until.unix_nanos() - r.at.unix_nanos() > longest
        }
        Asked::CheckRead(c) => match (c.reader.role, &c.reads) {
            (Role::Regulator | Role::Checker, _) => c.permission.is_none(),
            (Role::Provider, Read::Trail {} | Read::Order { .. }) => true,
            (Role::Provider, Read::Segment { provider, .. }) => provider != &c.reader.party,
            (Role::Customer, Read::Trail {}) => true,
            _ => false,
        },
        _ => false,
    };
    refused.then_some(RefusalClass::NotPermitted)
}

/// The input the fixed rule needs for `asked`, where it needs one: the holder's word that the requester is its
/// delegate, the order's record that the customer is its party, or the segment's that the provider is its own.
fn need_of(asked: Asked<'_>) -> Option<Need> {
    match asked {
        Asked::OpenSession(r) if r.requester.party != r.holder => Some(Need {
            input: r.requester.party.clone(),
            from: Source::Holder {
                account: r.account.clone(),
                holder: r.holder.clone(),
            },
        }),
        Asked::CheckRead(c) => match (c.reader.role, &c.reads) {
            (Role::Customer, Read::Order { order } | Read::Segment { order, .. }) => Some(Need {
                input: c.reader.party.clone(),
                from: Source::OrderParties(order.clone()),
            }),
            (Role::Provider, Read::Segment { order, segment, .. }) => Some(Need {
                input: c.reader.party.clone(),
                from: Source::SegmentProvider {
                    order: order.clone(),
                    segment: segment.clone(),
                },
            }),
            _ => None,
        },
        _ => None,
    }
}

/// What the fixed rule needs for `asked`: nothing for a call that isn't a session or a read, or that it refuses
/// outright.
pub(crate) fn needs(asked: Asked<'_>) -> Reply<Vec<Need>> {
    if refused_outright(asked).is_some() {
        return Reply::Given(Vec::new());
    }
    Reply::Given(need_of(asked).into_iter().collect())
}

/// The fixed rule's ruling: a session only for the holder or a delegate the holder names, never longer than
/// [`LONGEST_SESSION_SECS`]; a read only of what the log admits its reader may read. Every other call passes.
pub(crate) fn rule(asked: Asked<'_>, inputs: &[Input]) -> Reply<Ruling> {
    if let Some(class) = refused_outright(asked) {
        return Reply::Given(Ruling::Refuse(class));
    }
    let Some(need) = need_of(asked) else {
        return Reply::Given(Ruling::Permit);
    };
    match inputs.iter().find(|i| i.need == need) {
        Some(Input {
            said: Said::Yes, ..
        }) => Reply::Given(Ruling::Permit),
        Some(_) => Reply::Given(Ruling::Refuse(RefusalClass::NotPermitted)),
        None => Reply::Missing,
    }
}

impl<L, P, S> Engine<L, P, S> {
    /// What the engine opened, granted and revoked.
    pub fn register(&self) -> &Register {
        &self.register
    }
}

/// A session only for the account's holder or its delegate, bounded in time, decided and recorded before it is
/// answered; an open one is kept in the register the catalogue reads.
impl<L: PolicyLog, P: Policy, S: Sources> Sessions for Engine<L, P, S> {
    fn open_session(&mut self, request: &SessionRequest) -> Result<SessionAnswer, AppendFailure> {
        request.check().map_err(AppendFailure::Refused)?;
        let answer = if self.register.reissues(request) {
            Answer::Refuse {
                class: RefusalClass::NotPermitted,
            }
        } else {
            self.consult(Asked::OpenSession(request))
        };
        let outcome = session_outcome(
            answer,
            Opened {
                session: request.request.clone(),
                expires_at: request.until.clone(),
            },
        );
        let opened = match &outcome {
            SessionOutcome::Opened {
                session,
                expires_at,
            } => Some(Opened {
                session: session.clone(),
                expires_at: expires_at.clone(),
            }),
            SessionOutcome::Refused { .. } | SessionOutcome::NotKnown { .. } => None,
        };
        let what = What::OpenSession {
            request: request.clone(),
            opened: opened.clone(),
        };
        let receipt = self.record(&request.requester, what, &request.at, outcome.outcome())?;
        if let Some(opened) = opened {
            let session = OpenSession {
                session: opened.session,
                account: request.account.clone(),
                holder: request.holder.clone(),
                expires_at: opened.expires_at,
            };
            if !self.register.sessions.contains(&session) {
                self.register.sessions.push(session);
            }
        }
        Ok(SessionAnswer {
            request: request.request.clone(),
            outcome,
            receipt,
        })
    }
}

/// Each read decided and recorded, allowed or refused; a read permission granted or revoked through policy, and
/// kept in the register a read naming it is held to.
impl<L: PolicyLog, P: Policy, S: Sources> Reads for Engine<L, P, S> {
    fn check_read(&mut self, check: &ReadCheck) -> Result<ReadAnswer, AppendFailure> {
        let answer = match self.register.refuses(check) {
            Some(class) => Answer::Refuse { class },
            None => self.consult(Asked::CheckRead(check)),
        };
        let outcome = read_outcome(answer);
        let what = What::CheckRead {
            check: check.clone(),
        };
        let receipt = self.record(&check.reader, what, &check.at, outcome.outcome())?;
        Ok(ReadAnswer {
            request: check.request.clone(),
            outcome,
            receipt,
        })
    }

    fn grant_permission(&mut self, change: &PermissionChange) -> Result<Decision, AppendFailure> {
        change.check().map_err(AppendFailure::Refused)?;
        let answer = self.consult(Asked::GrantPermission(change));
        let what = |decision| What::GrantPermission {
            change: change.clone(),
            decision,
        };
        let decision = self.answer(&change.request, &change.by, what, &change.at, answer)?;
        if matches!(answer, Answer::Permit {}) && !self.register.grants.contains(change) {
            self.register.grants.push(change.clone());
        }
        Ok(decision)
    }

    fn revoke_permission(&mut self, change: &PermissionChange) -> Result<Decision, AppendFailure> {
        change.check().map_err(AppendFailure::Refused)?;
        let answer = if self.register.granted(&change.permission) {
            self.consult(Asked::RevokePermission(change))
        } else {
            Answer::Refuse {
                class: RefusalClass::NotPermitted,
            }
        };
        let what = |decision| What::RevokePermission {
            change: change.clone(),
            decision,
        };
        let decision = self.answer(&change.request, &change.by, what, &change.at, answer)?;
        if matches!(answer, Answer::Permit {})
            && !self.register.revoked.contains(&change.permission)
        {
            self.register.revoked.push(change.permission.clone());
        }
        Ok(decision)
    }
}

#[cfg(test)]
mod tests;

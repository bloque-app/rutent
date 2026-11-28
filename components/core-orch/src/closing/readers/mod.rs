//! Core's adapter for each public network's reader service (`T-020-60`, `DEC-28`, ADR-0007 as amended).
//!
//! [`PublicRecords`](super::PublicRecords) stays Core's seam (`REQ-MOD-3`), and Core implements it itself, here: a
//! [`Pass`] over [`Readers`] asks each listed network's reader, run by a reader host apart from Core, over an mTLS client
//! of Core's own ([`ReaderTls`]), under the operator's CA, on `rustls` with its `ring` provider. The deployment hands Core
//! its certificate and key, the operator's CA, the addresses its host settings hold, and its clock, when it builds
//! the adapter ([`Readers::new`], [`Readers::with_host`]): never a reader, and never a `PublicRecords` of its own.
//! Everything else is Core's (the review of `T-020-60`'s round 1, M1): the listings, read from the log
//! ([`listings_of`]); the start-up check when Core opens or reopens ([`Readers::start`], which `schedule::reopen`
//! runs), when a listing or delisting is decided and carried out ([`listing::change_public_network`]) and when the
//! deployment moves a host ([`Readers::move_host`]); the catalogue's public list of networks; the raise; and each pass,
//! which Core's own grading and reversals build from the adapter ([`super::Public::of`]).
//!
//! 1. **Checked before it is read.** The host's certificate is held to the operator's CA and to the subject name the
//!    listing names, never to a pinned leaf, so a rotated leaf under the same CA and name needs no listing change. The
//!    listing's `ca` must name the CA the deployment handed Core, by its SHA-256 digest, and that CA must be one by its
//!    basic constraints ([`tls::is_ca`]): a leaf's digest put in the listing, or a leaf handed as the CA, fails closed.
//!    Each answer's signature is verified against the reader's statement key in the listing, since transport identity
//!    alone is not authorisation, and the answer is then held to the question Core sent and to the listing's key and
//!    version ([`check_reader_answer`]), and to its deadline on Core's own clock, read when the answer reaches Core,
//!    never the host's. An answer arriving exactly at its deadline is taken. A failed check is [`Unreadable`].
//! 2. **Why no sequencer** (ADR-0007 as amended). Only Core asks, and a reader's answer reaches `CMP-STM`'s replicas only
//!    through the `evidence` entry Core records of it. Core draws each question's request id at random from its own
//!    source ([`RandomSource`], the system's secure random through `ring` by default, [`SystemRandom`]), never a counter
//!    and never Core's seeded identifiers: [`RequestId::from_random`] is the one way this crate makes a request id, which a
//!    scan of its source holds. The deadline is Core's.
//! 3. **The start-up check** ([`Readers::start`]). When Core opens or reopens, and when a `list_public_network` is
//!    decided, Core asks each listed reader `network_rules` and holds the answer to the listing: the network's identity,
//!    its rules, the reader's version (held by [`check_reader_answer`]) and its trust rule. A network whose reader is
//!    unreachable or fails the check does not start ([`Standing::NotStarted`]): it is not on the catalogue's public list,
//!    nothing is routed to it, and its segments read as unreadable. Core's other networks run on.
//! 4. **One answer per pass** ([`Pass`]). A pass is one grading of a part's results, or one round of `tick`. Within it the
//!    adapter asks each question once and keeps its answer, and the pass's answers go with it: Core keeps no copy of any
//!    network.
//! 5. **Unreadable.** Each of `PublicRecords`' three calls answers the reader's answer or [`Unreadable`]; closing takes
//!    `Unreadable` as "doesn't settle yet".

pub mod listing;
pub mod raise;
pub mod tls;

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::pki_types::ServerName;
use rustls::{ClientConnection, StreamOwned};
use rutent_contracts::core_internal::calls::AppendFailure;
use rutent_contracts::core_internal::{
    Action, CatalogueChange, Entry, MovementReader, NetworkListing, Outcome, PublicNetwork,
    Receipt, VerifiedEntry, What,
};
use rutent_contracts::module_interface::DigestAlgorithm;
use rutent_contracts::provider_role::{Deployment, NoDetail, ProtocolVersions, Statement};
use rutent_contracts::reader::{
    Answered, Asked, Expected, Finality, ReaderAnswer, ReaderMismatch, ReaderQuestion, Reason,
    ReorgRule, RequestId, Standing as Stood, check_pairing, check_reader_answer,
};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_evidence::{MovementId, NetworkRules, Status};
use rutent_prov::catalogue::Catalogue;

use super::{Moved, PublicRecords};
use crate::segments::RaiseDesk;
use crate::snapshot::{Clock, Reading};

pub use raise::Raised;
pub use tls::{ReaderTls, TlsRefusal};

/// How long, on Core's clock, Core waits for an answer: its deadline is the time it asked plus this. It also bounds the
/// whole of one exchange on the operating system's clock, connection, handshake, question and answer together, so a
/// host that trickles its answer a byte at a time holds Core's run no longer than this (the review of round 1, L4).
pub const ANSWER_WINDOW: Duration = Duration::from_secs(30);

/// The largest answer Core reads, in bytes.
pub const MAX_ANSWER: usize = 1 << 20;

/// How many questions put Core keeps a note of, the last ones ([`Readers::put`]).
const NOTED: usize = 4096;

/// Why a public record can't be read now: unreadable, which settles nothing and grades nothing down (ADR-0007 as
/// amended).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreadable {
    /// No listing in force names the network: it was never listed, or a decided delisting took it off. Core reads no
    /// record of a network without a reader, so a segment on it settles nothing, on its provider's word or otherwise
    /// (the review of round 1, M2).
    NotListed,
    /// The network's reader didn't pass its start-up check, or wasn't checked yet: why is its standing's
    /// ([`Readers::standing`]).
    NotStarted,
    /// The listing's host setting holds no address the deployment gave Core.
    NoHost,
    /// The listing names another CA than the operator's CA the deployment handed Core, or a digest of another kind.
    OtherCa,
    /// The host couldn't be reached, or the link's handshake failed: its certificate isn't the operator's CA's for the
    /// listed subject name, or it took none of Core's.
    Unreachable,
    /// The host gave no answer: an empty frame, or none at all.
    NoAnswer,
    /// The answer isn't a reader's signed answer.
    Malformed,
    /// Its signature doesn't verify under the reader's listed statement key.
    NotSigned,
    /// It fails [`check_reader_answer`]: it answers another question, request, network, movement or transaction, names
    /// another key, version or deployment, or came past its deadline on Core's clock.
    Refused(Vec<ReaderMismatch>),
    /// The reader said it can't stand behind an answer.
    Reader(Reason),
    /// The answer, though signed and held to the question, doesn't read as Core reads it (a movement's figures the
    /// evidence checker can't hold).
    Unstated,
}

/// Why a listed network did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotStarted {
    /// Its listing breaks the reader contract's rules: its identity and trust rule unpaired, or a trust rule that
    /// doesn't hold (`T-020-59`'s review: every path that takes a listing holds it to them).
    ListingBreaksItsRules,
    /// Its reader's `network_rules` couldn't be read.
    Unread(Unreadable),
    /// Its reader answered another network identity than the listing's.
    OtherIdentity,
    /// Its reader answered another trust rule than the listing's.
    OtherTrust,
    /// Not checked since it was listed, or since Core opened.
    Unchecked,
}

/// Where a network stands with Core's adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// No listing names it: Core reads no record of it.
    NotListed,
    /// Listed, and its reader passed the start-up check: it is read.
    Started,
    /// Listed, and its reader failed the start-up check, or wasn't checked: it doesn't start, and reads as
    /// unreadable.
    NotStarted(NotStarted),
}

/// Core's random source, which each request id is drawn from (`DEC-28`'s condition (b)).
pub trait RandomSource {
    /// Fills `bytes` from the source; `false` where it can't.
    fn fill(&self, bytes: &mut [u8]) -> bool;
}

/// The system's secure random, through `ring`, as `rustls`' provider holds it.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemRandom;

impl RandomSource for SystemRandom {
    fn fill(&self, bytes: &mut [u8]) -> bool {
        rustls::crypto::ring::default_provider()
            .secure_random
            .fill(bytes)
            .is_ok()
    }
}

/// One question Core put to a reader: its request id, the network and what it asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Put {
    /// The request id Core drew for it.
    pub request: RequestId,
    /// The network asked about.
    pub network: String,
    /// What was asked.
    pub asked: Asked,
}

/// Each listed network's reader, as Core reaches it: the deployment's TLS for the link, the addresses its host settings
/// hold, the listings Core's log holds and where each network stands.
pub struct Readers {
    tls: ReaderTls,
    hosts: BTreeMap<String, SocketAddr>,
    deployment: Deployment,
    versions: ProtocolVersions,
    listed: BTreeMap<String, NetworkListing>,
    standing: BTreeMap<String, NotStarted>,
    random: Box<dyn RandomSource>,
    clock: Box<dyn Clock>,
    put: RefCell<VecDeque<Put>>,
    raised: Raised,
}

impl core::fmt::Debug for Readers {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Readers")
            .field("hosts", &self.hosts)
            .field("listed", &self.listed.keys().collect::<Vec<_>>())
            .field("standing", &self.standing)
            .finish_non_exhaustive()
    }
}

impl Readers {
    /// Core's adapter, over the link's TLS the deployment handed it, for `deployment` under `versions`, at Core's
    /// `clock`, drawing request ids from the system's secure random. No network is listed yet, and no host is set.
    pub fn new(
        tls: ReaderTls,
        deployment: Deployment,
        versions: ProtocolVersions,
        clock: Box<dyn Clock>,
    ) -> Self {
        Readers {
            tls,
            hosts: BTreeMap::new(),
            deployment,
            versions,
            listed: BTreeMap::new(),
            standing: BTreeMap::new(),
            random: Box::new(SystemRandom),
            clock,
            put: RefCell::new(VecDeque::new()),
            raised: Raised::default(),
        }
    }

    /// The same, the deployment's host setting `setting` holding `address`: where the reader host a listing names by
    /// it is. The address stays the deployment's, so a host moved needs no listing change. Each listed network that
    /// names the setting is unchecked again until Core's next start-up check ([`Readers::move_host`] moves a host and
    /// checks at once).
    #[must_use]
    pub fn with_host(mut self, setting: &str, address: SocketAddr) -> Self {
        self.put_host(setting, address);
        self
    }

    fn put_host(&mut self, setting: &str, address: SocketAddr) {
        self.hosts.insert(setting.to_owned(), address);
        for (network, listing) in &self.listed {
            if listing.endpoint.host_setting.as_str() == setting {
                self.standing
                    .entry(network.clone())
                    .or_insert(NotStarted::Unchecked);
            }
        }
    }

    /// The same, drawing request ids from `random`: a test hook, never in a deployment's build (the review of round 1,
    /// L5), so a deployment can't replace Core's random source (`DEC-28`'s condition (b)).
    #[cfg(any(test, feature = "reader-test-hooks"))]
    #[must_use]
    pub fn drawing_from(mut self, random: Box<dyn RandomSource>) -> Self {
        self.random = random;
        self
    }

    /// The host setting `setting` now holds `address`, with no check: a test hook, never in a deployment's build. A
    /// deployment moves a host through [`Readers::move_host`], which checks each listed reader there.
    #[cfg(any(test, feature = "reader-test-hooks"))]
    pub fn set_host(&mut self, setting: &str, address: SocketAddr) {
        self.hosts.insert(setting.to_owned(), address);
    }

    /// The protocol versions in force, which each answer is held to.
    pub fn set_versions(&mut self, versions: ProtocolVersions) {
        self.versions = versions;
    }

    /// The link's TLS.
    pub fn tls(&self) -> &ReaderTls {
        &self.tls
    }

    /// Takes a decided listing of `network`'s reader in, in place of any earlier one, unchecked: a test hook, never in
    /// a deployment's build, where Core takes each listing from its log ([`Readers::start`],
    /// [`listing::change_public_network`]).
    #[cfg(any(test, feature = "reader-test-hooks"))]
    pub fn list(&mut self, network: &PublicNetwork) {
        self.take(network);
    }

    /// Takes a decided listing of `network`'s reader in, in place of any earlier one: unchecked until Core's start-up
    /// check. A delisting (a public network with no listing) takes it off.
    fn take(&mut self, network: &PublicNetwork) {
        let name = network.network.as_str().to_owned();
        match &network.listing {
            Some(listing) => {
                self.listed.insert(name.clone(), listing.clone());
                let holds = check_pairing(&listing.identity, &listing.trust).is_ok()
                    && listing.trust.check().is_ok();
                self.standing.insert(
                    name,
                    if holds {
                        NotStarted::Unchecked
                    } else {
                        NotStarted::ListingBreaksItsRules
                    },
                );
            }
            None => {
                self.listed.remove(&name);
                self.standing.remove(&name);
            }
        }
    }

    /// The listings in force, by network.
    pub fn listed(&self) -> &BTreeMap<String, NetworkListing> {
        &self.listed
    }

    /// Where `network` stands.
    pub fn standing(&self, network: &str) -> Standing {
        if !self.listed.contains_key(network) {
            return Standing::NotListed;
        }
        match self.standing.get(network) {
            None => Standing::Started,
            Some(why) => Standing::NotStarted(why.clone()),
        }
    }

    /// The networks whose reader passed its start-up check, in name order.
    pub fn started(&self) -> Vec<&str> {
        self.listed
            .keys()
            .filter(|n| !self.standing.contains_key(*n))
            .map(String::as_str)
            .collect()
    }

    /// The start-up check of `network`'s reader at `clock`, Core's own check with the clock a test hands it: a test
    /// hook, never in a deployment's build.
    #[cfg(any(test, feature = "reader-test-hooks"))]
    pub fn check_at(&mut self, network: &str, clock: &dyn Clock) -> Standing {
        self.check_with(network, clock)
    }

    /// The start-up check of `network`'s reader, at Core's clock: its `network_rules` answer, checked as every answer
    /// is, held to the listing's identity and trust rule. The network starts where it passes; otherwise it doesn't, and
    /// why is its standing. Nothing for a network no listing names.
    fn check(&mut self, network: &str) -> Standing {
        let clock = std::mem::replace(&mut self.clock, Box::new(crate::snapshot::SystemClock));
        let standing = self.check_with(network, &*clock);
        self.clock = clock;
        standing
    }

    fn check_with(&mut self, network: &str, clock: &dyn Clock) -> Standing {
        let Some(listing) = self.listed.get(network) else {
            return Standing::NotListed;
        };
        if self.standing.get(network) == Some(&NotStarted::ListingBreaksItsRules) {
            return self.standing(network);
        }
        let answered = self.ask_listed(network, listing, Asked::NetworkRules(NoDetail {}), clock);
        let outcome = match answered {
            Err(why) => Some(NotStarted::Unread(why)),
            Ok(Answered::NetworkRules { rules }) => {
                if rules.identity != listing.identity {
                    Some(NotStarted::OtherIdentity)
                } else if rules.trust != listing.trust {
                    Some(NotStarted::OtherTrust)
                } else {
                    None
                }
            }
            Ok(_) => Some(NotStarted::Unread(Unreadable::Malformed)),
        };
        match outcome {
            Some(why) => self.standing.insert(network.to_owned(), why),
            None => self.standing.remove(network),
        };
        self.standing(network)
    }

    /// Core's start-up check (`T-020-60` (2), (7)), as it runs when Core opens or reopens (`schedule::reopen`) and
    /// whenever a host moves: every listing `history` holds in force taken in, in place of what the adapter held, and
    /// one it no longer holds taken off; each listed reader asked `network_rules` and held to its listing; each network
    /// on the catalogue's public list where its reader passed, held off every route where it didn't, and off the list
    /// where no listing names it any more; and each network that didn't start raised to the operator, once in its spell,
    /// citing its listing ([`Raised`]). The networks that didn't start, with why. Core's other networks run on.
    ///
    /// # Errors
    ///
    /// The first raise the log refused or couldn't take now: that network stays out of its spell, so the next check
    /// raises it again. Every network is still checked and put on or off the list.
    pub fn start(
        &mut self,
        history: &[(Receipt, Entry)],
        catalogue: &mut Catalogue,
        desk: RaiseDesk<'_>,
    ) -> Result<Vec<(String, NotStarted)>, AppendFailure> {
        let in_force = listings_of(history);
        let gone: Vec<String> = self
            .listed
            .keys()
            .filter(|n| !in_force.iter().any(|l| l.network.as_str() == n.as_str()))
            .cloned()
            .collect();
        for network in gone {
            self.listed.remove(&network);
            self.standing.remove(&network);
            catalogue.drop_public_network(&network);
        }
        let mut not_started = BTreeMap::new();
        for network in &in_force {
            self.take(network);
            let name = network.network.as_str();
            let started = match self.check(name) {
                Standing::NotStarted(why) => {
                    not_started.insert(name.to_owned(), why);
                    false
                }
                _ => true,
            };
            let _ = catalogue.list_public_network(name, network.listing.is_some(), started);
        }
        let names: Vec<String> = self.listed.keys().cloned().collect();
        self.raised
            .raise(&names, &not_started, history, desk)
            .map(|_| not_started.into_iter().collect())
    }

    /// The deployment moved its host setting `setting` to `address`: Core checks each listed reader again there, as on
    /// opening ([`Readers::start`]), so a network started at the old address starts at the new one only on its
    /// reader's passing.
    ///
    /// # Errors
    ///
    /// As [`Readers::start`].
    pub fn move_host(
        &mut self,
        setting: &str,
        address: SocketAddr,
        history: &[(Receipt, Entry)],
        catalogue: &mut Catalogue,
        desk: RaiseDesk<'_>,
    ) -> Result<Vec<(String, NotStarted)>, AppendFailure> {
        self.put_host(setting, address);
        self.start(history, catalogue, desk)
    }

    /// Whether `network` is raised to the operator as not started, in the spell now running.
    pub fn is_raised(&self, network: &str) -> bool {
        self.raised.is_raised(network)
    }

    /// The questions Core put, the last few thousand, in the order put.
    pub fn put(&self) -> Vec<Put> {
        self.put.borrow().iter().cloned().collect()
    }

    /// A pass over the readers at Core's clock: each question asked once within it, its answer kept until the pass
    /// ends, when the pass is dropped. Core's grading and reversals each build theirs ([`super::Public::of`]).
    pub fn pass(&self) -> Pass<'_> {
        Pass {
            readers: self,
            kept: RefCell::new(BTreeMap::new()),
        }
    }

    /// The reader that reads `network`, as its listing names it, and its version: what an `evidence` entry names.
    pub fn reader_of(&self, network: &str) -> Option<MovementReader> {
        self.listed.get(network).map(|listing| MovementReader {
            reader: listing.endpoint.reader.clone(),
            version: listing.version.clone(),
        })
    }

    /// A request id drawn from Core's random source: never a counter, and never all zero in its top half, which
    /// [`RequestId::from_random`] refuses (the source draws again).
    fn draw(&self) -> Option<RequestId> {
        for _ in 0..4 {
            let mut bytes = [0u8; 16];
            if !self.random.fill(&mut bytes) {
                return None;
            }
            if let Some(id) = RequestId::from_random(bytes) {
                return Some(id);
            }
        }
        None
    }

    /// Asks `network`'s reader `asked` once, whatever its standing, at `clock`: a test hook, never in a deployment's
    /// build (the review of round 1, L5). A pass asks only a started network's reader ([`Pass`]).
    ///
    /// # Errors
    ///
    /// Where no listing names `network`, or as [`Unreadable`] names.
    #[cfg(any(test, feature = "reader-test-hooks"))]
    pub fn question(
        &self,
        network: &str,
        asked: Asked,
        clock: &dyn Clock,
    ) -> Result<Answered, Unreadable> {
        let listing = self.listed.get(network).ok_or(Unreadable::NotListed)?;
        self.ask_listed(network, listing, asked, clock)
    }

    /// Asks `network`'s reader `asked` at Core's clock, where its reader started: unreadable otherwise, a network no
    /// listing names included.
    fn ask(&self, network: &str, asked: Asked) -> Result<Answered, Unreadable> {
        let clock = &*self.clock;
        let listing = self.listed.get(network).ok_or(Unreadable::NotListed)?;
        if self.standing.contains_key(network) {
            return Err(Unreadable::NotStarted);
        }
        self.ask_listed(network, listing, asked, clock)
    }

    fn ask_listed(
        &self,
        network: &str,
        listing: &NetworkListing,
        asked: Asked,
        clock: &dyn Clock,
    ) -> Result<Answered, Unreadable> {
        let address = *self
            .hosts
            .get(listing.endpoint.host_setting.as_str())
            .ok_or(Unreadable::NoHost)?;
        if listing.reader.ca.algorithm != DigestAlgorithm::Sha256
            || listing.reader.ca.value.as_str() != self.tls.ca_sha256()
        {
            return Err(Unreadable::OtherCa);
        }
        let request = self.draw().ok_or(Unreadable::NoAnswer)?;
        let asked_at = clock.now();
        let question = ReaderQuestion {
            request: request.clone(),
            reader: listing.endpoint.reader.clone(),
            network: Id::new(network).ok_or(Unreadable::Malformed)?,
            deadline: later(asked_at, ANSWER_WINDOW).ok_or(Unreadable::Malformed)?,
            asked,
        };
        {
            let mut put = self.put.borrow_mut();
            if put.len() == NOTED {
                put.pop_front();
            }
            put.push_back(Put {
                request,
                network: network.to_owned(),
                asked: question.asked.clone(),
            });
        }
        let bytes = serde_json::to_vec(&question).map_err(|_| Unreadable::Malformed)?;
        let answer = self.exchange(address, listing, &bytes)?;
        // Core's own clock when the answer reached it: the deadline is held to it, never to the host's.
        let arrived = clock.now().timestamp().ok_or(Unreadable::Malformed)?;
        if answer.is_empty() {
            return Err(Unreadable::NoAnswer);
        }
        let statement: Statement<ReaderAnswer> =
            serde_json::from_slice(&answer).map_err(|_| Unreadable::Malformed)?;
        rutent_prov::catalogue::verify_envelope(
            &listing.reader.statement_key,
            &statement.signature.value,
            &answer,
        )
        .map_err(|_| Unreadable::NotSigned)?;
        let expected = Expected {
            deployment: &self.deployment,
            versions: &self.versions,
            key: &listing.reader.statement_key,
            version: &listing.version,
        };
        check_reader_answer(&question, &statement, &expected, &arrived)
            .map_err(Unreadable::Refused)?;
        match statement.statement.answer {
            Answered::Unreadable { why } => Err(Unreadable::Reader(why)),
            answered => Ok(answered),
        }
    }

    /// One question over the mTLS link to `address`, the host held to the operator's CA and the listed subject name:
    /// the answer's bytes, empty where the host had nothing to sign. The whole exchange is bounded by
    /// [`ANSWER_WINDOW`] on the operating system's clock.
    fn exchange(
        &self,
        address: SocketAddr,
        listing: &NetworkListing,
        question: &[u8],
    ) -> Result<Vec<u8>, Unreadable> {
        let name = ServerName::try_from(listing.reader.subject_name.as_str().to_owned())
            .map_err(|_| Unreadable::Unreachable)?;
        let until = Instant::now() + ANSWER_WINDOW;
        let tcp = TcpStream::connect_timeout(&address, Duration::from_secs(10))
            .map_err(|_| Unreadable::Unreachable)?;
        let connection = ClientConnection::new(Arc::clone(&self.tls.client), name)
            .map_err(|_| Unreadable::Unreachable)?;
        let mut stream = StreamOwned::new(connection, Bounded { tcp, until });
        write_frame(&mut stream, question).map_err(|_| Unreadable::Unreachable)?;
        match read_frame(&mut stream) {
            Ok(Some(answer)) => Ok(answer),
            Ok(None) => Ok(Vec::new()),
            Err(e) if e.kind() == io::ErrorKind::InvalidData => Err(Unreadable::Malformed),
            Err(_) => Err(Unreadable::Unreachable),
        }
    }
}

/// A connection bounded as a whole: each read and write waits only for what is left until `until`, and none starts
/// past it, so a peer that trickles bytes can't stretch one exchange past its bound (the review of round 1, L4).
struct Bounded {
    tcp: TcpStream,
    until: Instant,
}

impl Bounded {
    fn left(&self) -> io::Result<Duration> {
        self.until
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "the exchange's bound passed"))
    }
}

impl io::Read for Bounded {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let left = self.left()?;
        self.tcp.set_read_timeout(Some(left))?;
        io::Read::read(&mut self.tcp, bytes)
    }
}

impl io::Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let left = self.left()?;
        self.tcp.set_write_timeout(Some(left))?;
        io::Write::write(&mut self.tcp, bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(&mut self.tcp)
    }
}

/// The time `window` after `at`, as a timestamp.
fn later(at: Reading, window: Duration) -> Option<Timestamp> {
    Reading {
        seconds: at.seconds.checked_add(window.as_secs())?,
        nanos: at.nanos,
    }
    .timestamp()
}

/// The link's frame: its length as four little-endian bytes, then its bytes (the reader host's `link`).
fn write_frame(out: &mut impl io::Write, bytes: &[u8]) -> io::Result<()> {
    let length = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too long"))?;
    out.write_all(&length.to_le_bytes())?;
    out.write_all(bytes)?;
    out.flush()
}

/// One frame of at most [`MAX_ANSWER`] bytes, or `None` at the end of the stream; a longer one is refused from its
/// length alone.
fn read_frame(input: &mut impl io::Read) -> io::Result<Option<Vec<u8>>> {
    let mut length = [0u8; 4];
    match input.read_exact(&mut length) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_ANSWER {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too long"));
    }
    let mut bytes = vec![0u8; length];
    input.read_exact(&mut bytes)?;
    Ok(Some(bytes))
}

/// One pass over the readers (`T-020-60` (3)): each question asked once, its answer kept until the pass is dropped.
pub struct Pass<'a> {
    readers: &'a Readers,
    kept: RefCell<BTreeMap<(String, String), Result<Answered, Unreadable>>>,
}

impl Pass<'_> {
    fn answer(&self, network: &str, asked: Asked) -> Result<Answered, Unreadable> {
        let key = (
            network.to_owned(),
            serde_json::to_string(&asked).unwrap_or_default(),
        );
        if let Some(kept) = self.kept.borrow().get(&key) {
            return kept.clone();
        }
        let answered = self.readers.ask(network, asked);
        self.kept.borrow_mut().insert(key, answered.clone());
        answered
    }

    /// The adapter this pass reads through.
    pub fn readers(&self) -> &Readers {
        self.readers
    }
}

/// The evidence checker's rules, from a reader's.
fn rules_of(finality: &Finality, reorg: &ReorgRule) -> NetworkRules {
    NetworkRules {
        finality: match finality {
            Finality::Instant(_) => rutent_evidence::Finality::Instant,
            Finality::Confirmations { count } => {
                rutent_evidence::Finality::Confirmations(count.get())
            }
            Finality::Checkpoint(_) => rutent_evidence::Finality::Checkpoint,
        },
        reorg: match reorg {
            ReorgRule::Never(_) => rutent_evidence::ReorgRule::Never,
            ReorgRule::DepthBound { depth } => rutent_evidence::ReorgRule::DepthBound(depth.get()),
            ReorgRule::UntilFinal(_) => rutent_evidence::ReorgRule::UntilFinal,
        },
    }
}

fn movement_of(m: &rutent_contracts::reader::Movement) -> rutent_evidence::Movement {
    rutent_evidence::Movement {
        destination: m.destination.as_str().to_owned(),
        asset: m.asset.as_str().to_owned(),
        amount: m.amount.as_str().to_owned(),
    }
}

/// Each listed network, as its reader answers it in this pass: a network no listing names is unreadable, never "no
/// record" (the review of round 1, M2, reversing the round's departure), so nothing on it settles.
impl PublicRecords for Pass<'_> {
    fn rules(&self, network: &str) -> Result<Option<NetworkRules>, Unreadable> {
        match self.answer(network, Asked::NetworkRules(NoDetail {}))? {
            Answered::NetworkRules { rules } => Ok(Some(rules_of(&rules.finality, &rules.reorg))),
            _ => Err(Unreadable::Malformed),
        }
    }

    fn status(&self, network: &str, id: &MovementId) -> Result<Status, Unreadable> {
        let movement = Id::new(id.as_str()).ok_or(Unreadable::Unstated)?;
        match self.answer(network, Asked::MovementStatus { movement })? {
            Answered::MovementStatus { standing } => Ok(match standing {
                Stood::Unknown(_) => Status::Unknown,
                Stood::Pending(m) => Status::Pending(movement_of(&m)),
                Stood::Final(m) => Status::Final(movement_of(&m)),
                Stood::ReorgedOut(_) => Status::ReorgedOut,
            }),
            _ => Err(Unreadable::Malformed),
        }
    }

    fn movement(&self, network: &str, transaction: &Id) -> Result<Option<Moved>, Unreadable> {
        let read_by = self
            .readers
            .reader_of(network)
            .ok_or(Unreadable::NotListed)?;
        match self.answer(
            network,
            Asked::MovementOf {
                transaction: transaction.clone(),
            },
        )? {
            Answered::MovementOf { movement: None } => Ok(None),
            Answered::MovementOf {
                movement: Some(tied),
            } => Ok(Some(Moved {
                id: MovementId::new(tied.id.as_str()).ok_or(Unreadable::Unstated)?,
                destination: tied.destination.as_str().to_owned(),
                read_by,
            })),
            _ => Err(Unreadable::Malformed),
        }
    }
}

/// Every public network the log lists now, in the order first listed: each decided `list_public_network` carried out,
/// a later one of the same network taking its place, and a later `delist_public_network` carried out taking it off.
/// What a reopened Core lists its readers from (`T-020-60` (7)).
pub fn listings_of(history: &[(Receipt, Entry)]) -> Vec<PublicNetwork> {
    let done: Vec<&Receipt> = history
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(VerifiedEntry {
                what: What::Result { effect },
                outcome: Outcome::Done {},
                ..
            }) => Some(effect),
            _ => None,
        })
        .collect();
    let mut listed: Vec<PublicNetwork> = Vec::new();
    for (receipt, entry) in history {
        let Entry::Verified(VerifiedEntry {
            what:
                What::Effect {
                    action:
                        Action::ChangeCatalogue {
                            change,
                            public_network: Some(network),
                            ..
                        },
                    ..
                },
            ..
        }) = entry
        else {
            continue;
        };
        if !done.contains(&receipt) {
            continue;
        }
        let at = listed.iter().position(|n| n.network == network.network);
        match (change, at) {
            (CatalogueChange::ListPublicNetwork, Some(at)) => listed[at] = (**network).clone(),
            (CatalogueChange::ListPublicNetwork, None) => listed.push((**network).clone()),
            (CatalogueChange::DelistPublicNetwork, Some(at)) => {
                listed.remove(at);
            }
            _ => {}
        }
    }
    listed
}

/// Why a delisting of `network` is refused (#343's review, L7; #346's review, L3): a network no listing names has
/// nothing to delist, and a network an open or quoted order uses (`in_use`) keeps its standing while it does, as a node
/// does under `T-020-66`'s unlist rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelistRefused {
    /// No listing in force names it.
    NeverListed,
    /// An open or quoted order uses it.
    InUse,
}

/// Whether `network` may be delisted now, given the listings in force and whether an open or quoted order uses it.
///
/// # Errors
///
/// As [`DelistRefused`] names.
pub fn may_delist(
    listed: &[PublicNetwork],
    network: &str,
    in_use: bool,
) -> Result<(), DelistRefused> {
    if !listed.iter().any(|n| n.network.as_str() == network) {
        return Err(DelistRefused::NeverListed);
    }
    if in_use {
        return Err(DelistRefused::InUse);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// Every `.rs` file under `dir`.
    fn sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("the source tree") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// A host that trickles a frame a byte at a time can't hold one exchange past its bound: each read waits only for
    /// what is left of it (the review of round 1, L4).
    // verifies: REQ-MDL-11
    #[test]
    fn exchange_with_a_host_that_trickles_its_answer_ends_at_its_bound() {
        use std::io::Write as _;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let address = listener.local_addr().expect("its address");
        let server = std::thread::spawn(move || {
            let (mut peer, _) = listener.accept().expect("Core's connection");
            // A frame of a thousand bytes, announced, then a byte every 100 ms: 100 s in all, unbounded per read.
            let _ = peer.write_all(&1000u32.to_le_bytes());
            for _ in 0..1000 {
                if peer.write_all(&[0]).is_err() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });
        let tcp = std::net::TcpStream::connect(address).expect("connected");
        let started = std::time::Instant::now();
        let mut bounded = super::Bounded {
            tcp,
            until: started + std::time::Duration::from_millis(700),
        };
        let read = super::read_frame(&mut bounded);
        let took = started.elapsed();
        assert!(read.is_err(), "the frame never completes inside the bound");
        assert!(
            took < std::time::Duration::from_secs(3),
            "the exchange ends at its bound, not per byte: {took:?}"
        );
        drop(bounded);
        let _ = server.join();
    }

    // verifies: DEC-28
    #[test]
    fn core_makes_a_request_id_only_from_its_random_source() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        sources(&root, &mut files);
        // Every way to make a request id other than from random bytes (#343's review, L5; the check of T-020-59): its
        // checked constructor, its conversion from text, and its deserialisation on its own.
        let banned = [
            ["RequestId", "::new("].concat(),
            ["RequestId", "::try_from("].concat(),
            ["RequestId", "::deserialize("].concat(),
            ["::<Request", "Id>("].concat(),
            [": Request", "Id = serde"].concat(),
        ];
        let mut drawn = 0;
        for file in files {
            let text = std::fs::read_to_string(&file).expect("a source file");
            for ban in &banned {
                assert!(
                    !text.contains(ban.as_str()),
                    "{} makes a request id by `{ban}`",
                    file.display()
                );
            }
            drawn += text
                .matches(["RequestId", "::from_random("].concat().as_str())
                .count();
        }
        assert_eq!(
            drawn, 1,
            "one draw, from Core's random source, makes every request id"
        );
    }
}

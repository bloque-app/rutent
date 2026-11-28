//! Arrivals and automatic deposits in the assembly (`T-020-04`): [`Arrivals`] held beside intake, on the desk intake
//! reaches; the orders open at a reserve read from intake's started orders; each order a filing opens handed to policy's
//! order records before policy is asked about it, and dropped where none opens; and an order started on an automatic
//! deposit followed by the segments from its start as arrivals answers it, as intake's are.
//!
//! What it doesn't do: build a reserve's [`Report`] from what a simulated reserve's module reports. No simulated
//! provider reports value arriving yet, so a scenario hands the report in (`T-024-04`, `AC-10`, `AC-11`, `AC-26`).

use rutent_contracts::router_api::Id;
use rutent_core_orch::arrival::{Arrivals, Arrived, Filed, FiledOrders, Filing, OpenOrder, Report};
use rutent_core_orch::intake::{Desk, OrderRecord};
use rutent_router::RouteFinder;

use super::{Core, StepError, context};
use crate::kit;

/// Core's sources' records of the orders a filing opens before the log holds them, as the filing hands them over
/// (`FiledOrders`, `T-020-55`).
struct FiledOn(rutent_core_authz::core_sources::OpenedOrders);

impl FiledOrders for FiledOn {
    fn opened(&mut self, record: &OrderRecord) {
        self.0
            .opened(&record.order, &record.parties, &record.origin_reserve);
    }

    fn withdrawn(&mut self, order: &Id) {
        self.0.withdrawn(order);
    }
}

/// A quote a filed order waits on its customer to accept, owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Awaited {
    /// The quote, its slow segments named.
    pub quote: rutent_core_orch::intake::Quote,
    /// Its digest.
    pub digest: rutent_contracts::module_interface::Digest,
    /// Until when it stands.
    pub valid_until: Option<rutent_contracts::router_api::Timestamp>,
    /// Whether it stopped standing.
    pub expired: bool,
}

/// The orders open, as read before the call.
impl Core {
    /// Runs `call` on arrivals with the desk intake reaches. `Arrivals` is held beside intake, so it is taken out for
    /// the call, which the desk's closure can't otherwise reach.
    pub(super) fn with_arrivals<T>(
        &mut self,
        call: impl FnOnce(&mut Arrivals, &mut Desk<'_, '_>) -> T,
    ) -> T {
        let mut arrivals = std::mem::take(&mut self.arrivals);
        let out = self.with_desk(|_, desk| call(&mut arrivals, desk));
        self.arrivals = arrivals;
        out
    }

    /// The orders open at a reserve, as Core derives them (`rutent_core_orch::arrival::open::OpenAt`): the assembly
    /// answers only which account a session speaks for, and every session it opens speaks for the conformance account.
    fn open_orders(&self) -> Vec<OpenOrder> {
        let history = self.log.borrow().entries().to_vec();
        let account = kit::id(&context().account);
        rutent_core_orch::arrival::open::OpenAt {
            intake: &self.intake,
            history: &history,
            account_of: &|_| Some(account.clone()),
        }
        .all()
    }

    /// Takes value arriving at a reserve, as the reserve's `report` tells it.
    pub fn arrive(&mut self, report: &Report) -> Result<Arrived, StepError> {
        let mut open = rutent_core_orch::arrival::open::Listed(self.open_orders());
        let arrived =
            self.with_arrivals(|arrivals, desk| arrivals.arrive(desk, &mut open, report))?;
        self.feed()?;
        Ok(arrived)
    }

    /// `filer` files the arrival at `arrival` anew against the order it differed from.
    pub fn refile(
        &mut self,
        filer: &rutent_contracts::core_internal::Actor,
        arrival: &rutent_contracts::core_internal::Receipt,
    ) -> Result<rutent_contracts::core_internal::Receipt, StepError> {
        let refiled = self.with_arrivals(|arrivals, desk| arrivals.refile(desk, filer, arrival))?;
        self.feed()?;
        Ok(refiled)
    }

    /// Files an order on an automatic deposit; once it starts, its segments follow it.
    pub fn file(&mut self, filing: Filing) -> Result<Filed, StepError> {
        let mut filed_on = FiledOn(self.sources.opened_orders());
        let filed = self.with_arrivals(|arrivals, desk| {
            arrivals.file::<RouteFinder>(desk, &mut filed_on, filing.clone())
        });
        // A filed order that waits for its customer is started later, by `accept_filed`, from this filing.
        // A filing stated again, as a reopened Core's wait is restated (`T-020-51`), replaces what the log held of it.
        if let Ok(Filed::Waiting {
            order: Some(order), ..
        }) = &filed
        {
            self.filings
                .insert(order.as_str().to_owned(), filing.clone());
        }
        if let Ok(Filed::Started { order, .. }) = &filed {
            self.filings
                .entry(order.as_str().to_owned())
                .or_insert_with(|| filing.clone());
            // What the filed order runs on is built by Core from the log, its filing's inputs among it (`T-020-61`).
            rutent_core_orch::schedule::resume(&mut super::schedule::Due(self));
        }
        self.feed()?;
        Ok(filed?)
    }

    /// The quote the filed order `order` waits on its customer to accept, as of Core's clock (`T-020-41`): the quote,
    /// its digest, until when it stands and whether it stopped standing.
    pub fn awaiting(&self, order: &Id) -> Option<Awaited> {
        let now = self.clock.timestamp();
        self.arrivals.awaiting(order, &now).map(|a| Awaited {
            quote: a.quote.clone(),
            digest: a.digest.clone(),
            valid_until: a.valid_until,
            expired: a.expired,
        })
    }

    /// The customer `by` accepts the slow route of the filed order `order`, the quote of digest `quote`, in `session`, a
    /// session of its own that is open now (`T-020-51`); once it starts, its segments follow it.
    pub fn accept_filed(
        &mut self,
        by: &rutent_contracts::core_internal::Actor,
        session: &Id,
        order: &Id,
        quote: &rutent_contracts::module_interface::Digest,
    ) -> Result<Filed, StepError> {
        let filed = self.with_arrivals(|arrivals, desk| {
            arrivals.accept::<RouteFinder>(desk, order, by, session, quote)
        });
        if let Ok(Filed::Started { .. }) = &filed {
            rutent_core_orch::schedule::resume(&mut super::schedule::Due(self));
        }
        self.feed()?;
        Ok(filed?)
    }

    /// When the wait of the filed order `order` ends, where it waits for its customer (`T-020-51`).
    pub fn wait_ends(&self, order: &Id) -> Option<rutent_contracts::router_api::Timestamp> {
        self.arrivals.wait_ends(order)
    }
}

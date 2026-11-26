//! The check of a holding account against what its orders hold, as the assembly runs it (`REQ-AUD-15`, `AC-39`;
//! `T-024-05`).
//!
//! Core's schedule reads each holding account and checks it (`T-020-48`): the job is `core-orch`'s
//! (`rutent_core_orch::schedule::Jobs`), and the assembly answers only its ports (`schedule.rs`): the register's
//! readings through the platform, and [`Compare`], which records the check through the general handle on Core's log
//! and raises to the operator on Core's own channel, the notices'. A scenario calls [`Core::tick`], and a reading is
//! never handed in. What stays here is the comparison of the pairs of reports, which the tick runs too, and its
//! restating for a scenario that has a party report what the simulated providers don't ([`Core::write_pairs_as`]).

use rutent_contracts::core_internal::calls::{AppendFailure, Comparisons};
use rutent_contracts::core_internal::{HoldingReading, PairNotBuilt, Receipt, ReportPair};
use rutent_core_audit::compare::Compare;
use rutent_core_audit::log::General;

use super::Core;
use crate::kit;

impl Core {
    /// How many pairs of reports the segments' ends owe the comparison: those waiting for [`Core::write_pairs`], and
    /// those that couldn't be built or that the log refused ([`Core::unbuilt_reports`]).
    pub fn unpaired_reports(&self) -> usize {
        self.segments.unpaired() + self.segments.unbuilt_pairs().len()
    }

    /// The pairs that couldn't be built or that the log refused, each with why (`REQ-ORD-9`).
    pub fn unbuilt_reports(&self) -> &[rutent_core_orch::segments::UnbuiltPair] {
        self.segments.unbuilt_pairs()
    }

    /// Makes the segments forget the reports they keep for the pair of the segment after them, as a restart that lost
    /// their memory would: for a scenario of a pair that can't be built.
    pub fn lose_sent_reports(&mut self) {
        self.segments.lose_sent_reports();
    }

    /// Hands `CMP-CORE-AUDIT`'s comparison the pairs of reports the segments' ends gave since the last call, and
    /// answers how many it took (`REQ-ORD-9`, `T-020-35`): each pair is recorded in Core's log and, where its figures
    /// differ by one increment or more, raised to the operator on Core's channel. The assembly doesn't call it by
    /// itself: its caller does, as the measures' (`Core::write_measures`). A pair the log can't take now waits.
    ///
    /// # Errors
    ///
    /// The first failure of a pair's record or its raise's, with the pairs after it left waiting.
    pub fn write_pairs(&mut self) -> Result<usize, AppendFailure> {
        self.write_pairs_as(&mut |pair| pair)
    }

    /// [`Core::write_pairs`], each pair restated by `restate` on its way to the comparison: for a scenario that has a
    /// party report what the simulated providers don't (a figure a little off, under a tuple that declares an
    /// increment), the receipts of its statements and everything else as Core built them.
    ///
    /// # Errors
    ///
    /// As [`Core::write_pairs`].
    pub fn write_pairs_as(
        &mut self,
        restate: &mut dyn FnMut(ReportPair) -> ReportPair,
    ) -> Result<usize, AppendFailure> {
        struct Restating<'a, C> {
            inner: &'a mut C,
            restate: &'a mut dyn FnMut(ReportPair) -> ReportPair,
        }
        impl<C: Comparisons> Comparisons for Restating<'_, C> {
            fn report_pair(&mut self, pair: &ReportPair) -> Result<Receipt, AppendFailure> {
                let restated = (self.restate)(pair.clone());
                self.inner.report_pair(&restated)
            }

            fn holding_reading(
                &mut self,
                reading: &HoldingReading,
            ) -> Result<Receipt, AppendFailure> {
                self.inner.holding_reading(reading)
            }

            fn pair_not_built(
                &mut self,
                order: &rutent_contracts::router_api::Id,
                segment: &rutent_contracts::router_api::Id,
                why: &PairNotBuilt,
            ) -> Result<Receipt, AppendFailure> {
                self.inner.pair_not_built(order, segment, why)
            }

            fn holding_not_read(
                &mut self,
                account: &rutent_contracts::router_api::Id,
                node: &rutent_contracts::router_api::Id,
                provider: &rutent_contracts::router_api::Id,
                why: &rutent_contracts::core_internal::HoldingNotRead,
                last_check: Option<&Receipt>,
            ) -> Result<Receipt, AppendFailure> {
                self.inner
                    .holding_not_read(account, node, provider, why, last_check)
            }
        }
        let channel = rutent_core_orch::notice::Channel {
            notices: &mut self.notices,
            key: &mut self.notice_signer,
            transport: &mut self.wire,
        };
        let mut compare =
            Compare::new(
                Compare::<
                    super::CoreLog,
                    rutent_core_orch::notice::Channel<'_>,
                    crate::kit::SharedClock,
                >::core_audit(kit::core().party),
                General::new(&self.log),
                channel,
                self.clock.clone(),
            );
        self.segments.write_pairs(&mut Restating {
            inner: &mut compare,
            restate,
        })
    }
}

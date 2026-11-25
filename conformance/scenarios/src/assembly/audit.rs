//! The check of a holding account against what its orders hold, as the assembly runs it (`REQ-AUD-15`, `AC-39`;
//! `T-024-05`).
//!
//! Core's schedule reads each holding account and checks it (`T-020-48`, `schedule.rs`): the reading of a node's
//! account (`Core::holding_reading`), the real plain wallet's own figure beside the host's attribution, and the check
//! of a reading (`Core::check_holding`), which [`Compare`] records through the general handle on Core's log and
//! raises to the operator on Core's own channel, the notices'. Both only read; the check can reach nothing that moves
//! value. Neither is a call of the assembly's own: a scenario calls [`Core::tick`], and a reading is never handed in.

use rutent_contracts::core_internal::calls::{AppendFailure, Comparisons};
use rutent_contracts::core_internal::{HoldingReading, Receipt, ReportPair};
use rutent_core_audit::compare::Compare;
use rutent_core_audit::log::General;
use rutent_core_orch::snapshot::Clock as _;
use rutent_core_orch::stuck::OperatorSigns;
use rutent_prov::holding::BalanceSource;
use rutent_prov::holding::wallet::WalletError;
use serde_json::json;

use super::Core;
use crate::kit::{self, RealKey};

impl Core {
    /// The reading of the holding account of the public node `node`, held in an asset of `decimals` places, as
    /// `CMP-CORE-AUDIT` takes it: the balance the account's wallet states, asked of it, and what the host attributes
    /// to each order (`REQ-AUD-15`). The account is named `holding-` and its node.
    ///
    /// # Errors
    ///
    /// If the node has no account, or its wallet can't be read.
    ///
    /// # Panics
    ///
    /// If the clock's time can't be written as a timestamp.
    pub(super) fn holding_reading(
        &mut self,
        node: &str,
        decimals: u8,
    ) -> Result<HoldingReading, WalletError> {
        let at = self.clock.instant();
        self.fleet.platform.world.set_clock(at);
        let made_at = self
            .clock
            .now()
            .timestamp()
            .ok_or(WalletError::NotAnswered(
                rutent_contracts::module_interface::PlatformRefusal::Unavailable,
            ))?;
        let mut key = RealKey::assembly();
        let operator = OperatorSigns::new(&mut key, &self.bounds, kit::operator().acts, made_at);
        let read = self
            .holdings
            .borrow_mut()
            .readings(&mut self.fleet.platform, &operator)
            .into_iter()
            .find(|(n, _)| n == node)
            .ok_or_else(|| WalletError::NoHoldingAccount(node.to_owned()))?
            .1?;
        let provider = self
            .holdings
            .borrow()
            .account(node)
            .map(|account| account.provider().to_owned())
            .ok_or_else(|| WalletError::NoHoldingAccount(node.to_owned()))?;
        let word = match read.source {
            BalanceSource::Module => "checked",
            BalanceSource::Publisher(_) => "publisher",
        };
        Ok(serde_json::from_value(json!({
            "account": format!("holding-{node}"),
            "provider": provider,
            "node": read.node,
            "asset": { "id": read.asset, "decimals": decimals },
            "balance": read.balance.to_string(),
            "word": word,
            "held_for": read.held.iter()
                .map(|h| json!({ "order": h.order, "amount": h.amount.to_string() }))
                .collect::<Vec<_>>(),
            "at": self.clock.timestamp(),
        }))
        .expect("a reading of the contract's shape"))
    }

    /// Checks `reading` as `CMP-CORE-AUDIT` does: records the check, without its balance, in Core's log, and raises a
    /// difference to the operator on Core's channel. A match raises nothing. The receipt is of the check's record.
    ///
    /// # Errors
    ///
    /// If the log can't take the check's record or the raise's.
    pub(super) fn check_holding(
        &mut self,
        reading: &HoldingReading,
    ) -> Result<Receipt, AppendFailure> {
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
        compare.holding_reading(reading)
    }

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

use std::collections::{BTreeMap, VecDeque};

use crate::account::{AccountSummary, Balance};
use crate::command::{CommandReceipt, CommandStatus};
use crate::config::StatePolicy;
use crate::error::{ErrorKind, MarketError, Result};
use crate::execution::{DivergenceEvent, PrivateLaneEvent, PublicLaneEvent};
use crate::health::HealthReport;
use crate::ids::{AssetCode, InstrumentId, OrderId};
use crate::instrument::InstrumentSpec;
use crate::market::{
    BookTop, FastBookTop, FastKline, FastTicker, FastTrade, FundingRate, Kline, OpenInterest,
    Ticker, TradeTick,
};
use crate::position::Position;
use crate::reconcile::{AccountSnapshot, PrivateSnapshot, ReconcileOutcome, ReconcileReport};
use crate::trade::{Execution, Order};
use crate::types::{OrderStatus, Product, Venue};

/// In-memory engine state used by the facade.
#[derive(Clone, Debug)]
pub struct EngineState {
    venue: Venue,
    product: Product,
    state_policy: StatePolicy,
    instruments: BTreeMap<InstrumentId, InstrumentSpec>,
    tickers: BTreeMap<InstrumentId, Ticker>,
    recent_trades: BTreeMap<InstrumentId, VecDeque<TradeTick>>,
    book_tops: BTreeMap<InstrumentId, BookTop>,
    klines: BTreeMap<InstrumentId, Kline>,
    funding_rates: BTreeMap<InstrumentId, FundingRate>,
    open_interest: BTreeMap<InstrumentId, OpenInterest>,
    balances: BTreeMap<AssetCode, Balance>,
    account_summary: Option<AccountSummary>,
    positions: BTreeMap<InstrumentId, Position>,
    orders: BTreeMap<OrderId, Order>,
    executions: VecDeque<Execution>,
    health: HealthReport,
}

impl EngineState {
    #[must_use]
    pub fn new(
        venue: Venue,
        product: Product,
        state_policy: StatePolicy,
        instruments: impl IntoIterator<Item = InstrumentSpec>,
    ) -> Self {
        let instruments = instruments
            .into_iter()
            .map(|spec| (spec.instrument_id.clone(), spec))
            .collect();

        Self {
            venue,
            product,
            state_policy,
            instruments,
            tickers: BTreeMap::new(),
            recent_trades: BTreeMap::new(),
            book_tops: BTreeMap::new(),
            klines: BTreeMap::new(),
            funding_rates: BTreeMap::new(),
            open_interest: BTreeMap::new(),
            balances: BTreeMap::new(),
            account_summary: None,
            positions: BTreeMap::new(),
            orders: BTreeMap::new(),
            executions: VecDeque::new(),
            health: HealthReport::default(),
        }
    }

    pub fn apply_public_event(&mut self, event: PublicLaneEvent) -> Result<()> {
        match event {
            PublicLaneEvent::Ticker(ticker) => self.apply_fast_ticker(ticker)?,
            PublicLaneEvent::Trade(trade) => self.apply_fast_trade(trade)?,
            PublicLaneEvent::BookTop(book_top) => self.apply_fast_book_top(book_top)?,
            PublicLaneEvent::Kline(kline) => self.apply_fast_kline(kline)?,
            PublicLaneEvent::FundingRate(funding_rate) => {
                self.health.observe_public_message(funding_rate.event_time);
                self.funding_rates
                    .insert(funding_rate.instrument_id.clone(), funding_rate);
            }
            PublicLaneEvent::OpenInterest(open_interest) => {
                self.health.observe_public_message(open_interest.event_time);
                self.open_interest
                    .insert(open_interest.instrument_id.clone(), open_interest);
            }
            PublicLaneEvent::Divergence(divergence) => match divergence {
                DivergenceEvent::ReconcileRequired | DivergenceEvent::SequenceGap { .. } => {
                    self.health.mark_public_gap();
                }
                DivergenceEvent::StateDivergence => self.health.mark_state_divergence(),
            },
        }

        Ok(())
    }

    pub fn apply_private_event(&mut self, event: PrivateLaneEvent) {
        match event {
            PrivateLaneEvent::Balance(balance) => {
                self.health.observe_private_message(balance.updated_at);
                self.balances.insert(balance.asset.clone(), balance);
            }
            PrivateLaneEvent::Position(position) => {
                self.health.observe_private_message(position.updated_at);
                self.positions
                    .insert(position.instrument_id.clone(), position);
            }
            PrivateLaneEvent::Order(order) => {
                self.health.observe_private_message(order.updated_at);
                self.orders.insert(order.order_id.clone(), order);
            }
            PrivateLaneEvent::Execution(execution) => {
                self.health.observe_private_message(execution.executed_at);
                if self
                    .executions
                    .iter()
                    .any(|existing| existing.execution_id == execution.execution_id)
                {
                    return;
                }
                self.executions.push_back(execution);
                while self.executions.len() > self.state_policy.execution_capacity {
                    let _ = self.executions.pop_front();
                }
            }
            PrivateLaneEvent::Divergence(divergence) => match divergence {
                DivergenceEvent::ReconcileRequired => self.health.mark_reconcile_required(),
                DivergenceEvent::SequenceGap { .. } => self.health.mark_private_gap(),
                DivergenceEvent::StateDivergence => {
                    self.health.mark_state_divergence();
                }
            },
        }
    }

    pub fn apply_command_receipt(&mut self, receipt: &CommandReceipt) {
        match receipt.status {
            CommandStatus::Accepted => {}
            CommandStatus::Rejected => {}
            CommandStatus::UnknownExecution => self.health.mark_command_uncertain(),
        }
    }

    pub fn replace_instruments(&mut self, instruments: impl IntoIterator<Item = InstrumentSpec>) {
        self.instruments = instruments
            .into_iter()
            .map(|spec| (spec.instrument_id.clone(), spec))
            .collect();
        self.prune_orphan_market_state();
    }

    pub fn replace_account_snapshot(&mut self, snapshot: AccountSnapshot) {
        self.balances = snapshot
            .balances
            .into_iter()
            .map(|balance| (balance.asset.clone(), balance))
            .collect();
        self.account_summary = snapshot.summary;
        let updated_at = self
            .account_summary
            .as_ref()
            .map(|summary| summary.updated_at)
            .or_else(|| {
                self.balances
                    .values()
                    .map(|balance| balance.updated_at)
                    .max()
            });
        if let Some(updated_at) = updated_at {
            self.health.observe_private_message(updated_at);
        }
    }

    pub fn replace_positions(&mut self, positions: Vec<Position>) {
        let updated_at = positions.iter().map(|position| position.updated_at).max();
        self.positions = positions
            .into_iter()
            .map(|position| (position.instrument_id.clone(), position))
            .collect();
        if let Some(updated_at) = updated_at {
            self.health.observe_private_message(updated_at);
        }
    }

    pub fn replace_open_orders(&mut self, open_orders: Vec<Order>) {
        let updated_at = open_orders.iter().map(|order| order.updated_at).max();
        self.orders.retain(|_, order| {
            !matches!(
                order.status,
                OrderStatus::New | OrderStatus::PartiallyFilled
            )
        });
        for order in open_orders {
            self.orders.insert(order.order_id.clone(), order);
        }
        if let Some(updated_at) = updated_at {
            self.health.observe_private_message(updated_at);
        }
    }

    pub fn merge_order_history(&mut self, orders: Vec<Order>) {
        let updated_at = orders.iter().map(|order| order.updated_at).max();
        for order in orders {
            self.orders.insert(order.order_id.clone(), order);
        }
        if let Some(updated_at) = updated_at {
            self.health.observe_private_message(updated_at);
        }
    }

    pub fn merge_executions(&mut self, executions: Vec<Execution>) {
        let updated_at = executions
            .iter()
            .map(|execution| execution.executed_at)
            .max();
        for execution in executions {
            if self
                .executions
                .iter()
                .any(|existing| existing.execution_id == execution.execution_id)
            {
                continue;
            }
            self.executions.push_back(execution);
            while self.executions.len() > self.state_policy.execution_capacity {
                let _ = self.executions.pop_front();
            }
        }
        if let Some(updated_at) = updated_at {
            self.health.observe_private_message(updated_at);
        }
    }

    pub fn apply_private_snapshot(&mut self, snapshot: PrivateSnapshot) {
        if let Some(account) = snapshot.account {
            self.replace_account_snapshot(account);
        }
        self.replace_positions(snapshot.positions);
        self.replace_open_orders(snapshot.open_orders);
    }

    pub fn apply_reconcile_report(&mut self, report: &ReconcileReport) {
        match report.outcome {
            ReconcileOutcome::Synchronized => self.health.mark_reconcile_complete(),
            ReconcileOutcome::StillUncertain => self.health.mark_reconcile_required(),
            ReconcileOutcome::Diverged => self.health.mark_state_divergence(),
        }
    }

    pub fn mark_rest_success(&mut self, clock_skew_ms: Option<i64>) {
        self.health.observe_rest_success(clock_skew_ms);
    }

    pub fn mark_public_disconnect(&mut self) {
        self.health.mark_public_disconnect();
    }

    pub fn mark_private_disconnect(&mut self) {
        self.health.mark_private_disconnect();
    }

    pub fn mark_reconnect(&mut self) {
        self.health.mark_reconnect();
    }

    pub fn mark_snapshot_age(&mut self, age_ms: u64, stale_after_ms: u64) {
        self.health.mark_snapshot_age(age_ms, stale_after_ms);
    }

    #[must_use]
    pub fn ticker(&self, instrument_id: &InstrumentId) -> Option<&Ticker> {
        self.tickers.get(instrument_id)
    }

    #[must_use]
    pub fn recent_trades(&self, instrument_id: &InstrumentId) -> Option<Vec<TradeTick>> {
        self.recent_trades
            .get(instrument_id)
            .map(|trades| trades.iter().cloned().collect())
    }

    #[must_use]
    pub fn book_top(&self, instrument_id: &InstrumentId) -> Option<&BookTop> {
        self.book_tops.get(instrument_id)
    }

    #[must_use]
    pub fn funding_rate(&self, instrument_id: &InstrumentId) -> Option<&FundingRate> {
        self.funding_rates.get(instrument_id)
    }

    #[must_use]
    pub fn open_interest(&self, instrument_id: &InstrumentId) -> Option<&OpenInterest> {
        self.open_interest.get(instrument_id)
    }

    #[must_use]
    pub fn balances(&self) -> Vec<Balance> {
        self.balances.values().cloned().collect()
    }

    #[must_use]
    pub fn account_summary(&self) -> Option<AccountSummary> {
        if let Some(summary) = &self.account_summary {
            return Some(summary.clone());
        }

        let mut balances = self.balances.values();
        let first = balances.next()?;
        let mut wallet = first.wallet_balance;
        let mut available = first.available_balance;

        for balance in balances {
            wallet = wallet
                .value()
                .checked_add(balance.wallet_balance.value())
                .map(Into::into)?;
            available = available
                .value()
                .checked_add(balance.available_balance.value())
                .map(Into::into)?;
        }

        Some(AccountSummary {
            total_wallet_balance: wallet,
            total_available_balance: available,
            total_unrealized_pnl: crate::numeric::Amount::new(0.into()),
            updated_at: first.updated_at,
        })
    }

    #[must_use]
    pub fn positions(&self) -> Vec<Position> {
        self.positions.values().cloned().collect()
    }

    #[must_use]
    pub fn orders(&self) -> Vec<Order> {
        self.orders.values().cloned().collect()
    }

    #[must_use]
    pub fn open_orders(&self) -> Vec<Order> {
        self.orders
            .values()
            .filter(|order| {
                matches!(
                    order.status,
                    OrderStatus::New | OrderStatus::PartiallyFilled
                )
            })
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn executions(&self) -> Vec<Execution> {
        self.executions.iter().cloned().collect()
    }

    #[must_use]
    pub fn latest_order_update_at(
        &self,
        instrument_id: &InstrumentId,
    ) -> Option<crate::primitives::TimestampMs> {
        self.orders
            .values()
            .filter(|order| &order.instrument_id == instrument_id)
            .map(|order| order.updated_at)
            .max()
    }

    #[must_use]
    pub fn latest_execution_at(
        &self,
        instrument_id: &InstrumentId,
    ) -> Option<crate::primitives::TimestampMs> {
        self.executions
            .iter()
            .filter(|execution| &execution.instrument_id == instrument_id)
            .map(|execution| execution.executed_at)
            .max()
    }

    #[must_use]
    pub const fn health(&self) -> &HealthReport {
        &self.health
    }

    #[must_use]
    pub fn instrument_specs(&self) -> Vec<InstrumentSpec> {
        self.instruments.values().cloned().collect()
    }

    #[must_use]
    pub const fn venue(&self) -> Venue {
        self.venue
    }

    #[must_use]
    pub const fn product(&self) -> Product {
        self.product
    }

    fn apply_fast_ticker(&mut self, ticker: FastTicker) -> Result<()> {
        let unified = {
            let spec = self.spec(&ticker.instrument_id)?;
            ticker.to_unified(spec)
        };
        self.health.observe_public_message(ticker.event_time);
        self.tickers.insert(ticker.instrument_id.clone(), unified);
        Ok(())
    }

    fn apply_fast_trade(&mut self, trade: FastTrade) -> Result<()> {
        let unified = {
            let spec = self.spec(&trade.instrument_id)?;
            trade.to_unified(spec)
        };
        self.health.observe_public_message(trade.event_time);
        let entry = self
            .recent_trades
            .entry(trade.instrument_id.clone())
            .or_default();
        if entry
            .back()
            .is_some_and(|existing| existing.trade_id == unified.trade_id)
        {
            return Ok(());
        }
        entry.push_back(unified);
        while entry.len() > self.state_policy.recent_trade_capacity {
            let _ = entry.pop_front();
        }
        Ok(())
    }

    fn apply_fast_book_top(&mut self, book_top: FastBookTop) -> Result<()> {
        let unified = {
            let spec = self.spec(&book_top.instrument_id)?;
            book_top.to_unified(spec)
        };
        self.health.observe_public_message(book_top.event_time);
        self.book_tops
            .insert(book_top.instrument_id.clone(), unified);
        Ok(())
    }

    fn apply_fast_kline(&mut self, kline: FastKline) -> Result<()> {
        let unified = {
            let spec = self.spec(&kline.instrument_id)?;
            kline.to_unified(spec)
        };
        self.health.observe_public_message(kline.close_time);
        self.klines.insert(kline.instrument_id.clone(), unified);
        Ok(())
    }

    fn spec(&self, instrument_id: &InstrumentId) -> Result<&InstrumentSpec> {
        self.instruments.get(instrument_id).ok_or_else(|| {
            MarketError::new(
                ErrorKind::ConfigError,
                format!(
                    "unknown instrument {instrument_id} for {} {}",
                    self.venue, self.product
                ),
            )
        })
    }

    fn prune_orphan_market_state(&mut self) {
        let retain_instruments =
            |instrument_id: &InstrumentId, instruments: &BTreeMap<InstrumentId, InstrumentSpec>| {
                instruments.contains_key(instrument_id)
            };

        self.tickers
            .retain(|instrument_id, _| retain_instruments(instrument_id, &self.instruments));
        self.recent_trades
            .retain(|instrument_id, _| retain_instruments(instrument_id, &self.instruments));
        self.book_tops
            .retain(|instrument_id, _| retain_instruments(instrument_id, &self.instruments));
        self.klines
            .retain(|instrument_id, _| retain_instruments(instrument_id, &self.instruments));
        self.funding_rates
            .retain(|instrument_id, _| retain_instruments(instrument_id, &self.instruments));
        self.open_interest
            .retain(|instrument_id, _| retain_instruments(instrument_id, &self.instruments));
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use crate::ids::{AssetCode, ClientOrderId, InstrumentId};
    use crate::instrument::{InstrumentSpec, InstrumentStatus, InstrumentSupport};
    use crate::numeric::{Notional, Price, Quantity};
    use crate::primitives::TimestampMs;
    use crate::trade::Order;
    use crate::types::{MarketType, OrderStatus, OrderType, Product, Side, Venue};
    use crate::{
        execution::DivergenceEvent,
        health::{DegradedReason, HealthStatus},
        reconcile::{ReconcileOutcome, ReconcileReport, ReconcileTrigger},
    };

    use super::EngineState;

    #[test]
    fn open_orders_excludes_terminal_statuses() {
        let instrument_id = InstrumentId::from("BTC/USDT:USDT");
        let mut state = EngineState::new(
            Venue::Binance,
            Product::LinearUsdt,
            crate::config::StatePolicy {
                recent_trade_capacity: 16,
                execution_capacity: 16,
            },
            [InstrumentSpec {
                venue: Venue::Binance,
                product: Product::LinearUsdt,
                market_type: MarketType::LinearPerpetual,
                instrument_id: instrument_id.clone(),
                canonical_symbol: "BTC/USDT:USDT".into(),
                native_symbol: "BTCUSDT".into(),
                base: AssetCode::from("BTC"),
                quote: AssetCode::from("USDT"),
                settle: AssetCode::from("USDT"),
                contract_size: Quantity::new(Decimal::ONE),
                tick_size: Price::new(Decimal::new(1, 2)),
                step_size: Quantity::new(Decimal::new(1, 3)),
                min_qty: Quantity::new(Decimal::new(1, 3)),
                min_notional: Notional::new(Decimal::new(5, 0)),
                price_scale: 2,
                qty_scale: 3,
                quote_scale: 2,
                max_leverage: None,
                support: InstrumentSupport {
                    public_streams: true,
                    private_trading: true,
                    leverage_set: true,
                    margin_mode_set: true,
                    funding_rate: true,
                    open_interest: true,
                },
                status: InstrumentStatus::Active,
            }],
        );

        state.apply_private_event(crate::execution::PrivateLaneEvent::Order(Order {
            order_id: "open-order".into(),
            client_order_id: Some(ClientOrderId::from("open-client")),
            instrument_id: instrument_id.clone(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: None,
            status: OrderStatus::New,
            price: Some(Price::new(Decimal::new(70_000, 0))),
            quantity: Quantity::new(Decimal::new(1, 3)),
            filled_quantity: Quantity::new(Decimal::ZERO),
            average_fill_price: None,
            reduce_only: false,
            post_only: false,
            created_at: TimestampMs::new(1),
            updated_at: TimestampMs::new(1),
            venue_status: None,
        }));
        state.apply_private_event(crate::execution::PrivateLaneEvent::Order(Order {
            order_id: "done-order".into(),
            client_order_id: Some(ClientOrderId::from("done-client")),
            instrument_id,
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: None,
            status: OrderStatus::Filled,
            price: Some(Price::new(Decimal::new(70_000, 0))),
            quantity: Quantity::new(Decimal::new(1, 3)),
            filled_quantity: Quantity::new(Decimal::new(1, 3)),
            average_fill_price: Some(Price::new(Decimal::new(70_000, 0))),
            reduce_only: false,
            post_only: false,
            created_at: TimestampMs::new(1),
            updated_at: TimestampMs::new(2),
            venue_status: None,
        }));

        assert_eq!(state.orders().len(), 2);
        assert_eq!(state.open_orders().len(), 1);
    }

    #[test]
    fn reconcile_report_clears_state_divergence_after_gap() {
        let instrument_id = InstrumentId::from("BTC/USDT:USDT");
        let mut state = EngineState::new(
            Venue::Binance,
            Product::LinearUsdt,
            crate::config::StatePolicy {
                recent_trade_capacity: 16,
                execution_capacity: 16,
            },
            [InstrumentSpec {
                venue: Venue::Binance,
                product: Product::LinearUsdt,
                market_type: MarketType::LinearPerpetual,
                instrument_id: instrument_id.clone(),
                canonical_symbol: "BTC/USDT:USDT".into(),
                native_symbol: "BTCUSDT".into(),
                base: AssetCode::from("BTC"),
                quote: AssetCode::from("USDT"),
                settle: AssetCode::from("USDT"),
                contract_size: Quantity::new(Decimal::ONE),
                tick_size: Price::new(Decimal::new(1, 2)),
                step_size: Quantity::new(Decimal::new(1, 3)),
                min_qty: Quantity::new(Decimal::new(1, 3)),
                min_notional: Notional::new(Decimal::new(5, 0)),
                price_scale: 2,
                qty_scale: 3,
                quote_scale: 2,
                max_leverage: None,
                support: InstrumentSupport {
                    public_streams: true,
                    private_trading: true,
                    leverage_set: true,
                    margin_mode_set: true,
                    funding_rate: true,
                    open_interest: true,
                },
                status: InstrumentStatus::Active,
            }],
        );

        state.apply_private_event(crate::execution::PrivateLaneEvent::Divergence(
            DivergenceEvent::SequenceGap { at: None },
        ));
        assert_eq!(state.health().status, HealthStatus::ReconcileRequired);
        assert_eq!(
            state.health().degraded_reason,
            Some(DegradedReason::PrivateStreamGap)
        );

        state.apply_reconcile_report(&ReconcileReport {
            trigger: ReconcileTrigger::SequenceGap,
            outcome: ReconcileOutcome::Synchronized,
            repaired_at: TimestampMs::new(5),
            note: Some("unit reconcile".into()),
        });
        assert_eq!(state.health().status, HealthStatus::Disconnected);
        assert_eq!(
            state.health().degraded_reason,
            Some(DegradedReason::Disconnected)
        );
        assert!(!state.health().state_divergence);
    }
}

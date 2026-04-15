//! Shared fixtures, smoke helpers, and integration-style tests.

use bat_markets::types::{Product, Venue};
use bat_markets::{BatMarkets, BatMarketsBuilder};

/// Binance fixture payloads.
pub mod binance {
    pub const PUBLIC_TICKER: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_ticker.json"
    ));
    pub const PUBLIC_TRADE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_trade.json"
    ));
    pub const PUBLIC_BOOK_TICKER: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_book_ticker.json"
    ));
    pub const PUBLIC_MARK_PRICE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_mark_price.json"
    ));
    pub const PUBLIC_KLINE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_kline.json"
    ));
    pub const OPEN_INTEREST: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/open_interest.json"
    ));
    pub const PRIVATE_ACCOUNT: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/private_account_update.json"
    ));
    pub const PRIVATE_ORDER: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/private_order_trade_update.json"
    ));
    pub const COMMAND_CREATE_OK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/command_create_ok.json"
    ));
    pub const COMMAND_REJECT: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/command_reject.json"
    ));
}

/// Bybit fixture payloads.
pub mod bybit {
    pub const PUBLIC_TICKER: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/public_ticker.json"
    ));
    pub const PUBLIC_TRADE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/public_trade.json"
    ));
    pub const PUBLIC_ORDERBOOK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/public_orderbook.json"
    ));
    pub const PUBLIC_KLINE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/public_kline.json"
    ));
    pub const PRIVATE_WALLET: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_wallet.json"
    ));
    pub const PRIVATE_POSITION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_position.json"
    ));
    pub const PRIVATE_ORDER: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_order.json"
    ));
    pub const PRIVATE_ORDER_CANCELED: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_order_canceled.json"
    ));
    pub const PRIVATE_EXECUTION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_execution.json"
    ));
    pub const PRIVATE_EXECUTION_LATE_AFTER_CANCEL: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_execution_late_after_cancel.json"
    ));
    pub const PUBLIC_ORDERBOOK_GAP: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/public_orderbook_gap.json"
    ));
    pub const COMMAND_CREATE_OK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/command_create_ok.json"
    ));
    pub const COMMAND_REJECT: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/command_reject.json"
    ));
}

#[must_use]
pub fn build_binance() -> BatMarkets {
    BatMarketsBuilder::default()
        .venue(Venue::Binance)
        .product(Product::LinearUsdt)
        .build()
        .unwrap_or_else(|error| panic!("failed to build binance fixture engine: {error}"))
}

#[must_use]
pub fn build_bybit() -> BatMarkets {
    BatMarketsBuilder::default()
        .venue(Venue::Bybit)
        .product(Product::LinearUsdt)
        .build()
        .unwrap_or_else(|error| panic!("failed to build bybit fixture engine: {error}"))
}

#[must_use]
pub fn has_binance_live_env() -> bool {
    std::env::var_os("BINANCE_API_KEY").is_some()
        && std::env::var_os("BINANCE_API_SECRET").is_some()
}

#[must_use]
pub fn has_bybit_live_env() -> bool {
    std::env::var_os("BYBIT_API_KEY").is_some() && std::env::var_os("BYBIT_API_SECRET").is_some()
}

#[cfg(test)]
mod tests {
    use tokio::sync::broadcast::error::TryRecvError;

    use bat_markets::{BatMarkets, errors::Result};
    use bat_markets_core::{CommandOperation, CommandStatus, HealthStatus, OrderStatus};

    use super::{binance, build_binance, build_bybit, bybit};

    #[test]
    fn binance_fixtures_drive_market_and_private_state() -> Result<()> {
        let client = build_binance();
        ingest_binance(&client)?;

        let instrument = bat_markets::types::InstrumentId::from("BTC/USDT:USDT");
        let ticker = client
            .market()
            .ticker(&instrument)
            .expect("binance ticker missing after ingest");
        assert_eq!(ticker.last_price.to_string(), "70100.50");
        assert_eq!(client.account().balances().len(), 1);
        assert_eq!(client.position().list().len(), 1);
        assert_eq!(client.trade().open_orders().len(), 1);
        assert_eq!(client.trade().executions().len(), 1);
        assert_eq!(client.health().snapshot().status, HealthStatus::Healthy);

        Ok(())
    }

    #[test]
    fn bybit_fixtures_drive_market_and_private_state() -> Result<()> {
        let client = build_bybit();
        ingest_bybit(&client)?;

        let instrument = bat_markets::types::InstrumentId::from("BTC/USDT:USDT");
        let ticker = client
            .market()
            .ticker(&instrument)
            .expect("bybit ticker missing after ingest");
        assert_eq!(
            ticker.mark_price.expect("mark price missing").to_string(),
            "70108.50"
        );
        assert_eq!(client.account().balances().len(), 1);
        assert_eq!(client.position().list().len(), 1);
        assert_eq!(client.trade().open_orders().len(), 1);
        assert_eq!(client.trade().executions().len(), 1);
        assert_eq!(client.health().snapshot().status, HealthStatus::Healthy);

        Ok(())
    }

    #[test]
    fn command_lane_surfaces_unknown_execution() -> Result<()> {
        let client = build_binance();
        let receipt =
            client
                .stream()
                .command()
                .classify_json(CommandOperation::CreateOrder, None, None)?;

        assert_eq!(receipt.status, CommandStatus::UnknownExecution);
        assert_eq!(
            client.health().snapshot().status,
            HealthStatus::CommandUncertain
        );

        Ok(())
    }

    #[test]
    fn reject_paths_stay_explicit() -> Result<()> {
        let binance = build_binance();
        let receipt = binance.stream().command().classify_json(
            CommandOperation::CreateOrder,
            Some(binance::COMMAND_REJECT),
            None,
        )?;
        assert_eq!(receipt.status, CommandStatus::Rejected);
        assert_eq!(receipt.native_code.as_deref(), Some("-2019"));

        let bybit = build_bybit();
        let receipt = bybit.stream().command().classify_json(
            CommandOperation::CreateOrder,
            Some(bybit::COMMAND_REJECT),
            None,
        )?;
        assert_eq!(receipt.status, CommandStatus::Rejected);
        assert_eq!(receipt.native_code.as_deref(), Some("110007"));

        Ok(())
    }

    #[test]
    fn duplicate_private_execution_is_idempotent() -> Result<()> {
        let client = build_binance();
        let stream = client.stream();
        stream.private().ingest_json(binance::PRIVATE_ORDER)?;
        stream.private().ingest_json(binance::PRIVATE_ORDER)?;

        assert_eq!(client.trade().executions().len(), 1);
        assert_eq!(client.trade().orders().len(), 1);

        Ok(())
    }

    #[test]
    fn duplicate_public_trade_is_coalesced() -> Result<()> {
        let client = build_bybit();
        let instrument = bat_markets::types::InstrumentId::from("BTC/USDT:USDT");
        let stream = client.stream();
        stream.public().ingest_json(bybit::PUBLIC_TRADE)?;
        stream.public().ingest_json(bybit::PUBLIC_TRADE)?;

        let trades = client
            .market()
            .recent_trades(&instrument)
            .expect("recent trades missing after duplicate public ingest");
        assert_eq!(trades.len(), 1);

        Ok(())
    }

    #[test]
    fn contradictory_command_and_stream_outcome_stays_explicit() -> Result<()> {
        let client = build_binance();
        let receipt =
            client
                .stream()
                .command()
                .classify_json(CommandOperation::CreateOrder, None, None)?;
        assert_eq!(receipt.status, CommandStatus::UnknownExecution);

        client
            .stream()
            .private()
            .ingest_json(binance::PRIVATE_ORDER)?;

        assert_eq!(client.trade().executions().len(), 1);
        assert_eq!(
            client.health().snapshot().status,
            HealthStatus::CommandUncertain
        );

        Ok(())
    }

    #[test]
    fn health_notifications_emit_only_for_structural_changes() -> Result<()> {
        let client = build_binance();
        let mut notifications = client.health().notifications();
        let stream = client.stream();

        stream.public().ingest_json(binance::PUBLIC_TICKER)?;
        let first = notifications
            .try_recv()
            .expect("first public transition should emit a health notification");
        assert_eq!(first.previous.status, HealthStatus::Disconnected);
        assert_eq!(first.current.status, HealthStatus::Healthy);
        assert!(first.current.ws_public_ok);

        stream.public().ingest_json(binance::PUBLIC_BOOK_TICKER)?;
        assert!(matches!(notifications.try_recv(), Err(TryRecvError::Empty)));

        Ok(())
    }

    #[test]
    fn late_fill_after_cancel_stays_explicit() -> Result<()> {
        let client = build_bybit();
        let stream = client.stream();
        stream.private().ingest_json(bybit::PRIVATE_ORDER)?;
        stream
            .private()
            .ingest_json(bybit::PRIVATE_ORDER_CANCELED)?;
        stream
            .private()
            .ingest_json(bybit::PRIVATE_EXECUTION_LATE_AFTER_CANCEL)?;

        let orders = client.trade().orders();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].status, OrderStatus::Canceled);
        assert_eq!(client.trade().executions().len(), 1);

        Ok(())
    }

    fn ingest_binance(client: &BatMarkets) -> Result<()> {
        let stream = client.stream();
        stream.public().ingest_json(binance::PUBLIC_TICKER)?;
        stream.public().ingest_json(binance::PUBLIC_TRADE)?;
        stream.public().ingest_json(binance::PUBLIC_BOOK_TICKER)?;
        stream.public().ingest_json(binance::PUBLIC_MARK_PRICE)?;
        stream.public().ingest_json(binance::PUBLIC_KLINE)?;
        stream.public().ingest_json(binance::OPEN_INTEREST)?;
        stream.private().ingest_json(binance::PRIVATE_ACCOUNT)?;
        stream.private().ingest_json(binance::PRIVATE_ORDER)?;
        let receipt = stream.command().classify_json(
            CommandOperation::CreateOrder,
            Some(binance::COMMAND_CREATE_OK),
            None,
        )?;
        assert_eq!(receipt.status, CommandStatus::Accepted);
        Ok(())
    }

    fn ingest_bybit(client: &BatMarkets) -> Result<()> {
        let stream = client.stream();
        stream.public().ingest_json(bybit::PUBLIC_TICKER)?;
        stream.public().ingest_json(bybit::PUBLIC_TRADE)?;
        stream.public().ingest_json(bybit::PUBLIC_ORDERBOOK)?;
        stream.public().ingest_json(bybit::PUBLIC_KLINE)?;
        stream.private().ingest_json(bybit::PRIVATE_WALLET)?;
        stream.private().ingest_json(bybit::PRIVATE_POSITION)?;
        stream.private().ingest_json(bybit::PRIVATE_ORDER)?;
        stream.private().ingest_json(bybit::PRIVATE_EXECUTION)?;
        let receipt = stream.command().classify_json(
            CommandOperation::CreateOrder,
            Some(bybit::COMMAND_CREATE_OK),
            None,
        )?;
        assert_eq!(receipt.status, CommandStatus::Accepted);
        Ok(())
    }
}

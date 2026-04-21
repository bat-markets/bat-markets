use bat_markets::{
    BatMarketsBuilder,
    errors::Result,
    types::{CommandTransport, OrderStatus, PositionDirection, Venue},
};
use bat_markets_testing::{
    LiveTestEndpointMode, binance_mainnet_trade_cycle_enabled, has_binance_live_env,
    live_test_config, run_binance_trade_cycle,
};

#[tokio::test]
async fn binance_mainnet_trade_cycle_streams_and_commands_are_live_and_consistent() -> Result<()> {
    if !has_binance_live_env() || !binance_mainnet_trade_cycle_enabled() {
        return Ok(());
    }

    let client = BatMarketsBuilder::default()
        .config(live_test_config(
            Venue::Binance,
            LiveTestEndpointMode::Mainnet,
        ))
        .build_live()
        .await?;

    let Some(report) = run_binance_trade_cycle(&client).await? else {
        return Ok(());
    };

    assert_eq!(report.open_order.status, OrderStatus::Filled);
    assert_eq!(report.close_order.status, OrderStatus::Filled);
    assert_eq!(
        report.open_execution.client_order_id,
        report.open_order.client_order_id
    );
    assert_eq!(
        report.close_execution.client_order_id,
        report.close_order.client_order_id
    );
    assert!(matches!(
        report.maker_buy_order.status,
        OrderStatus::New | OrderStatus::PartiallyFilled
    ));
    assert!(matches!(
        report.maker_buy_cancel.status,
        OrderStatus::Canceled | OrderStatus::Expired | OrderStatus::PendingCancel
    ));
    assert!(matches!(
        report.maker_sell_order.status,
        OrderStatus::New | OrderStatus::PartiallyFilled
    ));
    assert!(matches!(
        report.maker_sell_cancel.status,
        OrderStatus::Canceled | OrderStatus::Expired | OrderStatus::PendingCancel
    ));
    assert_eq!(report.open_transport, CommandTransport::WebSocket);
    assert_eq!(report.close_transport, CommandTransport::WebSocket);
    assert_eq!(report.maker_buy_transport, CommandTransport::WebSocket);
    assert_eq!(
        report.maker_buy_cancel_transport,
        CommandTransport::WebSocket
    );
    assert_eq!(report.maker_sell_transport, CommandTransport::WebSocket);
    assert_eq!(
        report.maker_sell_cancel_transport,
        CommandTransport::WebSocket
    );
    assert!(report.final_open_orders.is_empty());
    assert_eq!(report.final_position.direction, PositionDirection::Flat);
    assert!(report.final_position.size.value().is_zero());
    assert!(
        report
            .refreshed_executions
            .iter()
            .any(|execution| execution.order_id == report.open_execution.order_id)
    );
    assert!(
        report
            .refreshed_executions
            .iter()
            .any(|execution| execution.order_id == report.close_execution.order_id)
    );
    assert!(report.event_counts.orders >= 4);
    assert!(report.event_counts.executions >= 2);
    assert_eq!(report.balance_update.asset.as_ref(), "USDT");
    assert!(report.account_update.total_wallet_balance.value() > rust_decimal::Decimal::ZERO);
    assert!(report.diagnostics.create_order.operations >= 3);
    assert!(report.diagnostics.cancel_order.operations >= 2);
    assert!(report.diagnostics.close_position.operations >= 1);
    assert!(report.diagnostics.validate_order.operations >= 3);
    assert!(report.diagnostics.get_order.operations >= 2);
    assert!(report.diagnostics.refresh_account.operations >= 2);
    assert!(report.diagnostics.refresh_open_orders.operations >= 2);
    assert!(report.diagnostics.refresh_positions.operations >= 2);
    assert!(report.diagnostics.refresh_executions.operations >= 2);

    Ok(())
}

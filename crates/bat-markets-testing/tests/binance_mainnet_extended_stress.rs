use bat_markets::{
    BatMarketsBuilder,
    errors::Result,
    types::{CommandTransport, OrderType, PositionDirection, Venue},
};
use bat_markets_testing::{
    LiveTestEndpointMode, binance_mainnet_extended_stress_enabled, has_binance_live_env,
    live_test_config, run_binance_extended_stress,
};

#[tokio::test]
async fn binance_mainnet_extended_stress_is_live_ws_first_and_consistent() -> Result<()> {
    if !has_binance_live_env() || !binance_mainnet_extended_stress_enabled() {
        return Ok(());
    }

    let client = BatMarketsBuilder::default()
        .config(live_test_config(
            Venue::Binance,
            LiveTestEndpointMode::Mainnet,
        ))
        .build_live()
        .await?;

    let Some(report) = run_binance_extended_stress(&client).await? else {
        return Ok(());
    };

    assert_eq!(report.open_transport, CommandTransport::WebSocket);
    assert_eq!(report.close_transport, CommandTransport::WebSocket);
    assert_eq!(report.protective.stop_transport, CommandTransport::Rest);
    assert_eq!(
        report.protective.take_profit_transport,
        CommandTransport::Rest
    );
    assert_eq!(
        report.protective.stop_cancel_transport,
        CommandTransport::Rest
    );
    assert_eq!(
        report.protective.take_profit_cancel_transport,
        CommandTransport::Rest
    );
    assert_eq!(
        report.protective.stop_order.order_type,
        OrderType::StopMarket
    );
    assert_eq!(
        report.protective.take_profit_order.order_type,
        OrderType::TakeProfitMarket
    );
    assert!(!report.burst_rounds.is_empty());
    assert!(report.burst_rounds.iter().all(|round| {
        round
            .create_transports
            .iter()
            .all(|transport| *transport == CommandTransport::WebSocket)
    }));
    assert!(report.burst_rounds.iter().all(|round| {
        round
            .cancel_transports
            .iter()
            .all(|transport| *transport == CommandTransport::WebSocket)
    }));
    assert!(
        report
            .burst_rounds
            .iter()
            .all(|round| round.create_stream.samples == round.create_transports.len())
    );
    assert!(
        report
            .burst_rounds
            .iter()
            .all(|round| round.cancel_stream.samples == round.cancel_transports.len())
    );
    assert!(report.hft_create_ack.samples >= report.burst_rounds.len());
    assert!(report.hft_cancel_ack.samples >= report.burst_rounds.len());
    assert!(report.hft_create_stream.samples >= report.burst_rounds.len());
    assert!(report.hft_cancel_stream.samples >= report.burst_rounds.len());
    assert_eq!(report.final_position.direction, PositionDirection::Flat);
    assert!(report.final_position.size.value().is_zero());
    assert!(report.final_open_orders.is_empty());
    assert!(report.account_update.total_wallet_balance.value() > rust_decimal::Decimal::ZERO);
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
    assert!(
        report.event_counts.orders
            >= 4 + report
                .burst_rounds
                .iter()
                .map(|round| round.create_transports.len() * 2)
                .sum::<usize>()
    );

    Ok(())
}

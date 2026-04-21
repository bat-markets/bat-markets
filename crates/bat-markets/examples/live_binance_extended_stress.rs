use bat_markets::{BatMarketsBuilder, errors::Result, types::Venue};
use bat_markets_testing::{LiveTestEndpointMode, live_test_config, run_binance_extended_stress};

#[tokio::main]
async fn main() -> Result<()> {
    let client = BatMarketsBuilder::default()
        .config(live_test_config(
            Venue::Binance,
            LiveTestEndpointMode::Mainnet,
        ))
        .build_live()
        .await?;

    let Some(report) = run_binance_extended_stress(&client).await? else {
        println!("extended stress plan could not be prepared");
        return Ok(());
    };

    println!(
        "instrument={} market_qty={} burst_qty={}",
        report.instrument_id, report.market_quantity, report.burst_quantity
    );
    println!(
        "open_transport={:?} close_transport={:?} stop_transport={:?} take_profit_transport={:?}",
        report.open_transport,
        report.close_transport,
        report.protective.stop_transport,
        report.protective.take_profit_transport
    );
    println!(
        "position_open_ns={} streamed={} position_flat_ns={} streamed={}",
        report.position_open_latency.latency_ns,
        report.position_open_latency.streamed,
        report.position_flat_latency.latency_ns,
        report.position_flat_latency.streamed
    );
    println!(
        "open_execution_ns={} streamed={} close_execution_ns={} streamed={}",
        report.open_execution_latency.latency_ns,
        report.open_execution_latency.streamed,
        report.close_execution_latency.latency_ns,
        report.close_execution_latency.streamed
    );
    if let Some(balance_latency) = &report.balance_latency {
        println!(
            "balance_latency_ns={} streamed={}",
            balance_latency.latency_ns, balance_latency.streamed
        );
    }
    if let Some(account_latency) = &report.account_latency {
        println!(
            "account_latency_ns={} streamed={}",
            account_latency.latency_ns, account_latency.streamed
        );
    }
    for round in &report.burst_rounds {
        println!(
            "round={} create_ack_ns={} create_stream_avg_ns={} cancel_ack_ns={} cancel_stream_avg_ns={}",
            round.round,
            round.create_ack_ns,
            round.create_stream.avg_ns,
            round.cancel_ack_ns,
            round.cancel_stream.avg_ns
        );
    }
    println!(
        "hft_create_ack_avg_ns={} hft_create_stream_avg_ns={} hft_cancel_ack_avg_ns={} hft_cancel_stream_avg_ns={}",
        report.hft_create_ack.avg_ns,
        report.hft_create_stream.avg_ns,
        report.hft_cancel_ack.avg_ns,
        report.hft_cancel_stream.avg_ns
    );
    println!(
        "events orders={} executions={} positions={} balances={} account={}",
        report.event_counts.orders,
        report.event_counts.executions,
        report.event_counts.positions,
        report.event_counts.balances,
        report.event_counts.account
    );
    println!(
        "final_position={:?} final_size={} final_open_orders={}",
        report.final_position.direction,
        report.final_position.size,
        report.final_open_orders.len()
    );
    println!(
        "diagnostics create_order_avg_ns={} create_orders_avg_ns={} cancel_order_avg_ns={} cancel_orders_avg_ns={} close_position_avg_ns={}",
        report.diagnostics.create_order.average_ns(),
        report.diagnostics.create_orders.average_ns(),
        report.diagnostics.cancel_order.average_ns(),
        report.diagnostics.cancel_orders.average_ns(),
        report.diagnostics.close_position.average_ns()
    );

    Ok(())
}

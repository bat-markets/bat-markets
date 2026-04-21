use bat_markets::{BatMarketsBuilder, errors::Result, types::Venue};
use bat_markets_testing::{
    LiveTestEndpointMode, has_binance_live_env, live_test_config, run_binance_trade_cycle,
};

#[tokio::main]
async fn main() -> Result<()> {
    if !has_binance_live_env() {
        println!("live trade cycle skipped: missing BINANCE_API_KEY/BINANCE_API_SECRET");
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
        println!("live trade cycle skipped: no eligible symbol or insufficient balance");
        return Ok(());
    };

    println!(
        "instrument={} qty={} maker_buy={} maker_sell={}",
        report.instrument_id,
        report.market_quantity,
        report.maker_buy_price,
        report.maker_sell_price,
    );
    println!(
        "open_status={:?} close_status={:?} maker_buy_status={:?} maker_buy_cancel={:?} maker_sell_status={:?} maker_sell_cancel={:?}",
        report.open_order.status,
        report.close_order.status,
        report.maker_buy_order.status,
        report.maker_buy_cancel.status,
        report.maker_sell_order.status,
        report.maker_sell_cancel.status,
    );
    println!(
        "events orders={} executions={} positions={} balances={} account={}",
        report.event_counts.orders,
        report.event_counts.executions,
        report.event_counts.positions,
        report.event_counts.balances,
        report.event_counts.account,
    );
    println!(
        "final_position={:?} final_size={} final_open_orders={}",
        report.final_position.direction,
        report.final_position.size,
        report.final_open_orders.len(),
    );
    println!(
        "diagnostics create_order_avg_ns={} cancel_order_avg_ns={} close_position_avg_ns={} validate_order_avg_ns={}",
        report.diagnostics.create_order.average_ns(),
        report.diagnostics.cancel_order.average_ns(),
        report.diagnostics.close_position.average_ns(),
        report.diagnostics.validate_order.average_ns(),
    );

    Ok(())
}

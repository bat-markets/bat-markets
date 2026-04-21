fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = bat_markets_testing::build_binance();
    let stream = client.stream();

    stream
        .public()
        .ingest_json(bat_markets_testing::binance::PUBLIC_TICKER)?;
    stream
        .public()
        .ingest_json(bat_markets_testing::binance::PUBLIC_TRADE)?;
    stream
        .public()
        .ingest_json(bat_markets_testing::binance::PUBLIC_MARK_PRICE)?;
    stream
        .public()
        .ingest_json(bat_markets_testing::binance::PUBLIC_LIQUIDATION)?;
    stream
        .private()
        .ingest_json(bat_markets_testing::binance::PRIVATE_ACCOUNT)?;
    stream
        .private()
        .ingest_json(bat_markets_testing::binance::PRIVATE_ORDER)?;

    let snapshot = client.diagnostics().snapshot();
    println!(
        "reads={} writes={} fetch_ticker_ops={} fetch_mark_price_ops={} fetch_liquidations_ops={} create_order_ops={} create_orders_ops={} amend_order_ops={} amend_orders_ops={} cancel_orders_ops={}",
        snapshot.state_reads.operations,
        snapshot.state_writes.operations,
        snapshot.fetch_ticker.operations,
        snapshot.fetch_mark_price.operations,
        snapshot.fetch_liquidations.operations,
        snapshot.create_order.operations,
        snapshot.create_orders.operations,
        snapshot.amend_order.operations,
        snapshot.amend_orders.operations,
        snapshot.cancel_orders.operations,
    );
    Ok(())
}

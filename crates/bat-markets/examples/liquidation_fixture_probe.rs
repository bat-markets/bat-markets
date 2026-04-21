use bat_markets::types::InstrumentId;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let instrument_id = InstrumentId::from("BTC/USDT:USDT");

    let binance = bat_markets_testing::build_binance();
    binance
        .stream()
        .public()
        .ingest_json(bat_markets_testing::binance::PUBLIC_LIQUIDATION)?;
    for liquidation in binance
        .market()
        .fetch_liquidations(&instrument_id, Some(10))
        .await?
    {
        println!(
            "binance liquidation {} {:?} px={} qty={} t={}",
            liquidation.instrument_id,
            liquidation.side,
            liquidation.price,
            liquidation.quantity,
            liquidation.event_time.value(),
        );
    }

    let bybit = bat_markets_testing::build_bybit();
    bybit
        .stream()
        .public()
        .ingest_json(bat_markets_testing::bybit::PUBLIC_LIQUIDATION)?;
    for liquidation in bybit
        .market()
        .fetch_liquidations(&instrument_id, Some(10))
        .await?
    {
        println!(
            "bybit liquidation {} {:?} px={} qty={} t={}",
            liquidation.instrument_id,
            liquidation.side,
            liquidation.price,
            liquidation.quantity,
            liquidation.event_time.value(),
        );
    }

    Ok(())
}

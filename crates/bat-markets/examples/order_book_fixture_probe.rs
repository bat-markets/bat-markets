use bat_markets::types::{InstrumentId, WatchOrderBookRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = bat_markets_testing::build_bybit();
    let instrument_id = InstrumentId::from("BTC/USDT:USDT");
    let mut updates = client
        .stream()
        .public()
        .subscribe_order_book(WatchOrderBookRequest::new(instrument_id.clone(), Some(50)));

    client
        .stream()
        .public()
        .ingest_json(bat_markets_testing::bybit::PUBLIC_ORDERBOOK)?;

    let delta = updates.recv().await?;
    println!(
        "delta {} bids={} asks={} first_bid={} first_ask={}",
        delta.instrument_id,
        delta.bids.len(),
        delta.asks.len(),
        delta
            .bids
            .first()
            .map(|level| level.price.to_string())
            .unwrap_or_else(|| "-".into()),
        delta
            .asks
            .first()
            .map(|level| level.price.to_string())
            .unwrap_or_else(|| "-".into()),
    );

    Ok(())
}

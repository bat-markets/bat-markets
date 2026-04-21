use bat_markets::{WatchInstrumentsRequest, types::WatchFastFeedRequest};
use bat_markets_core::{InstrumentId, PublicLaneEvent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = bat_markets_testing::build_binance();
    let instrument_id = InstrumentId::from("BTC/USDT:USDT");
    let mut fast = client
        .stream()
        .public()
        .subscribe_fast(WatchFastFeedRequest {
            instrument_ids: vec![instrument_id.clone()],
            ticker: true,
            trades: true,
            book_top: false,
            mark_price: true,
            funding_rate: true,
            open_interest: false,
            liquidations: true,
        });
    let mut tickers =
        client
            .stream()
            .public()
            .subscribe_ticker(WatchInstrumentsRequest::for_instrument(
                instrument_id.clone(),
            ));

    client
        .stream()
        .public()
        .ingest_json(bat_markets_testing::binance::PUBLIC_TICKER)?;
    client
        .stream()
        .public()
        .ingest_json(bat_markets_testing::binance::PUBLIC_TRADE)?;
    client
        .stream()
        .public()
        .ingest_json(bat_markets_testing::binance::PUBLIC_MARK_PRICE)?;
    client
        .stream()
        .public()
        .ingest_json(bat_markets_testing::binance::PUBLIC_LIQUIDATION)?;

    for _ in 0..5 {
        let event = fast.recv().await?;
        match event {
            PublicLaneEvent::Ticker(event) => {
                println!("fast:ticker {} {:?}", event.instrument_id, event.last_price)
            }
            PublicLaneEvent::Trade(event) => println!(
                "fast:trade {} {:?} {:?}",
                event.instrument_id, event.price, event.quantity
            ),
            PublicLaneEvent::MarkPrice(event) => {
                println!("fast:mark {} {:?}", event.instrument_id, event.price)
            }
            PublicLaneEvent::FundingRate(event) => {
                println!("fast:funding {} {}", event.instrument_id, event.value)
            }
            PublicLaneEvent::Liquidation(event) => println!(
                "fast:liquidation {} {:?} {:?}",
                event.instrument_id, event.side, event.quantity
            ),
            other => println!("fast:other {:?}", other),
        }
    }

    let ticker = tickers.recv().await?;
    println!(
        "typed ticker {} {}",
        ticker.instrument_id, ticker.last_price
    );
    Ok(())
}

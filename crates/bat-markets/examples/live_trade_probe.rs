use std::{env, time::Duration};

use bat_markets::{
    BatMarketsBuilder, WatchInstrumentsRequest,
    config::{AuthConfig, BatMarketsConfig, EndpointConfig},
    types::{InstrumentId, Product, Venue},
};
use tokio::time::timeout;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let venue = match env::var("BAT_MARKETS_VENUE")
        .unwrap_or_else(|_| "binance".to_owned())
        .to_ascii_lowercase()
        .as_str()
    {
        "bybit" => Venue::Bybit,
        _ => Venue::Binance,
    };
    let symbol = env::var("BAT_MARKETS_SYMBOL").unwrap_or_else(|_| "BTC/USDT:USDT".to_owned());
    let instrument_id = InstrumentId::from(symbol);

    let client = BatMarketsBuilder::default()
        .config(BatMarketsConfig {
            venue,
            product: Product::LinearUsdt,
            auth: AuthConfig::Env {
                api_key_var: "__BAT_MARKETS_UNUSED_PUBLIC_KEY__".into(),
                api_secret_var: "__BAT_MARKETS_UNUSED_PUBLIC_SECRET__".into(),
            },
            endpoints: EndpointConfig::mainnet_defaults(venue),
            ..BatMarketsConfig::new(venue, Product::LinearUsdt)
        })
        .build_live()
        .await?;

    let mut raw = client.stream().public().subscribe();
    let request = WatchInstrumentsRequest::for_instrument(instrument_id.clone());
    let mut trades = client
        .stream()
        .public()
        .watch_trades(request.clone())
        .await?;
    let mut book_top = client.stream().public().watch_book_top(request).await?;

    for index in 0..5 {
        let event = timeout(Duration::from_secs(10), raw.recv()).await??;
        println!("raw event {}: {:?}", index + 1, event);
        if matches!(event, bat_markets::types::PublicLaneEvent::Trade(_)) {
            break;
        }
    }

    let trade = timeout(Duration::from_secs(30), trades.recv()).await??;
    println!(
        "live trade {} {} {} {:?}",
        trade.instrument_id, trade.price, trade.quantity, trade.aggressor_side
    );

    let book = timeout(Duration::from_secs(30), book_top.recv()).await??;
    println!(
        "live book_top {} bid={} ask={}",
        book.instrument_id, book.bid.price, book.ask.price
    );

    Ok(())
}

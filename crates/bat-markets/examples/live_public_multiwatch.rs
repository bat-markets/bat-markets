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

    let request = WatchInstrumentsRequest::for_instrument(instrument_id.clone());
    let mut tickers = client
        .stream()
        .public()
        .watch_ticker(request.clone())
        .await?;
    let mut marks = client
        .stream()
        .public()
        .watch_mark_prices(request.clone())
        .await?;
    let mut liquidations = client.stream().public().watch_liquidations(request).await?;

    let ticker = timeout(Duration::from_secs(30), tickers.recv()).await??;
    println!("live ticker {} {}", ticker.instrument_id, ticker.last_price);

    let mark = timeout(Duration::from_secs(30), marks.recv()).await??;
    println!("live mark {} {}", mark.instrument_id, mark.price);

    if let Ok(Ok(liquidation)) = timeout(Duration::from_secs(30), liquidations.recv()).await {
        println!(
            "live liquidation {} {:?} {}",
            liquidation.instrument_id, liquidation.side, liquidation.quantity
        );
    } else {
        println!("live liquidation none within 30s");
    }

    Ok(())
}

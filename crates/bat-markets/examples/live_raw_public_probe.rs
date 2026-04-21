use std::{env, time::Duration};

use bat_markets::{
    BatMarketsBuilder, PublicSubscription,
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

    let mut receiver = client.stream().public().subscribe();
    let _stream = client
        .stream()
        .public()
        .spawn_live(PublicSubscription {
            instrument_ids: vec![instrument_id],
            ticker: false,
            trades: true,
            book_top: true,
            order_book: false,
            mark_price: false,
            funding_rate: false,
            open_interest: false,
            liquidations: false,
            kline_intervals: Vec::new(),
        })
        .await?;

    for index in 0..10 {
        let event = timeout(Duration::from_secs(10), receiver.recv()).await??;
        println!("raw event {}: {:?}", index + 1, event);
    }

    Ok(())
}

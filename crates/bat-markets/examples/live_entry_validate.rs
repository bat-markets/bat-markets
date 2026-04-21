use std::{env, time::Duration};

use bat_markets::{
    BatMarkets,
    types::{
        CreateOrderRequest, InstrumentId, OrderType, Product, Quantity, Side, TimeInForce, Venue,
    },
};
use rust_decimal::Decimal;
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

    let client = BatMarkets::builder()
        .venue(venue)
        .product(Product::LinearUsdt)
        .build_live()
        .await?;

    let request = CreateOrderRequest {
        request_id: None,
        instrument_id: instrument_id.clone(),
        client_order_id: None,
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: Some(TimeInForce::Gtc),
        quantity: Quantity::new(Decimal::new(1, 3)),
        price: Some(
            client
                .market()
                .fetch_book_top(&instrument_id)
                .await?
                .bid
                .price,
        ),
        trigger_price: None,
        trigger_type: None,
        reduce_only: false,
        post_only: true,
    };

    let mut handle = client
        .entry()
        .validate_order(&bat_markets::types::ValidateOrderRequest {
            request_id: None,
            order: request,
        })
        .await?;
    println!("validate ack status={:?}", handle.ack().receipt.status);

    let lifecycle = timeout(Duration::from_secs(5), handle.next_lifecycle()).await??;
    println!("validate lifecycle={:?}", lifecycle);
    Ok(())
}

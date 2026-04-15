use bat_markets::{
    BatMarkets,
    errors::Result,
    types::{InstrumentId, Product, Venue},
};

fn main() -> Result<()> {
    let client = BatMarkets::builder()
        .venue(Venue::Bybit)
        .product(Product::LinearUsdt)
        .build()?;

    let stream = client.stream();
    stream.public().ingest_json(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/public_ticker.json"
    )))?;
    stream.private().ingest_json(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bybit/private_position.json"
    )))?;

    let instrument = InstrumentId::from("BTC/USDT:USDT");
    if let Some(open_interest) = client.market().open_interest(&instrument) {
        println!("open interest: {}", open_interest.value);
    }
    println!("positions: {}", client.position().list().len());

    Ok(())
}

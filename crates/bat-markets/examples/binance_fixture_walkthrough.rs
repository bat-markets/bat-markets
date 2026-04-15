use bat_markets::{
    BatMarkets,
    errors::Result,
    types::{InstrumentId, Product, Venue},
};

fn main() -> Result<()> {
    let client = BatMarkets::builder()
        .venue(Venue::Binance)
        .product(Product::LinearUsdt)
        .build()?;

    let stream = client.stream();
    stream.public().ingest_json(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/public_ticker.json"
    )))?;
    stream.private().ingest_json(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/binance/private_account_update.json"
    )))?;

    let instrument = InstrumentId::from("BTC/USDT:USDT");
    if let Some(ticker) = client.market().ticker(&instrument) {
        println!("last price: {}", ticker.last_price);
    }
    println!("balances: {}", client.account().balances().len());

    Ok(())
}

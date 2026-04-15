use bat_markets::errors::Result;
use bat_markets::types::InstrumentId;
use bat_markets_testing::{binance, build_binance, build_bybit, bybit};

#[test]
fn stress_binance_public_ingest_stays_bounded() -> Result<()> {
    let client = build_binance();
    let stream = client.stream();
    let instrument = InstrumentId::from("BTC/USDT:USDT");

    for _ in 0..20_000 {
        stream.public().ingest_json(binance::PUBLIC_TICKER)?;
        stream.public().ingest_json(binance::PUBLIC_TRADE)?;
        stream.public().ingest_json(binance::PUBLIC_BOOK_TICKER)?;
    }

    let recent = client
        .market()
        .recent_trades(&instrument)
        .expect("recent trades should exist after stress ingest");
    assert!(recent.len() <= 128);
    assert!(client.market().ticker(&instrument).is_some());
    Ok(())
}

#[test]
fn stress_bybit_private_ingest_stays_bounded() -> Result<()> {
    let client = build_bybit();
    let stream = client.stream();

    for _ in 0..10_000 {
        stream.private().ingest_json(bybit::PRIVATE_WALLET)?;
        stream.private().ingest_json(bybit::PRIVATE_POSITION)?;
        stream.private().ingest_json(bybit::PRIVATE_ORDER)?;
        stream.private().ingest_json(bybit::PRIVATE_EXECUTION)?;
    }

    assert!(client.trade().executions().len() <= 1_024);
    assert_eq!(client.trade().orders().len(), 1);
    assert_eq!(client.position().list().len(), 1);
    Ok(())
}

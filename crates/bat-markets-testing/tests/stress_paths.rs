use bat_markets::errors::Result;
use bat_markets::types::InstrumentId;
use bat_markets_testing::{binance, build_binance, build_bybit, bybit};

#[test]
fn stress_binance_public_ingest_stays_bounded() -> Result<()> {
    let client = build_binance();
    let instrument = InstrumentId::from("BTC/USDT:USDT");

    for _ in 0..20_000 {
        client
            .advanced()
            .ingest_public_json(binance::PUBLIC_TICKER)?;
        client
            .advanced()
            .ingest_public_json(binance::PUBLIC_TRADE)?;
        client
            .advanced()
            .ingest_public_json(binance::PUBLIC_BOOK_TICKER)?;
    }

    let recent = client
        .advanced()
        .cached_recent_trades(&instrument)
        .expect("recent trades should exist after stress ingest");
    assert!(recent.len() <= 128);
    assert!(client.advanced().cached_ticker(&instrument).is_some());
    Ok(())
}

#[test]
fn stress_bybit_private_ingest_stays_bounded() -> Result<()> {
    let client = build_bybit();

    for _ in 0..10_000 {
        client
            .advanced()
            .ingest_private_json(bybit::PRIVATE_WALLET)?;
        client
            .advanced()
            .ingest_private_json(bybit::PRIVATE_POSITION)?;
        client
            .advanced()
            .ingest_private_json(bybit::PRIVATE_ORDER)?;
        client
            .advanced()
            .ingest_private_json(bybit::PRIVATE_EXECUTION)?;
    }

    assert!(client.advanced().cached_executions().len() <= 1_024);
    assert_eq!(client.advanced().cached_orders().len(), 1);
    assert_eq!(client.advanced().cached_positions().len(), 1);
    Ok(())
}

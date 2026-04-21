fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = bat_markets_testing::build_bybit();
    let stream = client.stream();

    stream
        .private()
        .ingest_json(bat_markets_testing::bybit::PRIVATE_WALLET)?;
    stream
        .private()
        .ingest_json(bat_markets_testing::bybit::PRIVATE_POSITION)?;
    stream
        .private()
        .ingest_json(bat_markets_testing::bybit::PRIVATE_ORDER)?;
    stream
        .private()
        .ingest_json(bat_markets_testing::bybit::PRIVATE_EXECUTION)?;

    println!("balances={}", client.account().balances().len());
    println!("positions={}", client.position().list().len());
    println!("orders={}", client.trade().orders().len());
    println!("executions={}", client.trade().executions().len());
    println!("health={:?}", client.health().snapshot().status);
    Ok(())
}

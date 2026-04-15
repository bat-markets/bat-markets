use bat_markets::BatMarkets;
use criterion::{Criterion, criterion_group, criterion_main};

fn ingest_binance_public(c: &mut Criterion) {
    c.bench_function("binance_public_ingest", |b| {
        b.iter(|| {
            let client = bat_markets_testing::build_binance();
            let stream = client.stream();
            let _ = stream
                .public()
                .ingest_json(bat_markets_testing::binance::PUBLIC_TICKER);
            let _ = stream
                .public()
                .ingest_json(bat_markets_testing::binance::PUBLIC_TRADE);
            let _ = stream
                .public()
                .ingest_json(bat_markets_testing::binance::PUBLIC_BOOK_TICKER);
        })
    });
}

fn ingest_bybit_private(c: &mut Criterion) {
    c.bench_function("bybit_private_ingest", |b| {
        b.iter(|| {
            let client = bat_markets_testing::build_bybit();
            ingest_bybit_private_fixtures(&client);
        })
    });
}

fn ingest_bybit_private_fixtures(client: &BatMarkets) {
    let stream = client.stream();
    let _ = stream
        .private()
        .ingest_json(bat_markets_testing::bybit::PRIVATE_WALLET);
    let _ = stream
        .private()
        .ingest_json(bat_markets_testing::bybit::PRIVATE_POSITION);
    let _ = stream
        .private()
        .ingest_json(bat_markets_testing::bybit::PRIVATE_ORDER);
    let _ = stream
        .private()
        .ingest_json(bat_markets_testing::bybit::PRIVATE_EXECUTION);
}

criterion_group!(benches, ingest_binance_public, ingest_bybit_private);
criterion_main!(benches);

use std::hint::black_box;

use bat_markets::{BatMarkets, types::InstrumentId};
use bat_markets_core::CommandOperation;
use criterion::{Criterion, criterion_group, criterion_main};

struct RuntimeBatchBenchHarness {
    _rest_stub: bat_markets_testing::RuntimeRestStub,
    _binance_command_ws_stub: bat_markets_testing::RuntimeCommandWsStub,
    _bybit_command_ws_stub: bat_markets_testing::RuntimeCommandWsStub,
    binance: BatMarkets,
    bybit: BatMarkets,
    binance_validate: Vec<bat_markets::types::ValidateOrderRequest>,
    bybit_validate: Vec<bat_markets::types::ValidateOrderRequest>,
    binance_create: bat_markets::types::CreateOrdersRequest,
    bybit_create: bat_markets::types::CreateOrdersRequest,
    binance_cancel: bat_markets::types::CancelOrdersRequest,
    bybit_cancel: bat_markets::types::CancelOrdersRequest,
}

impl RuntimeBatchBenchHarness {
    fn new() -> Self {
        let rest_stub =
            bat_markets_testing::RuntimeRestStub::spawn().expect("runtime stub should bind");
        let rest_base = rest_stub.rest_base();
        let binance_command_ws_stub =
            bat_markets_testing::RuntimeCommandWsStub::spawn(bat_markets::types::Venue::Binance)
                .expect("binance command ws stub should bind");
        let bybit_command_ws_stub =
            bat_markets_testing::RuntimeCommandWsStub::spawn(bat_markets::types::Venue::Bybit)
                .expect("bybit command ws stub should bind");
        let binance = bat_markets_testing::build_runtime_dual_stub_client(
            bat_markets::types::Venue::Binance,
            &rest_base,
            &binance_command_ws_stub.command_ws_base(),
        );
        let bybit = bat_markets_testing::build_runtime_dual_stub_client(
            bat_markets::types::Venue::Bybit,
            &rest_base,
            &bybit_command_ws_stub.command_ws_base(),
        );

        Self {
            _rest_stub: rest_stub,
            _binance_command_ws_stub: binance_command_ws_stub,
            _bybit_command_ws_stub: bybit_command_ws_stub,
            binance_validate: bat_markets_testing::runtime_stub_validate_requests(
                bat_markets::types::Venue::Binance,
            ),
            bybit_validate: bat_markets_testing::runtime_stub_validate_requests(
                bat_markets::types::Venue::Bybit,
            ),
            binance_create: bat_markets_testing::runtime_stub_batch_create_request(
                bat_markets::types::Venue::Binance,
            ),
            bybit_create: bat_markets_testing::runtime_stub_batch_create_request(
                bat_markets::types::Venue::Bybit,
            ),
            binance_cancel: bat_markets_testing::runtime_stub_batch_cancel_request(
                bat_markets::types::Venue::Binance,
            ),
            bybit_cancel: bat_markets_testing::runtime_stub_batch_cancel_request(
                bat_markets::types::Venue::Bybit,
            ),
            binance,
            bybit,
        }
    }
}

fn ingest_binance_public(c: &mut Criterion) {
    c.bench_function("binance_public_ingest", |b| {
        b.iter(|| {
            let client = bat_markets_testing::build_binance();
            let _ = client
                .advanced()
                .ingest_public_json(bat_markets_testing::binance::PUBLIC_TICKER);
            let _ = client
                .advanced()
                .ingest_public_json(bat_markets_testing::binance::PUBLIC_TRADE);
            let _ = client
                .advanced()
                .ingest_public_json(bat_markets_testing::binance::PUBLIC_BOOK_TICKER);
            let _ = client
                .advanced()
                .ingest_public_json(bat_markets_testing::binance::PUBLIC_MARK_PRICE);
            let _ = client
                .advanced()
                .ingest_public_json(bat_markets_testing::binance::PUBLIC_LIQUIDATION);
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

fn ingest_bybit_public(c: &mut Criterion) {
    c.bench_function("bybit_public_ingest", |b| {
        b.iter(|| {
            let client = bat_markets_testing::build_bybit();
            let _ = client
                .advanced()
                .ingest_public_json(bat_markets_testing::bybit::PUBLIC_TICKER);
            let _ = client
                .advanced()
                .ingest_public_json(bat_markets_testing::bybit::PUBLIC_TRADE);
            let _ = client
                .advanced()
                .ingest_public_json(bat_markets_testing::bybit::PUBLIC_ORDERBOOK);
            let _ = client
                .advanced()
                .ingest_public_json(bat_markets_testing::bybit::PUBLIC_LIQUIDATION);
        })
    });
}

fn classify_commands(c: &mut Criterion) {
    c.bench_function("command_classification", |b| {
        b.iter(|| {
            let client = bat_markets_testing::build_binance();
            let _ = client.advanced().classify_command_json(
                CommandOperation::CreateOrder,
                Some(bat_markets_testing::binance::COMMAND_CREATE_OK),
                None,
            );
            let _ = client.advanced().classify_command_json(
                CommandOperation::AmendOrder,
                Some(bat_markets_testing::binance::COMMAND_AMEND_OK),
                None,
            );
            let _ =
                client
                    .advanced()
                    .classify_command_json(CommandOperation::CreateOrder, None, None);
        })
    });
}

fn classify_batch_commands(c: &mut Criterion) {
    c.bench_function("batch_command_surface", |b| {
        b.iter(|| {
            let binance = bat_markets_testing::build_binance();
            let bybit = bat_markets_testing::build_bybit();

            let _ = binance.advanced().classify_command_json(
                CommandOperation::CreateOrders,
                Some(bat_markets_testing::binance::COMMAND_BATCH_CREATE_OK),
                None,
            );
            let _ = binance.advanced().classify_command_json(
                CommandOperation::AmendOrders,
                Some(bat_markets_testing::binance::COMMAND_BATCH_AMEND_OK),
                None,
            );
            let _ = binance.advanced().classify_command_json(
                CommandOperation::CancelOrders,
                Some(bat_markets_testing::binance::COMMAND_BATCH_CANCEL_OK),
                None,
            );

            let _ = bybit.advanced().classify_command_json(
                CommandOperation::CreateOrders,
                Some(bat_markets_testing::bybit::COMMAND_BATCH_CREATE_OK),
                None,
            );
            let _ = bybit.advanced().classify_command_json(
                CommandOperation::AmendOrders,
                Some(bat_markets_testing::bybit::COMMAND_BATCH_AMEND_OK),
                None,
            );
            let _ = bybit.advanced().classify_command_json(
                CommandOperation::CancelOrders,
                Some(bat_markets_testing::bybit::COMMAND_BATCH_CANCEL_OK),
                None,
            );
        })
    });
}

fn runtime_batch_entry_paths(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().expect("bench runtime should construct");
    let harness = RuntimeBatchBenchHarness::new();

    c.bench_function("binance_runtime_batch_entry_path", |b| {
        b.iter(|| {
            runtime.block_on(async {
                for request in &harness.binance_validate {
                    let handle = harness
                        .binance
                        .validate_order(request)
                        .await
                        .expect("binance runtime validate should succeed");
                    black_box(handle.ack().receipt.status);
                }

                let create = harness
                    .binance
                    .create_orders(&harness.binance_create)
                    .await
                    .expect("binance runtime batch create should succeed");
                black_box(create.len());

                let cancel = harness
                    .binance
                    .cancel_orders(&harness.binance_cancel)
                    .await
                    .expect("binance runtime batch cancel should succeed");
                black_box(cancel.len());
            });
        })
    });

    c.bench_function("bybit_runtime_batch_entry_path", |b| {
        b.iter(|| {
            runtime.block_on(async {
                for request in &harness.bybit_validate {
                    let handle = harness
                        .bybit
                        .validate_order(request)
                        .await
                        .expect("bybit runtime validate should succeed");
                    black_box(handle.ack().receipt.status);
                }

                let create = harness
                    .bybit
                    .create_orders(&harness.bybit_create)
                    .await
                    .expect("bybit runtime batch create should succeed");
                black_box(create.len());

                let cancel = harness
                    .bybit
                    .cancel_orders(&harness.bybit_cancel)
                    .await
                    .expect("bybit runtime batch cancel should succeed");
                black_box(cancel.len());
            });
        })
    });
}

fn liquidation_cache_reads(c: &mut Criterion) {
    c.bench_function("liquidation_cache_reads", |b| {
        let client = bat_markets_testing::build_binance();
        let instrument_id = InstrumentId::from("BTC/USDT:USDT");
        client
            .advanced()
            .ingest_public_json(bat_markets_testing::binance::PUBLIC_LIQUIDATION)
            .expect("binance liquidation fixture should parse");

        b.iter(|| {
            let _ = client.advanced().cached_liquidations(&instrument_id);
        })
    });
}

fn ingest_bybit_private_fixtures(client: &BatMarkets) {
    let _ = client
        .advanced()
        .ingest_private_json(bat_markets_testing::bybit::PRIVATE_WALLET);
    let _ = client
        .advanced()
        .ingest_private_json(bat_markets_testing::bybit::PRIVATE_POSITION);
    let _ = client
        .advanced()
        .ingest_private_json(bat_markets_testing::bybit::PRIVATE_ORDER);
    let _ = client
        .advanced()
        .ingest_private_json(bat_markets_testing::bybit::PRIVATE_EXECUTION);
}

criterion_group!(benches, ingest_binance_public, ingest_bybit_private);
criterion_group!(
    extended_benches,
    ingest_bybit_public,
    classify_commands,
    classify_batch_commands,
    runtime_batch_entry_paths,
    liquidation_cache_reads
);
criterion_main!(benches, extended_benches);

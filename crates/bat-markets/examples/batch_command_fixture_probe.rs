use bat_markets_core::CommandOperation;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let binance = bat_markets_testing::build_binance();
    let bybit = bat_markets_testing::build_bybit();

    let binance_create = binance.stream().command().classify_json(
        CommandOperation::CreateOrders,
        Some(bat_markets_testing::binance::COMMAND_BATCH_CREATE_OK),
        None,
    )?;
    let binance_amend = binance.stream().command().classify_json(
        CommandOperation::AmendOrders,
        Some(bat_markets_testing::binance::COMMAND_BATCH_AMEND_OK),
        None,
    )?;
    let binance_cancel = binance.stream().command().classify_json(
        CommandOperation::CancelOrders,
        Some(bat_markets_testing::binance::COMMAND_BATCH_CANCEL_OK),
        None,
    )?;

    let bybit_create = bybit.stream().command().classify_json(
        CommandOperation::CreateOrders,
        Some(bat_markets_testing::bybit::COMMAND_BATCH_CREATE_OK),
        None,
    )?;
    let bybit_amend = bybit.stream().command().classify_json(
        CommandOperation::AmendOrders,
        Some(bat_markets_testing::bybit::COMMAND_BATCH_AMEND_OK),
        None,
    )?;
    let bybit_cancel = bybit.stream().command().classify_json(
        CommandOperation::CancelOrders,
        Some(bat_markets_testing::bybit::COMMAND_BATCH_CANCEL_OK),
        None,
    )?;

    let binance_items: serde_json::Value =
        serde_json::from_str(bat_markets_testing::binance::COMMAND_BATCH_CREATE_OK)?;
    let bybit_items: serde_json::Value =
        serde_json::from_str(bat_markets_testing::bybit::COMMAND_BATCH_CREATE_OK)?;
    let binance_count = binance_items.as_array().map_or(0, Vec::len);
    let bybit_count = bybit_items
        .get("result")
        .and_then(|value| value.get("list"))
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);

    println!(
        "binance batch create={:?} amend={:?} cancel={:?} items={}",
        binance_create.status, binance_amend.status, binance_cancel.status, binance_count
    );
    println!(
        "bybit batch create={:?} amend={:?} cancel={:?} items={}",
        bybit_create.status, bybit_amend.status, bybit_cancel.status, bybit_count
    );

    Ok(())
}

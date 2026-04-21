use bat_markets::types::Venue;
use bat_markets_core::CommandOperation;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for venue in [Venue::Binance, Venue::Bybit] {
        let client = match venue {
            Venue::Binance => bat_markets_testing::build_binance(),
            Venue::Bybit => bat_markets_testing::build_bybit(),
        };
        let fixtures = match venue {
            Venue::Binance => (
                Some(bat_markets_testing::binance::COMMAND_CREATE_OK),
                Some(bat_markets_testing::binance::COMMAND_AMEND_OK),
                Some(bat_markets_testing::binance::COMMAND_REJECT),
            ),
            Venue::Bybit => (
                Some(bat_markets_testing::bybit::COMMAND_CREATE_OK),
                Some(bat_markets_testing::bybit::COMMAND_AMEND_OK),
                Some(bat_markets_testing::bybit::COMMAND_REJECT),
            ),
        };

        let create = client.stream().command().classify_json(
            CommandOperation::CreateOrder,
            fixtures.0,
            None,
        )?;
        let amend = client.stream().command().classify_json(
            CommandOperation::AmendOrder,
            fixtures.1,
            None,
        )?;
        let reject = client.stream().command().classify_json(
            CommandOperation::CreateOrder,
            fixtures.2,
            None,
        )?;
        let unknown =
            client
                .stream()
                .command()
                .classify_json(CommandOperation::CreateOrder, None, None)?;

        println!(
            "{venue:?} create={:?} amend={:?} reject={:?} unknown={:?}",
            create.status, amend.status, reject.status, unknown.status
        );
    }

    Ok(())
}

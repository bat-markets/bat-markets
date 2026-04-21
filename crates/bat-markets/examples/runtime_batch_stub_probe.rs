use bat_markets::types::{CommandStatus, Venue};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stub = bat_markets_testing::RuntimeRestStub::spawn()?;
    let rest_base = stub.rest_base();

    for venue in [Venue::Binance, Venue::Bybit] {
        let client = bat_markets_testing::build_runtime_stub_client(venue, &rest_base);

        for request in bat_markets_testing::runtime_stub_validate_requests(venue) {
            let handle = client.entry().validate_order(&request).await?;
            assert_eq!(handle.ack().receipt.status, CommandStatus::Accepted);
        }

        let create = client
            .entry()
            .create_orders(&bat_markets_testing::runtime_stub_batch_create_request(
                venue,
            ))
            .await?;
        let cancel = client
            .entry()
            .cancel_orders(&bat_markets_testing::runtime_stub_batch_cancel_request(
                venue,
            ))
            .await?;

        println!(
            "{venue:?} runtime stub validate=2 create={} cancel={}",
            create.len(),
            cancel.len()
        );
    }

    Ok(())
}

use std::{env, time::Duration};

use tokio::time::sleep;

use bat_markets::{
    BatMarketsBuilder, PublicSubscription,
    config::{AuthConfig, BatMarketsConfig, EndpointConfig},
    errors::Result,
    types::{InstrumentId, Product, Venue},
};
use bat_markets_core::VenueAdapter;
use bat_markets_testing::{has_binance_live_env, has_bybit_live_env};

#[tokio::test]
async fn binance_mainnet_read_flows_are_manual_and_read_only() -> Result<()> {
    if !has_binance_live_env() || env::var_os("BAT_MARKETS_ENABLE_MAINNET_READS").is_none() {
        return Ok(());
    }

    let config = BatMarketsConfig {
        venue: Venue::Binance,
        product: Product::LinearUsdt,
        auth: AuthConfig::Env {
            api_key_var: "BINANCE_API_KEY".into(),
            api_secret_var: "BINANCE_API_SECRET".into(),
        },
        endpoints: EndpointConfig::mainnet_defaults(Venue::Binance),
        ..BatMarketsConfig::new(Venue::Binance, Product::LinearUsdt)
    };
    let client = BatMarketsBuilder::default()
        .config(config)
        .build_live()
        .await?;

    assert!(!client.native().binance()?.config().endpoints.sandbox);
    let instrument = preferred_mainnet_instrument(&client);
    let public = client
        .stream()
        .public()
        .spawn_live(PublicSubscription {
            instrument_ids: vec![instrument.clone()],
            ticker: false,
            trades: false,
            book_top: true,
            funding_rate: false,
            open_interest: false,
            kline_interval: None,
        })
        .await?;
    let private = client.stream().private().spawn_live().await?;

    sleep(Duration::from_secs(3)).await;

    public.shutdown().await?;
    private.shutdown().await?;

    let _ = client.market().refresh_open_interest(&instrument).await?;
    let _ = client.account().refresh().await?;
    let _ = client.position().refresh().await?;
    let _ = client.trade().refresh_open_orders(None).await?;
    let _ = client.trade().refresh_executions(None).await?;
    let _ = client.stream().private().reconcile().await?;

    assert!(client.market().book_top(&instrument).is_some());
    assert!(client.market().open_interest(&instrument).is_some());
    Ok(())
}

#[tokio::test]
async fn bybit_mainnet_read_flows_are_manual_and_read_only() -> Result<()> {
    if !has_bybit_live_env() || env::var_os("BAT_MARKETS_ENABLE_MAINNET_READS").is_none() {
        return Ok(());
    }

    let config = BatMarketsConfig {
        venue: Venue::Bybit,
        product: Product::LinearUsdt,
        auth: AuthConfig::Env {
            api_key_var: "BYBIT_API_KEY".into(),
            api_secret_var: "BYBIT_API_SECRET".into(),
        },
        endpoints: EndpointConfig::mainnet_defaults(Venue::Bybit),
        ..BatMarketsConfig::new(Venue::Bybit, Product::LinearUsdt)
    };
    let client = BatMarketsBuilder::default()
        .config(config)
        .build_live()
        .await?;

    assert!(!client.native().bybit()?.config().endpoints.sandbox);
    let instrument = preferred_mainnet_instrument(&client);
    let public = client
        .stream()
        .public()
        .spawn_live(PublicSubscription {
            instrument_ids: vec![instrument.clone()],
            ticker: false,
            trades: false,
            book_top: true,
            funding_rate: false,
            open_interest: false,
            kline_interval: None,
        })
        .await?;
    let private = client.stream().private().spawn_live().await?;

    sleep(Duration::from_secs(3)).await;

    public.shutdown().await?;
    private.shutdown().await?;

    let _ = client.market().refresh_open_interest(&instrument).await?;
    let _ = client.account().refresh().await?;
    let _ = client.position().refresh().await?;
    let _ = client.trade().refresh_open_orders(None).await?;
    let _ = client.trade().refresh_executions(None).await?;
    let _ = client.stream().private().reconcile().await?;

    assert!(client.market().book_top(&instrument).is_some());
    assert!(client.market().open_interest(&instrument).is_some());
    Ok(())
}

fn preferred_mainnet_instrument(client: &bat_markets::BatMarkets) -> InstrumentId {
    const PREFERRED: &[&str] = &["BTC/USDT:USDT", "ETH/USDT:USDT", "SOL/USDT:USDT"];

    let specs = client.market().instrument_specs();
    for symbol in PREFERRED {
        if let Some(spec) = specs
            .iter()
            .find(|spec| spec.instrument_id.as_ref() == *symbol)
        {
            return spec.instrument_id.clone();
        }
    }

    specs
        .first()
        .expect("mainnet metadata should populate at least one instrument")
        .instrument_id
        .clone()
}

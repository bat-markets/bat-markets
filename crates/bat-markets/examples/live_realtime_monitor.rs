use std::{env, time::Duration};

use bat_markets::{
    BatMarketsBuilder, WatchInstrumentsRequest,
    config::{AuthConfig, BatMarketsConfig, EndpointConfig},
    types::{InstrumentId, Product, Venue},
};
use tokio::time::{Instant, sleep};

#[derive(Clone, Copy, Debug)]
struct MonitorPlan {
    ticker: bool,
    trades: bool,
    book_top: bool,
    mark_price: bool,
    orders: bool,
    executions: bool,
    positions: bool,
    balances: bool,
    account: bool,
}

impl MonitorPlan {
    fn requires_private(self) -> bool {
        self.orders || self.executions || self.positions || self.balances || self.account
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let venue = match env::var("BAT_MARKETS_VENUE")
        .unwrap_or_else(|_| "binance".to_owned())
        .to_ascii_lowercase()
        .as_str()
    {
        "bybit" => Venue::Bybit,
        _ => Venue::Binance,
    };
    let symbol = env::var("BAT_MARKETS_SYMBOL").unwrap_or_else(|_| "BTC/USDT:USDT".to_owned());
    let instrument_id = InstrumentId::from(symbol.clone());
    let max_events = env_usize("BAT_MARKETS_MAX_EVENTS").unwrap_or(40);
    let max_seconds = env_u64("BAT_MARKETS_MAX_SECONDS").unwrap_or(60);
    let plan = MonitorPlan {
        ticker: env_flag("BAT_MARKETS_WATCH_TICKER", true),
        trades: env_flag("BAT_MARKETS_WATCH_TRADES", true),
        book_top: env_flag("BAT_MARKETS_WATCH_BOOK_TOP", true),
        mark_price: env_flag("BAT_MARKETS_WATCH_MARK_PRICE", true),
        orders: env_flag("BAT_MARKETS_WATCH_ORDERS", false),
        executions: env_flag("BAT_MARKETS_WATCH_EXECUTIONS", false),
        positions: env_flag("BAT_MARKETS_WATCH_POSITIONS", false),
        balances: env_flag("BAT_MARKETS_WATCH_BALANCES", false),
        account: env_flag("BAT_MARKETS_WATCH_ACCOUNT", false),
    };

    let client = if plan.requires_private() {
        BatMarketsBuilder::default()
            .venue(venue)
            .product(Product::LinearUsdt)
            .build_live()
            .await?
    } else {
        BatMarketsBuilder::default()
            .config(BatMarketsConfig {
                venue,
                product: Product::LinearUsdt,
                auth: AuthConfig::Env {
                    api_key_var: "__BAT_MARKETS_UNUSED_PUBLIC_KEY__".into(),
                    api_secret_var: "__BAT_MARKETS_UNUSED_PUBLIC_SECRET__".into(),
                },
                endpoints: EndpointConfig::mainnet_defaults(venue),
                ..BatMarketsConfig::new(venue, Product::LinearUsdt)
            })
            .build_live()
            .await?
    };

    let request = WatchInstrumentsRequest::for_instrument(instrument_id.clone());
    let mut ticker_watch = if plan.ticker {
        Some(
            client
                .stream()
                .public()
                .watch_ticker(request.clone())
                .await?,
        )
    } else {
        None
    };
    let mut trades_watch = if plan.trades {
        Some(
            client
                .stream()
                .public()
                .watch_trades(request.clone())
                .await?,
        )
    } else {
        None
    };
    let mut book_top_watch = if plan.book_top {
        Some(
            client
                .stream()
                .public()
                .watch_book_top(request.clone())
                .await?,
        )
    } else {
        None
    };
    let mut mark_price_watch = if plan.mark_price {
        Some(
            client
                .stream()
                .public()
                .watch_mark_prices(request.clone())
                .await?,
        )
    } else {
        None
    };
    let mut orders_watch = if plan.orders {
        Some(client.stream().private().watch_orders().await?)
    } else {
        None
    };
    let mut executions_watch = if plan.executions {
        Some(client.stream().private().watch_executions().await?)
    } else {
        None
    };
    let mut positions_watch = if plan.positions {
        Some(client.stream().private().watch_positions().await?)
    } else {
        None
    };
    let mut balances_watch = if plan.balances {
        Some(client.stream().private().watch_balances().await?)
    } else {
        None
    };
    let mut account_watch = if plan.account {
        Some(client.stream().private().watch_account().await?)
    } else {
        None
    };

    println!(
        "monitor venue={:?} symbol={} private={} max_events={} max_seconds={}",
        venue,
        instrument_id,
        plan.requires_private(),
        max_events,
        max_seconds,
    );

    let deadline = Instant::now() + Duration::from_secs(max_seconds);
    let mut events = 0usize;
    while events < max_events {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        tokio::select! {
            result = async { ticker_watch.as_mut().unwrap().recv().await }, if ticker_watch.is_some() => {
                let ticker = result?;
                println!(
                    "ticker instrument={} last={} mark={:?} index={:?} volume_24h={:?} event_time={:?}",
                    ticker.instrument_id,
                    ticker.last_price,
                    ticker.mark_price,
                    ticker.index_price,
                    ticker.volume_24h,
                    ticker.event_time,
                );
                events += 1;
            }
            result = async { trades_watch.as_mut().unwrap().recv().await }, if trades_watch.is_some() => {
                let trade = result?;
                println!(
                    "trade instrument={} price={} qty={} aggressor={:?} event_time={:?}",
                    trade.instrument_id,
                    trade.price,
                    trade.quantity,
                    trade.aggressor_side,
                    trade.event_time,
                );
                events += 1;
            }
            result = async { book_top_watch.as_mut().unwrap().recv().await }, if book_top_watch.is_some() => {
                let book = result?;
                println!(
                    "book_top instrument={} bid={} bid_qty={} ask={} ask_qty={} event_time={:?}",
                    book.instrument_id,
                    book.bid.price,
                    book.bid.quantity,
                    book.ask.price,
                    book.ask.quantity,
                    book.event_time,
                );
                events += 1;
            }
            result = async { mark_price_watch.as_mut().unwrap().recv().await }, if mark_price_watch.is_some() => {
                let mark = result?;
                println!(
                    "mark_price instrument={} price={} funding_rate={:?} event_time={:?}",
                    mark.instrument_id,
                    mark.price,
                    mark.funding_rate,
                    mark.event_time,
                );
                events += 1;
            }
            result = async { orders_watch.as_mut().unwrap().recv().await }, if orders_watch.is_some() => {
                let order = result?;
                println!(
                    "order instrument={} order_id={} client_id={:?} status={:?} type={:?} side={:?} qty={} filled={} updated_at={:?}",
                    order.instrument_id,
                    order.order_id,
                    order.client_order_id,
                    order.status,
                    order.order_type,
                    order.side,
                    order.quantity,
                    order.filled_quantity,
                    order.updated_at,
                );
                events += 1;
            }
            result = async { executions_watch.as_mut().unwrap().recv().await }, if executions_watch.is_some() => {
                let execution = result?;
                println!(
                    "execution instrument={} order_id={} client_id={:?} price={} qty={} side={:?} liquidity={:?} executed_at={:?}",
                    execution.instrument_id,
                    execution.order_id,
                    execution.client_order_id,
                    execution.price,
                    execution.quantity,
                    execution.side,
                    execution.liquidity,
                    execution.executed_at,
                );
                events += 1;
            }
            result = async { positions_watch.as_mut().unwrap().recv().await }, if positions_watch.is_some() => {
                let position = result?;
                println!(
                    "position instrument={} direction={:?} size={} entry_price={:?} upnl={:?} updated_at={:?}",
                    position.instrument_id,
                    position.direction,
                    position.size,
                    position.entry_price,
                    position.unrealized_pnl,
                    position.updated_at,
                );
                events += 1;
            }
            result = async { balances_watch.as_mut().unwrap().recv().await }, if balances_watch.is_some() => {
                let balance = result?;
                println!(
                    "balance asset={} wallet={} available={} updated_at={:?}",
                    balance.asset,
                    balance.wallet_balance,
                    balance.available_balance,
                    balance.updated_at,
                );
                events += 1;
            }
            result = async { account_watch.as_mut().unwrap().recv().await }, if account_watch.is_some() => {
                let account = result?;
                println!(
                    "account wallet={} available={} upnl={} updated_at={:?}",
                    account.total_wallet_balance,
                    account.total_available_balance,
                    account.total_unrealized_pnl,
                    account.updated_at,
                );
                events += 1;
            }
            _ = sleep(remaining) => {
                break;
            }
        }
    }

    println!("monitor completed events={events}");
    Ok(())
}

fn env_flag(key: &str, default: bool) -> bool {
    match env::var(key) {
        Ok(value) => matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

fn env_usize(key: &str) -> Option<usize> {
    env::var(key).ok()?.parse().ok()
}

fn env_u64(key: &str) -> Option<u64> {
    env::var(key).ok()?.parse().ok()
}

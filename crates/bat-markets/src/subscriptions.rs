use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicU64, Ordering},
};

use tokio::sync::{mpsc, oneshot};

use bat_markets_core::{ErrorKind, MarketError, Result};

use crate::{
    client::LiveContext,
    runtime,
    stream::{LiveStreamHandle, PublicSubscription},
};

enum PublicHubCommand {
    Acquire {
        lease_id: u64,
        subscription: PublicSubscription,
        response: oneshot::Sender<Result<()>>,
    },
    Release {
        lease_id: u64,
    },
}

enum PrivateHubCommand {
    Acquire {
        lease_id: u64,
        response: oneshot::Sender<Result<()>>,
    },
    Release {
        lease_id: u64,
    },
}

#[derive(Debug)]
pub(crate) struct PublicSubscriptionHub {
    tx: Option<mpsc::UnboundedSender<PublicHubCommand>>,
    next_lease_id: AtomicU64,
}

#[derive(Debug)]
pub(crate) struct PrivateSubscriptionHub {
    tx: Option<mpsc::UnboundedSender<PrivateHubCommand>>,
    next_lease_id: AtomicU64,
}

#[derive(Debug)]
pub(crate) struct SubscriptionHubs {
    pub(crate) public: PublicSubscriptionHub,
    pub(crate) private: PrivateSubscriptionHub,
}

#[derive(Debug)]
pub(crate) struct PublicSubscriptionLease {
    lease_id: u64,
    tx: mpsc::UnboundedSender<PublicHubCommand>,
}

#[derive(Debug)]
pub(crate) struct PrivateSubscriptionLease {
    lease_id: u64,
    tx: mpsc::UnboundedSender<PrivateHubCommand>,
}

impl SubscriptionHubs {
    pub(crate) fn new(context: LiveContext) -> Self {
        Self {
            public: PublicSubscriptionHub::new(context.clone()),
            private: PrivateSubscriptionHub::new(context),
        }
    }
}

impl PublicSubscriptionHub {
    fn new(context: LiveContext) -> Self {
        let tx = if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let (tx, rx) = mpsc::unbounded_channel();
            handle.spawn(run_public_hub(context, rx));
            Some(tx)
        } else {
            None
        };
        Self {
            tx,
            next_lease_id: AtomicU64::new(1),
        }
    }

    pub(crate) async fn acquire(
        &self,
        subscription: PublicSubscription,
    ) -> Result<PublicSubscriptionLease> {
        let lease_id = self.next_lease_id.fetch_add(1, Ordering::Relaxed);
        let (response_tx, response_rx) = oneshot::channel();
        let tx = self.tx.as_ref().ok_or_else(hub_runtime_error)?.clone();
        tx.send(PublicHubCommand::Acquire {
            lease_id,
            subscription: normalize_public_subscription(subscription),
            response: response_tx,
        })
        .map_err(|_| hub_closed_error("public"))?;
        response_rx
            .await
            .map_err(|_| hub_closed_error("public"))??;
        Ok(PublicSubscriptionLease { lease_id, tx })
    }
}

impl PrivateSubscriptionHub {
    fn new(context: LiveContext) -> Self {
        let tx = if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let (tx, rx) = mpsc::unbounded_channel();
            handle.spawn(run_private_hub(context, rx));
            Some(tx)
        } else {
            None
        };
        Self {
            tx,
            next_lease_id: AtomicU64::new(1),
        }
    }

    pub(crate) async fn acquire(&self) -> Result<PrivateSubscriptionLease> {
        let lease_id = self.next_lease_id.fetch_add(1, Ordering::Relaxed);
        let (response_tx, response_rx) = oneshot::channel();
        let tx = self.tx.as_ref().ok_or_else(hub_runtime_error)?.clone();
        tx.send(PrivateHubCommand::Acquire {
            lease_id,
            response: response_tx,
        })
        .map_err(|_| hub_closed_error("private"))?;
        response_rx
            .await
            .map_err(|_| hub_closed_error("private"))??;
        Ok(PrivateSubscriptionLease { lease_id, tx })
    }
}

impl Drop for PublicSubscriptionLease {
    fn drop(&mut self) {
        let _ = self.tx.send(PublicHubCommand::Release {
            lease_id: self.lease_id,
        });
    }
}

impl Drop for PrivateSubscriptionLease {
    fn drop(&mut self) {
        let _ = self.tx.send(PrivateHubCommand::Release {
            lease_id: self.lease_id,
        });
    }
}

async fn run_public_hub(
    context: LiveContext,
    mut rx: mpsc::UnboundedReceiver<PublicHubCommand>,
) -> Result<()> {
    let mut active = BTreeMap::<u64, PublicSubscription>::new();
    let mut current_subscription: Option<PublicSubscription> = None;
    let mut current_stream: Option<LiveStreamHandle> = None;

    while let Some(command) = rx.recv().await {
        match command {
            PublicHubCommand::Acquire {
                lease_id,
                subscription,
                response,
            } => {
                let mut candidate = active.clone();
                candidate.insert(lease_id, subscription.clone());
                let desired = merge_public_subscriptions(candidate.values());
                match switch_public_stream(
                    &context,
                    &mut current_stream,
                    &mut current_subscription,
                    desired.clone(),
                )
                .await
                {
                    Ok(()) => {
                        active = candidate;
                        let _ = response.send(Ok(()));
                    }
                    Err(error) => {
                        let _ = response.send(Err(error));
                    }
                }
            }
            PublicHubCommand::Release { lease_id } => {
                if active.remove(&lease_id).is_none() {
                    continue;
                }
                let desired = merge_public_subscriptions(active.values());
                let _ = switch_public_stream(
                    &context,
                    &mut current_stream,
                    &mut current_subscription,
                    desired,
                )
                .await;
            }
        }
    }

    if let Some(stream) = current_stream.take() {
        stream.abort();
    }
    Ok(())
}

async fn switch_public_stream(
    context: &LiveContext,
    current_stream: &mut Option<LiveStreamHandle>,
    current_subscription: &mut Option<PublicSubscription>,
    desired: Option<PublicSubscription>,
) -> Result<()> {
    if *current_subscription == desired {
        return Ok(());
    }

    match desired {
        None => {
            if let Some(stream) = current_stream.take() {
                stream.abort();
            }
            *current_subscription = None;
            Ok(())
        }
        Some(subscription) => {
            let next_stream =
                runtime::spawn_public_stream(context.clone(), subscription.clone()).await?;
            if let Some(stream) = current_stream.replace(next_stream) {
                stream.abort();
            }
            *current_subscription = Some(subscription);
            Ok(())
        }
    }
}

async fn run_private_hub(
    context: LiveContext,
    mut rx: mpsc::UnboundedReceiver<PrivateHubCommand>,
) -> Result<()> {
    let mut active = BTreeSet::<u64>::new();
    let mut current_stream: Option<LiveStreamHandle> = None;

    while let Some(command) = rx.recv().await {
        match command {
            PrivateHubCommand::Acquire { lease_id, response } => {
                if current_stream.is_none() {
                    match runtime::spawn_private_stream(context.clone()).await {
                        Ok(stream) => current_stream = Some(stream),
                        Err(error) => {
                            let _ = response.send(Err(error));
                            continue;
                        }
                    }
                }
                active.insert(lease_id);
                let _ = response.send(Ok(()));
            }
            PrivateHubCommand::Release { lease_id } => {
                if !active.remove(&lease_id) {
                    continue;
                }
                if active.is_empty()
                    && let Some(stream) = current_stream.take()
                {
                    stream.abort();
                }
            }
        }
    }

    if let Some(stream) = current_stream.take() {
        stream.abort();
    }
    Ok(())
}

fn normalize_public_subscription(mut subscription: PublicSubscription) -> PublicSubscription {
    subscription.instrument_ids.sort();
    subscription.instrument_ids.dedup();
    subscription.kline_intervals.sort();
    subscription.kline_intervals.dedup();
    subscription
}

fn merge_public_subscriptions<'a>(
    subscriptions: impl IntoIterator<Item = &'a PublicSubscription>,
) -> Option<PublicSubscription> {
    let mut instrument_ids = BTreeSet::new();
    let mut kline_intervals = BTreeSet::<Box<str>>::new();
    let mut merged = PublicSubscription {
        instrument_ids: Vec::new(),
        ticker: false,
        trades: false,
        book_top: false,
        order_book: false,
        mark_price: false,
        funding_rate: false,
        open_interest: false,
        liquidations: false,
        kline_intervals: Vec::new(),
    };

    for subscription in subscriptions {
        instrument_ids.extend(subscription.instrument_ids.iter().cloned());
        kline_intervals.extend(subscription.kline_intervals.iter().cloned());
        merged.ticker |= subscription.ticker;
        merged.trades |= subscription.trades;
        merged.book_top |= subscription.book_top;
        merged.order_book |= subscription.order_book;
        merged.mark_price |= subscription.mark_price;
        merged.funding_rate |= subscription.funding_rate;
        merged.open_interest |= subscription.open_interest;
        merged.liquidations |= subscription.liquidations;
    }

    if instrument_ids.is_empty() {
        return None;
    }

    merged.instrument_ids = instrument_ids.into_iter().collect();
    merged.kline_intervals = kline_intervals.into_iter().collect();
    Some(merged)
}

fn hub_closed_error(name: &str) -> MarketError {
    MarketError::new(
        ErrorKind::TransportError,
        format!("{name} subscription hub is unavailable"),
    )
}

fn hub_runtime_error() -> MarketError {
    MarketError::new(
        ErrorKind::Unsupported,
        "subscription hubs require an active Tokio runtime",
    )
}

#[cfg(test)]
mod tests {
    use bat_markets_core::InstrumentId;

    use super::{PublicSubscription, merge_public_subscriptions, normalize_public_subscription};

    #[test]
    fn normalize_public_subscription_dedupes_symbols_and_intervals() {
        let normalized = normalize_public_subscription(PublicSubscription {
            instrument_ids: vec![
                InstrumentId::from("ETH/USDT:USDT"),
                InstrumentId::from("BTC/USDT:USDT"),
                InstrumentId::from("BTC/USDT:USDT"),
            ],
            ticker: true,
            trades: false,
            book_top: false,
            order_book: false,
            mark_price: false,
            funding_rate: false,
            open_interest: false,
            liquidations: false,
            kline_intervals: vec!["5m".into(), "1m".into(), "1m".into()],
        });

        assert_eq!(
            normalized.instrument_ids,
            vec![
                InstrumentId::from("BTC/USDT:USDT"),
                InstrumentId::from("ETH/USDT:USDT"),
            ]
        );
        assert_eq!(normalized.kline_intervals, vec!["1m".into(), "5m".into()]);
    }

    #[test]
    fn merge_public_subscriptions_unions_topics_without_duplicate_instruments() {
        let first = PublicSubscription {
            instrument_ids: vec![InstrumentId::from("BTC/USDT:USDT")],
            ticker: true,
            trades: false,
            book_top: false,
            order_book: false,
            mark_price: false,
            funding_rate: false,
            open_interest: false,
            liquidations: false,
            kline_intervals: vec!["1m".into()],
        };
        let second = PublicSubscription {
            instrument_ids: vec![
                InstrumentId::from("BTC/USDT:USDT"),
                InstrumentId::from("ETH/USDT:USDT"),
            ],
            ticker: false,
            trades: true,
            book_top: false,
            order_book: true,
            mark_price: false,
            funding_rate: false,
            open_interest: false,
            liquidations: true,
            kline_intervals: vec!["5m".into()],
        };

        let merged = merge_public_subscriptions([&first, &second])
            .expect("merged subscription should exist");

        assert_eq!(
            merged.instrument_ids,
            vec![
                InstrumentId::from("BTC/USDT:USDT"),
                InstrumentId::from("ETH/USDT:USDT"),
            ]
        );
        assert!(merged.ticker);
        assert!(merged.trades);
        assert!(merged.order_book);
        assert!(merged.liquidations);
        assert_eq!(merged.kline_intervals, vec!["1m".into(), "5m".into()]);
    }
}

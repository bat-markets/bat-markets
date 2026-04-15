use tokio::{sync::oneshot, task::JoinHandle};

use bat_markets_core::{
    CommandOperation, CommandReceipt, InstrumentId, PrivateLaneEvent, PublicLaneEvent,
    ReconcileReport, ReconcileTrigger, RequestId, Result,
};

use crate::{client::BatMarkets, runtime};

/// Public market-data subscription plan for live websocket runners.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicSubscription {
    pub instrument_ids: Vec<InstrumentId>,
    pub ticker: bool,
    pub trades: bool,
    pub book_top: bool,
    pub funding_rate: bool,
    pub open_interest: bool,
    pub kline_interval: Option<Box<str>>,
}

impl PublicSubscription {
    #[must_use]
    pub fn all_for(instrument_ids: Vec<InstrumentId>) -> Self {
        Self {
            instrument_ids,
            ticker: true,
            trades: true,
            book_top: true,
            funding_rate: true,
            open_interest: true,
            kline_interval: Some("1".into()),
        }
    }
}

/// Handle for a running live stream task.
pub struct LiveStreamHandle {
    pub(crate) shutdown: Option<oneshot::Sender<()>>,
    pub(crate) join: JoinHandle<Result<()>>,
}

impl LiveStreamHandle {
    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.join.await.map_err(|error| {
            bat_markets_core::MarketError::new(
                bat_markets_core::ErrorKind::TransportError,
                format!("stream task join failed: {error}"),
            )
        })?
    }

    pub fn abort(&self) {
        self.join.abort();
    }

    pub async fn wait(self) -> Result<()> {
        self.join.await.map_err(|error| {
            bat_markets_core::MarketError::new(
                bat_markets_core::ErrorKind::TransportError,
                format!("stream task join failed: {error}"),
            )
        })?
    }
}

/// Entry point for lane-specific ingestion.
pub struct StreamClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> StreamClient<'a> {
    pub(crate) const fn new(inner: &'a BatMarkets) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn public(&self) -> PublicLaneClient<'a> {
        PublicLaneClient { inner: self.inner }
    }

    #[must_use]
    pub fn private(&self) -> PrivateLaneClient<'a> {
        PrivateLaneClient { inner: self.inner }
    }

    #[must_use]
    pub fn command(&self) -> CommandLaneClient<'a> {
        CommandLaneClient { inner: self.inner }
    }
}

/// Public market-data lane ingestion.
pub struct PublicLaneClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> PublicLaneClient<'a> {
    pub fn ingest_json(&self, payload: &str) -> Result<Vec<PublicLaneEvent>> {
        let events = self.inner.adapter.as_adapter().parse_public(payload)?;
        self.inner.write_state(|state| {
            for event in events.iter().cloned() {
                let _ = state.apply_public_event(event);
            }
        });
        Ok(events)
    }

    /// Spawn a reconnecting live public-stream runner.
    pub async fn spawn_live(&self, subscription: PublicSubscription) -> Result<LiveStreamHandle> {
        runtime::spawn_public_stream(self.inner.live_context(), subscription).await
    }
}

/// Private state-lane ingestion.
pub struct PrivateLaneClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> PrivateLaneClient<'a> {
    pub fn ingest_json(&self, payload: &str) -> Result<Vec<PrivateLaneEvent>> {
        let events = self.inner.adapter.as_adapter().parse_private(payload)?;
        self.inner.write_state(|state| {
            for event in events.iter().cloned() {
                state.apply_private_event(event);
            }
        });
        Ok(events)
    }

    /// Spawn a reconnecting live private-stream runner.
    pub async fn spawn_live(&self) -> Result<LiveStreamHandle> {
        runtime::spawn_private_stream(self.inner.live_context()).await
    }

    /// Trigger a manual REST-backed repair cycle for the private-state lane.
    ///
    /// ```no_run
    /// use bat_markets::{BatMarkets, errors::Result, types::{Product, Venue}};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    /// let client = BatMarkets::builder()
    ///     .venue(Venue::Binance)
    ///     .product(Product::LinearUsdt)
    ///     .build_live()
    ///     .await?;
    ///
    /// let report = client.stream().private().reconcile().await?;
    /// println!("reconcile outcome: {:?}", report.outcome);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn reconcile(&self) -> Result<ReconcileReport> {
        runtime::reconcile_private(&self.inner.live_context(), ReconcileTrigger::Manual).await
    }
}

/// Command-lane classification and state hint application.
pub struct CommandLaneClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> CommandLaneClient<'a> {
    pub fn classify_json(
        &self,
        operation: CommandOperation,
        payload: Option<&str>,
        request_id: Option<RequestId>,
    ) -> Result<CommandReceipt> {
        let receipt = self
            .inner
            .adapter
            .as_adapter()
            .classify_command(operation, payload, request_id)?;
        self.inner
            .write_state(|state| state.apply_command_receipt(&receipt));
        Ok(receipt)
    }
}

use tokio::sync::{broadcast, watch};

use bat_markets_core::{ErrorKind, HealthNotification, HealthReport, MarketError, Result};

use crate::client::BatMarkets;

/// Cheap health snapshots for applications and automation.
pub struct HealthClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> HealthClient<'a> {
    pub(crate) const fn new(inner: &'a BatMarkets) -> Self {
        Self { inner }
    }

    /// Return the current health snapshot.
    #[must_use]
    pub fn snapshot(&self) -> HealthReport {
        self.inner.shared.health_snapshot()
    }

    /// Subscribe to snapshot-style health changes.
    ///
    /// ```no_run
    /// use bat_markets::{BatMarkets, errors::Result, types::{Product, Venue}};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    /// let client = BatMarkets::builder()
    ///     .venue(Venue::Bybit)
    ///     .product(Product::LinearUsdt)
    ///     .build_live()
    ///     .await?;
    /// let receiver = client.health().subscribe();
    /// let _initial = receiver.borrow().clone();
    /// # Ok(())
    /// # }
    /// ```
    pub fn subscribe(&self) -> watch::Receiver<HealthReport> {
        self.inner.shared.subscribe_health()
    }

    /// Subscribe to transition-style health notifications.
    ///
    /// Notifications are emitted for structural state changes, not every market-data tick.
    pub fn notifications(&self) -> broadcast::Receiver<HealthNotification> {
        self.inner.shared.subscribe_health_notifications()
    }
}

/// RAII-style watcher for runtime status changes.
///
/// Dropping the watcher releases the local subscription. There is no global
/// `un_watch` method because Rust ownership already scopes the subscription.
pub struct StatusWatch {
    receiver: watch::Receiver<HealthReport>,
}

impl StatusWatch {
    pub(crate) const fn new(receiver: watch::Receiver<HealthReport>) -> Self {
        Self { receiver }
    }

    /// Return the current cached status snapshot without waiting.
    #[must_use]
    pub fn current(&self) -> HealthReport {
        self.receiver.borrow().clone()
    }

    /// Wait for the next status change.
    pub async fn recv(&mut self) -> Result<HealthReport> {
        self.receiver.changed().await.map_err(|error| {
            MarketError::new(
                ErrorKind::TransportError,
                format!("status watch receive failed: {error}"),
            )
        })?;
        Ok(self.current())
    }

    /// Explicitly end the watch.
    ///
    /// This mirrors other watch handles. Dropping the handle is equivalent for
    /// the local health subscription.
    pub async fn shutdown(self) -> Result<()> {
        Ok(())
    }
}

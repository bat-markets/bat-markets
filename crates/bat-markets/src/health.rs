use tokio::sync::{broadcast, watch};

use bat_markets_core::{HealthNotification, HealthReport};

use crate::client::BatMarkets;

/// Cheap health snapshots for applications and automation.
pub struct HealthClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> HealthClient<'a> {
    pub(crate) const fn new(inner: &'a BatMarkets) -> Self {
        Self { inner }
    }

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

use tokio::sync::watch;

use bat_markets_core::{ErrorKind, HealthReport, MarketError, Result};

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

use bat_markets_core::{
    FundingRate, InstrumentId, InstrumentSpec, OpenInterest, Result, Ticker, TradeTick,
};

use crate::{client::BatMarkets, runtime};

/// Read-only ergonomic access to market snapshots maintained by the engine.
pub struct MarketClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> MarketClient<'a> {
    pub(crate) const fn new(inner: &'a BatMarkets) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn instrument_specs(&self) -> Vec<InstrumentSpec> {
        self.inner.instrument_specs()
    }

    #[must_use]
    pub fn ticker(&self, instrument_id: &InstrumentId) -> Option<Ticker> {
        self.inner
            .read_state(|state| state.ticker(instrument_id).cloned())
    }

    #[must_use]
    pub fn recent_trades(&self, instrument_id: &InstrumentId) -> Option<Vec<TradeTick>> {
        self.inner
            .read_state(|state| state.recent_trades(instrument_id))
    }

    #[must_use]
    pub fn book_top(&self, instrument_id: &InstrumentId) -> Option<bat_markets_core::BookTop> {
        self.inner
            .read_state(|state| state.book_top(instrument_id).cloned())
    }

    #[must_use]
    pub fn funding_rate(&self, instrument_id: &InstrumentId) -> Option<FundingRate> {
        self.inner
            .read_state(|state| state.funding_rate(instrument_id).cloned())
    }

    #[must_use]
    pub fn open_interest(&self, instrument_id: &InstrumentId) -> Option<OpenInterest> {
        self.inner
            .read_state(|state| state.open_interest(instrument_id).cloned())
    }

    pub fn require_instrument(&self, instrument_id: &InstrumentId) -> Result<InstrumentSpec> {
        self.instrument_specs()
            .into_iter()
            .find(|spec| &spec.instrument_id == instrument_id)
            .ok_or_else(|| {
                bat_markets_core::MarketError::new(
                    bat_markets_core::ErrorKind::Unsupported,
                    format!("unknown instrument {instrument_id}"),
                )
            })
    }

    /// Refresh live `InstrumentSpec` snapshots from the selected venue.
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
    ///
    /// let refreshed = client.market().refresh_metadata().await?;
    /// println!("refreshed {} instruments", refreshed.len());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn refresh_metadata(&self) -> Result<Vec<InstrumentSpec>> {
        runtime::refresh_metadata(&self.inner.live_context()).await
    }

    pub async fn refresh_open_interest(
        &self,
        instrument_id: &InstrumentId,
    ) -> Result<OpenInterest> {
        runtime::refresh_open_interest(&self.inner.live_context(), instrument_id).await
    }
}

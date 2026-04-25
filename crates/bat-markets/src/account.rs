use bat_markets_core::{AccountSummary, Balance};

use crate::{client::BatMarkets, runtime};

/// Ergonomic access to account state.
pub struct AccountClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> AccountClient<'a> {
    pub(crate) const fn new(inner: &'a BatMarkets) -> Self {
        Self { inner }
    }

    /// Return cached balances from the private state lane.
    #[must_use]
    pub fn balances(&self) -> Vec<Balance> {
        self.inner
            .read_state(bat_markets_core::EngineState::balances)
    }

    /// Return the cached account summary, if one has been observed or refreshed.
    #[must_use]
    pub fn summary(&self) -> Option<AccountSummary> {
        self.inner
            .read_state(bat_markets_core::EngineState::account_summary)
    }

    /// Refresh account summary and balances through live REST and merge them into state.
    pub async fn refresh(&self) -> bat_markets_core::Result<Option<AccountSummary>> {
        runtime::refresh_account(&self.inner.live_context()).await
    }
}

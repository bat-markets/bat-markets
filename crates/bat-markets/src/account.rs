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

    #[must_use]
    pub fn balances(&self) -> Vec<Balance> {
        self.inner
            .read_state(bat_markets_core::EngineState::balances)
    }

    #[must_use]
    pub fn summary(&self) -> Option<AccountSummary> {
        self.inner
            .read_state(bat_markets_core::EngineState::account_summary)
    }

    pub async fn refresh(&self) -> bat_markets_core::Result<Option<AccountSummary>> {
        runtime::refresh_account(&self.inner.live_context()).await
    }
}

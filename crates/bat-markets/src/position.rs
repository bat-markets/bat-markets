use bat_markets_core::{
    CommandReceipt, Position, Result, SetLeverageRequest, SetMarginModeRequest,
    SetPositionModeRequest,
};

use crate::{client::BatMarkets, runtime};

/// Ergonomic access to position state.
pub struct PositionClient<'a> {
    inner: &'a BatMarkets,
}

impl<'a> PositionClient<'a> {
    pub(crate) const fn new(inner: &'a BatMarkets) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn list(&self) -> Vec<Position> {
        self.inner
            .read_state(bat_markets_core::EngineState::positions)
    }

    pub async fn refresh(&self) -> Result<Vec<Position>> {
        runtime::refresh_positions(&self.inner.live_context()).await
    }

    #[deprecated(
        since = "0.2.0",
        note = "use entry().set_leverage(...) for command handles and lifecycle tracking"
    )]
    pub async fn set_leverage(&self, request: &SetLeverageRequest) -> Result<CommandReceipt> {
        runtime::set_leverage(&self.inner.live_context(), request).await
    }

    #[deprecated(
        since = "0.2.0",
        note = "use entry().set_margin_mode(...) for command handles and lifecycle tracking"
    )]
    pub async fn set_margin_mode(&self, request: &SetMarginModeRequest) -> Result<CommandReceipt> {
        runtime::set_margin_mode(&self.inner.live_context(), request).await
    }

    #[deprecated(
        since = "0.2.0",
        note = "use entry().set_position_mode(...) for command handles and lifecycle tracking"
    )]
    pub async fn set_position_mode(
        &self,
        request: &SetPositionModeRequest,
    ) -> Result<CommandReceipt> {
        runtime::set_position_mode(&self.inner.live_context(), request).await
    }
}

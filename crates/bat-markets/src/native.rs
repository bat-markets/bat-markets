use bat_markets_core::Result;
#[cfg(all(feature = "binance", feature = "bybit"))]
use bat_markets_core::{ErrorKind, MarketError};

use crate::client::AdapterHandle;

/// Access to venue-specific adapter functionality.
pub struct NativeClient<'a> {
    inner: &'a AdapterHandle,
}

impl<'a> NativeClient<'a> {
    pub(crate) const fn new(inner: &'a AdapterHandle) -> Self {
        Self { inner }
    }

    #[cfg(feature = "binance")]
    pub fn binance(&self) -> Result<&bat_markets_binance::BinanceLinearFuturesAdapter> {
        match self.inner {
            AdapterHandle::Binance(adapter) => Ok(adapter),
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(_) => Err(MarketError::new(
                ErrorKind::Unsupported,
                "native binance access requested for non-binance client",
            )),
        }
    }

    #[cfg(feature = "bybit")]
    pub fn bybit(&self) -> Result<&bat_markets_bybit::BybitLinearFuturesAdapter> {
        match self.inner {
            AdapterHandle::Bybit(adapter) => Ok(adapter),
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(_) => Err(MarketError::new(
                ErrorKind::Unsupported,
                "native bybit access requested for non-bybit client",
            )),
        }
    }
}

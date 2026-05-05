use bat_markets_core::Result;
#[cfg(any(
    all(feature = "binance", feature = "bybit"),
    all(feature = "binance", feature = "mexc"),
    all(feature = "bybit", feature = "mexc")
))]
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

    /// Return the Binance adapter when this client was built for Binance.
    #[cfg(feature = "binance")]
    pub fn binance(&self) -> Result<&bat_markets_binance::BinanceLinearFuturesAdapter> {
        match self.inner {
            AdapterHandle::Binance(adapter) => Ok(adapter),
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(_) => Err(MarketError::new(
                ErrorKind::Unsupported,
                "native binance access requested for non-binance client",
            )),
            #[cfg(feature = "mexc")]
            AdapterHandle::Mexc(_) => Err(MarketError::new(
                ErrorKind::Unsupported,
                "native binance access requested for non-binance client",
            )),
        }
    }

    /// Return the Bybit adapter when this client was built for Bybit.
    #[cfg(feature = "bybit")]
    pub fn bybit(&self) -> Result<&bat_markets_bybit::BybitLinearFuturesAdapter> {
        match self.inner {
            AdapterHandle::Bybit(adapter) => Ok(adapter),
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(_) => Err(MarketError::new(
                ErrorKind::Unsupported,
                "native bybit access requested for non-bybit client",
            )),
            #[cfg(feature = "mexc")]
            AdapterHandle::Mexc(_) => Err(MarketError::new(
                ErrorKind::Unsupported,
                "native bybit access requested for non-bybit client",
            )),
        }
    }

    /// Return the MEXC adapter when this client was built for MEXC.
    #[cfg(feature = "mexc")]
    pub fn mexc(&self) -> Result<&bat_markets_mexc::MexcLinearFuturesAdapter> {
        match self.inner {
            AdapterHandle::Mexc(adapter) => Ok(adapter),
            #[cfg(feature = "binance")]
            AdapterHandle::Binance(_) => Err(MarketError::new(
                ErrorKind::Unsupported,
                "native mexc access requested for non-mexc client",
            )),
            #[cfg(feature = "bybit")]
            AdapterHandle::Bybit(_) => Err(MarketError::new(
                ErrorKind::Unsupported,
                "native mexc access requested for non-mexc client",
            )),
        }
    }
}

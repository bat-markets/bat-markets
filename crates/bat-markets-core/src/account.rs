use serde::{Deserialize, Serialize};

use crate::ids::AssetCode;
use crate::numeric::Amount;
use crate::primitives::TimestampMs;

/// Balance snapshot for a single asset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Balance {
    pub asset: AssetCode,
    pub wallet_balance: Amount,
    pub available_balance: Amount,
    pub updated_at: TimestampMs,
}

/// Coarse account summary for futures trading.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountSummary {
    pub total_wallet_balance: Amount,
    pub total_available_balance: Amount,
    pub total_unrealized_pnl: Amount,
    pub updated_at: TimestampMs,
}

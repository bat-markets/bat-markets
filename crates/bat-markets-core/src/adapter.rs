use crate::capability::CapabilitySet;
use crate::command::{CommandOperation, CommandReceipt};
use crate::config::BatMarketsConfig;
use crate::error::Result;
use crate::execution::{LaneSet, PrivateLaneEvent, PublicLaneEvent};
use crate::ids::{InstrumentId, RequestId};
use crate::instrument::InstrumentSpec;
use crate::types::{Product, Venue};

/// Internal adapter contract for venue-specific parsing and classification.
pub trait VenueAdapter: Send + Sync {
    fn venue(&self) -> Venue;
    fn product(&self) -> Product;
    fn config(&self) -> &BatMarketsConfig;
    fn capabilities(&self) -> CapabilitySet;
    fn lane_set(&self) -> LaneSet;
    fn instrument_specs(&self) -> Vec<InstrumentSpec>;
    fn resolve_instrument(&self, instrument_id: &InstrumentId) -> Option<InstrumentSpec>;
    fn resolve_native_symbol(&self, native_symbol: &str) -> Option<InstrumentSpec>;
    fn parse_public(&self, payload: &str) -> Result<Vec<PublicLaneEvent>>;
    fn parse_private(&self, payload: &str) -> Result<Vec<PrivateLaneEvent>>;
    fn classify_command(
        &self,
        operation: CommandOperation,
        payload: Option<&str>,
        request_id: Option<RequestId>,
    ) -> Result<CommandReceipt>;
}

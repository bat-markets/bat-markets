use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ids::RequestId;
use crate::types::{Product, Venue};

/// Stable machine-handleable error classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    ConfigError,
    TransportError,
    Timeout,
    RateLimited,
    AuthError,
    PermissionDenied,
    Unsupported,
    DecodeError,
    ExchangeReject,
    UnknownExecution,
    StateDivergence,
    ComplianceRequired,
    TemporaryUnavailable,
}

/// Safe structured context for an error.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorContext {
    pub venue: Option<Venue>,
    pub product: Option<Product>,
    pub operation: Option<Box<str>>,
    pub request_id: Option<RequestId>,
    pub native_code: Option<Box<str>>,
    pub retriable: bool,
}

/// Canonical error type used across the engine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Error)]
#[error("{kind:?}: {message}")]
pub struct MarketError {
    pub kind: ErrorKind,
    pub message: Box<str>,
    pub context: ErrorContext,
}

/// Canonical result type.
pub type Result<T> = core::result::Result<T, MarketError>;

impl MarketError {
    #[must_use]
    pub fn new(kind: ErrorKind, message: impl Into<Box<str>>) -> Self {
        Self {
            kind,
            message: message.into(),
            context: ErrorContext::default(),
        }
    }

    #[must_use]
    pub fn with_venue(mut self, venue: Venue, product: Product) -> Self {
        self.context.venue = Some(venue);
        self.context.product = Some(product);
        self
    }

    #[must_use]
    pub fn with_operation(mut self, operation: impl Into<Box<str>>) -> Self {
        self.context.operation = Some(operation.into());
        self
    }

    #[must_use]
    pub fn with_request_id(mut self, request_id: RequestId) -> Self {
        self.context.request_id = Some(request_id);
        self
    }

    #[must_use]
    pub fn with_native_code(mut self, native_code: impl Into<Box<str>>) -> Self {
        self.context.native_code = Some(native_code.into());
        self
    }

    #[must_use]
    pub const fn with_retriable(mut self, retriable: bool) -> Self {
        self.context.retriable = retriable;
        self
    }
}

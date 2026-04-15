use core::fmt;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorKind, MarketError, Result};

macro_rules! decimal_value {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        pub struct $name(Decimal);

        impl $name {
            #[must_use]
            pub const fn new(value: Decimal) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn value(self) -> Decimal {
                self.0
            }
        }

        impl From<Decimal> for $name {
            fn from(value: Decimal) -> Self {
                Self::new(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

macro_rules! fast_value {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        pub struct $name(i64);

        impl $name {
            #[must_use]
            pub const fn new(value: i64) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn value(self) -> i64 {
                self.0
            }
        }
    };
}

decimal_value!(Amount);
decimal_value!(Price);
decimal_value!(Quantity);
decimal_value!(Notional);
decimal_value!(Leverage);
decimal_value!(Rate);

fast_value!(FastPrice);
fast_value!(FastQuantity);
fast_value!(FastNotional);

impl Notional {
    #[must_use]
    pub fn from_price_qty(price: Price, quantity: Quantity) -> Self {
        Self(price.value() * quantity.value())
    }
}

impl Price {
    pub fn quantize(self, scale: u32) -> Result<FastPrice> {
        quantize_decimal(self.value(), scale).map(FastPrice::new)
    }
}

impl Quantity {
    pub fn quantize(self, scale: u32) -> Result<FastQuantity> {
        quantize_decimal(self.value(), scale).map(FastQuantity::new)
    }
}

impl Notional {
    pub fn quantize(self, scale: u32) -> Result<FastNotional> {
        quantize_decimal(self.value(), scale).map(FastNotional::new)
    }
}

impl FastPrice {
    #[must_use]
    pub fn to_price(self, scale: u32) -> Price {
        Price::new(Decimal::new(self.value(), scale))
    }
}

impl FastQuantity {
    #[must_use]
    pub fn to_quantity(self, scale: u32) -> Quantity {
        Quantity::new(Decimal::new(self.value(), scale))
    }
}

impl FastNotional {
    #[must_use]
    pub fn to_notional(self, scale: u32) -> Notional {
        Notional::new(Decimal::new(self.value(), scale))
    }
}

fn quantize_decimal(mut value: Decimal, scale: u32) -> Result<i64> {
    value.rescale(scale);
    i64::try_from(value.mantissa()).map_err(|_| {
        MarketError::new(
            ErrorKind::ConfigError,
            format!("value {value} does not fit into fast-path i64 representation"),
        )
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use proptest::test_runner::TestCaseError;
    use rust_decimal::Decimal;

    use super::{Price, Quantity};

    proptest! {
        #[test]
        fn price_quantize_roundtrip(units in 1_i64..1_000_000_i64, scale in 0_u32..4_u32) {
            let value = Price::new(Decimal::new(units, scale));
            let fast = match value.quantize(scale) {
                Ok(value) => value,
                Err(error) => return Err(TestCaseError::fail(error.to_string())),
            };
            prop_assert_eq!(fast.to_price(scale), value);
        }

        #[test]
        fn quantity_quantize_roundtrip(units in 1_i64..1_000_000_i64, scale in 0_u32..4_u32) {
            let value = Quantity::new(Decimal::new(units, scale));
            let fast = match value.quantize(scale) {
                Ok(value) => value,
                Err(error) => return Err(TestCaseError::fail(error.to_string())),
            };
            prop_assert_eq!(fast.to_quantity(scale), value);
        }
    }
}

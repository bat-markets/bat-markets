use std::collections::BTreeMap;

use crate::ids::InstrumentId;
use crate::instrument::InstrumentSpec;

/// Cheap cloned instrument catalog used by adapters and state bootstrap.
#[derive(Clone, Debug, Default)]
pub struct InstrumentCatalog {
    by_id: BTreeMap<InstrumentId, InstrumentSpec>,
    by_native_symbol: BTreeMap<Box<str>, InstrumentId>,
}

impl InstrumentCatalog {
    #[must_use]
    pub fn new(specs: impl IntoIterator<Item = InstrumentSpec>) -> Self {
        let mut by_id = BTreeMap::new();
        let mut by_native_symbol = BTreeMap::new();

        for spec in specs {
            by_native_symbol.insert(spec.native_symbol.clone(), spec.instrument_id.clone());
            by_id.insert(spec.instrument_id.clone(), spec);
        }

        Self {
            by_id,
            by_native_symbol,
        }
    }

    pub fn replace(&mut self, specs: impl IntoIterator<Item = InstrumentSpec>) {
        *self = Self::new(specs);
    }

    #[must_use]
    pub fn all(&self) -> Vec<InstrumentSpec> {
        self.by_id.values().cloned().collect()
    }

    #[must_use]
    pub fn get(&self, instrument_id: &InstrumentId) -> Option<InstrumentSpec> {
        self.by_id.get(instrument_id).cloned()
    }

    #[must_use]
    pub fn by_native_symbol(&self, native_symbol: &str) -> Option<InstrumentSpec> {
        self.by_native_symbol
            .get(native_symbol)
            .and_then(|instrument_id| self.get(instrument_id))
    }
}

//! Named position memory. Mirrors src/agent/memory_bank.js.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryBank {
    places: HashMap<String, [f64; 3]>,
}

impl MemoryBank {
    pub fn new() -> Self { Self::default() }

    pub fn remember_place(&mut self, name: &str, x: f64, y: f64, z: f64) {
        self.places.insert(name.to_string(), [x, y, z]);
    }

    pub fn recall_place(&self, name: &str) -> Option<[f64; 3]> {
        self.places.get(name).copied()
    }

    pub fn get_place_names(&self) -> String {
        self.places.keys().cloned().collect::<Vec<_>>().join(", ")
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(&self.places).unwrap_or_default()
    }

    pub fn from_json(&mut self, val: &serde_json::Value) {
        if let Ok(places) = serde_json::from_value(val.clone()) {
            self.places = places;
        }
    }
}

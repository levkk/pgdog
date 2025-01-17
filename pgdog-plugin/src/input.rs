//! Plugin input helpers.
use crate::bindings::{self, Config, InputType_ROUTING_INPUT, RoutingInput};

impl bindings::Input {
    /// Create new plugin input.
    pub fn new(config: Config, input: RoutingInput) -> Self {
        Self {
            config,
            input,
            input_type: InputType_ROUTING_INPUT,
        }
    }

    /// Deallocate memory.
    ///
    /// # Safety
    ///
    /// This is not to be used by plugins.
    /// # Safety
    ///
    /// This is for internal pgDog usage only.
    pub unsafe fn deallocate(&self) {
        self.config.deallocate();
    }

    /// Get query if this is a routing input.
    #[allow(non_upper_case_globals)]
    pub fn query(&self) -> Option<bindings::Query> {
        match self.input_type {
            InputType_ROUTING_INPUT => Some(unsafe { self.input.query }),
            _ => None,
        }
    }
}

impl RoutingInput {
    /// Create query routing input.
    pub fn query(query: bindings::Query) -> Self {
        Self { query }
    }
}

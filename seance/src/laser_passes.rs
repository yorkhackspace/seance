//! `laser_passes`
//!
//! Contains definitions for tool passes.

use serde::{Deserialize, Serialize};

/// The settings for a single pass of the tool head over lines of a given colour.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Hash)]
pub struct ToolPass {
    /// The user-specified name of this tool pass.
    name: String,
    /// Colour channel value of lines to machine [R, G, B].
    colour: [u8; 3],
    /// Tool power, max 1000. Unitless, proportion of max.
    power: u64,
    /// Tool speed, max 1000. Unitless, proportion of max.
    speed: u64,
    /// Whether this tool pass is enabled.
    /// If so then paths with the colour of this pass will be cut with this tool pass.
    enabled: bool,
}

impl ToolPass {
    /// Creates a new [`ToolPass`]
    ///
    /// # Arguments
    /// * `name`: Name of the tool pass, used for display to user, not used to generate HPGL.
    /// * `r`: Red channel value.
    /// * `g`: Green channel value.
    /// * `b`: Blue channel value.
    /// * `power`: Tool power, will be clamped to 1000.
    /// * `speed`: Tool speed, will be clamped to 1000.
    /// * `enabled`: Whether the tool pass is enabled.
    ///
    /// # Returns
    /// A new [`ToolPass`] with values appropriately clamped.
    pub fn new(name: String, r: u8, g: u8, b: u8, power: u64, speed: u64, enabled: bool) -> Self {
        ToolPass {
            name,
            colour: [r, g, b],
            power: power.min(1000),
            speed: speed.min(1000),
            enabled,
        }
    }

    /// Gets the name of the tool pass.
    ///
    /// # Returns
    /// The name of the tool pass.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sets the name of the tool pass.
    ///
    /// # Arguments
    /// * `name`: The new name of the tool pass.
    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    /// Gets the colour of the tool pass.
    ///
    /// # Returns
    /// The colour of the tool pass.
    pub fn colour(&self) -> &[u8; 3] {
        &self.colour
    }

    /// Sets the colour of the tool pass.
    ///
    /// # Arguments
    /// * `colour`: The new colour of the tool pass.
    pub fn set_colour(&mut self, colour: [u8; 3]) {
        self.colour = colour;
    }

    /// Gets the speed of the tool pass.
    ///
    /// # Returns
    /// The speed of the tool pass.
    pub fn speed(&self) -> &u64 {
        &self.speed
    }

    /// Sets the speed of the tool pass.
    ///
    /// # Arguments
    /// * `speed`: The new speed of the tool pass.
    pub fn set_speed(&mut self, speed: u64) {
        self.speed = speed.min(1000);
    }

    /// Gets the power of the tool pass.
    ///
    /// # Returns
    /// The power of the tool pass.
    pub fn power(&self) -> &u64 {
        &self.power
    }

    /// Sets the power of the tool pass.
    ///
    /// # Arguments
    /// * `power`: The new power of the tool pass.
    pub fn set_power(&mut self, power: u64) {
        self.power = power.min(1000);
    }

    /// Gets the enable state of the tool pass
    ///
    /// # Returns
    /// Whether the tool pass is enabled.
    pub fn enabled(&self) -> &bool {
        &self.enabled
    }

    /// Sets the enable state of the tool pass
    ///
    /// # Arguments
    /// * `new_state`: The new enable state of the tool pass.
    pub fn set_enabled(&mut self, new_state: bool) {
        self.enabled = new_state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_pass_new() {
        assert_eq!(
            ToolPass::new("non-restricted pass".to_string(), 0, 0, 0, 500, 100, true),
            ToolPass {
                name: "non-restricted pass".to_string(),
                colour: [0, 0, 0],
                power: 500,
                speed: 100,
                enabled: true
            }
        );

        assert_eq!(
            ToolPass::new(
                "truncated power & speed pass".to_string(),
                0,
                0,
                0,
                10_000,
                u64::MAX,
                true
            ),
            ToolPass {
                name: "truncated power & speed pass".to_string(),
                colour: [0, 0, 0],
                power: 1000,
                speed: 1000,
                enabled: true
            }
        );
    }

    #[test]
    fn test_tool_pass_set_speed() {
        let mut pass = ToolPass::new("".to_string(), 0, 0, 0, 100, 100, false);
        assert_eq!(pass.speed, 100);

        // should not truncate
        pass.set_speed(500);
        assert_eq!(pass.speed, 500);

        // should truncate
        pass.set_speed(1_000_000);
        assert_eq!(pass.speed, 1000);
    }

    #[test]
    fn test_tool_pass_set_power() {
        let mut pass = ToolPass::new("".to_string(), 0, 0, 0, 100, 100, false);
        assert_eq!(pass.power, 100);

        // should not truncate
        pass.set_power(10);
        assert_eq!(pass.power, 10);

        // should truncate
        pass.set_power(1001);
        assert_eq!(pass.power, 1000);
    }
}

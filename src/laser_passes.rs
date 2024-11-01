use serde::{Deserialize, Serialize};

/// The settings for a single pass of the tool head over lines of a given colour.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Hash)]
pub struct ToolPass {
    name: String,
    /// Colour channel value of lines to machine [R, G, B].
    colour: [u8; 3],
    /// Tool power, max 1000. Unitless, proportion of max.
    power: u64,
    /// Tool speed, max 1000. Unitless, proportion of max.
    speed: u64,
    /// Raster engrave.
    rast: bool,
    /// ? Unknown.
    vect: bool,
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
    /// * `rast`: Raster engrave.
    /// * `vect`: ? Unknown.
    ///
    /// # Returns
    /// A new [`ToolPass`] with values appropriately clamped.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: String,
        r: u8,
        g: u8,
        b: u8,
        power: u64,
        speed: u64,
        rast: bool,
        vect: bool,
    ) -> Self {
        ToolPass {
            name,
            colour: [r, g, b],
            power: power.min(1000),
            speed: speed.min(1000),
            rast,
            vect,
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
}

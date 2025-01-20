//! A set of default passes that are populated when the program first starts
use crate::ToolPass;

/// Gets the default tool passes.
///
/// # Returns
/// An array of default tool passes.
pub fn default_passes() -> Vec<ToolPass> {
    [
        ToolPass::new("Pass 1".to_string(), 0, 0, 0, 100, 20, false),
        ToolPass::new("Pass 2".to_string(), 255, 0, 0, 100, 20, false),
        ToolPass::new("Pass 3".to_string(), 0, 255, 0, 100, 20, false),
        ToolPass::new("Pass 4".to_string(), 0, 0, 255, 100, 20, false),
    ]
    .to_vec()
}

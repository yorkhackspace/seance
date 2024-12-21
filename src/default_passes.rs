use crate::ToolPass;

/// Gets the default tool passes.
///
/// # Returns
/// An array of default tool passes.
pub fn default_passes() -> [ToolPass; 16] {
    [
        ToolPass::new("Pass 1".to_string(), 0, 0, 0, 100, 20, false),
        ToolPass::new("Pass 2".to_string(), 255, 0, 0, 100, 20, false),
        ToolPass::new("Pass 3".to_string(), 0, 255, 0, 100, 20, false),
        ToolPass::new("Pass 4".to_string(), 0, 0, 255, 100, 20, false),
        ToolPass::new("Pass 5".to_string(), 255, 255, 0, 100, 20, false),
        ToolPass::new("Pass 6".to_string(), 255, 0, 255, 100, 20, false),
        ToolPass::new("Pass 7".to_string(), 0, 255, 255, 100, 20, false),
        ToolPass::new("Pass 8".to_string(), 255, 255, 255, 100, 20, false),
        ToolPass::new("Pass 9".to_string(), 128, 0, 0, 100, 20, false),
        ToolPass::new("Pass 10".to_string(), 0, 128, 0, 100, 20, false),
        ToolPass::new("Pass 11".to_string(), 0, 0, 128, 100, 20, false),
        ToolPass::new("Pass 12".to_string(), 128, 128, 0, 100, 20, false),
        ToolPass::new("Pass 13".to_string(), 128, 0, 128, 100, 20, false),
        ToolPass::new("Pass 14".to_string(), 0, 128, 128, 100, 20, false),
        ToolPass::new("Pass 15".to_string(), 128, 128, 128, 100, 20, false),
        ToolPass::new("Pass 16".to_string(), 255, 128, 0, 100, 20, false),
    ]
}

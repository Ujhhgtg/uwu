/// The rustc version string embedded at build time.
/// Format: `"rustc X.Y.Z (hash YYYY-MM-DD)"`.
pub const RUSTC_VERSION: &str = env!("RUSTC_VERSION");

/// Trait that every plugin must implement.
///
/// Everything is called from the main render thread (winit event loop).
pub trait Plugin {
    /// Unique plugin identifier — must consist only of letters, digits, and underscores.
    fn id(&self) -> &'static str;

    /// Display name shown in the plugin list.
    fn name(&self) -> &'static str;

    /// Semantic version string.
    fn version(&self) -> &'static str;

    /// Called once immediately after the plugin is constructed and loaded.
    ///
    /// This is the plugin's opportunity to allocate resources, register
    /// callbacks, or spawn background tasks. Runs on the main render thread.
    fn init(&mut self) {}

    /// Called once per frame before the UI phase.
    ///
    /// Use this for per-frame setup that does not render its own window.
    fn before_ui(&mut self) {}

    /// Called once when the plugin is being unloaded (LoadedPlugin drop).
    ///
    /// Use this to tear down resources, join threads, or flush state.
    /// The library is still loaded when this runs, so vtable access is safe.
    fn uninit(&mut self) {}

    /// Called once per frame after `before_ui()`.
    ///
    /// The plugin should render its own egui window(s) using `ctx`.
    #[allow(unused)]
    fn ui(&mut self, ctx: &egui::Context) {}
}

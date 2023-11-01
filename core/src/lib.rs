#[macro_use]
extern crate slog;

mod agent;
pub mod error;
mod messaging;
mod stubs;

pub use agent::*;
pub use messaging::*;
pub use stubs::*;

pub static CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub static RUSTC_VERSION: &str = env!("RUSTC_VERSION");

pub struct PluginDeclaration {
    pub rustc_version: &'static str,
    pub core_version: &'static str,
    pub agent_version: u32,
    pub agent_name: &'static str,
    pub agent_builder: fn(
        config: Option<&str>,
        logger: slog::Logger,
        sender: tokio::sync::mpsc::Sender<AgentMessage>,
    ) -> Box<dyn AgentTrait>,
}

#[macro_export]
macro_rules! export_plugin {
    ($name:expr, $version:expr, $agent_builder:expr) => {
        #[doc(hidden)]
        #[no_mangle]
        pub static plugin_declaration: $crate::PluginDeclaration = $crate::PluginDeclaration {
            rustc_version: $crate::RUSTC_VERSION,
            core_version: $crate::CORE_VERSION,
            agent_name: $name,
            agent_version: $version,
            agent_builder: $agent_builder,
        };
    };
}

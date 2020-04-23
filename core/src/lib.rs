#[macro_use]
extern crate slog;

mod agent;
pub mod error;
mod messaging;

pub use agent::*;
pub use messaging::*;

pub static CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub static RUSTC_VERSION: &str = env!("RUSTC_VERSION");

pub struct PluginDeclaration {
    pub rustc_version: &'static str,
    pub core_version: &'static str,
    pub agent_name: &'static str,
    pub agent_builder: unsafe extern "C" fn(
        config: Option<&std::string::String>,
        logger: slog::Logger,
        sender: tokio::sync::mpsc::Sender<AgentPayload>,
    ) -> Box<dyn AgentTrait>,
}

#[macro_export]
macro_rules! export_plugin {
    ($name:expr, $agent_builder:expr) => {
        #[doc(hidden)]
        #[no_mangle]
        pub static plugin_declaration: $crate::PluginDeclaration = $crate::PluginDeclaration {
            rustc_version: $crate::RUSTC_VERSION,
            core_version: $crate::CORE_VERSION,
            agent_name: $name,
            agent_builder: $agent_builder,
        };
    };
}

/*
 * Serde Workaround
 */

pub fn logger_sentinel() -> slog::Logger {
    let sentinel = slog::Logger::root(slog::Discard, o!("" => ""));
    sentinel
}

pub fn sender_sentinel() -> tokio::sync::mpsc::Sender<AgentPayload> {
    let (sentinel, _) = tokio::sync::mpsc::channel(0);
    sentinel
}

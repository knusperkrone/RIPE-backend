mod agent;
pub mod error;
mod messaging;
mod stream;
mod stubs;

pub use agent::*;
pub use messaging::*;
pub use stream::*;
pub use stubs::*;

pub static CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub static RUSTC_VERSION: &str = env!("RUSTC_VERSION");

type AgentBuilder =
    extern "Rust" fn(config: Option<&str>, sender: AgentStreamSender) -> Box<dyn AgentTrait>;

pub struct PluginDeclaration {
    pub rustc_version: &'static str,
    pub core_version: &'static str,
    pub agent_version: u32,
    pub agent_name: &'static str,
    pub agent_builder: AgentBuilder,
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

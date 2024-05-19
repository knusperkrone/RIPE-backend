use crate::AgentStreamSender;
use std::sync::Arc;
use tracing::debug;

/*
 * conventions
 */

pub const CMD_ACTIVE: i32 = 1;
pub const CMD_INACTIVE: i32 = 0;

pub const DAY_MS: u32 = 86_400_000;

#[tracing::instrument]
pub fn sleep(runtime: &tokio::runtime::Handle, duration: std::time::Duration) {
    debug!("Sleeping for {:?}", duration);
    let _guard = runtime.enter();
    debug!("Entered runtime");
    runtime.block_on(tokio::time::sleep(duration));
    debug!("Slept for {:?}", duration);
}

pub fn ms_to_hr(time_ms: u32) -> String {
    let seconds = time_ms / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;

    let pad_fn = |x| {
        if x < 10 {
            format!("0{}", x)
        } else {
            format!("{}", x)
        }
    };
    format!("{}:{}", pad_fn(hours), pad_fn(minutes % 60))
}

/*
 * Serde Workaround
 */

pub fn sender_sentinel() -> AgentStreamSender {
    AgentStreamSender::sentinel()
}

pub fn sender_sentinel_arc() -> Arc<AgentStreamSender> {
    Arc::new(AgentStreamSender::sentinel())
}

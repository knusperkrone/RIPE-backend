use crate::AgentMessage;

/*
 * conventions
 */

pub const CMD_ACTIVE: i32 = 1;
pub const CMD_INACTIVE: i32 = 0;

pub const DAY_MS: u32 = 86_400_000;

/*
 * helper
 */

pub fn send_payload(
    logger: &slog::Logger,
    sender: &tokio::sync::mpsc::Sender<AgentMessage>,
    payload: AgentMessage,
) {
    if let Err(e) = sender.try_send(payload) {
        crit!(logger, "Failed sending {}", e);
    }
}

pub fn sleep(runtime: &tokio::runtime::Handle, duration: std::time::Duration) {
    if tokio::runtime::Handle::try_current().is_ok() {
        runtime.block_on(tokio::time::sleep(duration));
    } else {
        let _guard = runtime.enter();
        runtime.block_on(tokio::time::sleep(duration));
    }
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

pub fn logger_sentinel() -> slog::Logger {
    slog::Logger::root(slog::Discard, o!("" => ""))
}

pub fn sender_sentinel() -> tokio::sync::mpsc::Sender<AgentMessage> {
    let (sentinel, _) = tokio::sync::mpsc::channel(1);
    sentinel
}

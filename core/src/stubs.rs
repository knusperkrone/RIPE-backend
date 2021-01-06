use crate::AgentMessage;

/*
 * conventions
 */

pub const CMD_ACTIVE: i32 = 1;
pub const CMD_INACTIVE: i32 = 0;


/*
 * helper
 */

pub fn send_payload(
    logger: &slog::Logger,
    sender: &tokio::sync::mpsc::Sender<AgentMessage>,
    payload: AgentMessage,
) {
    let task_logger = logger.clone();
    let mut task_sender = sender.clone();
    if let Err(e) = task_sender.try_send(payload) {
        crit!(task_logger, "Failed sending {}", e);
    }
}

pub fn secs_to_hr(time_ms: i32) -> String {
    let seconds = time_ms / 1000;
    let minutes = seconds / 60; 
    let hours = minutes / 60;

    let pad_fn = |x| {
        return if x < 10 {
            format!("0{}", x)
        } else {
            format!("{}", x)
        };
    };
    return format!("{}:{}", pad_fn(hours), pad_fn(minutes % 60));
}

/*
 * task stubs
 */

pub async fn task_sleep(nanos: u64) {
    // Workaround until tokio 1 in actix-rt
    std::thread::sleep(std::time::Duration::from_nanos(nanos));
}

pub async fn delay_task_for(duration: std::time::Duration) {
    let until = std::time::Instant::now() + duration;
    while std::time::Instant::now() < until {
        task_sleep(0).await
    }
}

/*
 * Serde Workaround
 */

pub fn logger_sentinel() -> slog::Logger {
    let sentinel = slog::Logger::root(slog::Discard, o!("" => ""));
    sentinel
}

pub fn sender_sentinel() -> tokio::sync::mpsc::Sender<AgentMessage> {
    let (sentinel, _) = tokio::sync::mpsc::channel(1);
    sentinel
}

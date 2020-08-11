use crate::AgentMessage;

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

/*
 * Task stubs
 */

pub async fn task_sleep(nanos: u64) {
    tokio::task::yield_now().await;
    std::thread::sleep(std::time::Duration::from_nanos(nanos));
    //tokio::time::delay_for(std::time::Duration::from_nanos(nanos)).await;
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

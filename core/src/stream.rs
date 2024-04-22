use crate::AgentMessage;
use tokio::sync::mpsc::{channel, Receiver, Sender};

#[derive(Debug)]
pub struct AgentStreamReceiver {
    _logger: slog::Logger,
    proxy: Receiver<AgentMessage>,
    last_command: Option<i32>,
}

#[derive(Debug)]
pub struct AgentStreamSender {
    proxy: Option<(slog::Logger, Sender<AgentMessage>)>,
}

#[derive(Debug)]
pub struct AgentStream {
    pub sender: AgentStreamSender,
    pub receiver: AgentStreamReceiver,
}

impl AgentStream {
    pub fn new(logger: slog::Logger) -> Self {
        let (sender, receiver) = channel::<AgentMessage>(16);

        Self {
            sender: AgentStreamSender {
                proxy: Some((logger.clone(), sender)),
            },
            receiver: AgentStreamReceiver {
                _logger: logger.clone(),
                last_command: None,
                proxy: receiver,
            },
        }
    }
}

impl AgentStreamSender {
    pub fn sentinel() -> Self {
        Self { proxy: None }
    }

    pub fn send(&self, msg: AgentMessage) {
        if let Some((logger, sender)) = &self.proxy {
            if let Err(e) = sender.try_send(msg) {
                error!(logger, "Failed to send message to Sender {}", e);
            }
        } else {
            panic!("Tried to send message from a sentinel");
        }
    }
}

impl AgentStreamReceiver {
    pub async fn recv(&mut self) -> Result<Option<AgentMessage>, std::io::Error> {
        match self.proxy.recv().await {
            Some(msg) => {
                if let Some(last_command) = self.last_command {
                    if let AgentMessage::Command(cmd) = msg {
                        if cmd == last_command {
                            return Ok(None);
                        }
                        self.last_command = Some(cmd);
                    }
                }

                Ok(Some(msg))
            }
            None => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Channel closed",
            )),
        }
    }
}

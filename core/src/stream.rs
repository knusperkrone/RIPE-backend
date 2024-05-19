use crate::AgentMessage;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tracing::{debug, error};

#[derive(Debug)]
pub struct AgentStreamReceiver {
    proxy: Receiver<AgentMessage>,
    last_command: Option<i32>,
}

#[derive(Debug)]
pub struct AgentStreamSender {
    proxy: Option<Sender<AgentMessage>>,
}

#[derive(Debug)]
pub struct AgentStream {
    pub sender: AgentStreamSender,
    pub receiver: AgentStreamReceiver,
}

impl Default for AgentStream {
    fn default() -> Self {
        let (sender, receiver) = channel::<AgentMessage>(16);

        Self {
            sender: AgentStreamSender {
                proxy: Some(sender),
            },
            receiver: AgentStreamReceiver {
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
        debug!("Sending message: {:?}", msg);
        if let Some(sender) = &self.proxy {
            if let Err(e) = sender.try_send(msg) {
                error!("Failed to send message to Sender {}", e);
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
                if let AgentMessage::Command(cmd) = msg {
                    match self.last_command {
                        Some(last_command) => {
                            if cmd == last_command {
                                return Ok(None);
                            }
                            self.last_command = Some(cmd);
                        }
                        None => {
                            self.last_command = Some(cmd);
                        }
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

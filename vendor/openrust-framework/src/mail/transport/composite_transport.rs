use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::mail::{MailFuture, MailResult, Message, SentMessage, transport};

use super::Transport;

pub(in crate::mail) struct CompositeTransport {
    transports: Vec<Arc<dyn Transport>>,
    retry_after: Duration,
    round_robin: bool,
    state: Mutex<State>,
}

struct State {
    next: usize,
    failed_at: Vec<Option<Instant>>,
}

impl CompositeTransport {
    pub(in crate::mail) fn new(
        transports: Vec<Arc<dyn Transport>>,
        retry_after: Duration,
        round_robin: bool,
    ) -> Self {
        let failed_at = vec![None; transports.len()];
        Self {
            transports,
            retry_after,
            round_robin,
            state: Mutex::new(State {
                next: if round_robin {
                    fastrand::usize(..failed_at.len())
                } else {
                    0
                },
                failed_at,
            }),
        }
    }
}

impl Transport for CompositeTransport {
    fn send<'a>(&'a self, message: &'a Message) -> MailFuture<'a, MailResult<SentMessage>> {
        Box::pin(async move {
            let count = self.transports.len();
            let mut last_error = None;
            let mut tried = vec![false; count];
            loop {
                let index = {
                    let mut state = self.state.lock().expect("mail transport lock poisoned");
                    let selected = (state.next..count).chain(0..state.next).find(|&index| {
                        !tried[index]
                            && state.failed_at[index]
                                .is_none_or(|failed| failed.elapsed() >= self.retry_after)
                    });
                    if let Some(index) = selected.filter(|_| self.round_robin) {
                        state.next = (index + 1) % count;
                    }
                    selected
                };
                let Some(index) = index else { break };
                tried[index] = true;
                match self.transports[index].send(message).await {
                    Ok(sent) => {
                        if !self.round_robin {
                            self.state
                                .lock()
                                .expect("mail transport lock poisoned")
                                .next = index;
                        }
                        return Ok(sent);
                    }
                    Err(error) => {
                        self.state
                            .lock()
                            .expect("mail transport lock poisoned")
                            .failed_at[index] = Some(Instant::now());
                        last_error = Some(error);
                    }
                }
            }
            Err(last_error
                .unwrap_or_else(|| transport("All configured mail transports are cooling down.")))
        })
    }
}

use super::LogResult;
use serde_json::Value;
use std::{io, sync::mpsc, thread, time::Duration};

type Delivery = (Value, mpsc::SyncSender<LogResult<()>>);

/// 独立线程复用现有HTTP客户端，避免在当前异步运行时内嵌套创建或销毁运行时。
/// 调用仍同步等待传输结果，保持日志契约。
pub(super) struct Webhook {
    sender: Option<mpsc::Sender<Delivery>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Webhook {
    pub(super) fn new(url: reqwest::Url, timeout: Duration) -> LogResult<Self> {
        let (sender, receiver) = mpsc::channel::<Delivery>();
        let (ready, initialized) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("log-webhook".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                let client = reqwest::Client::builder()
                    .timeout(timeout)
                    .redirect(reqwest::redirect::Policy::none())
                    .build();
                let (runtime, client) = match (runtime, client) {
                    (Ok(runtime), Ok(client)) => (runtime, client),
                    _ => {
                        let _ = ready.send(false);
                        return;
                    }
                };
                if ready.send(true).is_err() {
                    return;
                }
                while let Ok((payload, completed)) = receiver.recv() {
                    let result = runtime.block_on(async {
                        for attempt in 0..5 {
                            let delivery = async {
                                let response =
                                    client.post(url.clone()).json(&payload).send().await?;
                                // 传输成功即完成；HTTP错误状态不改成传输异常，保持外部处理器契约。
                                response.bytes().await?;
                                Ok::<(), reqwest::Error>(())
                            }
                            .await;
                            match delivery {
                                Ok(()) => return Ok(()),
                                Err(error) => {
                                    let retry = error.is_connect()
                                        || error.is_timeout()
                                        || error.is_body()
                                        || error.is_request();
                                    if !retry || attempt == 4 {
                                        return Err(io::Error::other(error.without_url()).into());
                                    }
                                }
                            }
                        }
                        unreachable!("发送循环始终返回结果")
                    });
                    let _ = completed.send(result);
                }
            })?;
        if initialized.recv().unwrap_or(false) {
            Ok(Self {
                sender: Some(sender),
                worker: Some(worker),
            })
        } else {
            drop(sender);
            let _ = worker.join();
            Err(io::Error::other("无法初始化日志HTTP客户端。").into())
        }
    }
    pub(super) fn send(&self, payload: Value) -> LogResult<()> {
        let (completed, result) = mpsc::sync_channel(1);
        self.sender
            .as_ref()
            .expect("活动日志发送器保留通道")
            .send((payload, completed))
            .map_err(|_| io::Error::other("日志发送线程已结束。"))?;
        result
            .recv()
            .map_err(|_| io::Error::other("未收到日志传输结果。"))?
    }
}
impl Drop for Webhook {
    fn drop(&mut self) {
        drop(self.sender.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

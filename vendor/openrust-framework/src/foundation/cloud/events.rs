use serde_json::Value;
use std::{future::Future, io, pin::Pin, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    sync::Mutex,
};

type Socket = Box<dyn AsyncWrite + Unpin + Send>;
type Connecting<'a> = Pin<Box<dyn Future<Output = io::Result<Socket>> + Send + 'a>>;
/// 用于注入真实或受控连接；工厂属于服务实例，不影响其他应用。
pub type SocketFactory = Arc<dyn for<'a> Fn(&'a str) -> Connecting<'a> + Send + Sync>;
/// 托管事件采用逐行JSON；遥测故障返回false，不阻断业务操作。
pub struct Events {
    address: String,
    socket: Mutex<Option<Socket>>,
    factory: SocketFactory,
}
impl Events {
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            socket: Mutex::new(None),
            factory: Arc::new(|address| Box::pin(connect(address))),
        }
    }
    pub fn with_socket_factory(mut self, factory: SocketFactory) -> Self {
        self.factory = factory;
        self
    }
    pub async fn emit(&self, payload: &Value) -> bool {
        if payload.as_object().is_some_and(|p| p.is_empty())
            || payload.as_array().is_some_and(|p| p.is_empty())
        {
            return true;
        }
        self.emit_many(std::slice::from_ref(payload)).await
    }
    pub async fn emit_many(&self, payloads: &[Value]) -> bool {
        if payloads.is_empty() {
            return true;
        }
        let mut payload = Vec::new();
        for value in payloads {
            if !value.is_object() && !value.is_array() {
                return false;
            }
            if serde_json::to_writer(&mut payload, value).is_err() {
                return false;
            }
            payload.push(b'\n');
        }
        let mut retained = self.socket.lock().await;
        // 写入中的连接由本次Future持有；取消时直接关闭，避免下次拼接半行JSON。
        let mut socket = retained.take();
        if socket.is_none() {
            match tokio::time::timeout(Duration::from_secs(2), (self.factory)(&self.address)).await
            {
                Ok(Ok(connected)) => socket = Some(connected),
                _ => return false,
            }
        }
        let mut cursor = 0;
        let mut zero_writes = 0;
        while cursor < payload.len() {
            let result = tokio::time::timeout(
                Duration::from_secs(2),
                socket.as_mut().expect("已连接").write(&payload[cursor..]),
            )
            .await;
            match result {
                Ok(Ok(0)) => {
                    zero_writes += 1;
                    if zero_writes >= 5 {
                        return false;
                    }
                }
                Ok(Ok(written)) => cursor += written,
                _ => return false,
            }
        }
        *retained = socket;
        true
    }
    pub async fn disconnect(&self) {
        *self.socket.lock().await = None;
    }
}
async fn connect(address: &str) -> io::Result<Socket> {
    if let Some(address) = address.strip_prefix("tcp://") {
        return Ok(Box::new(TcpStream::connect(address).await?));
    }
    #[cfg(unix)]
    if let Some(address) = address.strip_prefix("unix://") {
        return Ok(Box::new(tokio::net::UnixStream::connect(address).await?));
    }
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "不支持的托管事件套接字地址",
    ))
}

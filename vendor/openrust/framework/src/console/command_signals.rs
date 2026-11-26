use std::{
    future::{Future, poll_fn},
    io,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use super::ConsoleError;

/// 当前进程可供 Artisan 命令订阅的操作系统信号。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandSignal {
    Interrupt,
    Terminate,
    Quit,
    Hangup,
    Break,
    /// Unix 原生信号编号。
    Raw(i32),
}

// 根命令轮询 OS 订阅，嵌套命令共享同一组订阅并拥有自己的注册作用域。
tokio::task_local! {
    static CURRENT: Scope;
}

type Callback = Arc<dyn Fn(CommandSignal) + Send + Sync>;
type Event = Vec<(CommandSignal, Callback)>;

struct Scope {
    id: Arc<()>,
    registry: Registry,
}

type Registry = Arc<Mutex<Vec<Subscription>>>;

struct Subscription {
    number: i32,
    receiver: Receiver,
    handlers: Vec<Handler>,
}

struct Handler {
    owner: Arc<()>,
    signal: CommandSignal,
    callback: Callback,
}

impl Scope {
    fn clear(&self) {
        let mut subscriptions = self.registry.lock().expect("信号注册表未在回调中持锁");
        subscriptions.retain_mut(|subscription| {
            subscription
                .handlers
                .retain(|handler| !Arc::ptr_eq(&handler.owner, &self.id));
            !subscription.handlers.is_empty()
        });
    }

    fn register(
        &self,
        signals: impl IntoIterator<Item = CommandSignal>,
        callback: Callback,
    ) -> Result<(), ConsoleError> {
        let signals = signals
            .into_iter()
            .map(|signal| signal.number().map(|number| (number, signal)))
            .collect::<io::Result<Vec<_>>>()
            .map_err(ConsoleError::from_source)?;
        let mut subscriptions = self.registry.lock().expect("信号注册表未在回调中持锁");
        let mut additions = Vec::new();
        for &(number, _) in &signals {
            if subscriptions
                .iter()
                .chain(&additions)
                .all(|entry| entry.number != number)
            {
                additions.push(Subscription {
                    number,
                    receiver: Receiver::new(number).map_err(ConsoleError::from_source)?,
                    handlers: Vec::new(),
                });
            }
        }
        subscriptions.extend(additions);
        for (number, signal) in signals {
            subscriptions
                .iter_mut()
                .find(|entry| entry.number == number)
                .expect("请求订阅已完成")
                .handlers
                .push(Handler {
                    owner: self.id.clone(),
                    signal,
                    callback: callback.clone(),
                });
        }
        Ok(())
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        self.clear();
    }
}

pub(super) fn trap(
    signals: impl IntoIterator<Item = CommandSignal>,
    callback: impl Fn(CommandSignal) + Send + Sync + 'static,
) -> Result<(), ConsoleError> {
    CURRENT
        .try_with(|scope| scope.register(signals, Arc::new(callback)))
        .map_err(|_| ConsoleError::new("只能在运行中的命令内注册信号处理器。"))?
}

pub(super) fn untrap() {
    let _ = CURRENT.try_with(Scope::clear);
}

pub(super) async fn scope<T>(future: impl Future<Output = T>) -> T {
    let id = Arc::new(());
    if let Ok(registry) = CURRENT.try_with(|scope| scope.registry.clone()) {
        return CURRENT.scope(Scope { id, registry }, future).await;
    }

    let registry = Registry::default();
    let command = CURRENT.scope(
        Scope {
            id,
            registry: registry.clone(),
        },
        future,
    );
    let mut command = std::pin::pin!(command);
    loop {
        tokio::select! {
            biased;
            result = &mut command => return result,
            event = poll_fn(|cx| poll_event(&registry, cx)) => {
                for (signal, callback) in event {
                    callback(signal);
                }
            }
        }
    }
}

fn poll_event(registry: &Registry, cx: &mut Context<'_>) -> Poll<Event> {
    let mut subscriptions = registry.lock().expect("信号注册表未在回调中持锁");
    for subscription in subscriptions.iter_mut() {
        if let Poll::Ready(Some(())) = subscription.receiver.poll_recv(cx) {
            return Poll::Ready(
                subscription
                    .handlers
                    .iter()
                    .rev()
                    .map(|handler| (handler.signal, handler.callback.clone()))
                    .collect(),
            );
        }
    }
    Poll::Pending
}

impl CommandSignal {
    fn number(self) -> io::Result<i32> {
        #[cfg(unix)]
        match self {
            Self::Interrupt => return Ok(libc::SIGINT),
            Self::Terminate => return Ok(libc::SIGTERM),
            Self::Quit => return Ok(libc::SIGQUIT),
            Self::Hangup => return Ok(libc::SIGHUP),
            Self::Raw(number) => return Ok(number),
            Self::Break => {}
        }
        #[cfg(windows)]
        match self {
            Self::Interrupt => return Ok(0),
            Self::Break => return Ok(1),
            _ => {}
        }
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("当前系统不支持命令信号 {self:?}。"),
        ))
    }
}

#[cfg(unix)]
struct Receiver(tokio::signal::unix::Signal);

#[cfg(unix)]
impl Receiver {
    fn new(number: i32) -> io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        signal(SignalKind::from_raw(number)).map(Self)
    }

    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<()>> {
        self.0.poll_recv(cx)
    }
}

#[cfg(windows)]
enum Receiver {
    Interrupt(tokio::signal::windows::CtrlC),
    Break(tokio::signal::windows::CtrlBreak),
}

#[cfg(windows)]
impl Receiver {
    fn new(number: i32) -> io::Result<Self> {
        use tokio::signal::windows::{ctrl_break, ctrl_c};
        match number {
            0 => ctrl_c().map(Self::Interrupt),
            1 => ctrl_break().map(Self::Break),
            _ => unreachable!("已校验的 Windows 信号"),
        }
    }

    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<()>> {
        match self {
            Self::Interrupt(signal) => signal.poll_recv(cx),
            Self::Break(signal) => signal.poll_recv(cx),
        }
    }
}

#[cfg(not(any(unix, windows)))]
enum Receiver {}

#[cfg(not(any(unix, windows)))]
impl Receiver {
    fn new(_: i32) -> io::Result<Self> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn poll_recv(&mut self, _: &mut Context<'_>) -> Poll<Option<()>> {
        match *self {}
    }
}

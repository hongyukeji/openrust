//! 原生数据库 `Session` 的真实 `Cookie WebSocket` 握手与现代因子撤销回归。
use super::*;
use futures_util::{SinkExt as _, StreamExt as _};
use openrust::session::{SessionStoreHandle, SessionStoreSemantics};
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest as _},
};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
const BUDGET: Duration = Duration::from_secs(5);

fn socket_failure() -> Error {
    // 不附原始握手请求或响应，避免Cookie进入失败诊断。
    Error::custom(
        "fortify_native_socket_contract",
        "原生Session的Fortify WebSocket回归失败",
    )
}
#[track_caller]
fn require(condition: bool) -> Result<()> {
    if condition {
        Ok(())
    } else {
        eprintln!(
            "Fortify Native socket fixture failed at line={}",
            std::panic::Location::caller().line()
        );
        Err(socket_failure())
    }
}

async fn factor_browser(fixture: &TestApp<RootApplication>) -> Result<Browser<'_>> {
    let mut browser = Browser::new(fixture);
    browser.refresh_csrf().await?;
    require(login(&mut browser, "other", false).await?.status == StatusCode::OK)?;
    require(
        browser
            .post("/user/confirm-password", json!({"password":PASSWORD}))
            .await?
            .status
            == StatusCode::CREATED,
    )?;
    require(
        browser
            .post("/user/two-factor-authentication", json!({}))
            .await?
            .status
            == StatusCode::OK,
    )?;
    let reply = browser.get("/user/two-factor-secret-key").await?;
    require(reply.status == StatusCode::OK)?;
    let secret = reply.json()?["secretKey"]
        .as_str()
        .ok_or_else(socket_failure)?
        .to_owned();
    require(
        browser
            .post(
                "/user/confirmed-two-factor-authentication",
                json!({"code":code(&secret)?}),
            )
            .await?
            .status
            == StatusCode::OK,
    )?;
    let state = browser.state().await?;
    require(state["id"] == 44 && state["factor_verified"] == true)?;
    Ok(browser)
}

struct Listener {
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}
impl Listener {
    async fn shutdown(mut self) -> Result<()> {
        if let Some(stop) = self.stop.take() {
            let _sent = stop.send(());
        }
        match tokio::time::timeout(BUDGET, &mut self.task).await {
            Ok(result) => result.map_err(Error::internal)?.map_err(Error::internal),
            Err(error) => {
                self.task.abort();
                let _retired = self.task.await;
                Err(Error::internal(error))
            }
        }
    }
}
async fn listen(fixture: &TestApp<RootApplication>) -> Result<(SocketAddr, Listener)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(Error::internal)?;
    let address = listener.local_addr().map_err(Error::internal)?;
    let router = openrust::foundation::http::build_router(fixture.app()?)?;
    let (stop, stopped) = oneshot::channel();
    let task = tokio::spawn(async move {
        openrust::http::axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _stopped = stopped.await;
        })
        .await
    });
    Ok((
        address,
        Listener {
            stop: Some(stop),
            task,
        },
    ))
}

async fn next_business(socket: &mut Socket, expected: &Value) -> Result<()> {
    tokio::time::timeout(BUDGET, async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    let value: Value =
                        serde_json::from_str(text.as_str()).map_err(Error::internal)?;
                    return require(&value == expected);
                }
                Some(Ok(Message::Ping(payload))) => socket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|_| socket_failure())?,
                Some(Ok(Message::Pong(_))) => {}
                _ => return Err(socket_failure()),
            }
        }
    })
    .await
    .map_err(Error::internal)?
}
async fn revoked_connection_closes(socket: &mut Socket) -> Result<()> {
    tokio::time::timeout(BUDGET, async {
        loop {
            match socket.next().await {
                None | Some(Err(_) | Ok(Message::Close(_))) => return Ok(()),
                Some(Ok(Message::Ping(payload))) => {
                    // 先前已排队的心跳可能仍在网络中；回Pong会触发下一次输入前重验。
                    if socket.send(Message::Pong(payload)).await.is_err() {
                        return Ok(());
                    }
                }
                Some(Ok(Message::Pong(_))) => {}
                // 撤销后的业务发送必须被validator抑制，不能只要求最终关闭。
                Some(Ok(_)) => return Err(socket_failure()),
            }
        }
    })
    .await
    .map_err(Error::internal)?
}

async fn socket_flow(
    browser: &mut Browser<'_>,
    address: SocketAddr,
    article_id: i64,
) -> Result<()> {
    let origin = url::Url::parse(&browser.app.app()?.config().app.url)
        .map_err(Error::internal)?
        .origin()
        .ascii_serialization();
    let allowed = browser
        .app
        .app()?
        .config_repository()
        .section::<Vec<String>>("realtime.allowed_origins")?;
    require(allowed.iter().any(|candidate| candidate == &origin))?;
    let mut request = format!("ws://{address}/api/example/articles/{article_id}/browser-socket")
        .into_client_request()
        .map_err(|_| socket_failure())?;
    request
        .headers_mut()
        .insert(header::HOST, "localhost".parse().map_err(Error::internal)?);
    request
        .headers_mut()
        .insert(header::ORIGIN, origin.parse().map_err(Error::internal)?);
    let cookies = browser
        .cookies
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    require(!cookies.is_empty())?;
    request
        .headers_mut()
        .insert(header::COOKIE, cookies.parse().map_err(Error::internal)?);
    require(!request.headers().contains_key(header::AUTHORIZATION))?;
    let (mut socket, reply) = tokio::time::timeout(BUDGET, connect_async(request))
        .await
        .map_err(Error::internal)?
        .map_err(|_| socket_failure())?;
    let outcome: Result<()> = async {
        require(reply.status() == StatusCode::SWITCHING_PROTOCOLS)?;
        let realtime = browser
            .app
            .app()?
            .state()
            .resolve::<openrust::realtime::Realtime>()
            .await?;
        let channel = format!("example.article.{article_id}");
        let before = json!({"event":"fortify.native.before","article_id":article_id});
        require(
            realtime
                .send_to_user(44, &channel, &before, Duration::from_secs(30))
                .await?
                == 1,
        )?;
        next_business(&mut socket, &before).await?;
        let disabled = browser
            .send(
                Method::DELETE,
                "/user/two-factor-authentication",
                Some(json!({})),
                "application/json",
                true,
            )
            .await?;
        require(disabled.status == StatusCode::OK)?;
        require(disabled.json()? == json!(""))?;
        let after = json!({"event":"fortify.native.after","article_id":article_id});
        // 即使连接已因后台心跳先关闭，也继续走真实发送与输入路径，不放宽业务帧断言。
        let _queued = realtime
            .send_to_user(44, &channel, &after, Duration::from_secs(30))
            .await?;
        let _input = socket.send(Message::Ping(Vec::new().into())).await;
        revoked_connection_closes(&mut socket).await
    }
    .await;
    // 对端已关闭时close可返回AlreadyClosed；drop仍释放TCP，且不能覆盖原失败。
    let _closed = tokio::time::timeout(BUDGET, socket.close(None)).await;
    drop(socket);
    outcome
}

pub(super) async fn exercise(fixture: &TestApp<RootApplication>) -> Result<()> {
    let store = fixture
        .app()?
        .state()
        .resolve::<SessionStoreHandle>()
        .await?;
    assert_eq!(
        store.store().semantics(),
        SessionStoreSemantics::NativeDatabase
    );
    let mut browser = factor_browser(fixture).await?;
    let db = fixture.app()?.state().resolve::<Db>().await?;
    let article_id = 7002_i64;
    db.table("example_articles")?
        .insert(vec![
            ("id", article_id.into()),
            ("owner_id", 44_i64.into()),
            ("title", "Fortify Native Session socket".into()),
            ("body", "factor revocation fixture".into()),
        ])
        .await?;
    let (address, server) = listen(fixture).await?;
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        socket_flow(&mut browser, address, article_id),
    )
    .await
    .map_err(Error::internal)
    .and_then(std::convert::identity);
    let stopped = server.shutdown().await;
    outcome?;
    stopped
}

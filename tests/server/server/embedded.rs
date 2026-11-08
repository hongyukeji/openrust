//! 验证默认命令确实消费真实 Redis 队列，显式 `serve` 只提供 HTTP。

use super::{ChildGuard, Command, Duration, Stdio, TcpStream, TestResult, request_status};
use openrust::tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    time::{sleep, timeout},
};
use redis::aio::MultiplexedConnection;

#[test]
#[ignore = "需要显式 OPENRUST_TEST_REDIS_URL"]
fn default_start_consumes_the_application_queue_and_serve_leaves_it_pending() -> TestResult {
    openrust::tokio::runtime::Runtime::new()?.block_on(async {
        let url = std::env::var("OPENRUST_TEST_REDIS_URL")?;
        let root = tempfile::tempdir()?;
        let lang = root.path().join("lang");
        std::fs::create_dir_all(&lang)?;
        std::fs::write(
            lang.join("en.json"),
            include_bytes!("../../../lang/en.json"),
        )?;
        let module = root.path().join("modules/example");
        std::fs::create_dir_all(module.join("lang"))?;
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/modules/example/module.json"),
            module.join("module.json"),
        )?;
        let namespace = format!(
            "embedded-{}",
            root.path()
                .file_name()
                .ok_or("临时根缺少名称")?
                .to_string_lossy()
                .trim_start_matches('.')
        );
        let prefix = format!("{{openrust:{namespace}}}:");
        let mut redis = redis::Client::open(url.as_str())?
            .get_multiplexed_async_connection()
            .await?;
        let outcome = async {
            let (address, mut http) = launch(root.path(), &url, &namespace, &["serve"]).await?;
            enqueue(&address).await?;
            if count(&mut redis, &prefix, "jobs").await? != 1 {
                return Err("独立 Web 未保留派发的任务".into());
            }
            stop(&mut http).await?;
            if count(&mut redis, &prefix, "jobs").await? != 1 {
                return Err("独立 Web 停机前意外消费了任务".into());
            }

            let (address, mut app) = launch(root.path(), &url, &namespace, &[]).await?;
            enqueue(&address).await?;
            timeout(Duration::from_secs(8), async {
                while count(&mut redis, &prefix, "jobs").await? != 0 {
                    sleep(Duration::from_millis(25)).await;
                }
                Ok::<_, redis::RedisError>(())
            })
            .await??;
            if count(&mut redis, &prefix, "failed").await? != 0 {
                return Err("同进程 worker 留下失败任务".into());
            }
            stop(&mut app).await
        }
        .await;
        let cleanup = cleanup(&mut redis, &prefix).await;
        outcome.and(cleanup)
    })
}

async fn launch(
    root: &std::path::Path,
    url: &str,
    namespace: &str,
    args: &[&str],
) -> Result<(String, ChildGuard), Box<dyn std::error::Error>> {
    let reservation = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = reservation.local_addr()?;
    drop(reservation);
    let mut child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_server"))
            .args(args)
            .current_dir(root)
            .env_clear()
            .env("APP_ENV", "local")
            .env("SERVER_PORT", address.port().to_string())
            .env("REDIS_URL", url)
            .env("QUEUE_CONNECTION", "redis")
            .env("QUEUE_REDIS_URL", url)
            .env("QUEUE_REDIS_NAMESPACE", namespace)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let address = address.to_string();
    timeout(Duration::from_secs(8), async {
        loop {
            if let Some(status) = child.0.try_wait()? {
                return Err(std::io::Error::other(format!("应用提前退出: {status}")));
            }
            if request_status(&address, "/ready")
                .await
                .is_ok_and(|status| status == 200)
            {
                return Ok(());
            }
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await??;
    Ok((address, child))
}

async fn enqueue(address: &str) -> TestResult {
    let mut stream = TcpStream::connect(address).await?;
    stream.write_all(format!("POST /api/example/jobs/heartbeat HTTP/1.1\r\nHost: {address}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await?;
    let mut response = String::new();
    timeout(Duration::from_secs(5), stream.read_to_string(&mut response)).await??;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    Ok(())
}

async fn stop(child: &mut ChildGuard) -> TestResult {
    assert!(
        Command::new("kill")
            .args(["-TERM", &child.0.id().to_string()])
            .status()?
            .success()
    );
    timeout(Duration::from_secs(15), async {
        loop {
            if let Some(status) = child.0.try_wait()? {
                assert!(status.success(), "应用停机失败: {status}");
                return Ok::<_, std::io::Error>(());
            }
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await??;
    Ok(())
}

async fn count(
    redis: &mut MultiplexedConnection,
    prefix: &str,
    suffix: &str,
) -> redis::RedisResult<usize> {
    redis::cmd("HLEN")
        .arg(format!("{prefix}{suffix}"))
        .query_async(redis)
        .await
}

async fn cleanup(redis: &mut MultiplexedConnection, prefix: &str) -> TestResult {
    let mut cursor = 0_u64;
    let mut keys = Vec::new();
    loop {
        let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg(format!("{prefix}*"))
            .query_async(redis)
            .await?;
        keys.extend(batch);
        cursor = next;
        if cursor == 0 {
            break;
        }
    }
    if !keys.is_empty() {
        let _: usize = redis::cmd("DEL").arg(keys).query_async(redis).await?;
    }
    Ok(())
}

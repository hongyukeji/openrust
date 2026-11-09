//! 通过示例的地址诊断路由验证实际 HTTP listener 的入口信任边界。

use super::{ChildGuard, Command, Duration, Stdio, TestResult, request_status};
use openrust::tokio::time::{sleep, timeout};

#[tokio::test]
async fn deployment_switch_controls_forwarded_headers_at_the_listener() -> TestResult {
    let root = tempfile::tempdir()?;
    std::fs::create_dir_all(root.path().join("bootstrap/cache"))?;
    let lang = root.path().join("lang");
    std::fs::create_dir(&lang)?;
    std::fs::write(
        lang.join("en.json"),
        include_bytes!("../../../lang/en.json"),
    )?;
    let module = root.path().join("modules/example");
    std::fs::create_dir_all(module.join("lang"))?;
    std::fs::write(
        module.join("module.json"),
        include_bytes!("../../../modules/example/module.json"),
    )?;
    // 应用配置能读到此值，但可信入口只能由监听进程声明。
    std::fs::write(root.path().join(".env"), "OPENRUST_CLOUD_INGRESS=1\n")?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()?;

    for (label, environment, proxy_ranges, trusted) in [
        ("仅 dotenv 与请求头", None, None, false),
        ("旧进程变量", Some(("LARAVEL_CLOUD", "1")), None, false),
        (
            "非精确值",
            Some(("OPENRUST_CLOUD_INGRESS", "true")),
            None,
            false,
        ),
        (
            "受控入口",
            Some(("OPENRUST_CLOUD_INGRESS", "1")),
            None,
            true,
        ),
        (
            "显式空列表",
            Some(("OPENRUST_CLOUD_INGRESS", "1")),
            Some("[]"),
            false,
        ),
    ] {
        let reservation = std::net::TcpListener::bind("127.0.0.1:0")?;
        let address = reservation.local_addr()?;
        drop(reservation);
        let log = tempfile::NamedTempFile::new()?;
        let mut command = Command::new(env!("CARGO_BIN_EXE_app"));
        command
            .arg("serve")
            .current_dir(root.path())
            .env_clear()
            .env("APP_ENV", "local")
            .env("APP_SERVER_HOST", "127.0.0.1")
            .env("SERVER_PORT", address.port().to_string())
            .env(
                "APP_KEY",
                "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            )
            .stdout(Stdio::from(log.as_file().try_clone()?))
            .stderr(Stdio::from(log.as_file().try_clone()?));
        if let Some((key, value)) = environment {
            command.env(key, value);
        }
        if let Some(ranges) = proxy_ranges {
            command.env("APP_SERVER_TRUSTED_PROXY_RANGES", ranges);
        }
        let mut child = ChildGuard(command.spawn()?);
        timeout(Duration::from_secs(8), async {
            loop {
                if let Some(status) = child.0.try_wait()? {
                    let diagnostic = std::fs::read_to_string(log.path())?;
                    return Err(std::io::Error::other(format!(
                        "{label}启动失败: {status}; {diagnostic}"
                    )));
                }
                if request_status(&address.to_string(), "/ready")
                    .await
                    .is_ok_and(|status| status == 200)
                {
                    return Ok(());
                }
                sleep(Duration::from_millis(25)).await;
            }
        })
        .await??;
        assert_forwarded_headers(&client, address, trusted, label).await?;
    }
    Ok(())
}

async fn assert_forwarded_headers(
    client: &reqwest::Client,
    address: std::net::SocketAddr,
    trusted: bool,
    label: &str,
) -> TestResult {
    let response = client
        .get(format!("http://{address}/api/example/request-url"))
        .header("host", "internal.test")
        .header("openrust-cloud-ingress", "1")
        .header("x-forwarded-for", "198.51.100.8")
        .header("x-forwarded-host", "public.test")
        .header("x-forwarded-proto", "https")
        .header("x-forwarded-prefix", "/gateway")
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?;
    let data = &response["data"]["address"];
    let (ip, scheme, host, prefix) = if trusted {
        ("198.51.100.8", "https", "public.test", "/gateway")
    } else {
        ("127.0.0.1", "http", "internal.test", "")
    };
    assert_eq!(data["client_ip"], ip, "{label}");
    assert_eq!(data["scheme"], scheme, "{label}");
    assert_eq!(data["http_host"], host, "{label}");
    assert_eq!(data["base_url"], prefix, "{label}");
    Ok(())
}

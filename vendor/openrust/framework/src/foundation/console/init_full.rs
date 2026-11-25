//! 完整预设及实际进程自检。

use std::{collections::BTreeMap, path::Path, process::Stdio, time::Duration};

use tokio::process::Command as Process;

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    foundation::Application,
    redis::{RedisError, RedisResult, Value, connections::Connection},
    support::facades::{Artisan as ArtisanFacade, Config, Db, Queue, Redis},
};

pub(super) const PACKAGES: &[&str] = &[
    "activitylog",
    "cashier",
    "horizon",
    "localization-database",
    "new-relic",
    "pail",
    "pay",
    "pennant",
    "pulse",
    "scout",
    "settings",
    "telescope",
];

pub(super) const PROBE: &str = "init:verify";
const DRIVERS: &[(&str, &str)] = &[
    ("DB_CONNECTION", "mysql"),
    ("CACHE_STORE", "redis"),
    ("SESSION_DRIVER", "redis"),
    ("QUEUE_CONNECTION", "redis"),
    ("USER_STARTER_PROFILE", "full"),
];
const TIMEOUT: Duration = Duration::from_secs(30);

/// 复用进程环境的优先级，但不把进程注入的凭据保存到文件。
pub(super) fn configure(
    values: &mut BTreeMap<String, String>,
    existing: bool,
) -> Result<BTreeMap<String, Option<String>>, ConsoleError> {
    let mut overrides = BTreeMap::new();
    for &(key, expected) in DRIVERS {
        if !existing || !values.contains_key(key) {
            values.insert(key.into(), expected.into());
        }
    }
    if !existing {
        let url = format!(
            "http://localhost:{}",
            super::value(values, "APP_PORT", "8000")
        );
        values.insert("APP_URL".into(), url);
    }
    for (key, value) in std::env::vars_os() {
        let Some(key) = key.to_str() else { continue };
        if ![
            "APP_",
            "DB_",
            "REDIS_",
            "CACHE_",
            "SESSION_",
            "QUEUE_",
            "MAIL_",
            "SMS_",
            "USER_",
            "REVERB_",
            "BROADCAST_",
            "PASSPORT_",
            "FORTIFY_",
            "HORIZON_",
            "FILESYSTEM_",
        ]
        .iter()
        .any(|prefix| key.starts_with(prefix))
        {
            continue;
        }
        let value = value
            .into_string()
            .map_err(|_| ConsoleError::new(format!("{key} 必须是 UTF-8 字符串")))?;
        if key == "APP_KEY" && super::key_missing(&BTreeMap::from([(key.into(), value.clone())])) {
            return Err(ConsoleError::new(
                "进程 APP_KEY 为空，请取消该变量以允许生成密钥",
            ));
        }
        overrides.insert(key.into(), values.insert(key.into(), value));
    }
    let mut conflicts = Vec::new();
    for &(key, expected) in DRIVERS {
        let conflict = super::value(values, key, "") != expected
            || (existing
                && overrides
                    .get(key)
                    .and_then(Option::as_deref)
                    .is_some_and(|old| old != expected));
        if conflict {
            conflicts.push(format!("{key} 应为 {expected}"));
        }
    }
    if !conflicts.is_empty() {
        return Err(ConsoleError::new(format!(
            "完整预设与现有配置冲突：{}。未修改文件。",
            conflicts.join("、")
        )));
    }
    Ok(overrides)
}

pub(super) fn require_connections(values: &BTreeMap<String, String>) -> Result<(), ConsoleError> {
    let present = |key| {
        let value = super::value(values, key, "");
        !value.is_empty()
            && !value.eq_ignore_ascii_case("null")
            && !value.eq_ignore_ascii_case("(null)")
    };
    if !(present("DB_URL") || (present("DB_DATABASE") && present("DB_USERNAME"))) {
        return Err(ConsoleError::new(
            "请提供现有 MySQL 库的 DB_URL，或配置 DB_DATABASE 和 DB_USERNAME",
        ));
    }
    if !present("REDIS_URL") && !present("REDIS_HOST") {
        return Err(ConsoleError::new(
            "请为已有 Redis 配置 REDIS_URL 或 REDIS_HOST",
        ));
    }
    Ok(())
}

pub(super) async fn probe_redis(connection: &Connection) -> RedisResult<()> {
    let key = connection.key(&marker(&ulid::Ulid::new().to_string()));
    let arguments = [key.as_bytes(), b"ok", b"EX", b"60"];
    connection.command("SET", &arguments).await?;
    let result = connection.command("GET", &[key.as_bytes()]).await;
    connection.command("DEL", &[key.as_bytes()]).await?;
    if result? != Value::BulkString(b"ok".to_vec()) {
        return Err(RedisError::InvalidConfiguration);
    }
    Ok(())
}

fn marker(token: &str) -> String {
    format!("openrust:init:{token}")
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(ProbeCommand(CommandDefinition::new(
        clap::Command::new(PROBE)
            .hide(true)
            .arg(clap::Arg::new("operation").required(true).value_parser([
                "configuration",
                "enqueue",
                "complete",
                "check",
                "cleanup",
            ]))
            .arg(clap::Arg::new("token").required(true)),
    ))))
}

struct ProbeCommand(CommandDefinition);

impl Command for ProbeCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        _: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let matches = input.matches();
            let token = matches.get_one::<String>("token").expect("required token");
            let operation = matches
                .get_one::<String>("operation")
                .expect("required operation");
            if operation == "configuration" {
                for key in ["cache.default", "queue.default", "session.driver"] {
                    if Config::string(key).as_deref().ok() != Some("redis") {
                        return Err(ConsoleError::new(format!(
                            "应用生效配置 {key} 与 Redis 完整预设冲突"
                        )));
                    }
                }
                let connection = Db::connection(None)
                    .map_err(|_| ConsoleError::new("无法解析构建后应用的数据库连接"))?;
                if connection.get_driver_name() != "mysql"
                    || connection.get_database_name() != token
                {
                    return Err(ConsoleError::new(
                        "应用实际 MySQL 库与预检目标不一致，未执行迁移",
                    ));
                }
                return Ok(0);
            }
            if token.len() != 26 || token.parse::<ulid::Ulid>().is_err() {
                return Err(ConsoleError::new("初始化自检标识无效"));
            }
            if operation == "enqueue" {
                ArtisanFacade::queue(PROBE, &["complete".into(), token.into()])
                    .on_connection("redis")
                    .on_queue(marker(token))
                    .dispatch()
                    .await
                    .map_err(|_| ConsoleError::new("无法从初始化应用向 Redis 派发自检任务"))?;
                return Ok(0);
            }
            let connection = Redis::connection(None)
                .map_err(|_| ConsoleError::new("无法解析初始化自检的 Redis 连接"))?;
            let key = connection.key(&marker(token));
            let redis_error = |_| ConsoleError::new("无法读取或写入 Redis Worker 自检凭据");
            if operation == "complete" {
                connection
                    .command("SET", &[key.as_bytes(), b"ok", b"EX", b"60"])
                    .await
                    .map_err(redis_error)?;
            } else if operation == "cleanup" {
                let queue = Queue::connection(Some("redis"))
                    .map_err(|_| ConsoleError::new("无法清理初始化自检队列"))?;
                let clearable = queue
                    .clearable()
                    .ok_or_else(|| ConsoleError::new("Redis 自检队列无法清理"))?;
                clearable
                    .clear(&marker(token))
                    .await
                    .map_err(|_| ConsoleError::new("无法清理初始化自检队列"))?;
                connection
                    .command("DEL", &[key.as_bytes()])
                    .await
                    .map_err(redis_error)?;
            } else {
                let result = connection
                    .command("GET", &[key.as_bytes()])
                    .await
                    .map_err(redis_error)?;
                connection
                    .command("DEL", &[key.as_bytes()])
                    .await
                    .map_err(redis_error)?;
                if result != Value::BulkString(b"ok".to_vec()) {
                    return Err(ConsoleError::new("Redis Worker 未完成初始化自检任务"));
                }
            }
            Ok(0)
        })
    }
}

pub(super) async fn verify(
    executable: &Path,
    root: &Path,
    environment: Option<&str>,
) -> Result<(), ConsoleError> {
    let failure = |step: &str| {
        ConsoleError::new(format!(
            "{step}自检失败；已完成的初始化内容保留，修正后重新执行 init:full"
        ))
    };
    let process = || {
        let mut command = Process::new(executable);
        command
            .current_dir(root)
            .env("APP_BASE_PATH", root)
            .arg("artisan")
            .stdin(Stdio::null())
            .kill_on_drop(true);
        command
    };
    let temporary = root.join("target/tmp");
    tokio::fs::create_dir_all(&temporary)
        .await
        .map_err(|_| failure("临时目录"))?;
    let directory = tempfile::tempdir_in(temporary).map_err(|_| failure("临时目录"))?;
    let ready = directory.path().join("http-ready");
    let mut http = process();
    http.args([
        super::super::serve_command::CHILD_COMMAND,
        "127.0.0.1",
        "0",
        "0",
        "0",
    ])
    .arg(&ready)
    .args(environment)
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    let mut server = http.spawn().map_err(|_| failure("HTTP 启动"))?;
    let http_result = tokio::time::timeout(TIMEOUT, async {
        loop {
            if let Ok(port) = tokio::fs::read_to_string(&ready).await {
                let client = reqwest::Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(TIMEOUT)
                    .build()
                    .map_err(|_| failure("HTTP 客户端"))?;
                let response = client
                    .get(format!("http://127.0.0.1:{port}/up"))
                    .header("Accept", "application/json")
                    .send()
                    .await
                    .map_err(|_| failure("HTTP 请求"))?;
                let status = response.status();
                let healthy = response.json::<serde_json::Value>().await.ok();
                if status == reqwest::StatusCode::OK
                    && healthy
                        .as_ref()
                        .and_then(|body| body.get("status"))
                        .and_then(serde_json::Value::as_str)
                        == Some("up")
                {
                    return Ok(());
                }
                return Err(failure("HTTP /up"));
            }
            if server
                .try_wait()
                .map_err(|_| failure("HTTP 进程"))?
                .is_some()
            {
                return Err(failure("HTTP 启动"));
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await;
    drop(server.stdin.take());
    let stopped = tokio::time::timeout(TIMEOUT, server.wait()).await;
    http_result.map_err(|_| failure("HTTP 超时"))??;
    if !matches!(stopped, Ok(Ok(status)) if status.success()) {
        return Err(failure("HTTP 关停"));
    }

    let token = ulid::Ulid::new().to_string();
    let queue = format!("--queue={}", marker(&token));
    for (stage, args) in [
        ("Artisan", &["inspire"][..]),
        ("Redis 派发", &[PROBE, "enqueue", token.as_str()]),
        (
            "Redis Worker",
            &["queue:work", "redis", &queue, "--once", "--tries=1"],
        ),
        ("Redis Worker 结果", &[PROBE, "check", token.as_str()]),
    ] {
        let mut command = process();
        command
            .args(args)
            .arg("--no-interaction")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(environment) = environment {
            command.args(["--env", environment]);
        }
        if stage == "Redis Worker" {
            command.env("QUEUE_FAILED_DRIVER", "null");
        }
        match tokio::time::timeout(TIMEOUT, command.status()).await {
            Ok(Ok(status)) if status.success() => (),
            _ => {
                let mut cleanup = process();
                cleanup
                    .args([PROBE, "cleanup", &token, "--no-interaction"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                if let Some(environment) = environment {
                    cleanup.args(["--env", environment]);
                }
                drop(command);
                let _ = tokio::time::timeout(TIMEOUT, cleanup.status()).await;
                return Err(failure(stage));
            }
        }
    }
    Ok(())
}

//! 真实 Redis 上独立连接的 Session 共享、TTL、身份与有界读取回归。
//! 仅使用随机私有 namespace 和明确 ID；不扫描、清空或停止共享服务。
//! 两个 namespace、三个连接的证据不代表多节点或跨进程集群验收。
#![cfg(feature = "session-redis")]

use std::{future::Future, time::Duration};

use openrust::session::{
    DEFAULT_MAX_BYTES, RedisSessionStore, SessionId, SessionRecord, SessionStore, SessionWriteMode,
    SessionWriteOutcome,
};
use openrust::{Error, Result};
use serde_json::{Value, json};
use time::OffsetDateTime;

const IO_TIMEOUT: Duration = Duration::from_secs(2);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
const CLEANUP_TIMEOUT: Duration = Duration::from_millis(500);

fn failed() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "session_redis_test_failed",
        "Redis Session 测试失败，不包含服务地址或会话内容",
    )
}
fn ensure(condition: bool) -> Result<()> {
    if condition { Ok(()) } else { Err(failed()) }
}
async fn io<T>(future: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(IO_TIMEOUT, future)
        .await
        .map_err(|_| failed())?
}
async fn connect(url: &str, namespace: &str) -> Result<RedisSessionStore> {
    tokio::time::timeout(CONNECT_TIMEOUT, RedisSessionStore::connect(url, namespace))
        .await
        .map_err(|_| failed())?
}
fn record(id: SessionId, data: Value) -> SessionRecord {
    SessionRecord {
        id,
        data,
        expiry_date: OffsetDateTime::now_utc() + time::Duration::minutes(2),
    }
}
async fn load(
    store: &RedisSessionStore,
    record: &SessionRecord,
    maximum: usize,
) -> Result<Option<SessionRecord>> {
    io(store.load(&record.id, maximum, record.expiry_date)).await
}
async fn required(store: &RedisSessionStore, record: &SessionRecord) -> Result<SessionRecord> {
    load(store, record, DEFAULT_MAX_BYTES)
        .await?
        .ok_or_else(failed)
}
async fn write(
    store: &RedisSessionStore,
    record: &SessionRecord,
    mode: SessionWriteMode,
    expected: SessionWriteOutcome,
) -> Result<()> {
    ensure(io(store.write(record, mode, DEFAULT_MAX_BYTES)).await? == expected)
}

struct Fixture {
    first: RedisSessionStore,
    second: RedisSessionStore,
    isolated: RedisSessionStore,
    ids: [SessionId; 3],
}
impl Fixture {
    async fn connect(url: &str) -> Result<Self> {
        let namespace = format!("session_it_{}", SessionId::generate()?.as_str());
        // 每次公开 connect 都新建连接管理器；second 不是 first 的 clone。
        let first = connect(url, &namespace).await?;
        let second = connect(url, &namespace).await?;
        let isolated = connect(url, &format!("{namespace}_other")).await?;
        Ok(Self {
            first,
            second,
            isolated,
            ids: [
                SessionId::parse(&"a".repeat(40))?,
                SessionId::parse(&"A".repeat(40))?,
                SessionId::parse(&"t".repeat(40))?,
            ],
        })
    }

    async fn cleanup(&self) -> Result<()> {
        let mut clean = true;
        for store in [&self.first, &self.second, &self.isolated] {
            for id in &self.ids {
                // 即使一项失败也继续清理其他明确持有的键；绝不按通配符删 namespace。
                if !matches!(
                    tokio::time::timeout(CLEANUP_TIMEOUT, store.delete(id)).await,
                    Ok(Ok(()))
                ) {
                    clean = false;
                }
            }
        }
        ensure(clean)
    }
}

async fn shared_conflict_update_and_bounded_payload(fixture: &Fixture) -> Result<()> {
    let data: Value = serde_json::from_str(
        r#"{"z":"before\u0000after","a":{"second":2,"first":1},"empty":null}"#,
    )
    .map_err(|_| failed())?;
    let original = record(fixture.ids[0].clone(), data);
    write(
        &fixture.first,
        &original,
        SessionWriteMode::Insert,
        SessionWriteOutcome::Written,
    )
    .await?;
    ensure(io(fixture.first.gc(1)).await? == 0)?;
    ensure(io(fixture.second.gc(i64::MAX)).await? == 0)?;
    let shared = required(&fixture.second, &original).await?;
    ensure(shared.id == original.id && shared.data == original.data)?;
    // Value 等值不足以发现对象键顺序丢失，另比较按插入顺序序列化后的字符串。
    let shared_serialized = shared.data.to_string();
    let original_serialized = original.data.to_string();
    ensure(shared_serialized == original_serialized)?;
    ensure(
        shared_serialized == r#"{"z":"before\u0000after","a":{"second":2,"first":1},"empty":null}"#,
    )?;
    ensure(shared.data["z"] == "before\0after")?;
    ensure(
        shared.expiry_date <= original.expiry_date
            && shared.expiry_date > OffsetDateTime::now_utc(),
    )?;
    let mut collision = original.clone();
    collision.data = json!({"must_not_overwrite":true});
    collision.expiry_date += time::Duration::minutes(1);
    write(
        &fixture.second,
        &collision,
        SessionWriteMode::Insert,
        SessionWriteOutcome::Conflict,
    )
    .await?;
    let retained = required(&fixture.first, &original).await?;
    let retained_serialized = retained.data.to_string();
    ensure(
        retained_serialized == original_serialized && retained.expiry_date <= original.expiry_date,
    )?;
    for _ in 0..2 {
        write(
            &fixture.second,
            &original,
            SessionWriteMode::Update,
            SessionWriteOutcome::Written,
        )
        .await?;
    }
    let maximum = original.encode(DEFAULT_MAX_BYTES)?.len();
    ensure(load(&fixture.first, &original, maximum).await?.is_some())?;
    let error = load(
        &fixture.first,
        &original,
        maximum.checked_sub(1).ok_or_else(failed)?,
    )
    .await
    .err()
    .ok_or_else(failed)?;
    ensure(
        error.code() == "session_limits_exceeded"
            && error.status() == http::StatusCode::PAYLOAD_TOO_LARGE,
    )?;
    ensure(required(&fixture.second, &original).await?.data == original.data)
}

async fn case_sensitive_namespace_isolation_and_delete(fixture: &Fixture) -> Result<()> {
    let lower = record(fixture.ids[0].clone(), json!({"marker":"lower"}));
    write(
        &fixture.first,
        &lower,
        SessionWriteMode::Update,
        SessionWriteOutcome::Written,
    )
    .await?;
    let upper = record(fixture.ids[1].clone(), json!({"marker":"upper"}));
    write(
        &fixture.second,
        &upper,
        SessionWriteMode::Insert,
        SessionWriteOutcome::Written,
    )
    .await?;
    ensure(required(&fixture.second, &lower).await?.data == lower.data)?;
    ensure(required(&fixture.first, &upper).await?.data == upper.data)?;
    ensure(
        load(&fixture.isolated, &lower, DEFAULT_MAX_BYTES)
            .await?
            .is_none(),
    )?;
    let separate = record(lower.id.clone(), json!({"marker":"other namespace"}));
    write(
        &fixture.isolated,
        &separate,
        SessionWriteMode::Insert,
        SessionWriteOutcome::Written,
    )
    .await?;
    ensure(required(&fixture.second, &lower).await?.data == lower.data)?;
    ensure(required(&fixture.isolated, &separate).await?.data == separate.data)?;
    io(fixture.first.delete(&lower.id)).await?;
    io(fixture.second.delete(&lower.id)).await?;
    write(
        &fixture.second,
        &lower,
        SessionWriteMode::Update,
        SessionWriteOutcome::Missing,
    )
    .await?;
    ensure(
        load(&fixture.first, &lower, DEFAULT_MAX_BYTES)
            .await?
            .is_none(),
    )?;
    ensure(required(&fixture.second, &upper).await?.data == upper.data)?;
    ensure(required(&fixture.isolated, &separate).await?.data == separate.data)
}

async fn server_ttl_expiry_rejects_a_future_dated_late_update(fixture: &Fixture) -> Result<()> {
    let mut expiring = record(fixture.ids[2].clone(), json!({"marker":"must expire"}));
    expiring.expiry_date = OffsetDateTime::now_utc() + time::Duration::seconds(3);
    write(
        &fixture.first,
        &expiring,
        SessionWriteMode::Insert,
        SessionWriteOutcome::Written,
    )
    .await?;
    ensure(
        load(&fixture.second, &expiring, DEFAULT_MAX_BYTES)
            .await?
            .is_some(),
    )?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if load(&fixture.second, &expiring, DEFAULT_MAX_BYTES)
            .await?
            .is_none()
        {
            break;
        }
        ensure(tokio::time::Instant::now() < deadline)?;
        // 轮询真实服务的缺失结果；不是等待一个固定时长后假定 TTL 已到期。
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // 若仍使用过期 record，driver 可在本地直接返回 Missing，不能证明 Redis XX。
    let mut late = expiring.clone();
    late.expiry_date = OffsetDateTime::now_utc() + time::Duration::minutes(2);
    write(
        &fixture.first,
        &late,
        SessionWriteMode::Update,
        SessionWriteOutcome::Missing,
    )
    .await?;
    ensure(
        load(&fixture.second, &late, DEFAULT_MAX_BYTES)
            .await?
            .is_none(),
    )
}

#[tokio::test]
#[ignore = "需要真实 Redis 8 与 OPENRUST_TEST_SESSION_REDIS_URL；缺少服务配置将失败，使用 --ignored 显式执行"]
async fn redis_session_shared_atomic_writes_ttl_namespace_and_limits() -> Result<()> {
    let url = std::env::var("OPENRUST_TEST_SESSION_REDIS_URL").map_err(|_| {
        Error::custom(
            "session_redis_test_configuration_missing",
            "需要 OPENRUST_TEST_SESSION_REDIS_URL 才能执行真实 Redis Session 回归",
        )
    })?;
    let fixture = Fixture::connect(&url).await?;
    let outcome = tokio::time::timeout(CHECK_TIMEOUT, async {
        shared_conflict_update_and_bounded_payload(&fixture).await?;
        case_sensitive_namespace_isolation_and_delete(&fixture).await?;
        server_ttl_expiry_rejects_a_future_dated_late_update(&fixture).await
    })
    .await
    .map_err(|_| failed())
    .and_then(|result| result);
    // 业务、连接或资源断言均返回 Result；超时/失败后仍逐项回收本次精确 ID。
    let cleanup = fixture.cleanup().await;
    cleanup?;
    outcome
}

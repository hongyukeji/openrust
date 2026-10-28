//! 对主库和显式只读路由进行小规模、可重复的查询路径比较。

use std::time::{Duration, Instant};

use figment::{
    Figment,
    providers::{Format, Toml},
};
use openrust::{
    config::FrameworkConfig,
    database::{
        DatabaseServiceProvider, Db,
        sea_orm::{ConnectionTrait, Statement},
    },
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let primary_url = std::env::var("OPENRUST_TEST_DATABASE_URL")?;
    let replica_url = std::env::var("OPENRUST_TEST_READ_REPLICA_URL")?;
    let sample_count = std::env::var("OPENRUST_DB_ROUTE_BENCH_SAMPLES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|count| *count >= 100)
        .unwrap_or(500);
    let source = format!("[database]\nurl = {primary_url:?}\nread_replicas = [{replica_url:?}]\n");
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(Figment::new().merge(Toml::string(&source)));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder.provider(DatabaseServiceProvider);
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let db = app.state().resolve::<Db>().await?;
    let backend = db.backend()?;

    eprintln!(
        "OpenRust DB route smoke benchmark: backend={backend:?}, query=SELECT 1, concurrency=1, warmup=20, samples={sample_count}, pool_max_connections=10"
    );
    measure("primary", sample_count, || db.primary_connection()).await?;
    measure("read_replica", sample_count, || db.read_connection()).await?;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

async fn measure(
    label: &str,
    sample_count: usize,
    connection: impl Fn() -> openrust::Result<openrust::database::DatabaseConnection>,
) -> Result<(), Box<dyn std::error::Error>> {
    for _ in 0..20 {
        query_one(connection()?).await?;
    }
    let mut samples = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        let pool = connection()?;
        let started = Instant::now();
        query_one(pool).await?;
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    let total: Duration = samples.iter().copied().sum();
    let percentile = |percent: usize| samples[(samples.len() - 1) * percent / 100];
    let requests = samples.len().to_string().parse::<f64>()?;
    eprintln!(
        "route={label} requests_per_second={:.1} p50={:?} p95={:?} p99={:?}",
        requests / total.as_secs_f64(),
        percentile(50),
        percentile(95),
        percentile(99)
    );
    Ok(())
}

async fn query_one(
    connection: openrust::database::DatabaseConnection,
) -> Result<(), Box<dyn std::error::Error>> {
    let row = connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            "SELECT 1 AS value".to_owned(),
        ))
        .await?
        .ok_or("SELECT 1 returned no row")?;
    let value = row.try_get::<i32>("", "value")?;
    if value != 1 {
        return Err("SELECT 1 returned an unexpected value".into());
    }
    Ok(())
}

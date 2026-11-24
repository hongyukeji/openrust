use openrust::{
    contracts::foundation::Application as _,
    support::facades::{App, Config, Db, Facade, ParallelTesting, Schema},
    testing::TestApp,
};
use std::{io::Write, path::PathBuf};
fn record(event: &str) {
    let Ok(root) = std::env::var("OPENRUST_PARALLEL_FIXTURE_LOG") else {
        return;
    };
    let token = ParallelTesting::token().unwrap();
    let token = token.as_str().unwrap_or("serial");
    let path = PathBuf::from(root).join(format!("{token}.jsonl"));
    let row = serde_json::json!({"event":event,"token":token,"pid":std::process::id(),"cache_prefix":Config::get("cache.prefix").unwrap()});
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .unwrap();
    writeln!(file, "{row}").unwrap();
}
async fn scenario(fail: bool) {
    let mut app = TestApp::create(|b| b).unwrap();
    app.boot().await.unwrap();
    if let Ok(url) = std::env::var("OPENRUST_PARALLEL_DB_FIXTURE_URL") {
        app.app()
            .config()
            .unwrap()
            .set(
                "database",
                serde_json::json!({
                    "default":"fixture", "connections":{"fixture":{"url":url}}
                }),
            )
            .unwrap();
        app = app.with_parallel_database();
    }
    App::set_facade_application(Some(app.app()), async {
        ParallelTesting::set_up_process(|_, _| {
            Box::pin(async {
                record("process+");
                Ok(())
            })
        })
        .unwrap();
        ParallelTesting::tear_down_process(|_, _| {
            Box::pin(async {
                record("process-");
                Ok(())
            })
        })
        .unwrap();
        ParallelTesting::set_up_test_case(|_, _| {
            Box::pin(async {
                record("case+");
                Ok(())
            })
        })
        .unwrap();
        ParallelTesting::tear_down_test_case(|_, _| {
            Box::pin(async {
                record("case-");
                Ok(())
            })
        })
        .unwrap();
    })
    .await;
    app.run_parallel_test(move |case| {
        Box::pin(async move {
            record("body");
            if std::env::var_os("OPENRUST_PARALLEL_DB_FIXTURE_URL").is_some() {
                let existed = Schema::has_table("parallel_cli_marker").await?;
                if !existed {
                    Db::unprepared("create table parallel_cli_marker (id integer)").await?;
                    Db::unprepared("insert into parallel_cli_marker values (1)").await?;
                }
                let root = std::env::var("OPENRUST_PARALLEL_FIXTURE_LOG")?;
                let token = ParallelTesting::token()?;
                let token = token.as_str().unwrap();
                let row = serde_json::json!({"event":"database", "token":token,
                    "database": Db::connection(None)?.get_database_name(),
                    "existed":existed, "count":Db::connection(None)?.table("parallel_cli_marker").count("*").await?});
                let mut file = std::fs::OpenOptions::new().create(true).append(true)
                    .open(PathBuf::from(root).join(format!("{token}.jsonl")))?;
                writeln!(file, "{row}")?;
            }
            case.get("/missing").await?.assert_not_found();
            if let (Ok(root), Ok(count)) = (
                std::env::var("OPENRUST_PARALLEL_FIXTURE_LOG"),
                std::env::var("OPENRUST_PARALLEL_FIXTURE_BARRIER"),
            ) {
                let count: usize = count.parse()?;
                let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
                loop {
                    let files = std::fs::read_dir(&root)?.count();
                    if files >= count {
                        break;
                    }
                    assert!(tokio::time::Instant::now() < deadline, "并行进程未同时启动");
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }
            assert!(!fail, "验收中的预期失败");
            Ok(())
        })
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn success_first() {
    scenario(false).await;
}
#[tokio::test]
async fn success_second() {
    scenario(false).await;
}
#[tokio::test]
async fn controlled_failure() {
    scenario(std::env::var("OPENRUST_PARALLEL_FIXTURE_FAIL").is_ok_and(|v| v == "1")).await;
}

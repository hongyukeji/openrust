use std::{collections::BTreeMap, path::Path, time::Duration};

use openrust::{
    config::{ConfigError, Environment, Value},
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    queue::{Payload, QueueManager},
    redis::{RedisManager, Value as RedisValue},
    support::facades::{App, Route},
    testing::TestApp,
};
use openrust_horizon::{HorizonServiceProvider, Repository};
use serde_json::json;

fn config(base: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    let mut config: BTreeMap<String, Value> =
        serde_json::from_slice(&std::fs::read(base.join("cluster-config.json")).unwrap()).unwrap();
    config.insert(
        "cache".into(),
        json!({"default":"file","stores":{"file":{
            "driver":"file","path":base.join("cache"),
        }}}),
    );
    Ok(config)
}

fn queue_round_trip(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let manager = App::make::<QueueManager>().await.unwrap();
        let queue = manager.connection(None).unwrap();
        let began = tokio::time::Instant::now();
        assert!(
            queue
                .pop_within(None, Duration::from_millis(180))
                .await
                .unwrap()
                .is_none()
        );
        assert!(began.elapsed() >= Duration::from_millis(150));
        let payload = Payload::new("ClusterProbe", json!({"fixture":true})).unwrap();
        let id = queue.push(&payload, None).await.unwrap().unwrap();
        assert_eq!(queue.size(None).await.unwrap(), 1);
        let mut reservation = queue.pop(None).await.unwrap().unwrap();
        assert_eq!(reservation.get_job_id(), id);
        assert_eq!(reservation.payload().uuid(), payload.uuid());
        reservation.delete().await.unwrap();
        assert_eq!(queue.size(None).await.unwrap(), 0);
        Response::json(StatusCode::OK, id)
    })
}

#[tokio::test]
#[ignore = "需要隔离的 Redis Cluster 和单节点验收实例"]
async fn redis_cluster_metadata_and_queue_transport_use_public_horizon_consumers() {
    let ports = std::env::var("HORIZON_CLUSTER_TEST_PORTS").unwrap();
    let standalone = std::env::var("HORIZON_CLUSTER_TEST_STANDALONE_PORT").unwrap();
    let password = std::env::var("HORIZON_CLUSTER_TEST_PASSWORD").unwrap();
    let nodes: Vec<_> = ports
        .split(',')
        .map(|port| json!({"url": format!("redis://:{password}@127.0.0.1:{port}/0")}))
        .collect();
    assert_eq!(nodes.len(), 3);
    let standalone_url = format!("redis://:{password}@127.0.0.1:{standalone}/0");
    let run = uuid::Uuid::now_v7().simple().to_string();

    for (index, (metadata, transport, queue)) in [
        ("cluster", "standalone", "probe"),
        ("standalone", "cluster", "probe"),
        ("cluster", "cluster", "probe"),
        ("cluster", "cluster", "{explicit-probe}"),
        ("shared", "shared", "probe"),
    ]
    .into_iter()
    .enumerate()
    {
        let prefix = if index == 3 {
            format!("cluster[example]-{{{run}}}:")
        } else {
            format!("horizon-{run}-{index}:")
        };
        let global = format!("transport-{run}-{index}:");
        let settings = BTreeMap::from([
            ("app", json!({"env": "testing"})),
            (
                "horizon",
                json!({"use": metadata, "prefix": prefix, "environments":{"testing":{}}}),
            ),
            (
                "database",
                json!({"redis":{
                    "options":{"cluster":"redis","prefix":global},
                    "standalone":{"url":standalone_url},
                    "shared":{"url":standalone_url},
                    "clusters":{"cluster": nodes, "shared": nodes},
                }}),
            ),
            (
                "queue",
                json!({"default":"redis","connections":{"redis":{
                "driver":"redis", "connection":transport, "queue":queue, "retry_after":90, "block_for":1,
            }}, "failed":{"driver":"null"}}),
            ),
        ]);
        let mut app = TestApp::create(|builder| {
            builder
                .with_config(config)
                .with_providers(vec![Box::new(HorizonServiceProvider)])
                .with_routing(Routing::default().api(|| {
                    Route::get("/round-trip", queue_round_trip)?;
                    Ok(())
                }))
        })
        .unwrap();
        std::fs::write(
            app.root().join("cluster-config.json"),
            serde_json::to_vec(&settings).unwrap(),
        )
        .unwrap();
        app.boot().await.unwrap();
        let repository = app.app().make::<Repository>().await.unwrap();
        assert!(
            repository
                .master_supervisors()
                .all()
                .await
                .unwrap()
                .is_empty()
        );
        let command_queue = repository.command_queue();
        command_queue
            .push("cluster-probe", "pause", json!({}))
            .await
            .unwrap();
        assert_eq!(
            command_queue.pending("cluster-probe").await.unwrap().len(),
            1
        );
        let processes = repository.processes();
        assert!(
            processes
                .all_orphans("cluster-probe")
                .await
                .unwrap()
                .is_empty()
        );
        processes
            .orphaned("cluster-probe", &[10, 20])
            .await
            .unwrap();
        assert_eq!(
            processes
                .all_orphans("cluster-probe")
                .await
                .unwrap()
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [10, 20]
        );
        let tags = repository.tags();
        tags.monitor("fixture:alpha").await.unwrap();
        assert_eq!(
            tags.monitored(&["fixture:alpha".into(), "missing".into()])
                .await
                .unwrap(),
            ["fixture:alpha"]
        );

        let response = app.get("/api/round-trip").await.unwrap();
        response.assert_ok();
        let id = response.json::<String>().unwrap();
        let job = repository.jobs().find(&id).await.unwrap().unwrap();
        assert_eq!(job.record.status, "completed");
        assert_eq!(job.record.queue, queue);
        let metrics = repository.metrics_repository();
        metrics
            .increment_job("manual-cluster-probe", 12.5)
            .await
            .unwrap();
        assert_eq!(
            metrics
                .throughput_for_job("manual-cluster-probe")
                .await
                .unwrap(),
            1
        );

        let manager = app.app().make::<RedisManager>().await.unwrap();
        assert_eq!(
            manager.connection(Some(transport)).unwrap().is_cluster(),
            transport == "cluster"
        );
        let redis = if metadata == "standalone" {
            manager.connection(Some(metadata))
        } else {
            manager.cluster_connection(metadata)
        }
        .unwrap();
        let namespace = match index {
            1 => format!("{global}{prefix}{{horizon}}:"),
            3 => prefix,
            _ => format!("{{{prefix}}}"),
        };
        let rolling = format!("{namespace}metrics:fixture");
        let neighbor = format!("clustere-{{{run}}}:metrics:fixture");
        assert_eq!(
            redis
                .command("SET", &[rolling.as_bytes(), b"retained until clear"])
                .await
                .unwrap(),
            RedisValue::Okay
        );
        if index == 3 {
            redis
                .command("SET", &[neighbor.as_bytes(), b"neighbor"])
                .await
                .unwrap();
        }
        metrics.clear().await.unwrap();
        assert_eq!(
            redis.command("GET", &[rolling.as_bytes()]).await.unwrap(),
            RedisValue::Nil
        );
        assert!(repository.jobs().find(&id).await.unwrap().is_some());
        assert_eq!(tags.monitoring().await.unwrap(), ["fixture:alpha"]);
        if index == 3 {
            assert_eq!(
                redis.command("GET", &[neighbor.as_bytes()]).await.unwrap(),
                RedisValue::BulkString(b"neighbor".to_vec())
            );
        }
    }
}

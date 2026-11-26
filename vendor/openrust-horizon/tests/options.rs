use openrust::config::Repository;
use openrust_horizon::{AutoScaler, SupervisorOptions};
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn autoscaler_matches_pinned_php_reference() {
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/autoscaler.json")).unwrap();
    assert_eq!(
        reference["commit"],
        "b86d5aa87ca474db352ea2668bdd1df3a90bf8aa"
    );
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 224);
    for case in cases {
        let sizes: Vec<u64> = serde_json::from_value(case["sizes"].clone()).unwrap();
        let runtimes: Vec<f64> = serde_json::from_value(case["runtimes"].clone()).unwrap();
        let mut current: Vec<usize> = serde_json::from_value(case["current"].clone()).unwrap();
        let terminating: Vec<usize> = serde_json::from_value(case["terminating"].clone()).unwrap();
        let expected: Vec<usize> = serde_json::from_value(case["expected"].clone()).unwrap();
        let options = SupervisorOptions {
            queue: (0..sizes.len()).map(|index| index.to_string()).collect(),
            max_processes: case["max"].as_u64().unwrap() as usize,
            min_processes: case["min"].as_u64().unwrap_or(1) as usize,
            balance_max_shift: case["shift"].as_u64().unwrap_or(1) as usize,
            balance: case.get("balance").cloned().unwrap_or(json!("auto")),
            auto_scaling_strategy: case["strategy"].as_str().unwrap_or("time").into(),
            ..Default::default()
        };
        let targets = AutoScaler::number_of_workers_per_queue(&options, &sizes, &runtimes).unwrap();
        for (index, target) in targets.iter().enumerate() {
            let expected = case["recommendations"][index].as_f64().unwrap();
            assert!(
                (target - expected).abs() < 1e-10,
                "{}: {targets:?}",
                case["name"]
            );
        }
        let mut order: Vec<usize> = (0..targets.len()).collect();
        order.sort_by(|left, right| targets[*left].total_cmp(&targets[*right]));
        assert_eq!(json!(order), case["order"], "{}", case["name"]);
        for index in order {
            let pool = current[index] + terminating[index];
            let total = current.iter().sum::<usize>() + terminating.iter().sum::<usize>();
            if let Some(target) =
                AutoScaler::scale_pool(&options, pool, total, targets[index]).unwrap()
            {
                current[index] = target;
            }
        }
        assert_eq!(current, expected, "{}: {case}", case["name"]);
    }
}

#[test]
fn scaler_rejects_invalid_metrics_and_configuration() {
    let options = SupervisorOptions {
        queue: vec!["emails".into(), "reports".into()],
        max_processes: 8,
        balance: json!("auto"),
        ..Default::default()
    };
    assert!(options.validate().is_ok());
    let mut invalid = options.clone();
    invalid.min_processes = 0;
    assert!(invalid.validate().is_err());
    let mut invalid = options.clone();
    invalid.queue.push("emails".into());
    assert!(invalid.validate().is_err());
    for runtime in [f64::NAN, f64::INFINITY, -1., f64::MAX] {
        assert!(
            AutoScaler::number_of_workers_per_queue(&options, &[2, 1], &[runtime, 1.]).is_err()
        );
    }
    assert!(AutoScaler::number_of_workers_per_queue(&options, &[1], &[1.]).is_err());
}

#[test]
fn supervisor_preserves_redis_queue_names_as_literal_command_arguments() {
    let long = "a".repeat(255);
    let group = format!("出库 汇总:报表,{long}");
    let options = SupervisorOptions::from_array(json!({"queue":group,"balance":false})).unwrap();
    assert_eq!(options.pools(), std::slice::from_ref(&group));
    let executable = std::path::Path::new("openrust");
    let option = format!("--queue={group}");
    assert!(
        options
            .to_supervisor_command(executable)
            .unwrap()
            .contains(&option.clone().into())
    );
    assert!(
        options
            .to_worker_command(executable)
            .unwrap()
            .contains(&option.into())
    );
    for invalid in [
        "",
        "a\nb",
        "queue:delayed",
        "queue:reserved",
        "queue:notify",
    ] {
        assert!(
            SupervisorOptions::from_array(json!({"queue":[invalid]})).is_err(),
            "{invalid:?}"
        );
    }
    assert!(SupervisorOptions::from_array(json!({"queue":["a".repeat(256)]})).is_err());
    let invalid = SupervisorOptions {
        queue: vec!["one,two".into()],
        ..Default::default()
    };
    assert!(invalid.validate().is_err());
}

#[test]
fn options_preserve_upstream_defaults_and_allow_unlimited_tries_and_zero_sleep() {
    let options = SupervisorOptions::default();
    assert_eq!(options.balance, "off");
    assert_eq!(options.sleep, 3);
    assert_eq!(options.max_tries, 0);
    assert!(!options.balancing());
    assert!(options.auto_scaling());
    let options = SupervisorOptions {
        max_processes: 1000,
        max_tries: 0,
        sleep: 0,
        timeout: 0,
        balance_cooldown: 0,
        balance_max_shift: 0,
        ..options
    };
    assert!(options.validate().is_ok());
}

#[test]
fn worker_options_preserve_literals_aliases_zero_values_and_all_backoff_forms() {
    for backoff in [json!(0), json!("1,5,10"), json!([1, 5, 10])] {
        let options = SupervisorOptions::from_array(json!({
            "workersName":"reports worker $(literal)", "queue":"emails,reports",
            "tries":0, "timeout":0, "sleep":0, "backoff":backoff,
            "maxTime":42, "maxJobs":10, "rest":2, "force":true, "json":true,
        }))
        .unwrap();
        let arguments = options
            .to_worker_command(std::path::Path::new("/path with spaces/app"))
            .unwrap();
        let arguments: Vec<_> = arguments
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect();
        assert_eq!(
            &arguments[..4],
            &["/path with spaces/app", "artisan", "horizon:work", "redis"]
        );
        assert!(arguments.contains(&"--name=reports worker $(literal)"));
        for argument in [
            "--tries=0",
            "--timeout=0",
            "--sleep=0",
            "--rest=2",
            "--max-time=42",
            "--max-jobs=10",
            "--force",
            "--json",
        ] {
            assert!(arguments.contains(&argument), "{argument}: {arguments:?}");
        }
        assert!(arguments.contains(&if backoff == json!(0) {
            "--backoff=0"
        } else {
            "--backoff=1,5,10"
        }));
        assert_eq!(options.with_queue("urgent").queue, ["urgent"]);
        assert_eq!(options.queue, ["emails", "reports"]);
        let encoded = options.to_json().unwrap();
        let decoded =
            SupervisorOptions::from_array(serde_json::from_str(&encoded).unwrap()).unwrap();
        assert_eq!(decoded.to_array().unwrap(), options.to_array().unwrap());
    }
    for backoff in [
        json!([]),
        json!(-1),
        json!("1,no"),
        json!([1, "5"]),
        json!(null),
    ] {
        assert!(SupervisorOptions::from_array(json!({"backoff":backoff})).is_err());
    }
}

#[test]
fn environment_patterns_recursive_arrays_disabled_and_shared_queues_match_provisioning() {
    let config = |env: &str, horizon: serde_json::Value| {
        Repository::new(BTreeMap::from([
            ("app".into(), json!({"env":env})),
            ("horizon".into(), horizon),
            (
                "queue".into(),
                json!({"connections":{"redis":{"driver":"redis","retry_after":90}}}),
            ),
        ]))
    };
    let mut environments = serde_json::Map::new();
    environments.insert(
        "prod*".into(),
        json!({"one":{"queue":["override"],"maxProcesses":6}}),
    );
    environments.insert("*".into(), json!({"one":{"maxProcesses":7}}));
    let horizon = json!({
        "defaults":{
            "one":{"queue":["a","b"],"balance":"auto","maxProcesses":2},
            "two":{"queue":["override"],"maxProcesses":1},
            "disabled":{"maxProcesses":0,"connection":"not-configured"}
        },
        "environments":environments,
    });
    let options = SupervisorOptions::from_config(&config("production", horizon.clone())).unwrap();
    assert_eq!(options["one"].max_processes, 6);
    assert_eq!(options["one"].queue, ["override", "b"]);
    assert!(options.contains_key("two"));
    assert!(!options.contains_key("disabled"));
    let options = SupervisorOptions::from_config(&config("staging", horizon.clone())).unwrap();
    assert_eq!(options["one"].max_processes, 7);
    let mut horizon = horizon;
    horizon["environments"].as_object_mut().unwrap().remove("*");
    assert!(
        SupervisorOptions::from_config(&config("staging", horizon.clone()))
            .unwrap()
            .is_empty()
    );
    // the framework chooses the first matching pattern, including an earlier wildcard.
    let mut environments = serde_json::Map::new();
    environments.insert("*".into(), json!({"one":{"maxProcesses":9}}));
    environments.insert("production".into(), json!({"one":{"maxProcesses":10}}));
    horizon["environments"] = environments.into();
    assert_eq!(
        SupervisorOptions::from_config(&config("production", horizon)).unwrap()["one"]
            .max_processes,
        9
    );
}

#[test]
fn environment_overrides_merge_without_modifying_defaults_and_reject_unsafe_workers() {
    let config = Repository::new(BTreeMap::from([
        ("app".into(), json!({"env":"production"})),
        (
            "horizon".into(),
            json!({"defaults":{"supervisor-1":{"queue":["a","b"],"maxProcesses":2}},"environments":{"production":{"supervisor-1":{"maxProcesses":6}}}}),
        ),
        (
            "queue".into(),
            json!({"connections":{"redis":{"driver":"redis","retry_after":90}}}),
        ),
    ]));
    let options = SupervisorOptions::from_config(&config).unwrap();
    assert_eq!(options["supervisor-1"].max_processes, 6);
    assert_eq!(options["supervisor-1"].queue, vec!["a", "b"]);
    for driver in ["database", "sync"] {
        let unsafe_config = Repository::new(BTreeMap::from([
            (
                "horizon".into(),
                json!({"defaults":{"supervisor-1":{}},"environments":{"production":{}}}),
            ),
            (
                "queue".into(),
                json!({"connections":{"redis":{"driver":driver,"retry_after":90}}}),
            ),
        ]));
        assert!(SupervisorOptions::from_config(&unsafe_config).is_err());
    }
    for retry in [0, 30, 60] {
        let unsafe_config = Repository::new(BTreeMap::from([
            (
                "horizon".into(),
                json!({"defaults":{"supervisor-1":{}},"environments":{"production":{}}}),
            ),
            (
                "queue".into(),
                json!({"connections":{"redis":{"driver":"redis","retry_after":retry}}}),
            ),
        ]));
        assert!(SupervisorOptions::from_config(&unsafe_config).is_err());
    }
}

#[test]
fn cluster_queue_transports_can_be_provisioned() {
    let config = Repository::new(BTreeMap::from([
        (
            "horizon".into(),
            json!({"defaults":{"supervisor-1":{}},"environments":{"production":{}}}),
        ),
        (
            "queue".into(),
            json!({"connections":{"redis":{"driver":"redis","connection":"cluster","retry_after":90}}}),
        ),
        (
            "database".into(),
            json!({"redis":{"clusters":{"cluster":[{"host":"127.0.0.1","port":7000}]}}}),
        ),
    ]));
    assert_eq!(SupervisorOptions::from_config(&config).unwrap().len(), 1);
}

#[test]
fn horizon_environment_selects_the_deployment_plan_without_changing_app_environment() {
    let config = Repository::new(BTreeMap::from([
        ("app".into(), json!({"env":"testing"})),
        (
            "horizon".into(),
            json!({"env":"workers", "defaults":{"one":{"queue":"default"}},"environments":{"testing":{"one":{"maxProcesses":1}},"workers":{"one":{"maxProcesses":4}}}}),
        ),
        (
            "queue".into(),
            json!({"connections":{"redis":{"driver":"redis","retry_after":90}}}),
        ),
    ]));
    assert_eq!(
        SupervisorOptions::from_config(&config).unwrap()["one"].max_processes,
        4
    );
    assert_eq!(
        SupervisorOptions::from_config_for_environment(&config, "testing").unwrap()["one"]
            .max_processes,
        1
    );
}

#[test]
fn native_option_fields_keep_existing_json_and_tries_alias() {
    let options: SupervisorOptions = serde_json::from_value(json!({
        "workersName":"mail", "autoScalingStrategy":"size", "minProcesses":2,
        "maxProcesses":7, "balanceMaxShift":3, "balanceCooldown":4, "tries":5,
        "parentId":6, "maxJobs":10, "maxTime":11
    }))
    .unwrap();
    assert_eq!(options.workers_name, "mail");
    assert_eq!(options.max_tries, 5);
    let encoded = serde_json::to_value(&options).unwrap();
    for (key, expected) in [
        ("workersName", json!("mail")),
        ("autoScalingStrategy", json!("size")),
        ("minProcesses", json!(2)),
        ("maxProcesses", json!(7)),
        ("balanceMaxShift", json!(3)),
        ("balanceCooldown", json!(4)),
        ("maxTries", json!(5)),
        ("parentId", json!(6)),
        ("maxJobs", json!(10)),
        ("maxTime", json!(11)),
    ] {
        assert_eq!(encoded[key], expected, "{key}");
    }
    assert!(encoded.get("max_processes").is_none());
    assert!(serde_json::from_value::<SupervisorOptions>(json!({"max_processes":7})).is_err());
}

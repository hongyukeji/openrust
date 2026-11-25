use crate::{Error, Result, label};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SupervisorOptions {
    pub name: String,
    #[serde(rename = "workersName")]
    pub workers_name: String,
    pub connection: String,
    #[serde(deserialize_with = "queues")]
    pub queue: Vec<String>,
    /// "auto", "simple", or false (one priority-ordered queue pool).
    pub balance: serde_json::Value,
    #[serde(rename = "autoScalingStrategy")]
    pub auto_scaling_strategy: String,
    #[serde(rename = "minProcesses")]
    pub min_processes: usize,
    #[serde(rename = "maxProcesses")]
    pub max_processes: usize,
    #[serde(rename = "balanceMaxShift")]
    pub balance_max_shift: usize,
    #[serde(rename = "balanceCooldown")]
    pub balance_cooldown: u64,
    #[serde(rename = "maxTries")]
    #[serde(alias = "tries")]
    pub max_tries: u32,
    pub backoff: serde_json::Value,
    pub force: bool,
    pub rest: u32,
    pub nice: i32,
    pub directory: Option<std::path::PathBuf>,
    #[serde(rename = "parentId")]
    pub parent_id: u32,
    pub json: bool,
    pub timeout: u32,
    pub sleep: u32,
    pub memory: u32,
    #[serde(rename = "maxJobs")]
    pub max_jobs: u64,
    #[serde(rename = "maxTime")]
    pub max_time: u32,
}
impl Default for SupervisorOptions {
    fn default() -> Self {
        Self {
            name: String::new(),
            workers_name: "default".into(),
            connection: "redis".into(),
            queue: vec!["default".into()],
            balance: "off".into(),
            auto_scaling_strategy: "time".into(),
            min_processes: 1,
            max_processes: 1,
            balance_max_shift: 1,
            balance_cooldown: 3,
            max_tries: 0,
            backoff: serde_json::json!(0),
            force: false,
            rest: 0,
            nice: 0,
            directory: None,
            parent_id: 0,
            json: false,
            timeout: 60,
            sleep: 3,
            memory: 128,
            max_jobs: 0,
            max_time: 0,
        }
    }
}
impl SupervisorOptions {
    pub fn validate(&self) -> Result<()> {
        if !label(&self.connection)
            || self.queue.is_empty()
            || self.queue.iter().any(|queue| {
                queue.contains(',')
                    || openrust::queue::RedisQueue::validate_queue_name(queue).is_err()
            })
            || self.queue.iter().collect::<BTreeSet<_>>().len() != self.queue.len()
            || !matches!(self.balance.as_str(), Some("auto" | "simple" | "off"))
                && self.balance != false
            || !matches!(self.auto_scaling_strategy.as_str(), "time" | "size" | "log")
            || self.min_processes == 0
        {
            return Err(Error::InvalidConfiguration);
        }
        self.backoff_values()?;
        Ok(())
    }
    pub fn pools(&self) -> Vec<String> {
        if !self.balancing() {
            vec![self.queue.join(",")]
        } else {
            self.queue.clone()
        }
    }
    pub fn balancing(&self) -> bool {
        matches!(self.balance.as_str(), Some("auto" | "simple"))
    }
    pub fn auto_scaling(&self) -> bool {
        self.balance != "simple"
    }
    pub fn auto_scale_by_number_of_jobs(&self) -> bool {
        self.auto_scaling_strategy == "size"
    }
    pub fn auto_scale_logarithmically(&self) -> bool {
        self.auto_scaling_strategy == "log"
    }
    pub fn with_queue(&self, queue: impl Into<String>) -> Self {
        let mut options = self.clone();
        options.queue = queue.into().split(',').map(str::to_owned).collect();
        options
    }
    pub fn to_array(&self) -> Result<serde_json::Value> {
        serde_json::to_value(self).map_err(|_| Error::InvalidConfiguration)
    }
    pub fn from_array(value: serde_json::Value) -> Result<Self> {
        let options: Self =
            serde_json::from_value(value).map_err(|_| Error::InvalidConfiguration)?;
        options.validate()?;
        Ok(options)
    }
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|_| Error::InvalidConfiguration)
    }
    /// Literal argv for the native worker; no shell interpolation is used.
    pub fn to_worker_command(
        &self,
        executable: &std::path::Path,
    ) -> Result<Vec<std::ffi::OsString>> {
        self.validate()?;
        let mut arguments = Vec::new();
        if self.nice != 0 {
            #[cfg(unix)]
            arguments.extend(["nice".into(), "-n".into(), self.nice.to_string().into()]);
            #[cfg(not(unix))]
            return Err(Error::Unavailable);
        }
        arguments.extend([
            executable.as_os_str().to_owned(),
            "artisan".into(),
            "horizon:work".into(),
            self.connection.clone().into(),
            format!("--name={}", self.workers_name).into(),
            format!("--supervisor={}", self.name).into(),
            format!("--queue={}", self.queue.join(",")).into(),
            format!("--tries={}", self.max_tries).into(),
            format!(
                "--backoff={}",
                self.backoff_values()?
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            )
            .into(),
            format!("--timeout={}", self.timeout).into(),
            format!("--sleep={}", self.sleep).into(),
            format!("--memory={}", self.memory).into(),
            format!("--max-jobs={}", self.max_jobs).into(),
            format!("--max-time={}", self.max_time).into(),
            format!("--rest={}", self.rest).into(),
        ]);
        if self.force {
            arguments.push("--force".into());
        }
        if self.json {
            arguments.push("--json".into());
        }
        Ok(arguments)
    }
    /// Literal argv for a dedicated supervisor process.
    pub fn to_supervisor_command(
        &self,
        executable: &std::path::Path,
    ) -> Result<Vec<std::ffi::OsString>> {
        self.validate()?;
        let mut arguments: Vec<std::ffi::OsString> = vec![
            executable.as_os_str().to_owned(),
            "artisan".into(),
            "horizon:supervisor".into(),
            self.name.clone().into(),
            self.connection.clone().into(),
        ];
        for (key, value) in [
            ("balance", self.balance.as_str().unwrap_or("off").to_owned()),
            ("queue", self.queue.join(",")),
            ("workers-name", self.workers_name.clone()),
            (
                "backoff",
                self.backoff_values()?
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            ("max-jobs", self.max_jobs.to_string()),
            ("max-time", self.max_time.to_string()),
            ("max-processes", self.max_processes.to_string()),
            ("min-processes", self.min_processes.to_string()),
            ("memory", self.memory.to_string()),
            ("nice", self.nice.to_string()),
            ("sleep", self.sleep.to_string()),
            ("timeout", self.timeout.to_string()),
            ("tries", self.max_tries.to_string()),
            ("auto-scaling-strategy", self.auto_scaling_strategy.clone()),
            ("balance-cooldown", self.balance_cooldown.to_string()),
            ("balance-max-shift", self.balance_max_shift.to_string()),
            ("parent-id", self.parent_id.to_string()),
            ("rest", self.rest.to_string()),
        ] {
            arguments.push(format!("--{key}={value}").into());
        }
        if let Some(directory) = &self.directory {
            arguments.push(format!("--directory={}", directory.display()).into());
        }
        if self.force {
            arguments.push("--force".into());
        }
        if self.json {
            arguments.push("--json".into());
        }
        Ok(arguments)
    }
    pub(crate) fn backoff_values(&self) -> Result<Vec<u32>> {
        let values = match &self.backoff {
            serde_json::Value::Number(value) => vec![value.to_string()],
            serde_json::Value::String(value) => value.split(',').map(str::to_owned).collect(),
            serde_json::Value::Array(values) => {
                values.iter().map(serde_json::Value::to_string).collect()
            }
            _ => return Err(Error::InvalidConfiguration),
        };
        if values.is_empty() {
            return Err(Error::InvalidConfiguration);
        }
        values
            .iter()
            .map(|value| value.parse().map_err(|_| Error::InvalidConfiguration))
            .collect()
    }
    pub fn from_config(
        config: &dyn openrust::contracts::config::Repository,
    ) -> Result<BTreeMap<String, Self>> {
        let environment = config
            .get("horizon.env")
            .and_then(|v| v.as_str().map(str::to_owned))
            .or_else(|| {
                config
                    .get("app.env")
                    .and_then(|v| v.as_str().map(str::to_owned))
            })
            .unwrap_or("production".into());
        Self::from_config_for_environment(config, &environment)
    }
    pub fn from_config_for_environment(
        config: &dyn openrust::contracts::config::Repository,
        environment: &str,
    ) -> Result<BTreeMap<String, Self>> {
        Self::provisioning_plan(config, environment).map(|(options, _)| options)
    }

    pub(crate) fn provisioning_plan(
        config: &dyn openrust::contracts::config::Repository,
        environment: &str,
    ) -> Result<(BTreeMap<String, Self>, bool)> {
        let defaults = config
            .get("horizon.defaults")
            .unwrap_or(serde_json::json!({}));
        let overrides = config
            .get("horizon.environments")
            .unwrap_or(serde_json::json!({}));
        let overrides = overrides
            .as_object()
            .ok_or(Error::InvalidConfiguration)?
            .iter()
            .find(|(pattern, _)| {
                openrust::support::Str::is(&[pattern.as_str()], environment, false)
            })
            .map(|(_, value)| value);
        let defaults = defaults.as_object().ok_or(Error::InvalidConfiguration)?;
        let Some(overrides) = overrides else {
            return Ok((BTreeMap::new(), false));
        };
        let mut values = defaults.clone();
        for (name, value) in overrides.as_object().ok_or(Error::InvalidConfiguration)? {
            let mut merged = values.remove(name).unwrap_or(serde_json::json!({}));
            replace_recursive(&mut merged, value);
            values.insert(name.clone(), merged);
        }
        let mut result = BTreeMap::new();
        for (name, value) in values {
            if !label(&name) {
                return Err(Error::InvalidConfiguration);
            }
            let mut value: serde_json::Map<String, serde_json::Value> = value
                .as_object()
                .ok_or(Error::InvalidConfiguration)?
                .iter()
                .map(|(key, value)| {
                    let key = match key.as_str() {
                        "tries" => "maxTries".to_owned(),
                        "processes" => "maxProcesses".to_owned(),
                        _ => openrust::support::Str::camel(key),
                    };
                    (key, value.clone())
                })
                .collect();
            let connection = value
                .get("connection")
                .and_then(|v| v.as_str())
                .unwrap_or("redis");
            if !value.contains_key("queue") {
                let queue = config
                    .get(&format!("queue.connections.{connection}.queue"))
                    .unwrap_or(serde_json::json!("default"));
                value.insert("queue".into(), queue);
            }
            value
                .entry("name")
                .or_insert_with(|| serde_json::json!(name));
            value.insert("parentId".into(), serde_json::json!(std::process::id()));
            let options = Self::from_array(value.into())?;
            if options.max_processes == 0 {
                continue;
            }
            if config
                .get(&format!("queue.connections.{}.driver", options.connection))
                .and_then(|v| v.as_str().map(str::to_owned))
                .as_deref()
                != Some("redis")
                || config
                    .get(&format!(
                        "queue.connections.{}.retry_after",
                        options.connection
                    ))
                    .and_then(|v| v.as_u64())
                    .is_none_or(|v| options.timeout > 0 && v <= u64::from(options.timeout))
            {
                return Err(Error::InvalidConfiguration);
            }
            let redis = config
                .get(&format!(
                    "queue.connections.{}.connection",
                    options.connection
                ))
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or("default".into());
            if config
                .get(&format!("database.redis.clusters.{redis}"))
                .is_some()
            {
                return Err(Error::InvalidConfiguration);
            }
            result.insert(name, options);
        }
        Ok((result, true))
    }
}

fn queues<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Queues {
        List(Vec<String>),
        Names(String),
    }
    let values = match Queues::deserialize(deserializer)? {
        Queues::List(values) => values,
        Queues::Names(value) => vec![value],
    };
    Ok(values
        .iter()
        .flat_map(|value| value.split(',').map(str::to_owned))
        .collect())
}

// PHP array_replace_recursive also replaces numeric array keys individually.
fn replace_recursive(default: &mut serde_json::Value, replacement: &serde_json::Value) {
    match (default, replacement) {
        (serde_json::Value::Object(default), serde_json::Value::Object(replacement)) => {
            for (key, value) in replacement {
                replace_recursive(
                    default
                        .entry(key.clone())
                        .or_insert(serde_json::Value::Null),
                    value,
                );
            }
        }
        (serde_json::Value::Array(default), serde_json::Value::Array(replacement)) => {
            for (index, value) in replacement.iter().enumerate() {
                if let Some(default) = default.get_mut(index) {
                    replace_recursive(default, value);
                } else {
                    default.push(value.clone());
                }
            }
        }
        (default, replacement) => *default = replacement.clone(),
    }
}

pub struct AutoScaler;
impl AutoScaler {
    /// floating-point recommendations, in the original pool order.
    /// Apply smaller recommendations first so shrinking pools release capacity.
    pub fn number_of_workers_per_queue(
        options: &SupervisorOptions,
        sizes: &[u64],
        runtimes: &[f64],
    ) -> Result<Vec<f64>> {
        options.validate()?;
        let count = options.pools().len();
        if sizes.len() != count
            || runtimes.len() != count
            || runtimes.iter().any(|v| !v.is_finite() || *v < 0.)
        {
            return Err(Error::InvalidConfiguration);
        }
        let times: Vec<f64> = sizes
            .iter()
            .zip(runtimes)
            .map(|(size, runtime)| *size as f64 * runtime)
            .collect();
        let time_to_clear_all = times.iter().sum::<f64>();
        if !time_to_clear_all.is_finite() {
            return Err(Error::InvalidConfiguration);
        }
        let total_jobs = sizes.iter().map(|size| *size as f64).sum::<f64>();
        let total_log_jobs = sizes.iter().map(|size| (*size as f64).ln_1p()).sum::<f64>();
        Ok(sizes
            .iter()
            .zip(times)
            .map(|(size, time)| {
                if !options.balancing() {
                    (*size as f64)
                        .max(options.min_processes as f64)
                        .min(options.max_processes as f64)
                } else if options.balance == "simple" {
                    options.max_processes as f64 / count as f64
                } else if time_to_clear_all == 0. {
                    if *size == 0 {
                        options.min_processes as f64
                    } else {
                        options.max_processes as f64
                    }
                } else {
                    let proportion = match options.auto_scaling_strategy.as_str() {
                        "size" => *size as f64 / total_jobs,
                        "log" => (*size as f64).ln_1p() / total_log_jobs,
                        _ => time / time_to_clear_all,
                    };
                    proportion * options.max_processes as f64
                }
            })
            .collect())
    }

    /// Return the active pool size requested by scalePool, if changed.
    /// Both counts include terminating workers; ProcessPool scales active workers.
    pub fn scale_pool(
        options: &SupervisorOptions,
        total_process_count: usize,
        supervisor_process_count: usize,
        workers: f64,
    ) -> Result<Option<usize>> {
        options.validate()?;
        if !workers.is_finite() || workers < 0. || total_process_count > supervisor_process_count {
            return Err(Error::InvalidConfiguration);
        }
        let desired_process_count = workers.ceil() as usize;
        if desired_process_count > total_process_count {
            let max_up_shift = options
                .max_processes
                .saturating_sub(supervisor_process_count)
                .min(options.balance_max_shift);
            Ok(Some(
                total_process_count
                    .saturating_add(max_up_shift)
                    .min(
                        options
                            .max_processes
                            .saturating_sub(
                                (options.pools().len() - 1).saturating_mul(options.min_processes),
                            )
                            .max(options.min_processes),
                    )
                    .min(desired_process_count),
            ))
        } else if desired_process_count < total_process_count {
            let max_down_shift = supervisor_process_count
                .saturating_sub(options.min_processes)
                .min(options.balance_max_shift);
            Ok(Some(
                total_process_count
                    .saturating_sub(max_down_shift)
                    .max(options.min_processes)
                    .max(desired_process_count),
            ))
        } else {
            Ok(None)
        }
    }
}

use crate::{Error, Result, label};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SupervisorOptions {
    pub name: String,
    pub workersName: String,
    pub connection: String,
    #[serde(deserialize_with = "queues")]
    pub queue: Vec<String>,
    /// "auto", "simple", or false (one priority-ordered queue pool).
    pub balance: serde_json::Value,
    pub autoScalingStrategy: String,
    pub minProcesses: usize,
    pub maxProcesses: usize,
    pub balanceMaxShift: usize,
    pub balanceCooldown: u64,
    #[serde(alias = "tries")]
    pub maxTries: u32,
    pub backoff: serde_json::Value,
    pub force: bool,
    pub rest: u32,
    pub nice: i32,
    pub directory: Option<std::path::PathBuf>,
    pub parentId: u32,
    pub json: bool,
    pub timeout: u32,
    pub sleep: u32,
    pub memory: u32,
    pub maxJobs: u64,
    pub maxTime: u32,
}
impl Default for SupervisorOptions {
    fn default() -> Self {
        Self {
            name: String::new(),
            workersName: "default".into(),
            connection: "redis".into(),
            queue: vec!["default".into()],
            balance: "off".into(),
            autoScalingStrategy: "time".into(),
            minProcesses: 1,
            maxProcesses: 1,
            balanceMaxShift: 1,
            balanceCooldown: 3,
            maxTries: 0,
            backoff: serde_json::json!(0),
            force: false,
            rest: 0,
            nice: 0,
            directory: None,
            parentId: 0,
            json: false,
            timeout: 60,
            sleep: 3,
            memory: 128,
            maxJobs: 0,
            maxTime: 0,
        }
    }
}
impl SupervisorOptions {
    pub fn validate(&self) -> Result<()> {
        if !label(&self.connection)
            || self.queue.is_empty()
            || self.queue.iter().any(|q| !label(q))
            || self.queue.iter().collect::<BTreeSet<_>>().len() != self.queue.len()
            || !matches!(self.balance.as_str(), Some("auto" | "simple" | "off"))
                && self.balance != false
            || !matches!(self.autoScalingStrategy.as_str(), "time" | "size" | "log")
            || self.minProcesses == 0
        {
            return Err(Error::InvalidConfiguration);
        }
        self.backoffValues()?;
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
    pub fn autoScaling(&self) -> bool {
        self.balance != "simple"
    }
    pub fn autoScaleByNumberOfJobs(&self) -> bool {
        self.autoScalingStrategy == "size"
    }
    pub fn autoScaleLogarithmically(&self) -> bool {
        self.autoScalingStrategy == "log"
    }
    pub fn withQueue(&self, queue: impl Into<String>) -> Self {
        let mut options = self.clone();
        options.queue = queue.into().split(',').map(str::to_owned).collect();
        options
    }
    pub fn to_array(&self) -> Result<serde_json::Value> {
        serde_json::to_value(self).map_err(|_| Error::InvalidConfiguration)
    }
    pub fn fromArray(value: serde_json::Value) -> Result<Self> {
        let options: Self =
            serde_json::from_value(value).map_err(|_| Error::InvalidConfiguration)?;
        options.validate()?;
        Ok(options)
    }
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|_| Error::InvalidConfiguration)
    }
    /// Literal argv for the native worker; no shell interpolation is used.
    pub fn toWorkerCommand(&self, executable: &std::path::Path) -> Result<Vec<std::ffi::OsString>> {
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
            format!("--name={}", self.workersName).into(),
            format!("--supervisor={}", self.name).into(),
            format!("--queue={}", self.queue.join(",")).into(),
            format!("--tries={}", self.maxTries).into(),
            format!(
                "--backoff={}",
                self.backoffValues()?
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            )
            .into(),
            format!("--timeout={}", self.timeout).into(),
            format!("--sleep={}", self.sleep).into(),
            format!("--memory={}", self.memory).into(),
            format!("--max-jobs={}", self.maxJobs).into(),
            format!("--max-time={}", self.maxTime).into(),
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
    pub fn toSupervisorCommand(
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
            ("workers-name", self.workersName.clone()),
            (
                "backoff",
                self.backoffValues()?
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            ("max-jobs", self.maxJobs.to_string()),
            ("max-time", self.maxTime.to_string()),
            ("max-processes", self.maxProcesses.to_string()),
            ("min-processes", self.minProcesses.to_string()),
            ("memory", self.memory.to_string()),
            ("nice", self.nice.to_string()),
            ("sleep", self.sleep.to_string()),
            ("timeout", self.timeout.to_string()),
            ("tries", self.maxTries.to_string()),
            ("auto-scaling-strategy", self.autoScalingStrategy.clone()),
            ("balance-cooldown", self.balanceCooldown.to_string()),
            ("balance-max-shift", self.balanceMaxShift.to_string()),
            ("parent-id", self.parentId.to_string()),
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
    pub(crate) fn backoffValues(&self) -> Result<Vec<u32>> {
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
    pub fn fromConfig(
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
        Self::fromConfigForEnvironment(config, &environment)
    }
    pub fn fromConfigForEnvironment(
        config: &dyn openrust::contracts::config::Repository,
        environment: &str,
    ) -> Result<BTreeMap<String, Self>> {
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
            return Ok(BTreeMap::new());
        };
        let mut values = defaults.clone();
        for (name, value) in overrides.as_object().ok_or(Error::InvalidConfiguration)? {
            let mut merged = values.remove(name).unwrap_or(serde_json::json!({}));
            replaceRecursive(&mut merged, value);
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
            let options = Self::fromArray(value.into())?;
            if options.maxProcesses == 0 {
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
        Ok(result)
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
fn replaceRecursive(default: &mut serde_json::Value, replacement: &serde_json::Value) {
    match (default, replacement) {
        (serde_json::Value::Object(default), serde_json::Value::Object(replacement)) => {
            for (key, value) in replacement {
                replaceRecursive(
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
                    replaceRecursive(default, value);
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
    pub fn numberOfWorkersPerQueue(
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
        let timeToClearAll = times.iter().sum::<f64>();
        if !timeToClearAll.is_finite() {
            return Err(Error::InvalidConfiguration);
        }
        let totalJobs = sizes.iter().map(|size| *size as f64).sum::<f64>();
        let totalLogJobs = sizes.iter().map(|size| (*size as f64).ln_1p()).sum::<f64>();
        Ok(sizes
            .iter()
            .zip(times)
            .map(|(size, time)| {
                if !options.balancing() {
                    (*size as f64)
                        .max(options.minProcesses as f64)
                        .min(options.maxProcesses as f64)
                } else if options.balance == "simple" {
                    options.maxProcesses as f64 / count as f64
                } else if timeToClearAll == 0. {
                    if *size == 0 {
                        options.minProcesses as f64
                    } else {
                        options.maxProcesses as f64
                    }
                } else {
                    let proportion = match options.autoScalingStrategy.as_str() {
                        "size" => *size as f64 / totalJobs,
                        "log" => (*size as f64).ln_1p() / totalLogJobs,
                        _ => time / timeToClearAll,
                    };
                    proportion * options.maxProcesses as f64
                }
            })
            .collect())
    }

    /// Return the active pool size requested by scalePool, if changed.
    /// Both counts include terminating workers; ProcessPool scales active workers.
    pub fn scalePool(
        options: &SupervisorOptions,
        totalProcessCount: usize,
        supervisorProcessCount: usize,
        workers: f64,
    ) -> Result<Option<usize>> {
        options.validate()?;
        if !workers.is_finite() || workers < 0. || totalProcessCount > supervisorProcessCount {
            return Err(Error::InvalidConfiguration);
        }
        let desiredProcessCount = workers.ceil() as usize;
        if desiredProcessCount > totalProcessCount {
            let maxUpShift = options
                .maxProcesses
                .saturating_sub(supervisorProcessCount)
                .min(options.balanceMaxShift);
            Ok(Some(
                totalProcessCount
                    .saturating_add(maxUpShift)
                    .min(
                        options
                            .maxProcesses
                            .saturating_sub(
                                (options.pools().len() - 1).saturating_mul(options.minProcesses),
                            )
                            .max(options.minProcesses),
                    )
                    .min(desiredProcessCount),
            ))
        } else if desiredProcessCount < totalProcessCount {
            let maxDownShift = supervisorProcessCount
                .saturating_sub(options.minProcesses)
                .min(options.balanceMaxShift);
            Ok(Some(
                totalProcessCount
                    .saturating_sub(maxDownShift)
                    .max(options.minProcesses)
                    .max(desiredProcessCount),
            ))
        } else {
            Ok(None)
        }
    }
}

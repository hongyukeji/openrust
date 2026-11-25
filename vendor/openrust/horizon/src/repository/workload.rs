use super::Repository;
use crate::{Error, Result};
use serde_json::{Map, Value as Json, json};
use std::collections::BTreeMap;
#[derive(Clone)]
pub struct WorkloadRepository(pub(super) Repository);
impl Repository {
    pub fn workload(&self) -> WorkloadRepository {
        WorkloadRepository(self.clone())
    }
}
impl WorkloadRepository {
    pub fn new(repository: Repository) -> Self {
        Self(repository)
    }
    pub async fn get(&self) -> Result<Vec<Json>> {
        let manager = openrust::queue::QueueFacade::manager().map_err(|_| Error::Unavailable)?;
        let mut processes = BTreeMap::<String, u64>::new();
        for supervisor in self.0.supervisors().await? {
            if let Some(pools) = supervisor.get("processes").and_then(Json::as_object) {
                for (queue, count) in pools {
                    *processes.entry(queue.clone()).or_default() += count.as_u64().unwrap_or(0);
                }
            }
        }
        let mut result = Vec::new();
        for (name, workers) in processes {
            let Some((connection, queues)) = name.split_once(':') else {
                continue;
            };
            let queue = manager
                .connection(Some(connection))
                .map_err(|_| Error::Queue)?;
            let mut length = 0;
            let mut total_runtime = 0.;
            let mut split_wait = 0.;
            let workers_for_estimate = workers.max(1) as f64;
            let grouped = queues.contains(',');
            let mut split = Map::new();
            for name in queues.split(',') {
                let metrics = queue.metrics(Some(name)).await.map_err(|_| Error::Queue)?;
                let ready = metrics.pending.unwrap_or(metrics.size);
                let runtime = self
                    .0
                    .metrics_repository()
                    .runtime_for_queue(&format!("{connection}:{name}"))
                    .await?;
                let runtime = ready as f64 * runtime;
                total_runtime += runtime;
                if !grouped {
                    length = ready;
                } else if !split.contains_key(name) {
                    length += ready;
                    split_wait += (runtime / workers_for_estimate / 1000.).round();
                    // PHP 将规范的有符号整数字符串用作整数键。
                    let name_value = name
                        .parse::<i64>()
                        .ok()
                        .filter(|value| value.to_string() == name)
                        .map_or_else(|| json!(name), |value| json!(value));
                    split.insert(
                        name.into(),
                        json!({"name":name_value,"length":ready,"wait":split_wait}),
                    );
                }
            }
            let wait = (total_runtime / workers_for_estimate / 1000.).round();
            // PHP 只将按插入顺序从 0 连续编号的集合编码为 JSON 数组。
            let split_queues = if !grouped {
                Json::Null
            } else if split
                .keys()
                .enumerate()
                .all(|(index, key)| *key == index.to_string())
            {
                Json::Array(split.into_values().collect())
            } else {
                Json::Object(split)
            };
            result.push(json!({"name":queues,"connection":connection,"length":length,"wait":wait,"processes":workers,"split_queues":split_queues}));
        }
        result.sort_by(|left, right| {
            right["wait"]
                .as_f64()
                .unwrap_or(0.)
                .total_cmp(&left["wait"].as_f64().unwrap_or(0.))
        });
        Ok(result)
    }
}

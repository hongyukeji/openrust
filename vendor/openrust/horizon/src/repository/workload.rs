use super::Repository;
use crate::{Error, Result};
use serde_json::{Value as Json, json};
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
            let mut wait = 0.;
            let mut split = Vec::new();
            for name in queues.split(',') {
                let metrics = queue.metrics(Some(name)).await.map_err(|_| Error::Queue)?;
                let ready = metrics.pending.unwrap_or(metrics.size);
                length += ready;
                let runtime = self
                    .0
                    .metrics_repository()
                    .runtime_for_queue(&format!("{connection}:{name}"))
                    .await?;
                wait += ready as f64 * runtime / workers.max(1) as f64 / 1000.;
                split.push(json!({"name":name,"length":ready,"wait":wait.round()}));
            }
            result.push(json!({"name":queues,"connection":connection,"length":length,"wait":wait.round(),"processes":workers,"split_queues":if split.len()>1{Some(split)}else{None}}));
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

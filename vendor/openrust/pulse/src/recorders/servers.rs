use crate::{Error, Pulse, Result, events::SharedBeat};
use openrust::{
    contracts::{cache::Repository as _, config::Repository},
    support::{
        Str,
        facades::{Cache, Facade},
    },
};
use serde_json::json;
use std::{
    path::Path,
    sync::{Arc, Mutex, RwLock},
};

/// System memory in MiB, matching Pulse's server metrics.
#[derive(Clone, Copy, Debug)]
pub struct Memory {
    pub total: i64,
    pub used: i64,
}
type CpuDetector = dyn Fn() -> i64 + Send + Sync;
type MemoryDetector = dyn Fn() -> Memory + Send + Sync;
/// Application-owned system sampler. Native OS calls replace upstream shell pipelines.
pub struct Servers {
    config: Arc<dyn Repository>,
    system: Arc<Mutex<Option<sysinfo::System>>>,
    cpu: RwLock<Option<Arc<CpuDetector>>>,
    memory: RwLock<Option<Arc<MemoryDetector>>>,
}
impl Servers {
    /// 为包默认配置和发布后的应用配置读取本机名称。
    #[doc(hidden)]
    pub fn default_server_name() -> String {
        sysinfo::System::host_name().unwrap_or_else(|| "unknown".into())
    }

    pub fn new(config: Arc<dyn Repository>) -> Self {
        Self {
            config,
            system: Arc::default(),
            cpu: RwLock::default(),
            memory: RwLock::default(),
        }
    }
    pub fn detect_cpu_using(&self, callback: Option<Arc<CpuDetector>>) {
        *self.cpu.write().unwrap_or_else(|e| e.into_inner()) = callback;
    }
    pub fn detect_memory_using(&self, callback: Option<Arc<MemoryDetector>>) {
        *self.memory.write().unwrap_or_else(|e| e.into_inner()) = callback;
    }
    pub async fn record(&self, event: &SharedBeat, pulse: &Pulse) -> Result<()> {
        let options = self
            .config
            .get("pulse.recorders.Servers")
            .unwrap_or(false.into());
        if options == false
            || options.get("enabled").and_then(serde_json::Value::as_bool) == Some(false)
        {
            return Ok(());
        }
        let cache_name = self
            .config
            .get("pulse.cache")
            .and_then(|v| v.as_str().map(str::to_owned));
        let cache = Cache::get_facade_root()
            .map_err(|_| Error::StateUnavailable)?
            .store(cache_name.as_deref())?;
        let key = format!("openrust:pulse:throttle:{}:Servers", event.instance);
        if cache
            .get(&key)
            .await?
            .and_then(|v| v.as_i64())
            .is_some_and(|time| time + 15 > event.time)
        {
            return Ok(());
        }
        let system = self.system.clone();
        let cpu = self.cpu.read().unwrap_or_else(|e| e.into_inner()).clone();
        let memory = self
            .memory
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let directories = options
            .get("directories")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_else(|| vec!["/".into()]);
        let (cpu,memory,storage)=tokio::task::spawn_blocking(move || {
            let mut system=system.lock().unwrap_or_else(|e|e.into_inner());
            if (cpu.is_none()||memory.is_none())&&system.is_none() {
                let mut sample=sysinfo::System::new();sample.refresh_cpu_usage();
                std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);system.replace(sample);
            }
            if let Some(s)=system.as_mut(){s.refresh_cpu_usage();s.refresh_memory();}
            let cpu=cpu.map(|f|f()).unwrap_or_else(||system.as_ref().map(|s|s.global_cpu_usage().trunc() as i64).unwrap_or_default());
            let memory=memory.map(|f|f()).unwrap_or_else(||system.as_ref().map(|s|Memory{total:(s.total_memory()/1024/1024) as i64,used:(s.used_memory()/1024/1024) as i64}).unwrap_or(Memory{total:0,used:0}));
            let disks=sysinfo::Disks::new_with_refreshed_list();
            let storage:Vec<_>=directories.iter().filter_map(|v|{
                let directory=v.as_str()?;let path=Path::new(directory).canonicalize().ok()?;
                let disk=disks.iter().filter(|d|path.starts_with(d.mount_point())).max_by_key(|d|d.mount_point().as_os_str().len())?;
                Some(json!({"directory":directory,"total":(disk.total_space() as f64/1048576.0).round() as u64,"used":((disk.total_space()-disk.available_space()) as f64/1048576.0).round() as u64}))
            }).collect();(cpu,memory,storage)
        }).await.map_err(|_|Error::StateUnavailable)?;
        let name = options
            .get("server_name")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .unwrap_or_else(Self::default_server_name);
        let slug = Str::slug(&name);
        pulse
            .record("cpu", &slug, Some(cpu), Some(event.time))
            .await
            .avg()
            .only_buckets();
        pulse
            .record("memory", &slug, Some(memory.used), Some(event.time))
            .await
            .avg()
            .only_buckets();
        pulse.set("system",slug,json!({"name":name,"cpu":cpu,"memory_used":memory.used,"memory_total":memory.total,"storage":storage}).to_string(),Some(event.time)).await;
        cache
            .put(
                &key,
                &event.time.into(),
                Some(openrust::cache::Ttl::Seconds(15)),
            )
            .await?;
        Ok(())
    }
}

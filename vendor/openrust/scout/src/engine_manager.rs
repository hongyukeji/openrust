use crate::{
    Error, Result, ScoutFuture, Searchable,
    engines::{CollectionEngine, Engine, NullEngine},
};
use openrust::{contracts::config::Repository, foundation::Application};
use std::{
    any::{Any, TypeId},
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, RwLock},
};
type EngineKey = (TypeId, String);
type Engines = HashMap<EngineKey, Arc<dyn Any + Send + Sync>>;
#[derive(Clone)]
pub(crate) struct Registration {
    pub import: Arc<dyn Fn(Option<u64>) -> ScoutFuture<'static, u64> + Send + Sync>,
    pub flush: Arc<dyn Fn() -> ScoutFuture<'static, ()> + Send + Sync>,
}
pub struct EngineManager {
    pub(crate) config: Arc<dyn Repository>,
    pub(crate) queue: Arc<openrust::queue::QueueManager>,
    engines: RwLock<Engines>,
    disabled: RwLock<HashSet<TypeId>>,
    registered: RwLock<HashSet<TypeId>>,
    pub(crate) models: RwLock<BTreeMap<String, Registration>>,
}
impl EngineManager {
    pub fn new(config: Arc<dyn Repository>, queue: Arc<openrust::queue::QueueManager>) -> Self {
        Self {
            config,
            queue,
            engines: RwLock::default(),
            disabled: RwLock::default(),
            registered: RwLock::default(),
            models: RwLock::default(),
        }
    }
    pub(crate) fn boolean(&self, key: &str) -> bool {
        self.config
            .get(&format!("scout.{key}"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
    pub fn getDefaultDriver(&self) -> String {
        self.config
            .get("scout.driver")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or("collection".into())
    }
    pub fn engine<M: Searchable>(&self, name: Option<&str>) -> Result<Arc<dyn Engine<M>>> {
        let name = name
            .map(str::to_owned)
            .unwrap_or_else(|| self.getDefaultDriver());
        if let Some(engine) = self
            .engines
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(TypeId::of::<M>(), name.clone()))
        {
            return Ok(engine
                .downcast_ref::<Arc<dyn Engine<M>>>()
                .expect("engine indexed by model type")
                .clone());
        }
        match name.as_str() {
            "collection" => Ok(Arc::new(CollectionEngine)),
            "null" => Ok(Arc::new(NullEngine)),
            _ => Err(Error::InvalidConfiguration(format!(
                "Driver [{name}] not supported."
            ))),
        }
    }
    pub fn extend<M: Searchable>(
        &self,
        name: impl Into<String>,
        engine: Arc<dyn Engine<M>>,
    ) -> &Self {
        self.engines
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert((TypeId::of::<M>(), name.into()), Arc::new(engine));
        self
    }
    pub fn disableSyncingFor<M: Searchable>(&self) {
        self.disabled
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(TypeId::of::<M>());
    }
    pub fn enableSyncingFor<M: Searchable>(&self) {
        self.disabled
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&TypeId::of::<M>());
    }
    pub fn syncingDisabledFor<M: Searchable>(&self) -> bool {
        self.disabled
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&TypeId::of::<M>())
            || crate::model_observer::suppressed::<M>(self)
    }
    pub async fn register<M: Searchable>(
        self: &Arc<Self>,
        app: &Application,
        name: &str,
    ) -> Result<()> {
        if self
            .models
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(name)
        {
            return Err(Error::InvalidConfiguration(format!(
                "Scout model [{name}] is already registered"
            )));
        }
        let mut registered = self.registered.write().unwrap_or_else(|e| e.into_inner());
        if registered.contains(&TypeId::of::<M>()) {
            return Err(Error::InvalidConfiguration(
                "Scout model type is already registered".into(),
            ));
        }
        self.queue.register(
            &crate::jobs::jobName::<M>(false),
            Arc::new(crate::jobs::MakeSearchable::<M>::new(self.clone())),
        )?;
        self.queue.register(
            &crate::jobs::jobName::<M>(true),
            Arc::new(crate::jobs::RemoveFromSearch::<M>::new(self.clone())),
        )?;
        if self.boolean("after_commit") {
            M::observeAfterCommit(app, crate::ModelObserver::new(self.clone()))?;
        } else {
            M::observe(app, crate::ModelObserver::new(self.clone()))?;
        }
        let manager = self.clone();
        let flush = self.clone();
        self.models
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                name.into(),
                Registration {
                    import: Arc::new(move |chunk| {
                        let manager = manager.clone();
                        Box::pin(
                            async move { crate::searchable::import::<M>(&manager, chunk).await },
                        )
                    }),
                    flush: Arc::new(move || {
                        let manager = flush.clone();
                        Box::pin(async move {
                            let model = M::from_state(Default::default());
                            manager
                                .engine::<M>(model.scoutDriver().as_deref())?
                                .flush(&model)
                                .await
                        })
                    }),
                },
            );
        registered.insert(TypeId::of::<M>());
        Ok(())
    }
    pub(crate) async fn sync<M: Searchable>(
        &self,
        models: &[M],
        delete: bool,
        forceSync: bool,
    ) -> Result<()> {
        let Some(first) = models.first() else {
            return Ok(());
        };
        let queued = self
            .config
            .get("scout.queue")
            .is_some_and(|v| v.as_bool().unwrap_or_else(|| v.is_object()));
        if queued && !forceSync {
            let keys = models
                .iter()
                .map(|m| {
                    m.getScoutKey()
                        .ok_or_else(|| {
                            Error::InvalidConfiguration(
                                "Searchable models require a Scout key".into(),
                            )
                        })
                        .and_then(crate::jobs::keyToJson)
                })
                .collect::<Result<Vec<_>>>()?;
            let payload = openrust::queue::Payload::new(
                crate::jobs::jobName::<M>(delete),
                serde_json::json!({"keys":keys,"connection":first.getConnectionName()}),
            )?;
            let connection = first.syncWithSearchUsing(self);
            let queue = first.syncWithSearchUsingQueue(self);
            self.queue
                .dispatch(
                    payload,
                    openrust::queue::DispatchOptions {
                        queue,
                        connection,
                        ..Default::default()
                    },
                    None,
                )
                .await?;
        } else {
            let engine = self.engine::<M>(first.scoutDriver().as_deref())?;
            if delete {
                engine.delete(models).await?;
            } else {
                let models = M::makeSearchableUsing(models.to_vec()).await?;
                if !models.is_empty() {
                    engine.update(&models).await?;
                }
            }
        }
        Ok(())
    }
}

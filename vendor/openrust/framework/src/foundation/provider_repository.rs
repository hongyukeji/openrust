//! Compiled deferred provider directory, shared by all entry points of one application.
use super::{Application, application::ApplicationState};
use crate::{
    container::{
        ServiceKey,
        deferred::{DeferredServices, LoadFuture},
    },
    contracts::{container::BindingResolutionError, foundation::Application as _},
    support::{ProviderFactory, ServiceProvider},
};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};
use tokio::sync::Mutex as AsyncMutex;

const PENDING: u8 = 0;
const REGISTERING: u8 = 1;
const REGISTERED: u8 = 2;
const BOOTING: u8 = 3;
const READY: u8 = 4;
const FAILED: u8 = 5;
const REGISTRATION_FAILED: u8 = 6;

tokio::task_local! { static CURRENT: Vec<usize>; }

struct Slot {
    provider: AsyncMutex<Option<Box<dyn ServiceProvider>>>,
    factory: Option<Arc<ProviderFactory>>,
    keys: Vec<ServiceKey>,
    name: &'static str,
    phase: AtomicU8,
    early: AtomicBool,
    failure: Mutex<Option<Arc<dyn Error + Send + Sync>>>,
}
impl Slot {
    fn id(&self) -> usize {
        std::ptr::from_ref(self).addr()
    }
    fn owned(&self) -> bool {
        CURRENT
            .try_with(|stack| stack.contains(&self.id()))
            .unwrap_or(false)
    }
    fn failed(&self) -> BindingResolutionError {
        let source = self
            .failure
            .lock()
            .expect("provider failure lock poisoned")
            .clone()
            .unwrap_or_else(|| {
                Arc::new(std::io::Error::other(
                    "deferred provider initialization interrupted",
                ))
            });
        BindingResolutionError::DeferredProviderFailed {
            provider: self.name,
            source,
        }
    }
    fn failure(&self, source: Box<dyn Error + Send + Sync>, phase: u8) -> BindingResolutionError {
        let previous = self
            .failure
            .lock()
            .expect("provider failure lock poisoned")
            .replace(Arc::from(source));
        drop(previous);
        self.phase.store(phase, Ordering::Release);
        self.failed()
    }
}
struct Attempt<'a> {
    slot: &'a Slot,
    complete: bool,
}
impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        if !self.complete
            && matches!(
                self.slot.phase.load(Ordering::Acquire),
                REGISTERING | BOOTING
            )
        {
            self.slot.phase.store(FAILED, Ordering::Release);
        }
    }
}

pub(super) struct ProviderRepository {
    application: Weak<ApplicationState>,
    services: HashMap<ServiceKey, Arc<Slot>>,
    pending: Mutex<HashSet<ServiceKey>>,
    ordered: Vec<Arc<Slot>>,
    waits: Mutex<HashMap<(usize, usize), usize>>,
    registered: Mutex<Vec<Arc<Slot>>>,
}
struct Edge<'a> {
    registry: &'a ProviderRepository,
    key: (usize, usize),
}
impl Drop for Edge<'_> {
    fn drop(&mut self) {
        let mut waits = self
            .registry
            .waits
            .lock()
            .expect("provider wait graph poisoned");
        if let Some(count) = waits.get_mut(&self.key) {
            *count -= 1;
            if *count == 0 {
                waits.remove(&self.key);
            }
        }
    }
}
impl ProviderRepository {
    pub(super) fn new(
        application: Weak<ApplicationState>,
        providers: Vec<Box<dyn ServiceProvider>>,
    ) -> Arc<Self> {
        let mut services = HashMap::new();
        let mut ordered = Vec::new();
        for provider in providers {
            let keys = provider.provides();
            let factory = provider.deferredFactory();
            let name = provider.name();
            let provider = if factory.is_some() {
                None
            } else {
                Some(provider)
            };
            let slot = Arc::new(Slot {
                name,
                factory,
                keys: keys.clone(),
                provider: AsyncMutex::new(provider),
                phase: AtomicU8::new(PENDING),
                early: AtomicBool::new(false),
                failure: Mutex::new(None),
            });
            for key in keys {
                services.insert(key, slot.clone());
            }
            ordered.push(slot);
        }
        Arc::new(Self {
            application,
            pending: Mutex::new(services.keys().cloned().collect()),
            services,
            ordered,
            waits: Mutex::new(HashMap::new()),
            registered: Mutex::new(Vec::new()),
        })
    }
    pub(super) fn is_empty(&self) -> bool {
        self.services.is_empty()
    }
    fn application(&self) -> Result<Application, BindingResolutionError> {
        self.application
            .upgrade()
            .map(|shared| Application { shared })
            .ok_or_else(|| BindingResolutionError::DeferredProviderFailed {
                provider: "application",
                source: Arc::new(std::io::Error::other("application no longer available")),
            })
    }
    fn edge(
        &self,
        slot: &Slot,
        key: &ServiceKey,
    ) -> Result<Option<Edge<'_>>, BindingResolutionError> {
        let Some(owner) = CURRENT
            .try_with(|stack| stack.last().copied())
            .ok()
            .flatten()
        else {
            return Ok(None);
        };
        let target = slot.id();
        let mut waits = self.waits.lock().expect("provider wait graph poisoned");
        let mut pending = vec![target];
        let mut seen = HashSet::new();
        while let Some(next) = pending.pop() {
            if next == owner {
                return Err(BindingResolutionError::CircularDependency {
                    services: vec![key.clone(), key.clone()],
                });
            }
            if seen.insert(next) {
                pending.extend(
                    waits
                        .keys()
                        .filter(|(from, _)| *from == next)
                        .map(|(_, to)| *to),
                );
            }
        }
        *waits.entry((owner, target)).or_default() += 1;
        Ok(Some(Edge {
            registry: self,
            key: (owner, target),
        }))
    }
    async fn initialize(
        &self,
        slot: &Arc<Slot>,
        key: &ServiceKey,
        boot: bool,
    ) -> Result<(), BindingResolutionError> {
        let phase = slot.phase.load(Ordering::Acquire);
        if phase == READY || (phase == REGISTERED && !boot) {
            return Ok(());
        }
        // A provider boot hook may resolve its already registered service recursively.
        if slot.owned() {
            if matches!(phase, REGISTERED | BOOTING | READY) {
                return Ok(());
            }
            return Err(BindingResolutionError::CircularDependency {
                services: vec![key.clone(), key.clone()],
            });
        }
        let _edge = self.edge(slot, key)?;
        let mut provider = slot.provider.lock().await;
        let phase = slot.phase.load(Ordering::Acquire);
        if phase == FAILED {
            return Err(slot.failed());
        }
        if phase == READY || (phase == REGISTERED && !boot) {
            return Ok(());
        }
        if phase == REGISTRATION_FAILED && !self.contains(key) {
            return Err(slot.failed());
        }
        let registering = matches!(phase, PENDING | REGISTRATION_FAILED);
        if registering {
            self.pending
                .lock()
                .expect("deferred keys lock poisoned")
                .remove(key);
        }
        let app = self.application()?;
        let mut stack = CURRENT.try_with(Clone::clone).unwrap_or_default();
        stack.push(slot.id());
        CURRENT
            .scope(
                stack,
                crate::support::facades::scope(app.container_handle(), async {
                    let mut attempt = Attempt {
                        slot,
                        complete: false,
                    };
                    if registering {
                        slot.phase.store(REGISTERING, Ordering::Release);
                        if let Some(factory) = &slot.factory {
                            let previous = provider.replace(factory());
                            drop(previous);
                        }
                        provider
                            .as_mut()
                            .expect("deferred provider constructed")
                            .register(&app)
                            .await
                            .map_err(|source| slot.failure(source, REGISTRATION_FAILED))?;
                        slot.early.store(!app.booting_started(), Ordering::Release);
                        slot.phase.store(REGISTERED, Ordering::Release);
                        self.registered
                            .lock()
                            .expect("registered provider list poisoned")
                            .push(slot.clone());
                    }
                    if boot || app.is_booted() {
                        slot.phase.store(BOOTING, Ordering::Release);
                        provider
                            .as_mut()
                            .expect("deferred provider constructed")
                            .boot(&app)
                            .await
                            .map_err(|source| slot.failure(source, FAILED))?;
                        slot.phase.store(READY, Ordering::Release);
                    }
                    attempt.complete = true;
                    Ok(())
                }),
            )
            .await
    }
    pub(super) async fn load_all(&self) -> Result<(), BindingResolutionError> {
        let boot = self.application()?.is_booted();
        for slot in &self.ordered {
            for key in &slot.keys {
                if self.contains(key)
                    && self
                        .services
                        .get(key)
                        .is_some_and(|owner| Arc::ptr_eq(owner, slot))
                {
                    self.initialize(slot, key, boot).await?;
                }
            }
        }
        self.pending
            .lock()
            .expect("deferred keys lock poisoned")
            .clear();
        Ok(())
    }
    pub(super) async fn boot_prepared(&self) -> Result<(), BindingResolutionError> {
        let registered = self
            .registered
            .lock()
            .expect("registered provider list poisoned")
            .clone();
        for slot in registered {
            if slot.early.load(Ordering::Acquire) {
                let key = slot
                    .keys
                    .first()
                    .cloned()
                    .unwrap_or_else(|| ServiceKey::named(slot.name));
                self.initialize(&slot, &key, true).await?;
            }
        }
        Ok(())
    }
    /// Providers first used during bootstrap register immediately and join the boot tail.
    pub(super) async fn boot_registered(&self) -> Result<(), BindingResolutionError> {
        loop {
            let mut advanced = false;
            let registered = self
                .registered
                .lock()
                .expect("registered provider list poisoned")
                .clone();
            for slot in registered {
                if slot.early.swap(false, Ordering::AcqRel)
                    && slot.phase.load(Ordering::Acquire) == READY
                {
                    slot.phase.store(REGISTERED, Ordering::Release);
                }
                if slot.phase.load(Ordering::Acquire) == REGISTERED {
                    let key = slot
                        .keys
                        .first()
                        .cloned()
                        .unwrap_or_else(|| ServiceKey::named(slot.name));
                    self.initialize(&slot, &key, true).await?;
                    advanced = true;
                }
            }
            if !advanced {
                return Ok(());
            }
        }
    }
}
impl DeferredServices for ProviderRepository {
    fn in_flight(&self, key: &ServiceKey) -> bool {
        self.services.get(key).is_some_and(|slot| {
            matches!(
                slot.phase.load(Ordering::Acquire),
                REGISTERING | BOOTING | FAILED
            )
        })
    }
    fn contains(&self, key: &ServiceKey) -> bool {
        self.pending
            .lock()
            .expect("deferred keys lock poisoned")
            .contains(key)
    }

    fn needs_loading(&self, key: &ServiceKey, has_instance: bool) -> bool {
        let Some(slot) = self.services.get(key) else {
            return false;
        };
        match slot.phase.load(Ordering::Acquire) {
            READY => false,
            PENDING | REGISTRATION_FAILED => self.contains(key) && !has_instance,
            REGISTERED => self
                .application
                .upgrade()
                .is_some_and(|shared| Application { shared }.is_booted()),
            BOOTING => !slot.owned(),
            _ => true,
        }
    }
    fn load<'a>(&'a self, key: &'a ServiceKey) -> LoadFuture<'a> {
        Box::pin(async move {
            let Some(slot) = self.services.get(key) else {
                return Ok(());
            };
            let boot = self.application()?.is_booted();
            self.initialize(slot, key, boot).await
        })
    }
}

//! The framework's service container.

use std::{
    any::{Any, TypeId, type_name},
    cell::RefCell,
    collections::HashMap,
    panic::{self, AssertUnwindSafe},
    sync::{Arc, Condvar, Mutex, RwLock},
    thread::{self, ThreadId},
};

use crate::contracts::container::{self, BindingResolutionError};

type Factory<T> = dyn Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync;
type ErasedBinding = Arc<dyn Any + Send + Sync>;
type Wait = (ThreadId, &'static str, usize);

/// Owns the application's service bindings and shared instances.
#[derive(Default)]
pub struct Container {
    bindings: RwLock<HashMap<TypeId, ErasedBinding>>,
    waiting: Mutex<HashMap<ThreadId, Wait>>,
}

enum Binding<T: ?Sized> {
    Factory(Box<Factory<T>>),
    Singleton {
        factory: Box<Factory<T>>,
        state: Mutex<SingletonState<T>>,
        ready: Condvar,
    },
    Instance(Arc<T>),
}

enum SingletonState<T: ?Sized> {
    Empty,
    Running(ThreadId),
    Ready(Arc<T>),
}

thread_local! {
    static RESOLVING: RefCell<Vec<(usize, TypeId, &'static str)>> = const { RefCell::new(Vec::new()) };
}

struct Resolution;

impl Drop for Resolution {
    fn drop(&mut self) {
        RESOLVING.with_borrow_mut(|stack| {
            stack.pop();
        });
    }
}

impl Container {
    fn insert<T: ?Sized + Send + Sync + 'static>(&self, binding: Binding<T>) {
        let previous = self
            .bindings
            .write()
            .expect("container lock poisoned")
            .insert(TypeId::of::<T>(), Arc::new(binding));
        drop(previous);
    }

    fn resolve<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> Result<Resolution, BindingResolutionError> {
        let container = std::ptr::from_ref(self).addr();
        let id = TypeId::of::<T>();
        let service = type_name::<T>();
        RESOLVING.with_borrow_mut(|stack| {
            if let Some(start) = stack
                .iter()
                .position(|&(owner, binding, _)| owner == container && binding == id)
            {
                let services = stack[start..]
                    .iter()
                    .map(|&(_, _, name)| name)
                    .chain(std::iter::once(service))
                    .collect();
                return Err(BindingResolutionError::CircularDependency { services });
            }
            stack.push((container, id, service));
            Ok(Resolution)
        })
    }

    fn wait_for(
        &self,
        caller: ThreadId,
        owner: ThreadId,
        service: &'static str,
        binding: usize,
    ) -> Result<(), BindingResolutionError> {
        let mut waiting = self.waiting.lock().expect("container wait graph poisoned");
        let mut thread = owner;
        let mut services = vec![service];
        while let Some(&(next, name, _)) = waiting.get(&thread) {
            services.push(name);
            thread = next;
            if thread == caller {
                services.push(service);
                return Err(BindingResolutionError::CircularDependency { services });
            }
        }
        waiting.insert(caller, (owner, service, binding));
        Ok(())
    }
}

impl container::Container for Container {
    fn bind<T, F>(&self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert(Binding::Factory(Box::new(factory)));
    }

    fn singleton<T, F>(&self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert(Binding::Singleton {
            factory: Box::new(factory),
            state: Mutex::new(SingletonState::Empty),
            ready: Condvar::new(),
        });
    }

    fn instance<T: ?Sized + Send + Sync + 'static>(&self, service: Arc<T>) {
        self.insert(Binding::Instance(service));
    }

    fn bound<T: ?Sized + Send + Sync + 'static>(&self) -> bool {
        self.bindings
            .read()
            .expect("container lock poisoned")
            .contains_key(&TypeId::of::<T>())
    }

    fn make<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, BindingResolutionError> {
        let service = type_name::<T>();
        let binding = self
            .bindings
            .read()
            .expect("container lock poisoned")
            .get(&TypeId::of::<T>())
            .cloned()
            .ok_or(BindingResolutionError::NotBound { service })?;
        let binding = binding
            .downcast::<Binding<T>>()
            .expect("binding type matches its key");
        let _resolution = self.resolve::<T>()?;
        match binding.as_ref() {
            Binding::Factory(factory) => factory(self),
            Binding::Instance(instance) => Ok(Arc::clone(instance)),
            Binding::Singleton {
                factory,
                state,
                ready,
            } => {
                let caller = thread::current().id();
                let binding = std::ptr::from_ref(state).addr();
                let mut slot = state.lock().expect("singleton state poisoned");
                loop {
                    match &*slot {
                        SingletonState::Ready(instance) => return Ok(Arc::clone(instance)),
                        SingletonState::Empty => {
                            *slot = SingletonState::Running(caller);
                            break;
                        }
                        SingletonState::Running(owner) => {
                            self.wait_for(caller, *owner, service, binding)?;
                            slot = ready.wait(slot).expect("singleton state poisoned");
                            self.waiting
                                .lock()
                                .expect("container wait graph poisoned")
                                .remove(&caller);
                        }
                    }
                }
                drop(slot);
                let result = panic::catch_unwind(AssertUnwindSafe(|| factory(self)));
                let mut slot = state.lock().expect("singleton state poisoned");
                *slot = match &result {
                    Ok(Ok(instance)) => SingletonState::Ready(Arc::clone(instance)),
                    _ => SingletonState::Empty,
                };
                self.waiting
                    .lock()
                    .expect("container wait graph poisoned")
                    .retain(|_, &mut (_, _, waiting_for)| waiting_for != binding);
                ready.notify_all();
                drop(slot);
                result.unwrap_or_else(|payload| panic::resume_unwind(payload))
            }
        }
    }
}

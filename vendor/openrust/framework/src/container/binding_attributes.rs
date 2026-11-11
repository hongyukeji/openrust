//! Compiled binding attributes selected in Laravel declaration order.

use std::{
    cell::RefCell,
    sync::{Arc, atomic::Ordering},
};

use crate::contracts::container::{BindingResolutionError, Container as _};

use super::{
    Binding, Container, EnvironmentQuery, RegisteredBinding, Registry, ServiceKey, parameters,
    scope,
};

type Result<T> = std::result::Result<T, BindingResolutionError>;
type Condition = dyn Fn(&Container) -> Result<bool> + Send + Sync;
pub(super) static BINDING_ATTRIBUTE_FRAME: u8 = 0;

// A condition can bind its own service. Keep that write distinct from a
// concurrent explicit binding, which may have a different recipe.
struct Evaluation {
    registry: usize,
    key: ServiceKey,
    written: Option<Arc<()>>,
}

thread_local! {
    static EVALUATING: RefCell<Vec<Evaluation>> = const { RefCell::new(Vec::new()) };
}

struct EvaluationGuard;
impl EvaluationGuard {
    fn enter(registry: usize, key: ServiceKey) -> Self {
        EVALUATING.with_borrow_mut(|stack| {
            stack.push(Evaluation {
                registry,
                key,
                written: None,
            })
        });
        Self
    }

    fn wrote(&self, entry: &RegisteredBinding) -> bool {
        EVALUATING.with_borrow(|stack| {
            stack
                .last()
                .and_then(|frame| frame.written.as_ref())
                .is_some_and(|version| Arc::ptr_eq(version, &entry.version))
        })
    }
}
impl Drop for EvaluationGuard {
    fn drop(&mut self) {
        EVALUATING.with_borrow_mut(|stack| {
            stack.pop();
        });
    }
}

pub(super) fn record_write(registry: &Registry, key: &ServiceKey, entry: &RegisteredBinding) {
    let registry = std::ptr::from_ref(registry).addr();
    EVALUATING.with_borrow_mut(|stack| {
        if let Some(frame) = stack
            .iter_mut()
            .rev()
            .find(|frame| frame.registry == registry && frame.key == *key)
        {
            frame.written = Some(entry.version.clone());
        }
    });
}

/// A concrete implementation and the environments in which it should be selected.
#[derive(Clone)]
pub struct Bind {
    key: ServiceKey,
    entry: RegisteredBinding,
    environments: Vec<String>,
    wildcard: bool,
}

impl Bind {
    /// Declares the fallback implementation. The projection expresses Rust trait coercion.
    pub fn new<T, C>(project: impl Fn(Arc<C>) -> Arc<T> + Send + Sync + 'static) -> Self
    where
        T: ?Sized + Send + Sync + 'static,
        C: Send + Sync + 'static,
    {
        let key = ServiceKey::typed::<T>();
        let same_class = key == ServiceKey::typed::<C>();
        let project = Arc::new(project);
        let synchronous = project.clone();
        let mut entry = Container::entry(Binding::Factory(Arc::new(move |container| {
            let value = if same_class {
                container.build::<C>()?
            } else {
                container.makeWith::<C>(parameters::current(container))?
            };
            Ok(synchronous(value))
        })));
        let asynchronous = Container::entry(Binding::AsyncFactory(Arc::new(move |container| {
            let project = project.clone();
            Box::pin(async move {
                let value = if same_class {
                    container.buildAsync::<C>().await?
                } else {
                    container
                        .makeWithAsync::<C>(parameters::current(container))
                        .await?
                };
                Ok(project(value))
            })
        })));
        entry.asynchronous_value = Some(asynchronous.value);
        Self {
            key,
            entry,
            environments: vec!["*".into()],
            wildcard: true,
        }
    }

    #[allow(non_snake_case, reason = "Compiled Laravel binding declaration")]
    pub fn forEnvironments(
        mut self,
        environments: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let mut patterns = environments.into_iter().map(Into::into);
        let first = patterns.next();
        let starts_with_wildcard = first.as_deref() == Some("*");
        self.environments = first
            .into_iter()
            .chain(patterns)
            .filter(|pattern| !matches!(pattern.as_str(), "" | "0"))
            .collect();
        self.wildcard = starts_with_wildcard && self.environments.len() == 1;
        self
    }
}

/// A binding whose condition receives the current container.
#[derive(Clone)]
pub struct BindWhen {
    binding: Bind,
    condition: Arc<Condition>,
}

impl BindWhen {
    pub fn new(
        binding: Bind,
        condition: impl Fn(&Container) -> Result<bool> + Send + Sync + 'static,
    ) -> Self {
        Self {
            binding,
            condition: Arc::new(condition),
        }
    }
}

#[derive(Clone)]
pub enum BindingAttribute {
    Bind(Bind),
    BindWhen(BindWhen),
}

impl From<Bind> for BindingAttribute {
    fn from(binding: Bind) -> Self {
        Self::Bind(binding)
    }
}

impl From<BindWhen> for BindingAttribute {
    fn from(binding: BindWhen) -> Self {
        Self::BindWhen(binding)
    }
}

impl BindingAttribute {
    fn binding(&self) -> &Bind {
        match self {
            Self::Bind(binding) => binding,
            Self::BindWhen(conditional) => &conditional.binding,
        }
    }
}

impl Container {
    /// Registers the ordered attributes for a compiled service type.
    #[allow(non_snake_case, reason = "Compiled Laravel binding declaration")]
    pub fn registerBindingAttributes<T: ?Sized + Send + Sync + 'static>(
        &self,
        attributes: Vec<BindingAttribute>,
    ) -> Result<()> {
        let key = ServiceKey::typed::<T>();
        if attributes.is_empty()
            || attributes
                .iter()
                .any(|attribute| attribute.binding().key != key)
        {
            return Err(BindingResolutionError::InvalidConstructor {
                service: key,
                reason: "binding attributes must declare the same service",
            });
        }

        let mut registry = self.registry.write().expect("container lock poisoned");
        if registry.binding_attributes.contains_key(&key) {
            return Err(BindingResolutionError::InvalidConstructor {
                service: key,
                reason: "binding attributes are already declared",
            });
        }
        registry.binding_attributes.insert(key, attributes.into());
        self.has_binding_attributes.store(true, Ordering::Release);
        Ok(())
    }

    pub(super) fn install_attribute_binding(
        &self,
        key: &ServiceKey,
        consumer: Option<&ServiceKey>,
        parameterized: bool,
    ) -> Result<()> {
        let (declarations, resolver, registry_id) = {
            let registry = self.registry.read().expect("container lock poisoned");
            let Some(declarations) = registry.binding_attributes.get(key) else {
                return Ok(());
            };
            if registry.binding_is_bound(self, key)
                || registry.binding_attribute_misses.contains(key)
                || (!parameterized
                    && ((registry.instances.contains_key(key)
                        && registry.root_instance_visible(self, key))
                        || scope::lookup(self, key).is_some()))
                || consumer.is_some_and(|consumer| {
                    registry.contextual_binding(self, consumer, key).is_some()
                })
            {
                return Ok(());
            }
            (
                declarations.clone(),
                registry.environment_resolver.clone(),
                std::ptr::from_ref(&*registry).addr(),
            )
        };

        let _resolution = self.resolve(key, std::ptr::from_ref(&BINDING_ATTRIBUTE_FRAME).addr())?;
        let evaluation = EvaluationGuard::enter(registry_id, key.clone());
        let mut fallback = None;
        let mut selected = None;
        let mut conditional = false;
        for declaration in declarations.iter() {
            match declaration {
                BindingAttribute::BindWhen(binding) => {
                    conditional = true;
                    if (binding.condition)(self)? {
                        selected = Some(&binding.binding.entry);
                        break;
                    }
                }
                BindingAttribute::Bind(binding) => {
                    let Some(resolver) = &resolver else { continue };
                    if binding.environments.is_empty() {
                        return Err(BindingResolutionError::InvalidConstructor {
                            service: key.clone(),
                            reason: "Bind environments must not be empty",
                        });
                    }
                    if binding.wildcard {
                        fallback.get_or_insert(&binding.entry);
                        continue;
                    }
                    let environments: Vec<_> =
                        binding.environments.iter().map(String::as_str).collect();
                    if bool::from(resolver(EnvironmentQuery::Names(&environments))?) {
                        selected = Some(&binding.entry);
                        break;
                    }
                }
            }
        }

        let retired = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            let existing = registry.bindings.get(key);
            if let Some(entry) = selected.or(fallback)
                && (!registry.binding_is_bound(self, key)
                    || existing.is_some_and(|entry| evaluation.wrote(entry)))
            {
                let entry = registry.declared_recipe(key, entry.clone().with_version(Arc::new(())));
                registry.insert_binding(key.clone(), entry)
            } else {
                if existing.is_none() && resolver.is_some() && !conditional {
                    registry.binding_attribute_misses.insert(key.clone());
                }
                None
            }
        };
        drop(retired);
        Ok(())
    }
}

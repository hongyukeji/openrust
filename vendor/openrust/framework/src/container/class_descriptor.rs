//! Static constructor metadata replaces PHP reflection without creating service bindings.
use super::{
    Binding, Container, FactoryFuture, ParameterKey, ParameterValue, RegisteredBinding, ServiceKey,
    scope,
};
use crate::contracts::container::{BindingResolutionError, Container as _};
use std::{any::type_name, collections::HashSet, future::Future, pin::Pin, sync::Arc};

type Result<T> = std::result::Result<T, BindingResolutionError>;
type ValueFuture<'a> = Pin<Box<dyn Future<Output = Result<ParameterValue>> + Send + 'a>>;
type Resolve = dyn Fn(&Container, ServiceKey) -> Result<ParameterValue> + Send + Sync;
type ResolveAsync = dyn for<'a> Fn(&'a Container, ServiceKey) -> ValueFuture<'a> + Send + Sync;

/// One named constructor parameter with an explicit Rust output contract.
#[derive(Clone)]
pub struct ConstructorParam {
    name: &'static str,
    class: Option<ServiceKey>,
    check: fn(&ParameterValue) -> Result<()>,
    resolve: Arc<Resolve>,
    resolve_async: Arc<ResolveAsync>,
    nullable: bool,
    default: Option<Arc<dyn Fn() -> Result<ParameterValue> + Send + Sync>>,
}
impl ConstructorParam {
    pub fn scalar<T: Send + Sync + 'static>(name: &'static str) -> Self {
        Self {
            name,
            class: None,
            check: |value| value.downcast::<T>().map(drop),
            resolve: Arc::new(|c, key| c.get::<T>(key).map(ParameterValue::from_arc)),
            resolve_async: Arc::new(|c, key| {
                Box::pin(async move { c.getAsync::<T>(key).await.map(ParameterValue::from_arc) })
            }),
            nullable: false,
            default: None,
        }
    }
    pub fn class<T: ?Sized + Send + Sync + 'static>(name: &'static str) -> Self {
        Self {
            name,
            class: Some(ServiceKey::typed::<T>()),
            check: |value| value.downcast::<Arc<T>>().map(drop),
            resolve: Arc::new(|c, key| c.get::<T>(key).map(ParameterValue::new)),
            resolve_async: Arc::new(|c, key| {
                Box::pin(async move { c.getAsync::<T>(key).await.map(ParameterValue::new) })
            }),
            nullable: false,
            default: None,
        }
    }
    pub fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }
    #[allow(non_snake_case, reason = "Static reflection adapter")]
    pub fn withDefault(
        mut self,
        default: impl Fn() -> Result<ParameterValue> + Send + Sync + 'static,
    ) -> Self {
        self.default = Some(Arc::new(default));
        self
    }
    pub fn name(&self) -> &'static str {
        self.name
    }
    fn validate(&self, value: &ParameterValue) -> Result<()> {
        if value.is_null() && self.nullable {
            Ok(())
        } else {
            (self.check)(value)
        }
    }
    fn select(&self, c: &Container) -> Result<Selection> {
        if let Some(value) = c
            .getLastParameterOverride()
            .get(&ParameterKey::name(self.name))
        {
            return Ok(Selection::Value(value.clone()));
        }
        if let Some(key) = &self.class {
            if self.default.is_some() && !c.has(key.clone()) && !c.has_contextual_direct(key) {
                return Ok(Selection::Default);
            }
            return Ok(Selection::Service(key.clone()));
        }
        let key = ServiceKey::named(format!("${}", self.name));
        if c.has_contextual(&key)? {
            return Ok(Selection::Service(key));
        }
        if self.default.is_some() {
            return Ok(Selection::Default);
        }
        if self.nullable {
            return Ok(Selection::Value(ParameterValue::null()));
        }
        Err(BindingResolutionError::UnresolvableParameter {
            parameter: self.name,
            service: c.consumer(),
        })
    }
    fn fixed(&self, selection: Selection) -> Result<ParameterValue> {
        match selection {
            Selection::Value(value) => Ok(value),
            Selection::Default => (self.default.as_ref().expect("selected default"))(),
            Selection::Service(_) => unreachable!("service resolution has its own channel"),
        }
    }
}
enum Selection {
    Value(ParameterValue),
    Default,
    Service(ServiceKey),
}

/// Owns validated constructor slots. Values and service references may be retained by the result.
pub struct ResolvedArgs {
    values: Vec<ParameterValue>,
}
impl ResolvedArgs {
    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
    pub fn value(&self, index: usize) -> Result<&ParameterValue> {
        self.values
            .get(index)
            .ok_or(BindingResolutionError::InvalidArgumentSlot { index })
    }
    pub fn get<T: Send + Sync + 'static>(&self, index: usize) -> Result<Arc<T>> {
        self.value(index)?.downcast()
    }
    pub fn service<T: ?Sized + Send + Sync + 'static>(&self, index: usize) -> Result<Arc<T>> {
        self.get::<Arc<T>>(index).map(|value| (*value).clone())
    }
    pub fn optional<T: Send + Sync + 'static>(&self, index: usize) -> Result<Option<Arc<T>>> {
        let value = self.value(index)?;
        if value.is_null() {
            Ok(None)
        } else {
            value.downcast().map(Some)
        }
    }
    #[allow(non_snake_case, reason = "Static reflection adapter")]
    pub fn optionalService<T: ?Sized + Send + Sync + 'static>(
        &self,
        index: usize,
    ) -> Result<Option<Arc<T>>> {
        self.optional::<Arc<T>>(index)
            .map(|value| value.map(|value| (*value).clone()))
    }
}
fn validate_signature(key: &ServiceKey, parameters: &[ConstructorParam]) -> Result<()> {
    let mut names = HashSet::new();
    if parameters
        .iter()
        .any(|p| p.name.is_empty() || !names.insert(p.name))
    {
        return Err(BindingResolutionError::InvalidConstructor {
            service: key.clone(),
            reason: "constructor parameter names must be nonempty and unique",
        });
    }
    Ok(())
}
fn resolve(c: &Container, parameters: &[ConstructorParam]) -> Result<ResolvedArgs> {
    let mut values = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        let value = match parameter.select(c)? {
            Selection::Service(key) => (parameter.resolve)(c, key)?,
            fixed => parameter.fixed(fixed)?,
        };
        values.push(value);
    }
    for (parameter, value) in parameters.iter().zip(&values) {
        parameter.validate(value)?;
    }
    Ok(ResolvedArgs { values })
}
async fn resolve_async(c: &Container, parameters: &[ConstructorParam]) -> Result<ResolvedArgs> {
    let mut values = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        let value = match parameter.select(c)? {
            Selection::Service(key) => (parameter.resolve_async)(c, key).await?,
            fixed => parameter.fixed(fixed)?,
        };
        values.push(value);
    }
    for (parameter, value) in parameters.iter().zip(&values) {
        parameter.validate(value)?;
    }
    Ok(ResolvedArgs { values })
}

/// Immutable metadata for one compiled concrete type; registration does not mark it bound.
#[derive(Clone)]
pub struct ClassDescriptor {
    key: ServiceKey,
    pub(super) sync: RegisteredBinding,
    pub(super) asynchronous: RegisteredBinding,
}
impl ClassDescriptor {
    pub fn concrete<T, F>(parameters: Vec<ConstructorParam>, construct: F) -> Result<Self>
    where
        T: Send + Sync + 'static,
        F: Fn(ResolvedArgs) -> Result<Arc<T>> + Send + Sync + 'static,
    {
        let key = ServiceKey::typed::<T>();
        validate_signature(&key, &parameters)?;
        let (parameters, construct) = (Arc::new(parameters), Arc::new(construct));
        let (signature, new) = (parameters.clone(), construct.clone());
        let sync = Container::entry(Binding::Factory(Arc::new(move |c| {
            new(resolve(c, &signature)?)
        })));
        let asynchronous = Container::entry(Binding::AsyncFactory(Arc::new(move |c| {
            let (signature, new) = (parameters.clone(), construct.clone());
            Box::pin(async move { new(resolve_async(c, &signature).await?) })
        })))
        .with_version(sync.version.clone());
        Ok(Self {
            key,
            sync,
            asynchronous,
        })
    }
    #[allow(non_snake_case, reason = "Asynchronous Rust constructor adapter")]
    pub fn concreteAsync<T, F>(parameters: Vec<ConstructorParam>, construct: F) -> Result<Self>
    where
        T: Send + Sync + 'static,
        F: Fn(ResolvedArgs) -> FactoryFuture<'static, T> + Send + Sync + 'static,
    {
        let key = ServiceKey::typed::<T>();
        validate_signature(&key, &parameters)?;
        let (signature, new) = (Arc::new(parameters), Arc::new(construct));
        let asynchronous = Container::entry(Binding::AsyncFactory(Arc::new(move |c| {
            let (signature, new) = (signature.clone(), new.clone());
            Box::pin(async move { new(resolve_async(c, &signature).await?).await })
        })));
        Ok(Self {
            key,
            sync: asynchronous.clone(),
            asynchronous,
        })
    }
    pub fn key(&self) -> &ServiceKey {
        &self.key
    }
}
impl Container {
    fn has_contextual(&self, key: &ServiceKey) -> Result<bool> {
        let Some(consumer) = self.consumer() else {
            return Ok(false);
        };
        let key = self.getAlias(key.clone())?;
        Ok(self
            .registry
            .read()
            .expect("container lock poisoned")
            .contextual_binding(self, &consumer, &key)
            .is_some())
    }
    fn has_contextual_direct(&self, key: &ServiceKey) -> bool {
        let Some(consumer) = self.consumer() else {
            return false;
        };
        self.registry
            .read()
            .expect("container lock poisoned")
            .contextual
            .get(&consumer)
            .is_some_and(|needs| needs.contains_key(key))
    }
    pub(super) fn register_class(&self, descriptor: ClassDescriptor) -> Result<()> {
        let mut pending = Some(descriptor);
        let result = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            let descriptor = pending.as_ref().expect("pending descriptor");
            if let Some(previous) = registry.classes.get(&descriptor.key) {
                if Arc::ptr_eq(&previous.sync.version, &descriptor.sync.version) {
                    Ok(())
                } else {
                    Err(BindingResolutionError::InvalidConstructor {
                        service: descriptor.key.clone(),
                        reason: "a different constructor is already registered",
                    })
                }
            } else {
                registry.classes.insert(
                    descriptor.key.clone(),
                    pending.take().expect("new descriptor"),
                );
                Ok(())
            }
        };
        drop(pending);
        result
    }
    fn constructor(&self, key: &ServiceKey, asynchronous: bool) -> Result<RegisteredBinding> {
        self.registry
            .read()
            .expect("container lock poisoned")
            .classes
            .get(key)
            .map(|record| {
                if asynchronous {
                    record.asynchronous.clone()
                } else {
                    record.sync.clone()
                }
            })
            .ok_or_else(|| BindingResolutionError::NotBound {
                service: key.clone(),
            })
    }
    pub(super) fn build_class<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        scope::check_open(self)?;
        let key = ServiceKey::typed::<T>();
        let entry = self.constructor(&key, false)?;
        let _frame = self.resolve(&key, Arc::as_ptr(&entry.version).addr())?;
        match entry.value.downcast_ref::<Binding<T>>() {
            Some(Binding::Factory(factory)) => factory(self),
            Some(Binding::AsyncFactory(_)) => {
                Err(BindingResolutionError::AsyncResolutionRequired { service: key })
            }
            _ => Err(BindingResolutionError::TypeMismatch {
                service: key,
                expected: type_name::<T>(),
                actual: entry.output,
            }),
        }
    }
    pub(super) async fn build_class_async<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> Result<Arc<T>> {
        scope::check_open(self)?;
        let key = ServiceKey::typed::<T>();
        let entry = self.constructor(&key, true)?;
        scope::preserve(self, async {
            self.with_resolution(&key, Arc::as_ptr(&entry.version).addr(), async {
                match entry.value.downcast_ref::<Binding<T>>() {
                    Some(Binding::AsyncFactory(factory)) => factory(self).await,
                    _ => Err(BindingResolutionError::TypeMismatch {
                        service: key.clone(),
                        expected: type_name::<T>(),
                        actual: entry.output,
                    }),
                }
            })
            .await
        })
        .await
    }
}

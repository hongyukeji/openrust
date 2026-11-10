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

type ManyFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<ParameterValue>>> + Send + 'a>>;
type ResolveMany = dyn Fn(&Container, ServiceKey) -> Result<Vec<ParameterValue>> + Send + Sync;
type ResolveManyAsync = dyn for<'a> Fn(&'a Container, ServiceKey) -> ManyFuture<'a> + Send + Sync;

/// One named constructor parameter with an explicit Rust output contract.
#[derive(Clone)]
pub struct ConstructorParam {
    name: &'static str,
    class: Option<ServiceKey>,
    check: fn(&ParameterValue) -> Result<()>,
    resolve: Arc<Resolve>,
    resolve_async: Arc<ResolveAsync>,
    variadic: bool,
    many: Arc<ResolveMany>,
    many_async: Arc<ResolveManyAsync>,
    nullable: bool,
    default: Option<Arc<dyn Fn() -> Result<ParameterValue> + Send + Sync>>,
}
impl ConstructorParam {
    pub fn scalar<T: Send + Sync + 'static>(name: &'static str) -> Self {
        Self {
            name,
            class: None,
            check: |value| value.downcast::<T>().map(drop),
            resolve: Arc::new(super::variadic::primitive::<T>),
            resolve_async: Arc::new(|c, key| {
                Box::pin(super::variadic::primitive_async::<T>(c, key))
            }),
            variadic: false,
            many: Arc::new(super::variadic::scalars::<T>),
            many_async: Arc::new(|c, key| Box::pin(super::variadic::scalars_async::<T>(c, key))),
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
            variadic: false,
            many: Arc::new(super::variadic::classes::<T>),
            many_async: Arc::new(|c, key| Box::pin(super::variadic::classes_async::<T>(c, key))),
            nullable: false,
            default: None,
        }
    }
    pub fn variadic(mut self) -> Self {
        self.variadic = true;
        self
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
enum Argument {
    One(ParameterValue),
    Many(Vec<ParameterValue>),
}
pub struct ResolvedArgs {
    values: Vec<Argument>,
    variadic: Vec<bool>,
}
impl ResolvedArgs {
    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
    pub fn value(&self, index: usize) -> Result<&ParameterValue> {
        match self.values.get(index) {
            Some(Argument::One(value)) if !self.variadic[index] => Ok(value),
            Some(_) => Err(BindingResolutionError::InvalidArgumentShape { index }),
            None => Err(BindingResolutionError::InvalidArgumentSlot { index }),
        }
    }
    /// Reads the ordered values of a variadic declaration, including an explicit single override.
    pub fn values(&self, index: usize) -> Result<&[ParameterValue]> {
        match self.values.get(index) {
            Some(argument) if self.variadic[index] => Ok(match argument {
                Argument::One(value) => std::slice::from_ref(value),
                Argument::Many(values) => values,
            }),
            Some(_) => Err(BindingResolutionError::InvalidArgumentShape { index }),
            None => Err(BindingResolutionError::InvalidArgumentSlot { index }),
        }
    }
    pub fn services<T: ?Sized + Send + Sync + 'static>(&self, index: usize) -> Result<Vec<Arc<T>>> {
        self.values(index)?
            .iter()
            .map(|value| value.downcast::<Arc<T>>().map(|value| (*value).clone()))
            .collect()
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
    if parameters
        .iter()
        .enumerate()
        .any(|(index, p)| p.variadic && (index + 1 != parameters.len() || p.default.is_some()))
    {
        return Err(BindingResolutionError::InvalidConstructor {
            service: key.clone(),
            reason: "a variadic parameter must be last and cannot have a default",
        });
    }
    Ok(())
}
fn validate_arguments(
    parameters: &[ConstructorParam],
    values: Vec<Argument>,
) -> Result<ResolvedArgs> {
    for (parameter, argument) in parameters.iter().zip(&values) {
        match argument {
            Argument::One(value) => parameter.validate(value)?,
            Argument::Many(values) => {
                for value in values {
                    parameter.validate(value)?;
                }
            }
        }
    }
    Ok(ResolvedArgs {
        values,
        variadic: parameters.iter().map(|p| p.variadic).collect(),
    })
}
fn resolve(c: &Container, parameters: &[ConstructorParam]) -> Result<ResolvedArgs> {
    let mut values = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        let argument = if parameter.variadic {
            if let Some(value) = c
                .getLastParameterOverride()
                .get(&ParameterKey::name(parameter.name))
            {
                Argument::One(value.clone())
            } else {
                Argument::Many((parameter.many)(
                    c,
                    parameter
                        .class
                        .clone()
                        .unwrap_or_else(|| ServiceKey::named(format!("${}", parameter.name))),
                )?)
            }
        } else {
            Argument::One(match parameter.select(c)? {
                Selection::Service(key) => (parameter.resolve)(c, key)?,
                fixed => parameter.fixed(fixed)?,
            })
        };
        values.push(argument);
    }
    validate_arguments(parameters, values)
}
async fn resolve_async(c: &Container, parameters: &[ConstructorParam]) -> Result<ResolvedArgs> {
    let mut values = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        let argument = if parameter.variadic {
            if let Some(value) = c
                .getLastParameterOverride()
                .get(&ParameterKey::name(parameter.name))
            {
                Argument::One(value.clone())
            } else {
                Argument::Many(
                    (parameter.many_async)(
                        c,
                        parameter
                            .class
                            .clone()
                            .unwrap_or_else(|| ServiceKey::named(format!("${}", parameter.name))),
                    )
                    .await?,
                )
            }
        } else {
            Argument::One(match parameter.select(c)? {
                Selection::Service(key) => (parameter.resolve_async)(c, key).await?,
                fixed => parameter.fixed(fixed)?,
            })
        };
        values.push(argument);
    }
    validate_arguments(parameters, values)
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

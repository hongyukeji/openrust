//! Static constructor metadata replaces PHP reflection without creating service bindings.
use super::{
    AttributeParameter, AttributeValue, Binding, ClassAttribute, Container, FactoryFuture,
    ParameterKey, ParameterOverrides, ParameterValue, RegisteredBinding, ServiceKey, StaticCall,
    method_call::RegisteredMethod, scope,
};
use crate::contracts::container::{BindingResolutionError, Container as _, SelfBuilding};
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
    pub(super) name: &'static str,
    attributes: Vec<ClassAttribute>,
    pub(super) class: Option<ServiceKey>,
    check: fn(&ParameterValue) -> Result<()>,
    resolve: Arc<Resolve>,
    pub(super) resolve_async: Arc<ResolveAsync>,
    pub(super) variadic: bool,
    many: Arc<ResolveMany>,
    many_async: Arc<ResolveManyAsync>,
    pub(super) call_many: Arc<ResolveManyAsync>,
    nullable: bool,
    pub(super) default: Option<Arc<dyn Fn() -> Result<ParameterValue> + Send + Sync>>,
}
impl ConstructorParam {
    pub fn scalar<T: Send + Sync + 'static>(name: &'static str) -> Self {
        Self {
            name,
            attributes: Vec::new(),
            class: None,
            check: |value| value.downcast::<T>().map(drop),
            resolve: Arc::new(super::variadic::primitive::<T>),
            resolve_async: Arc::new(|c, key| {
                Box::pin(super::variadic::primitive_async::<T>(c, key))
            }),
            variadic: false,
            call_many: Arc::new(|_, _| Box::pin(async { Ok(Vec::new()) })),
            many: Arc::new(super::variadic::scalars::<T>),
            many_async: Arc::new(|c, key| Box::pin(super::variadic::scalars_async::<T>(c, key))),
            nullable: false,
            default: None,
        }
    }
    pub fn class<T: ?Sized + Send + Sync + 'static>(name: &'static str) -> Self {
        Self {
            name,
            attributes: Vec::new(),
            class: Some(ServiceKey::typed::<T>()),
            check: |value| value.downcast::<Arc<T>>().map(drop),
            resolve: Arc::new(|c, key| c.get::<T>(key).map(ParameterValue::new)),
            resolve_async: Arc::new(|c, key| {
                Box::pin(async move { c.getAsync::<T>(key).await.map(ParameterValue::new) })
            }),
            variadic: false,
            call_many: Arc::new(|c, key| {
                Box::pin(super::variadic::method_classes_async::<T>(c, key))
            }),
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
    #[allow(non_snake_case, reason = "Compiled attribute declaration")]
    pub fn withAttribute(mut self, attribute: ClassAttribute) -> Self {
        self.attributes.push(attribute);
        self
    }

    fn attribute_parameter(&self) -> AttributeParameter {
        AttributeParameter {
            name: self.name,
            variadic: self.variadic,
        }
    }

    pub(super) fn attribute_value(&self, container: &Container) -> Result<Option<Argument>> {
        self.attributes
            .iter()
            .find(|attribute| attribute.is_contextual())
            .map(|attribute| container.resolveFromAttribute(attribute, self.attribute_parameter()))
            .transpose()
    }

    pub(super) fn observe(&self, container: &Container, value: &Argument) -> Result<()> {
        let service = match value {
            Argument::One(value) if !value.is_null() => self.class.as_ref(),
            _ => None,
        };
        container.fire_attribute_callbacks(
            &self.attributes,
            value,
            Some(self.attribute_parameter()),
            service,
        )
    }

    fn override_value(&self, container: &Container) -> Option<ParameterValue> {
        container
            .getLastParameterOverride()
            .get(&ParameterKey::name(self.name))
            .cloned()
    }

    fn constructor_attribute(&self, container: &Container) -> Result<Option<Argument>> {
        match self.attribute_value(container)? {
            Some(Argument::One(value)) if value.is_null() => Ok(None),
            Some(Argument::Many(_)) if !self.variadic => {
                Err(BindingResolutionError::InvalidAttributeArguments {
                    parameter: self.name,
                })
            }
            result => Ok(result),
        }
    }

    fn variadic_key(&self) -> ServiceKey {
        self.class
            .clone()
            .unwrap_or_else(|| ServiceKey::named(format!("${}", self.name)))
    }

    fn validate(&self, value: &ParameterValue) -> Result<()> {
        if value.is_null() && self.nullable {
            Ok(())
        } else {
            (self.check)(value)
        }
    }
    fn select(&self, c: &Container) -> Result<Selection> {
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
pub(super) type Argument = AttributeValue;
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
pub(super) fn validate_signature(key: &ServiceKey, parameters: &[ConstructorParam]) -> Result<()> {
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
pub(super) fn validate_arguments(
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
        if let Some(value) = parameter.override_value(c) {
            values.push(Argument::One(value));
            continue;
        }
        let argument = if let Some(value) = parameter.constructor_attribute(c)? {
            value
        } else if parameter.variadic {
            Argument::Many((parameter.many)(c, parameter.variadic_key())?)
        } else {
            Argument::One(match parameter.select(c)? {
                Selection::Service(key) => (parameter.resolve)(c, key)?,
                fixed => parameter.fixed(fixed)?,
            })
        };
        parameter.observe(c, &argument)?;
        values.push(argument);
    }
    validate_arguments(parameters, values)
}

async fn resolve_async(c: &Container, parameters: &[ConstructorParam]) -> Result<ResolvedArgs> {
    let mut values = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        if let Some(value) = parameter.override_value(c) {
            values.push(Argument::One(value));
            continue;
        }
        let argument = if let Some(value) = parameter.constructor_attribute(c)? {
            value
        } else if parameter.variadic {
            Argument::Many((parameter.many_async)(c, parameter.variadic_key()).await?)
        } else {
            Argument::One(match parameter.select(c)? {
                Selection::Service(key) => (parameter.resolve_async)(c, key).await?,
                fixed => parameter.fixed(fixed)?,
            })
        };
        parameter.observe(c, &argument)?;
        values.push(argument);
    }
    validate_arguments(parameters, values)
}

/// Immutable metadata for one compiled concrete type; registration does not mark it bound.
#[derive(Clone)]
pub struct ClassDescriptor {
    key: ServiceKey,
    sync: RegisteredBinding,
    asynchronous: RegisteredBinding,
    self_building: Option<SelfBuildingDescriptor>,
}
#[derive(Clone)]
struct SelfBuildingDescriptor {
    entry: RegisteredBinding,
    method: RegisteredMethod,
}
pub(super) const NEW_INSTANCE: &str = "newInstance";
static SELF_BUILDING_FRAME: u8 = 0;

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
            self_building: None,
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
            self_building: None,
        })
    }

    /// Declares an ordinary constructor and an independent static newInstance signature.
    #[allow(non_snake_case, reason = "Matches Laravel's SelfBuilding name")]
    pub fn selfBuilding<T: SelfBuilding>(
        parameters: Vec<ConstructorParam>,
        construct: impl Fn(ResolvedArgs) -> Result<Arc<T>> + Send + Sync + 'static,
        method_parameters: Vec<ConstructorParam>,
    ) -> Result<Self> {
        let mut descriptor = Self::concrete(parameters, construct)?;
        validate_signature(&descriptor.key, &method_parameters)?;
        let key = descriptor.key.clone();
        let entry = Container::entry(Binding::<T>::AsyncFactory(Arc::new(move |container| {
            let key = key.clone();
            Box::pin(async move {
                container
                    .with_resolution(
                        &key,
                        std::ptr::from_ref(&SELF_BUILDING_FRAME).addr(),
                        container.call(
                            StaticCall::<T>::array(NEW_INSTANCE),
                            ParameterOverrides::new(),
                        ),
                    )
                    .await
            })
        })));
        let method = RegisteredMethod::static_declaration::<T, Arc<T>>(
            method_parameters,
            |args, container| T::newInstance(args.resolved, container),
        );
        descriptor.self_building = Some(SelfBuildingDescriptor { entry, method });
        Ok(descriptor)
    }

    /// Attaches an attribute to objects produced by this class descriptor.
    #[allow(non_snake_case, reason = "Compiled attribute declaration")]
    pub fn withAttribute(mut self, attribute: ClassAttribute) -> Self {
        let mut attributes = self.sync.class_attributes.clone().unwrap_or_default();
        Arc::make_mut(&mut attributes).push(attribute);
        self.sync.class_attributes = Some(attributes.clone());
        self.asynchronous.class_attributes = Some(attributes.clone());
        if let Some(builder) = &mut self.self_building {
            builder.entry.class_attributes = Some(attributes);
        }
        self
    }

    pub fn key(&self) -> &ServiceKey {
        &self.key
    }

    pub(super) fn new_instance_method(&self) -> Option<&RegisteredMethod> {
        self.self_building.as_ref().map(|builder| &builder.method)
    }

    fn building_instance(&self, container: &Container) -> bool {
        container.is_resolving(&self.key, std::ptr::from_ref(&SELF_BUILDING_FRAME).addr())
    }

    pub(super) fn reenters_binding(
        &self,
        container: &Container,
        binding: &RegisteredBinding,
    ) -> bool {
        self.building_instance(container)
            && container.is_resolving(&self.key, Arc::as_ptr(&binding.version).addr())
    }

    pub(super) fn entry(&self, container: &Container, asynchronous: bool) -> &RegisteredBinding {
        if let Some(builder) = &self.self_building
            && !self.building_instance(container)
            && !container.is_resolving(&self.key, Arc::as_ptr(&self.sync.version).addr())
        {
            return &builder.entry;
        }
        if asynchronous {
            &self.asynchronous
        } else {
            &self.sync
        }
    }

    pub(super) fn owns_version(&self, entry: &RegisteredBinding) -> bool {
        Arc::ptr_eq(&self.sync.version, &entry.version)
            || self
                .self_building
                .as_ref()
                .is_some_and(|builder| Arc::ptr_eq(&builder.entry.version, &entry.version))
    }

    pub(super) fn owns_entry(&self, entry: &RegisteredBinding) -> bool {
        Arc::ptr_eq(&self.sync.value, &entry.value)
            || Arc::ptr_eq(&self.asynchronous.value, &entry.value)
            || self
                .self_building
                .as_ref()
                .is_some_and(|builder| Arc::ptr_eq(&builder.entry.value, &entry.value))
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
                if Arc::ptr_eq(&previous.sync.version, &descriptor.sync.version)
                    && match (
                        &previous.sync.class_attributes,
                        &descriptor.sync.class_attributes,
                    ) {
                        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                        (None, None) => true,
                        _ => false,
                    }
                {
                    Ok(())
                } else {
                    Err(BindingResolutionError::InvalidConstructor {
                        service: descriptor.key.clone(),
                        reason: "a different constructor is already registered",
                    })
                }
            } else if descriptor.self_building.is_some()
                && registry.methods.contains_key(&(
                    descriptor.key.clone(),
                    Arc::from(NEW_INSTANCE.to_ascii_lowercase()),
                ))
            {
                Err(BindingResolutionError::InvalidMethod {
                    service: descriptor.key.clone(),
                    method: Arc::from(NEW_INSTANCE),
                    reason: "newInstance already has a method declaration",
                })
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
            .map(|record| record.entry(self, asynchronous).clone())
            .ok_or_else(|| BindingResolutionError::NotBound {
                service: key.clone(),
            })
    }
    pub(super) fn build_class<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        scope::check_open(self)?;
        let key = ServiceKey::typed::<T>();
        let entry = self.constructor(&key, false)?;
        let frame = self.resolve(&key, Arc::as_ptr(&entry.version).addr())?;
        let value = match entry.value.downcast_ref::<Binding<T>>() {
            Some(Binding::Factory(factory)) => factory(self),
            Some(Binding::AsyncFactory(_)) => {
                Err(BindingResolutionError::AsyncResolutionRequired {
                    service: key.clone(),
                })
            }
            _ => Err(BindingResolutionError::TypeMismatch {
                service: key.clone(),
                expected: type_name::<T>(),
                actual: entry.output,
            }),
        }?;
        drop(frame);
        self.after_class(&key, &entry, &value)?;
        Ok(value)
    }
    pub(super) async fn build_class_async<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> Result<Arc<T>> {
        scope::check_open(self)?;
        let key = ServiceKey::typed::<T>();
        let entry = self.constructor(&key, true)?;
        scope::preserve(self, async {
            let value = self
                .with_resolution(&key, Arc::as_ptr(&entry.version).addr(), async {
                    match entry.value.downcast_ref::<Binding<T>>() {
                        Some(Binding::AsyncFactory(factory)) => factory(self).await,
                        _ => Err(BindingResolutionError::TypeMismatch {
                            service: key.clone(),
                            expected: type_name::<T>(),
                            actual: entry.output,
                        }),
                    }
                })
                .await?;
            scope::check_open(self)?;
            self.after_class(&key, &entry, &value)?;
            Ok(value)
        })
        .await
    }

    pub(super) fn after_class<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: &ServiceKey,
        entry: &RegisteredBinding,
        value: &Arc<T>,
    ) -> Result<()> {
        if let Some(attributes) = &entry.class_attributes {
            self.fire_attribute_callbacks(
                attributes,
                &AttributeValue::One(ParameterValue::new(value.clone())),
                None,
                Some(key),
            )?;
        }
        Ok(())
    }
}

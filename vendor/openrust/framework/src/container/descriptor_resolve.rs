//! 描述符缺项沿真实 Resolver 构造，并由原 transient 索引跟踪。

use super::{
    ActiveFactory, Arc, AttributeEvent, AttributeParameter, AttributeValue, BoxFuture,
    ClassAttribute, ClassBuildFrame, Error, Inner, Ordering, ResolveRequest, Resolver, Result,
    ServiceKey, State, Value,
    class_descriptor::{
        Argument, ClassKind, ConstructorParam, ParameterKind, PreparedConstructor,
        SelfBuildingMethod,
    },
    container_error,
    contextual::ContextualImplementation,
    descriptor_source::DescriptorSource,
    extensions::DescriptorAttempt,
    is_unresolvable_dependency, lock,
    parameters::ParameterValue,
    resolving_hooks::object_matches,
    unresolvable_dependency,
};
use crate::foundation::resources::{ResourceInlineOwner, ResourceInlineSlot, ResourceTaskFailure};
#[path = "call.rs"]
mod call;
pub use call::{CallArguments, StaticCall};
#[path = "method_call.rs"]
mod method_call;
pub use method_call::MethodCall;
pub(super) use method_call::{MethodConsumer, MethodRegistry};

use std::{
    future::{Future, poll_fn},
    task::Poll,
};

enum SelectedArgument {
    Value(ResourceInlineSlot<ParameterValue>),
    Many(Vec<ResourceInlineSlot<ParameterValue>>),
}

impl SelectedArgument {
    fn view(&mut self) -> Result<Argument> {
        match self {
            Self::Value(slot) => slot
                .get_mut()
                .map(|value| Argument::One(value.clone()))
                .ok_or_else(missing_slot),
            Self::Many(slots) => slots
                .iter_mut()
                .map(|slot| {
                    slot.get_mut()
                        .map(|value| value.clone())
                        .ok_or_else(missing_slot)
                })
                .collect::<Result<Vec<_>>>()
                .map(Argument::Many),
        }
    }

    fn from_attribute(owner: &ResourceInlineOwner, value: AttributeValue) -> Result<Self> {
        match value {
            AttributeValue::One(value) => owner.slot(value).map(Self::Value),
            AttributeValue::Many(values) => {
                let mut values = owner.slot(values)?;
                let mut selected = Vec::new();
                while let Some(value) = values
                    .get_mut()
                    .and_then(super::AttributeArguments::pop_front)
                {
                    selected.push(owner.slot(value)?);
                }
                Ok(Self::Many(selected))
            }
        }
    }

    fn attribute_view(&mut self) -> Result<AttributeValue> {
        self.view().map(|value| match value {
            Argument::One(value) => AttributeValue::One(value),
            Argument::Many(values) => AttributeValue::many(values),
        })
    }
}

struct DescriptorRun<'a> {
    caller: &'a Resolver,
    inner: &'a Arc<Inner>,
    source: &'a DescriptorSource,
    attempt: &'a DescriptorAttempt,
    child: Resolver,
    events: bool,
    ordinary_reentry: bool,
    started: std::sync::atomic::AtomicBool,
}

impl Resolver {
    /// 直接构造具体编译类，沿当前作用域和构造覆盖栈解析依赖。
    ///
    /// 不查找类的显式绑定或共享缓存。类属性照常运行。
    pub async fn build_class<T: Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        let container = self.container()?;
        let key = ServiceKey::typed::<T>();
        let source = lock(&container.inner.state).direct_class_source(key.clone(), self)?;
        let value = self
            .resolve_descriptor(
                &container.inner,
                &source,
                &key,
                std::any::TypeId::of::<Arc<T>>(),
                false,
            )
            .await?
            .ok_or_else(replaced)?;
        value.downcast_ref::<Arc<T>>().cloned().ok_or_else(|| {
            container_error(
                "service_type_mismatch",
                "编译类直接构造的输出不符合声明类型",
            )
        })
    }

    pub(super) async fn resolve_descriptor(
        &self,
        inner: &Arc<Inner>,
        source: &DescriptorSource,
        output: &ServiceKey,
        erased_output: std::any::TypeId,
        events: bool,
    ) -> Result<Option<Value>> {
        let prepared = source.prepare(output);
        #[cfg(test)]
        super::descriptor_race_tests::after_prepare()?;
        if source
            .claim(&mut lock(&inner.state), self, |_| ())?
            .is_none()
        {
            return Ok(None);
        }
        let prepared = prepared?;
        let same_class = self.class_build(source.key());
        let cyclic = if source.decorates() {
            self.chain
                .iter()
                .any(|(key, active)| key == source.key() && active.load(Ordering::Acquire))
        } else {
            same_class.is_some()
        };
        let ordinary_reentry = same_class.is_some_and(|frame| frame.self_building);
        if cyclic && !ordinary_reentry {
            return Err(container_error(
                "service_cycle",
                self.dependency_path(source.key()),
            ));
        }
        let Some(attempt) = DescriptorAttempt::admit(inner, self, source)? else {
            return Ok(None);
        };
        let mut child = attempt.child(self)?;
        if ordinary_reentry {
            child.suppress_self_building_reentry(source.key());
        }
        let run = DescriptorRun {
            caller: self,
            inner,
            source,
            attempt: &attempt,
            child,
            events,
            ordinary_reentry,
            started: std::sync::atomic::AtomicBool::new(false),
        };
        let owner = attempt.owner()?;
        let result = run.execute(prepared, erased_output).await;
        // execute 的 future 和局部受控槽均已退役，Attempt 仍处于活动状态。
        match result {
            Ok(value) => Ok(Some(value)),
            Err(primary) => match owner.check_cleanup() {
                Err(cleanup) => Err(ResourceTaskFailure::error(primary, cleanup)),
                Ok(())
                    if !run.started.load(Ordering::Relaxed)
                        && primary.code() == "binding_replaced" =>
                {
                    Ok(None)
                }
                Ok(()) => Err(primary),
            },
        }
    }

    fn descriptor_dependency(
        &self,
        lookup: ServiceKey,
        target: ServiceKey,
        erased_output: std::any::TypeId,
    ) -> BoxFuture<'_, Result<Value>> {
        let child = self.clone().for_new_lookup();
        Box::pin(async move {
            child
                .resolve_projected(
                    ResolveRequest::Typed {
                        lookup: Some(lookup),
                        target,
                        erased_output,
                    },
                    None,
                )
                .await
        })
    }
}

struct DescriptorGuard<'a> {
    source: &'a DescriptorSource,
    attempt: &'a DescriptorAttempt,
    started: &'a std::sync::atomic::AtomicBool,
}

struct ArgumentRun<'a> {
    caller: &'a Resolver,
    inner: &'a Arc<Inner>,
    owner: &'a ResourceInlineOwner,
    consumer_key: Option<&'a ServiceKey>,
    descriptor: Option<DescriptorGuard<'a>>,
}

impl ArgumentRun<'_> {
    fn claim<T>(&self, action: impl FnOnce(&mut State) -> T) -> Result<T> {
        let owner = self.owner;
        let Some(descriptor) = &self.descriptor else {
            let mut state = lock(&self.inner.state);
            self.caller.check_open(&state)?;
            owner.check_cleanup()?;
            return self
                .caller
                .resources()?
                .with_open_owner(|| action(&mut state));
        };
        let claimed = descriptor
            .source
            .claim(&mut lock(&self.inner.state), self.caller, |state| {
                owner.check_cleanup()?;
                Ok::<Option<T>, Error>(
                    descriptor
                        .attempt
                        .active(state, descriptor.source)
                        .then(|| action(state)),
                )
            })?
            .ok_or_else(replaced)??;
        claimed.ok_or_else(replaced)
    }

    fn start(&self) -> Result<()> {
        self.claim(|_| {
            if let Some(descriptor) = &self.descriptor {
                descriptor.started.store(true, Ordering::Relaxed);
            }
        })
    }

    async fn poll_stage<T, F>(&self, future: F) -> Result<ResourceInlineSlot<T>>
    where
        F: Future<Output = Result<T>>,
    {
        let owner = self.owner;
        let mut future = owner.slot(Box::pin(future))?;
        let polled = poll_fn(|cx| {
            // 已开始的阶段可以在换源后返回；真实 scope/owner 关闭在下一次 poll 观察。
            let open = self
                .caller
                .check_open(&lock(&self.inner.state))
                .and_then(|()| {
                    if let Some(descriptor) = &self.descriptor {
                        descriptor.source.owner.with_open_owner(|| ())
                    } else {
                        self.caller.resources()?.with_open_owner(|| ())
                    }
                });
            if let Err(error) = open {
                return Poll::Ready(Err(error));
            }
            match owner.run(|| match future.get_mut() {
                Some(future) => future.as_mut().poll(cx),
                None => Poll::Ready(Err(missing_slot())),
            }) {
                Ok(Poll::Pending) => match owner.check_cleanup() {
                    Ok(()) => Poll::Pending,
                    Err(error) => Poll::Ready(Err(error)),
                },
                Ok(ready) => ready,
                Err(error) => Poll::Ready(Err(error)),
            }
        });
        let termination = async {
            if let Some(descriptor) = &self.descriptor {
                descriptor.attempt.termination.wait_closed().await;
            } else {
                std::future::pending::<()>().await;
            }
        };
        let output = tokio::select! {
            result = polled => result,
            () = self.caller.closed.wait_closed() => Err(lock(&self.inner.state).resolution_closure_error()),
            () = termination => Err(container_error("container_closed", "容器已关闭")),
        };
        // Ready 值须先入自己的槽，原 Future 的清理失败时仍有真实退役责任。
        let value = output.and_then(|value| owner.slot(value));
        future.clear();
        let value = value?;
        owner.check_cleanup()?;
        Ok(value)
    }

    fn select_ordinary_value(&self, parameter: &ConstructorParam) -> Result<SelectedArgument> {
        if parameter.has_default() {
            self.default_value(parameter)
        } else if parameter.is_variadic() {
            Ok(SelectedArgument::Many(Vec::new()))
        } else if parameter.allows_null() {
            self.owner
                .slot(ParameterValue::null())
                .map(SelectedArgument::Value)
        } else {
            Err(unresolvable_dependency(
                "container_parameter_missing",
                format!("无法解析构造参数 `{}`", parameter.name()),
            ))
        }
    }

    fn default_value(&self, parameter: &ConstructorParam) -> Result<SelectedArgument> {
        self.start()?;
        let owner = self.owner;
        let value = owner
            .run(|| parameter.evaluate_default())??
            .ok_or_else(missing_slot)?;
        owner.slot(value).map(SelectedArgument::Value)
    }

    async fn select_argument(
        &self,
        consumer: &Resolver,
        parameter: &ConstructorParam,
    ) -> Result<SelectedArgument> {
        let owner = self.owner;
        if let Some(value) = self
            .caller
            .parameters
            .as_ref()
            .and_then(|frame| frame.get_named(parameter.name()))
        {
            let mut value = owner.slot(value)?;
            parameter.validate_selected(value.get_mut().ok_or_else(missing_slot)?)?;
            return Ok(SelectedArgument::Value(value));
        }
        let selected = self.select_attribute(consumer, parameter, false).await?;
        let from_attribute = selected.is_some();
        let scalar_variadic =
            parameter.is_variadic() && matches!(&selected, Some(SelectedArgument::Value(_)));
        let mut selected = match selected {
            Some(selected) => selected,
            None => self.select_ordinary_argument(consumer, parameter).await?,
        };
        if !parameter.attributes().is_empty() {
            self.dispatch_attributes(consumer, parameter.attributes(), selected.attribute_view()?)
                .await?;
        }
        if scalar_variadic {
            return Err(container_error(
                "container_constructor_argument_shape",
                "构造变参的属性必须返回参数列表",
            ));
        }
        if from_attribute {
            self.adapt_attribute_argument(parameter, &mut selected)?;
        }
        Ok(selected)
    }

    async fn select_attribute(
        &self,
        consumer: &Resolver,
        parameter: &ConstructorParam,
        method: bool,
    ) -> Result<Option<SelectedArgument>> {
        let Some(attribute) = parameter
            .attributes()
            .iter()
            .find(|attribute| attribute.is_contextual())
        else {
            return Ok(None);
        };
        self.start()?;
        let owner = self.owner;
        let handler = self
            .claim(|state| state.attributes.handler(attribute.key()))?
            .or_else(|| attribute.resolver())
            .ok_or_else(|| {
                container_error(
                    "container_attribute_handler_missing",
                    "上下文属性没有处理器",
                )
            })?;
        let mut handler = owner.slot(handler)?;
        let mut instance = owner.slot(owner.run(|| attribute.instantiate())??)?;
        let stage = owner.run(|| {
            Ok::<_, Error>(
                Arc::clone(handler.get_mut().ok_or_else(missing_slot)?).invoke(
                    instance.get_mut().ok_or_else(missing_slot)?.clone(),
                    AttributeParameter::new(parameter.name(), parameter.is_variadic(), method),
                    consumer.clone(),
                ),
            )
        })??;
        let mut value = self.poll_stage(stage).await?;
        instance.clear();
        handler.clear();
        owner.check_cleanup()?;
        match value.take().ok_or_else(missing_slot)? {
            AttributeValue::One(value) if value.is_null() && !method => Ok(None),
            AttributeValue::Many(_) if method => Err(container_error(
                "container_constructor_argument_shape",
                "方法属性返回数组时应作为一个参数值声明",
            )),
            value => SelectedArgument::from_attribute(owner, value).map(Some),
        }
    }

    async fn select_method_argument(
        &self,
        consumer: &Resolver,
        parameter: &ConstructorParam,
        mut explicit: Option<&mut call::CallParameters>,
    ) -> Result<SelectedArgument> {
        let named = explicit
            .as_deref_mut()
            .and_then(|parameters| parameters.take_named(parameter.name()));
        let mut selected = if let Some(value) = named {
            SelectedArgument::Value(value)
        } else if let Some(value) = self.select_attribute(consumer, parameter, true).await? {
            value
        } else if let ParameterKind::Class {
            key, erased_output, ..
        } = parameter.kind()
        {
            if let Some(value) = explicit.and_then(|parameters| parameters.take_named(key.name())) {
                SelectedArgument::Value(value)
            } else {
                self.select_method_class(consumer, parameter, key, *erased_output)
                    .await?
            }
        } else if parameter.has_default() {
            self.default_value(parameter)?
        } else if parameter.is_variadic() {
            SelectedArgument::Many(Vec::new())
        } else {
            return Err(unresolvable_dependency(
                "container_parameter_missing",
                format!("无法解析方法参数 `{}`", parameter.name()),
            ));
        };
        if !parameter.attributes().is_empty() {
            match selected.attribute_view()? {
                AttributeValue::One(value) => {
                    self.dispatch_attributes(
                        consumer,
                        parameter.attributes(),
                        AttributeValue::One(value),
                    )
                    .await?;
                }
                AttributeValue::Many(values) => {
                    let mut values = self.owner.slot(values)?;
                    while let Some(value) = values
                        .get_mut()
                        .and_then(super::AttributeArguments::pop_front)
                    {
                        self.dispatch_attributes(
                            consumer,
                            parameter.attributes(),
                            AttributeValue::One(value),
                        )
                        .await?;
                    }
                }
            }
        }
        self.adapt_attribute_argument(parameter, &mut selected)?;
        Ok(selected)
    }

    fn adapt_attribute_argument(
        &self,
        parameter: &ConstructorParam,
        argument: &mut SelectedArgument,
    ) -> Result<()> {
        let ParameterKind::Class {
            key, erased_output, ..
        } = parameter.kind()
        else {
            return Ok(());
        };
        match argument {
            SelectedArgument::Value(value) => {
                self.project_attribute_service(value, key, *erased_output)
            }
            SelectedArgument::Many(values) => values
                .iter_mut()
                .try_for_each(|value| self.project_attribute_service(value, key, *erased_output)),
        }
    }

    fn project_attribute_service(
        &self,
        slot: &mut ResourceInlineSlot<ParameterValue>,
        target: &ServiceKey,
        erased_output: std::any::TypeId,
    ) -> Result<()> {
        let owner = self.owner;
        let mut candidate = slot.get_mut().ok_or_else(missing_slot)?.service_value();
        while let Some(value) = candidate {
            if value.as_ref().type_id() == erased_output {
                let projected = owner.slot(ParameterValue::from_erased(Arc::clone(value)))?;
                *slot = projected;
                return owner.check_cleanup();
            }
            if let Some(projection) = self.claim(|state| {
                state
                    .classes
                    .output_projection(value.as_ref().type_id(), target)
                    .cloned()
            })? {
                let projected = owner.run(|| projection.project(value))??;
                let projected = owner.slot(ParameterValue::from_erased(projected))?;
                *slot = projected;
                return owner.check_cleanup();
            }
            candidate = value.origin();
        }
        Ok(())
    }

    async fn select_method_class(
        &self,
        consumer: &Resolver,
        parameter: &ConstructorParam,
        key: &ServiceKey,
        erased_output: std::any::TypeId,
    ) -> Result<SelectedArgument> {
        if parameter.has_default()
            && !self.claim(|state| state.bound_key_at(key, consumer.scope))?
        {
            return self.default_value(parameter);
        }
        self.start()?;
        if parameter.is_variadic() {
            let contextual = self.claim(|state| {
                let key = state.aliases.resolve(&state.classes.canonical_key(key))?;
                Ok::<_, Error>(
                    self.consumer_key
                        .and_then(|class| state.contextual_implementation(class, &key))
                        .cloned(),
                )
            })??;
            if let Some(contextual) = contextual
                && let Some(values) = self
                    .resolve_many(consumer, contextual, key, erased_output)
                    .await?
            {
                return Ok(SelectedArgument::Many(values));
            }
        }
        self.resolve_argument(consumer, key.clone(), key, erased_output)
            .await
            .map(SelectedArgument::Value)
    }

    async fn dispatch_attributes(
        &self,
        resolver: &Resolver,
        attributes: &[ClassAttribute],
        value: AttributeValue,
    ) -> Result<()> {
        let owner = self.owner;
        let mut value = owner.slot(value)?;
        for attribute in attributes {
            if let Some(callback) = attribute.after() {
                self.invoke_attribute_after(resolver, attribute, &mut value, callback)
                    .await?;
            }
            let object = match value.get_mut().ok_or_else(missing_slot)? {
                AttributeValue::One(value) => value.service_value(),
                AttributeValue::Many(_) => None,
            };
            let callbacks = self.claim(|state| {
                state.attributes.matching(attribute.key(), |key| {
                    object_matches(&state.classes, object, key)
                })
            })?;
            let mut callbacks = owner.slot(callbacks.into_iter())?;
            while let Some(callback) = callbacks.get_mut().and_then(Iterator::next) {
                self.invoke_attribute_after(resolver, attribute, &mut value, callback)
                    .await?;
            }
        }
        value.clear();
        owner.check_cleanup()
    }

    async fn invoke_attribute_after(
        &self,
        resolver: &Resolver,
        attribute: &ClassAttribute,
        value: &mut ResourceInlineSlot<AttributeValue>,
        callback: Arc<super::attributes::AttributeAfter>,
    ) -> Result<()> {
        let owner = self.owner;
        let mut callback = owner.slot(callback)?;
        self.start()?;
        let mut instance = owner.slot(owner.run(|| attribute.instantiate())??)?;
        let stage = owner.run(|| {
            let value = value.get_mut().ok_or_else(missing_slot)?.clone();
            let event = AttributeEvent::new(
                instance.get_mut().ok_or_else(missing_slot)?.clone(),
                value,
                self.inner,
            );
            Ok::<_, Error>(
                Arc::clone(callback.get_mut().ok_or_else(missing_slot)?)
                    .invoke(event, resolver.clone()),
            )
        })??;
        let mut finished = self.poll_stage(stage).await?;
        finished.clear();
        instance.clear();
        callback.clear();
        owner.check_cleanup()
    }

    async fn select_ordinary_argument(
        &self,
        consumer: &Resolver,
        parameter: &ConstructorParam,
    ) -> Result<SelectedArgument> {
        match parameter.kind() {
            ParameterKind::Class {
                key, erased_output, ..
            } => {
                let (use_default, services, missing) = self.claim(|state| {
                    let class = self.consumer_key.ok_or_else(missing_slot)?;
                    let direct = state.contextual.get_service(class, key);
                    let use_default = parameter.has_default()
                        && !state.bound_key_at(key, consumer.scope)
                        && direct.is_none();
                    if !parameter.is_variadic() {
                        return Ok::<_, Error>((use_default, None, false));
                    }
                    let lookup = state.aliases.resolve(&state.classes.canonical_key(key))?;
                    let contextual = state.contextual_implementation(class, &lookup);
                    let missing = !state.bound_key_at(&lookup, consumer.scope)
                        && contextual.is_none()
                        && state
                            .classes
                            .class(&lookup)
                            .is_none_or(|class| class.kind() != ClassKind::Concrete);
                    Ok((use_default, contextual.cloned(), missing))
                })??;
                if use_default {
                    return self.default_value(parameter);
                }
                if missing {
                    return Ok(SelectedArgument::Many(Vec::new()));
                }
                self.start()?;
                let selected = async {
                    if let Some(implementation) = services
                        && let Some(values) = self
                            .resolve_many(consumer, implementation, key, *erased_output)
                            .await?
                    {
                        return Ok(SelectedArgument::Many(values));
                    }
                    self.resolve_argument(consumer, key.clone(), key, *erased_output)
                        .await
                        .map(SelectedArgument::Value)
                }
                .await;
                match selected {
                    Err(error) if parameter.is_variadic() && is_unresolvable_dependency(&error) => {
                        Ok(SelectedArgument::Many(Vec::new()))
                    }
                    _ => selected,
                }
            }
            ParameterKind::Scalar { .. } | ParameterKind::Php(_) => {
                self.select_parameter(consumer, parameter).await
            }
            #[cfg(test)]
            ParameterKind::Untyped => self.select_parameter(consumer, parameter).await,
        }
    }

    async fn resolve_many(
        &self,
        consumer: &Resolver,
        implementation: Arc<ContextualImplementation>,
        target: &ServiceKey,
        erased_output: std::any::TypeId,
    ) -> Result<Option<Vec<ResourceInlineSlot<ParameterValue>>>> {
        let owner = self.owner;
        let mut implementation = owner.slot(implementation)?;
        let mut closure = consumer.clone();
        closure.build_consumer = None;
        let (context, services) = match implementation.get_mut().ok_or_else(missing_slot)?.as_ref()
        {
            ContextualImplementation::Services(services) => (consumer, services.clone()),
            ContextualImplementation::Tagged(tag) => {
                let services = self.claim(|state| state.contextual.tagged_keys(tag).to_vec())?;
                (&closure, services)
            }
            ContextualImplementation::FactoryMany { factory, output } => {
                let projection = if *output == erased_output {
                    None
                } else {
                    Some(
                        self.claim(|state| {
                            state.classes.output_projection(*output, target).cloned()
                        })?
                        .ok_or_else(|| {
                            container_error(
                                "service_type_mismatch",
                                "上下文变参工厂声明输出与请求类型不同",
                            )
                        })?,
                    )
                };
                let future = owner.run(|| factory(closure))?;
                let mut values = self.poll_stage(future).await?;
                let values = values.take().ok_or_else(missing_slot)?;
                let mut slots = Vec::with_capacity(values.len());
                let mut values = owner.slot(values.into_iter())?;
                while let Some(value) = values.get_mut().and_then(Iterator::next) {
                    let output = owner.run(|| {
                        consumer.deliver_output(self.inner, value, projection.as_ref())
                    })??;
                    slots.push(owner.slot(ParameterValue::from_erased(output))?);
                }
                return Ok(Some(slots));
            }
            _ => return Ok(None),
        };
        let mut values = Vec::with_capacity(services.len());
        for service in services {
            values.push(
                self.resolve_argument(context, service, target, erased_output)
                    .await?,
            );
        }
        Ok(Some(values))
    }

    async fn resolve_argument(
        &self,
        consumer: &Resolver,
        lookup: ServiceKey,
        target: &ServiceKey,
        erased_output: std::any::TypeId,
    ) -> Result<ResourceInlineSlot<ParameterValue>> {
        let mut value = self
            .poll_stage(consumer.descriptor_dependency(lookup, target.clone(), erased_output))
            .await?;
        self.owner.slot(ParameterValue::from_erased(
            value.take().ok_or_else(missing_slot)?,
        ))
    }

    async fn select_parameter(
        &self,
        consumer: &Resolver,
        parameter: &ConstructorParam,
    ) -> Result<SelectedArgument> {
        let class = self.consumer_key.ok_or_else(missing_slot)?;
        let implementation = self.claim(|state| {
            state
                .contextual
                .get_parameter(class, parameter.name())
                .cloned()
        })?;
        let Some(implementation) = implementation else {
            return self.select_ordinary_value(parameter);
        };
        self.start()?;
        let owner = self.owner;
        let mut implementation = owner.slot(implementation)?;
        let mut value = match implementation.get_mut().ok_or_else(missing_slot)?.as_ref() {
            ContextualImplementation::Parameter(value) => owner.slot(value.clone())?,
            ContextualImplementation::ParameterFactory(factory) => {
                let future = owner.run(|| factory(consumer.clone()))?;
                self.poll_stage(future).await?
            }
            _ => {
                return Err(container_error(
                    "container_contextual_need_invalid",
                    "普通参数需要值来源",
                ));
            }
        };
        parameter.validate_selected(value.get_mut().ok_or_else(missing_slot)?)?;
        implementation.clear();
        owner.check_cleanup()?;
        Ok(SelectedArgument::Value(value))
    }
}

impl DescriptorRun<'_> {
    fn owner(&self) -> Result<&ResourceInlineOwner> {
        self.attempt.owner()
    }

    fn arguments(&self) -> Result<ArgumentRun<'_>> {
        Ok(ArgumentRun {
            caller: self.caller,
            inner: self.inner,
            owner: self.owner()?,
            consumer_key: Some(self.source.key()),
            descriptor: Some(DescriptorGuard {
                source: self.source,
                attempt: self.attempt,
                started: &self.started,
            }),
        })
    }

    fn claim<T>(&self, action: impl FnOnce(&mut State) -> T) -> Result<T> {
        self.arguments()?.claim(action)
    }

    fn start(&self) -> Result<()> {
        self.arguments()?.start()
    }

    async fn poll_stage<T, F>(&self, future: F) -> Result<ResourceInlineSlot<T>>
    where
        F: Future<Output = Result<T>>,
    {
        self.arguments()?.poll_stage(future).await
    }

    async fn call_self_building(
        &self,
        consumer: Resolver,
        method: &SelfBuildingMethod,
    ) -> Result<ResourceInlineSlot<Value>> {
        let owner = self.owner()?;
        self.start()?;
        let binding = self.claim(|state| state.self_building.get(self.source.key()).cloned())?;
        if let Some(binding) = binding {
            let mut binding = owner.slot(binding)?;
            let factory = binding.get_mut().ok_or_else(missing_slot)?;
            let future = owner.run(|| factory(consumer))?;
            let result = self.poll_stage(future).await?;
            binding.clear();
            owner.check_cleanup()?;
            return Ok(result);
        }
        let mut arguments = Vec::new();
        for parameter in method.parameters() {
            self.claim(|_| ())?;
            arguments.push(
                self.arguments()?
                    .select_method_argument(&consumer, parameter, None)
                    .await?,
            );
        }
        let future = owner.run(|| {
            let views = arguments
                .iter_mut()
                .map(SelectedArgument::view)
                .collect::<Result<Vec<_>>>()?;
            method.call(
                &views,
                consumer,
                &mut |event| self.inner.scalar_diagnostics.report(event),
                &mut || self.claim(|_| ()),
            )
        })??;
        let value = self.poll_stage(future).await?;
        drop(arguments);
        owner.check_cleanup()?;
        Ok(value)
    }

    async fn dispatch_completed(&self, value: &mut ResourceInlineSlot<Value>) -> Result<()> {
        if self.events && lock(&self.inner.state).hooks.has_completed() {
            self.start()?;
            let mut stage = self
                .poll_stage(self.child.fire_completed(
                    self.inner,
                    self.source.key(),
                    value.get_mut().ok_or_else(missing_slot)?,
                ))
                .await?;
            stage.clear();
            self.owner()?.check_cleanup()?;
        }
        Ok(())
    }

    async fn instantiate(
        &self,
        prepared: &PreparedConstructor<'_>,
    ) -> Result<(ResourceInlineSlot<Value>, Vec<SelectedArgument>)> {
        let owner = self.owner()?;
        let mut arguments = Vec::new();
        let self_building = prepared.self_building().filter(|_| !self.ordinary_reentry);
        {
            let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
            let _frame = ActiveFactory(Arc::clone(&active));
            let mut consumer = self.child.clone();
            let key = self.source.key().clone();
            consumer
                .class_builds
                .retain(|frame| frame.active.load(Ordering::Acquire));
            consumer.class_builds.push(ClassBuildFrame {
                key: key.clone(),
                active: Arc::clone(&active),
                self_building: self_building.is_some(),
            });
            consumer.build_consumer = Some((key, active));
            if let Some(method) = self_building {
                return self
                    .call_self_building(consumer, method)
                    .await
                    .map(|value| (value, arguments));
            }
            for parameter in prepared.parameters() {
                self.claim(|_| ())?;
                arguments.push(
                    self.arguments()?
                        .select_argument(&consumer, parameter)
                        .await?,
                );
            }
        }
        self.start()?;
        let value = owner.slot(owner.run(|| {
            // 每个临时视图仍有独立槽保有底层值，构造阶段不会成为其最后一次引用。
            let views = arguments
                .iter_mut()
                .map(SelectedArgument::view)
                .collect::<Result<Vec<_>>>()?;
            if prepared.has_weak_parameters() {
                prepared.construct_selected_checked(
                    &views,
                    &mut |event| self.inner.scalar_diagnostics.report(event),
                    &mut || self.claim(|_| ()),
                )
            } else {
                prepared.construct(&views)
            }
        })??)?;
        Ok((value, arguments))
    }

    async fn execute(
        &self,
        prepared: PreparedConstructor<'_>,
        erased_output: std::any::TypeId,
    ) -> Result<Value> {
        let owner = self.owner()?;
        let (mut value, arguments) = self.instantiate(&prepared).await?;

        if !prepared.attributes().is_empty() {
            let object = value.get_mut().ok_or_else(missing_slot)?.clone();
            self.arguments()?
                .dispatch_attributes(
                    &self.child,
                    prepared.attributes(),
                    AttributeValue::One(ParameterValue::from_erased(object)),
                )
                .await?;
        }

        let chain = if self.source.decorates() {
            self.claim(|state| state.extensions.chain(&self.attempt.key))?
        } else {
            Vec::new()
        };
        let mut chain = chain.into_iter();
        let mut extenders = Vec::new();
        while let Some(extender) = chain.next() {
            match owner.slot(extender) {
                Ok(slot) => extenders.push(slot),
                Err(error) => {
                    for remaining in chain {
                        drop(owner.slot(remaining));
                    }
                    return Err(error);
                }
            }
        }
        for extender in &mut extenders {
            self.start()?;
            let current = value.get_mut().ok_or_else(missing_slot)?;
            let extender_fn = extender.get_mut().ok_or_else(missing_slot)?;
            let future = owner.run(|| (extender_fn.0)(Arc::clone(current), self.child.clone()))?;
            let next = self.poll_stage(future).await?;
            value.clear();
            extender.clear();
            value = next;
        }

        self.dispatch_completed(&mut value).await?;

        self.start()?;
        let mut result = owner
            .slot(owner.run(|| prepared.project(value.get_mut().ok_or_else(missing_slot)?))??)?;
        if result
            .get_mut()
            .ok_or_else(missing_slot)?
            .as_ref()
            .type_id()
            != erased_output
        {
            return Err(container_error(
                "service_type_mismatch",
                "描述符输出与请求的擦除类型不符",
            ));
        }
        value.clear();
        drop(extenders);
        drop(arguments);
        owner.check_cleanup()?;
        // 只借用结果槽，失效分支不得在 State→owner 内析构被拒绝的值。
        self.claim(|state| {
            owner.check_cleanup()?;
            let value = result.take().ok_or_else(missing_slot)?;
            if self.source.records_history() {
                state.resolved_history.insert(self.source.key().clone());
                state.extensions.resolved.insert(self.attempt.key.clone());
            }
            Ok(value)
        })?
    }
}

fn replaced() -> Error {
    container_error("binding_replaced", "描述符构造期间服务来源已更换")
}

fn missing_slot() -> Error {
    container_error(
        "container_descriptor_slot_missing",
        "描述符构造的受控持有槽已退役",
    )
}

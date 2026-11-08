//! 描述符缺项沿真实 Resolver 构造，并由原 transient 索引跟踪。

use super::{
    Arc, BoxFuture, Error, Inner, Ordering, ResolveRequest, Resolver, Result, ServiceKey, State,
    Value,
    class_descriptor::{Argument, ClassKind, ConstructorParam, ParameterKind, PreparedConstructor},
    container_error,
    descriptor_source::DescriptorSource,
    extensions::DescriptorAttempt,
    lock,
    parameters::ParameterValue,
};
use crate::foundation::resources::{ResourceInlineOwner, ResourceInlineSlot, ResourceTaskFailure};
use std::{
    future::{Future, poll_fn},
    task::Poll,
};

enum SelectedArgument {
    Value(ResourceInlineSlot<ParameterValue>),
    EmptyVariadic,
}

impl SelectedArgument {
    fn view(&mut self) -> Result<Argument> {
        match self {
            Self::Value(slot) => slot
                .get_mut()
                .map(|value| Argument::One(value.clone()))
                .ok_or_else(missing_slot),
            Self::EmptyVariadic => Ok(Argument::Many(Vec::new())),
        }
    }
}

struct DescriptorRun<'a> {
    caller: &'a Resolver,
    inner: &'a Arc<Inner>,
    source: &'a DescriptorSource,
    attempt: &'a DescriptorAttempt,
    child: Resolver,
    started: std::sync::atomic::AtomicBool,
}

impl Resolver {
    pub(super) async fn resolve_descriptor(
        &self,
        inner: &Arc<Inner>,
        source: &DescriptorSource,
        output: &ServiceKey,
        erased_output: std::any::TypeId,
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
        if self
            .chain
            .iter()
            .any(|(key, active)| key == source.key() && active.load(Ordering::Acquire))
        {
            return Err(container_error(
                "service_cycle",
                self.dependency_path(source.key()),
            ));
        }
        let Some(attempt) = DescriptorAttempt::admit(inner, self, source)? else {
            return Ok(None);
        };
        let run = DescriptorRun {
            caller: self,
            inner,
            source,
            attempt: &attempt,
            child: attempt.child(self)?,
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
        key: ServiceKey,
        erased_output: std::any::TypeId,
    ) -> BoxFuture<'_, Result<Value>> {
        let child = self.clone().for_new_lookup();
        Box::pin(async move {
            child
                .resolve_projected(
                    ResolveRequest::Typed {
                        lookup: None,
                        target: key,
                        erased_output,
                    },
                    None,
                )
                .await
        })
    }
}

impl DescriptorRun<'_> {
    fn owner(&self) -> Result<&ResourceInlineOwner> {
        self.attempt.owner()
    }

    fn claim<T>(&self, action: impl FnOnce(&mut State) -> T) -> Result<T> {
        let owner = self.owner()?;
        let claimed = self
            .source
            .claim(&mut lock(&self.inner.state), self.caller, |state| {
                owner.check_cleanup()?;
                Ok::<Option<T>, Error>(
                    self.attempt
                        .active(state, self.source)
                        .then(|| action(state)),
                )
            })?
            .ok_or_else(replaced)??;
        claimed.ok_or_else(replaced)
    }

    fn start(&self) -> Result<()> {
        self.claim(|_| self.started.store(true, Ordering::Relaxed))
    }

    async fn poll_stage<T, F>(&self, future: F) -> Result<ResourceInlineSlot<T>>
    where
        F: Future<Output = Result<T>>,
    {
        let owner = self.owner()?;
        let mut future = owner.slot(Box::pin(future))?;
        let polled = poll_fn(|cx| {
            // 已开始的阶段可以在换源后返回；真实 scope/owner 关闭在下一次 poll 观察。
            let open = self
                .caller
                .check_open(&lock(&self.inner.state))
                .and_then(|()| self.source.owner.with_open_owner(|| ()));
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
        let output = tokio::select! {
            result = polled => result,
            () = self.caller.closed.wait_closed() => Err(container_error("scope_closed", "生命周期已关闭")),
            () = self.attempt.termination.wait_closed() => Err(container_error("container_closed", "容器已关闭")),
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
            Ok(SelectedArgument::EmptyVariadic)
        } else if parameter.allows_null() {
            self.owner()?
                .slot(ParameterValue::null())
                .map(SelectedArgument::Value)
        } else {
            Err(container_error(
                "container_parameter_missing",
                format!("无法解析构造参数 `{}`", parameter.name()),
            ))
        }
    }

    fn default_value(&self, parameter: &ConstructorParam) -> Result<SelectedArgument> {
        self.start()?;
        let owner = self.owner()?;
        let value = owner
            .run(|| parameter.evaluate_default())??
            .ok_or_else(missing_slot)?;
        owner.slot(value).map(SelectedArgument::Value)
    }

    async fn select_argument(&self, parameter: &ConstructorParam) -> Result<SelectedArgument> {
        let owner = self.owner()?;
        if let Some(value) = self
            .caller
            .parameters
            .as_ref()
            .and_then(|frame| frame.get_named(parameter.name()))
        {
            let mut value = owner.slot(value)?;
            parameter.validate_value(value.get_mut().ok_or_else(missing_slot)?)?;
            return Ok(SelectedArgument::Value(value));
        }
        match parameter.kind() {
            ParameterKind::Class {
                key, erased_output, ..
            } => {
                let (bound, missing) = self.claim(|state| {
                    let bound = state.bound_key(key);
                    let concrete = state
                        .classes
                        .class(key)
                        .is_some_and(|class| class.kind() == ClassKind::Concrete);
                    (bound, !bound && !concrete)
                })?;
                if parameter.has_default() && !bound {
                    return self.default_value(parameter);
                }
                if parameter.is_variadic() && missing {
                    return Ok(SelectedArgument::EmptyVariadic);
                }
                self.start()?;
                let mut value = self
                    .poll_stage(
                        self.child
                            .descriptor_dependency(key.clone(), *erased_output),
                    )
                    .await?;
                if parameter.is_variadic() {
                    return Err(container_error(
                        "container_constructor_variadic_invalid",
                        "类变参需要有序依赖集合，单个解析值不能展开",
                    ));
                }
                owner
                    .slot(ParameterValue::from_erased(
                        value.take().ok_or_else(missing_slot)?,
                    ))
                    .map(SelectedArgument::Value)
            }
            ParameterKind::Scalar { .. } => self.select_ordinary_value(parameter),
            #[cfg(test)]
            ParameterKind::Untyped => self.select_ordinary_value(parameter),
            #[cfg(test)]
            ParameterKind::Php(_) => Err(container_error(
                "container_constructor_weak_frame_required",
                "弱标量需要已选参数帧和诊断入口",
            )),
        }
    }

    async fn execute(
        &self,
        prepared: PreparedConstructor<'_>,
        erased_output: std::any::TypeId,
    ) -> Result<Value> {
        let owner = self.owner()?;
        let mut arguments = Vec::new();
        for parameter in prepared.parameters() {
            self.claim(|_| ())?;
            arguments.push(self.select_argument(parameter).await?);
        }
        self.start()?;
        let mut value = owner.slot(owner.run(|| {
            // 每个临时视图仍有独立槽保有底层值，构造阶段不会成为其最后一次引用。
            let views = arguments
                .iter_mut()
                .map(SelectedArgument::view)
                .collect::<Result<Vec<_>>>()?;
            prepared.construct(&views)
        })??)?;

        let chain = self.claim(|state| state.extensions.chain(&self.attempt.key))?;
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
            if self.caller.parameters.is_none() {
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

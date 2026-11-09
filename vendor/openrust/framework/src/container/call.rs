//! Laravel 方法参数解析对应的显式 Rust callable 入口。

use std::future::Future;

use super::{ArgumentRun, ResourceInlineOwner, ResourceInlineSlot, SelectedArgument, missing_slot};
use crate::container::class_descriptor::{ResolvedArgs, validate_parameters};
use crate::container::parameters::ParameterFrame;
use crate::container::{
    ConstructorParam, Container, ParameterKey, ParameterOverrides, ParameterValue, Resolver, Scope,
    lock,
};
use crate::foundation::lifecycle::{CallbackDisposal, disposal};
use crate::foundation::resources::ResourceTaskFailure;
use crate::{BoxFuture, Result};

/// 传给已声明 callable 的参数。剩余实参依输入顺序提供，不再携带原键。
pub struct CallArguments<'a> {
    /// 经过依赖解析和类型校验的声明槽。
    pub resolved: ResolvedArgs<'a>,
    /// 未被声明槽或末尾变参消费的实参。
    pub remaining: &'a [ParameterValue],
}

impl Container {
    /// 在根生命周期调用显式声明参数的 Rust callable。
    pub fn call<'a, T: Send + 'a>(
        &'a self,
        signature: &'a [ConstructorParam],
        overrides: ParameterOverrides,
        callback: impl for<'b> FnOnce(CallArguments<'b>) -> BoxFuture<'a, Result<T>> + Send + 'a,
    ) -> impl Future<Output = Result<T>> + Send + 'a {
        invoke(|| self.root_resolver(), signature, overrides, callback)
    }

    pub(crate) fn call_in_entry<'a, T: Send + 'a>(
        &'a self,
        signature: &'a [ConstructorParam],
        overrides: ParameterOverrides,
        callback: impl for<'b> FnOnce(CallArguments<'b>) -> BoxFuture<'a, Result<T>> + Send + 'a,
    ) -> impl Future<Output = Result<T>> + Send + 'a {
        invoke(
            || {
                self.current_entry_scope()
                    .map_or_else(|| self.root_resolver(), |scope| Ok(scope.resolver()))
            },
            signature,
            overrides,
            callback,
        )
    }
}

impl Scope {
    /// 在本作用域调用显式声明参数的 Rust callable。
    pub fn call<'a, T: Send + 'a>(
        &'a self,
        signature: &'a [ConstructorParam],
        overrides: ParameterOverrides,
        callback: impl for<'b> FnOnce(CallArguments<'b>) -> BoxFuture<'a, Result<T>> + Send + 'a,
    ) -> impl Future<Output = Result<T>> + Send + 'a {
        invoke(|| Ok(self.resolver()), signature, overrides, callback)
    }
}

impl Resolver {
    /// 沿当前解析链和作用域调用显式声明参数的 Rust callable。
    /// 调用实参不传给依赖的工厂，也不修改当前解析器的参数帧。
    pub fn call<'a, T: Send + 'a>(
        &'a self,
        signature: &'a [ConstructorParam],
        overrides: ParameterOverrides,
        callback: impl for<'b> FnOnce(CallArguments<'b>) -> BoxFuture<'a, Result<T>> + Send + 'a,
    ) -> impl Future<Output = Result<T>> + Send + 'a {
        invoke(
            || Ok(self.clone().for_new_lookup()),
            signature,
            overrides,
            callback,
        )
    }
}

struct Callback<F>(Option<F>);

impl<F> Drop for Callback<F> {
    fn drop(&mut self) {
        if let Some(callback) = self.0.take() {
            disposal::dispose(callback, None);
        }
    }
}

fn invoke<'a, T: Send + 'a>(
    select: impl FnOnce() -> Result<Resolver> + Send + 'a,
    signature: &'a [ConstructorParam],
    overrides: ParameterOverrides,
    callback: impl for<'b> FnOnce(CallArguments<'b>) -> BoxFuture<'a, Result<T>> + Send + 'a,
) -> impl Future<Output = Result<T>> + Send + 'a {
    invoke_operation(
        select,
        overrides,
        DirectCall {
            signature,
            callback,
        },
    )
}

pub(super) trait CallOperation<T>: Send {
    fn validate(&self) -> Result<()>;

    fn execute(
        self,
        run: &ArgumentRun<'_>,
        overrides: ParameterOverrides,
    ) -> impl Future<Output = Result<ResourceInlineSlot<T>>> + Send;
}

struct DirectCall<'a, F> {
    signature: &'a [ConstructorParam],
    callback: F,
}

impl<'a, T, F> CallOperation<T> for DirectCall<'a, F>
where
    T: Send + 'a,
    F: for<'b> FnOnce(CallArguments<'b>) -> BoxFuture<'a, Result<T>> + Send,
{
    fn validate(&self) -> Result<()> {
        validate_parameters(self.signature)
    }

    async fn execute(
        self,
        run: &ArgumentRun<'_>,
        overrides: ParameterOverrides,
    ) -> Result<ResourceInlineSlot<T>> {
        run.invoke_callable(self.signature, overrides, self.callback)
            .await
    }
}

pub(super) fn invoke_operation<'a, T: Send + 'a>(
    select: impl FnOnce() -> Result<Resolver> + Send + 'a,
    overrides: ParameterOverrides,
    operation: impl CallOperation<T> + 'a,
) -> impl Future<Output = Result<T>> + Send + 'a {
    let mut inputs = Some(ParameterFrame::new(
        overrides,
        CallbackDisposal::new(None, None),
    ));
    let mut operation = Callback(Some(operation));
    async move {
        let result = async {
            let resolver = select()?;
            inputs
                .as_mut()
                .ok_or_else(missing_slot)?
                .set_disposal(resolver.callback_disposal());
            operation.0.as_ref().ok_or_else(missing_slot)?.validate()?;
            let container = resolver.container()?;
            let registrar = resolver.resources()?;
            let mut owner = ResourceInlineOwner::new();
            {
                let state = lock(&container.inner.state);
                owner.reserve(&registrar, || resolver.check_open(&state))?;
            }
            owner.register()?;
            let mut inputs = owner.slot(inputs.take().ok_or_else(missing_slot)?)?;
            let frame = inputs.get_mut().ok_or_else(missing_slot)?;
            frame.set_disposal(CallbackDisposal::new(
                Some(owner.completion()),
                Some(registrar),
            ));
            let overrides = frame.view().snapshot();
            let run = ArgumentRun {
                caller: &resolver,
                inner: &container.inner,
                owner: &owner,
                consumer_key: resolver.active_consumer_frame().map(|(key, _)| key),
                descriptor: None,
            };
            let operation = operation.0.take().ok_or_else(missing_slot)?;
            let result = operation.execute(&run, overrides).await;
            // 原实参先逐项退役。Scope 的完成栅栏继续观察这个调用直到退役结束。
            inputs.clear();
            match (result, owner.check_cleanup()) {
                (Err(primary), Err(cleanup)) => Err(ResourceTaskFailure::error(primary, cleanup)),
                (Err(primary), Ok(())) => Err(primary),
                (Ok(mut value), cleanup) => {
                    cleanup?;
                    let value = value.take().ok_or_else(missing_slot)?;
                    owner.finish();
                    Ok(value)
                }
            }
        }
        .await;
        match inputs {
            Some(inputs) => inputs.finish(result),
            None => result,
        }
    }
}

type InputSlot = ResourceInlineSlot<ParameterValue>;

pub(super) struct CallParameters(Vec<(ParameterKey, InputSlot)>);

impl CallParameters {
    pub(super) fn take_named(&mut self, name: &str) -> Option<InputSlot> {
        let index = self
            .0
            .iter()
            .position(|(key, _)| matches!(key, ParameterKey::Name(key) if key.as_ref() == name))?;
        Some(self.0.remove(index).1)
    }
}

impl ArgumentRun<'_> {
    pub(super) async fn invoke_callable<'a, T: Send + 'a>(
        &self,
        signature: &[ConstructorParam],
        overrides: ParameterOverrides,
        callback: impl for<'b> FnOnce(CallArguments<'b>) -> BoxFuture<'a, Result<T>> + Send,
    ) -> Result<ResourceInlineSlot<T>> {
        let mut callback = self.owner.slot(callback)?;
        // 原输入帧在调用结束前仍持有全部显式参数。这里仅在受管槽中持有克隆。
        let mut remaining = CallParameters(
            overrides
                .entries
                .into_iter()
                .map(|(key, value)| self.owner.slot(value).map(|value| (key, value)))
                .collect::<Result<_>>()?,
        );
        let mut arguments = Vec::with_capacity(signature.len());
        for parameter in signature {
            self.claim(|_| ())?;
            arguments.push(
                self.select_method_argument(self.caller, parameter, Some(&mut remaining))
                    .await?,
            );
        }
        if signature.last().is_some_and(ConstructorParam::is_variadic)
            && !remaining.0.is_empty()
            && let Some(last) = arguments.last_mut()
        {
            let old = std::mem::replace(last, SelectedArgument::Many(Vec::new()));
            let mut items = match old {
                SelectedArgument::Value(value) => vec![value],
                SelectedArgument::Many(values) => values,
            };
            items.extend(remaining.0.drain(..).map(|(_, value)| value));
            *last = SelectedArgument::Many(items);
        }
        self.start()?;
        let future = self.owner.run(|| {
            let selected = arguments
                .iter_mut()
                .map(SelectedArgument::view)
                .collect::<Result<Vec<_>>>()?;
            let materialized = ResolvedArgs::materialize_selected(
                signature,
                &selected,
                &mut |event| self.inner.scalar_diagnostics.report(event),
                &mut || self.claim(|_| ()),
            )?;
            let remaining = remaining
                .0
                .iter_mut()
                .map(|(_, slot)| slot.get_mut().cloned().ok_or_else(missing_slot))
                .collect::<Result<Vec<_>>>()?;
            self.claim(|_| ())?;
            let callback = callback.take().ok_or_else(missing_slot)?;
            Ok::<_, crate::Error>(callback(CallArguments {
                resolved: ResolvedArgs::from_materialized(signature, &materialized),
                remaining: &remaining,
            }))
        })??;
        let result = self.poll_stage(future).await?;
        drop(arguments);
        drop(remaining);
        self.owner.check_cleanup()?;
        Ok(result)
    }
}

#[cfg(test)]
#[path = "call_tests.rs"]
mod tests;

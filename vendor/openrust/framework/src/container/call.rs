//! Laravel BoundMethod argument policy; it deliberately differs from constructor injection.
use super::class_descriptor::{Argument, validate_arguments, validate_signature};
use super::{
    ConstructorParam, Container, ParameterKey, ParameterOverrides, ParameterValue, ResolvedArgs,
    ServiceKey, call_context, scope,
};
use crate::contracts::container::BindingResolutionError;
use std::{future::Future, pin::Pin};
pub type CallFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, BindingResolutionError>> + Send + 'a>>;
/// A compiled callable target consumed by the container's canonical call entry.
pub trait Callable<'a, R: Send + 'a>: Send + 'a {
    fn invoke(self, container: &'a Container, parameters: ParameterOverrides) -> CallFuture<'a, R>;
}
/// Explicit closure metadata; its callback may borrow data and return a generic Future.
pub struct Callback<'p, F> {
    signature: &'p [ConstructorParam],
    callback: F,
}
impl<'p, F> Callback<'p, F> {
    pub fn new<R, Fut>(signature: &'p [ConstructorParam], callback: F) -> Self
    where
        F: FnOnce(CallArguments) -> Fut + Send + 'p,
        Fut: Future<Output = Result<R, BindingResolutionError>> + Send + 'p,
        R: Send + 'p,
    {
        Self {
            signature,
            callback,
        }
    }
}
impl<'a, 'p: 'a, F, Fut, R> Callable<'a, R> for Callback<'p, F>
where
    F: FnOnce(CallArguments) -> Fut + Send + 'a,
    Fut: Future<Output = Result<R, BindingResolutionError>> + Send + 'a,
    R: Send + 'a,
{
    fn invoke(self, container: &'a Container, parameters: ParameterOverrides) -> CallFuture<'a, R> {
        Box::pin(container.invoke_callable(self.signature, parameters, self.callback))
    }
}
/// Declared slots and positional values left after named/type-key dependency selection.
pub struct CallArguments {
    pub resolved: ResolvedArgs,
    pub remaining: Vec<ParameterValue>,
}
fn take(
    inputs: &mut Vec<(ParameterKey, ParameterValue)>,
    key: &ParameterKey,
) -> Option<ParameterValue> {
    inputs
        .iter()
        .position(|(known, _)| known == key)
        .map(|index| inputs.remove(index).1)
}
pub(super) async fn arguments(
    c: &Container,
    signature: &[ConstructorParam],
    parameters: ParameterOverrides,
) -> Result<CallArguments, BindingResolutionError> {
    let mut inputs: Vec<_> = parameters
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let mut values = Vec::with_capacity(signature.len());
    for parameter in signature {
        let explicit = take(&mut inputs, &ParameterKey::name(parameter.name)).or_else(|| {
            parameter
                .class
                .as_ref()
                .and_then(|key| take(&mut inputs, &ParameterKey::name(key.name())))
        });
        let argument = if let Some(value) = explicit {
            Argument::One(value)
        } else if let Some(key) = &parameter.class {
            if parameter.variadic {
                Argument::Many((parameter.call_many)(c, key.clone()).await?)
            } else if parameter.default.is_some() && !c.has_key_for_call(key) {
                (Argument::One)((parameter
                    .default
                    .as_ref()
                    .expect("selected method default"))()?)
            } else {
                Argument::One((parameter.resolve_async)(c, key.clone()).await?)
            }
        } else if let Some(default) = &parameter.default {
            Argument::One(default()?)
        } else if parameter.variadic {
            Argument::Many(Vec::new())
        } else {
            return Err(BindingResolutionError::UnresolvableParameter {
                parameter: parameter.name,
                service: c.consumer(),
            });
        };
        values.push(argument);
    }
    let mut remaining: Vec<_> = inputs.into_iter().map(|(_, value)| value).collect();
    if signature.last().is_some_and(|parameter| parameter.variadic) {
        let last = values.pop().expect("variadic slot");
        let mut items = match last {
            Argument::One(value) => vec![value],
            Argument::Many(values) => values,
        };
        items.append(&mut remaining);
        values.push(Argument::Many(items));
    }
    Ok(CallArguments {
        resolved: validate_arguments(signature, values)?,
        remaining,
    })
}
impl Container {
    fn has_key_for_call(&self, key: &ServiceKey) -> bool {
        use crate::contracts::container::Container as _;
        self.has(key.clone())
    }
    pub(super) async fn invoke_callable<'a, R: Send + 'a, F, Fut>(
        &'a self,
        signature: &'a [ConstructorParam],
        parameters: ParameterOverrides,
        callback: F,
    ) -> Result<R, BindingResolutionError>
    where
        F: FnOnce(CallArguments) -> Fut + Send + 'a,
        Fut: Future<Output = Result<R, BindingResolutionError>> + Send + 'a,
    {
        let stack = call_context::stack(self);
        scope::preserve(
            self,
            call_context::run(self, stack, async move {
                validate_signature(&ServiceKey::named("Closure"), signature)?;
                let args = arguments(self, signature, parameters.clone()).await?;
                let result = callback(args).await;
                drop(parameters);
                result
            }),
        )
        .await
    }
}

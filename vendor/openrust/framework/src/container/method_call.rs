//! Compiled instance/static declarations and Laravel method-binding overrides.
use super::class_descriptor::validate_signature;
use super::{
    CallArguments, CallFuture, ConstructorParam, Container, ParameterOverrides, ServiceKey, call,
    call_context, scope,
};
use crate::contracts::container::{BindingResolutionError, Container as _};
use std::{
    any::{Any, type_name},
    marker::PhantomData,
    sync::Arc,
};
type Result<T> = std::result::Result<T, BindingResolutionError>;
type Instance<C, R> =
    dyn for<'a> Fn(Arc<C>, CallArguments, &'a Container) -> CallFuture<'a, R> + Send + Sync;
type Static<R> = dyn for<'a> Fn(CallArguments, &'a Container) -> CallFuture<'a, R> + Send + Sync;
type Bound<C, R> = dyn for<'a> Fn(Arc<C>, &'a Container) -> CallFuture<'a, R> + Send + Sync;
type StaticBound<R> = dyn for<'a> Fn(ServiceKey, &'a Container) -> CallFuture<'a, R> + Send + Sync;
enum Declaration<C: ?Sized, R> {
    Instance(Vec<ConstructorParam>, Box<Instance<C, R>>),
    Static(Vec<ConstructorParam>, Box<Static<R>>),
}
impl<C: ?Sized, R> Declaration<C, R> {
    fn signature(&self) -> &[ConstructorParam] {
        match self {
            Self::Instance(signature, _) | Self::Static(signature, _) => signature,
        }
    }
}
struct Binding<C: ?Sized, R>(Box<Bound<C, R>>);
struct StaticBinding<R>(Box<StaticBound<R>>);
#[derive(Clone, Copy)]
enum Form {
    Instance,
    Static,
}
#[derive(Clone)]
pub(super) struct RegisteredMethod {
    value: Arc<dyn Any + Send + Sync>,
    output: &'static str,
    form: Form,
}
impl RegisteredMethod {
    fn new<R: 'static>(value: impl Any + Send + Sync, form: Form) -> Self {
        Self {
            value: Arc::new(value),
            output: type_name::<R>(),
            form,
        }
    }
    fn cast<T: Any + Send + Sync, R: 'static>(
        &self,
        class: &ServiceKey,
        name: &str,
    ) -> Result<Arc<T>> {
        self.value
            .clone()
            .downcast()
            .map_err(|_| BindingResolutionError::MethodContractMismatch {
                service: class.clone(),
                method: Arc::from(name),
                expected: type_name::<R>(),
                actual: self.output,
            })
    }
    fn wrong_form(
        &self,
        class: &ServiceKey,
        name: &str,
        expected: &'static str,
    ) -> BindingResolutionError {
        BindingResolutionError::MethodContractMismatch {
            service: class.clone(),
            method: Arc::from(name),
            expected,
            actual: match self.form {
                Form::Instance => "instance method",
                Form::Static => "static method",
            },
        }
    }
}
/// Typed equivalent of [object, method], Class@method, or a class's default/__invoke entry.
pub struct MethodCall<'a, C: ?Sized> {
    name: &'a str,
    receiver: Option<Arc<C>>,
    implicit: bool,
}
#[allow(non_snake_case, reason = "Typed Rust method target adapter")]
impl<'a, C: ?Sized> MethodCall<'a, C> {
    pub fn on(receiver: Arc<C>, name: &'a str) -> Self {
        Self {
            name,
            receiver: Some(receiver),
            implicit: false,
        }
    }
    pub fn class(name: &'a str) -> Self {
        Self {
            name,
            receiver: None,
            implicit: false,
        }
    }
    pub fn defaultMethod(name: &'a str) -> Self {
        if name.is_empty() || name == "0" {
            Self::invokable()
        } else {
            Self::class(name)
        }
    }
    pub fn invokable() -> Self {
        Self {
            name: "__invoke",
            receiver: None,
            implicit: true,
        }
    }
}
/// Qualified Class::method ignores bindings; a class-array callable consults static bindings.
pub struct StaticCall<'a, C: ?Sized> {
    name: &'a str,
    binding: bool,
    marker: PhantomData<fn() -> Arc<C>>,
}
impl<'a, C: ?Sized> StaticCall<'a, C> {
    pub fn qualified(name: &'a str) -> Self {
        Self {
            name,
            binding: false,
            marker: PhantomData,
        }
    }
    pub fn array(name: &'a str) -> Self {
        Self {
            name,
            binding: true,
            marker: PhantomData,
        }
    }
}
fn key<C: ?Sized + 'static>(name: &str, declared: bool) -> (ServiceKey, Arc<str>) {
    (
        ServiceKey::typed::<C>(),
        Arc::from(if declared {
            name.to_ascii_lowercase()
        } else {
            name.to_owned()
        }),
    )
}
fn context(c: &Container, class: &ServiceKey) -> Vec<ServiceKey> {
    let mut stack = call_context::stack(c);
    if !stack.contains(class) {
        stack.push(class.clone());
    }
    stack
}
impl Container {
    fn set_declaration<C: ?Sized + 'static, R: Send + 'static>(
        &self,
        name: &str,
        callback: Declaration<C, R>,
        form: Form,
    ) -> Result<()> {
        let class = ServiceKey::typed::<C>();
        if name.is_empty() {
            return Err(BindingResolutionError::InvalidMethod {
                service: class,
                method: Arc::from(name),
                reason: "method name cannot be empty",
            });
        }
        validate_signature(&class, callback.signature())?;
        let mut pending = Some(RegisteredMethod::new::<R>(callback, form));
        let result = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            let key = key::<C>(name, true);
            match registry.methods.entry(key) {
                std::collections::hash_map::Entry::Occupied(_) => {
                    Err(BindingResolutionError::InvalidMethod {
                        service: class,
                        method: Arc::from(name),
                        reason: "a method declaration already exists",
                    })
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(pending.take().expect("new method"));
                    Ok(())
                }
            }
        };
        drop(pending);
        result
    }
    pub(super) fn declare_method<C: ?Sized + Send + Sync + 'static, R: Send + 'static, F>(
        &self,
        name: &str,
        signature: Vec<ConstructorParam>,
        callback: F,
    ) -> Result<()>
    where
        F: for<'a> Fn(Arc<C>, CallArguments, &'a Self) -> CallFuture<'a, R> + Send + Sync + 'static,
    {
        self.set_declaration::<C, R>(
            name,
            Declaration::Instance(signature, Box::new(callback)),
            Form::Instance,
        )
    }
    pub(super) fn declare_static<C: ?Sized + 'static, R: Send + 'static, F>(
        &self,
        name: &str,
        signature: Vec<ConstructorParam>,
        callback: F,
    ) -> Result<()>
    where
        F: for<'a> Fn(CallArguments, &'a Self) -> CallFuture<'a, R> + Send + Sync + 'static,
    {
        self.set_declaration::<C, R>(
            name,
            Declaration::Static(signature, Box::new(callback)),
            Form::Static,
        )
    }
    pub(super) fn bind_method<C: ?Sized + Send + Sync + 'static, R: Send + 'static, F>(
        &self,
        name: &str,
        callback: F,
    ) -> Result<()>
    where
        F: for<'a> Fn(Arc<C>, &'a Self) -> CallFuture<'a, R> + Send + Sync + 'static,
    {
        let method =
            RegisteredMethod::new::<R>(Binding::<C, R>(Box::new(callback)), Form::Instance);
        let retired = self
            .registry
            .write()
            .expect("container lock poisoned")
            .method_bindings
            .insert(key::<C>(name, false), method);
        drop(retired);
        Ok(())
    }
    pub(super) fn bind_static<C: ?Sized + 'static, R: Send + 'static, F>(
        &self,
        name: &str,
        callback: F,
    ) -> Result<()>
    where
        F: for<'a> Fn(ServiceKey, &'a Self) -> CallFuture<'a, R> + Send + Sync + 'static,
    {
        let method =
            RegisteredMethod::new::<R>(StaticBinding::<R>(Box::new(callback)), Form::Static);
        let retired = self
            .registry
            .write()
            .expect("container lock poisoned")
            .method_bindings
            .insert(key::<C>(name, false), method);
        drop(retired);
        Ok(())
    }
    pub(super) fn has_method<C: ?Sized + 'static>(&self, name: &str) -> bool {
        self.registry
            .read()
            .expect("container lock poisoned")
            .method_bindings
            .contains_key(&key::<C>(name, false))
    }
    fn selected<C: ?Sized + 'static>(
        &self,
        name: &str,
    ) -> (Option<RegisteredMethod>, Option<RegisteredMethod>) {
        let registry = self.registry.read().expect("container lock poisoned");
        (
            registry.methods.get(&key::<C>(name, true)).cloned(),
            registry
                .method_bindings
                .get(&key::<C>(name, false))
                .cloned(),
        )
    }
    pub(super) async fn invoke_method<C: ?Sized + Send + Sync + 'static, R: Send + 'static>(
        &self,
        target: MethodCall<'_, C>,
        parameters: ParameterOverrides,
    ) -> Result<R> {
        let parent = call_context::stack(self);
        scope::preserve(
            self,
            call_context::run(self, parent, async move {
                let class = ServiceKey::typed::<C>();
                if target.implicit && self.selected::<C>(target.name).0.is_none() {
                    return Err(BindingResolutionError::InvalidMethod {
                        service: class,
                        method: Arc::from(target.name),
                        reason: "method not provided and __invoke is not declared",
                    });
                }
                let callable_object =
                    target.receiver.is_some() && self.selected::<C>(target.name).0.is_some();
                let receiver = match target.receiver {
                    Some(receiver) => receiver,
                    None => self.makeAsync::<C>().await?,
                };
                let stack = if callable_object {
                    context(self, &class)
                } else {
                    call_context::stack(self)
                };
                call_context::run(self, stack, async move {
                    let (declared, bound) = self.selected::<C>(target.name);
                    let result = if let Some(bound) = bound {
                        if !matches!(bound.form, Form::Instance) {
                            return Err(bound.wrong_form(
                                &class,
                                target.name,
                                "instance method binding",
                            ));
                        }
                        let bound = bound.cast::<Binding<C, R>, R>(&class, target.name)?;
                        (bound.0)(receiver.clone(), self).await
                    } else {
                        let declared = declared
                            .ok_or_else(|| BindingResolutionError::MethodNotFound {
                                service: class.clone(),
                                method: Arc::from(target.name),
                            })?
                            .cast::<Declaration<C, R>, R>(&class, target.name)?;
                        let args =
                            call::arguments(self, declared.signature(), parameters.clone()).await?;
                        match declared.as_ref() {
                            Declaration::Instance(_, callback) => {
                                callback(receiver.clone(), args, self).await
                            }
                            Declaration::Static(_, callback) => callback(args, self).await,
                        }
                    };
                    drop(receiver);
                    drop(parameters);
                    result
                })
                .await
            }),
        )
        .await
    }
    pub(super) async fn invoke_binding<C: ?Sized + Send + Sync + 'static, R: Send + 'static>(
        &self,
        name: &str,
        receiver: Arc<C>,
    ) -> Result<R> {
        let parent = call_context::stack(self);
        scope::preserve(
            self,
            call_context::run(self, parent, async {
                let class = ServiceKey::typed::<C>();
                let (_, bound) = self.selected::<C>(name);
                let bound = bound.ok_or_else(|| BindingResolutionError::MethodBindingNotFound {
                    service: class.clone(),
                    method: Arc::from(name),
                })?;
                if !matches!(bound.form, Form::Instance) {
                    return Err(bound.wrong_form(&class, name, "instance method binding"));
                }
                let bound = bound.cast::<Binding<C, R>, R>(&class, name)?;
                let result = (bound.0)(receiver.clone(), self).await;
                drop(receiver);
                result
            }),
        )
        .await
    }
    pub(super) async fn invoke_static<C: ?Sized + 'static, R: Send + 'static>(
        &self,
        target: StaticCall<'_, C>,
        parameters: ParameterOverrides,
    ) -> Result<R> {
        let parent = call_context::stack(self);
        scope::preserve(
            self,
            call_context::run(self, parent, async move {
                let class = ServiceKey::typed::<C>();
                let (declared, binding) = self.selected::<C>(target.name);
                let binding = if target.binding { binding } else { None };
                let binding = binding
                    .map(|value| {
                        if !matches!(value.form, Form::Static) {
                            return Err(value.wrong_form(
                                &class,
                                target.name,
                                "static method binding",
                            ));
                        }
                        value.cast::<StaticBinding<R>, R>(&class, target.name)
                    })
                    .transpose()?;
                let declared = if binding.is_none() {
                    Some(
                        declared
                            .ok_or_else(|| BindingResolutionError::MethodNotFound {
                                service: class.clone(),
                                method: Arc::from(target.name),
                            })?
                            .cast::<Declaration<C, R>, R>(&class, target.name)?,
                    )
                } else {
                    None
                };
                let stack = if self.selected::<C>(target.name).0.is_some() {
                    context(self, &class)
                } else {
                    call_context::stack(self)
                };
                call_context::run(self, stack, async move {
                    let result = if let Some(binding) = binding {
                        (binding.0)(class, self).await
                    } else {
                        let declared = declared.expect("selected static declaration");
                        match declared.as_ref() {
                            Declaration::Static(signature, callback) => {
                                let args =
                                    call::arguments(self, signature, parameters.clone()).await?;
                                callback(args, self).await
                            }
                            Declaration::Instance(_, _) => {
                                return Err(BindingResolutionError::MethodContractMismatch {
                                    service: class,
                                    method: Arc::from(target.name),
                                    expected: "static method",
                                    actual: "instance method",
                                });
                            }
                        }
                    };
                    drop(parameters);
                    result
                })
                .await
            }),
        )
        .await
    }
    pub(super) async fn invoke_static_binding<C: ?Sized + 'static, R: Send + 'static>(
        &self,
        name: &str,
    ) -> Result<R> {
        let parent = call_context::stack(self);
        scope::preserve(
            self,
            call_context::run(self, parent, async {
                let class = ServiceKey::typed::<C>();
                let (_, bound) = self.selected::<C>(name);
                let bound = bound.ok_or_else(|| BindingResolutionError::MethodBindingNotFound {
                    service: class.clone(),
                    method: Arc::from(name),
                })?;
                if !matches!(bound.form, Form::Static) {
                    return Err(bound.wrong_form(&class, name, "static method binding"));
                }
                let bound = bound.cast::<StaticBinding<R>, R>(&class, name)?;
                (bound.0)(class, self).await
            }),
        )
        .await
    }
}

impl<'a, 'p: 'a, C: ?Sized + Send + Sync + 'static, R: Send + 'static> super::Callable<'a, R>
    for MethodCall<'p, C>
{
    fn invoke(self, container: &'a Container, parameters: ParameterOverrides) -> CallFuture<'a, R> {
        Box::pin(container.invoke_method(self, parameters))
    }
}
impl<'a, 'p: 'a, C: ?Sized + 'static, R: Send + 'static> super::Callable<'a, R>
    for StaticCall<'p, C>
{
    fn invoke(self, container: &'a Container, parameters: ParameterOverrides) -> CallFuture<'a, R> {
        Box::pin(container.invoke_static(self, parameters))
    }
}

//! 通过显式类型和方法名选择容器可调用方法。

use std::any::Any;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};

use super::call::{CallOperation, invoke_operation};
use super::{ArgumentRun, ResourceInlineSlot, missing_slot};
use crate::container::attributes::retire_registration;
use crate::container::class_descriptor::validate_parameters;
use crate::container::{
    ActiveFactory, Arc, CallArguments, ConstructorParam, Container, ContainerBuilder, HashMap,
    ParameterOverrides, Resolver, Scope, ServiceKey, container_error, lock,
};
use crate::foundation::ApplicationState;
use crate::foundation::lifecycle::{CallbackDisposal, disposal};
use crate::{BoxFuture, Result};

type Declared<C, T> =
    dyn Fn(Arc<C>, CallArguments<'_>, Resolver) -> BoxFuture<'static, Result<T>> + Send + Sync;
type Bound<C, T> = dyn Fn(Arc<C>, Resolver) -> BoxFuture<'static, Result<T>> + Send + Sync;
type ErasedMethod = Arc<dyn Any + Send + Sync>;

pub(in crate::container) struct MethodConsumer {
    class: ServiceKey,
    active: Arc<AtomicBool>,
    parent: Option<Arc<Self>>,
}

enum Method<C: ?Sized, T> {
    Declared(Vec<ConstructorParam>, Box<Declared<C, T>>),
    Bound(Box<Bound<C, T>>),
}

#[derive(Default)]
struct Entry {
    declared: Option<ErasedMethod>,
    bound: Option<ErasedMethod>,
}

#[derive(Default)]
pub(in crate::container) struct MethodRegistry(HashMap<ServiceKey, HashMap<Arc<str>, Entry>>);

impl MethodRegistry {
    fn get(&self, class: &ServiceKey, name: &str) -> Option<&Entry> {
        self.0.get(class)?.get(name)
    }

    fn insert<C: ?Sized + Send + Sync + 'static, T: Send + 'static>(
        &mut self,
        name: &str,
        method: Method<C, T>,
    ) -> Option<ErasedMethod> {
        let entry = self
            .0
            .entry(ServiceKey::typed::<C>())
            .or_default()
            .entry(Arc::from(name))
            .or_default();
        let slot = match &method {
            Method::Declared(..) => &mut entry.declared,
            Method::Bound(_) => &mut entry.bound,
        };
        slot.replace(Arc::new(method))
    }
}

impl Drop for MethodRegistry {
    fn drop(&mut self) {
        for entry in std::mem::take(&mut self.0)
            .into_values()
            .flat_map(HashMap::into_values)
        {
            if disposal::dispose(entry.declared, None).seen
                | disposal::dispose(entry.bound, None).seen
            {
                tracing::error!("容器方法登记析构失败");
            }
        }
    }
}

/// 同一个类型化方法的调用入口。方法名采用精确匹配。
pub struct MethodCall<'a, C: ?Sized> {
    name: &'a str,
    receiver: Option<Arc<C>>,
    implicit: bool,
    use_binding: bool,
}

impl<'a, C: ?Sized> MethodCall<'a, C> {
    /// 调用已取得的对象，保持接收者的实例身份。
    pub fn on(receiver: Arc<C>, name: &'a str) -> Self {
        Self {
            name,
            receiver: Some(receiver),
            implicit: false,
            use_binding: true,
        }
    }

    /// 直接调用已声明方法，沿用对象类型上下文，跳过方法绑定。
    pub fn direct(receiver: Arc<C>, name: &'a str) -> Self {
        Self {
            name,
            receiver: Some(receiver),
            implicit: false,
            use_binding: false,
        }
    }

    /// 从容器解析此类型，然后调用指定方法。可传入调用方选择的默认方法名。
    pub fn class(name: &'a str) -> Self {
        Self {
            name,
            receiver: None,
            implicit: false,
            use_binding: true,
        }
    }

    /// 从容器解析此类型，然后调用已声明的默认入口 `__invoke`。
    pub fn invokable() -> Self {
        Self {
            name: "__invoke",
            receiver: None,
            implicit: true,
            use_binding: true,
        }
    }
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(container_error(
            "container_method_name_missing",
            "必须提供容器方法名",
        ));
    }
    Ok(())
}

impl<C: ?Sized, T> Method<C, T> {
    fn validate(&self, name: &str) -> Result<()> {
        validate_name(name)?;
        if let Self::Declared(signature, _) = self {
            validate_parameters(signature)?;
        }
        Ok(())
    }
}

impl ContainerBuilder {
    /// 声明可通过容器调用的方法签名和实现。
    pub fn register_method<C: ?Sized + Send + Sync + 'static, T: Send + 'static>(
        &mut self,
        name: &str,
        signature: Vec<ConstructorParam>,
        callback: impl Fn(Arc<C>, CallArguments<'_>, Resolver) -> BoxFuture<'static, Result<T>>
        + Send
        + Sync
        + 'static,
    ) -> Result<()> {
        self.set_method(name, Method::Declared(signature, Box::new(callback)))
    }

    /// 为类型化方法登记替代回调。调用时由回调取得接收者和解析器。
    pub fn bind_method<C: ?Sized + Send + Sync + 'static, T: Send + 'static>(
        &mut self,
        name: &str,
        callback: impl Fn(Arc<C>, Resolver) -> BoxFuture<'static, Result<T>> + Send + Sync + 'static,
    ) -> Result<()> {
        self.set_method(name, Method::Bound(Box::new(callback)))
    }

    fn set_method<C: ?Sized + Send + Sync + 'static, T: Send + 'static>(
        &mut self,
        name: &str,
        method: Method<C, T>,
    ) -> Result<()> {
        let disposal = CallbackDisposal::new(None, None);
        if let Err(error) = method.validate(name) {
            return retire_registration(&disposal, method, Err(error));
        }
        retire_registration(&disposal, self.methods.insert(name, method), Ok(()))
    }
}

impl Container {
    /// 在本容器中声明可调用方法。再次登记更新默认实现。
    pub fn register_method<C: ?Sized + Send + Sync + 'static, T: Send + 'static>(
        &self,
        name: &str,
        signature: Vec<ConstructorParam>,
        callback: impl Fn(Arc<C>, CallArguments<'_>, Resolver) -> BoxFuture<'static, Result<T>>
        + Send
        + Sync
        + 'static,
    ) -> Result<()> {
        self.set_method(name, Method::Declared(signature, Box::new(callback)))
    }

    /// 登记类型化方法的替代回调，后登记覆盖本方法的旧绑定。
    /// 命中绑定时跳过默认签名、调用实参和属性解析。
    pub fn bind_method<C: ?Sized + Send + Sync + 'static, T: Send + 'static>(
        &self,
        name: &str,
        callback: impl Fn(Arc<C>, Resolver) -> BoxFuture<'static, Result<T>> + Send + Sync + 'static,
    ) -> Result<()> {
        self.set_method(name, Method::Bound(Box::new(callback)))
    }

    fn set_method<C: ?Sized + Send + Sync + 'static, T: Send + 'static>(
        &self,
        name: &str,
        method: Method<C, T>,
    ) -> Result<()> {
        let validation = method.validate(name);
        let mut state = lock(&self.inner.state);
        let disposal = CallbackDisposal::new(None, Some(state.root_resources.registrar()));
        if let Err(error) = validation.and_then(|()| {
            if state.terminated {
                Err(container_error("container_closed", "容器已关闭"))
            } else {
                Ok(())
            }
        }) {
            drop(state);
            return retire_registration(&disposal, method, Err(error));
        }
        let previous = state.methods.insert(name, method);
        drop(state);
        retire_registration(&disposal, previous, Ok(()))
    }

    /// 检查类型化方法是否存在替代绑定，不执行回调。
    pub fn has_method_binding<C: ?Sized + Send + Sync + 'static>(&self, name: &str) -> bool {
        let state = lock(&self.inner.state);
        !state.terminated
            && state
                .methods
                .get(&ServiceKey::typed::<C>(), name)
                .is_some_and(|entry| entry.bound.is_some())
    }

    /// 在根生命周期调用已声明或绑定的方法。
    pub fn call_method<'a, C: ?Sized + Send + Sync + 'static, T: Send + 'static>(
        &'a self,
        method: MethodCall<'a, C>,
        overrides: ParameterOverrides,
    ) -> impl Future<Output = Result<T>> + Send + 'a {
        invoke_operation(|| self.root_resolver(), overrides, method)
    }
}

impl Scope {
    /// 在本作用域调用已声明或绑定的方法。
    pub fn call_method<'a, C: ?Sized + Send + Sync + 'static, T: Send + 'static>(
        &'a self,
        method: MethodCall<'a, C>,
        overrides: ParameterOverrides,
    ) -> impl Future<Output = Result<T>> + Send + 'a {
        invoke_operation(|| Ok(self.resolver()), overrides, method)
    }
}

impl Resolver {
    /// 沿已有解析链及其作用域调用已声明或绑定的方法。
    pub fn call_method<'a, C: ?Sized + Send + Sync + 'static, T: Send + 'static>(
        &'a self,
        method: MethodCall<'a, C>,
        overrides: ParameterOverrides,
    ) -> impl Future<Output = Result<T>> + Send + 'a {
        invoke_operation(|| Ok(self.clone().for_new_lookup()), overrides, method)
    }
}

impl ApplicationState {
    /// 在当前应用入口作用域或根生命周期调用已声明或绑定的方法。
    pub fn call_method<'a, C: ?Sized + Send + Sync + 'static, T: Send + 'static>(
        &'a self,
        method: MethodCall<'a, C>,
        overrides: ParameterOverrides,
    ) -> impl Future<Output = Result<T>> + Send + 'a {
        let container = self.container();
        invoke_operation(
            || {
                container
                    .current_entry_scope()
                    .map_or_else(|| container.root_resolver(), |scope| Ok(scope.resolver()))
            },
            overrides,
            method,
        )
    }
}

impl<C: ?Sized + Send + Sync + 'static, T: Send + 'static> CallOperation<T> for MethodCall<'_, C> {
    fn validate(&self) -> Result<()> {
        validate_name(self.name)
    }

    async fn execute(
        self,
        run: &ArgumentRun<'_>,
        overrides: ParameterOverrides,
    ) -> Result<ResourceInlineSlot<T>> {
        let Self {
            name,
            receiver,
            implicit,
            use_binding,
        } = self;
        let class = ServiceKey::typed::<C>();
        let explicit_receiver = receiver.is_some();
        if implicit
            && !run.claim(|state| {
                state
                    .methods
                    .get(&class, name)
                    .is_some_and(|entry| entry.declared.is_some())
            })?
        {
            return Err(missing_method());
        }
        // 类入口先取得接收者，再选方法快照。解析接收者不能看见方法的覆盖参数。
        let mut receiver = match receiver {
            Some(receiver) => run.owner.slot(receiver)?,
            None => run.poll_stage(run.caller.resolve::<C>()).await?,
        };
        let method = run
            .claim(|state| {
                state.methods.get(&class, name).and_then(|entry| {
                    entry
                        .bound
                        .as_ref()
                        .filter(|_| use_binding)
                        .or(entry.declared.as_ref())
                        .cloned()
                })
            })?
            .ok_or_else(missing_method)?;
        let method = Arc::downcast::<Method<C, T>>(method).map_err(|method| {
            drop(run.owner.slot(method));
            container_error(
                "container_method_output_mismatch",
                "方法输出类型与调用声明不同",
            )
        })?;
        let mut method = run.owner.slot(method)?;
        let method = method.get_mut().ok_or_else(missing_slot)?;
        let receiver = receiver.get_mut().ok_or_else(missing_slot)?;

        // 固定 BoundMethod 的类入口直接分派已构造对象，只在显式对象入口建立新类型帧。
        let mut caller = run.caller.clone();
        let _frame = if explicit_receiver && !in_call_stack(&caller, &class) {
            let active = Arc::new(AtomicBool::new(true));
            let frame = ActiveFactory(Arc::clone(&active));
            let parent = std::iter::successors(caller.method_consumer.as_ref(), |frame| {
                frame.parent.as_ref()
            })
            .find(|frame| frame.active.load(Ordering::Acquire))
            .cloned();
            caller.method_consumer = Some(Arc::new(MethodConsumer {
                class: class.clone(),
                active: Arc::clone(&active),
                parent,
            }));
            caller.build_consumer = Some((class, active));
            Some(frame)
        } else {
            None
        };
        let scoped = ArgumentRun {
            caller: &caller,
            inner: run.inner,
            owner: run.owner,
            consumer_key: caller.active_consumer_frame().map(|(key, _)| key),
            descriptor: None,
        };
        match method.as_ref() {
            Method::Declared(signature, callback) => {
                scoped
                    .invoke_callable(signature, overrides, |arguments| {
                        callback(Arc::clone(receiver), arguments, caller.clone())
                    })
                    .await
            }
            Method::Bound(callback) => {
                scoped.start()?;
                let future = run
                    .owner
                    .run(|| callback(Arc::clone(receiver), caller.clone()))?;
                scoped.poll_stage(future).await
            }
        }
    }
}

fn in_call_stack(caller: &Resolver, class: &ServiceKey) -> bool {
    caller.class_build(class).is_some()
        || caller
            .active_consumer_frame()
            .is_some_and(|(key, _)| key == class)
        || std::iter::successors(caller.method_consumer.as_deref(), |frame| {
            frame.parent.as_deref()
        })
        .any(|frame| &frame.class == class && frame.active.load(Ordering::Acquire))
}

fn missing_method() -> crate::Error {
    container_error("container_method_missing", "容器中没有声明或绑定该方法")
}

#[cfg(test)]
#[path = "method_call_tests.rs"]
mod tests;

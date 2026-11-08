//! 真实绑定快照与由调用方作用域管理的原工厂执行。

use std::{
    any::TypeId,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use tokio::sync::Notify;

use super::super::class_binding::ClassFactory;
use super::super::{
    Attempt, CacheKey, Container, ENTRY_SCOPE, Factory, Inner, Lifetime, ParameterOverrides,
    Resolver, Scope, ScopeStatus, ServiceKey, Value, WaitEdge, container_error, extensions, lock,
};
use super::raw_delivery::{CallFrame, CallerFrame, RawResult};
use crate::core::{Cancellation, ExecutionContext};
use crate::foundation::resources::{ResourceRegistrar, ResourceTaskCompletion};
use crate::{BoxFuture, Error, Result};

/// 显式登记的原工厂快照，支持用户闭包和类名包装工厂。绑定替换后，已有句柄仍调用取得时的工厂。
///
/// 调用者提供实际的 [Resolver]。直接调用使用该解析器的容器与资源作用域，
/// 跳过原键的实例、共享缓存、装饰和解析历史。闭包内部显式解析的服务仍走通常流程。
#[derive(Clone)]
pub struct FactoryHandle {
    factory: Factory,
    class_factory: Option<Arc<ClassFactory>>,
    output: TypeId,
    service: ServiceKey,
}

/// 绑定目录中显式登记了工厂的条目。
pub struct BindingSnapshot {
    /// 绑定登记时的原服务键。
    pub key: ServiceKey,
    /// 原工厂句柄。实例覆盖保留原工厂。
    pub concrete: FactoryHandle,
    /// 绑定声明为单例或作用域共享时为真。
    pub shared: bool,
}

impl FactoryHandle {
    /// 判断两个句柄是否指向同一个已登记工厂。
    #[must_use]
    pub fn same_factory(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.factory, &other.factory)
    }

    /// 使用空参数集合执行原工厂。
    pub fn invoke<T: ?Sized + Send + Sync + 'static>(
        &self,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<Arc<T>>> {
        self.invoke_with(resolver, ParameterOverrides::new())
    }

    /// 执行原工厂，显式实参由 [`Resolver::parameters`] 提供。
    ///
    /// 轮询需要 `Tokio` 运行时。作用域关闭、执行取消或声明输出类型不匹配时返回错误。
    /// 输出类型不匹配会在工厂运行前拒绝。类名包装工厂也支持调用容器声明的同对象视图。
    pub fn invoke_with<T: ?Sized + Send + Sync + 'static>(
        &self,
        mut resolver: Resolver,
        parameters: ParameterOverrides,
    ) -> BoxFuture<'static, Result<Arc<T>>> {
        let requested = TypeId::of::<Arc<T>>();
        let with_parameters = !parameters.is_empty();
        let target = ServiceKey::typed::<T>();
        let factory = self
            .class_factory
            .as_ref()
            .filter(|_| self.output != requested)
            .map_or_else(
                || Arc::clone(&self.factory),
                |class| class.factory(target, requested),
            );
        let previous = resolver.take_factory_inputs();
        let caller = CallerFrame(CallFrame::new(
            factory,
            parameters,
            resolver.resources.clone(),
        ));
        // 两种新输入已有frame票据；旧refs在返回future前实际清完，关闭不能先缓存Ok。
        let unknown = previous.dispose(caller.0.disposal_credential());
        caller.0.complete_startup(unknown);
        let key = RawKey {
            service: self.service.clone(),
            output: self.output,
            class_factory: self.class_factory.clone(),
            with_parameters,
        };
        Box::pin(async move {
            let result = key
                .invoke_admitted::<T>(resolver, Arc::clone(&caller.0))
                .await;
            caller.finish(result).await
        })
    }

    #[cfg(test)]
    pub(in crate::container) fn invoke_internal<T: ?Sized + Send + Sync + 'static>(
        &self,
        resolver: Resolver,
        parameters: ParameterOverrides,
    ) -> BoxFuture<'static, Result<Arc<T>>> {
        self.invoke_with(resolver, parameters)
    }
}
struct RawKey {
    service: ServiceKey,
    output: TypeId,
    class_factory: Option<Arc<ClassFactory>>,
    with_parameters: bool,
}
impl RawKey {
    async fn invoke_admitted<T: ?Sized + Send + Sync + 'static>(
        &self,
        resolver: Resolver,
        frame: Arc<CallFrame>,
    ) -> Result<Arc<T>> {
        let inner = resolver.container()?.inner;
        {
            let state = lock(&inner.state);
            resolver.check_open(&state)?;
            let supported = if self.output == TypeId::of::<Arc<T>>() {
                true
            } else if let Some(class) = &self.class_factory {
                class.supports_view(
                    &state,
                    &resolver,
                    ServiceKey::typed::<T>(),
                    TypeId::of::<Arc<T>>(),
                    self.with_parameters,
                )?
            } else {
                false
            };
            if !supported {
                return Err(container_error(
                    "service_type_mismatch",
                    "原工厂声明输出与请求类型不同",
                ));
            }
        }
        let execution = ExecutionContext::current();
        let entry_scope = ENTRY_SCOPE
            .try_with(|scope| {
                (Arc::ptr_eq(&scope.container.inner, &inner)
                    && scope.id == resolver.scope
                    && Arc::ptr_eq(&scope.closed, &resolver.closed)
                    && resolver
                        .resources
                        .as_ref()
                        .is_some_and(|owner| owner.same_owner(&scope.resources.registrar())))
                .then(|| Arc::clone(scope))
            })
            .ok()
            .flatten();
        check_execution(execution.as_ref())?;
        let resources = resolver.resources()?;
        let caller_cancel = Cancellation::new();
        let _cancel_on_drop = CancelRawOnDrop(caller_cancel.clone());
        let (edge, receiver) = self.submit_raw(
            &resolver,
            &inner,
            &frame,
            RawContext {
                execution: execution.clone(),
                entry_scope,
                extension_context: extensions::extension_context(),
                caller_cancel,
                resources: resources.clone(),
            },
        )?;
        if let Some(error) = edge.2 {
            if let Err(cleanup) = edge.1.wait().await {
                return Err(crate::foundation::resources::ResourceTaskRejection::error(
                    error, cleanup,
                ));
            }
            resolver.check_open(&lock(&inner.state))?;
            return Err(error);
        }
        let notification = receiver.await;
        #[cfg(test)]
        super::super::directory_tests_v8::after_notification();
        let completed = edge.1.wait().await;
        drop(edge.0);
        if notification.is_err() {
            if let Err(cleanup) = completed {
                return Err(cleanup);
            }
            resolver.check_open(&lock(&inner.state))?;
            resources.with_open_owner(|| ())?;
            return Err(container_error(
                "service_initialization_cancelled",
                "原工厂受管任务已中断",
            ));
        }
        // 这里只移动frame内部所有权，不在State/owner锁中析构任何用户对象。
        let delivery = (|| {
            let state = lock(&inner.state);
            check_execution(execution.as_ref())?;
            resolver.check_open(&state)?;
            resources.with_open_owner(|| frame.take_delivery())?
        })();
        let delivery = match delivery {
            Ok(delivery) => delivery,
            Err(primary) => {
                return match completed {
                    Err(cleanup) => Err(crate::foundation::resources::ResourceTaskFailure::error(
                        primary, cleanup,
                    )),
                    Ok(()) => Err(primary),
                };
            }
        };
        delivery.finish::<T>(completed.err())
    }
    fn submit_raw(
        &self,
        resolver: &Resolver,
        inner: &Arc<Inner>,
        frame: &Arc<CallFrame>,
        context: RawContext,
    ) -> Result<(RawAdmission, tokio::sync::oneshot::Receiver<()>)> {
        let RawContext {
            execution,
            entry_scope,
            extension_context,
            caller_cancel,
            resources,
        } = context;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let edge = {
            let mut state = lock(&inner.state);
            resolver.check_open(&state)?;
            check_execution(execution.as_ref())?;
            let attempt = Arc::new(Attempt {
                id: state.id()?,
                service: self.service.clone(),
                outcome: Mutex::new(None),
                changed: Notify::new(),
            });
            let edge = resolver.wait_edge(inner, &mut state, &attempt)?;
            let activity = Arc::new(AtomicBool::new(true));
            let mut child = resolver.clone();
            child.constructing = Some((attempt.id, Arc::clone(&activity)));
            // completion创建后才把唯一已准入参数持有者交给child。
            let termination = Arc::clone(&state.termination);
            let weak = Arc::downgrade(inner);
            let done = Arc::clone(&attempt);
            let completion = ResourceTaskCompletion::new(move || complete_raw(&weak, &done));
            let inputs = match resources.reserve_observed(&completion, || frame.take_inputs()) {
                Ok(inputs) => inputs,
                Err(error) => {
                    // WaitEdge的析构也取State；owner已退役frame时须先离锁。
                    drop(state);
                    drop(edge);
                    return Err(error);
                }
            };
            let mut admitted = inputs.parameters;
            admitted.owner = Some(resources.clone());
            admitted.completion = Some(completion.clone());
            let factory = RawFactoryCapture {
                factory: Some(inputs.factory),
                completion: completion.clone(),
            };
            child.factory_parameters = Arc::new(admitted);
            child.factory_completion = Some(completion.clone());
            let guard = RawInvocation {
                inner: Arc::downgrade(inner),
                id: attempt.id,
                activity,
                closed: Arc::clone(&resolver.closed),
                owner: resources.clone(),
                completion: completion.clone(),
            };
            state.raw_attempts.insert(
                attempt.id,
                RawRecord {
                    #[cfg(test)]
                    attempt,
                    completion: completion.clone(),
                    result_committed: false,
                },
            );
            // ResourceRegistrar 在真实 owner 的关闭闸门下登记；用户代码在受管 task 内运行。
            // guard 不能在 State 锁内被失败的 spawn 析构，故先离锁后提交。
            drop(state);
            #[cfg(test)]
            super::super::directory_tests_v8::after_reservation();
            #[cfg(test)]
            super::super::directory_tests_v9::after_reservation(completion.clone());
            let work = RawWork {
                guard,
                factory,
                resolver: child,
                execution,
                caller_cancel,
                termination,
                frame: Arc::clone(frame),
                sender,
                entry_scope,
                extension_context,
            };
            let registration = resources.spawn_observed(completion.clone(), move |owner_cancel| {
                work.run(owner_cancel)
            });
            if let Err(error) = registration {
                (edge, completion, Some(error))
            } else {
                (edge, completion, None)
            }
        };
        Ok((edge, receiver))
    }
}

type RawAdmission = (Option<WaitEdge>, ResourceTaskCompletion, Option<Error>);
struct RawContext {
    execution: Option<ExecutionContext>,
    entry_scope: Option<Arc<Scope>>,
    extension_context: Vec<(usize, CacheKey, u64)>,
    caller_cancel: Cancellation,
    resources: ResourceRegistrar,
}

struct RawWork {
    guard: RawInvocation,
    factory: RawFactoryCapture,
    resolver: Resolver,
    execution: Option<ExecutionContext>,
    caller_cancel: Cancellation,
    termination: Arc<ScopeStatus>,
    frame: Arc<CallFrame>,
    sender: tokio::sync::oneshot::Sender<()>,
    entry_scope: Option<Arc<Scope>>,
    extension_context: Vec<(usize, CacheKey, u64)>,
}
impl RawWork {
    async fn run(self, owner_cancel: Cancellation) {
        let Self {
            guard,
            factory,
            resolver,
            execution,
            caller_cancel,
            termination,
            frame,
            sender,
            entry_scope,
            extension_context,
        } = self;
        let body = async move {
            #[cfg(test)]
            let completion = guard.completion.clone();
            let result = guard
                .run(
                    factory,
                    resolver,
                    execution,
                    caller_cancel,
                    owner_cancel,
                    termination,
                )
                .await;
            frame.store(result);
            let _ = sender.send(());
            #[cfg(test)]
            super::super::directory_tests_v8::after_send(completion.clone()).await;
            #[cfg(test)]
            super::super::directory_tests_v10::after_send(completion).await;
        };
        extensions::with_extension_context(extension_context, async move {
            if let Some(scope) = entry_scope {
                ENTRY_SCOPE.scope(scope, body).await;
            } else {
                body.await;
            }
        })
        .await;
    }
}

fn check_execution(context: Option<&ExecutionContext>) -> Result<()> {
    if let Some(context) = context {
        if context.is_cancelled() {
            return Err(container_error("execution_cancelled", "当前执行已取消"));
        }
        if context
            .remaining_budget()
            .is_some_and(|budget| budget.is_zero())
        {
            return Err(container_error(
                "execution_deadline_exceeded",
                "当前执行期限已到",
            ));
        }
    }
    Ok(())
}

struct CancelRawOnDrop(Cancellation);
impl Drop for CancelRawOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub(in crate::container) struct RawRecord {
    #[cfg(test)]
    pub(in crate::container) attempt: Arc<Attempt>,
    pub(in crate::container) completion: ResourceTaskCompletion,
    result_committed: bool,
}

struct RawFactoryFuture {
    future: Option<BoxFuture<'static, Result<Value>>>,
    completion: ResourceTaskCompletion,
}
impl RawFactoryFuture {
    fn clear(&mut self) -> bool {
        let report = crate::foundation::lifecycle::disposal::dispose(
            self.future.take(),
            Some(
                crate::foundation::lifecycle::disposal::Credential::Completion(
                    self.completion.clone(),
                ),
            ),
        );
        if report.seen {
            self.completion.note_diagnostic();
        }
        report.seen
    }
}
impl Drop for RawFactoryFuture {
    fn drop(&mut self) {
        self.clear();
    }
}
impl std::future::Future for RawFactoryFuture {
    type Output = RawResult;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.get_mut();
        let (polled, report) = crate::foundation::lifecycle::disposal::scope(
            Some(
                crate::foundation::lifecycle::disposal::Credential::Completion(
                    this.completion.clone(),
                ),
            ),
            || {
                crate::foundation::lifecycle::catch_isolated(|| {
                    this.future.as_mut().map_or(
                        std::task::Poll::Ready(Err(container_error(
                            "resource_task_interrupted",
                            "原工厂future已结束",
                        ))),
                        |future| future.as_mut().poll(cx),
                    )
                })
            },
        );
        if report.seen {
            this.completion.note_diagnostic();
        }
        match polled {
            Ok(std::task::Poll::Pending) => std::task::Poll::Pending,
            Ok(std::task::Poll::Ready(result)) => {
                let mut result = RawResult::producer(result, this.completion.clone());
                if this.clear() {
                    result.suppress_success();
                }
                std::task::Poll::Ready(result)
            }
            Err(()) => {
                this.completion.note_unknown();
                this.clear();
                std::task::Poll::Ready(RawResult::producer(
                    Err(container_error(
                        "resource_task_interrupted",
                        "原工厂future轮询异常",
                    )),
                    this.completion.clone(),
                ))
            }
        }
    }
}
struct RawFactoryCapture {
    factory: Option<Factory>,
    completion: ResourceTaskCompletion,
}
impl Drop for RawFactoryCapture {
    fn drop(&mut self) {
        let report = crate::foundation::lifecycle::disposal::dispose(
            self.factory.take(),
            Some(
                crate::foundation::lifecycle::disposal::Credential::Completion(
                    self.completion.clone(),
                ),
            ),
        );
        if report.seen {
            self.completion.note_diagnostic();
        }
    }
}

struct RawInvocation {
    inner: Weak<Inner>,
    id: u64,
    activity: Arc<AtomicBool>,
    closed: Arc<ScopeStatus>,
    owner: ResourceRegistrar,
    completion: ResourceTaskCompletion,
}
impl RawInvocation {
    async fn run(
        self,
        factory: RawFactoryCapture,
        resolver: Resolver,
        execution: Option<ExecutionContext>,
        caller_cancel: Cancellation,
        owner_cancel: Cancellation,
        termination: Arc<ScopeStatus>,
    ) -> RawResult {
        let completion = self.completion.clone();
        let work = async {
            if let Err(error) = check_execution(execution.as_ref()) {
                return RawResult::producer(Err(error), completion.clone());
            }
            let (called, report) = crate::foundation::lifecycle::disposal::scope(
                Some(
                    crate::foundation::lifecycle::disposal::Credential::Completion(
                        completion.clone(),
                    ),
                ),
                || {
                    crate::foundation::lifecycle::catch_isolated(|| {
                        factory
                            .factory
                            .as_ref()
                            .map(|factory| factory(resolver.clone()))
                    })
                },
            );
            if report.seen {
                completion.note_diagnostic();
            }
            let future = if let Ok(Some(future)) = called {
                RawFactoryFuture {
                    future: Some(future),
                    completion: completion.clone(),
                }
            } else {
                completion.note_unknown();
                return RawResult::producer(
                    Err(container_error(
                        "resource_task_interrupted",
                        "原工厂同步构造异常",
                    )),
                    completion.clone(),
                );
            };
            let mut result = if let Some(context) = &execution {
                match context.run(future).await {
                    Ok(result) => result,
                    Err(error) => RawResult::producer(Err(error), completion.clone()),
                }
            } else {
                future.await
            };
            if let Err(error) = check_execution(execution.as_ref()) {
                result.reject(error);
            }
            result
        };
        let outcome = tokio::select! {
            biased;
            ()=caller_cancel.cancelled()=>RawResult::producer(Err(container_error("service_initialization_cancelled","原工厂调用者已取消")),completion.clone()),
            ()=owner_cancel.cancelled()=>RawResult::producer(Err(container_error("scope_closed","资源所有者已关闭")),completion.clone()),
            ()=termination.wait_closed()=>RawResult::producer(Err(container_error("container_closed","容器已关闭")),completion.clone()),
            ()=resolver.closed.wait_closed()=>RawResult::producer(Err(container_error("scope_closed","生命周期已关闭")),completion.clone()),
            result=work=>result,
        };
        self.finish(outcome, &owner_cancel)
    }

    fn finish(&self, mut result: RawResult, cancellation: &Cancellation) -> RawResult {
        self.activity.store(false, Ordering::Release);
        let accepted = self.accept_result(cancellation);
        // 用户结果/错误的最后析构均在State与owner闸门之外，原拒绝码先保存。
        if let Err(error) = accepted {
            result.reject_lifecycle(error);
        }
        result
    }

    fn accept_result(&self, cancellation: &Cancellation) -> Result<()> {
        if let Some(inner) = self.inner.upgrade() {
            let mut state = lock(&inner.state);
            if state.terminated {
                Err(container_error("container_closed", "容器已关闭"))
            } else if self.closed.closed.load(Ordering::Acquire) {
                Err(container_error("scope_closed", "生命周期已关闭"))
            } else {
                #[cfg(test)]
                super::super::directory_tests_v3::before_owner_commit(&self.owner)?;
                self.owner
                    .with_open_owner(|| {
                        if cancellation.is_cancelled() {
                            Err(container_error("scope_closed", "资源任务已取消"))
                        } else if let Some(record) = state.raw_attempts.get_mut(&self.id) {
                            if record.result_committed {
                                Err(container_error(
                                    "service_initialization_cancelled",
                                    "原工厂结果已提交",
                                ))
                            } else {
                                record.result_committed = true;
                                Ok(())
                            }
                        } else {
                            Err(container_error(
                                "service_initialization_cancelled",
                                "原工厂调用已结束",
                            ))
                        }
                    })
                    .and_then(std::convert::identity)
            }
        } else {
            Err(container_error("container_closed", "容器已释放"))
        }
    }
}
impl Drop for RawInvocation {
    fn drop(&mut self) {
        self.activity.store(false, Ordering::Release);
    }
}

fn complete_raw(inner: &Weak<Inner>, attempt: &Arc<Attempt>) {
    let removed = if let Some(inner) = inner.upgrade() {
        let mut state = lock(&inner.state);
        state.edges.retain(|(from, _), _| *from != attempt.id);
        let removed = state.raw_attempts.remove(&attempt.id);
        *lock(&attempt.outcome) = Some(Ok(super::super::plain(())));
        removed
    } else {
        *lock(&attempt.outcome) = Some(Ok(super::super::plain(())));
        None
    };
    drop(removed);
    attempt.changed.notify_waiters();
}

impl Container {
    /// 取得当前根作用域的解析器，用于直接调用原工厂。
    ///
    /// 解析器持有弱容器引用。根作用域关闭后须重新从仍开放的容器取得解析器。
    pub fn factory_resolver(&self) -> Result<Resolver> {
        let state = lock(&self.inner.state);
        if state.terminated {
            return Err(container_error("container_closed", "容器已关闭"));
        }
        if state.root_closed.closed.load(Ordering::Acquire) {
            return Err(container_error("scope_closed", "根生命周期已关闭"));
        }
        Ok(self.resolver(
            state.root_scope,
            Arc::clone(&state.root_closed),
            state.root_resources.registrar(),
        ))
    }

    #[cfg(test)]
    pub(in crate::container) fn factory_resolver_internal(&self) -> Result<Resolver> {
        self.factory_resolver()
    }
}

impl Scope {
    /// 取得当前作用域的解析器，用于直接调用原工厂。
    ///
    /// 工厂登记的资源随本作用域关闭，作用域关闭后返回错误。
    pub fn factory_resolver(&self) -> Result<Resolver> {
        let state = lock(&self.container.inner.state);
        if state.terminated {
            return Err(container_error("container_closed", "容器已关闭"));
        }
        if self.closed.closed.load(Ordering::Acquire) {
            return Err(container_error("scope_closed", "生命周期已关闭"));
        }
        Ok(self.container.resolver(
            self.id,
            Arc::clone(&self.closed),
            self.resources.registrar(),
        ))
    }

    #[cfg(test)]
    pub(in crate::container) fn factory_resolver_internal(&self) -> Result<Resolver> {
        self.factory_resolver()
    }
}

impl Container {
    /// 按登记顺序返回显式工厂快照，无需执行用户工厂。
    ///
    /// 仅有实例、别名或编译构造描述的键由各自目录管理。
    pub fn get_bindings(&self) -> Result<Vec<BindingSnapshot>> {
        let state = lock(&self.inner.state);
        if state.terminated {
            return Err(container_error("container_closed", "容器已关闭"));
        }
        state
            .binding_order
            .iter()
            .filter_map(|key| {
                let binding = state.bindings.get(key)?;
                binding.factory.as_ref().map(|factory| {
                    let output = binding.factory_output.ok_or_else(|| {
                        container_error("service_factory_metadata_missing", "原工厂缺少声明输出")
                    })?;
                    Ok(BindingSnapshot {
                        key: key.clone(),
                        concrete: FactoryHandle {
                            factory: Arc::clone(factory),
                            class_factory: binding.class_factory.clone(),
                            output,
                            service: key.clone(),
                        },
                        shared: binding.lifetime != Lifetime::Bind,
                    })
                })
            })
            .collect()
    }

    #[cfg(test)]
    pub(in crate::container) fn bindings_snapshot(&self) -> Result<Vec<BindingSnapshot>> {
        self.get_bindings()
    }
}

#[cfg(test)]
mod public_contract_tests {
    use crate::{
        Error, Result,
        container::{
            BindingSnapshot, ContainerBuilder, FactoryHandle, ParameterKey, ParameterOverrides,
            ParameterValue, ServiceKey,
        },
    };
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    #[tokio::test]
    async fn raw_public_api_uses_supplied_owner_and_rejects_wrong_output_before_call() -> Result<()>
    {
        let called = Arc::new(AtomicUsize::new(0));
        let cleaned = Arc::new(AtomicUsize::new(0));
        let original_calls = Arc::clone(&called);
        let original_cleanup = Arc::clone(&cleaned);
        let key = ServiceKey::named("original");
        let parameter = ParameterKey::name("answer");
        let mut builder = ContainerBuilder::new();
        builder.singleton_key::<usize, _>(key.clone(), move |resolver| {
            original_calls.fetch_add(1, Ordering::SeqCst);
            let cleanup = Arc::clone(&original_cleanup);
            Box::pin(async move {
                resolver.resources()?.on_shutdown(move || {
                    Box::pin(async move {
                        cleanup.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                })?;
                let root_error = resolver
                    .resolve_root::<usize>()
                    .await
                    .err()
                    .ok_or_else(|| {
                        Error::custom("test_error_missing", "原工厂活跃时不能切换根作用域")
                    })?;
                assert_eq!(root_error.code(), "root_resolution_during_construction");
                let parameters = resolver.parameters();
                parameters
                    .get(&ParameterKey::name("answer"))
                    .map_or_else(|| Ok(Arc::new(0)), ParameterValue::downcast::<usize>)
            })
        });
        let source = builder.build();
        let entries: Vec<BindingSnapshot> = source.get_bindings()?;
        let entry = entries
            .into_iter()
            .next()
            .ok_or_else(|| Error::custom("test_binding_missing", "缺少原工厂快照"))?;
        assert_eq!(entry.key, key);
        assert!(entry.shared);
        let factory: FactoryHandle = entry.concrete;
        assert_eq!(called.load(Ordering::SeqCst), 0);
        drop(source);

        let target = ContainerBuilder::new().build();
        let scope = target.scope()?;
        let root = target.factory_resolver()?;
        let scoped = scope.factory_resolver()?;
        let mismatch = factory
            .invoke::<String>(scoped.clone())
            .await
            .err()
            .ok_or_else(|| Error::custom("test_error_missing", "错型不能运行工厂"))?;
        assert_eq!(mismatch.code(), "service_type_mismatch");
        assert_eq!(called.load(Ordering::SeqCst), 0);

        let mut parameters = ParameterOverrides::new();
        parameters.insert(parameter, ParameterValue::new(42_usize));
        assert_eq!(*factory.invoke_with::<usize>(scoped, parameters).await?, 42);
        assert_eq!(*factory.invoke::<usize>(root).await?, 0);
        assert!(!target.resolved_key(&key)?);
        assert_eq!(called.load(Ordering::SeqCst), 2);
        assert_eq!(cleaned.load(Ordering::SeqCst), 0);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        scope.shutdown(deadline).await?;
        assert_eq!(cleaned.load(Ordering::SeqCst), 1);
        target.shutdown_root(deadline).await?;
        assert_eq!(cleaned.load(Ordering::SeqCst), 2);
        Ok(())
    }
}

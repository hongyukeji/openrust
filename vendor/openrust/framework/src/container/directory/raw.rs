//! 尚未公开的原工厂调用与交付原型，由容器内部契约测试覆盖。

use super::super::*;
use super::raw_delivery::{CallFrame, CallerFrame, RawResult};
use crate::core::{Cancellation, ExecutionContext};
use crate::foundation::resources::ResourceTaskCompletion;

// 仅用户 Closure 纯内核快照；不公开未完成的 class 包装器/constructor API。
/// 原用户 Closure 的可调用稳定快照；调用者显式提供真实 Resolver owner。
#[derive(Clone)]
pub(in crate::container) struct FactoryHandle {
    factory: Factory,
    output: TypeId,
    service: ServiceKey,
}

/// 不执行用户代码取得的真实 binding 目录条目；instance-only/alias-only 不入目录。
pub(in crate::container) struct BindingSnapshot {
    /// 规范原键，不包含本次期望返回类型或临时 alias 展开结果。
    pub(in crate::container) key: ServiceKey,
    /// 登记时的原 factory，instance 覆盖不会改写此快照。
    pub(in crate::container) concrete: FactoryHandle,
    /// 原 binding 的共享位，不取 instance 覆盖后的 effective lifetime。
    pub(in crate::container) shared: bool,
}

impl FactoryHandle {
    /// 比较两个句柄是否持有同一原工厂 Arc，不公开内存地址。
    pub(in crate::container) fn same_factory(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.factory, &other.factory)
    }

    pub(in crate::container) fn invoke_internal<T: ?Sized + Send + Sync + 'static>(
        &self,
        mut resolver: Resolver,
        parameters: ParameterOverrides,
    ) -> BoxFuture<'static, Result<Arc<T>>> {
        let previous = resolver.take_factory_inputs();
        let caller = CallerFrame(CallFrame::new(
            Arc::clone(&self.factory),
            parameters,
            resolver.resources.clone(),
        ));
        // 两种新输入已有frame票据；旧refs在返回future前实际清完，关闭不能先缓存Ok。
        let unknown = previous.dispose(caller.0.disposal_credential());
        caller.0.complete_startup(unknown);
        let key = RawKey {
            service: self.service.clone(),
            output: self.output,
        };
        Box::pin(async move {
            let result = key
                .invoke_admitted::<T>(resolver, Arc::clone(&caller.0))
                .await;
            caller.finish(result).await
        })
    }
}
struct RawKey {
    service: ServiceKey,
    output: TypeId,
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
            if self.output != TypeId::of::<Arc<T>>() {
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
        let accepted = (|| {
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
        })();
        // 用户结果/错误的最后析构均在State与owner闸门之外，原拒绝码先保存。
        if let Err(error) = accepted {
            result.reject_lifecycle(error);
        }
        result
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
        *lock(&attempt.outcome) = Some(Ok(Arc::new(())));
        removed
    } else {
        *lock(&attempt.outcome) = Some(Ok(Arc::new(())));
        None
    };
    drop(removed);
    attempt.changed.notify_waiters();
}

impl Container {
    pub(in crate::container) fn factory_resolver_internal(&self) -> Result<Resolver> {
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
}

impl Scope {
    pub(in crate::container) fn factory_resolver_internal(&self) -> Result<Resolver> {
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
}

impl Container {
    pub(in crate::container) fn bindings_snapshot(&self) -> Result<Vec<BindingSnapshot>> {
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
                            output,
                            service: key.clone(),
                        },
                        shared: binding.lifetime != Lifetime::Bind,
                    })
                })
            })
            .collect()
    }
}

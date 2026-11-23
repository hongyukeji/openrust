//! Typed asynchronous pipelines shared by HTTP handlers and Artisan commands.

use std::{future::Future, pin::Pin};

use crate::database::{DatabaseError, DatabaseResult, Db, Transaction};

pub type PipelineFuture<'a, R, E> = Pin<Box<dyn Future<Output = Result<R, E>> + Send + 'a>>;
pub type PipelinePipe<T, R, E, C = ()> =
    Box<dyn for<'a> FnOnce(T, Next<'a, T, R, E, C>) -> PipelineFuture<'a, R, E> + Send>;
type Destination<T, R, E, C> =
    Box<dyn for<'a> FnOnce(T, &'a mut C) -> PipelineFuture<'a, R, E> + Send>;

struct Finalizer(Option<Box<dyn FnOnce() + Send>>);

impl Drop for Finalizer {
    fn drop(&mut self) {
        if let Some(callback) = self.0.take() {
            callback();
        }
    }
}

/// A single-use continuation. Returning without calling it short-circuits the remaining stages.
pub struct Next<'a, T, R, E, C = ()> {
    remaining: std::vec::IntoIter<PipelinePipe<T, R, E, C>>,
    destination: Destination<T, R, E, C>,
    context: &'a mut C,
}

impl<'a, T, R, E, C> Next<'a, T, R, E, C> {
    pub fn context(&mut self) -> &mut C {
        self.context
    }

    pub fn run(mut self, value: T) -> PipelineFuture<'a, R, E> {
        match self.remaining.next() {
            Some(pipe) => pipe(value, self),
            None => (self.destination)(value, self.context),
        }
    }
}

pub struct Pipeline<T, E, R = T, C = ()> {
    value: T,
    pipes: Vec<PipelinePipe<T, R, E, C>>,
    finalizer: Option<Box<dyn FnOnce() + Send>>,
}

#[allow(non_snake_case, reason = "保留对应公开方法名")]
impl<T, E, R, C> Pipeline<T, E, R, C> {
    pub fn send(value: T) -> Self {
        Self {
            value,
            pipes: Vec::new(),
            finalizer: None,
        }
    }

    /// Replaces the configured stages.
    pub fn through(mut self, pipes: impl IntoIterator<Item = PipelinePipe<T, R, E, C>>) -> Self {
        self.pipes = pipes.into_iter().collect();
        self
    }

    /// Appends a stage to the pipeline.
    pub fn pipe(
        mut self,
        pipe: impl for<'a> FnOnce(T, Next<'a, T, R, E, C>) -> PipelineFuture<'a, R, E> + Send + 'static,
    ) -> Self {
        self.pipes.push(Box::new(pipe));
        self
    }

    /// Runs after the pipeline completes, with a snapshot of the original input.
    pub fn finally(mut self, callback: impl FnOnce(T) + Send + 'static) -> Self
    where
        T: Clone + Send + 'static,
    {
        let original = self.value.clone();
        self.finalizer = Some(Box::new(move || callback(original)));
        self
    }

    pub async fn then_with(
        self,
        context: &mut C,
        destination: impl for<'a> FnOnce(T, &'a mut C) -> PipelineFuture<'a, R, E> + Send + 'static,
    ) -> Result<R, E> {
        let (value, pipes, finalizer) = (self.value, self.pipes, self.finalizer);
        let _finalizer = Finalizer(finalizer);
        Next {
            remaining: pipes.into_iter(),
            destination: Box::new(destination),
            context,
        }
        .run(value)
        .await
    }
}

impl<T, R: Send + 'static, E: Send + 'static> Pipeline<T, E, R> {
    pub async fn then(
        self,
        destination: impl FnOnce(T) -> Result<R, E> + Send + 'static,
    ) -> Result<R, E> {
        self.then_with(&mut (), |value, _| {
            Box::pin(std::future::ready(destination(value)))
        })
        .await
    }
}

#[allow(non_snake_case, reason = "保留管道公开方法名")]
impl<T: Send + 'static, E: Send + 'static> Pipeline<T, E> {
    pub async fn thenReturn(self) -> Result<T, E> {
        self.then(Ok).await
    }
}

#[allow(non_snake_case, reason = "保留管道公开方法名")]
impl<T: Send + 'static, R: Send + 'static> Pipeline<T, DatabaseError, R, Transaction> {
    /// 仅配置事务；直到 then/thenReturn 才解析连接、开启事务并执行管道。
    pub fn within_transaction(self, connection: Option<&str>) -> TransactionalPipeline<T, R> {
        TransactionalPipeline {
            pipeline: self,
            connection: connection.map(str::to_owned),
        }
    }
}

/// 事务管道保留 Transaction 上下文，阶段和终点使用同一个事务。
pub struct TransactionalPipeline<T, R = T> {
    pipeline: Pipeline<T, DatabaseError, R, Transaction>,
    connection: Option<String>,
}

impl<T: Send + 'static, R: Send + 'static> TransactionalPipeline<T, R> {
    pub fn through(
        mut self,
        pipes: impl IntoIterator<Item = PipelinePipe<T, R, DatabaseError, Transaction>>,
    ) -> Self {
        self.pipeline = self.pipeline.through(pipes);
        self
    }

    pub fn pipe(
        mut self,
        pipe: impl for<'a> FnOnce(
            T,
            Next<'a, T, R, DatabaseError, Transaction>,
        ) -> PipelineFuture<'a, R, DatabaseError>
        + Send
        + 'static,
    ) -> Self {
        self.pipeline = self.pipeline.pipe(pipe);
        self
    }

    pub fn finally(mut self, callback: impl FnOnce(T) + Send + 'static) -> Self
    where
        T: Clone,
    {
        self.pipeline = self.pipeline.finally(callback);
        self
    }

    pub async fn then(
        mut self,
        destination: impl for<'a> FnOnce(T, &'a mut Transaction) -> PipelineFuture<'a, R, DatabaseError>
        + Send
        + 'static,
    ) -> DatabaseResult<R> {
        let _finalizer = Finalizer(self.pipeline.finalizer.take());
        match Db::connection(self.connection.as_deref()) {
            Ok(connection) => {
                let mut run = Some((self.pipeline, destination));
                connection
                    .transaction(
                        move |transaction| {
                            let (pipeline, destination) = run.take().expect("事务管道仅执行一次");
                            Box::pin(pipeline.then_with(transaction, destination))
                        },
                        1,
                    )
                    .await
            }
            Err(error) => Err(error),
        }
    }
}

#[allow(non_snake_case)]
impl<T: Send + 'static> TransactionalPipeline<T> {
    pub async fn thenReturn(self) -> DatabaseResult<T> {
        self.then(|value, _| Box::pin(async move { Ok(value) }))
            .await
    }
}

#[allow(non_snake_case, reason = "保留管道公开方法名")]
impl<T: Send + 'static> Pipeline<T, DatabaseError, T, Transaction> {
    pub async fn thenReturnWithinTransaction(self, connection: Option<&str>) -> DatabaseResult<T> {
        self.within_transaction(connection).thenReturn().await
    }
}

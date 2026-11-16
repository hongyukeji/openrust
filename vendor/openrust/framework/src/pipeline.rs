//! Typed asynchronous pipelines shared by HTTP handlers and Artisan commands.

use std::{future::Future, pin::Pin};

use crate::database::{DB, DatabaseError, DatabaseResult, Transaction};

pub type PipelineFuture<'a, R, E> = Pin<Box<dyn Future<Output = Result<R, E>> + Send + 'a>>;
pub type PipelinePipe<T, R, E, C = ()> =
    Box<dyn for<'a> FnOnce(T, Next<'a, T, R, E, C>) -> PipelineFuture<'a, R, E> + Send>;
type Destination<T, R, E, C> =
    Box<dyn for<'a> FnOnce(T, &'a mut C) -> PipelineFuture<'a, R, E> + Send>;

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

#[allow(non_snake_case, reason = "Matches Laravel-style method names")]
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

    pub async fn thenWith(
        self,
        context: &mut C,
        destination: impl for<'a> FnOnce(T, &'a mut C) -> PipelineFuture<'a, R, E> + Send + 'static,
    ) -> Result<R, E> {
        let (value, pipes, finalizer) = (self.value, self.pipes, self.finalizer);
        let result = Next {
            remaining: pipes.into_iter(),
            destination: Box::new(destination),
            context,
        }
        .run(value)
        .await;
        if let Some(finalizer) = finalizer {
            finalizer();
        }
        result
    }
}

impl<T, R: Send + 'static, E: Send + 'static> Pipeline<T, E, R> {
    pub async fn then(
        self,
        destination: impl FnOnce(T) -> Result<R, E> + Send + 'static,
    ) -> Result<R, E> {
        self.thenWith(&mut (), |value, _| {
            Box::pin(std::future::ready(destination(value)))
        })
        .await
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Pipeline API")]
impl<T: Send + 'static, E: Send + 'static> Pipeline<T, E> {
    pub async fn thenReturn(self) -> Result<T, E> {
        self.then(Ok).await
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Pipeline API")]
impl<T: Send + 'static, R: Send + 'static> Pipeline<T, DatabaseError, R, Transaction> {
    /// Executes every stage and the destination on a single transaction.
    /// The continuation exposes that transaction through `context()`.
    pub async fn withinTransaction(
        mut self,
        connection: Option<&str>,
        destination: impl for<'a> FnOnce(T, &'a mut Transaction) -> PipelineFuture<'a, R, DatabaseError>
        + Send
        + 'static,
    ) -> DatabaseResult<R> {
        let finalizer = self.finalizer.take();
        let result = match DB::connection(connection) {
            Ok(connection) => {
                let mut run = Some((self, destination));
                connection
                    .transaction(
                        move |transaction| {
                            let (pipeline, destination) =
                                run.take().expect("one pipeline transaction attempt");
                            Box::pin(pipeline.thenWith(transaction, destination))
                        },
                        1,
                    )
                    .await
            }
            Err(error) => Err(error),
        };
        if let Some(finalizer) = finalizer {
            finalizer();
        }
        result
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Pipeline API")]
impl<T: Send + 'static> Pipeline<T, DatabaseError, T, Transaction> {
    pub async fn thenReturnWithinTransaction(self, connection: Option<&str>) -> DatabaseResult<T> {
        self.withinTransaction(connection, |value, _| Box::pin(async move { Ok(value) }))
            .await
    }
}

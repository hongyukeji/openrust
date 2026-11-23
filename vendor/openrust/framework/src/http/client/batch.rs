use std::num::NonZeroUsize;

use indexmap::IndexMap;

use super::{Error, Factory, PendingRequest, Pool, Response, Result};

pub type BatchResults = IndexMap<String, Result<Response>>;

/// Counts available to callbacks as requests complete.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BatchProgress {
    pub total_requests: usize,
    pub pending_requests: usize,
    pub failed_requests: usize,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl BatchProgress {
    pub fn processedRequests(self) -> usize {
        self.total_requests - self.pending_requests
    }

    pub fn hasFailures(self) -> bool {
        self.failed_requests != 0
    }
}

type Before<'a> = Box<dyn FnOnce(BatchProgress) + Send + 'a>;
type Progress<'a> = Box<dyn FnMut(BatchProgress, &str, &Response) + Send + 'a>;
type Catch<'a> = Box<dyn FnMut(BatchProgress, &str, &Result<Response>) + Send + 'a>;
type Finished<'a> = Box<dyn FnOnce(BatchProgress, &BatchResults) + Send + 'a>;

/// A one-shot batch with callbacks and results in request registration order.
pub struct Batch<'a> {
    pool: Pool<'a>,
    concurrency: NonZeroUsize,
    before: Option<Before<'a>>,
    progress: Option<Progress<'a>>,
    catch: Option<Catch<'a>>,
    then: Option<Finished<'a>>,
    finally: Option<Finished<'a>>,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<'a> Batch<'a> {
    pub(super) fn new(factory: Factory) -> Self {
        Self {
            pool: Pool::new(factory),
            concurrency: NonZeroUsize::MAX,
            before: None,
            progress: None,
            catch: None,
            then: None,
            finally: None,
        }
    }

    pub fn r#as<F>(
        &mut self,
        key: impl Into<String>,
        build: impl FnOnce(PendingRequest) -> F,
    ) -> &mut Self
    where
        F: Future<Output = Result<Response>> + Send + 'a,
    {
        self.pool.r#as(key, build);
        self
    }

    pub fn newRequest<F>(&mut self, build: impl FnOnce(PendingRequest) -> F) -> &mut Self
    where
        F: Future<Output = Result<Response>> + Send + 'a,
    {
        self.pool.newRequest(build);
        self
    }

    pub fn concurrency(&mut self, limit: NonZeroUsize) -> &mut Self {
        self.concurrency = limit;
        self
    }

    pub fn before(&mut self, callback: impl FnOnce(BatchProgress) + Send + 'a) -> &mut Self {
        self.before = Some(Box::new(callback));
        self
    }

    pub fn progress(
        &mut self,
        callback: impl FnMut(BatchProgress, &str, &Response) + Send + 'a,
    ) -> &mut Self {
        self.progress = Some(Box::new(callback));
        self
    }

    pub fn catch(
        &mut self,
        callback: impl FnMut(BatchProgress, &str, &Result<Response>) + Send + 'a,
    ) -> &mut Self {
        self.catch = Some(Box::new(callback));
        self
    }

    pub fn then(
        &mut self,
        callback: impl FnOnce(BatchProgress, &BatchResults) + Send + 'a,
    ) -> &mut Self {
        self.then = Some(Box::new(callback));
        self
    }

    pub fn finally(
        &mut self,
        callback: impl FnOnce(BatchProgress, &BatchResults) + Send + 'a,
    ) -> &mut Self {
        self.finally = Some(Box::new(callback));
        self
    }

    pub async fn send(self) -> BatchResults {
        let Self {
            pool,
            concurrency,
            before,
            mut progress,
            mut catch,
            then,
            finally,
        } = self;
        let mut counts = BatchProgress {
            total_requests: pool.len(),
            pending_requests: pool.len(),
            failed_requests: 0,
        };
        if let Some(before) = before {
            before(counts);
        }
        let results = pool
            .run_with(concurrency, true, |key, result| {
                counts.pending_requests -= 1;
                match result {
                    Ok(response) if response.successful() => {
                        if let Some(progress) = &mut progress {
                            progress(counts, key, response);
                        }
                    }
                    Ok(response) if !response.failed() => {}
                    _ => {
                        counts.failed_requests += 1;
                        if let Some(catch) = &mut catch {
                            catch(counts, key, result);
                        }
                    }
                }
            })
            .await;
        if !counts.hasFailures()
            && let Some(then) = then
        {
            then(counts, &results);
        }
        if let Some(finally) = finally {
            finally(counts, &results);
        }
        results
    }
}

impl Batch<'static> {
    pub fn defer(self) -> Result<crate::concurrency::DeferredCallback> {
        crate::concurrency::enqueue_deferred(Box::pin(async move {
            self.send().await;
            Ok::<(), Error>(())
        }))
        .map_err(|_| Error::OutsideLifecycle)
    }
}

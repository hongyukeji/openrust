use std::sync::{Arc, Mutex};

use tokio::sync::OnceCell;

use super::{DatabaseResult, connection::Pool};

/// The physical pools for one generation of a logical connection.
pub(super) struct PoolGeneration {
    pub writer: Pool,
    pub reader: Option<Pool>,
    pub established: OnceCell<()>,
}

impl PoolGeneration {
    pub(super) fn retire(&self) {
        self.writer.retire();
        if let Some(reader) = &self.reader {
            reader.retire();
        }
    }
}

/// Shares pool replacement with every handle for the cached connection.
pub(super) struct ConnectionPools {
    current: Mutex<Option<Arc<PoolGeneration>>>,
    resolve: Box<dyn Fn() -> DatabaseResult<PoolGeneration> + Send + Sync>,
}

impl ConnectionPools {
    pub(super) fn new(
        initial: PoolGeneration,
        resolve: impl Fn() -> DatabaseResult<PoolGeneration> + Send + Sync + 'static,
    ) -> Self {
        Self {
            current: Mutex::new(Some(Arc::new(initial))),
            resolve: Box::new(resolve),
        }
    }

    pub(super) fn generation(&self) -> DatabaseResult<Arc<PoolGeneration>> {
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(generation) = current.as_ref() {
            return Ok(generation.clone());
        }
        let generation = Arc::new((self.resolve)()?);
        *current = Some(generation.clone());
        Ok(generation)
    }

    pub(super) fn disconnect(&self) {
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(generation) = current.take() {
            generation.retire();
        }
    }
}

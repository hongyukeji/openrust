use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use tokio::sync::OnceCell;

use super::{DatabaseResult, connection::Pool};

/// The physical pools for one generation of a logical connection.
pub(super) struct PoolGeneration {
    pub writer: Pool,
    pub schema_credentials: Option<super::migrations::SchemaCredentials>,
    pub reader: Option<Pool>,
    pub established: OnceCell<()>,
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
enum MysqlEndpoint {
    Tcp(String, u16),
    Socket(PathBuf),
}

impl PoolGeneration {
    fn mysql_endpoints(&self) -> Option<BTreeSet<MysqlEndpoint>> {
        let Pool::Mysql(writer) = &self.writer else {
            return None;
        };
        let reader = match &self.reader {
            Some(Pool::Mysql(reader)) => reader.as_ref(),
            Some(Pool::Sqlite(_) | Pool::Postgres(_)) => return None,
            None => &[],
        };
        Some(
            writer
                .iter()
                .chain(reader)
                .map(|pool| {
                    let options = pool.connect_options();
                    if let Some(path) = options.get_socket() {
                        MysqlEndpoint::Socket(path.clone())
                    } else {
                        MysqlEndpoint::Tcp(
                            options.get_host().to_ascii_lowercase(),
                            options.get_port(),
                        )
                    }
                })
                .collect(),
        )
    }

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

    pub(super) fn mysql_schemas_for_subquery(
        &self,
        other: &Self,
    ) -> DatabaseResult<Option<(String, String)>> {
        let left = self.generation()?;
        let right = other.generation()?;
        let (Pool::Mysql(left_writer), Pool::Mysql(right_writer)) = (&left.writer, &right.writer)
        else {
            return Ok(None);
        };
        if left
            .mysql_endpoints()
            .is_none_or(|endpoints| Some(endpoints) != right.mysql_endpoints())
        {
            return Ok(None);
        }
        let left = left_writer.first().map(|pool| pool.connect_options());
        let right = right_writer.first().map(|pool| pool.connect_options());
        Ok(left
            .as_ref()
            .and_then(|options| options.get_database())
            .zip(right.as_ref().and_then(|options| options.get_database()))
            .map(|(left, right)| (left.to_owned(), right.to_owned())))
    }

    pub(super) fn has_reader(&self) -> bool {
        self.current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|generation| generation.reader.is_some())
    }

    pub(super) fn retire_if_current(&self, failed: &Arc<PoolGeneration>) {
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if current
            .as_ref()
            .is_some_and(|generation| Arc::ptr_eq(generation, failed))
        {
            failed.retire();
            current.take();
        }
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

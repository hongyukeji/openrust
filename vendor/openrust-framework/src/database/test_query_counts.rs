use std::{io, sync::Mutex};

use super::Connection;

#[derive(Default)]
pub(crate) struct TestQueryCounts(Mutex<Vec<Expectation>>);

struct Expectation {
    expected: u64,
    actual: u64,
    label: String,
    connection: Option<Connection>,
}

impl TestQueryCounts {
    pub(crate) fn expect(&self, expected: u64, label: String, connection: Option<Connection>) {
        self.expectations().push(Expectation {
            expected,
            actual: 0,
            label,
            connection,
        });
    }

    pub(super) fn record(&self, connection: &Connection) {
        for expectation in self.expectations().iter_mut() {
            if expectation
                .connection
                .as_ref()
                .is_none_or(|selected| selected.same_query_source(connection))
            {
                expectation.actual += 1;
            }
        }
    }

    pub(crate) fn finish(&self) -> io::Result<()> {
        let mismatches: Vec<_> = self
            .expectations()
            .drain(..)
            .filter(|expectation| expectation.expected != expectation.actual)
            .map(|expectation| {
                format!(
                    "Expected {} database queries on the [{}] connection. {} occurred.",
                    expectation.expected, expectation.label, expectation.actual
                )
            })
            .collect();
        if mismatches.is_empty() {
            return Ok(());
        }
        Err(io::Error::other(mismatches.join("\n")))
    }

    fn expectations(&self) -> std::sync::MutexGuard<'_, Vec<Expectation>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

use crate::database::{DatabaseResult, query};

#[derive(Clone, Copy, Default)]
pub(super) enum Trashed {
    #[default]
    Exclude,
    Without,
    Include,
    Only,
}

impl Trashed {
    pub(super) fn apply<'connection>(
        &self,
        query: query::Builder<'connection>,
        column: Option<&str>,
    ) -> DatabaseResult<query::Builder<'connection>> {
        match (self, column) {
            (Self::Exclude | Self::Without, Some(column)) => Ok(query.whereNull(column)),
            (Self::Only, Some(column)) => Ok(query.whereNotNull(column)),
            (Self::Only, None) => Err(super::soft_deletes::unsupported()),
            _ => Ok(query),
        }
    }
}

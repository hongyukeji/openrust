//! Schema inspection and data definition operations.

mod blueprint;
mod builder;
mod column_definition;
mod foreign_id_column_definition;
mod foreign_key_definition;
pub mod grammars;
mod index_definition;

pub use blueprint::Blueprint;
pub use builder::Builder;
pub use column_definition::ColumnDefinition;
pub use foreign_id_column_definition::ForeignIdColumnDefinition;
pub use foreign_key_definition::ForeignKeyDefinition;
pub use index_definition::IndexDefinition;

/// A column read from the database catalog.
pub struct Column {
    _private: (),
}

/// An index read from the database catalog.
pub struct Index {
    _private: (),
}

/// A foreign key read from the database catalog.
pub struct ForeignKey {
    _private: (),
}

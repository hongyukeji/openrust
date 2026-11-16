//! Schema inspection and data definition operations.

mod blueprint;
mod builder;
mod column_definition;
mod drop_column_definition;
mod facade;
mod foreign_id_column_definition;
mod foreign_key_definition;
pub mod grammars;
mod index_definition;
mod session;

pub use blueprint::Blueprint;
pub(super) use blueprint::TableOptions;
pub use builder::Builder;
pub use column_definition::{ColumnDefinition, DefaultValue};
pub use drop_column_definition::DropColumnDefinition;
pub use facade::Schema;
pub use foreign_id_column_definition::ForeignIdColumnDefinition;
pub use foreign_key_definition::ForeignKeyDefinition;
pub use index_definition::IndexDefinition;
pub use session::SchemaSession;

/// Default key for polymorphic schema columns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MorphKeyType {
    #[default]
    Int,
    Uuid,
    Ulid,
}

/// A table read from the database catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableInfo {
    pub name: String,
    pub schema: String,
    pub engine: Option<String>,
    pub collation: Option<String>,
    pub comment: Option<String>,
}

/// How the database computes a generated column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenerationType {
    Virtual,
    Stored,
}

/// A generated column's mode and SQL expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnGeneration {
    pub kind: GenerationType,
    pub expression: String,
}

/// A column read from the database catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub type_name: String,
    pub full_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub auto_increment: bool,
    pub primary: bool,
    pub comment: Option<String>,
    pub generation: Option<ColumnGeneration>,
    pub collation: Option<String>,
}

/// An index read from the database catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Index {
    pub name: String,
    pub columns: Vec<Option<String>>,
    pub index_type: Option<String>,
    pub unique: bool,
    pub primary: bool,
}

/// A foreign key read from the database catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignKey {
    pub name: Option<String>,
    pub columns: Vec<String>,
    pub foreign_schema: Option<String>,
    pub foreign_table: String,
    pub foreign_columns: Vec<Option<String>>,
    pub on_delete: String,
    pub on_update: String,
}

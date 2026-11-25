//! Schema inspection and data definition operations.

// 在调用入口捕获位置，异步工作仍在轮询时执行。
macro_rules! schema_queries {
    ($( $(#[$attribute:meta])* pub async fn $name:ident $(<$generic:tt $(: $bound:path)?>)?
        ($($argument:tt)*) -> $result:ty $body:block )+) => {
        $(
            $(#[$attribute])*
            #[track_caller]
            pub fn $name $(<$generic $(: $bound)?>)? ($($argument)*)
                -> impl std::future::Future<Output = $result>
            {
                crate::database::events::with_query_caller(std::panic::Location::caller(), async move $body)
            }
        )+
    };
}

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
pub use index_definition::{ColumnIndex, IndexDefinition};
pub use session::SchemaSession;

#[derive(Clone, Copy, Default)]
pub(super) struct SqliteJsonTypes {
    pub(super) json: bool,
    pub(super) jsonb: bool,
}

/// Default key for polymorphic schema columns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MorphKeyType {
    #[default]
    Int,
    Uuid,
    Ulid,
}

#[derive(Clone, Copy)]
pub(super) struct SchemaDefaults {
    pub(super) morph_key_type: MorphKeyType,
    pub(super) string_length: u32,
    pub(super) time_precision: u8,
}

impl Default for SchemaDefaults {
    fn default() -> Self {
        Self {
            morph_key_type: MorphKeyType::default(),
            string_length: 255,
            time_precision: 0,
        }
    }
}

/// A schema visible to the database connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaInfo {
    pub name: String,
    pub path: Option<String>,
    pub default: bool,
}

/// A view read from the database catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewInfo {
    pub name: String,
    pub schema: String,
    pub schema_qualified_name: String,
    pub definition: Option<String>,
}

/// A PostgreSQL type read from the database catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeInfo {
    pub name: String,
    pub schema: String,
    pub schema_qualified_name: String,
    pub implicit: bool,
    pub r#type: Option<String>,
    pub category: Option<String>,
}

/// A table read from the database catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableInfo {
    pub name: String,
    pub schema: String,
    pub schema_qualified_name: String,
    pub size: Option<u64>,
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

/// Identifies an index or foreign key by name or by its ordered columns.
#[derive(Clone, Copy, Debug)]
pub enum KeySelector<'a> {
    Name(&'a str),
    Columns(&'a [&'a str]),
}

/// Supplies a schema when an object reference contains only a table name.
#[derive(Clone, Copy, Debug)]
pub enum SchemaFallback<'a> {
    None,
    Current,
    Named(&'a str),
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

/// The result of configuring a schema blueprint. Fallible callbacks can use `?`.
/// Existing callbacks returning `()` continue to work unchanged.
pub trait BlueprintCallbackResult {
    fn into_result(self) -> crate::database::DatabaseResult<()>;
}

impl BlueprintCallbackResult for () {
    fn into_result(self) -> crate::database::DatabaseResult<()> {
        Ok(())
    }
}

impl BlueprintCallbackResult for crate::database::DatabaseResult<()> {
    fn into_result(self) -> crate::database::DatabaseResult<()> {
        self
    }
}

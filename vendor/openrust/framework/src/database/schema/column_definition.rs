use crate::database::{Value, query::Expression};

#[derive(Clone)]
pub(super) enum ColumnType {
    TinyInteger,
    SmallInteger,
    MediumInteger,
    Integer,
    BigInteger,
    String(u32),
    Char(u32),
    TinyText,
    Text,
    MediumText,
    LongText,
    Boolean,
    Decimal(u8, u8),
    Float(u8),
    Double,
    Binary,
    SizedBinary(u32, bool),
    Enum(Vec<String>),
    Set(Vec<String>),
    Json,
    Jsonb,
    IpAddress,
    MacAddress,
    Year,
    Spatial(&'static str, u32),
    Date,
    DateTime(u8),
    Timestamp(u8),
    Time(u8),
    Raw(String),
}

/// A literal, trusted SQL expression, or current-time default for a schema column.
#[derive(Clone)]
pub enum DefaultValue {
    Value(Value),
    Expression(Expression),
    CurrentTimestamp,
}

impl<T: Into<Value>> From<T> for DefaultValue {
    fn from(value: T) -> Self {
        Self::Value(value.into())
    }
}

impl From<Expression> for DefaultValue {
    fn from(expression: Expression) -> Self {
        Self::Expression(expression)
    }
}

#[derive(Clone)]
pub(super) enum Position {
    First,
    After(String),
}

#[derive(Clone)]
pub(super) struct GeneratedColumn {
    pub(super) expression: String,
    pub(super) stored: bool,
    pub(super) json: bool,
}

/// A column's type and modifiers.
#[derive(Clone)]
pub struct ColumnDefinition {
    pub(super) name: String,
    pub(super) kind: ColumnType,
    pub(super) nullable: bool,
    pub(super) unsigned: bool,
    pub(super) increment: bool,
    pub(super) primary: bool,
    pub(super) unique: bool,
    pub(super) indexed: bool,
    pub(super) spatial_indexed: bool,
    pub(super) default: Option<DefaultValue>,
    pub(super) generated: Option<GeneratedColumn>,
    pub(super) change: bool,
    pub(super) rename_to: Option<String>,
    pub(super) comment: Option<String>,
    pub(super) charset: Option<String>,
    pub(super) collation: Option<String>,
    pub(super) current_on_update: bool,
    pub(super) on_update: Option<Box<Expression>>,
    pub(super) invisible: bool,
    pub(super) position: Option<Position>,
    pub(super) instant: bool,
    pub(super) lock: Option<String>,
    pub(super) start: Option<u64>,
}

#[allow(non_snake_case, reason = "Matches Laravel's column modifiers")]
impl ColumnDefinition {
    pub(super) fn new(name: &str, kind: ColumnType) -> Self {
        Self {
            name: name.to_owned(),
            kind,
            nullable: false,
            unsigned: false,
            increment: false,
            primary: false,
            unique: false,
            indexed: false,
            spatial_indexed: false,
            default: None,
            generated: None,
            change: false,
            rename_to: None,
            comment: None,
            charset: None,
            collation: None,
            current_on_update: false,
            on_update: None,
            invisible: false,
            position: None,
            instant: false,
            lock: None,
            start: None,
        }
    }

    /// Returns the column name in the blueprint, before any pending rename.
    pub fn getName(&self) -> &str {
        &self.name
    }

    pub fn nullable(&mut self) -> &mut Self {
        self.nullable = true;
        self
    }

    pub fn unsigned(&mut self) -> &mut Self {
        self.unsigned = true;
        self
    }

    /// Enables auto-increment without changing the column's signedness.
    pub fn autoIncrement(&mut self) -> &mut Self {
        self.increment = true;
        self
    }

    /// Sets the starting value of an auto-incrementing column where supported.
    pub fn from(&mut self, value: u64) -> &mut Self {
        self.startingValue(value)
    }

    /// Sets the starting value of an auto-incrementing column where supported.
    pub fn startingValue(&mut self, value: u64) -> &mut Self {
        self.start = Some(value);
        self
    }

    pub fn primary(&mut self) -> &mut Self {
        self.primary = true;
        self
    }

    pub fn unique(&mut self) -> &mut Self {
        self.unique = true;
        self
    }

    pub fn index(&mut self) -> &mut Self {
        self.indexed = true;
        self
    }

    pub fn spatialIndex(&mut self) -> &mut Self {
        self.spatial_indexed = true;
        self
    }

    pub fn default(&mut self, value: impl Into<DefaultValue>) -> &mut Self {
        self.default = Some(value.into());
        self
    }

    /// Computes the column from a trusted SQL expression when read.
    pub fn virtualAs(&mut self, expression: impl Into<String>) -> &mut Self {
        self.generated(expression, false, false)
    }

    /// Computes and stores the column using a trusted SQL expression.
    pub fn storedAs(&mut self, expression: impl Into<String>) -> &mut Self {
        self.generated(expression, true, false)
    }

    /// Computes the column from a JSON selector when read.
    pub fn virtualAsJson(&mut self, selector: impl Into<String>) -> &mut Self {
        self.generated(selector, false, true)
    }

    /// Computes and stores the column selected from JSON.
    pub fn storedAsJson(&mut self, selector: impl Into<String>) -> &mut Self {
        self.generated(selector, true, true)
    }

    fn generated(&mut self, expression: impl Into<String>, stored: bool, json: bool) -> &mut Self {
        self.generated = Some(GeneratedColumn {
            expression: expression.into(),
            stored,
            json,
        });
        self
    }

    pub fn useCurrent(&mut self) -> &mut Self {
        self.default = Some(DefaultValue::CurrentTimestamp);
        self
    }

    /// Updates a MySQL timestamp or datetime when another value in the row changes.
    pub fn useCurrentOnUpdate(&mut self) -> &mut Self {
        self.current_on_update = true;
        self
    }

    /// Sets a trusted MySQL ON UPDATE expression for a temporal column.
    ///
    /// `useCurrentOnUpdate` takes precedence when both modifiers are present.
    pub fn onUpdate(&mut self, expression: Expression) -> &mut Self {
        self.on_update = Some(Box::new(expression));
        self
    }

    /// Excludes a MySQL column from wildcard selects.
    pub fn invisible(&mut self) -> &mut Self {
        self.invisible = true;
        self
    }

    /// Places the column first when altering a MySQL table.
    pub fn first(&mut self) -> &mut Self {
        self.position = Some(Position::First);
        self
    }

    /// Places the column after an existing column when altering a MySQL table.
    pub fn after(&mut self, column: &str) -> &mut Self {
        self.position = Some(Position::After(column.to_owned()));
        self
    }

    /// Requests MySQL's instant algorithm when adding or changing this column.
    pub fn instant(&mut self) -> &mut Self {
        self.instant = true;
        self
    }

    /// Requests a MySQL DDL lock mode when adding or changing this column.
    pub fn lock(&mut self, mode: &str) -> &mut Self {
        self.lock = Some(mode.to_owned());
        self
    }

    pub fn charset(&mut self, charset: &str) -> &mut Self {
        self.charset = Some(charset.to_owned());
        self
    }

    pub fn collation(&mut self, collation: &str) -> &mut Self {
        self.collation = Some(collation.to_owned());
        self
    }

    pub fn comment(&mut self, comment: &str) -> &mut Self {
        self.comment = Some(comment.to_owned());
        self
    }

    /// Renames the column as part of a MySQL change operation.
    pub fn renameTo(&mut self, name: &str) -> &mut Self {
        self.rename_to = Some(name.to_owned());
        self
    }

    pub fn change(&mut self) -> &mut Self {
        self.change = true;
        self
    }
}

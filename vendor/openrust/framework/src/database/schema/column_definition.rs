use crate::database::Value;

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
    Float,
    Double,
    Binary,
    SizedBinary(u32, bool),
    Enum(Vec<String>),
    Set(Vec<String>),
    Json,
    IpAddress,
    MacAddress,
    Year,
    Date,
    DateTime(u8),
    Timestamp(u8),
    Time(u8),
}

#[derive(Clone)]
pub(super) enum DefaultValue {
    Value(Value),
    CurrentTimestamp,
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
    pub(super) default: Option<DefaultValue>,
    pub(super) generated: Option<GeneratedColumn>,
    pub(super) change: bool,
    pub(super) comment: Option<String>,
    pub(super) current_on_update: bool,
    pub(super) invisible: bool,
    pub(super) position: Option<Position>,
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
            default: None,
            generated: None,
            change: false,
            comment: None,
            current_on_update: false,
            invisible: false,
            position: None,
        }
    }

    pub fn nullable(&mut self) -> &mut Self {
        self.nullable = true;
        self
    }

    pub fn unsigned(&mut self) -> &mut Self {
        self.unsigned = true;
        self
    }

    pub fn autoIncrement(&mut self) -> &mut Self {
        self.increment = true;
        self.unsigned = true;
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

    pub fn default(&mut self, value: impl Into<Value>) -> &mut Self {
        self.default = Some(DefaultValue::Value(value.into()));
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

    pub fn comment(&mut self, comment: &str) -> &mut Self {
        self.comment = Some(comment.to_owned());
        self
    }

    pub fn change(&mut self) -> &mut Self {
        self.change = true;
        self
    }
}

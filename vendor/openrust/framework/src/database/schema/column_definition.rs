use crate::database::Value;

#[derive(Clone, Copy)]
pub(super) enum ColumnType {
    TinyInteger,
    SmallInteger,
    MediumInteger,
    Integer,
    BigInteger,
    String(u32),
    Char(u32),
    Text,
    MediumText,
    LongText,
    Boolean,
    Decimal(u8, u8),
    Float,
    Double,
    Binary,
    Json,
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
    pub(super) change: bool,
    pub(super) comment: Option<String>,
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
            change: false,
            comment: None,
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

    pub fn useCurrent(&mut self) -> &mut Self {
        self.default = Some(DefaultValue::CurrentTimestamp);
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

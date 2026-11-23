/// The kind of index a blueprint creates or drops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IndexType {
    Primary,
    Unique,
    Index,
    FullText,
    Spatial,
    Vector,
}

impl IndexType {
    pub(super) fn suffix(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Unique => "unique",
            Self::Index => "index",
            Self::FullText => "fulltext",
            Self::Spatial => "spatialindex",
            Self::Vector => "vectorindex",
        }
    }
}

/// A fluent column index declaration.
#[derive(Clone)]
pub enum ColumnIndex {
    /// Adds the conventionally named index.
    Add,
    /// Adds an explicitly named index.
    Named(String),
    /// Drops the conventionally named index when changing the column.
    Drop,
}

impl From<bool> for ColumnIndex {
    fn from(enabled: bool) -> Self {
        if enabled { Self::Add } else { Self::Drop }
    }
}

impl From<&str> for ColumnIndex {
    fn from(name: &str) -> Self {
        Self::Named(name.to_owned())
    }
}

impl From<String> for ColumnIndex {
    fn from(name: String) -> Self {
        Self::Named(name)
    }
}

/// An index with optional database-specific algorithm and lock modifiers.
pub struct IndexDefinition {
    pub(super) kind: IndexType,
    pub(super) columns: Vec<String>,
    pub(super) raw_expression: Option<String>,
    pub(super) name: Option<String>,
    pub(super) algorithm: Option<String>,
    pub(super) language: Option<String>,
    pub(super) operator_class: Option<String>,
    pub(super) inplace: bool,
    pub(super) online: bool,
    pub(super) nulls_not_distinct: Option<bool>,
    pub(super) deferrable: Option<bool>,
    pub(super) initially_immediate: Option<bool>,
    pub(super) lock: Option<String>,
}

impl IndexDefinition {
    pub(super) fn new(kind: IndexType, columns: &[&str], name: Option<&str>) -> Self {
        Self {
            kind,
            columns: columns.iter().map(|column| (*column).to_owned()).collect(),
            raw_expression: None,
            name: name.map(str::to_owned),
            algorithm: None,
            language: None,
            operator_class: None,
            inplace: false,
            online: false,
            nulls_not_distinct: None,
            deferrable: None,
            initially_immediate: None,
            lock: None,
        }
    }

    /// Specifies the index method independently of the DDL algorithm.
    pub fn algorithm(&mut self, method: &str) -> &mut Self {
        self.algorithm = Some(method.to_owned());
        self
    }

    /// Specifies the PostgreSQL text search configuration for a full-text index.
    pub fn language(&mut self, language: &str) -> &mut Self {
        self.language = Some(language.to_owned());
        self
    }

    pub fn inplace(&mut self) -> &mut Self {
        self.inplace = true;
        self
    }

    /// Requests a concurrent PostgreSQL index build.
    pub fn online(&mut self) -> &mut Self {
        self.online = true;
        self
    }

    pub fn lock(&mut self, mode: &str) -> &mut Self {
        self.lock = Some(mode.to_owned());
        self
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl IndexDefinition {
    /// Controls whether a PostgreSQL unique constraint treats NULL values as equal.
    pub fn nullsNotDistinct(&mut self, enabled: bool) -> &mut Self {
        self.nulls_not_distinct = Some(enabled);
        self
    }

    /// Controls whether PostgreSQL can defer this unique constraint until commit.
    pub fn deferrable(&mut self, enabled: bool) -> &mut Self {
        self.deferrable = Some(enabled);
        self
    }

    /// Chooses the initial check timing for a deferrable PostgreSQL unique constraint.
    pub fn initiallyImmediate(&mut self, enabled: bool) -> &mut Self {
        self.initially_immediate = Some(enabled);
        self
    }

    pub(super) fn has_unique_modifiers(&self) -> bool {
        self.nulls_not_distinct.is_some()
            || self.deferrable.is_some()
            || self.initially_immediate.is_some()
    }

    /// Chooses a PostgreSQL operator class or MariaDB options for a vector index.
    pub fn operatorClass(&mut self, operator_class: &str) -> &mut Self {
        self.operator_class = Some(operator_class.to_owned());
        self
    }
}

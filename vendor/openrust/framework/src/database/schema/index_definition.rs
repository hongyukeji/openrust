/// The kind of index a blueprint creates or drops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IndexType {
    Primary,
    Unique,
    Index,
    FullText,
    Spatial,
}

impl IndexType {
    pub(super) fn suffix(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Unique => "unique",
            Self::Index => "index",
            Self::FullText => "fulltext",
            Self::Spatial => "spatialindex",
        }
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
    pub(super) inplace: bool,
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
            inplace: false,
            lock: None,
        }
    }

    /// Specifies the MySQL index method, independently of the DDL algorithm.
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

    pub fn lock(&mut self, mode: &str) -> &mut Self {
        self.lock = Some(mode.to_owned());
        self
    }
}

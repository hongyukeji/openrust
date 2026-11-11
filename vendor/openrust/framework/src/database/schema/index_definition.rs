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
    pub(super) name: Option<String>,
    pub(super) inplace: bool,
    pub(super) lock: Option<String>,
}

impl IndexDefinition {
    pub(super) fn new(kind: IndexType, columns: &[&str], name: Option<&str>) -> Self {
        Self {
            kind,
            columns: columns.iter().map(|column| (*column).to_owned()).collect(),
            name: name.map(str::to_owned),
            inplace: false,
            lock: None,
        }
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

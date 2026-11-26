/// A drop-column operation and its optional MySQL DDL modifiers.
pub struct DropColumnDefinition {
    pub(super) columns: Vec<String>,
    pub(super) instant: bool,
    pub(super) lock: Option<String>,
}

impl DropColumnDefinition {
    pub(super) fn new(columns: &[&str]) -> Self {
        Self {
            columns: columns.iter().map(|column| (*column).to_owned()).collect(),
            instant: false,
            lock: None,
        }
    }

    /// Requests MySQL's instant algorithm for this drop.
    pub fn instant(&mut self) -> &mut Self {
        self.instant = true;
        self
    }

    /// Requests a MySQL DDL lock mode for this drop.
    pub fn lock(&mut self, mode: &str) -> &mut Self {
        self.lock = Some(mode.to_owned());
        self
    }
}

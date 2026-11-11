/// A foreign key and its referential actions.
pub struct ForeignKeyDefinition {
    pub(super) columns: Vec<String>,
    pub(super) name: Option<String>,
    pub(super) foreign_columns: Vec<String>,
    pub(super) foreign_table: Option<String>,
    pub(super) on_update: Option<String>,
    pub(super) on_delete: Option<String>,
    pub(super) inplace: bool,
    pub(super) lock: Option<String>,
}

#[allow(non_snake_case, reason = "Matches Laravel's ForeignKeyDefinition API")]
impl ForeignKeyDefinition {
    pub(super) fn new(columns: &[&str], name: Option<&str>) -> Self {
        Self {
            columns: columns.iter().map(|column| (*column).to_owned()).collect(),
            name: name.map(str::to_owned),
            foreign_columns: Vec::new(),
            foreign_table: None,
            on_update: None,
            on_delete: None,
            inplace: false,
            lock: None,
        }
    }

    pub fn references(&mut self, column: &str) -> &mut Self {
        self.referencesColumns(&[column])
    }

    pub fn referencesColumns(&mut self, columns: &[&str]) -> &mut Self {
        self.foreign_columns = columns.iter().map(|column| (*column).to_owned()).collect();
        self
    }

    pub fn on(&mut self, table: &str) -> &mut Self {
        self.foreign_table = Some(table.to_owned());
        self
    }

    pub fn onDelete(&mut self, action: &str) -> &mut Self {
        self.on_delete = Some(action.to_owned());
        self
    }

    pub fn onUpdate(&mut self, action: &str) -> &mut Self {
        self.on_update = Some(action.to_owned());
        self
    }

    pub fn cascadeOnDelete(&mut self) -> &mut Self {
        self.onDelete("cascade")
    }
    pub fn restrictOnDelete(&mut self) -> &mut Self {
        self.onDelete("restrict")
    }
    pub fn nullOnDelete(&mut self) -> &mut Self {
        self.onDelete("set null")
    }
    pub fn noActionOnDelete(&mut self) -> &mut Self {
        self.onDelete("no action")
    }
    pub fn cascadeOnUpdate(&mut self) -> &mut Self {
        self.onUpdate("cascade")
    }
    pub fn restrictOnUpdate(&mut self) -> &mut Self {
        self.onUpdate("restrict")
    }
    pub fn nullOnUpdate(&mut self) -> &mut Self {
        self.onUpdate("set null")
    }
    pub fn noActionOnUpdate(&mut self) -> &mut Self {
        self.onUpdate("no action")
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

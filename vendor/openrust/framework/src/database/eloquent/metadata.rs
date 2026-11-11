/// Table, connection, keys, and persistence options declared by a model.
#[derive(Clone, Copy)]
pub struct ModelMetadata {
    pub(super) table: &'static str,
    pub(super) connection: Option<&'static str>,
    pub(super) primary_key: &'static str,
    pub(super) incrementing: bool,
    pub(super) fillable: &'static [&'static str],
    pub(super) guarded: &'static [&'static str],
    pub(super) created_at: Option<&'static str>,
    pub(super) updated_at: Option<&'static str>,
    pub(super) deleted_at: Option<&'static str>,
    pub(super) date_format: &'static str,
    pub(super) hidden: &'static [&'static str],
    pub(super) visible: &'static [&'static str],
    pub(super) appends: &'static [&'static str],
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent model options")]
impl ModelMetadata {
    pub const fn new(table: &'static str) -> Self {
        Self {
            table,
            connection: None,
            primary_key: "id",
            incrementing: true,
            fillable: &[],
            guarded: &["*"],
            created_at: Some("created_at"),
            updated_at: Some("updated_at"),
            deleted_at: None,
            date_format: "%Y-%m-%d %H:%M:%S",
            hidden: &[],
            visible: &[],
            appends: &[],
        }
    }

    pub const fn connection(mut self, name: &'static str) -> Self {
        self.connection = Some(name);
        self
    }

    pub const fn primaryKey(mut self, name: &'static str) -> Self {
        self.primary_key = name;
        self
    }

    pub const fn incrementing(mut self, enabled: bool) -> Self {
        self.incrementing = enabled;
        self
    }

    pub const fn fillable(mut self, columns: &'static [&'static str]) -> Self {
        self.fillable = columns;
        self
    }

    pub const fn guarded(mut self, columns: &'static [&'static str]) -> Self {
        self.guarded = columns;
        self
    }

    pub const fn timestamps(
        mut self,
        created_at: Option<&'static str>,
        updated_at: Option<&'static str>,
    ) -> Self {
        self.created_at = created_at;
        self.updated_at = updated_at;
        self
    }

    pub const fn softDeletes(mut self, deleted_at: &'static str) -> Self {
        self.deleted_at = Some(deleted_at);
        self
    }

    pub const fn dateFormat(mut self, format: &'static str) -> Self {
        self.date_format = format;
        self
    }

    pub const fn hidden(mut self, attributes: &'static [&'static str]) -> Self {
        self.hidden = attributes;
        self
    }

    pub const fn visible(mut self, attributes: &'static [&'static str]) -> Self {
        self.visible = attributes;
        self
    }

    pub const fn appends(mut self, attributes: &'static [&'static str]) -> Self {
        self.appends = attributes;
        self
    }
}

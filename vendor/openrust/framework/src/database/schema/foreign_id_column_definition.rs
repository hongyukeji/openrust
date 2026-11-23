use super::{Blueprint, ColumnDefinition, DefaultValue, ForeignKeyDefinition, blueprint::Command};

/// A column that can define its foreign key.
pub struct ForeignIdColumnDefinition<'a> {
    blueprint: &'a mut Blueprint,
    name: String,
    model_target: Option<(&'static str, &'static str)>,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<'a> ForeignIdColumnDefinition<'a> {
    pub(super) fn new(blueprint: &'a mut Blueprint, name: &str) -> Self {
        Self {
            blueprint,
            name: name.to_owned(),
            model_target: None,
        }
    }

    pub(super) fn for_model(mut self, table: &'static str, column: &'static str) -> Self {
        self.model_target = Some((table, column));
        self
    }

    fn column(&mut self) -> &mut ColumnDefinition {
        let Some(Command::Column(column)) = self.blueprint.commands.last_mut() else {
            unreachable!("foreign ID owns its just-appended column")
        };
        column
    }

    pub fn comment(mut self, comment: impl Into<String>) -> Self {
        self.column().comment(comment);
        self
    }

    pub fn nullable(mut self) -> Self {
        self.column().nullable();
        self
    }

    /// Adds the ordinary column index without creating a foreign-key constraint.
    pub fn index(mut self) -> Self {
        self.column().index();
        self
    }

    pub fn default(mut self, value: impl Into<DefaultValue>) -> Self {
        self.column().default(value);
        self
    }

    pub fn constrained(self) -> &'a mut ForeignKeyDefinition {
        self.constrainedWith(None, None, None)
    }

    /// Overrides individual parts of the inferred foreign key reference.
    pub fn constrainedWith(
        self,
        table: Option<&str>,
        column: Option<&str>,
        index_name: Option<&str>,
    ) -> &'a mut ForeignKeyDefinition {
        let column = column
            .or(self.model_target.map(|(_, key)| key))
            .unwrap_or("id");
        if let Some(table) = table.or(self.model_target.map(|(table, _)| table)) {
            return self.references(column, index_name).on(table);
        }

        let suffix = format!("_{column}");
        let name = self
            .name
            .rsplit_once(&suffix)
            .map_or(self.name.as_str(), |(name, _)| name);
        let table = pluralizer::pluralize(name, 2, false);
        self.references(column, index_name).on(&table)
    }

    pub fn references(self, column: &str, name: Option<&str>) -> &'a mut ForeignKeyDefinition {
        self.blueprint
            .foreign(&[&self.name], name)
            .references(column)
    }

    pub fn constrainedOn(
        self,
        table: &str,
        column: &str,
        name: Option<&str>,
    ) -> &'a mut ForeignKeyDefinition {
        self.constrainedWith(Some(table), Some(column), name)
    }
}

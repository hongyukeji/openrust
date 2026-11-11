use crate::database::Value;

use super::{Blueprint, ColumnDefinition, ForeignKeyDefinition, blueprint::Command};

/// An unsigned big integer column that can define its foreign key.
pub struct ForeignIdColumnDefinition<'a> {
    blueprint: &'a mut Blueprint,
    name: String,
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel's ForeignIdColumnDefinition API"
)]
impl<'a> ForeignIdColumnDefinition<'a> {
    pub(super) fn new(blueprint: &'a mut Blueprint, name: &str) -> Self {
        Self {
            blueprint,
            name: name.to_owned(),
        }
    }

    fn column(&mut self) -> &mut ColumnDefinition {
        let Some(Command::Column(column)) = self.blueprint.commands.last_mut() else {
            unreachable!("foreign ID owns its just-appended column")
        };
        column
    }

    pub fn nullable(mut self) -> Self {
        self.column().nullable();
        self
    }

    pub fn default(mut self, value: impl Into<Value>) -> Self {
        self.column().default(value);
        self
    }

    pub fn constrained(self) -> &'a mut ForeignKeyDefinition {
        let name = self
            .name
            .rsplit_once("_id")
            .map_or(self.name.as_str(), |(name, _)| name);
        let table = pluralizer::pluralize(name, 2, false);
        self.constrainedOn(&table, "id", None)
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
        self.blueprint
            .foreign(&[&self.name], name)
            .references(column)
            .on(table)
    }
}

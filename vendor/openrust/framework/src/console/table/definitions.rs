use super::{ConsoleError, ConsoleTableError, Table, TableStyle};
use crate::contracts::container::Container as _;
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};
pub(crate) struct Definitions(RwLock<BTreeMap<String, TableStyle>>);
impl Default for Definitions {
    fn default() -> Self {
        Self(RwLock::new(
            [
                "default",
                "markdown",
                "borderless",
                "compact",
                "symfony-style-guide",
                "box",
                "box-double",
            ]
            .into_iter()
            .map(|name| {
                (
                    name.to_owned(),
                    TableStyle::from_name(name).expect("内置样式定义有效"),
                )
            })
            .collect(),
        ))
    }
}
impl Definitions {
    pub(super) fn get(&self, name: &str) -> Result<TableStyle, ConsoleError> {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .map(TableStyle::shared_reference)
            .ok_or_else(|| {
                ConsoleError::from_source(ConsoleTableError::UndefinedStyle(name.to_owned()))
            })
    }
    fn set(&self, name: &str, style: TableStyle) {
        let retired = self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(name.to_owned(), style);
        drop(retired);
    }
}
pub(super) fn current(required: bool) -> Result<Arc<Definitions>, ConsoleError> {
    if let Some(container) = crate::support::facades::current_container() {
        return container
            .make::<Definitions>()
            .map_err(ConsoleError::from_source);
    }
    if required {
        return Err(ConsoleError::from_source(
            ConsoleTableError::MissingDefinitions,
        ));
    }
    Ok(Arc::new(Definitions::default()))
}
impl Table<'_, '_> {
    pub fn set_style_definition(
        name: &str,
        style: impl Into<TableStyle>,
    ) -> Result<(), ConsoleError> {
        current(true)?.set(name, style.into());
        Ok(())
    }
    pub fn get_style_definition(name: &str) -> Result<TableStyle, ConsoleError> {
        current(true)?.get(name)
    }
    pub fn set_style_named(&mut self, name: &str) -> Result<&mut Self, ConsoleError> {
        self.style = self.definitions.get(name)?;
        Ok(self)
    }
    pub fn set_column_style_named(
        &mut self,
        column: usize,
        name: &str,
    ) -> Result<&mut Self, ConsoleError> {
        self.column_styles
            .insert(column, self.definitions.get(name)?);
        Ok(self)
    }
}

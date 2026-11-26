use crate::database::{eloquent::Attributes, query};

#[derive(Clone)]
pub(super) struct PivotConstraints {
    conditions: Option<query::Conditions>,
    pub(super) defaults: Attributes,
}

impl PivotConstraints {
    pub(super) const fn new() -> Self {
        Self {
            conditions: None,
            defaults: Attributes::new(),
        }
    }

    pub(super) fn add(
        &mut self,
        table: &str,
        column: &str,
        predicate: impl FnOnce(query::Conditions, &str) -> query::Conditions,
    ) {
        let qualified;
        let column = if column.contains('.') {
            column
        } else {
            qualified = format!("{table}.{column}");
            &qualified
        };
        self.conditions = Some(predicate(
            self.conditions.take().unwrap_or_default(),
            column,
        ));
    }

    pub(super) fn add_group(
        &mut self,
        table: &str,
        or: bool,
        configure: impl FnOnce(query::Conditions) -> query::Conditions,
    ) {
        let mut group = configure(query::Conditions::default());
        group.qualify_columns(table);
        let conditions = self.conditions.take().unwrap_or_default();
        self.conditions = Some(if or {
            conditions.or_where_nested(|_| group)
        } else {
            conditions.where_nested(|_| group)
        });
    }

    pub(super) fn apply<'a>(&self, query: query::Builder<'a>) -> query::Builder<'a> {
        match &self.conditions {
            Some(conditions) => query.where_nested(|_| conditions.clone()),
            None => query,
        }
    }
}

macro_rules! pivot_predicates {
    ($apply:ident, $group:ident) => {
        /// Filters Pivot rows with a grouped predicate.
        #[must_use]
        pub fn where_pivot_with(
            self,
            configure: impl FnOnce(query::Conditions) -> query::Conditions,
        ) -> Self {
            self.$group(false, configure)
        }

        /// Adds a grouped alternative within the current Pivot scope.
        #[must_use]
        pub fn or_where_pivot_with(
            self,
            configure: impl FnOnce(query::Conditions) -> query::Conditions,
        ) -> Self {
            self.$group(true, configure)
        }


        pivot_predicates!(@methods $apply;
            where_pivot => r#where(operator: &str, value: impl Into<Value>),
            or_where_pivot => or_where(operator: &str, value: impl Into<Value>),
            where_pivot_in => where_in(values: Vec<Value>),
            or_where_pivot_in => or_where_in(values: Vec<Value>),
            where_pivot_not_in => where_not_in(values: Vec<Value>),
            or_where_pivot_not_in => or_where_not_in(values: Vec<Value>),
            where_pivot_between => where_between(bounds: [Value; 2]),
            or_where_pivot_between => or_where_between(bounds: [Value; 2]),
            where_pivot_not_between => where_not_between(bounds: [Value; 2]),
            or_where_pivot_not_between => or_where_not_between(bounds: [Value; 2]),
            where_pivot_null => where_null(),
            or_where_pivot_null => or_where_null(),
            where_pivot_not_null => where_not_null(),
            or_where_pivot_not_null => or_where_not_null()
        );
    };
    (@methods $apply:ident; $($name:ident => $method:ident($($arg:ident: $ty:ty),*)),+ $(,)?) => {
        $(
            #[must_use]
            pub fn $name(self, column: &str $(, $arg: $ty)*) -> Self {
                self.$apply(column, |conditions, column| conditions.$method(column $(, $arg)*))
            }
        )+
    };
}

pub(super) use pivot_predicates;

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
            conditions.orWhereNested(|_| group)
        } else {
            conditions.whereNested(|_| group)
        });
    }

    pub(super) fn apply<'a>(&self, query: query::Builder<'a>) -> query::Builder<'a> {
        match &self.conditions {
            Some(conditions) => query.whereNested(|_| conditions.clone()),
            None => query,
        }
    }
}

macro_rules! pivot_predicates {
    ($apply:ident, $group:ident) => {
        /// Filters Pivot rows with a grouped predicate.
        #[must_use]
        pub fn wherePivotWith(
            self,
            configure: impl FnOnce(query::Conditions) -> query::Conditions,
        ) -> Self {
            self.$group(false, configure)
        }

        /// Adds a grouped alternative within the current Pivot scope.
        #[must_use]
        pub fn orWherePivotWith(
            self,
            configure: impl FnOnce(query::Conditions) -> query::Conditions,
        ) -> Self {
            self.$group(true, configure)
        }


        pivot_predicates!(@methods $apply;
            wherePivot => r#where(operator: &str, value: impl Into<Value>),
            orWherePivot => orWhere(operator: &str, value: impl Into<Value>),
            wherePivotIn => whereIn(values: Vec<Value>),
            orWherePivotIn => orWhereIn(values: Vec<Value>),
            wherePivotNotIn => whereNotIn(values: Vec<Value>),
            orWherePivotNotIn => orWhereNotIn(values: Vec<Value>),
            wherePivotBetween => whereBetween(bounds: [Value; 2]),
            orWherePivotBetween => orWhereBetween(bounds: [Value; 2]),
            wherePivotNotBetween => whereNotBetween(bounds: [Value; 2]),
            orWherePivotNotBetween => orWhereNotBetween(bounds: [Value; 2]),
            wherePivotNull => whereNull(),
            orWherePivotNull => orWhereNull(),
            wherePivotNotNull => whereNotNull(),
            orWherePivotNotNull => orWhereNotNull()
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

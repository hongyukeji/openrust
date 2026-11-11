use std::sync::Arc;

use crate::database::{
    DatabaseResult,
    eloquent::{Attributes, Model},
};

use super::relation::RelationKeys;

pub(super) type ConfigureDefault<P, R> = dyn Fn(&mut R, &P) + Send + Sync;

pub(super) fn match_defaults<P: Model, R: Model>(
    parents: &[&P],
    matches: impl Iterator<Item = Option<Arc<R>>>,
    configure: Option<&ConfigureDefault<P, R>>,
    keys: &RelationKeys<P, R>,
    attributes: impl Fn(&P) -> Attributes,
) -> DatabaseResult<Vec<Option<Arc<R>>>> {
    parents
        .iter()
        .zip(matches)
        .map(|(&parent, model)| {
            let Some(configure) = configure.filter(|_| model.is_none()) else {
                return Ok(model);
            };
            let mut model = R::newInstance(Attributes::new())?;
            if parent.state().connection.is_some()
                || (R::metadata().connection.is_none() && P::metadata().connection.is_some())
            {
                model.state_mut().connection = Some(keys.connection(parent)?);
            }
            model.forceFill(attributes(parent));
            configure(&mut model, parent);
            Ok(Some(Arc::new(model)))
        })
        .collect()
}

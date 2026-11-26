use super::{HasMany, HasManyThrough, HasOne, HasOneThrough, MorphMany, MorphOne};
use crate::database::eloquent::Model;

/// The keys and cardinality of an existing direct or polymorphic relationship.
pub trait ThroughRelation: Sized {
    type Parent: Model;
    type Related: Model;
    type Pending;
    type Through<P: Model>;

    #[doc(hidden)]
    fn keys(
        &self,
    ) -> (
        &'static str,
        &'static str,
        Option<(&'static str, &'static str)>,
    );

    #[doc(hidden)]
    fn pending(self) -> Self::Pending;

    #[doc(hidden)]
    fn finish<P: Model>(
        relation: HasManyThrough<P, Self::Parent, Self::Related>,
    ) -> Self::Through<P>;
}

/// A through relationship waiting for its second relationship descriptor.
pub struct PendingHasThroughRelationship<L, const MANY: bool>(L);

impl<L: ThroughRelation> PendingHasThroughRelationship<L, true> {
    pub fn has<D: ThroughRelation<Parent = L::Related>>(
        self,
        distant: D,
    ) -> HasManyThrough<L::Parent, L::Related, D::Related> {
        combine(&self.0, &distant)
    }
}

impl<L: ThroughRelation> PendingHasThroughRelationship<L, false> {
    pub fn has<D: ThroughRelation<Parent = L::Related>>(self, distant: D) -> D::Through<L::Parent> {
        D::finish(combine(&self.0, &distant))
    }
}

fn combine<L: ThroughRelation, D: ThroughRelation<Parent = L::Related>>(
    local: &L,
    distant: &D,
) -> HasManyThrough<L::Parent, L::Related, D::Related> {
    let (local_key, first_key, morph) = local.keys();
    let (second_local_key, second_key, _) = distant.keys();
    HasManyThrough::new(first_key, second_key, local_key, second_local_key)
        .with_intermediate_morph(morph)
}

macro_rules! through_relation {
    ($relation:ident, $many:literal, $output:ident, $keys:expr, $finish:expr) => {
        impl<P: Model, R: Model> ThroughRelation for $relation<P, R> {
            type Parent = P;
            type Related = R;
            type Pending = PendingHasThroughRelationship<Self, $many>;
            type Through<T: Model> = $output<T, P, R>;

            fn keys(
                &self,
            ) -> (
                &'static str,
                &'static str,
                Option<(&'static str, &'static str)>,
            ) {
                $keys(self)
            }

            fn pending(self) -> Self::Pending {
                PendingHasThroughRelationship(self)
            }

            fn finish<T: Model>(relation: HasManyThrough<T, P, R>) -> Self::Through<T> {
                $finish(relation)
            }
        }
    };
}

fn keys<P: Model, R: Model>(
    relation: &HasMany<P, R>,
) -> (
    &'static str,
    &'static str,
    Option<(&'static str, &'static str)>,
) {
    let keys = &relation.0;
    (keys.parent, keys.related, keys.discriminator)
}

through_relation!(HasMany, true, HasManyThrough, keys, std::convert::identity);
through_relation!(
    HasOne,
    false,
    HasOneThrough,
    |relation: &HasOne<P, R>| keys(&relation.0),
    HasManyThrough::one
);
through_relation!(
    MorphMany,
    true,
    HasManyThrough,
    |relation: &MorphMany<P, R>| keys(&relation.0),
    std::convert::identity
);
through_relation!(
    MorphOne,
    false,
    HasOneThrough,
    |relation: &MorphOne<P, R>| keys(&relation.0.0),
    HasManyThrough::one
);

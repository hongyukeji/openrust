use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{MapAccess, SeqAccess, Visitor},
    ser::{SerializeMap, SerializeSeq},
};
use serde_json::Value;

use super::{ArrayKey, Collection, CollectionError, CollectionResult, finite::Finite};

#[allow(non_snake_case, reason = "Matches Laravel's Collection API")]
impl<T: Serialize> Collection<T> {
    pub fn toArray(&self) -> CollectionResult<Value> {
        serde_json::to_value(self).map_err(CollectionError::Serialization)
    }

    pub fn toJson(&self) -> CollectionResult<String> {
        serde_json::to_string(self).map_err(CollectionError::Serialization)
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Collection API")]
impl Collection<Value> {
    pub fn fromValue(value: Value) -> Self {
        match value {
            Value::Null => Self::new(),
            Value::Array(items) => Self::make(items),
            Value::Object(items) => Self::fromEntries(items),
            value => Self::make([value]),
        }
    }
}

impl<T: Serialize> Serialize for Collection<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let sequential = self.items.keys().enumerate().all(|(index, key)| {
            matches!(key, ArrayKey::Integer(key) if usize::try_from(*key) == Ok(index))
        });
        if sequential {
            let mut output = serializer.serialize_seq(Some(self.count()))?;
            for value in self.items.values() {
                output.serialize_element(&Finite(value))?;
            }
            output.end()
        } else {
            let mut output = serializer.serialize_map(Some(self.count()))?;
            for (key, value) in &self.items {
                match key {
                    ArrayKey::Integer(key) => {
                        output.serialize_entry(&key.to_string(), &Finite(value))?
                    }
                    ArrayKey::String(key) => output.serialize_entry(key, &Finite(value))?,
                }
            }
            output.end()
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Collection<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OrderedCollection<T>(std::marker::PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for OrderedCollection<T> {
            type Value = Collection<T>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an array or an ordered object")
            }

            fn visit_seq<S: SeqAccess<'de>>(
                self,
                mut sequence: S,
            ) -> Result<Self::Value, S::Error> {
                let mut collection = Collection::new();
                while let Some(value) = sequence.next_element()? {
                    collection.push(value).map_err(serde::de::Error::custom)?;
                }
                Ok(collection)
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut collection = Collection::new();
                while let Some((key, value)) = map.next_entry::<String, T>()? {
                    collection.put(key, value);
                }
                Ok(collection)
            }
        }

        deserializer.deserialize_any(OrderedCollection(std::marker::PhantomData))
    }
}

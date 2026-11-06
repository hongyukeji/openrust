//! 保留表单字符串值的、按目标类型反序列化的适配器。

use crate::core::{Error, Result};
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};
use serde_json::Value;

pub(super) fn query<T: DeserializeOwned>(value: Value) -> Result<T> {
    T::deserialize(InputValue(value)).map_err(|_| {
        Error::BadRequest("Failed to deserialize query input into the requested type".into())
    })
}
struct InputValue(Value);
macro_rules! number {
    ($method:ident,$visit:ident,$ty:ty) => {
        fn $method<V: Visitor<'de>>(
            self,
            visitor: V,
        ) -> std::result::Result<V::Value, Self::Error> {
            if let Value::String(value) = self.0 {
                let number = value
                    .parse::<$ty>()
                    .map_err(|_| <Self::Error as de::Error>::custom("invalid input number"))?;
                visitor.$visit(number)
            } else {
                self.0.$method(visitor).map_err(de::Error::custom)
            }
        }
    };
}
impl<'de> de::Deserializer<'de> for InputValue {
    type Error = serde::de::value::Error;
    fn deserialize_any<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> std::result::Result<V::Value, Self::Error> {
        match self.0 {
            Value::Array(values) => visitor.visit_seq(Sequence(values.into_iter())),
            Value::Object(values) => visitor.visit_map(Mapping {
                entries: values.into_iter(),
                value: None,
            }),
            other => other.deserialize_any(visitor).map_err(de::Error::custom),
        }
    }
    number!(deserialize_i8, visit_i8, i8);
    number!(deserialize_i16, visit_i16, i16);
    number!(deserialize_i32, visit_i32, i32);
    number!(deserialize_i64, visit_i64, i64);
    number!(deserialize_i128, visit_i128, i128);
    number!(deserialize_u8, visit_u8, u8);
    number!(deserialize_u16, visit_u16, u16);
    number!(deserialize_u32, visit_u32, u32);
    number!(deserialize_u64, visit_u64, u64);
    number!(deserialize_u128, visit_u128, u128);
    number!(deserialize_f32, visit_f32, f32);
    number!(deserialize_f64, visit_f64, f64);
    fn deserialize_bool<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> std::result::Result<V::Value, Self::Error> {
        if let Value::String(value) = self.0 {
            visitor.visit_bool(
                value
                    .parse()
                    .map_err(|_| <Self::Error as de::Error>::custom("invalid input boolean"))?,
            )
        } else {
            self.0.deserialize_bool(visitor).map_err(de::Error::custom)
        }
    }
    fn deserialize_option<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> std::result::Result<V::Value, Self::Error> {
        if self.0.is_null() {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> std::result::Result<V::Value, Self::Error> {
        visitor.visit_newtype_struct(self)
    }
    fn deserialize_map<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> std::result::Result<V::Value, Self::Error> {
        if matches!(&self.0,Value::Array(values) if values.is_empty()) {
            visitor.visit_map(Mapping {
                entries: serde_json::Map::new().into_iter(),
                value: None,
            })
        } else {
            self.deserialize_any(visitor)
        }
    }
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> std::result::Result<V::Value, Self::Error> {
        self.deserialize_map(visitor)
    }
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> std::result::Result<V::Value, Self::Error> {
        let (name, payload) = match self.0 {
            Value::String(name) => (name, None),
            Value::Object(values) if values.len() == 1 => {
                let (name, value) = values
                    .into_iter()
                    .next()
                    .ok_or_else(|| <Self::Error as de::Error>::custom("missing enum variant"))?;
                (name, Some(value))
            }
            _ => {
                return Err(de::Error::custom(
                    "expected enum name or one tagged variant",
                ));
            }
        };
        visitor.visit_enum(EnumValue { name, payload })
    }
    serde::forward_to_deserialize_any! { char str string bytes byte_buf unit unit_struct seq tuple tuple_struct identifier ignored_any }
}
struct Sequence(std::vec::IntoIter<Value>);
impl<'de> SeqAccess<'de> for Sequence {
    type Error = serde::de::value::Error;
    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> std::result::Result<Option<T::Value>, Self::Error> {
        self.0
            .next()
            .map(|value| seed.deserialize(InputValue(value)))
            .transpose()
    }
}
struct Mapping {
    entries: serde_json::map::IntoIter,
    value: Option<Value>,
}
impl<'de> MapAccess<'de> for Mapping {
    type Error = serde::de::value::Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> std::result::Result<Option<K::Value>, Self::Error> {
        if let Some((key, value)) = self.entries.next() {
            self.value = Some(value);
            seed.deserialize(InputValue(Value::String(key))).map(Some)
        } else {
            Ok(None)
        }
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> std::result::Result<V::Value, Self::Error> {
        seed.deserialize(InputValue(
            self.value
                .take()
                .ok_or_else(|| de::Error::custom("missing map value"))?,
        ))
    }
}

struct EnumValue {
    name: String,
    payload: Option<Value>,
}
impl<'de> EnumAccess<'de> for EnumValue {
    type Error = serde::de::value::Error;
    type Variant = Variant;
    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> std::result::Result<(V::Value, Self::Variant), Self::Error> {
        let name = seed.deserialize(InputValue(Value::String(self.name)))?;
        Ok((name, Variant(self.payload)))
    }
}
struct Variant(Option<Value>);
impl Variant {
    fn required(self) -> std::result::Result<Value, serde::de::value::Error> {
        self.0
            .ok_or_else(|| de::Error::custom("enum payload is missing"))
    }
}
impl<'de> VariantAccess<'de> for Variant {
    type Error = serde::de::value::Error;
    fn unit_variant(self) -> std::result::Result<(), Self::Error> {
        match self.0 {
            None => Ok(()),
            Some(value) => serde::Deserialize::deserialize(InputValue(value)),
        }
    }
    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> std::result::Result<T::Value, Self::Error> {
        seed.deserialize(InputValue(self.required()?))
    }
    fn tuple_variant<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> std::result::Result<V::Value, Self::Error> {
        de::Deserializer::deserialize_seq(InputValue(self.required()?), visitor)
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> std::result::Result<V::Value, Self::Error> {
        de::Deserializer::deserialize_map(InputValue(self.required()?), visitor)
    }
}

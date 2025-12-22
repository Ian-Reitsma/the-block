#![allow(dead_code, unused_imports, unused_variables)]
use core::marker::PhantomData;
mod foundation_serialization {
    pub mod serde {
        pub trait Deserialize<'de>: Sized {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>;
        }
        pub trait Deserializer<'de> {
            type Error;
            fn deserialize_identifier<V>(&mut self, _visitor: V) -> Result<V::Value, Self::Error>
            where
                V: de::Visitor<'de>,
            {
                unimplemented!()
            }
            fn deserialize_struct<V>(&mut self, _name: &str, _fields: &[&str], _visitor: V) -> Result<V::Value, Self::Error>
            where
                V: de::Visitor<'de>,
            {
                unimplemented!()
            }
            fn deserialize_map<V>(&mut self, _visitor: V) -> Result<V::Value, Self::Error>
            where
                V: de::Visitor<'de>,
            {
                unimplemented!()
            }
        }
        pub mod de {
            pub trait Visitor<'de> {
                type Value;
            }
            pub trait MapAccess<'de> {
                type Error;
                fn next_key<K>(&mut self) -> Result<Option<K>, Self::Error>;
                fn next_value<T>(&mut self) -> Result<T, Self::Error>;
            }
            pub struct IgnoredAny;
            pub mod Error {
                pub fn duplicate_field<T>(_field: &str) -> T { panic!() }
                pub fn unknown_variant<T>(_value: &str, _variants: &[&str]) -> T { panic!() }
                pub fn missing_field<T>(_field: &str) -> T { panic!() }
                pub fn invalid_length<T>(_len: usize, _visitor: &impl Visitor<'static>) -> T { panic!() }
            }
        }
    }
}
fn default_limit() -> u64 { 0 }
pub struct DutyLogRequest;
#[automatically_derived]
impl<'de> foundation_serialization::serde::Deserialize<'de> for DutyLogRequest  {
    fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>
    where
        D: foundation_serialization::serde::Deserializer<'de>,
    {
        #[allow(non_camel_case_types)]
enum __Field {
Field0,
Field1,
Field2,
__Ignore,
}
impl<'de> foundation_serialization::serde::Deserialize<'de> for __Field {
    fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>
    where
        D: foundation_serialization::serde::Deserializer<'de>,
    {
        struct __FieldVisitor;
        impl<'de> foundation_serialization::serde::de::Visitor<'de> for __FieldVisitor {
            type Value = __Field;
            fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                formatter.write_str("field identifier")
            }
            fn visit_str<E>(self, value: &str) -> ::core::result::Result<__Field, E>
            where
                E: foundation_serialization::serde::de::Error,
            {
                match value {
                    "asset" => Ok(__Field::Field0),
                    "relayer" => Ok(__Field::Field1),
                    "limit" => Ok(__Field::Field2),
                    _ => Ok(__Field::__Ignore),
                }
            }
        }
        deserializer.deserialize_identifier(__FieldVisitor)
    }
}
const FIELDS: &[&str] = &["asset", "relayer", "limit"];
struct __Visitor<'de>(::core::marker::PhantomData<fn(&'de ())>);
impl<'de> foundation_serialization::serde::de::Visitor<'de> for __Visitor<'de> {
    type Value = DutyLogRequest;
    fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        formatter.write_str("struct DutyLogRequest")
    }
    fn visit_map<A>(self, mut map: A) -> ::core::result::Result<Self::Value, A::Error>
    where
        A: foundation_serialization::serde::de::MapAccess<'de>,
    {
        let mut asset = None;
        let mut relayer = None;
        let mut limit = None;
        while let Some(__field) = map.next_key::<__Field>()? {
            match __field {
                __Field::Field0 => {
                if asset.is_some() {
                    return Err(foundation_serialization::serde::de::Error::duplicate_field("asset"));
                }
                asset = Some(map.next_value()?);
            },
                __Field::Field1 => {
                if relayer.is_some() {
                    return Err(foundation_serialization::serde::de::Error::duplicate_field("relayer"));
                }
                relayer = Some(map.next_value()?);
            },
                __Field::Field2 => {
                if limit.is_some() {
                    return Err(foundation_serialization::serde::de::Error::duplicate_field("limit"));
                }
                limit = Some(map.next_value()?);
            },
                __Field::__Ignore => { let _ = map.next_value::<foundation_serialization::serde::de::IgnoredAny>()?; }
            }
        }
        Ok(DutyLogRequest {
        })
    }
}
deserializer.deserialize_struct(stringify!(DutyLogRequest), FIELDS, __Visitor(::core::marker::PhantomData))
    }
}
fn main() {}

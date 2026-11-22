//! Field-by-field decoding. A field that is missing or unreadable keeps its value from the
//! type's fallback, so a file written by an older or newer Kairos (on either platform) still
//! loads. A `req` field has no sensible fallback: without it the whole value fails, and inside a
//! `list` only that element is dropped. Encoding stays derived.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

pub type Fields = Map<String, Value>;

pub fn field<T: DeserializeOwned>(map: &Fields, key: &str, out: &mut T) -> Result<(), String> {
    if let Some(v) = map.get(key) {
        if v.is_null() {
            return Ok(());
        }
        if let Ok(t) = serde_json::from_value::<T>(v.clone()) {
            *out = t;
        }
    }
    Ok(())
}

/// For optional fields: a value that is present and readable replaces the fallback.
pub fn opt<T: DeserializeOwned>(map: &Fields, key: &str, out: &mut Option<T>) -> Result<(), String> {
    if let Some(v) = map.get(key) {
        if v.is_null() {
            return Ok(());
        }
        if let Ok(t) = serde_json::from_value::<T>(v.clone()) {
            *out = Some(t);
        }
    }
    Ok(())
}

pub fn req<T: DeserializeOwned>(map: &Fields, key: &str, out: &mut T) -> Result<(), String> {
    let v = map.get(key).ok_or_else(|| format!("missing field {key}"))?;
    *out = serde_json::from_value::<T>(v.clone()).map_err(|e| format!("field {key}: {e}"))?;
    Ok(())
}

/// Keeps every element that decodes and drops the rest.
pub fn list<T: DeserializeOwned>(map: &Fields, key: &str, out: &mut Vec<T>) -> Result<(), String> {
    if let Some(Value::Array(items)) = map.get(key) {
        *out = items.iter().filter_map(|v| serde_json::from_value::<T>(v.clone()).ok()).collect();
    }
    Ok(())
}

#[macro_export]
macro_rules! lenient {
    ($ty:ty, $fallback:expr, { $( $kind:ident $key:literal => $field:ident ),* $(,)? }) => {
        impl<'de> serde::Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let v = serde_json::Value::deserialize(d)?;
                let map = v.as_object().ok_or_else(|| serde::de::Error::custom("expected an object"))?;
                #[allow(unused_mut)]
                let mut out: $ty = $fallback;
                $( $crate::lenient::$kind(map, $key, &mut out.$field).map_err(serde::de::Error::custom)?; )*
                Ok(out)
            }
        }
        impl $ty {
            /// The keys `Deserialize` reads, for the test that every field is listed.
            #[allow(dead_code)]
            pub const LENIENT_KEYS: &'static [&'static str] = &[$($key),*];
        }
    };
}

//! Transparent decoding of cockpit `dbus-json3` variant envelopes.
//!
//! cockpit's `dbus-json3` payload delivers some D-Bus values flat and others
//! wrapped in a type-tagged variant envelope `{"t": <signature>, "v": <value>}`.
//! `a{sv}` dictionaries (e.g. systemd's `org.freedesktop.DBus.Properties.GetAll`
//! reply) arrive wrapped; positional out-args and journald JSON arrive flat.
//!
//! [`Variant<T>`] lets a capability's payload struct name a field as the plain
//! inner type `T` and still deserialize from either shape, so the per-field
//! `.get("v").unwrap_or(field)` unwrapping that used to live in each capability
//! is expressed once, here.

use serde::de::{self, Deserialize, Deserializer};
use serde_json::Value;

/// A D-Bus value that cockpit may deliver either flat or wrapped in a
/// `{"t": <signature>, "v": <value>}` variant envelope.
///
/// Deserialization unwraps the envelope only when the incoming JSON is an object
/// carrying *both* a `"t"` (type signature) and a `"v"` (value) member — the
/// exact shape cockpit emits for a D-Bus variant. Any other JSON, including a
/// bare scalar or an object lacking either key, is decoded directly as `T`. The
/// "both keys" rule keeps a genuine `a{sv}`-of-`T` payload from being mistaken
/// for an envelope.
///
/// The inner value is public; use `.0` or [`Variant::into_inner`] to access it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Variant<T>(pub T);

impl<T> Variant<T> {
    /// Consume the wrapper and return the decoded inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<'de, T> Deserialize<'de> for Variant<T>
where
    T: serde::de::DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let inner = match value {
            Value::Object(mut map) if map.contains_key("t") && map.contains_key("v") => {
                // Guarded by `contains_key("v")`, so the remove cannot be None.
                map.remove("v").unwrap_or(Value::Null)
            }
            other => other,
        };
        serde_json::from_value(inner)
            .map(Variant)
            .map_err(de::Error::custom)
    }
}

/// A `u64` that cockpit may deliver as a JSON number, a string-encoded number,
/// or inside a `{"t","v"}` variant envelope containing either form.
///
/// dnf5daemon in particular sends some numeric fields as strings (`"12345"`
/// rather than `12345`); this type absorbs both representations transparently.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VariantU64(pub u64);

impl<'de> Deserialize<'de> for VariantU64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let inner = match value {
            Value::Object(mut map) if map.contains_key("t") && map.contains_key("v") => {
                map.remove("v").unwrap_or(Value::Null)
            }
            other => other,
        };
        let n = match &inner {
            Value::Number(n) => n
                .as_u64()
                .or_else(|| n.as_i64().and_then(|i| u64::try_from(i).ok()))
                .unwrap_or(0),
            Value::String(s) => s.parse().unwrap_or(0),
            _ => 0,
        };
        Ok(VariantU64(n))
    }
}

#[cfg(test)]
mod tests {
    use super::{Variant, VariantU64};
    use serde::Deserialize;
    use serde_json::json;

    #[test]
    fn unwraps_string_variant_envelope() {
        let v: Variant<String> = serde_json::from_value(json!({"t": "s", "v": "active"})).unwrap();
        assert_eq!(v.into_inner(), "active");
    }

    #[test]
    fn passes_through_flat_string() {
        let v: Variant<String> = serde_json::from_value(json!("active")).unwrap();
        assert_eq!(v.0, "active");
    }

    #[test]
    fn unwraps_bool_and_u64_envelopes() {
        let b: Variant<bool> = serde_json::from_value(json!({"t": "b", "v": true})).unwrap();
        assert!(b.0);
        let n: Variant<u64> = serde_json::from_value(json!({"t": "t", "v": 42})).unwrap();
        assert_eq!(n.0, 42);
    }

    #[test]
    fn passes_through_flat_bool_and_u64() {
        let b: Variant<bool> = serde_json::from_value(json!(false)).unwrap();
        assert!(!b.0);
        let n: Variant<u64> = serde_json::from_value(json!(7)).unwrap();
        assert_eq!(n.0, 7);
    }

    #[test]
    fn object_without_type_tag_is_not_unwrapped() {
        // Only the cockpit `{"t":_, "v":_}` shape is an envelope. An object that
        // carries "v" but no "t" is a real payload and decodes as itself.
        #[derive(Debug, Deserialize, PartialEq)]
        struct Inner {
            v: u64,
        }
        let v: Variant<Inner> = serde_json::from_value(json!({"v": 9})).unwrap();
        assert_eq!(v.0, Inner { v: 9 });
    }

    #[test]
    fn variant_u64_from_number() {
        let v: VariantU64 = serde_json::from_value(json!(42)).unwrap();
        assert_eq!(v.0, 42);
    }

    #[test]
    fn variant_u64_from_string() {
        let v: VariantU64 = serde_json::from_value(json!("12345")).unwrap();
        assert_eq!(v.0, 12345);
    }

    #[test]
    fn variant_u64_from_wrapped_number() {
        let v: VariantU64 = serde_json::from_value(json!({"t": "t", "v": 99})).unwrap();
        assert_eq!(v.0, 99);
    }

    #[test]
    fn variant_u64_from_wrapped_string() {
        let v: VariantU64 = serde_json::from_value(json!({"t": "t", "v": "456"})).unwrap();
        assert_eq!(v.0, 456);
    }

    #[test]
    fn variant_u64_defaults_on_bad_input() {
        let v: VariantU64 = serde_json::from_value(json!(null)).unwrap();
        assert_eq!(v.0, 0);
        let v: VariantU64 = serde_json::from_value(json!("not_a_number")).unwrap();
        assert_eq!(v.0, 0);
    }

    #[test]
    fn variant_u64_handles_negative_i64() {
        let v: VariantU64 = serde_json::from_value(json!(-5)).unwrap();
        assert_eq!(v.0, 0);
    }

    #[test]
    fn variant_u64_defaults_when_absent_in_struct() {
        #[derive(Debug, Deserialize, Default)]
        struct Props {
            #[serde(default)]
            size: VariantU64,
        }
        let p: Props = serde_json::from_value(json!({})).unwrap();
        assert_eq!(p.size.0, 0);
    }

    #[test]
    fn wrong_inner_type_is_a_decode_error() {
        let err = serde_json::from_value::<Variant<String>>(json!({"t": "i", "v": 5}));
        assert!(err.is_err());
    }

    #[test]
    fn fields_default_when_absent_in_struct() {
        // With `#[serde(default)]`, an absent envelope field falls back to the
        // inner type's Default — mirroring the old `s()` "" behavior.
        #[derive(Debug, Deserialize, Default)]
        struct Props {
            #[serde(default)]
            id: Variant<String>,
        }
        let p: Props = serde_json::from_value(json!({})).unwrap();
        assert_eq!(p.id.0, "");
    }
}

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;
use uuid::Uuid;

const BASE62_ALPHABET: &[u8; 62] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Generate a compact, URL-safe identifier for presentation layers.
///
/// UUIDv4 stores 122 random bits and is normally rendered as 36 characters
/// with hyphens. This keeps the same collision profile but renders the backing
/// 128-bit value in base62, producing up to 22 ASCII characters.
#[must_use]
pub fn compact_id() -> String {
    compact_uuid(Uuid::new_v4())
}

/// Render an existing UUID in compact base62 form.
#[must_use]
pub fn compact_uuid(uuid: Uuid) -> String {
    encode_base62(uuid.as_u128())
}

fn encode_base62(mut value: u128) -> String {
    if value == 0 {
        return "0".to_string();
    }

    let mut buf = [0_u8; 22];
    let mut index = buf.len();

    while value > 0 {
        index -= 1;
        buf[index] = BASE62_ALPHABET[(value % 62) as usize];
        value /= 62;
    }

    // SAFETY: BASE62_ALPHABET contains only ASCII bytes [0-9A-Za-z],
    // so the buffer is always valid UTF-8.
    unsafe { std::str::from_utf8_unchecked(&buf[index..]) }.to_string()
}

macro_rules! define_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn into_inner(self) -> Uuid {
                self.0
            }

            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            pub fn is_nil(&self) -> bool {
                self.0.is_nil()
            }
        }

        impl From<Uuid> for $name {
            fn from(u: Uuid) -> Self {
                Self(u)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }

        impl Deref for $name {
            type Target = Uuid;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl PartialEq<Uuid> for $name {
            fn eq(&self, other: &Uuid) -> bool {
                &self.0 == other
            }
        }

        impl PartialEq<$name> for Uuid {
            fn eq(&self, other: &$name) -> bool {
                self == &other.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::from_str(s).map(Self)
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self(Uuid::nil())
            }
        }

        #[cfg(feature = "sqlx-postgres")]
        impl sqlx::Type<sqlx::Postgres> for $name {
            fn type_info() -> sqlx::postgres::PgTypeInfo {
                <Uuid as sqlx::Type<sqlx::Postgres>>::type_info()
            }
        }

        #[cfg(feature = "sqlx-postgres")]
        impl<'r> sqlx::Decode<'r, sqlx::Postgres> for $name {
            fn decode(
                value: sqlx::postgres::PgValueRef<'r>,
            ) -> Result<Self, Box<dyn std::error::Error + Send + Sync + 'static>> {
                let uuid = <Uuid as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
                if uuid.is_nil() {
                    return Err("Decoded nil UUID for typed ID".into());
                }
                Ok(Self(uuid))
            }
        }

        #[cfg(feature = "sqlx-postgres")]
        impl sqlx::Encode<'_, sqlx::Postgres> for $name {
            fn encode_by_ref(
                &self,
                buf: &mut sqlx::postgres::PgArgumentBuffer,
            ) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync + 'static>>
            {
                <Uuid as sqlx::Encode<sqlx::Postgres>>::encode_by_ref(&self.0, buf)
            }
        }
    };
}

define_id!(AppId, "Unique identifier for a Mikrom application.");
define_id!(UserId, "Unique identifier for a Mikrom user.");
define_id!(DeploymentId, "Unique identifier for a Mikrom deployment.");
define_id!(VmId, "Unique identifier for a microVM instance.");
define_id!(WorkerId, "Unique identifier for a Mikrom worker node.");
define_id!(VpcId, "Unique identifier for a Virtual Private Cloud.");
define_id!(
    SecurityRuleId,
    "Unique identifier for a security firewall rule."
);

#[cfg(test)]
mod tests {
    use super::{compact_id, encode_base62};
    use std::collections::HashSet;

    #[test]
    fn compact_id_is_short_ascii_base62() {
        let id = compact_id();

        assert!(!id.is_empty());
        assert!(id.len() <= 22);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn compact_id_does_not_collide_in_sample() {
        let mut seen = HashSet::new();

        for _ in 0..10_000 {
            assert!(seen.insert(compact_id()));
        }
    }

    #[test]
    fn encodes_known_values() {
        assert_eq!(encode_base62(0), "0");
        assert_eq!(encode_base62(61), "z");
        assert_eq!(encode_base62(62), "10");
    }
}

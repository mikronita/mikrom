use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;
use uuid::Uuid;

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

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct VmId(String);

impl VmId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for VmId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for VmId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for VmId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AppId(String);

impl AppId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for AppId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for AppId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

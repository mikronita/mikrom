use rovo::schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TypeError {
    #[error("Invalid port: {0}. Must be between 1 and 65535")]
    InvalidPort(i32),
    #[error("Invalid memory: {0}MB. Must be at least 128MB")]
    InvalidMemory(i32),
    #[error("Invalid CPU cores: {0}. Must be at least 1")]
    InvalidCpuCores(i32),
}

/// A valid network port (1-65535)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Port(u32);

impl Port {
    pub fn new(val: u32) -> Result<Self, TypeError> {
        if val > 0 && val <= 65535 {
            Ok(Self(val))
        } else {
            Err(TypeError::InvalidPort(val as i32))
        }
    }

    pub fn value(&self) -> u32 {
        self.0
    }
}

impl TryFrom<i32> for Port {
    type Error = TypeError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        if value > 0 && value <= 65535 {
            Ok(Self(value as u32))
        } else {
            Err(TypeError::InvalidPort(value))
        }
    }
}

impl TryFrom<i64> for Port {
    type Error = TypeError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if value > 0 && value <= 65535 {
            Ok(Self(value as u32))
        } else {
            Err(TypeError::InvalidPort(value as i32))
        }
    }
}

impl<'de> Deserialize<'de> for Port {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        Port::new(value).map_err(serde::de::Error::custom)
    }
}

impl From<Port> for i32 {
    fn from(p: Port) -> Self {
        p.0 as i32
    }
}

impl fmt::Display for Port {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq<u32> for Port {
    fn eq(&self, other: &u32) -> bool {
        self.0 == *other
    }
}

impl PartialEq<Port> for u32 {
    fn eq(&self, other: &Port) -> bool {
        *self == other.0
    }
}

/// Memory in Megabytes
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct MemoryMb(u32);

impl MemoryMb {
    pub fn new(val: u32) -> Result<Self, TypeError> {
        if val >= 128 {
            Ok(Self(val))
        } else {
            Err(TypeError::InvalidMemory(val as i32))
        }
    }

    pub fn value(&self) -> u32 {
        self.0
    }
}

impl TryFrom<i32> for MemoryMb {
    type Error = TypeError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        if value >= 128 {
            Ok(Self(value as u32))
        } else {
            Err(TypeError::InvalidMemory(value))
        }
    }
}

impl TryFrom<i64> for MemoryMb {
    type Error = TypeError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if value >= 128 {
            Ok(Self(value as u32))
        } else {
            Err(TypeError::InvalidMemory(value as i32))
        }
    }
}

impl TryFrom<u32> for MemoryMb {
    type Error = TypeError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value >= 128 {
            Ok(Self(value))
        } else {
            Err(TypeError::InvalidMemory(value as i32))
        }
    }
}

impl<'de> Deserialize<'de> for MemoryMb {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        MemoryMb::new(value).map_err(serde::de::Error::custom)
    }
}

impl From<MemoryMb> for i32 {
    fn from(m: MemoryMb) -> Self {
        m.0 as i32
    }
}

impl PartialEq<u32> for MemoryMb {
    fn eq(&self, other: &u32) -> bool {
        self.0 == *other
    }
}

impl PartialEq<MemoryMb> for u32 {
    fn eq(&self, other: &MemoryMb) -> bool {
        *self == other.0
    }
}

/// Number of CPU cores
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct CpuCores(u32);

impl CpuCores {
    pub fn new(val: u32) -> Result<Self, TypeError> {
        if val >= 1 {
            Ok(Self(val))
        } else {
            Err(TypeError::InvalidCpuCores(val as i32))
        }
    }

    pub fn value(&self) -> u32 {
        self.0
    }
}

impl TryFrom<i32> for CpuCores {
    type Error = TypeError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        if value >= 1 {
            Ok(Self(value as u32))
        } else {
            Err(TypeError::InvalidCpuCores(value))
        }
    }
}

impl TryFrom<i64> for CpuCores {
    type Error = TypeError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if value >= 1 {
            Ok(Self(value as u32))
        } else {
            Err(TypeError::InvalidCpuCores(value as i32))
        }
    }
}

impl TryFrom<u32> for CpuCores {
    type Error = TypeError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value >= 1 {
            Ok(Self(value))
        } else {
            Err(TypeError::InvalidCpuCores(value as i32))
        }
    }
}

impl<'de> Deserialize<'de> for CpuCores {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        CpuCores::new(value).map_err(serde::de::Error::custom)
    }
}

impl From<CpuCores> for i32 {
    fn from(c: CpuCores) -> Self {
        c.0 as i32
    }
}

impl PartialEq<u32> for CpuCores {
    fn eq(&self, other: &u32) -> bool {
        self.0 == *other
    }
}

impl PartialEq<CpuCores> for u32 {
    fn eq(&self, other: &CpuCores) -> bool {
        *self == other.0
    }
}

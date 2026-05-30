use thiserror::Error;

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("Netlink error: {0}")]
    Netlink(String),

    #[error("IO error: {0}")]
    StdIo(#[from] std::io::Error),

    #[error("WireGuard device not found: {0}")]
    DeviceNotFound(String),

    #[error("Key decode error: {0}")]
    KeyDecode(String),

    #[error("Key derivation error: {0}")]
    KeyDerivation(String),

    #[error("Invalid IP address or prefix: {0}")]
    InvalidIp(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Address parse error: {0}")]
    AddrParse(#[from] std::net::AddrParseError),

    #[error("Base64 error: {0}")]
    Base64(#[from] base64::DecodeError),

    #[error("Hex error: {0}")]
    Hex(#[from] hex::FromHexError),

    #[error("Neli builder error: {0}")]
    NeliBuilder(String),
}

impl
    From<
        neli::err::RouterError<
            u16,
            neli::genl::Genlmsghdr<
                crate::wireguard::netlink::WgCmd,
                crate::wireguard::netlink::WgDeviceAttr,
            >,
        >,
    > for NetworkError
{
    fn from(
        err: neli::err::RouterError<
            u16,
            neli::genl::Genlmsghdr<
                crate::wireguard::netlink::WgCmd,
                crate::wireguard::netlink::WgDeviceAttr,
            >,
        >,
    ) -> Self {
        Self::Netlink(err.to_string())
    }
}

impl From<neli::err::RouterError<u16, neli::genl::Genlmsghdr<u8, u16>>> for NetworkError {
    fn from(err: neli::err::RouterError<u16, neli::genl::Genlmsghdr<u8, u16>>) -> Self {
        Self::Netlink(err.to_string())
    }
}

impl From<neli::genl::AttrTypeBuilderError> for NetworkError {
    fn from(err: neli::genl::AttrTypeBuilderError) -> Self {
        Self::NeliBuilder(err.to_string())
    }
}

impl From<neli::genl::NlattrBuilderError> for NetworkError {
    fn from(err: neli::genl::NlattrBuilderError) -> Self {
        Self::NeliBuilder(err.to_string())
    }
}

impl From<neli::genl::GenlmsghdrBuilderError> for NetworkError {
    fn from(err: neli::genl::GenlmsghdrBuilderError) -> Self {
        Self::NeliBuilder(err.to_string())
    }
}

impl From<anyhow::Error> for NetworkError {
    fn from(err: anyhow::Error) -> Self {
        Self::Internal(err.to_string())
    }
}

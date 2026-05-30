use neli::consts::genl::{Cmd, NlAttrType};
use neli::neli_enum;

#[neli_enum(serialized_type = "u8")]
pub enum WgCmd {
    GetDevice = 0,
    SetDevice = 1,
}

impl Cmd for WgCmd {}

#[neli_enum(serialized_type = "u16")]
pub enum WgDeviceAttr {
    Unspec = 0,
    Ifindex = 1,
    Ifname = 2,
    PrivateKey = 3,
    PublicKey = 4,
    Flags = 5,
    ListenPort = 6,
    Fwmark = 7,
    Peers = 8,
}

impl NlAttrType for WgDeviceAttr {}

#[neli_enum(serialized_type = "u16")]
pub enum WgPeerAttr {
    Unspec = 0,
    PublicKey = 1,
    PresharedKey = 2,
    Flags = 3,
    Endpoint = 4,
    PersistentKeepaliveInterval = 5,
    LastHandshakeTime = 6,
    RxBytes = 7,
    TxBytes = 8,
    AllowedIps = 9,
    ProtocolVersion = 10,
}

impl NlAttrType for WgPeerAttr {}

#[neli_enum(serialized_type = "u16")]
pub enum WgAllowedIpAttr {
    Unspec = 0,
    Family = 1,
    Ipaddr = 2,
    CidrMask = 3,
    Flags = 4,
}

impl NlAttrType for WgAllowedIpAttr {}

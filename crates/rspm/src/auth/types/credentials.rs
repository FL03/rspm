/*
    Appellation: credentials <module>
    Created At: 2026.08.08:06:47:47
    Contrib: @FL03
*/

pub enum PolymarketCredentials {
    L1(PrivateKey),
    L2(Layer2Creds),
}

pub struct Layer2Creds {
    pub(crate) key: String,
    pub(crate) secret: String,
    pub(crate) passphrase: String,
}

pub struct PrivateKey(String);

impl PrivateKey {
    pub fn from_env() -> Result<Self, std::env::VarError> {
        std::env::var("POLYMARKET_PRIVATE_KEY").map(Self)
    }
}

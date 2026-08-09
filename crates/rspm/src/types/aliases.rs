/*
    Appellation: aliases <module>
    Created At: 2026.05.09:07:50:46
    Contrib: @FL03
*/

#[cfg(feature = "alloc")]
/// A type alias providing a named type for handling indentifiers to and from polymarket.
pub type Id = alloc::string::String;
/// Concrete signer type used by the Polymarket SDK.
#[cfg(all(feature = "ecdsa", feature = "sdk"))]
pub type PrivateKeySigner = polymarket::auth::LocalSigner<k256::ecdsa::SigningKey>;

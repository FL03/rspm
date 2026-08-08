/*
    Appellation: helpers <module>
    Created At: 2026.08.08:02:46:07
    Contrib: @FL03
*/
use polymarket::{auth::{Credentials, ExposeSecret}, types::Address};
use crate::URL_SAFE;
use hmac::Hmac;
use reqwest::header::{HeaderMap, HeaderValue};
use crate::auth::{POLY_ADDRESS, POLY_API_KEY, POLY_PASSPHRASE, POLY_SIGNATURE, POLY_TIMESTAMP};
use sha2::Sha256;

pub fn authenticated_endpoint_is_safe(endpoint: &url::Url) -> bool {
    crate::clob::authenticated_clob_endpoint_is_safe(endpoint)
}


pub fn signature_material(timestamp: i64, path: &str) -> String {
    format!("{timestamp}GET{path}")
}

pub fn l2_signature(credentials: &Credentials, timestamp: i64, path: &str) -> Option<String> {
    let secret = URL_SAFE
        .decode(credentials.secret().expose_secret().as_bytes())
        .ok()?;
    let mut mac = Hmac::<Sha256>::new_from_slice(&secret).ok()?;
    mac.update(signature_material(timestamp, path).as_bytes());
    Some(URL_SAFE.encode(mac.finalize().into_bytes()))
}

pub fn l2_headers(
    address: Address,
    credentials: &Credentials,
    timestamp: i64,
    path: &str,
) -> Option<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        POLY_ADDRESS,
        sensitive_header_value(&address.to_checksum(None))?,
    );
    headers.insert(
        POLY_API_KEY,
        sensitive_header_value(&credentials.key().to_string())?,
    );
    headers.insert(
        POLY_PASSPHRASE,
        sensitive_header_value(credentials.passphrase().expose_secret())?,
    );
    headers.insert(
        POLY_SIGNATURE,
        sensitive_header_value(&l2_signature(credentials, timestamp, path)?)?,
    );
    headers.insert(
        POLY_TIMESTAMP,
        HeaderValue::from_str(&timestamp.to_string()).ok()?,
    );
    Some(headers)
}

pub(crate) fn sensitive_header_value(value: &str) -> Option<HeaderValue> {
    let mut value = HeaderValue::from_str(value).ok()?;
    value.set_sensitive(true);
    Some(value)
}

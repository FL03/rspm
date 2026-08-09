//! Canonical content identities for authenticated private transport evidence.

use alloc::string::String;

use sha2::{Digest as _, Sha256};

const EVIDENCE_DOMAIN_V1: &[u8] = b"authenticated-private-frame-evidence.v1";
const PAYLOAD_DOMAIN_V1: &[u8] = b"authenticated-private-frame-payload.v1";

/// Canonical wire-encoding discriminator used by the v1 evidence identity.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticatedPrivateFrameIdentityEncodingV1 {
    Text,
    Binary,
    Raw,
}

/// Canonical gap discriminator used by the v1 evidence identity.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticatedPrivateFrameIdentityGapV1 {
    InvalidTextSchema,
    UnsupportedBinary,
    UnsupportedRawFrame,
}

/// Every field bound by the v1 authenticated private-frame evidence key.
#[doc(hidden)]
pub struct AuthenticatedPrivateFrameIdentityV1<'a> {
    pub owner_id: u64,
    pub session_id: u64,
    pub process_generation: &'a str,
    pub receipt_wall_time_ns: i64,
    pub receipt_monotonic_ns: u64,
    pub frame_sequence: u64,
    pub first_transport_sequence: u64,
    pub last_transport_sequence: u64,
    pub socket_generation: u64,
    pub socket_gap_version: u64,
    pub encoding: AuthenticatedPrivateFrameIdentityEncodingV1,
    pub gap: Option<AuthenticatedPrivateFrameIdentityGapV1>,
    pub payload: &'a [u8],
}

fn framed_sha256(domain: &[u8], parts: &[&[u8]]) -> Result<String, &'static str> {
    let mut digest = Sha256::new();
    for part in core::iter::once(domain).chain(parts.iter().copied()) {
        let length = u64::try_from(part.len())
            .map_err(|_| "authenticated private frame digest input is too large")?;
        digest.update(length.to_be_bytes());
        digest.update(part);
    }
    Ok(crate::to_hex(digest.finalize().as_slice()))
}

impl AuthenticatedPrivateFrameIdentityV1<'_> {
    /// Compute the sole v1 key authority for exact authenticated frame bytes.
    pub fn evidence_key(&self) -> Result<String, &'static str> {
        let owner_id = self.owner_id.to_be_bytes();
        let session_id = self.session_id.to_be_bytes();
        let receipt_wall_time_ns = self.receipt_wall_time_ns.to_be_bytes();
        let receipt_monotonic_ns = self.receipt_monotonic_ns.to_be_bytes();
        let frame_sequence = self.frame_sequence.to_be_bytes();
        let first_transport_sequence = self.first_transport_sequence.to_be_bytes();
        let last_transport_sequence = self.last_transport_sequence.to_be_bytes();
        let socket_generation = self.socket_generation.to_be_bytes();
        let socket_gap_version = self.socket_gap_version.to_be_bytes();
        let encoding = [match self.encoding {
            AuthenticatedPrivateFrameIdentityEncodingV1::Text => 1,
            AuthenticatedPrivateFrameIdentityEncodingV1::Binary => 2,
            AuthenticatedPrivateFrameIdentityEncodingV1::Raw => 3,
        }];
        let gap = [match self.gap {
            None => 0,
            Some(AuthenticatedPrivateFrameIdentityGapV1::InvalidTextSchema) => 1,
            Some(AuthenticatedPrivateFrameIdentityGapV1::UnsupportedBinary) => 2,
            Some(AuthenticatedPrivateFrameIdentityGapV1::UnsupportedRawFrame) => 3,
        }];
        framed_sha256(
            EVIDENCE_DOMAIN_V1,
            &[
                &owner_id,
                &session_id,
                self.process_generation.as_bytes(),
                &receipt_wall_time_ns,
                &receipt_monotonic_ns,
                &frame_sequence,
                &first_transport_sequence,
                &last_transport_sequence,
                &socket_generation,
                &socket_gap_version,
                &encoding,
                &gap,
                self.payload,
            ],
        )
    }
}

/// Compute the sole v1 digest authority for exact private-frame payload bytes.
#[doc(hidden)]
pub fn authenticated_private_frame_payload_digest_v1(
    payload: &[u8],
) -> Result<String, &'static str> {
    framed_sha256(PAYLOAD_DOMAIN_V1, &[payload])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity<'a>(payload: &'a [u8]) -> AuthenticatedPrivateFrameIdentityV1<'a> {
        AuthenticatedPrivateFrameIdentityV1 {
            owner_id: 1,
            session_id: 2,
            process_generation: "00000000-0000-4000-8000-000000000001",
            receipt_wall_time_ns: 1_700_000_000_000_000_001,
            receipt_monotonic_ns: 3,
            frame_sequence: 4,
            first_transport_sequence: 5,
            last_transport_sequence: 6,
            socket_generation: 7,
            socket_gap_version: 8,
            encoding: AuthenticatedPrivateFrameIdentityEncodingV1::Text,
            gap: None,
            payload,
        }
    }

    #[test]
    fn v1_framing_separates_field_boundaries_and_hash_domains() {
        assert_ne!(
            framed_sha256(EVIDENCE_DOMAIN_V1, &[b"a", b"bc"]).unwrap(),
            framed_sha256(EVIDENCE_DOMAIN_V1, &[b"ab", b"c"]).unwrap()
        );
        assert_ne!(
            framed_sha256(EVIDENCE_DOMAIN_V1, &[b"same"]).unwrap(),
            authenticated_private_frame_payload_digest_v1(b"same").unwrap()
        );
    }

    #[test]
    fn every_identity_field_changes_the_v1_evidence_key() {
        let baseline = identity(b"payload").evidence_key().unwrap();
        let mutations = [
            AuthenticatedPrivateFrameIdentityV1 {
                owner_id: 9,
                ..identity(b"payload")
            },
            AuthenticatedPrivateFrameIdentityV1 {
                session_id: 9,
                ..identity(b"payload")
            },
            AuthenticatedPrivateFrameIdentityV1 {
                process_generation: "00000000-0000-4000-8000-000000000009",
                ..identity(b"payload")
            },
            AuthenticatedPrivateFrameIdentityV1 {
                receipt_wall_time_ns: 9,
                ..identity(b"payload")
            },
            AuthenticatedPrivateFrameIdentityV1 {
                receipt_monotonic_ns: 9,
                ..identity(b"payload")
            },
            AuthenticatedPrivateFrameIdentityV1 {
                frame_sequence: 9,
                ..identity(b"payload")
            },
            AuthenticatedPrivateFrameIdentityV1 {
                first_transport_sequence: 9,
                ..identity(b"payload")
            },
            AuthenticatedPrivateFrameIdentityV1 {
                last_transport_sequence: 9,
                ..identity(b"payload")
            },
            AuthenticatedPrivateFrameIdentityV1 {
                socket_generation: 9,
                ..identity(b"payload")
            },
            AuthenticatedPrivateFrameIdentityV1 {
                socket_gap_version: 9,
                ..identity(b"payload")
            },
            AuthenticatedPrivateFrameIdentityV1 {
                encoding: AuthenticatedPrivateFrameIdentityEncodingV1::Binary,
                ..identity(b"payload")
            },
            AuthenticatedPrivateFrameIdentityV1 {
                gap: Some(AuthenticatedPrivateFrameIdentityGapV1::InvalidTextSchema),
                ..identity(b"payload")
            },
            identity(b"changed"),
        ];
        for mutation in mutations {
            assert_ne!(mutation.evidence_key().unwrap(), baseline);
        }
    }
}

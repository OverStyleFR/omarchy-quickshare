//! Native Android QR bootstrap, following NearDrop's protocol and wire fields
//! at add655c6c5a40b9ba03c0329b598f2817185f5a9. A discovery match associates
//! a receiver; only the UKEY2-bound signature proves ownership of the QR key.
use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes128Gcm, Nonce,
};
use anyhow::{anyhow, ensure, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hkdf::Hkdf;
use p256::{
    ecdsa::{signature::Signer, Signature, SigningKey},
    elliptic_curve::{rand_core::OsRng, sec1::ToEncodedPoint},
    SecretKey,
};
use sha2::Sha256;
use std::{fmt, sync::Arc, time::{Duration, Instant}};
use subtle::ConstantTimeEq;

use crate::{DeviceType, EndpointInfo};

pub const QR_INVITATION_TTL: Duration = Duration::from_secs(120);
pub(crate) const MAX_ENDPOINT_INFO: usize = 1024;
const MAX_ENDPOINT_TLVS: usize = 32;

pub struct QrInvitation {
    id: String,
    url: String,
    expires_at: Instant,
    signing_key: SigningKey,
    advertising_token: [u8; 16],
    name_key: [u8; 16],
}

// Deliberately exclude key material, derived tokens and even the public URL.
impl fmt::Debug for QrInvitation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QrInvitation")
            .field("id", &self.id)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct QrPeer {
    pub invitation_id: String,
    pub peer: EndpointInfo,
    pub endpoint_id: String,
}

#[derive(Debug, Clone)]
pub struct QrSendContext {
    pub invitation: Arc<QrInvitation>,
    pub endpoint_id: String,
}

pub(crate) struct EndpointMetadata<'a> {
    pub device_type: DeviceType,
    pub name: Option<&'a str>,
    qr_data: Option<&'a [u8]>,
}

impl QrInvitation {
    pub fn new() -> Result<Self> {
        Self::from_secret(SecretKey::random(&mut OsRng), Instant::now())
    }

    fn from_secret(mut secret: SecretKey, now: Instant) -> Result<Self> {
        // Working native implementations fix the format byte to 02. Normalize
        // the scalar rather than publishing 02 with an inconsistent odd-Y key.
        if secret.public_key().to_encoded_point(true).as_bytes()[0] == 3 {
            let scalar = -*secret.to_nonzero_scalar();
            secret = SecretKey::from_bytes(&scalar.to_bytes())?;
        }
        let point = secret.public_key().to_encoded_point(true);
        let mut wire = [0u8; 35];
        wire[2..].copy_from_slice(point.as_bytes());
        let (advertising_token, name_key) = derive_keys(&wire)?;
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            url: format!("https://quickshare.google/qrcode#key={}", URL_SAFE_NO_PAD.encode(wire)),
            expires_at: now + QR_INVITATION_TTL,
            signing_key: SigningKey::from(secret),
            advertising_token,
            name_key,
        })
    }

    pub fn id(&self) -> &str { &self.id }
    pub fn url(&self) -> &str { &self.url }
    pub fn expires_at(&self) -> Instant { self.expires_at }
    pub fn is_expired_at(&self, now: Instant) -> bool { now >= self.expires_at }

    pub(crate) fn matched_name(&self, metadata: &EndpointMetadata<'_>, now: Instant) -> Option<String> {
        if self.is_expired_at(now) { return None; }
        let data = metadata.qr_data?;
        if let Some(name) = metadata.name {
            if bool::from(data.ct_eq(&self.advertising_token)) {
                return Some(name.to_owned());
            }
        }
        // The first 12 bytes are the nonce; decrypt authenticates the final
        // 16-byte tag and the advertising token before exposing the name.
        if data.len() <= 28 { return None; }
        let cipher = Aes128Gcm::new_from_slice(&self.name_key).ok()?;
        let nonce: [u8; 12] = data[..12].try_into().ok()?;
        let bytes = cipher.decrypt(&Nonce::from(nonce), Payload {
            msg: &data[12..], aad: &self.advertising_token,
        }).ok()?;
        let name = String::from_utf8(bytes).ok()?;
        (!name.is_empty()).then_some(name)
    }

    pub(crate) fn sign_auth_token(&self, auth_token: &[u8; 32]) -> [u8; 64] {
        // Signer hashes the raw UKEY2 auth token with SHA256, then ECDSA signs
        // that digest. Signature::to_bytes is P1363 R32 || S32, never DER/PIN.
        let signature: Signature = self.signing_key.sign(auth_token);
        signature.to_bytes().into()
    }
}

fn derive_keys(wire: &[u8; 35]) -> Result<([u8; 16], [u8; 16])> {
    let hkdf = Hkdf::<Sha256>::new(Some(&[]), wire);
    let mut token = [0; 16];
    let mut key = [0; 16];
    hkdf.expand(b"advertisingContext", &mut token).map_err(|_| anyhow!("QR token derivation"))?;
    hkdf.expand(b"encryptionKey", &mut key).map_err(|_| anyhow!("QR name key derivation"))?;
    Ok((token, key))
}

pub(crate) fn parse_endpoint_metadata(bytes: &[u8]) -> Result<EndpointMetadata<'_>> {
    ensure!((17..=MAX_ENDPOINT_INFO).contains(&bytes.len()), "invalid endpoint info size");
    let flags = bytes[0];
    ensure!(flags >> 5 <= 1 && flags & 1 == 0, "invalid endpoint info flags");
    let mut offset = 17;
    let name = if flags & 0x10 == 0 {
        let length = *bytes.get(offset).ok_or_else(|| anyhow!("missing endpoint name"))? as usize;
        offset += 1;
        ensure!(length > 0, "empty endpoint name");
        let end = offset + length; // length is bounded by its single wire byte.
        let name = std::str::from_utf8(bytes.get(offset..end).ok_or_else(|| anyhow!("truncated endpoint name"))?)?;
        offset = end;
        Some(name)
    } else { None };
    let mut qr_data = None;
    let mut count = 0;
    while offset < bytes.len() {
        count += 1;
        ensure!(count <= MAX_ENDPOINT_TLVS, "too many endpoint TLVs");
        let header = bytes.get(offset..offset + 2).ok_or_else(|| anyhow!("truncated endpoint TLV"))?;
        let end = offset + 2 + header[1] as usize;
        let value = bytes.get(offset + 2..end).ok_or_else(|| anyhow!("truncated endpoint TLV value"))?;
        match header[0] {
            1 => {
                ensure!(qr_data.is_none(), "duplicate QR endpoint TLV");
                ensure!(value.len() == 16 || value.len() > 28, "invalid QR endpoint TLV length");
                qr_data = Some(value);
            }
            2 => ensure!(value.len() == 1, "invalid vendor endpoint TLV"),
            _ => {}
        }
        offset = end;
    }
    Ok(EndpointMetadata { device_type: DeviceType::from_raw_value((flags >> 1) & 7), name, qr_data })
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::{signature::Verifier, VerifyingKey};

    pub(crate) fn invitation(value: u8, now: Instant) -> QrInvitation {
        let mut scalar = [0; 32];
        scalar[31] = value;
        QrInvitation::from_secret(SecretKey::from_slice(&scalar).unwrap(), now).unwrap()
    }

    fn visible(name: &str, token: &[u8]) -> Vec<u8> {
        let mut bytes = vec![2; 17];
        bytes.push(name.len() as u8);
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&[1, token.len() as u8]);
        bytes.extend_from_slice(token);
        bytes
    }

    fn hidden(invitation: &QrInvitation, name: &[u8]) -> Vec<u8> {
        let nonce = [7u8; 12];
        let cipher = Aes128Gcm::new_from_slice(&invitation.name_key).unwrap();
        let encrypted = cipher.encrypt(&Nonce::from(nonce), Payload {
            msg: name, aad: &invitation.advertising_token,
        }).unwrap();
        let mut bytes = vec![0x12; 17];
        bytes.extend_from_slice(&[1, (nonce.len() + encrypted.len()) as u8]);
        bytes.extend_from_slice(&nonce);
        bytes.extend_from_slice(&encrypted);
        bytes
    }

    #[test]
    fn published_hkdf_vector_covers_version_bytes_and_context_labels() {
        // Bada QrCodeVectors.kt at afd44cd5b18b3e00f817ac8323be258127f16464.
        let mut wire = [0; 35];
        wire[2] = 2;
        for (index, byte) in wire[3..].iter_mut().enumerate() { *byte = index as u8 + 1; }
        let (token, key) = derive_keys(&wire).unwrap();
        assert_eq!(hex::encode(token), "0437d95175045e8ce8dc777e93b6c358");
        assert_eq!(hex::encode(key), "39cfade1d68ccfdfe2b882e84d4e3860");
    }

    #[test]
    fn fixed02_url_reconstructs_the_signing_key_even_after_scalar_negation() {
        let now = Instant::now();
        let mut saw_odd = false;
        for value in 1..=16 {
            let qr = invitation(value, now);
            let wire = URL_SAFE_NO_PAD.decode(qr.url().split_once("#key=").unwrap().1).unwrap();
            assert_eq!(wire.len(), 35);
            assert_eq!(&wire[..3], &[0, 0, 2]);
            let verifier = VerifyingKey::from_sec1_bytes(&wire[2..]).unwrap();
            assert_eq!(verifier, *qr.signing_key.verifying_key());
            let mut original = [0; 32]; original[31] = value;
            if SecretKey::from_slice(&original).unwrap().public_key().to_encoded_point(true).as_bytes()[0] == 3 {
                saw_odd = true;
                assert_ne!(&qr.signing_key.to_bytes()[..], &original);
            }
        }
        assert!(saw_odd, "exercise scalar normalization, not just naturally even keys");
        assert_ne!(invitation(1, now).id(), invitation(1, now).id());
    }

    #[test]
    fn visible_and_hidden_matches_require_current_key_unexpired_and_authenticated_names() {
        let now = Instant::now();
        let qr = invitation(1, now);
        let other = invitation(2, now);
        for bytes in [visible("Pixel ñ", &qr.advertising_token), hidden(&qr, "Pixel ñ".as_bytes())] {
            let metadata = parse_endpoint_metadata(&bytes).unwrap();
            assert_eq!(qr.matched_name(&metadata, now).as_deref(), Some("Pixel ñ"));
            assert!(other.matched_name(&metadata, now).is_none());
            assert!(qr.matched_name(&metadata, now + QR_INVITATION_TTL).is_none());
        }
        assert!(!qr.is_expired_at(qr.expires_at() - Duration::from_nanos(1)));
        assert!(qr.is_expired_at(qr.expires_at()));
        let mut tampered = hidden(&qr, b"Pixel");
        *tampered.last_mut().unwrap() ^= 1;
        assert!(qr.matched_name(&parse_endpoint_metadata(&tampered).unwrap(), now).is_none());
        for name in [b"".as_slice(), &[0xff]] {
            let encoded = hidden(&qr, name);
            let matched = parse_endpoint_metadata(&encoded).ok().and_then(|m| qr.matched_name(&m, now));
            assert!(matched.is_none());
        }
    }

    #[test]
    fn malformed_metadata_cannot_hide_truncation_or_duplicate_qr_values() {
        let now = Instant::now();
        let qr = invitation(1, now);
        let valid = visible("Pixel", &qr.advertising_token);
        for end in 0..valid.len() {
            let parsed = parse_endpoint_metadata(&valid[..end]);
            // The ordinary, complete name without any TLV is valid, but cannot match.
            assert!(parsed.ok().and_then(|m| qr.matched_name(&m, now)).is_none());
        }
        let mut duplicate = valid.clone();
        duplicate.extend_from_slice(&[1, 16]); duplicate.extend_from_slice(&qr.advertising_token);
        assert!(parse_endpoint_metadata(&duplicate).is_err());
        let mut many = valid.clone();
        for _ in 0..MAX_ENDPOINT_TLVS { many.extend_from_slice(&[9, 0]); }
        assert!(parse_endpoint_metadata(&many).is_err());
        let mut reserved = valid; reserved[0] |= 1;
        assert!(parse_endpoint_metadata(&reserved).is_err());
        assert!(parse_endpoint_metadata(&vec![0; MAX_ENDPOINT_INFO + 1]).is_err());
    }

    #[test]
    fn proof_is_p1363_over_sha256_of_raw_auth_and_not_pin_or_another_session() {
        // Once claimed, expiry/clearing of the pending offer cannot invalidate
        // the transfer's retained signing key.
        let qr = invitation(1, Instant::now() - QR_INVITATION_TTL);
        assert!(qr.is_expired_at(Instant::now()));
        let auth = [42; 32];
        let proof = qr.sign_auth_token(&auth);
        let signature = Signature::from_slice(&proof).unwrap();
        let verifier = qr.signing_key.verifying_key();
        verifier.verify(&auth, &signature).unwrap();
        use p256::ecdsa::signature::hazmat::PrehashVerifier;
        use sha2::Digest;
        verifier.verify_prehash(&Sha256::digest(auth), &signature).unwrap();
        assert!(verifier.verify_prehash(&auth, &signature).is_err());
        assert!(verifier.verify(b"1234", &signature).is_err());
        assert!(verifier.verify(&[43; 32], &signature).is_err());
        assert!(invitation(2, Instant::now()).signing_key.verifying_key().verify(&auth, &signature).is_err());
        let mut changed = proof; changed[0] ^= 1;
        assert!(verifier.verify(&auth, &Signature::from_slice(&changed).unwrap()).is_err());
    }
}

//! UKEY2/P-256 and SecureMessage, adapted from the GPLv3 rquickshare engine.
//! Google wire formats and derivation labels are preserved; contact/account
//! authentication is intentionally not claimed (PairedKeyResult::Unable).
use crate::utils::{gen_random, hkdf_extract_expand, to_four_digit_string};
use crate::{securegcm as g, securemessage as s};
use anyhow::{anyhow, bail, ensure, Result};
use hmac::{Hmac, Mac};
use libaes::Cipher;
use p256::ecdh::diffie_hellman;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use prost::Message;
use sha2::{Digest, Sha256, Sha512};
type Hmac256 = Hmac<Sha256>;

pub struct Handshake {
    secret: SecretKey,
    client_init: Vec<u8>,
    server_init: Vec<u8>,
    finish: Vec<u8>,
    commitment: Vec<u8>,
}
pub struct Crypto {
    encrypt: [u8; 32],
    decrypt: [u8; 32],
    send_mac: [u8; 32],
    recv_mac: [u8; 32],
    send_sequence: i32,
    recv_sequence: i32,
    auth_token: [u8; 32],
    pub pin: String,
}
fn public_bytes(secret: &SecretKey) -> Result<Vec<u8>> {
    let point = secret.public_key().to_encoded_point(false);
    fn signed(v: &[u8]) -> Vec<u8> {
        let first = v.iter().position(|b| *b != 0).unwrap_or(v.len() - 1);
        let mut result = v[first..].to_vec();
        if result[0] & 0x80 != 0 {
            result.insert(0, 0);
        }
        result
    }
    Ok(s::GenericPublicKey {
        r#type: s::PublicKeyType::EcP256.into(),
        ec_p256_public_key: Some(s::EcP256PublicKey {
            x: signed(point.x().ok_or_else(|| anyhow!("missing public x"))?),
            y: signed(point.y().ok_or_else(|| anyhow!("missing public y"))?),
        }),
        ..Default::default()
    }
    .encode_to_vec())
}
fn peer_key(data: &[u8]) -> Result<PublicKey> {
    let public = s::GenericPublicKey::decode(data)?;
    ensure!(
        public.r#type == s::PublicKeyType::EcP256 as i32,
        "unexpected public key type"
    );
    let key = public
        .ec_p256_public_key
        .ok_or_else(|| anyhow!("missing P256 key"))?;
    fn coordinate(bytes: &[u8]) -> Result<[u8; 32]> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= 33,
            "invalid P256 coordinate length"
        );
        ensure!(bytes[0] & 0x80 == 0, "negative P256 coordinate");
        let bytes = if bytes.len() == 33 {
            ensure!(bytes[0] == 0, "overflow P256 coordinate");
            &bytes[1..]
        } else {
            bytes
        };
        let mut padded = [0; 32];
        padded[32 - bytes.len()..].copy_from_slice(bytes);
        Ok(padded)
    }
    let mut sec1 = [0; 65];
    sec1[0] = 4;
    sec1[1..33].copy_from_slice(&coordinate(&key.x)?);
    sec1[33..].copy_from_slice(&coordinate(&key.y)?);
    Ok(PublicKey::from_sec1_bytes(&sec1)?)
}
impl Handshake {
    pub fn client() -> Result<(Self, Vec<u8>)> {
        let secret = crate::utils::gen_ecdsa_keypair().0;
        let finish = g::Ukey2Message {
            message_type: Some(g::ukey2_message::Type::ClientFinish.into()),
            message_data: Some(
                g::Ukey2ClientFinished {
                    public_key: Some(public_bytes(&secret)?),
                }
                .encode_to_vec(),
            ),
        }
        .encode_to_vec();
        let commitment = Sha512::digest(&finish).to_vec();
        let client_init = g::Ukey2Message {
            message_type: Some(g::ukey2_message::Type::ClientInit.into()),
            message_data: Some(
                g::Ukey2ClientInit {
                    version: Some(1),
                    random: Some(gen_random(32)),
                    cipher_commitments: vec![g::ukey2_client_init::CipherCommitment {
                        handshake_cipher: Some(g::Ukey2HandshakeCipher::P256Sha512.into()),
                        commitment: Some(commitment.clone()),
                    }],
                    next_protocol: Some("AES_256_CBC-HMAC_SHA256".into()),
                }
                .encode_to_vec(),
            ),
        }
        .encode_to_vec();
        Ok((
            Self {
                secret,
                client_init: client_init.clone(),
                server_init: vec![],
                finish,
                commitment,
            },
            client_init,
        ))
    }
    pub fn server(bytes: &[u8]) -> Result<(Self, Vec<u8>)> {
        let message = g::Ukey2Message::decode(bytes)?;
        ensure!(
            message.message_type() == g::ukey2_message::Type::ClientInit,
            "expected UKEY2 client init"
        );
        let init = g::Ukey2ClientInit::decode(message.message_data())?;
        ensure!(
            init.version() == 1 && init.random().len() == 32,
            "invalid UKEY2 version/random"
        );
        ensure!(
            init.next_protocol() == "AES_256_CBC-HMAC_SHA256",
            "unsupported UKEY2 protocol"
        );
        let commitment = init
            .cipher_commitments
            .iter()
            .find(|c| c.handshake_cipher() == g::Ukey2HandshakeCipher::P256Sha512)
            .ok_or_else(|| anyhow!("no P256 commitment"))?
            .commitment()
            .to_vec();
        ensure!(commitment.len() == 64, "invalid UKEY2 commitment");
        let secret = crate::utils::gen_ecdsa_keypair().0;
        let server_init = g::Ukey2Message {
            message_type: Some(g::ukey2_message::Type::ServerInit.into()),
            message_data: Some(
                g::Ukey2ServerInit {
                    version: Some(1),
                    random: Some(gen_random(32)),
                    handshake_cipher: Some(g::Ukey2HandshakeCipher::P256Sha512.into()),
                    public_key: Some(public_bytes(&secret)?),
                }
                .encode_to_vec(),
            ),
        }
        .encode_to_vec();
        Ok((
            Self {
                secret,
                client_init: bytes.to_vec(),
                server_init: server_init.clone(),
                finish: vec![],
                commitment,
            },
            server_init,
        ))
    }
    pub fn client_finish(mut self, bytes: &[u8]) -> Result<(Crypto, Vec<u8>)> {
        let message = g::Ukey2Message::decode(bytes)?;
        ensure!(
            message.message_type() == g::ukey2_message::Type::ServerInit,
            "expected UKEY2 server init"
        );
        let init = g::Ukey2ServerInit::decode(message.message_data())?;
        ensure!(
            init.version() == 1 && init.random().len() == 32,
            "invalid UKEY2 server init"
        );
        ensure!(
            init.handshake_cipher() == g::Ukey2HandshakeCipher::P256Sha512,
            "unsupported UKEY2 cipher"
        );
        self.server_init = bytes.to_vec();
        Ok((
            self.derive(peer_key(init.public_key())?, true)?,
            self.finish,
        ))
    }
    pub fn server_finish(self, bytes: &[u8]) -> Result<Crypto> {
        // Digest equality uses constant-time verification, as does message HMAC.
        use subtle::ConstantTimeEq;
        ensure!(
            bool::from(self.commitment.as_slice().ct_eq(&Sha512::digest(bytes)[..])),
            "UKEY2 commitment mismatch"
        );
        let message = g::Ukey2Message::decode(bytes)?;
        ensure!(
            message.message_type() == g::ukey2_message::Type::ClientFinish,
            "expected UKEY2 client finish"
        );
        let finish = g::Ukey2ClientFinished::decode(message.message_data())?;
        self.derive(peer_key(finish.public_key())?, false)
    }
    fn derive(&self, peer: PublicKey, client: bool) -> Result<Crypto> {
        let shared = diffie_hellman(self.secret.to_nonzero_scalar(), peer.as_affine());
        let derived = Sha256::digest(shared.raw_secret_bytes());
        let mut transcript = Vec::with_capacity(self.client_init.len() + self.server_init.len());
        transcript.extend_from_slice(&self.client_init);
        transcript.extend_from_slice(&self.server_init);
        let auth = hkdf_extract_expand(b"UKEY2 v1 auth", &derived, &transcript, 32)?;
        let pin = to_four_digit_string(&auth);
        let auth_token = auth.try_into().map_err(|_| anyhow!("auth token length"))?;
        let next = hkdf_extract_expand(b"UKEY2 v1 next", &derived, &transcript, 32)?;
        let salt = hex::decode("82AA55A0D397F88346CA1CEE8D3909B95F13FA7DEB1D4AB38376B8256DA85510")?;
        let key_salt =
            hex::decode("BF9D2A53C63616D75DB0A7165B91C1EF73E537F2427405FA23610A4BE657642E")?;
        let c = hkdf_extract_expand(&salt, &next, b"client", 32)?;
        let s = hkdf_extract_expand(&salt, &next, b"server", 32)?;
        let (send, recv) = if client { (&c, &s) } else { (&s, &c) };
        Ok(Crypto {
            encrypt: hkdf_extract_expand(&key_salt, send, b"ENC:2", 32)?
                .try_into()
                .map_err(|_| anyhow!("key length"))?,
            decrypt: hkdf_extract_expand(&key_salt, recv, b"ENC:2", 32)?
                .try_into()
                .map_err(|_| anyhow!("key length"))?,
            send_mac: hkdf_extract_expand(&key_salt, send, b"SIG:1", 32)?
                .try_into()
                .map_err(|_| anyhow!("key length"))?,
            recv_mac: hkdf_extract_expand(&key_salt, recv, b"SIG:1", 32)?
                .try_into()
                .map_err(|_| anyhow!("key length"))?,
            send_sequence: 0,
            recv_sequence: 0,
            auth_token,
            pin,
        })
    }
}
impl Crypto {
    pub(crate) fn auth_token(&self) -> &[u8; 32] {
        &self.auth_token
    }
    pub fn seal(&mut self, bytes: Vec<u8>) -> Result<Vec<u8>> {
        self.send_sequence = self
            .send_sequence
            .checked_add(1)
            .ok_or_else(|| anyhow!("send sequence exhausted"))?;
        let message = g::DeviceToDeviceMessage {
            sequence_number: Some(self.send_sequence),
            message: Some(bytes),
        }
        .encode_to_vec();
        let iv = gen_random(16);
        let mut cipher = Cipher::new_256(&self.encrypt);
        cipher.set_auto_padding(true);
        let header_and_body = s::HeaderAndBody {
            header: s::Header {
                signature_scheme: s::SigScheme::HmacSha256.into(),
                encryption_scheme: s::EncScheme::Aes256Cbc.into(),
                iv: Some(iv.clone()),
                public_metadata: Some(
                    g::GcmMetadata {
                        r#type: g::Type::DeviceToDeviceMessage.into(),
                        version: Some(1),
                    }
                    .encode_to_vec(),
                ),
                ..Default::default()
            },
            body: cipher.cbc_encrypt(&iv, &message),
        }
        .encode_to_vec();
        let mut mac = Hmac256::new_from_slice(&self.send_mac)?;
        mac.update(&header_and_body);
        Ok(s::SecureMessage {
            header_and_body,
            signature: mac.finalize().into_bytes().to_vec(),
        }
        .encode_to_vec())
    }
    pub fn open(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        let message = s::SecureMessage::decode(bytes)?;
        let mut mac = Hmac256::new_from_slice(&self.recv_mac)?;
        mac.update(&message.header_and_body);
        mac.verify_slice(&message.signature)
            .map_err(|_| anyhow!("invalid SecureMessage authentication"))?;
        let hb = s::HeaderAndBody::decode(message.header_and_body.as_slice())?;
        ensure!(
            hb.header.encryption_scheme() == s::EncScheme::Aes256Cbc
                && hb.header.signature_scheme() == s::SigScheme::HmacSha256,
            "invalid secure schemes"
        );
        ensure!(
            hb.header.iv().len() == 16 && !hb.body.is_empty() && hb.body.len() % 16 == 0,
            "invalid CBC framing"
        );
        let metadata = g::GcmMetadata::decode(hb.header.public_metadata())?;
        ensure!(
            metadata.r#type() == g::Type::DeviceToDeviceMessage && metadata.version() == 1,
            "invalid secure metadata"
        );
        let mut cipher = Cipher::new_256(&self.decrypt);
        cipher.set_auto_padding(true);
        let plaintext = cipher.cbc_decrypt(hb.header.iv(), &hb.body);
        if plaintext.is_empty() {
            bail!("invalid CBC padding");
        }
        let d2d = g::DeviceToDeviceMessage::decode(plaintext.as_slice())?;
        let expected = self
            .recv_sequence
            .checked_add(1)
            .ok_or_else(|| anyhow!("receive sequence exhausted"))?;
        ensure!(
            d2d.sequence_number() == expected,
            "invalid/replayed encrypted sequence"
        );
        self.recv_sequence = expected;
        Ok(d2d.message().to_vec())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pair() -> (Crypto, Crypto) {
        let (c, ci) = Handshake::client().unwrap();
        let (s, si) = Handshake::server(&ci).unwrap();
        let (c, cf) = c.client_finish(&si).unwrap();
        let s = s.server_finish(&cf).unwrap();
        (c, s)
    }
    #[test]
    fn encrypted_peers_match_pin_and_reject_replay_and_mutation() {
        let (mut c, mut s) = pair();
        assert_eq!(c.pin, s.pin);
        assert_eq!(c.auth_token(), s.auth_token());
        assert_eq!(c.pin, to_four_digit_string(&c.auth_token().to_vec()));
        let bytes = c.seal(b"private".to_vec()).unwrap();
        assert_eq!(s.open(&bytes).unwrap(), b"private");
        assert!(s.open(&bytes).is_err());
        let mut bytes = c.seal(b"private again".to_vec()).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert!(s.open(&bytes).is_err());
    }
    #[test]
    fn commitment_and_invalid_curve_point_are_rejected() {
        let (c, ci) = Handshake::client().unwrap();
        let (s, si) = Handshake::server(&ci).unwrap();
        let (_, mut cf) = c.client_finish(&si).unwrap();
        let last = cf.len() - 1;
        cf[last] ^= 1;
        assert!(s.server_finish(&cf).is_err());
        assert!(peer_key(
            &s::GenericPublicKey {
                r#type: s::PublicKeyType::EcP256.into(),
                ec_p256_public_key: Some(s::EcP256PublicKey {
                    x: vec![0],
                    y: vec![0]
                }),
                ..Default::default()
            }
            .encode_to_vec()
        )
        .is_err());
    }
}

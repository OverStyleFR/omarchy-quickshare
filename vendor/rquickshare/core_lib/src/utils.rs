use anyhow::{anyhow, ensure, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hkdf::Hkdf;
use if_addrs::get_if_addrs;
use p256::elliptic_curve::rand_core::OsRng;
use p256::{PublicKey, SecretKey};
use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::net::Ipv4Addr;
use std::path::PathBuf;

pub(crate) fn get_hostname() -> Result<String> {
    let mut bytes = [0u8; 256];
    // SAFETY: this writable buffer remains alive for the full bounded syscall.
    if unsafe { libc::gethostname(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(std::ffi::CStr::from_bytes_until_nul(&bytes)?
        .to_str()?
        .to_owned())
}
#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
pub enum DeviceType {
    Unknown = 0,
    Phone = 1,
    Tablet = 2,
    Laptop = 3,
}
impl DeviceType {
    pub fn from_raw_value(value: u8) -> Self {
        match value {
            1 => Self::Phone,
            2 => Self::Tablet,
            3 => Self::Laptop,
            _ => Self::Unknown,
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RemoteDeviceInfo {
    pub name: String,
    pub device_type: DeviceType,
}
impl RemoteDeviceInfo {
    pub fn serialize(&self) -> Vec<u8> {
        let mut info = vec![(self.device_type.clone() as u8) << 1];
        info.extend_from_slice(&rand::rng().random::<[u8; 16]>());
        let mut length = self.name.len().min(255);
        while !self.name.is_char_boundary(length) {
            length -= 1;
        }
        info.push(length as u8);
        info.extend_from_slice(&self.name.as_bytes()[..length]);
        info
    }
}
pub fn gen_mdns_name(endpoint: [u8; 4]) -> String {
    let mut data = vec![0x23];
    data.extend_from_slice(&endpoint);
    data.extend_from_slice(&[0xfc, 0x9f, 0x5e, 0, 0]);
    URL_SAFE_NO_PAD.encode(data)
}
pub fn gen_mdns_endpoint_info(device_type: u8, device_name: &str) -> String {
    URL_SAFE_NO_PAD.encode(
        RemoteDeviceInfo {
            name: device_name.into(),
            device_type: DeviceType::from_raw_value(device_type),
        }
        .serialize(),
    )
}
pub fn parse_mdns_endpoint_info(encoded: &str) -> Result<(DeviceType, String)> {
    let info = URL_SAFE_NO_PAD.decode(encoded)?;
    ensure!(info.len() >= 18, "short endpoint info");
    let length = info[17] as usize;
    ensure!(
        length > 0 && info.len() >= 18 + length,
        "invalid endpoint name length"
    );
    Ok((
        DeviceType::from_raw_value((info[0] >> 1) & 7),
        std::str::from_utf8(&info[18..18 + length])?.to_owned(),
    ))
}
pub fn gen_ecdsa_keypair() -> (SecretKey, PublicKey) {
    let secret = SecretKey::random(&mut OsRng);
    let public = secret.public_key();
    (secret, public)
}
pub fn hkdf_extract_expand(
    salt: &[u8],
    input: &[u8],
    info: &[u8],
    length: usize,
) -> Result<Vec<u8>> {
    let hkdf = Hkdf::<Sha256>::new(Some(salt), input);
    let mut output = vec![0; length];
    hkdf.expand(info, &mut output)
        .map_err(|e| anyhow!("HKDF expand: {e}"))?;
    Ok(output)
}
pub fn to_four_digit_string(bytes: &Vec<u8>) -> String {
    let mut hash = 0i32;
    let mut multiplier = 1;
    for byte in bytes {
        hash = (hash + (*byte as i8 as i32) * multiplier) % 9973;
        multiplier = multiplier * 31 % 9973;
    }
    format!("{:04}", hash.abs())
}
pub fn gen_random(size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    rand::rng().fill_bytes(&mut bytes);
    bytes
}
pub fn get_download_dir() -> PathBuf {
    if let Some(dirs) = directories::UserDirs::new() {
        return dirs
            .download_dir()
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs.home_dir().join("Downloads"));
    }
    std::env::current_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("Downloads")
}
pub fn is_not_self_ip(ip: &Ipv4Addr) -> bool {
    !get_if_addrs()
        .map(|interfaces| {
            interfaces
                .into_iter()
                .any(|interface| interface.ip() == *ip)
        })
        .unwrap_or(false)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_unicode_roundtrip_and_truncation_preserve_utf8() {
        for name in ["a_device", "equipo ñ", &"ñ".repeat(150)] {
            let encoded = gen_mdns_endpoint_info(3, name);
            let (kind, decoded) = parse_mdns_endpoint_info(&encoded).unwrap();
            assert_eq!(kind, DeviceType::Laptop);
            assert!(name.starts_with(&decoded));
            assert!(decoded.len() <= 255);
        }
    }
}

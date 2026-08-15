use std::{collections::HashMap, num::NonZeroU32};

use ring::{
    aead, hkdf, pbkdf2,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::{error::CommandError, models::VaultEnvelope};

pub(super) const FORMAT_VERSION: i64 = 1;
pub(super) const PAYLOAD_VERSION: u32 = 1;
pub(super) const MODE_DEFAULT: &str = "default";
pub(super) const MODE_PASSWORD: &str = "password";
pub(super) const KEY_LENGTH: usize = 32;
pub(super) const PASSWORD_ITERATIONS: u32 = 600_000;

const SALT_LENGTH: usize = 16;
const NONCE_LENGTH: usize = 12;
const MAX_CREDENTIAL_BYTES: usize = 64 * 1024;
const MAX_PLAINTEXT_BYTES: usize = 1024 * 1024 - 16;
const DEFAULT_KEY_INFO: &[u8] = b"QuailCard credential vault default v1";
const DEFAULT_KEY_MATERIAL: [u8; KEY_LENGTH] = [
    0x2a, 0x79, 0x5e, 0xb1, 0x47, 0x8c, 0xe3, 0x16, 0xa4, 0xd9, 0x03, 0x6f, 0x91, 0xc8, 0x55, 0x2d,
    0x7b, 0xe0, 0x34, 0x89, 0xfa, 0x61, 0x0c, 0xd7, 0x43, 0x9e, 0xb8, 0x25, 0x70, 0x1f, 0xcc, 0x5a,
];

/// 保险库解密后的版本化凭据映射。
#[derive(Default, Deserialize, Serialize)]
pub(super) struct VaultPayload {
    schema_version: u32,
    pub(super) credentials: HashMap<String, String>,
}

impl Drop for VaultPayload {
    /// 尽力清除保险库明文映射占用的内存。
    fn drop(&mut self) {
        for (mut secret_ref, mut credential) in std::mem::take(&mut self.credentials) {
            secret_ref.zeroize();
            credential.zeroize();
        }
    }
}

impl VaultPayload {
    /// 创建不包含任何供应商凭据的版本化载荷。
    pub(super) fn empty() -> Self {
        Self {
            schema_version: PAYLOAD_VERSION,
            credentials: HashMap::new(),
        }
    }
}

/// 使用随机安装盐创建默认保护的密文记录。
pub(super) fn encrypt_new_default(payload: &VaultPayload) -> Result<VaultEnvelope, CommandError> {
    let salt = random_bytes::<SALT_LENGTH>()?;
    let key = derive_default_key(&salt)?;
    encrypt_payload(payload, &key, MODE_DEFAULT, salt.to_vec(), None)
}

/// 使用新盐和用户密码创建密码保护的密文记录及会话密钥。
pub(super) fn encrypt_new_password(
    payload: &VaultPayload,
    password: &str,
) -> Result<(VaultEnvelope, Zeroizing<[u8; KEY_LENGTH]>), CommandError> {
    let salt = random_bytes::<SALT_LENGTH>()?;
    let key = derive_password_key_from_parts(&salt, PASSWORD_ITERATIONS, password)?;
    let envelope = encrypt_payload(
        payload,
        &key,
        MODE_PASSWORD,
        salt.to_vec(),
        Some(i64::from(PASSWORD_ITERATIONS)),
    )?;
    Ok((envelope, key))
}

/// 使用 AES-256-GCM 将凭据映射封装为认证密文。
pub(super) fn encrypt_payload(
    payload: &VaultPayload,
    key_bytes: &[u8; KEY_LENGTH],
    protection_mode: &str,
    kdf_salt: Vec<u8>,
    kdf_iterations: Option<i64>,
) -> Result<VaultEnvelope, CommandError> {
    let mut plaintext = serde_json::to_vec(payload)
        .map_err(|_| CommandError::new("VAULT_SERIALIZATION_ERROR", "无法序列化凭据保险库"))?;
    if plaintext.len() > MAX_PLAINTEXT_BYTES {
        plaintext.zeroize();
        return Err(CommandError::new(
            "VAULT_TOO_LARGE",
            "凭据保险库超过 1 MiB 限制",
        ));
    }
    let nonce_bytes = random_bytes::<NONCE_LENGTH>()?;
    let key = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, key_bytes)
            .map_err(|_| crypto_error("无法创建保险库加密密钥"))?,
    );
    let aad = envelope_aad(protection_mode, &kdf_salt, kdf_iterations);
    key.seal_in_place_append_tag(
        aead::Nonce::assume_unique_for_key(nonce_bytes),
        aead::Aad::from(aad.as_slice()),
        &mut plaintext,
    )
    .map_err(|_| crypto_error("无法加密凭据保险库"))?;
    Ok(VaultEnvelope {
        format_version: FORMAT_VERSION,
        protection_mode: protection_mode.to_string(),
        kdf_salt,
        kdf_iterations,
        nonce: nonce_bytes.to_vec(),
        ciphertext: plaintext,
    })
}

/// 使用密文记录元数据和密钥验证并解析保险库载荷。
pub(super) fn decrypt_payload_with_key(
    envelope: &VaultEnvelope,
    key_bytes: &[u8; KEY_LENGTH],
) -> Result<VaultPayload, CommandError> {
    validate_envelope(envelope)?;
    let nonce: [u8; NONCE_LENGTH] = envelope
        .nonce
        .as_slice()
        .try_into()
        .map_err(|_| invalid_envelope())?;
    let key = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, key_bytes)
            .map_err(|_| crypto_error("无法创建保险库解密密钥"))?,
    );
    let aad = envelope_aad(
        &envelope.protection_mode,
        &envelope.kdf_salt,
        envelope.kdf_iterations,
    );
    let mut ciphertext = envelope.ciphertext.clone();
    let plaintext = key
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(aad.as_slice()),
            &mut ciphertext,
        )
        .map_err(|_| crypto_error("保险库认证失败，密钥错误或数据已损坏"))?;
    let result = serde_json::from_slice::<VaultPayload>(plaintext)
        .map_err(|_| invalid_envelope())
        .and_then(|payload| {
            if payload.schema_version != PAYLOAD_VERSION {
                return Err(invalid_envelope());
            }
            Ok(payload)
        });
    ciphertext.zeroize();
    result
}

/// 使用 HKDF-SHA256 从固定应用材料和安装盐派生默认密钥。
pub(super) fn derive_default_key(
    salt_bytes: &[u8],
) -> Result<Zeroizing<[u8; KEY_LENGTH]>, CommandError> {
    if salt_bytes.len() != SALT_LENGTH {
        return Err(invalid_envelope());
    }
    let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, salt_bytes);
    let pseudo_random_key = salt.extract(&DEFAULT_KEY_MATERIAL);
    let info = [DEFAULT_KEY_INFO];
    let output = pseudo_random_key
        .expand(&info, &aead::AES_256_GCM)
        .map_err(|_| crypto_error("无法派生默认保险库密钥"))?;
    let mut key = Zeroizing::new([0_u8; KEY_LENGTH]);
    output
        .fill(key.as_mut())
        .map_err(|_| crypto_error("无法生成默认保险库密钥"))?;
    Ok(key)
}

/// 使用密文记录保存的 PBKDF2 参数派生用户密码密钥。
pub(super) fn derive_password_key(
    envelope: &VaultEnvelope,
    password: &str,
) -> Result<Zeroizing<[u8; KEY_LENGTH]>, CommandError> {
    validate_envelope(envelope)?;
    let iterations = envelope
        .kdf_iterations
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(invalid_envelope)?;
    derive_password_key_from_parts(&envelope.kdf_salt, iterations, password)
}

/// 使用 PBKDF2-HMAC-SHA256、随机盐和指定迭代次数派生 256 位密钥。
fn derive_password_key_from_parts(
    salt: &[u8],
    iterations: u32,
    password: &str,
) -> Result<Zeroizing<[u8; KEY_LENGTH]>, CommandError> {
    if salt.len() != SALT_LENGTH || !(100_000..=2_000_000).contains(&iterations) {
        return Err(invalid_envelope());
    }
    let iterations = NonZeroU32::new(iterations).ok_or_else(invalid_envelope)?;
    let mut key = Zeroizing::new([0_u8; KEY_LENGTH]);
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        salt,
        password.as_bytes(),
        key.as_mut(),
    );
    Ok(key)
}

/// 生成指定长度的密码学安全随机字节。
fn random_bytes<const LENGTH: usize>() -> Result<[u8; LENGTH], CommandError> {
    let mut bytes = [0_u8; LENGTH];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| crypto_error("无法生成保险库随机数"))?;
    Ok(bytes)
}

/// 构造绑定版本、模式、盐和 KDF 参数的附加认证数据。
fn envelope_aad(mode: &str, salt: &[u8], iterations: Option<i64>) -> Vec<u8> {
    let mut aad = format!(
        "QuailCard|credential-vault|{FORMAT_VERSION}|{mode}|{}|",
        iterations.unwrap_or(0)
    )
    .into_bytes();
    aad.extend_from_slice(salt);
    aad
}

/// 校验来自 SQLite 的保险库密文记录边界和模式参数。
pub(super) fn validate_envelope(envelope: &VaultEnvelope) -> Result<(), CommandError> {
    let valid_mode = match envelope.protection_mode.as_str() {
        MODE_DEFAULT => envelope.kdf_iterations.is_none(),
        MODE_PASSWORD => envelope
            .kdf_iterations
            .is_some_and(|value| (100_000..=2_000_000).contains(&value)),
        _ => false,
    };
    if envelope.format_version != FORMAT_VERSION
        || !valid_mode
        || envelope.kdf_salt.len() != SALT_LENGTH
        || envelope.nonce.len() != NONCE_LENGTH
        || !(16..=1024 * 1024).contains(&envelope.ciphertext.len())
    {
        return Err(invalid_envelope());
    }
    Ok(())
}

/// 校验写入保险库的随机引用和凭据大小。
pub(super) fn validate_credential(secret_ref: &str, credential: &str) -> Result<(), CommandError> {
    if secret_ref.trim().is_empty() || secret_ref.len() > 200 {
        return Err(CommandError::validation("凭据引用无效"));
    }
    if credential.is_empty() || credential.len() > MAX_CREDENTIAL_BYTES {
        return Err(CommandError::validation(
            "供应商凭据长度必须为 1-65,536 字节",
        ));
    }
    Ok(())
}

/// 校验用户保险库密码的最小长度和输入上限。
pub(super) fn validate_password(password: &str) -> Result<(), CommandError> {
    if password.chars().count() < 8 || password.len() > 1024 {
        return Err(CommandError::validation(
            "保险库密码长度必须为 8-1,024 个字节",
        ));
    }
    Ok(())
}

/// 创建固定错误码的保险库加密错误。
fn crypto_error(message: &str) -> CommandError {
    CommandError::new("VAULT_CRYPTO_ERROR", message)
}

/// 创建固定错误码的保险库密文格式错误。
pub(super) fn invalid_envelope() -> CommandError {
    CommandError::new("VAULT_DATA_INVALID", "凭据保险库格式无效或已损坏")
}

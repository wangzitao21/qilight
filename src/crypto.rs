use crate::{message, Result};
use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use md5::{Digest, Md5};
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

pub struct Codec {
    token: [u8; 16],
    key: [u8; 16],
    iv: [u8; 16],
}
impl Codec {
    pub fn new(token: [u8; 16]) -> Self {
        let key: [u8; 16] = Md5::digest(token).into();
        let mut hash = Md5::new();
        hash.update(key);
        hash.update(token);
        Self {
            token,
            key,
            iv: hash.finalize().into(),
        }
    }
    pub fn encrypt(&self, bytes: &[u8]) -> Vec<u8> {
        cbc::Encryptor::<aes::Aes128>::new(&self.key.into(), &self.iv.into())
            .encrypt_padded_vec_mut::<Pkcs7>(bytes)
    }
    pub fn decrypt(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        cbc::Decryptor::<aes::Aes128>::new(&self.key.into(), &self.iv.into())
            .decrypt_padded_vec_mut::<Pkcs7>(bytes)
            .map_err(|_| message("设备回复解密失败，请重新扫码。"))
    }
    pub fn checksum(&self, header: &[u8], encrypted: &[u8]) -> [u8; 16] {
        let mut hash = Md5::new();
        hash.update(header);
        hash.update(self.token);
        hash.update(encrypted);
        hash.finalize().into()
    }
    pub fn verify(&self, header: &[u8], encrypted: &[u8], received: &[u8]) -> bool {
        bool::from(self.checksum(header, encrypted).as_slice().ct_eq(received))
    }
}
impl Drop for Codec {
    fn drop(&mut self) {
        self.token.zeroize();
        self.key.zeroize();
        self.iv.zeroize();
    }
}

// Xiaomi's cloud protocol requires RC4-drop1024. This is never used for local storage.
pub fn rc4(data: &[u8], key: &[u8], drop: usize) -> Result<Vec<u8>> {
    if key.is_empty() {
        return Err(message("云端会话密钥为空。"));
    }
    let mut s = [0u8; 256];
    for (i, v) in s.iter_mut().enumerate() {
        *v = i as u8;
    }
    let mut j = 0usize;
    for i in 0..256 {
        j = (j + s[i] as usize + key[i % key.len()] as usize) & 255;
        s.swap(i, j);
    }
    let (mut i, mut j) = (0usize, 0usize);
    let mut out = Vec::with_capacity(data.len());
    for position in 0..drop + data.len() {
        i = (i + 1) & 255;
        j = (j + s[i] as usize) & 255;
        s.swap(i, j);
        if position >= drop {
            out.push(data[position - drop] ^ s[(s[i] as usize + s[j] as usize) & 255]);
        }
    }
    s.zeroize();
    Ok(out)
}

#[cfg(windows)]
pub fn protect(data: &[u8], decrypt: bool) -> Result<Vec<u8>> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: data
            .len()
            .try_into()
            .map_err(|_| message("配置文件过大。"))?,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // DPAPI output is allocated by Windows and is always released using LocalFree.
    unsafe {
        let ok = if decrypt {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(message(
                "设备配置无法解密；请在原 Windows 账户运行，或备份后重新扫码。",
            ));
        }
        let bytes = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
        let result = bytes.to_vec();
        bytes.zeroize();
        LocalFree(output.pbData as _);
        Ok(result)
    }
}

#[cfg(not(windows))]
pub fn protect(_: &[u8], _: bool) -> Result<Vec<u8>> {
    Err(message("本程序需要 Windows 的本机密钥保护服务。"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rfc6229_rc4_vector() {
        assert_eq!(
            hex::encode(rc4(&[0; 16], &[1, 2, 3, 4, 5], 0).unwrap()),
            "b2396305f03dc027ccc3524a0a1118a8"
        );
        assert_eq!(
            hex::encode(rc4(&[0; 16], &[1, 2, 3, 4, 5], 1024).unwrap()),
            "30abbcc7c20b01609f23ee2d5f6bb7df"
        );
    }
    #[test]
    fn authenticated_codec_round_trip() {
        let codec = Codec::new([5; 16]);
        let text = b"example payload";
        assert_eq!(codec.decrypt(&codec.encrypt(text)).unwrap(), text);
        assert!(codec.decrypt(&[1; 15]).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn dpapi_round_trip() {
        let encrypted = protect(b"private token", false).unwrap();
        assert_ne!(encrypted, b"private token");
        assert_eq!(protect(&encrypted, true).unwrap(), b"private token");
    }
}

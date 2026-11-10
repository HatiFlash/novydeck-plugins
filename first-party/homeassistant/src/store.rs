//! Stockage local : jeton chiffré DPAPI (`secrets.bin`, inchangé depuis 1.0) et
//! connexion globale du plugin (`config.json` : URL + option TLS, sans secret).
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

fn data_dir() -> Result<PathBuf, &'static str> {
    let root = std::env::var_os("APPDATA").ok_or("APPDATA indisponible")?;
    Ok(PathBuf::from(root).join("NovyDeck/plugins/com.novydeck.homeassistant"))
}
fn write_atomic(name: &str, bytes: &[u8]) -> Result<(), &'static str> {
    let path = data_dir()?.join(name);
    fs::create_dir_all(path.parent().ok_or("Dossier invalide")?)
        .map_err(|_| "Stockage impossible")?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|_| "Stockage impossible")?;
    fs::rename(&tmp, &path).map_err(|_| "Stockage impossible")
}

#[cfg(windows)]
fn protect(data: &[u8], encrypt: bool) -> Result<Vec<u8>, &'static str> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    let mut input = data.to_vec();
    if input.len() > 16_384 {
        return Err("Jeton trop long");
    }
    let blob = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &blob,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &blob,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 {
        return Err("Chiffrement Windows indisponible");
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(result)
}
/// Hors Windows (tests Linux) : aucun chiffrement disponible, rien n'est jamais écrit en clair.
#[cfg(not(windows))]
fn protect(_data: &[u8], _encrypt: bool) -> Result<Vec<u8>, &'static str> {
    Err("Chiffrement Windows indisponible")
}

pub fn load_token() -> Result<Option<String>, &'static str> {
    let bytes = match fs::read(data_dir()?.join("secrets.bin")) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Lecture du jeton impossible"),
    };
    if bytes.is_empty() || bytes.len() > 16_384 {
        return Err("Jeton chiffré invalide");
    }
    let plain = protect(&bytes, false)?;
    String::from_utf8(plain)
        .map(Some)
        .map_err(|_| "Jeton invalide")
}
pub fn store_token(token: &str) -> Result<(), &'static str> {
    write_atomic("secrets.bin", &protect(token.as_bytes(), true)?)
}

/// Connexion globale : jamais de jeton ici.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub url: String,
    pub insecure: bool,
}
impl Config {
    pub fn from_value(v: &Value) -> Self {
        Self {
            url: v["url"]
                .as_str()
                .unwrap_or("")
                .trim()
                .chars()
                .take(512)
                .collect(),
            insecure: v["insecure"].as_bool().unwrap_or(false),
        }
    }
    pub fn configured(&self) -> bool {
        !self.url.is_empty()
    }
}
pub fn load_config() -> Config {
    data_dir()
        .ok()
        .and_then(|d| fs::read(d.join("config.json")).ok())
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .map(|v| Config::from_value(&v))
        .unwrap_or_default()
}
pub fn store_config(config: &Config) -> Result<(), &'static str> {
    write_atomic(
        "config.json",
        json!({"url": config.url, "insecure": config.insecure})
            .to_string()
            .as_bytes(),
    )
}

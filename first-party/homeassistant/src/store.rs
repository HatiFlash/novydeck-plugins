//! Stockage local : jeton chiffré DPAPI (`secrets.bin`, inchangé depuis 1.0) et
//! connexion globale du plugin (`config.json` : URL + option TLS, sans secret).
use reqwest::Url;
use serde_json::{json, Value};
use std::{fs, net::IpAddr, path::PathBuf};

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

// ------------------------------------------------------------ jeton lié à une origine

pub const MISMATCH: &str = "Jeton lié à une autre adresse, ressaisissez-le";
pub const INSECURE_REFUSED: &str = "Certificat non vérifié refusé : réservé aux adresses locales";

/// Origine normalisée `schéma://hôte:port` (`ws`/`wss` ≡ `http`/`https`).
pub fn origin_of(url: &str) -> Result<String, &'static str> {
    let url = Url::parse(url.trim()).map_err(|_| "URL invalide")?;
    let scheme = match url.scheme() {
        "http" | "ws" => "http",
        "https" | "wss" => "https",
        _ => return Err("URL invalide"),
    };
    let host = url.host_str().ok_or("URL invalide")?;
    let port = url.port_or_known_default().ok_or("URL invalide")?;
    Ok(format!("{scheme}://{host}:{port}"))
}

/// Hôte du réseau local : boucle locale, privé, lien local, `.local`/`.lan`/`.home.arpa`, nom sans point.
pub fn is_local_host(host: &str) -> bool {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(a) => local_v4(a),
            IpAddr::V6(a) => match a.to_ipv4_mapped() {
                Some(v4) => local_v4(v4),
                None => {
                    let first = a.segments()[0];
                    a.is_loopback() || first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80
                }
            },
        };
    }
    !host.contains('.')
        || host.ends_with(".local")
        || host.ends_with(".lan")
        || host.ends_with(".home.arpa")
}
fn local_v4(a: std::net::Ipv4Addr) -> bool {
    a.is_loopback() || a.is_private() || a.is_link_local()
}
/// Hôte de l'URL (vide si illisible) : `false` si l'URL est invalide.
pub fn url_is_local(url: &str) -> bool {
    Url::parse(url.trim())
        .ok()
        .and_then(|u| u.host_str().map(is_local_host))
        .unwrap_or(false)
}
/// Texte clair (`http`/`ws`) ?
pub fn is_cleartext(url: &str) -> bool {
    Url::parse(url.trim()).is_ok_and(|u| matches!(u.scheme(), "http" | "ws"))
}

/// Jeton et origine pour laquelle il a été saisi (`None` : secrets.bin 1.1.0, jeton seul).
#[derive(Clone, PartialEq, Eq)]
pub struct Secret {
    pub token: String,
    pub origin: Option<String>,
}
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secret")
            .field("token", &"***")
            .field("origin", &self.origin)
            .finish()
    }
}
impl Secret {
    pub fn encode(&self) -> Vec<u8> {
        match &self.origin {
            Some(origin) => json!({"origin": origin, "token": self.token})
                .to_string()
                .into_bytes(),
            None => self.token.clone().into_bytes(),
        }
    }
    /// Accepte `{origin, token}` ou l'ancien format (jeton brut, jamais un objet JSON).
    pub fn decode(plain: &[u8]) -> Result<Self, &'static str> {
        let text = std::str::from_utf8(plain).map_err(|_| "Jeton invalide")?;
        if text.trim_start().starts_with('{') {
            let v: Value = serde_json::from_str(text).map_err(|_| "Jeton invalide")?;
            let token = v["token"].as_str().ok_or("Jeton invalide")?;
            let origin = v["origin"].as_str().ok_or("Jeton invalide")?;
            return Ok(Self {
                token: token.to_owned(),
                origin: Some(origin.to_owned()),
            });
        }
        Ok(Self {
            token: text.to_owned(),
            origin: None,
        })
    }
    /// Jeton utilisable pour `url` uniquement si l'origine est celle liée.
    pub fn token_for(&self, url: &str) -> Result<&str, &'static str> {
        let target = origin_of(url)?;
        match &self.origin {
            Some(o) if *o == target => Ok(&self.token),
            _ => Err(MISMATCH),
        }
    }
}

/// Ce que `saveConnection` doit faire du secret stocké.
#[derive(Debug, PartialEq, Eq)]
pub enum SecretPlan {
    Keep,
    Replace(Secret),
    Clear,
}
/// `new_origin` : origine de la connexion enregistrée (`None` = adresse vidée).
pub fn plan_secret(
    current: Option<&Secret>,
    new_origin: Option<&str>,
    provided: Option<&str>,
) -> SecretPlan {
    let Some(origin) = new_origin else {
        return SecretPlan::Keep;
    };
    if let Some(token) = provided {
        return SecretPlan::Replace(Secret {
            token: token.to_owned(),
            origin: Some(origin.to_owned()),
        });
    }
    match current {
        None => SecretPlan::Keep,
        Some(s) if s.origin.is_none() => SecretPlan::Replace(Secret {
            token: s.token.clone(),
            origin: Some(origin.to_owned()),
        }),
        Some(s) if s.origin.as_deref() == Some(origin) => SecretPlan::Keep,
        Some(_) => SecretPlan::Clear,
    }
}
/// Identifiants pour « Tester » : le jeton enregistré seulement si l'origine candidate est
/// celle liée ; sinon le jeton fourni dans le message (jamais enregistré).
pub fn test_credentials(
    candidate_url: &str,
    insecure: bool,
    stored: Option<&Secret>,
    provided: Option<&str>,
) -> Result<Secret, &'static str> {
    let origin = origin_of(candidate_url)?;
    if insecure && !url_is_local(candidate_url) {
        return Err(INSECURE_REFUSED);
    }
    if let Some(token) = provided {
        return Ok(Secret {
            token: token.to_owned(),
            origin: Some(origin),
        });
    }
    match stored {
        Some(s) if s.origin.as_deref() == Some(origin.as_str()) => Ok(s.clone()),
        Some(_) => Err("Saisissez le jeton pour tester cette adresse"),
        None => Err("Non connecté"),
    }
}

/// Stockage du secret (DPAPI en production, mémoire dans les tests).
pub trait Vault: Send + Sync {
    fn load(&self) -> Result<Option<Secret>, &'static str>;
    fn save(&self, secret: &Secret) -> Result<(), &'static str>;
    fn clear(&self) -> Result<(), &'static str>;
}
pub struct DpapiVault;
impl Vault for DpapiVault {
    fn load(&self) -> Result<Option<Secret>, &'static str> {
        let bytes = match fs::read(data_dir()?.join("secrets.bin")) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("Lecture du jeton impossible"),
        };
        if bytes.is_empty() || bytes.len() > 16_384 {
            return Err("Jeton chiffré invalide");
        }
        Secret::decode(&protect(&bytes, false)?).map(Some)
    }
    fn save(&self, secret: &Secret) -> Result<(), &'static str> {
        write_atomic("secrets.bin", &protect(&secret.encode(), true)?)
    }
    fn clear(&self) -> Result<(), &'static str> {
        match fs::remove_file(data_dir()?.join("secrets.bin")) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("Stockage impossible"),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_hosts() {
        for h in [
            "localhost",
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.254",
            "192.168.1.20",
            "169.254.3.4",
            "::1",
            "[::1]",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "febf::1",
            "ha.local",
            "nas.LAN",
            "ha.home.arpa",
            "homeassistant",
            "::ffff:192.168.0.2",
        ] {
            assert!(is_local_host(h), "{h}");
        }
        for h in [
            "8.8.8.8",
            "172.32.0.1",
            "172.15.0.1",
            "11.0.0.1",
            "192.169.0.1",
            "2001:db8::1",
            "fec0::1",
            "ha.example.org",
            "local",
            "evil.local.example.com",
            "",
            "100.64.0.1",
        ] {
            // « local » seul est un nom sans point : local ; les autres non.
            if h == "local" {
                assert!(is_local_host(h));
            } else {
                assert!(!is_local_host(h), "{h}");
            }
        }
        assert!(url_is_local("http://192.168.1.2:8123"));
        assert!(!url_is_local("https://ha.example.org"));
        assert!(is_cleartext("ws://x") && !is_cleartext("https://x"));
    }
    #[test]
    fn origins_are_normalized() {
        assert_eq!(
            origin_of("HTTP://Ha.Local:80/").unwrap(),
            "http://ha.local:80"
        );
        assert_eq!(origin_of("http://ha.local").unwrap(), "http://ha.local:80");
        assert_eq!(origin_of("ws://ha.local").unwrap(), "http://ha.local:80");
        assert_eq!(origin_of("wss://ha:8123").unwrap(), "https://ha:8123");
        assert_ne!(origin_of("http://ha:8123"), origin_of("https://ha:8123"));
        assert_ne!(origin_of("http://ha:8123"), origin_of("http://ha:8124"));
        assert!(origin_of("ftp://x").is_err());
    }
    #[test]
    fn secret_format_is_backward_compatible() {
        let legacy = Secret::decode(b"abc.def").unwrap();
        assert_eq!(
            (legacy.token.as_str(), legacy.origin.clone()),
            ("abc.def", None)
        );
        assert_eq!(legacy.token_for("http://ha:8123"), Err(MISMATCH));
        let bound = Secret {
            token: "t".into(),
            origin: Some("http://ha:8123".into()),
        };
        assert_eq!(Secret::decode(&bound.encode()).unwrap(), bound);
        assert_eq!(bound.token_for("http://HA:8123/"), Ok("t"));
        assert_eq!(bound.token_for("http://evil:8123"), Err(MISMATCH));
        assert!(!format!("{bound:?}").contains("\"t\""));
    }
    #[test]
    fn plan_for_save_connection() {
        let s = Secret {
            token: "t".into(),
            origin: Some("http://a:1".into()),
        };
        assert_eq!(
            plan_secret(Some(&s), Some("http://a:1"), None),
            SecretPlan::Keep
        );
        assert_eq!(
            plan_secret(Some(&s), Some("http://b:1"), None),
            SecretPlan::Clear
        );
        assert_eq!(
            plan_secret(Some(&s), Some("http://b:1"), Some("n")),
            SecretPlan::Replace(Secret {
                token: "n".into(),
                origin: Some("http://b:1".into())
            })
        );
        assert_eq!(plan_secret(Some(&s), None, None), SecretPlan::Keep);
        let legacy = Secret {
            token: "t".into(),
            origin: None,
        };
        assert_eq!(
            plan_secret(Some(&legacy), Some("http://a:1"), None),
            SecretPlan::Replace(s.clone())
        );
        assert_eq!(
            plan_secret(None, Some("http://a:1"), None),
            SecretPlan::Keep
        );
    }
    #[test]
    fn test_uses_stored_token_only_for_the_bound_origin() {
        let s = Secret {
            token: "t".into(),
            origin: Some("http://192.168.1.2:8123".into()),
        };
        let same = test_credentials("http://192.168.1.2:8123", false, Some(&s), None).unwrap();
        assert_eq!(same.token, "t");
        assert_eq!(
            test_credentials("http://192.168.1.9:8123", false, Some(&s), None),
            Err("Saisissez le jeton pour tester cette adresse")
        );
        let given =
            test_credentials("http://192.168.1.9:8123", false, Some(&s), Some("x")).unwrap();
        assert_eq!(given.token, "x");
        assert_eq!(given.origin.as_deref(), Some("http://192.168.1.9:8123"));
        assert_eq!(
            test_credentials("https://ha.example.org", true, Some(&s), Some("x")),
            Err(INSECURE_REFUSED)
        );
    }
}

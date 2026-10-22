use std::{collections::HashMap, fs, path::PathBuf, sync::mpsc::{self, Receiver, Sender}, thread, time::{Duration, Instant}};
use novydeck_sdk::{Client, Event, Plugin};
use reqwest::{blocking::Client as Http, redirect::Policy, Url};
use serde_json::{json, Value};
use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN}};

const TOGGLE: &str = "com.novydeck.homeassistant.toggle";
const SERVICE: &str = "com.novydeck.homeassistant.service";
const SENSOR: &str = "com.novydeck.homeassistant.state";

fn identifier(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
fn entity(s: &str) -> bool {
    s.split_once('.').is_some_and(|(domain, id)| identifier(domain) && identifier(id))
}
fn endpoint(base: &str, path: &str) -> Result<Url, &'static str> {
    let mut url = Url::parse(base.trim()).map_err(|_| "URL invalide")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !url.username().is_empty()
        || url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
        return Err("URL invalide");
    }
    if url.path() != "/" { return Err("URL : utiliser la racine du serveur"); }
    url.set_path(path);
    Ok(url)
}
fn refresh_interval(settings: &Value) -> Duration {
    Duration::from_secs(settings["refresh"].as_u64().unwrap_or(10).clamp(2, 300))
}
fn safe_settings(settings: &Value) -> Value {
    json!({"url": settings["url"].as_str().unwrap_or(""),
        "entity": settings["entity"].as_str().unwrap_or(""),
        "domain": settings["domain"].as_str().unwrap_or(""),
        "service": settings["service"].as_str().unwrap_or(""),
        "data": settings["data"].as_str().unwrap_or(""),
        "refresh": refresh_interval(settings).as_secs(),
        "insecure": settings["insecure"].as_bool().unwrap_or(false)})
}
fn service_body(settings: &Value) -> Result<Value, &'static str> {
    let id = settings["entity"].as_str().unwrap_or("");
    if !entity(id) { return Err("Entité invalide"); }
    let text = settings["data"].as_str().unwrap_or("").trim();
    let mut body: Value = if text.is_empty() { json!({}) } else { serde_json::from_str(text).map_err(|_| "JSON invalide")? };
    let obj = body.as_object_mut().ok_or("JSON objet requis")?;
    // An entity chosen in the inspector always takes precedence over advanced data.
    obj.insert("entity_id".into(), json!(id));
    Ok(body)
}
fn state_label(value: &Value) -> String {
    let state = value["state"].as_str().unwrap_or("—");
    let unit = value["attributes"]["unit_of_measurement"].as_str().unwrap_or("");
    format!("{}{}{}", state.chars().take(16).collect::<String>(), if unit.is_empty() { "" } else { " " }, unit.chars().take(8).collect::<String>())
}
fn secret_path() -> Result<PathBuf, &'static str> {
    let root = std::env::var_os("APPDATA").ok_or("APPDATA indisponible")?;
    Ok(PathBuf::from(root).join("NovyDeck/plugins/com.novydeck.homeassistant/secrets.bin"))
}
fn protect(data: &[u8], encrypt: bool) -> Result<Vec<u8>, &'static str> {
    let mut input = data.to_vec();
    if input.len() > 16_384 { return Err("Jeton trop long"); }
    let blob = CRYPT_INTEGER_BLOB { cbData: input.len() as u32, pbData: input.as_mut_ptr() };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        if encrypt { CryptProtectData(&blob, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) }
        else { CryptUnprotectData(&blob, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) }
    };
    if ok == 0 { return Err("Chiffrement Windows indisponible"); }
    let result = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe { LocalFree(output.pbData.cast()); }
    Ok(result)
}
fn load_token() -> Result<Option<String>, &'static str> {
    let bytes = match fs::read(secret_path()?) { Ok(v) => v, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None), Err(_) => return Err("Lecture du jeton impossible") };
    if bytes.is_empty() || bytes.len() > 16_384 { return Err("Jeton chiffré invalide"); }
    let plain = protect(&bytes, false)?;
    String::from_utf8(plain).map(Some).map_err(|_| "Jeton invalide")
}
fn store_token(token: &str) -> Result<(), &'static str> {
    let path = secret_path()?;
    fs::create_dir_all(path.parent().ok_or("Dossier invalide")?).map_err(|_| "Stockage impossible")?;
    let bytes = protect(token.as_bytes(), true)?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|_| "Stockage impossible")?;
    fs::rename(&tmp, &path).map_err(|_| "Stockage impossible")
}
fn http(settings: &Value) -> Result<Http, &'static str> {
    Http::builder().timeout(Duration::from_secs(10)).connect_timeout(Duration::from_secs(5))
        .redirect(Policy::none()).danger_accept_invalid_certs(settings["insecure"].as_bool().unwrap_or(false))
        .build().map_err(|_| "Client HTTP indisponible")
}
fn request(settings: &Value, token: &str, method: &str, path: &str, body: Option<Value>) -> Result<Value, &'static str> {
    let url = endpoint(settings["url"].as_str().unwrap_or(""), path)?;
    let client = http(settings)?;
    let req = if method == "POST" { client.post(url) } else { client.get(url) };
    let req = req.bearer_auth(token);
    let req = if let Some(body) = body { req.json(&body) } else { req };
    let response = req.send().map_err(|_| "Hors ligne")?;
    if !response.status().is_success() { return Err(if response.status().as_u16() == 401 || response.status().as_u16() == 403 { "Accès refusé" } else { "Erreur serveur" }); }
    response.json::<Value>().map_err(|_| "Réponse invalide")
}
#[derive(Clone, Copy)] enum Job { Test, List, Poll, Toggle, Service }
struct Reply { context: String, job: Job, result: Result<Value, &'static str> }
struct Key { action: String, settings: Value, pending: bool, last: Instant, label: String }
struct Home { keys: HashMap<String, Key>, tx: Sender<Reply>, rx: Receiver<Reply>, connected: bool }
impl Default for Home {
    fn default() -> Self { let (tx, rx) = mpsc::channel(); Self { keys: HashMap::new(), tx, rx, connected: load_token().ok().flatten().is_some() } }
}
impl Home {
    fn draw(&self, ctx: &str, sdk: &mut Client) -> Result<(), String> {
        if let Some(key) = self.keys.get(ctx) {
            let title = if key.label.is_empty() { "Home Assistant" } else { &key.label };
            sdk.title(ctx, title)?;
            let lines = [if key.action == TOGGLE { "Maison" } else if key.action == SENSOR { "État" } else { "Service" }, title];
            let color = if key.label == "on" { Some(1.0) } else if key.label == "off" { Some(0.0) } else { None };
            sdk.image(ctx, &novydeck_sdk::render(&lines, color)?)?;
        }
        Ok(())
    }
    fn schedule(&mut self, ctx: &str, job: Job) {
        let Some(key) = self.keys.get_mut(ctx) else { return; };
        if key.pending || (matches!(job, Job::Poll) && !entity(key.settings["entity"].as_str().unwrap_or(""))) { return; }
        key.pending = true;
        key.last = Instant::now();
        let settings = key.settings.clone();
        let sender = self.tx.clone();
        let context = ctx.to_owned();
        thread::spawn(move || {
            let result = (|| {
                let token = load_token()?.ok_or("Non connecté")?;
                match job {
                    Job::Test => request(&settings, &token, "GET", "/api/", None),
                    Job::List => request(&settings, &token, "GET", "/api/states", None).and_then(|v| {
                        let items = v.as_array().ok_or("Réponse invalide")?;
                        Ok(json!(items.iter().filter_map(|item| {
                            let id = item["entity_id"].as_str()?;
                            if !entity(id) { return None; }
                            Some(json!({"id": id, "name": item["attributes"]["friendly_name"].as_str().unwrap_or(id)}))
                        }).take(2000).collect::<Vec<_>>()))
                    }),
                    Job::Poll => {
                        let id = settings["entity"].as_str().unwrap_or("");
                        if !entity(id) { return Err("Entité invalide"); }
                        request(&settings, &token, "GET", &format!("/api/states/{id}"), None)
                    }
                    Job::Toggle => {
                        let body = service_body(&settings)?;
                        request(&settings, &token, "POST", "/api/services/homeassistant/toggle", Some(body))
                    }
                    Job::Service => {
                        let domain = settings["domain"].as_str().unwrap_or("");
                        let service = settings["service"].as_str().unwrap_or("");
                        if !identifier(domain) || !identifier(service) { return Err("Service invalide"); }
                        let body = service_body(&settings)?;
                        request(&settings, &token, "POST", &format!("/api/services/{domain}/{service}"), Some(body))
                    }
                }
            })();
            let _ = sender.send(Reply { context, job, result });
        });
    }
    fn status(&self, ctx: &str, sdk: &mut Client) -> Result<(), String> {
        sdk.inspector(ctx, json!({"type":"status", "connected":self.connected}))
    }
}
impl Plugin for Home {
    fn event(&mut self, event: Event, sdk: &mut Client) -> Result<(), String> {
        let ctx = event.context.clone();
        match event.name.as_str() {
            "willAppear" => {
                self.keys.insert(ctx.clone(), Key { action:event.action.clone(), settings:safe_settings(event.settings()), pending:false, last:Instant::now() - Duration::from_secs(301), label:String::new() });
                self.draw(&ctx, sdk)?;
                if self.keys[&ctx].action != SERVICE { self.schedule(&ctx, Job::Poll); }
            }
            "willDisappear" => { self.keys.remove(&ctx); }
            "didReceiveSettings" => {
                if let Some(key) = self.keys.get_mut(&ctx) { key.settings = safe_settings(event.settings()); key.last = Instant::now() - Duration::from_secs(301); }
                self.draw(&ctx, sdk)?;
            }
            "propertyInspectorDidAppear" => { self.status(&ctx, sdk)?; }
            "sendToPlugin" => {
                let op = event.payload["op"].as_str().unwrap_or("");
                if matches!(op, "test" | "list") {
                    if let (Some(key), Some(settings)) = (self.keys.get_mut(&ctx), event.payload.get("settings")) {
                        key.settings = safe_settings(settings);
                    }
                }
                match op {
                    "saveToken" => {
                        let value = event.payload["token"].as_str().unwrap_or("");
                        if !value.is_empty() && value.len() <= 8_192 && !value.contains('\n') && !value.contains('\r') {
                            self.connected = store_token(value).is_ok();
                        }
                        self.status(&ctx, sdk)?;
                    }
                    "status" => self.status(&ctx, sdk)?,
                    "test" => self.schedule(&ctx, Job::Test),
                    "list" => self.schedule(&ctx, Job::List),
                    _ => {}
                }
            }
            "keyUp" => {
                if let Some(key) = self.keys.get(&ctx) { match key.action.as_str() {
                    TOGGLE => self.schedule(&ctx, Job::Toggle),
                    SERVICE => self.schedule(&ctx, Job::Service),
                    _ => {}
                } }
            }
            _ => {}
        }
        Ok(())
    }
    fn tick(&mut self, sdk: &mut Client) -> Result<(), String> {
        while let Ok(reply) = self.rx.try_recv() {
            let Some(key) = self.keys.get_mut(&reply.context) else { continue; };
            key.pending = false;
            match reply.result {
                Ok(value) => match reply.job {
                    Job::List => sdk.inspector(&reply.context, json!({"type":"entities", "items":value}))?,
                    Job::Test => sdk.inspector(&reply.context, json!({"type":"test", "ok":true}))?,
                    Job::Poll => {
                        key.label = state_label(&value);
                        self.draw(&reply.context, sdk)?;
                    }
                    Job::Toggle => { sdk.ok(&reply.context)?; self.schedule(&reply.context, Job::Poll); }
                    Job::Service => sdk.ok(&reply.context)?,
                },
                Err(message) => {
                    if matches!(reply.job, Job::Test | Job::List) { sdk.inspector(&reply.context, json!({"type":"error", "message":message}))?; }
                    else { key.label = message.into(); self.draw(&reply.context, sdk)?; sdk.alert(&reply.context)?; }
                }
            }
        }
        let due: Vec<_> = self.keys.iter().filter(|(_, k)| k.action != SERVICE && !k.pending && k.last.elapsed() >= refresh_interval(&k.settings)).map(|(c, _)| c.clone()).collect();
        for ctx in due { self.schedule(&ctx, Job::Poll); }
        Ok(())
    }
}
fn main() { if let Err(e) = novydeck_sdk::run(Home::default()) { eprintln!("homeassistant: {e}"); std::process::exit(1); } }
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn url_validation() {
        assert_eq!(endpoint("http://homeassistant.local:8123", "/api/").unwrap().as_str(), "http://homeassistant.local:8123/api/");
        for url in ["ftp://host", "https://user:pass@host/", "http://host/path", "http://host/?x=1"] { assert!(endpoint(url, "/api/").is_err()); }
    }
    #[test] fn service_validation() {
        assert_eq!(service_body(&json!({"entity":"light.salon", "data":"{\"brightness\":50,\"entity_id\":\"switch.other\"}"})).unwrap(), json!({"brightness":50,"entity_id":"light.salon"}));
        assert!(service_body(&json!({"entity":"../admin", "data":"{}"})).is_err());
        assert!(service_body(&json!({"entity":"light.salon", "data":"[]"})).is_err());
    }
    #[test] fn state_and_refresh() {
        assert_eq!(state_label(&json!({"state":"19.5","attributes":{"unit_of_measurement":"°C"}})), "19.5 °C");
        assert_eq!(refresh_interval(&json!({"refresh":0})).as_secs(), 2);
        assert_eq!(refresh_interval(&Value::Null).as_secs(), 10);
        assert!(safe_settings(&json!({"token":"secret"})).get("token").is_none());
    }
}

use std::{collections::HashMap, fs, io::{Read, Write}, net::TcpListener, path::PathBuf, sync::mpsc::{self, Receiver, Sender}, thread, time::{Duration, Instant}};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use novydeck_sdk::{Client, Event, Plugin};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tonic::{codegen::http::uri::PathAndQuery, transport::{Channel, ClientTlsConfig, Endpoint}, Request};
use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN}};

const ACTION: &str = "com.novydeck.googleassistant.command";
const SCOPE: &str = "https://www.googleapis.com/auth/assistant-sdk-prototype";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const RPC: &str = "/google.assistant.embedded.v1alpha2.EmbeddedAssistant/Assist";

// Field numbers verified against googleapis/google/assistant/embedded/v1alpha2/embedded_assistant.proto.
mod proto {
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct AssistRequest { #[prost(message, optional, tag="1")] pub config: Option<AssistConfig> }
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct AssistConfig {
        #[prost(string, tag="6")] pub text_query: String,
        #[prost(message, optional, tag="2")] pub audio_out_config: Option<AudioOutConfig>,
        #[prost(message, optional, tag="3")] pub dialog_state_in: Option<DialogStateIn>,
        #[prost(message, optional, tag="4")] pub device_config: Option<DeviceConfig>,
    }
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct AudioOutConfig {
        #[prost(int32, tag="1")] pub encoding: i32,
        #[prost(int32, tag="2")] pub sample_rate_hertz: i32,
        #[prost(int32, tag="3")] pub volume_percentage: i32,
    }
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DialogStateIn {
        #[prost(string, tag="2")] pub language_code: String,
        #[prost(bool, tag="7")] pub is_new_conversation: bool,
    }
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DeviceConfig {
        #[prost(string, tag="1")] pub device_id: String,
        #[prost(string, tag="3")] pub device_model_id: String,
    }
    // Discard the audio payload; stream must still be drained so server errors are observed.
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct AssistResponse {
        #[prost(int32, tag="1")] pub event_type: i32,
        #[prost(message, optional, tag="3")] pub audio_out: Option<AudioOut>,
    }
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct AudioOut { #[prost(bytes, tag="1")] pub audio_data: Vec<u8> }
}

#[derive(Clone, Serialize, Deserialize)]
struct Secret { client_id: String, client_secret: String, refresh_token: String }
fn secret_path() -> Result<PathBuf, String> {
    Ok(PathBuf::from(std::env::var_os("APPDATA").ok_or("APPDATA indisponible")?)
        .join("NovyDeck/plugins/com.novydeck.googleassistant/secrets.bin"))
}
fn protect(data: &[u8], decrypt: bool) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB { cbData: data.len().try_into().map_err(|_| "Secret trop grand")?, pbData: data.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        if decrypt { CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) }
        else { CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) }
    };
    if ok == 0 { return Err("Chiffrement Windows indisponible".into()); }
    let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe { LocalFree(output.pbData as *mut _); }
    Ok(bytes)
}
fn load() -> Result<Option<Secret>, String> {
    let bytes = match fs::read(secret_path()?) { Ok(bytes) => bytes, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None), Err(_) => return Err("Lecture du secret impossible".into()) };
    serde_json::from_slice(&protect(&bytes, true)?).map(Some).map_err(|_| "Secret invalide".into())
}
fn save(secret: &Secret) -> Result<(), String> {
    let path = secret_path()?;
    fs::create_dir_all(path.parent().ok_or("Chemin invalide")?).map_err(|_| "Dossier secret inaccessible")?;
    let encrypted = protect(&serde_json::to_vec(secret).map_err(|_| "Secret invalide")?, false)?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, encrypted).map_err(|_| "Écriture du secret impossible")?;
    fs::rename(tmp, path).map_err(|_| "Enregistrement du secret impossible".into())
}
fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder().timeout(Duration::from_secs(10)).build().map_err(|_| "Client HTTPS indisponible".into())
}
fn token(form: &[(&str, &str)]) -> Result<Value, String> {
    let resp = client()?.post(TOKEN_URL).form(form).send().map_err(|_| "Hors ligne".to_string())?;
    if !resp.status().is_success() { return Err("Autorisation refusée (vérifier le compte Google)".into()); }
    resp.json().map_err(|_| "Réponse OAuth invalide".into())
}
fn access_token(secret: &Secret) -> Result<String, String> {
    let response = token(&[("client_id", &secret.client_id), ("client_secret", &secret.client_secret), ("refresh_token", &secret.refresh_token), ("grant_type", "refresh_token")])?;
    response["access_token"].as_str().map(str::to_owned).ok_or("Token absent".into())
}
fn settings(value: &Value) -> Result<(String, String), String> {
    let phrase = value["phrase"].as_str().unwrap_or("").trim();
    if phrase.is_empty() || phrase.chars().count() > 500 { return Err("Phrase requise (500 caractères max)".into()); }
    let language = value["language"].as_str().unwrap_or("fr-FR");
    if language.len() > 20 || language.is_empty() || !language.bytes().all(|c| c.is_ascii_alphabetic() || c == b'-') { return Err("Langue invalide".into()); }
    Ok((phrase.into(), language.into()))
}
fn request(phrase: String, language: String) -> proto::AssistRequest {
    proto::AssistRequest { config: Some(proto::AssistConfig {
        text_query: phrase,
        // MP3=2. Audio is discarded, not played. Volume 100 is valid (0 is not).
        audio_out_config: Some(proto::AudioOutConfig { encoding: 2, sample_rate_hertz: 16000, volume_percentage: 100 }),
        dialog_state_in: Some(proto::DialogStateIn { language_code: language, is_new_conversation: true }),
        // gassist-text defaults (also used by Home Assistant's SDK integration).
        device_config: Some(proto::DeviceConfig { device_id: "default".into(), device_model_id: "default".into() }),
    }) }
}
fn assist(phrase: String, language: String) -> Result<(), String> {
    let secret = load()?.filter(|s| !s.refresh_token.is_empty()).ok_or("Non connecté")?;
    let access = access_token(&secret)?;
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_| "Service indisponible")?;
    rt.block_on(async {
        let endpoint = Endpoint::from_static("https://embeddedassistant.googleapis.com")
            .tls_config(ClientTlsConfig::new().domain_name("embeddedassistant.googleapis.com")).map_err(|_| "TLS indisponible")?
            .connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(10));
        let channel: Channel = endpoint.connect().await.map_err(|_| "Hors ligne")?;
        let mut grpc = tonic::client::Grpc::new(channel);
        grpc.ready().await.map_err(|_| "Hors ligne")?;
        let mut rpc = Request::new(tokio_stream_once(request(phrase, language)));
        let auth = format!("Bearer {access}").parse().map_err(|_| "Token invalide")?;
        rpc.metadata_mut().insert("authorization", auth);
        rpc.set_timeout(Duration::from_secs(10));
        let response: tonic::Response<tonic::Streaming<proto::AssistResponse>> = grpc.streaming(rpc, PathAndQuery::from_static(RPC), tonic::codec::ProstCodec::default()).await.map_err(grpc_error)?;
        let mut stream = response.into_inner();
        loop {
            match tokio::time::timeout(Duration::from_secs(10), stream.message()).await {
                Ok(Ok(Some(_))) => {},
                Ok(Ok(None)) => return Ok(()),
                Ok(Err(e)) => return Err(grpc_error(e)),
                Err(_) => return Err("Délai Google dépassé".into()),
            }
        }
    })
}
fn tokio_stream_once(item: proto::AssistRequest) -> impl tokio_stream::Stream<Item=proto::AssistRequest> {
    tokio_stream::iter([item])
}
fn grpc_error(status: tonic::Status) -> String {
    use tonic::Code;
    match status.code() { Code::Unauthenticated | Code::PermissionDenied => "Refusé par Google".into(), Code::DeadlineExceeded => "Délai Google dépassé".into(), Code::Unavailable => "Hors ligne".into(), _ => format!("Erreur Google ({})", status.code()) }
}
fn random() -> String { let mut bytes = [0u8; 32]; rand::thread_rng().fill_bytes(&mut bytes); URL_SAFE_NO_PAD.encode(bytes) }
fn auth_url(client_id: &str, redirect: &str, state: &str, challenge: &str) -> String {
    let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth").expect("static URL");
    url.query_pairs_mut().append_pair("client_id", client_id).append_pair("redirect_uri", redirect)
        .append_pair("response_type", "code").append_pair("scope", SCOPE).append_pair("access_type", "offline")
        .append_pair("prompt", "consent").append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256").append_pair("state", state);
    url.into()
}
fn callback(listener: TcpListener, expected: &str) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|_| "Écoute OAuth indisponible")?;
    let deadline = Instant::now() + Duration::from_secs(300);
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut socket, addr)) => {
                if !addr.ip().is_loopback() { continue; }
                socket.set_read_timeout(Some(Duration::from_secs(2))).ok();
                let mut bytes = [0u8; 4096];
                let n = socket.read(&mut bytes).unwrap_or(0);
                let line = String::from_utf8_lossy(&bytes[..n]);
                let target = line.lines().next().unwrap_or("").split_whitespace().nth(1).unwrap_or("");
                let params = url::Url::parse(&format!("http://127.0.0.1{target}"));
                let (mut state, mut code) = (None, None);
                if target.starts_with("/callback?") { if let Ok(url) = params {
                    for (key, val) in url.query_pairs() {
                        if key == "state" { state = Some(val.into_owned()); }
                        else if key == "code" { code = Some(val.into_owned()); }
                    }
                } }
                let valid = state.as_deref() == Some(expected) && code.is_some();
                let text = if valid { "Connexion reçue. Vous pouvez fermer cette page." } else { "Requête non reconnue." };
                let status = if valid { "200 OK" } else { "400 Bad Request" };
                let body = format!("<html><meta charset=utf-8><body>{text}</body></html>");
                let _ = write!(socket, "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                if valid { return Ok(code.expect("checked")); }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(100)),
            Err(_) => return Err("Écoute OAuth interrompue".into()),
        }
    }
    Err("Connexion expirée (5 min)".into())
}
fn parse_client(raw: &str) -> Result<Secret, String> {
    if raw.len() > 65536 { return Err("JSON trop grand".into()); }
    let data: Value = serde_json::from_str(raw).map_err(|_| "JSON OAuth invalide")?;
    let desktop = data.get("installed").ok_or("Choisir une application de bureau")?;
    let id = desktop["client_id"].as_str().filter(|s| !s.is_empty()).ok_or("client_id absent")?;
    let secret = desktop["client_secret"].as_str().filter(|s| !s.is_empty()).ok_or("client_secret absent")?;
    Ok(Secret { client_id: id.into(), client_secret: secret.into(), refresh_token: String::new() })
}
enum ResultEvent { Authorized(String, u64, Result<Secret, String>), Tested(String, Result<(), String>) }
#[derive(Default)] struct Google {
    states: HashMap<String, Value>,
    tx: Option<Sender<ResultEvent>>, rx: Option<Receiver<ResultEvent>>,
    pending: bool, generation: u64,
}
impl Google {
    fn send_status(&self, ctx: &str, sdk: &mut Client) -> Result<(), String> {
        let connected = load().ok().flatten().is_some_and(|s| !s.refresh_token.is_empty());
        sdk.inspector(ctx, json!({"type":"status","connected":connected,"pending":self.pending}))
    }
    fn show_error(ctx: &str, message: &str, sdk: &mut Client) -> Result<(), String> {
        sdk.title(ctx, message)?; sdk.alert(ctx)
    }
    fn test(&self, ctx: String, sdk: &mut Client) -> Result<(), String> {
        let Some(value) = self.states.get(&ctx) else { return Ok(()); };
        let (phrase, language) = match settings(value) { Ok(s) => s, Err(e) => return Self::show_error(&ctx, &e, sdk) };
        let tx = self.tx.as_ref().ok_or("Canal indisponible")?.clone();
        sdk.title(&ctx, "Envoi…")?;
        thread::spawn(move || { let _ = tx.send(ResultEvent::Tested(ctx, assist(phrase, language))); });
        Ok(())
    }
}
impl Plugin for Google {
    fn event(&mut self, event: Event, sdk: &mut Client) -> Result<(), String> {
        let ctx = event.context.clone();
        let outcome = (|| -> Result<(), String> {
        match event.name.as_str() {
            "willAppear" => { self.states.insert(ctx.clone(), event.settings().clone()); sdk.title(&ctx, "Google")?; }
            "willDisappear" => { self.states.remove(&ctx); }
            "didReceiveSettings" => { self.states.insert(ctx, event.settings().clone()); }
            "propertyInspectorDidAppear" => { self.send_status(&ctx, sdk)?; }
            "keyDown" if event.action == ACTION => { self.test(ctx, sdk)?; }
            "sendToPlugin" => match event.payload["type"].as_str().unwrap_or("") {
                "status" => self.send_status(&ctx, sdk)?,
                "import" => {
                    let result = event.payload["json"].as_str().ok_or_else(|| "JSON absent".to_string()).and_then(parse_client).and_then(|s| save(&s));
                    match result { Ok(()) => sdk.inspector(&ctx, json!({"type":"message","message":"Identifiant importé ; connectez-vous."}))?, Err(e) => sdk.inspector(&ctx, json!({"type":"message","message":e}))? }
                    self.send_status(&ctx, sdk)?;
                }
                "connect" if !self.pending => {
                    let secret = load()?.ok_or("Importer d'abord le JSON client")?;
                    let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| "Port local indisponible")?;
                    let redirect = format!("http://127.0.0.1:{}/callback", listener.local_addr().map_err(|_| "Port local indisponible")?.port());
                    let state = random(); let verifier = random();
                    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
                    let link = auth_url(&secret.client_id, &redirect, &state, &challenge);
                    let tx = self.tx.as_ref().ok_or("Canal indisponible")?.clone();
                    let origin = ctx.clone();
                    let generation = self.generation;
                    self.pending = true;
                    thread::spawn(move || {
                        let result = callback(listener, &state).and_then(|code| {
                            let data = token(&[("client_id", &secret.client_id), ("client_secret", &secret.client_secret), ("code", &code), ("code_verifier", &verifier), ("redirect_uri", &redirect), ("grant_type", "authorization_code")])?;
                            let refresh = data["refresh_token"].as_str().filter(|s| !s.is_empty()).ok_or("Google n'a pas remis de refresh token")?;
                            Ok(Secret { refresh_token: refresh.into(), ..secret })
                        });
                        let _ = tx.send(ResultEvent::Authorized(origin, generation, result));
                    });
                    sdk.send(json!({"event":"openUrl","payload":{"url":link}}))?;
                    self.send_status(&ctx, sdk)?;
                }
                "disconnect" => {
                    self.generation += 1;
                    self.pending = false;
                    match fs::remove_file(secret_path()?) { Ok(()) => {}, Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}, Err(_) => return Err("Suppression impossible".into()) }
                    sdk.title(&ctx, "Non connecté")?; self.send_status(&ctx, sdk)?;
                }
                "test" => self.test(ctx, sdk)?,
                _ => {}
            },
            _ => {}
        }
        Ok(())
        })();
        if let Err(message) = outcome {
            let _ = sdk.inspector(&ctx, json!({"type":"message","message":message}));
            Self::show_error(&ctx, &message, sdk)?;
        }
        Ok(())
    }
    fn tick(&mut self, sdk: &mut Client) -> Result<(), String> {
        while let Ok(result) = self.rx.as_ref().ok_or("Canal indisponible")?.try_recv() {
            match result {
                ResultEvent::Authorized(ctx, generation, result) => {
                    if generation != self.generation { continue; }
                    self.pending = false;
                    match result.and_then(|s| save(&s)) {
                        Ok(()) => sdk.inspector(&ctx, json!({"type":"message","message":"Connexion établie."}))?,
                        Err(e) => sdk.inspector(&ctx, json!({"type":"message","message":e}))?,
                    }
                    self.send_status(&ctx, sdk)?;
                }
                ResultEvent::Tested(ctx, result) => match result {
                    Ok(()) => { sdk.title(&ctx, "Envoyé")?; sdk.ok(&ctx)?; },
                    Err(e) => Self::show_error(&ctx, &e, sdk)?,
                }
            }
        }
        Ok(())
    }
}
fn main() {
    let (tx, rx) = mpsc::channel();
    let google = Google { tx: Some(tx), rx: Some(rx), ..Google::default() };
    if let Err(e) = novydeck_sdk::run(google) { eprintln!("googleassistant: {e}"); std::process::exit(1); }
}
#[cfg(test)] mod tests {
    use super::*;
    use prost::Message;
    #[test] fn parse_and_validate() {
        assert!(parse_client(r#"{"web":{"client_id":"bad"}}"#).is_err());
        assert_eq!(parse_client(r#"{"installed":{"client_id":"id","client_secret":"secret"}}"#).unwrap().client_id,"id");
        assert!(settings(&json!({"phrase":"  "})).is_err());
        assert_eq!(settings(&json!({"phrase":" allume ","language":"fr-FR"})).unwrap(), ("allume".into(),"fr-FR".into()));
    }
    #[test] fn wire_tags() {
        let bytes = request("test".into(),"fr-FR".into()).encode_to_vec();
        assert_eq!(bytes[0], 0x0a); // AssistRequest.config = 1
        let config = request("test".into(),"fr-FR".into()).config.unwrap().encode_to_vec();
        assert!(config.windows(2).any(|b| b == [0x32,4])); // text_query = 6
        let device = proto::DeviceConfig { device_id: "default".into(), device_model_id: "default".into() }.encode_to_vec();
        assert!(device.contains(&0x1a)); // device_model_id = 3
    }
    #[test] fn authorization_url() {
        let link = auth_url("id", "http://127.0.0.1:123/callback", "state", "challenge");
        assert!(link.contains("code_challenge_method=S256")); assert!(link.contains("state=state"));
    }
}

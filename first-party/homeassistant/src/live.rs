//! Connexion WebSocket temps réel unique, partagée par toutes les touches du plugin.
//! Adapté du client NovyDeck 0.19 : authentification, `subscribe_events state_changed`,
//! `get_states` après chaque (re)connexion, reconnexion avec délai croissant.
//! Le jeton n'apparaît ni dans les erreurs, ni dans `Debug`, ni dans les journaux.
use reqwest::Url;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fmt,
    net::{TcpStream, ToSocketAddrs},
    sync::{
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tungstenite::{stream::MaybeTlsStream, Connector, Message, WebSocket};

/// Échec lisible : jamais de jeton, jamais d'URL complète.
#[derive(Debug)]
pub enum Fail {
    Auth(String),
    Other(String),
}
impl Fail {
    fn other(message: &str) -> Self {
        Self::Other(message.to_owned())
    }
    pub fn message(&self) -> String {
        match self {
            Self::Auth(m) if m.is_empty() => "Jeton refusé par Home Assistant".into(),
            Self::Auth(m) => format!("Jeton refusé par Home Assistant : {m}"),
            Self::Other(m) => m.clone(),
        }
    }
}
type Res<T> = Result<T, Fail>;

#[derive(Clone, PartialEq, Eq)]
pub struct Settings {
    pub url: String,
    pub token: String,
    pub insecure: bool,
    /// Origine pour laquelle le jeton a été saisi : le jeton n'est jamais envoyé ailleurs.
    pub bound_origin: String,
}
impl fmt::Debug for Settings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Settings")
            .field("url", &self.url)
            .field("token", &"***")
            .field("insecure", &self.insecure)
            .field("bound_origin", &self.bound_origin)
            .finish()
    }
}

// ---------------------------------------------------------------- adresse

/// `http(s)://hôte:port` (racine du serveur) → `ws(s)://hôte:port/api/websocket`.
pub fn websocket_url(base: &str) -> Result<Url, &'static str> {
    let mut url = Url::parse(base.trim()).map_err(|_| "URL invalide")?;
    let scheme = match url.scheme() {
        "http" | "ws" => "ws",
        "https" | "wss" => "wss",
        _ => return Err("URL invalide"),
    };
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("URL invalide");
    }
    if url.path() != "/" {
        return Err("URL : utiliser la racine du serveur");
    }
    url.set_scheme(scheme).map_err(|_| "URL invalide")?;
    url.set_path("/api/websocket");
    Ok(url)
}

// ---------------------------------------------------------------- protocole

#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    pub state: String,
    pub attributes: Value,
}
impl Entity {
    pub fn friendly_name(&self) -> Option<&str> {
        self.attributes["friendly_name"].as_str()
    }
    pub fn unit(&self) -> &str {
        self.attributes["unit_of_measurement"]
            .as_str()
            .unwrap_or("")
    }
    /// Valeur comparée : l'état, ou un attribut si `attribute` n'est pas vide.
    pub fn value(&self, attribute: &str) -> Option<String> {
        if attribute.is_empty() {
            return Some(self.state.clone());
        }
        match self.attributes.get(attribute)? {
            Value::Null => None,
            Value::String(s) => Some(s.clone()),
            other => Some(other.to_string()),
        }
    }
}
fn entity_of(value: &Value) -> Option<(String, Entity)> {
    let id = value["entity_id"].as_str()?.to_owned();
    Some((
        id,
        Entity {
            state: value["state"].as_str().unwrap_or("").to_owned(),
            attributes: value.get("attributes").cloned().unwrap_or(Value::Null),
        },
    ))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    AuthRequired,
    AuthOk,
    AuthInvalid(String),
    Result {
        id: u64,
        success: bool,
        result: Value,
        error: String,
    },
    StateChanged {
        entity_id: String,
        new_state: Option<Entity>,
    },
    Other,
}
fn short(v: &Value) -> String {
    v.as_str().unwrap_or("").chars().take(200).collect()
}
pub fn parse(text: &str) -> Result<Incoming, &'static str> {
    let v: Value = serde_json::from_str(text).map_err(|_| "Message Home Assistant illisible")?;
    Ok(match v["type"].as_str().unwrap_or("") {
        "auth_required" => Incoming::AuthRequired,
        "auth_ok" => Incoming::AuthOk,
        "auth_invalid" => Incoming::AuthInvalid(short(&v["message"])),
        "result" => Incoming::Result {
            id: v["id"].as_u64().unwrap_or(0),
            success: v["success"].as_bool().unwrap_or(false),
            result: v["result"].clone(),
            error: short(&v["error"]["message"]),
        },
        "event" if v["event"]["event_type"] == "state_changed" => {
            let data = &v["event"]["data"];
            let Some(entity_id) = data["entity_id"].as_str() else {
                return Ok(Incoming::Other);
            };
            Incoming::StateChanged {
                entity_id: entity_id.to_owned(),
                new_state: entity_of(&data["new_state"]).map(|(_, e)| e),
            }
        }
        _ => Incoming::Other,
    })
}
pub fn states_of(result: &Value) -> HashMap<String, Entity> {
    result
        .as_array()
        .map(|xs| xs.iter().filter_map(entity_of).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------- connexion

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;
const SUBSCRIBE_ID: u64 = 1;
const STATES_ID: u64 = 2;

fn tcp(socket: &mut Socket) -> Option<&mut TcpStream> {
    match socket.get_mut() {
        MaybeTlsStream::Plain(s) => Some(s),
        MaybeTlsStream::Rustls(s) => Some(s.get_mut()),
        _ => None,
    }
}
fn set_timeout(ws: &mut Socket, timeout: Duration) -> Res<()> {
    tcp(ws)
        .ok_or_else(|| Fail::other("Connexion Home Assistant indisponible"))?
        .set_read_timeout(Some(timeout))
        .map_err(|_| Fail::other("Connexion Home Assistant indisponible"))
}
fn read_text(ws: &mut Socket) -> Res<Option<String>> {
    match ws.read() {
        Ok(Message::Text(text)) => Ok(Some(text.to_string())),
        Ok(Message::Close(_)) => Err(Fail::other("Home Assistant a fermé la connexion")),
        Ok(_) => Ok(None),
        Err(tungstenite::Error::Io(e))
            if matches!(
                e.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) =>
        {
            Ok(None)
        }
        Err(_) => Err(Fail::other("Connexion Home Assistant interrompue")),
    }
}
fn send(ws: &mut Socket, value: Value) -> Res<()> {
    ws.send(Message::Text(value.to_string()))
        .map_err(|_| Fail::other("Envoi vers Home Assistant impossible"))
}

/// Accepte tout certificat (option « certificats auto-signés », dangereuse et explicite).
#[derive(Debug)]
struct AcceptAny(Arc<rustls::crypto::CryptoProvider>);
impl rustls::client::danger::ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
fn tls_config(insecure: bool) -> Res<Arc<rustls::ClientConfig>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|_| Fail::other("TLS indisponible"))?;
    let config = if insecure {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAny(provider)))
            .with_no_client_auth()
    } else {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        builder.with_root_certificates(roots).with_no_client_auth()
    };
    Ok(Arc::new(config))
}

/// Ouvre la connexion et s'authentifie.
fn open(settings: &Settings, timeout: Duration) -> Res<Socket> {
    // Avant toute résolution ou connexion : le jeton ne quitte jamais son origine liée.
    if crate::store::origin_of(&settings.url).ok().as_deref()
        != Some(settings.bound_origin.as_str())
    {
        return Err(Fail::other(crate::store::MISMATCH));
    }
    if settings.insecure && !crate::store::url_is_local(&settings.url) {
        return Err(Fail::other(crate::store::INSECURE_REFUSED));
    }
    let url = websocket_url(&settings.url).map_err(Fail::other)?;
    let host = url
        .host_str()
        .ok_or_else(|| Fail::other("URL invalide"))?
        .to_owned();
    let port = url
        .port_or_known_default()
        .ok_or_else(|| Fail::other("URL invalide"))?;
    let addrs = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|_| Fail::other("Serveur Home Assistant introuvable"))?;
    let stream = addrs
        .into_iter()
        .find_map(|addr| TcpStream::connect_timeout(&addr, timeout).ok())
        .ok_or_else(|| Fail::other("Serveur Home Assistant injoignable"))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let _ = stream.set_nodelay(true);
    let connector = if url.scheme() == "wss" {
        Some(Connector::Rustls(tls_config(settings.insecure)?))
    } else {
        None
    };
    let (mut ws, _) = tungstenite::client_tls_with_config(url.as_str(), stream, None, connector)
        .map_err(|e| match e {
            tungstenite::HandshakeError::Failure(tungstenite::Error::Tls(_)) => {
                Fail::other("Connexion sécurisée refusée (certificat ou TLS)")
            }
            _ => Fail::other("Connexion WebSocket Home Assistant impossible"),
        })?;
    let deadline = Instant::now() + timeout;
    let mut sent = false;
    loop {
        if Instant::now() >= deadline {
            return Err(Fail::other(
                "Home Assistant ne répond pas à l'authentification",
            ));
        }
        let Some(text) = read_text(&mut ws)? else {
            continue;
        };
        match parse(&text).map_err(Fail::other)? {
            Incoming::AuthRequired if !sent => {
                send(
                    &mut ws,
                    json!({"type": "auth", "access_token": settings.token}),
                )?;
                sent = true;
            }
            Incoming::AuthOk => return Ok(ws),
            Incoming::AuthInvalid(message) => return Err(Fail::Auth(message)),
            _ => {}
        }
    }
}

/// « Tester la connexion » : bloquant, à appeler hors du fil principal. Renvoie le nombre d'entités.
pub fn test_connection(settings: &Settings, timeout: Duration) -> Result<usize, String> {
    (|| -> Res<usize> {
        let mut ws = open(settings, timeout)?;
        send(&mut ws, json!({"id": STATES_ID, "type": "get_states"}))?;
        let deadline = Instant::now() + timeout;
        loop {
            if Instant::now() >= deadline {
                return Err(Fail::other("Home Assistant ne renvoie pas ses états"));
            }
            let Some(text) = read_text(&mut ws)? else {
                continue;
            };
            if let Incoming::Result {
                id: STATES_ID,
                success,
                result,
                ..
            } = parse(&text).map_err(Fail::other)?
            {
                if !success {
                    return Err(Fail::other("Home Assistant refuse la lecture des états"));
                }
                let _ = ws.close(None);
                return Ok(states_of(&result).len());
            }
        }
    })()
    .map_err(|f| f.message())
}

// ---------------------------------------------------------------- moniteur

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Disabled,
    Connecting,
    Connected,
    Reconnecting {
        attempt: u32,
        error: String,
    },
    AuthFailed(String),
}
impl Status {
    pub fn connected(&self) -> bool {
        matches!(self, Self::Connected)
    }
}

#[derive(Default)]
pub struct Shared {
    pub status: Status,
    pub entities: HashMap<String, Entity>,
}

#[derive(Clone, Copy)]
struct Timing {
    connect: Duration,
    keepalive: Duration,
    silence: Duration,
    poll: Duration,
    scale_ms: u64,
}
const REAL: Timing = Timing {
    connect: Duration::from_secs(8),
    keepalive: Duration::from_secs(30),
    silence: Duration::from_secs(75),
    poll: Duration::from_millis(250),
    scale_ms: 1000,
};

/// Délai croissant plafonné ; beaucoup plus long après un jeton refusé.
pub fn backoff(attempt: u32, auth_failed: bool) -> Duration {
    let secs = [1u64, 2, 5, 10, 30, 60][attempt.min(5) as usize];
    Duration::from_secs(if auth_failed { secs.max(60) * 5 } else { secs })
}

pub struct Monitor {
    tx: Sender<Option<Settings>>,
    shared: Arc<Mutex<Shared>>,
}
impl Monitor {
    pub fn spawn() -> Self {
        Self::spawn_with(REAL)
    }
    /// Moniteur sans thread ni réseau (tests) : les configurations sont ignorées.
    #[cfg(test)]
    pub fn inert() -> Self {
        let (tx, _rx) = mpsc::channel();
        Self {
            tx,
            shared: Arc::new(Mutex::new(Shared::default())),
        }
    }
    fn spawn_with(timing: Timing) -> Self {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("home-assistant".into())
            .spawn(move || run(rx, worker, timing))
            .expect("thread Home Assistant");
        Self { tx, shared }
    }
    /// Remplace les réglages (None = désactivé). Aucun accès réseau côté appelant.
    pub fn configure(&self, settings: Option<Settings>) {
        let _ = self.tx.send(settings);
    }
    pub fn read<R>(&self, f: impl FnOnce(&Shared) -> R) -> R {
        f(&self.shared.lock().unwrap_or_else(|e| e.into_inner()))
    }
    pub fn status(&self) -> Status {
        self.read(|s| s.status.clone())
    }
    /// `None` si Home Assistant n'est pas connecté ; sinon l'entité (ou son absence).
    pub fn lookup(&self, entity: &str) -> Option<Option<Entity>> {
        self.read(|s| {
            s.status
                .connected()
                .then(|| s.entities.get(entity).cloned())
        })
    }
    /// Entités connues triées par identifiant.
    pub fn entity_list(&self) -> Vec<(String, Entity)> {
        let mut list = self.read(|s| {
            s.entities
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>()
        });
        list.sort_by(|a, b| a.0.cmp(&b.0));
        list
    }
}

fn update(shared: &Mutex<Shared>, f: impl FnOnce(&mut Shared)) {
    f(&mut shared.lock().unwrap_or_else(|e| e.into_inner()));
}

enum Exit {
    Settings(Option<Settings>),
    Stop,
}

fn session(
    settings: &Settings,
    shared: &Mutex<Shared>,
    rx: &Receiver<Option<Settings>>,
    timing: Timing,
) -> Res<Exit> {
    let mut ws = open(settings, timing.connect)?;
    // Abonnement avant la lecture complète : aucun changement ne peut passer entre les deux.
    send(
        &mut ws,
        json!({"id": SUBSCRIBE_ID, "type": "subscribe_events", "event_type": "state_changed"}),
    )?;
    send(&mut ws, json!({"id": STATES_ID, "type": "get_states"}))?;
    set_timeout(&mut ws, timing.poll)?;
    let mut next_id = STATES_ID + 1;
    let mut last_rx = Instant::now();
    let mut last_ping = Instant::now();
    loop {
        match rx.try_recv() {
            Ok(other) => {
                if other.as_ref() != Some(settings) {
                    let _ = ws.close(None);
                    return Ok(Exit::Settings(other));
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                let _ = ws.close(None);
                return Ok(Exit::Stop);
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if let Some(text) = read_text(&mut ws)? {
            last_rx = Instant::now();
            match parse(&text).map_err(Fail::other)? {
                Incoming::Result {
                    id: STATES_ID,
                    success,
                    result,
                    ..
                } => {
                    if !success {
                        return Err(Fail::other("Home Assistant refuse la lecture des états"));
                    }
                    let entities = states_of(&result);
                    update(shared, |s| {
                        s.entities = entities;
                        s.status = Status::Connected;
                    });
                }
                Incoming::Result {
                    id: SUBSCRIBE_ID,
                    success: false,
                    ..
                } => {
                    return Err(Fail::other("Abonnement Home Assistant refusé"));
                }
                Incoming::StateChanged {
                    entity_id,
                    new_state,
                } => update(shared, |s| match new_state {
                    Some(e) => {
                        s.entities.insert(entity_id, e);
                    }
                    None => {
                        s.entities.remove(&entity_id);
                    }
                }),
                _ => {}
            }
        }
        if last_rx.elapsed() >= timing.silence {
            return Err(Fail::other("Home Assistant ne répond plus"));
        }
        if last_ping.elapsed() >= timing.keepalive {
            last_ping = Instant::now();
            send(&mut ws, json!({"id": next_id, "type": "ping"}))?;
            next_id += 1;
        }
    }
}

fn run(rx: Receiver<Option<Settings>>, shared: Arc<Mutex<Shared>>, timing: Timing) {
    let mut current: Option<Settings> = None;
    let mut attempt = 0u32;
    let mut due = Instant::now();
    let adopt = |next: Option<Settings>,
                 current: &mut Option<Settings>,
                 attempt: &mut u32,
                 due: &mut Instant| {
        if next != *current {
            *current = next;
            *attempt = 0;
            *due = Instant::now();
            let enabled = current.is_some();
            update(&shared, |s| {
                s.status = if enabled {
                    Status::Connecting
                } else {
                    Status::Disabled
                };
                // Un autre serveur ou un jeton différent : l'ancien cache n'a plus de sens.
                s.entities.clear();
            });
        }
    };
    loop {
        let wait = due
            .saturating_duration_since(Instant::now())
            .min(timing.poll);
        match rx.recv_timeout(wait) {
            Ok(next) => adopt(next, &mut current, &mut attempt, &mut due),
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        if Instant::now() < due {
            continue;
        }
        let Some(settings) = current.clone() else {
            due = Instant::now() + Duration::from_secs(3600);
            continue;
        };
        match session(&settings, &shared, &rx, timing) {
            Ok(Exit::Stop) => return,
            Ok(Exit::Settings(next)) => adopt(next, &mut current, &mut attempt, &mut due),
            Err(fail) => {
                let auth = matches!(fail, Fail::Auth(_));
                let message: String = fail.message().chars().take(240).collect();
                update(&shared, |s| {
                    // Connexion perdue : l'état est inconnu tant qu'une relecture complète n'a pas eu lieu.
                    s.status = if auth {
                        Status::AuthFailed(message)
                    } else {
                        Status::Reconnecting {
                            attempt: attempt + 1,
                            error: message,
                        }
                    };
                });
                let delay = backoff(attempt, auth);
                due = Instant::now()
                    + Duration::from_millis(delay.as_millis() as u64 * timing.scale_ms / 1000);
                attempt = attempt.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn settings(port: u16, token: &str) -> Settings {
        Settings {
            url: format!("http://127.0.0.1:{port}"),
            token: token.into(),
            insecure: false,
            bound_origin: format!("http://127.0.0.1:{port}"),
        }
    }

    #[test]
    fn websocket_url_forms() {
        assert_eq!(
            websocket_url("http://homeassistant.local:8123")
                .unwrap()
                .as_str(),
            "ws://homeassistant.local:8123/api/websocket"
        );
        assert_eq!(
            websocket_url(" https://ha.example.org/ ").unwrap().as_str(),
            "wss://ha.example.org/api/websocket"
        );
        assert_eq!(
            websocket_url("ws://10.0.0.2:8123").unwrap().as_str(),
            "ws://10.0.0.2:8123/api/websocket"
        );
        for bad in [
            "ftp://h",
            "https://u:p@h/",
            "http://h/path",
            "http://h/?x=1",
            "nope",
            "http://h/#a",
        ] {
            assert!(websocket_url(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn parse_messages() {
        assert_eq!(
            parse(r#"{"type":"auth_required"}"#).unwrap(),
            Incoming::AuthRequired
        );
        assert_eq!(
            parse(r#"{"type":"auth_invalid","message":"no"}"#).unwrap(),
            Incoming::AuthInvalid("no".into())
        );
        let changed = parse(r#"{"id":1,"type":"event","event":{"event_type":"state_changed","data":{"entity_id":"light.a","new_state":{"entity_id":"light.a","state":"on","attributes":{"brightness":12}}}}}"#).unwrap();
        let Incoming::StateChanged {
            entity_id,
            new_state: Some(e),
        } = changed
        else {
            panic!()
        };
        assert_eq!(
            (
                entity_id.as_str(),
                e.state.as_str(),
                e.value("brightness").as_deref()
            ),
            ("light.a", "on", Some("12"))
        );
        assert_eq!(e.value("absent"), None);
        let removed = parse(r#"{"type":"event","event":{"event_type":"state_changed","data":{"entity_id":"light.a","new_state":null}}}"#).unwrap();
        assert_eq!(
            removed,
            Incoming::StateChanged {
                entity_id: "light.a".into(),
                new_state: None
            }
        );
        assert!(parse("pas du json").is_err());
        assert_eq!(
            states_of(&json!([{"entity_id":"a.b","state":"1","attributes":{}}, {"x":1}])).len(),
            1
        );
    }
    #[test]
    fn backoff_grows_and_caps() {
        let d: Vec<u64> = (0..8).map(|a| backoff(a, false).as_secs()).collect();
        assert_eq!(d, [1, 2, 5, 10, 30, 60, 60, 60]);
        assert!(backoff(0, true) >= Duration::from_secs(300));
    }
    #[test]
    fn debug_never_shows_the_token() {
        let s = Settings {
            url: "http://ha:8123".into(),
            token: "jeton-super-secret".into(),
            bound_origin: "http://h:1".into(),
            insecure: false,
        };
        assert!(!format!("{s:?}").contains("super-secret"));
    }

    // ------------------------------------------------ faux serveur Home Assistant
    struct Fake {
        port: u16,
        states: Arc<Mutex<Vec<Value>>>,
        events: Sender<Value>,
        kick: Arc<AtomicBool>,
        sessions: Arc<AtomicUsize>,
    }
    fn state(id: &str, s: &str) -> Value {
        json!({"entity_id": id, "state": s, "attributes": {"friendly_name": id.to_uppercase()}})
    }
    fn fake(token: &'static str) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let states = Arc::new(Mutex::new(vec![state("binary_sensor.test_presence", "on")]));
        let (events, rx) = mpsc::channel::<Value>();
        let rx = Arc::new(Mutex::new(rx));
        let kick = Arc::new(AtomicBool::new(false));
        let sessions = Arc::new(AtomicUsize::new(0));
        let (st, k, n) = (states.clone(), kick.clone(), sessions.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let (st, k, n, rx) = (st.clone(), k.clone(), n.clone(), rx.clone());
                std::thread::spawn(move || {
                    let Ok(mut ws) = tungstenite::accept(stream) else {
                        return;
                    };
                    let text = |v: Value| Message::Text(v.to_string());
                    ws.send(text(json!({"type":"auth_required","ha_version":"test"})))
                        .unwrap();
                    let auth: Value =
                        serde_json::from_str(&ws.read().unwrap().into_text().unwrap()).unwrap();
                    if auth["access_token"] != token {
                        let _ = ws.send(text(json!({"type":"auth_invalid","message":"Invalid access token or password"})));
                        return;
                    }
                    ws.send(text(json!({"type":"auth_ok"}))).unwrap();
                    n.fetch_add(1, Ordering::SeqCst);
                    ws.get_mut()
                        .set_read_timeout(Some(Duration::from_millis(30)))
                        .unwrap();
                    loop {
                        if k.swap(false, Ordering::SeqCst) {
                            return; // coupure brutale
                        }
                        match ws.read() {
                            Ok(Message::Text(t)) => {
                                let m: Value = serde_json::from_str(&t).unwrap();
                                let reply = match m["type"].as_str().unwrap() {
                                    "get_states" => {
                                        json!({"id": m["id"], "type":"result","success":true,"result": st.lock().unwrap().clone()})
                                    }
                                    "ping" => json!({"id": m["id"], "type":"pong"}),
                                    _ => {
                                        json!({"id": m["id"], "type":"result","success":true,"result":null})
                                    }
                                };
                                if ws.send(text(reply)).is_err() {
                                    return;
                                }
                            }
                            Err(tungstenite::Error::Io(ref e))
                                if matches!(
                                    e.kind(),
                                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                                ) => {}
                            Err(_) => return,
                            _ => {}
                        }
                        while let Ok(new_state) = rx.lock().unwrap().try_recv() {
                            let id = new_state["entity_id"].clone();
                            let ev = json!({"id":1,"type":"event","event":{"event_type":"state_changed","data":{"entity_id": id, "new_state": new_state}}});
                            if ws.send(text(ev)).is_err() {
                                return;
                            }
                        }
                    }
                });
            }
        });
        Fake {
            port,
            states,
            events,
            kick,
            sessions,
        }
    }
    fn wait(what: &str, mut f: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while !f() {
            assert!(Instant::now() < end, "délai dépassé : {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    const FAST: Timing = Timing {
        connect: Duration::from_secs(3),
        keepalive: Duration::from_millis(300),
        silence: Duration::from_secs(3),
        poll: Duration::from_millis(20),
        scale_ms: 20,
    };

    #[test]
    fn connection_test_and_auth_never_leak_the_token() {
        let f = fake("good-token");
        assert_eq!(
            test_connection(&settings(f.port, "good-token"), Duration::from_secs(3)).unwrap(),
            1
        );
        let err = test_connection(&settings(f.port, "bad-token-XYZ"), Duration::from_secs(3))
            .unwrap_err();
        assert!(err.contains("refusé") && !err.contains("XYZ"), "{err}");
    }
    /// Serveur témoin brut : compte les connexions TCP reçues.
    fn counting_listener() -> (u16, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        std::thread::spawn(move || {
            for _ in listener.incoming() {
                h.fetch_add(1, Ordering::SeqCst);
            }
        });
        (port, hits)
    }
    #[test]
    fn token_is_never_sent_to_another_origin() {
        let (port, hits) = counting_listener();
        let mut s = settings(port, "good-token");
        s.bound_origin = "http://autre-hote.local:8123".into();
        let err = test_connection(&s, Duration::from_secs(1)).unwrap_err();
        assert_eq!(err, crate::store::MISMATCH);
        let monitor = Monitor::spawn_with(FAST);
        monitor.configure(Some(s));
        wait("refus", || {
            matches!(monitor.status(), Status::Reconnecting { .. })
        });
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(hits.load(Ordering::SeqCst), 0, "aucune connexion reçue");
    }
    #[test]
    fn insecure_is_refused_for_a_public_host_without_network() {
        let mut s = settings(1, "t");
        s.url = "https://ha.example.org".into();
        s.bound_origin = "https://ha.example.org:443".into();
        s.insecure = true;
        assert_eq!(
            test_connection(&s, Duration::from_secs(1)).unwrap_err(),
            crate::store::INSECURE_REFUSED
        );
    }
    #[test]
    fn live_events_reconnect_and_reload_states() {
        let f = fake("good-token");
        let monitor = Monitor::spawn_with(FAST);
        assert_eq!(
            monitor.lookup("binary_sensor.test_presence"),
            None,
            "inconnu avant connexion"
        );
        monitor.configure(Some(settings(f.port, "good-token")));
        wait("connexion", || monitor.status().connected());
        assert_eq!(monitor.entity_list().len(), 1);
        f.events
            .send(state("binary_sensor.test_presence", "off"))
            .unwrap();
        wait("événement poussé", || {
            monitor
                .lookup("binary_sensor.test_presence")
                .flatten()
                .is_some_and(|e| e.state == "off")
        });
        assert_eq!(monitor.lookup("light.absente"), Some(None));
        // Coupure : un état change pendant l'absence, la relecture complète le rattrape.
        *f.states.lock().unwrap() = vec![
            state("binary_sensor.test_presence", "on"),
            state("sensor.test_temp", "21.5"),
        ];
        f.kick.store(true, Ordering::SeqCst);
        wait("détection de la coupure", || !monitor.status().connected());
        wait("reconnexion", || {
            monitor.status().connected() && f.sessions.load(Ordering::SeqCst) >= 2
        });
        wait("relecture", || {
            monitor.read(|s| {
                s.entities.len() == 2 && s.entities["binary_sensor.test_presence"].state == "on"
            })
        });
        monitor.configure(None);
        wait("désactivation", || {
            monitor.status() == Status::Disabled && monitor.entity_list().is_empty()
        });
    }
    #[test]
    fn rejected_token_status_has_no_secret() {
        let f = fake("good-token");
        let monitor = Monitor::spawn_with(FAST);
        monitor.configure(Some(settings(f.port, "nope")));
        wait("refus", || {
            matches!(monitor.status(), Status::AuthFailed(_))
        });
        assert!(!format!("{:?}", monitor.status()).contains("nope"));
    }
    #[test]
    fn unreachable_server_reports_reconnecting() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let monitor = Monitor::spawn_with(FAST);
        monitor.configure(Some(settings(port, "t")));
        wait(
            "tentatives",
            || matches!(monitor.status(), Status::Reconnecting { attempt, .. } if attempt >= 2),
        );
    }
}

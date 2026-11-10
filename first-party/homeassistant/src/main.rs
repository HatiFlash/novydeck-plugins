//! NovyDeck Home Assistant 1.1 : une seule connexion WebSocket temps réel pour tout le plugin
//! (état affiché en direct, action « État automatique »), REST uniquement pour agir
//! (basculer / appeler un service).
mod live;
mod rule;
mod store;

use live::{Entity, Monitor, Settings, Status};
use novydeck_sdk::{Client, Event, Plugin, StateSource};
use reqwest::{blocking::Client as Http, redirect::Policy, Url};
use rule::{Edge, Machine, Reading, Rule};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};
use store::{load_config, load_token, store_config, store_token, Config};

const TOGGLE: &str = "com.novydeck.homeassistant.toggle";
const SERVICE: &str = "com.novydeck.homeassistant.service";
const SENSOR: &str = "com.novydeck.homeassistant.state";
const AUTO: &str = "com.novydeck.homeassistant.auto";
const MAX_LIST: usize = 3000;

fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
fn entity(s: &str) -> bool {
    rule::valid_entity_id(s)
}
fn endpoint(base: &str, path: &str) -> Result<Url, &'static str> {
    let mut url = Url::parse(base.trim()).map_err(|_| "URL invalide")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
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
    url.set_path(path);
    Ok(url)
}
/// Réglages d'une touche conservés tels quels (anciens `url`/`refresh` inclus : aucune perte au retour en 1.0.x).
fn safe_settings(settings: &Value) -> Value {
    let text = |k: &str| settings[k].as_str().unwrap_or("").to_owned();
    json!({"url": text("url"), "entity": text("entity"), "domain": text("domain"), "service": text("service"),
        "data": text("data"), "refresh": settings["refresh"].as_u64().unwrap_or(10).clamp(2, 300),
        "insecure": settings["insecure"].as_bool().unwrap_or(false),
        "attribute": text("attribute"), "op": text("op"), "value": text("value"),
        "hold_seconds": settings["hold_seconds"].clone(), "fire_at_start": settings["fire_at_start"].as_bool().unwrap_or(false)})
}
fn service_body(settings: &Value) -> Result<Value, &'static str> {
    let id = settings["entity"].as_str().unwrap_or("");
    if !entity(id) {
        return Err("Entité invalide");
    }
    let text = settings["data"].as_str().unwrap_or("").trim();
    let mut body: Value = if text.is_empty() {
        json!({})
    } else {
        serde_json::from_str(text).map_err(|_| "JSON invalide")?
    };
    let obj = body.as_object_mut().ok_or("JSON objet requis")?;
    // L'entité choisie dans l'inspecteur prime toujours sur les données avancées.
    obj.insert("entity_id".into(), json!(id));
    Ok(body)
}
fn state_label(e: &Entity) -> String {
    let unit = e.unit();
    format!(
        "{}{}{}",
        e.state.chars().take(16).collect::<String>(),
        if unit.is_empty() { "" } else { " " },
        unit.chars().take(8).collect::<String>()
    )
}

// ------------------------------------------------------------ migration 1.0.x → 1.1

/// URL et option TLS étaient par touche en 1.0.x : la première touche qui en porte une les
/// transmet au plugin si rien n'est encore configuré. Les réglages des touches restent intacts.
fn migrated(config: &Config, key_settings: &Value) -> Option<Config> {
    if config.configured() {
        return None;
    }
    let candidate = Config::from_value(key_settings);
    (candidate.configured() && endpoint(&candidate.url, "/").is_ok()).then_some(candidate)
}

// ------------------------------------------------------------ REST (actions)

fn http(insecure: bool) -> Result<Http, &'static str> {
    Http::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .redirect(Policy::none())
        .danger_accept_invalid_certs(insecure)
        .build()
        .map_err(|_| "Client HTTP indisponible")
}
fn request(config: &Config, token: &str, path: &str, body: Value) -> Result<(), &'static str> {
    let url = endpoint(&config.url, path)?;
    let response = http(config.insecure)?
        .post(url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .map_err(|_| "Hors ligne")?;
    match response.status().as_u16() {
        200..=299 => Ok(()),
        401 | 403 => Err("Accès refusé"),
        _ => Err("Erreur serveur"),
    }
}

// ------------------------------------------------------------ État automatique

#[derive(Debug, PartialEq, Eq)]
enum Out {
    State(Option<bool>),
    Trigger { edge: &'static str, initial: bool },
}
struct Auto {
    rule: Result<Rule, &'static str>,
    machine: Machine,
    /// Dernier état publié : `None` = rien publié ; `Some(None)` = « inconnu » publié.
    published: Option<Option<bool>>,
    /// Après une modification des réglages : pas de déclenchement « au démarrage ».
    suppress_initial: bool,
}
impl Auto {
    fn new(settings: &Value) -> Self {
        Self {
            rule: Rule::from_settings(settings),
            machine: Machine::default(),
            published: None,
            suppress_initial: false,
        }
    }
    /// `lookup` : `None` = déconnecté ; `Some(None)` = entité absente.
    fn step(&mut self, now: Instant, lookup: Option<Option<Entity>>) -> Vec<Out> {
        let mut out = Vec::new();
        let state = match &self.rule {
            Err(_) => None,
            Ok(rule) => {
                let value = lookup
                    .as_ref()
                    .and_then(|e| e.as_ref())
                    .map(|e| e.value(&rule.attribute));
                let reading = match &value {
                    None => Reading::Unknown,
                    Some(v) => Reading::Known(v.as_deref()),
                };
                let edge = self.machine.step(rule, now, reading);
                let state = self.machine.state(reading);
                if state != self.published.flatten() || self.published.is_none() {
                    out.push(Out::State(state));
                    self.published = Some(state);
                }
                match edge {
                    Some(Edge::Active { initial }) if !(initial && self.suppress_initial) => out
                        .push(Out::Trigger {
                            edge: "active",
                            initial,
                        }),
                    Some(Edge::Inactive) => out.push(Out::Trigger {
                        edge: "inactive",
                        initial: false,
                    }),
                    _ => {}
                }
                return out;
            }
        };
        if self.published != Some(state) {
            out.push(Out::State(state));
            self.published = Some(state);
        }
        out
    }
}

// ------------------------------------------------------------ plugin

#[derive(Clone, Copy)]
enum Job {
    Test,
    Toggle,
    Service,
}
struct Reply {
    context: String,
    job: Job,
    result: Result<usize, String>,
}
struct Key {
    action: String,
    settings: Value,
    pending: bool,
    /// Dernier libellé reçu/affiché (erreur d'action ou état en direct).
    label: String,
    auto: Option<Auto>,
}
struct Home {
    keys: HashMap<String, Key>,
    inspectors: HashSet<String>,
    tx: Sender<Reply>,
    rx: Receiver<Reply>,
    config: Config,
    has_token: bool,
    monitor: Monitor,
    last_status: String,
    last_tick: Instant,
}
impl Home {
    fn with(config: Config, monitor: Monitor) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            keys: HashMap::new(),
            inspectors: HashSet::new(),
            tx,
            rx,
            config,
            has_token: false,
            monitor,
            last_status: String::new(),
            last_tick: Instant::now(),
        }
    }
}
impl Default for Home {
    fn default() -> Self {
        let mut home = Self::with(load_config(), Monitor::spawn());
        home.reconfigure();
        home
    }
}

fn status_code(config: &Config, has_token: bool, status: &Status) -> &'static str {
    if !config.configured() {
        "not_configured"
    } else if !has_token {
        "no_token"
    } else {
        match status {
            Status::Connected => "connected",
            Status::AuthFailed(_) => "auth_failed",
            Status::Reconnecting { .. } => "reconnecting",
            Status::Connecting | Status::Disabled => "connecting",
        }
    }
}
fn status_message(code: &str, status: &Status) -> String {
    match (code, status) {
        ("not_configured", _) => "Configure l'URL et le jeton dans les réglages du plugin".into(),
        ("no_token", _) => {
            "Enregistre le jeton d'accès Home Assistant dans les réglages du plugin".into()
        }
        ("auth_failed", Status::AuthFailed(m)) => m.clone(),
        ("reconnecting", Status::Reconnecting { error, .. }) => error.clone(),
        ("connecting", _) => "Connexion à Home Assistant…".into(),
        _ => String::new(),
    }
}

impl Home {
    /// Applique URL/jeton courants à la connexion unique (jamais d'accès réseau ici).
    fn reconfigure(&mut self) {
        let token = load_token().ok().flatten();
        self.has_token = token.is_some();
        let settings = match token {
            Some(token) if self.config.configured() && endpoint(&self.config.url, "/").is_ok() => {
                Some(Settings {
                    url: self.config.url.clone(),
                    token,
                    insecure: self.config.insecure,
                })
            }
            _ => None,
        };
        self.monitor.configure(settings);
    }
    fn adopt(&mut self, key_settings: &Value) {
        if let Some(config) = migrated(&self.config, key_settings) {
            if store_config(&config).is_ok() {
                self.config = config;
                self.reconfigure();
            }
        }
    }
    fn status_payload(&self) -> Value {
        let status = self.monitor.status();
        let code = status_code(&self.config, self.has_token, &status);
        json!({"type":"status", "code":code, "message":status_message(code, &status), "connected":code == "connected",
            "configured":self.config.configured(), "hasToken":self.has_token,
            "url":self.config.url, "insecure":self.config.insecure})
    }
    fn send_status(&self, ctx: &str, sdk: &mut Client) -> Result<(), String> {
        sdk.inspector(ctx, self.status_payload())
    }
    fn list_payload(&self) -> Value {
        let status = self.monitor.status();
        let code = status_code(&self.config, self.has_token, &status);
        if code != "connected" {
            return json!({"op":"list", "type":"list", "status":"error", "code":code, "message":status_message(code, &status), "entities":[]});
        }
        let entities: Vec<Value> = self
            .monitor
            .entity_list()
            .into_iter()
            .filter(|(id, _)| entity(id))
            .take(MAX_LIST)
            .map(|(id, e)| {
                let name = e.friendly_name().filter(|n| !n.is_empty()).unwrap_or(&id).to_owned();
                let domain = id.split('.').next().unwrap_or("").to_owned();
                json!({"id":id, "name":name, "domain":domain, "state":e.state.chars().take(64).collect::<String>(), "unit":e.unit()})
            })
            .collect();
        json!({"op":"list", "type":"list", "status":"ok", "code":"ok", "message":"", "entities":entities})
    }
    fn draw(&self, ctx: &str, sdk: &mut Client) -> Result<(), String> {
        if let Some(key) = self.keys.get(ctx) {
            if key.action == AUTO {
                return Ok(());
            }
            let title = if key.label.is_empty() {
                "Home Assistant"
            } else {
                &key.label
            };
            sdk.title(ctx, title)?;
            let lines = [
                if key.action == TOGGLE {
                    "Maison"
                } else if key.action == SENSOR {
                    "État"
                } else {
                    "Service"
                },
                title,
            ];
            let color = if key.label == "on" {
                Some(1.0)
            } else if key.label == "off" {
                Some(0.0)
            } else {
                None
            };
            sdk.image(ctx, &novydeck_sdk::render(&lines, color)?)?;
        }
        Ok(())
    }
    fn live_label(&self, key: &Key) -> Option<String> {
        let id = key.settings["entity"].as_str().unwrap_or("");
        if !entity(id) {
            return None;
        }
        Some(match self.monitor.lookup(id) {
            Some(Some(e)) => state_label(&e),
            Some(None) => "Introuvable".into(),
            None => match self.monitor.status() {
                Status::AuthFailed(_) => "Accès refusé".into(),
                Status::Disabled => "Non configuré".into(),
                _ => "Hors ligne".into(),
            },
        })
    }
    fn schedule(&mut self, ctx: &str, job: Job, test: Option<Config>) {
        let Some(key) = self.keys.get_mut(ctx) else {
            return;
        };
        if key.pending {
            return;
        }
        key.pending = true;
        let settings = key.settings.clone();
        let config = test.unwrap_or_else(|| self.config.clone());
        let sender = self.tx.clone();
        let context = ctx.to_owned();
        thread::spawn(move || {
            let result = (|| -> Result<usize, String> {
                let token = load_token().map_err(str::to_owned)?.ok_or("Non connecté")?;
                match job {
                    Job::Test => live::test_connection(
                        &Settings {
                            url: config.url,
                            token,
                            insecure: config.insecure,
                        },
                        Duration::from_secs(8),
                    ),
                    Job::Toggle => {
                        let body = service_body(&settings)?;
                        request(&config, &token, "/api/services/homeassistant/toggle", body)
                            .map(|_| 0)
                            .map_err(str::to_owned)
                    }
                    Job::Service => {
                        let domain = settings["domain"].as_str().unwrap_or("");
                        let service = settings["service"].as_str().unwrap_or("");
                        if !identifier(domain) || !identifier(service) {
                            return Err("Service invalide".into());
                        }
                        let body = service_body(&settings)?;
                        request(
                            &config,
                            &token,
                            &format!("/api/services/{domain}/{service}"),
                            body,
                        )
                        .map(|_| 0)
                        .map_err(str::to_owned)
                    }
                }
            })();
            let _ = sender.send(Reply {
                context,
                job,
                result,
            });
        });
    }
    fn insert_key(&mut self, ctx: &str, action: &str, raw: &Value) {
        let settings = safe_settings(raw);
        let auto = (action == AUTO).then(|| Auto::new(&settings));
        self.keys.insert(
            ctx.to_owned(),
            Key {
                action: action.to_owned(),
                settings,
                pending: false,
                label: String::new(),
                auto,
            },
        );
    }
}

impl Plugin for Home {
    fn event(&mut self, event: Event, sdk: &mut Client) -> Result<(), String> {
        let ctx = event.context.clone();
        match event.name.as_str() {
            "willAppear" => {
                self.adopt(event.settings());
                self.insert_key(&ctx, &event.action, event.settings());
                self.draw(&ctx, sdk)?;
            }
            "willDisappear" => {
                self.keys.remove(&ctx);
            }
            "didReceiveSettings" => {
                self.adopt(event.settings());
                let settings = safe_settings(event.settings());
                if let Some(key) = self.keys.get_mut(&ctx) {
                    if key.action == AUTO && key.settings != settings {
                        let mut auto = Auto::new(&settings);
                        auto.suppress_initial = true;
                        auto.published = key.auto.as_ref().and_then(|a| a.published);
                        key.auto = Some(auto);
                    }
                    key.settings = settings;
                }
                self.draw(&ctx, sdk)?;
            }
            "propertyInspectorDidAppear" => {
                self.inspectors.insert(ctx.clone());
                self.send_status(&ctx, sdk)?;
            }
            "propertyInspectorDidDisappear" => {
                self.inspectors.remove(&ctx);
            }
            "sendToPlugin" => {
                let op = event.payload["op"].as_str().unwrap_or("");
                match op {
                    "saveToken" | "saveConnection" => {
                        if op == "saveConnection" {
                            let config = Config::from_value(&event.payload);
                            if config.url.is_empty() || endpoint(&config.url, "/").is_ok() {
                                if config != self.config && store_config(&config).is_ok() {
                                    self.config = config;
                                }
                            }
                        }
                        let value = event.payload["token"].as_str().unwrap_or("");
                        if !value.is_empty()
                            && value.len() <= 8_192
                            && !value.contains('\n')
                            && !value.contains('\r')
                        {
                            let _ = store_token(value);
                        }
                        self.reconfigure();
                        self.send_status(&ctx, sdk)?;
                    }
                    "status" => self.send_status(&ctx, sdk)?,
                    "list" => sdk.inspector(&ctx, self.list_payload())?,
                    "test" => {
                        // Teste l'adresse saisie (même non enregistrée) avec le jeton enregistré.
                        let candidate = Config::from_value(&event.payload);
                        let candidate = if candidate.configured() {
                            candidate
                        } else {
                            self.config.clone()
                        };
                        if endpoint(&candidate.url, "/").is_err() {
                            sdk.inspector(
                                &ctx,
                                json!({"type":"test", "ok":false, "message":"URL invalide"}),
                            )?;
                        } else {
                            self.schedule(&ctx, Job::Test, Some(candidate));
                        }
                    }
                    _ => {}
                }
            }
            "keyUp" => {
                if let Some(key) = self.keys.get(&ctx) {
                    match key.action.as_str() {
                        TOGGLE => self.schedule(&ctx, Job::Toggle, None),
                        SERVICE => self.schedule(&ctx, Job::Service, None),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn tick(&mut self, sdk: &mut Client) -> Result<(), String> {
        while let Ok(reply) = self.rx.try_recv() {
            let Some(key) = self.keys.get_mut(&reply.context) else {
                continue;
            };
            key.pending = false;
            match (reply.job, reply.result) {
                (Job::Test, Ok(count)) => sdk.inspector(
                    &reply.context,
                    json!({"type":"test", "ok":true, "count":count}),
                )?,
                (Job::Test, Err(message)) => sdk.inspector(
                    &reply.context,
                    json!({"type":"test", "ok":false, "message":message}),
                )?,
                (_, Ok(_)) => sdk.ok(&reply.context)?,
                (_, Err(message)) => {
                    key.label = message;
                    let context = reply.context.clone();
                    self.draw(&context, sdk)?;
                    sdk.alert(&context)?;
                }
            }
        }
        // Connexion en direct : au plus un passage toutes les 200 ms (cadence de l'hôte).
        let now = Instant::now();
        self.last_tick = now;
        let contexts: Vec<String> = self.keys.keys().cloned().collect();
        for ctx in contexts {
            let Some(key) = self.keys.get(&ctx) else {
                continue;
            };
            if key.action == AUTO {
                let lookup = key.settings["entity"]
                    .as_str()
                    .and_then(|id| self.monitor.lookup(id));
                let Some(auto) = self.keys.get_mut(&ctx).and_then(|k| k.auto.as_mut()) else {
                    continue;
                };
                for out in auto.step(now, lookup) {
                    match out {
                        Out::State(Some(on)) => sdk.state(&ctx, on, StateSource::Confirmed)?,
                        Out::State(None) => sdk.state_unknown(&ctx)?,
                        Out::Trigger { edge, initial } => {
                            sdk.send(json!({"event":"novydeckTrigger", "context":ctx, "payload":{"edge":edge, "initial":initial}}))?
                        }
                    }
                }
            } else if key.action != SERVICE {
                if let Some(label) = self.live_label(key) {
                    if label != key.label {
                        if let Some(key) = self.keys.get_mut(&ctx) {
                            key.label = label;
                        }
                        self.draw(&ctx, sdk)?;
                    }
                }
            }
        }
        // Inspecteurs ouverts : l'état de la connexion se met à jour sans rechargement.
        let payload = self.status_payload();
        let signature = payload.to_string();
        if signature != self.last_status {
            self.last_status = signature;
            for ctx in self.inspectors.clone() {
                sdk.inspector(&ctx, payload.clone())?;
            }
        }
        Ok(())
    }
}
fn main() {
    if let Err(e) = novydeck_sdk::run(Home::default()) {
        eprintln!("homeassistant: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ent(state: &str) -> Entity {
        Entity {
            state: state.into(),
            attributes: json!({"unit_of_measurement":"°C", "friendly_name":"Salon"}),
        }
    }
    #[test]
    fn url_validation() {
        assert_eq!(
            endpoint("http://homeassistant.local:8123", "/api/")
                .unwrap()
                .as_str(),
            "http://homeassistant.local:8123/api/"
        );
        for url in [
            "ftp://host",
            "https://user:pass@host/",
            "http://host/path",
            "http://host/?x=1",
        ] {
            assert!(endpoint(url, "/api/").is_err());
        }
    }
    #[test]
    fn service_validation() {
        assert_eq!(
            service_body(&json!({"entity":"light.salon", "data":"{\"brightness\":50,\"entity_id\":\"switch.other\"}"})).unwrap(),
            json!({"brightness":50,"entity_id":"light.salon"})
        );
        assert!(service_body(&json!({"entity":"../admin", "data":"{}"})).is_err());
        assert!(service_body(&json!({"entity":"light.salon", "data":"[]"})).is_err());
    }
    #[test]
    fn label_and_settings_never_carry_a_token() {
        assert_eq!(state_label(&ent("19.5")), "19.5 °C");
        let safe = safe_settings(&json!({"token":"secret", "url":"http://ha:8123", "refresh":0}));
        assert!(safe.get("token").is_none());
        assert_eq!(safe["refresh"], 2);
        assert_eq!(safe["url"], "http://ha:8123");
    }
    #[test]
    fn migration_from_per_key_settings() {
        let none = Config::default();
        let old = json!({"url":" http://ha.local:8123 ", "insecure":true, "entity":"light.a"});
        assert_eq!(
            migrated(&none, &old),
            Some(Config {
                url: "http://ha.local:8123".into(),
                insecure: true
            })
        );
        assert_eq!(migrated(&none, &json!({"entity":"light.a"})), None);
        assert_eq!(
            migrated(&none, &json!({"url":"ftp://x"})),
            None,
            "URL invalide non migrée"
        );
        let set = Config {
            url: "http://other:8123".into(),
            insecure: false,
        };
        assert_eq!(
            migrated(&set, &old),
            None,
            "la configuration globale existante n'est jamais écrasée"
        );
    }
    #[test]
    fn status_messages_explain_the_cause() {
        let none = Config::default();
        assert_eq!(
            status_code(&none, false, &Status::Disabled),
            "not_configured"
        );
        assert!(status_message("not_configured", &Status::Disabled)
            .contains("Configure l'URL et le jeton"));
        let cfg = Config {
            url: "http://ha:8123".into(),
            insecure: false,
        };
        assert_eq!(status_code(&cfg, false, &Status::Disabled), "no_token");
        assert_eq!(
            status_code(&cfg, true, &Status::AuthFailed("x".into())),
            "auth_failed"
        );
        assert_eq!(
            status_code(
                &cfg,
                true,
                &Status::Reconnecting {
                    attempt: 1,
                    error: "e".into()
                }
            ),
            "reconnecting"
        );
        assert_eq!(status_code(&cfg, true, &Status::Connected), "connected");
    }
    #[test]
    fn disconnected_list_is_an_explained_error_and_never_has_a_token() {
        let home = Home::with(Config::default(), Monitor::inert());
        let list = home.list_payload();
        assert_eq!(list["status"], "error");
        assert_eq!(list["entities"], json!([]));
        assert!(!list["message"].as_str().unwrap().is_empty());
        assert!(
            !home.status_payload().to_string().contains("token\":\""),
            "aucun champ jeton"
        );
    }

    fn auto(settings: Value) -> (Auto, Instant) {
        (Auto::new(&safe_settings(&settings)), Instant::now())
    }
    fn at(t0: Instant, s: u64) -> Instant {
        t0 + Duration::from_secs(s)
    }
    fn on(state: &str) -> Option<Option<Entity>> {
        Some(Some(Entity {
            state: state.into(),
            attributes: json!({}),
        }))
    }
    #[test]
    fn auto_publishes_state_unknown_and_edges_only_on_real_changes() {
        let (mut a, t0) = auto(
            json!({"entity":"binary_sensor.presence","op":"eq","value":"off","hold_seconds":"30"}),
        );
        assert_eq!(
            a.step(at(t0, 0), None),
            vec![Out::State(None)],
            "déconnecté : inconnu"
        );
        assert_eq!(a.step(at(t0, 1), None), vec![], "pas de répétition");
        assert_eq!(a.step(at(t0, 2), on("on")), vec![Out::State(Some(false))]);
        assert_eq!(a.step(at(t0, 3), on("off")), vec![]);
        assert_eq!(
            a.step(at(t0, 33), on("off")),
            vec![
                Out::State(Some(true)),
                Out::Trigger {
                    edge: "active",
                    initial: false
                }
            ]
        );
        assert_eq!(a.step(at(t0, 34), on("off")), vec![]);
        // Coupure puis retour dans le même état : inconnu puis état réel, sans transition.
        assert_eq!(a.step(at(t0, 40), None), vec![Out::State(None)]);
        assert_eq!(a.step(at(t0, 41), on("off")), vec![Out::State(Some(true))]);
        assert_eq!(
            a.step(at(t0, 42), on("on")),
            vec![
                Out::State(Some(false)),
                Out::Trigger {
                    edge: "inactive",
                    initial: false
                }
            ]
        );
    }
    #[test]
    fn auto_already_true_at_start_has_state_one_without_trigger_unless_fire_at_start() {
        let (mut a, t0) = auto(json!({"entity":"light.a","value":"on"}));
        assert_eq!(a.step(at(t0, 0), on("on")), vec![Out::State(Some(true))]);
        let (mut b, t0) = auto(json!({"entity":"light.a","value":"on","fire_at_start":true}));
        assert_eq!(
            b.step(at(t0, 0), on("on")),
            vec![
                Out::State(Some(true)),
                Out::Trigger {
                    edge: "active",
                    initial: true
                }
            ]
        );
        // Modification des réglages dans l'inspecteur : jamais de déclenchement parasite.
        let (mut c, t0) = auto(json!({"entity":"light.a","value":"on","fire_at_start":true}));
        c.suppress_initial = true;
        assert_eq!(c.step(at(t0, 0), on("on")), vec![Out::State(Some(true))]);
    }
    #[test]
    fn auto_with_invalid_rule_or_missing_entity_is_unknown() {
        let (mut a, t0) = auto(json!({"entity":"../x","value":"on"}));
        assert_eq!(a.step(at(t0, 0), on("on")), vec![Out::State(None)]);
        let (mut b, t0) = auto(json!({"entity":"light.a","value":"on"}));
        assert_eq!(
            b.step(at(t0, 0), Some(None)),
            vec![Out::State(None)],
            "entité absente"
        );
        let (mut c, t0) =
            auto(json!({"entity":"light.a","attribute":"brightness","op":"gt","value":"100"}));
        let with_attr = Some(Some(Entity {
            state: "on".into(),
            attributes: json!({"brightness": 200}),
        }));
        assert_eq!(c.step(at(t0, 0), with_attr), vec![Out::State(Some(true))]);
    }
}

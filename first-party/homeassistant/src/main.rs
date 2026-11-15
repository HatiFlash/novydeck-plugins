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
use std::sync::Arc;
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};
use store::{
    is_cleartext, load_config, load_groups, origin_of, plan_secret, store_config, store_groups,
    test_credentials, url_is_local, Config, DpapiVault, Group, Secret, SecretPlan, Vault,
    INSECURE_REFUSED, MAX_GROUPS, MAX_MEMBERS, MISMATCH,
};

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
        "data": text("data"), "command": text("command"), "group": text("group"), "refresh": settings["refresh"].as_u64().unwrap_or(10).clamp(2, 300),
        "insecure": settings["insecure"].as_bool().unwrap_or(false),
        "attribute": text("attribute"), "op": text("op"), "value": text("value"),
        "hold_seconds": settings["hold_seconds"].clone(), "fire_at_start": settings["fire_at_start"].as_bool().unwrap_or(false)})
}
#[cfg(test)]
fn service_body(settings: &Value) -> Result<Value, &'static str> {
    let id = settings["entity"].as_str().unwrap_or("");
    service_body_for(settings, &[id.to_owned()])
}
/// Corps d'appel pour une ou plusieurs entités (groupe). Chaque identifiant est validé ;
/// une seule entité reste une chaîne (comportement inchangé des touches existantes), plusieurs
/// donnent un tableau que Home Assistant accepte tel quel.
fn service_body_for(settings: &Value, ids: &[String]) -> Result<Value, &'static str> {
    if ids.is_empty() || ids.len() > MAX_MEMBERS {
        return Err("Groupe invalide");
    }
    let mut unique: Vec<&str> = Vec::new();
    for id in ids {
        if !entity(id) {
            return Err("Entité invalide");
        }
        if !unique.contains(&id.as_str()) {
            unique.push(id);
        }
    }
    let text = settings["data"].as_str().unwrap_or("").trim();
    let mut body: Value = if text.is_empty() {
        json!({})
    } else {
        serde_json::from_str(text).map_err(|_| "JSON invalide")?
    };
    let obj = body.as_object_mut().ok_or("JSON objet requis")?;
    // La cible choisie dans l'inspecteur prime toujours sur les données avancées.
    obj.insert(
        "entity_id".into(),
        if unique.len() == 1 {
            json!(unique[0])
        } else {
            json!(unique)
        },
    );
    Ok(body)
}
/// Cibles d'une touche : le groupe enregistré s'il est choisi, sinon l'entité unique.
fn resolve_targets(settings: &Value, groups: &[Group]) -> Result<Vec<String>, &'static str> {
    let group = settings["group"].as_str().unwrap_or("");
    if !group.is_empty() {
        return groups
            .iter()
            .find(|g| g.id == group)
            .map(|g| g.entities.clone())
            .ok_or("Groupe introuvable");
    }
    let id = settings["entity"].as_str().unwrap_or("");
    if entity(id) {
        Ok(vec![id.to_owned()])
    } else {
        Err("Entité invalide")
    }
}
type Lookup = Option<Option<Entity>>;
#[derive(Debug, PartialEq)]
enum GroupState {
    /// Home Assistant non connecté : aucun état fiable.
    Offline,
    /// Connecté mais aucune entité du groupe n'existe.
    Missing,
    On,
    Off,
}
/// Sémantique de groupe : au moins une entité allumée (`on`) → le groupe est « on ».
fn group_state(states: &[Lookup]) -> GroupState {
    if states.is_empty() || states.iter().any(Option::is_none) {
        return GroupState::Offline;
    }
    if states.iter().flatten().all(Option::is_none) {
        return GroupState::Missing;
    }
    if states.iter().flatten().flatten().any(|e| e.state == "on") {
        GroupState::On
    } else {
        GroupState::Off
    }
}
/// Basculer un groupe : au moins une allumée → tout éteindre, sinon tout allumer.
fn group_toggle_service(states: &[Lookup]) -> Result<&'static str, &'static str> {
    match group_state(states) {
        GroupState::Offline => Err("États indisponibles"),
        GroupState::Missing => Err("Entités introuvables"),
        GroupState::On => Ok("turn_off"),
        GroupState::Off => Ok("turn_on"),
    }
}
/// Ce que la touche doit envoyer : cibles et, pour une bascule de groupe, le service décidé
/// d'après les états actuels (domaine `homeassistant` : fonctionne pour des domaines mélangés).
#[derive(Debug, PartialEq)]
struct Plan {
    ids: Vec<String>,
    domain_service: Option<(&'static str, &'static str)>,
}
/// Touche « Basculer une entité » (1.2.1) : `command` = identifiant du préréglage choisi dans
/// l'inspecteur. Absent ou `toggle` = basculement historique (les éventuels domain/service
/// résiduels des anciennes touches sont ignorés) ; toute autre valeur = appel du service
/// domain/service enregistré avec le préréglage.
fn toggle_runs_service(settings: &Value) -> bool {
    let command = settings["command"].as_str().unwrap_or("").trim();
    !command.is_empty() && command != "toggle"
}
/// Chemin d'API et corps de la requête pour une touche Basculer / Appeler un service.
fn service_call(job: Job, settings: &Value, plan: &Plan) -> Result<(String, Value), &'static str> {
    let body = service_body_for(settings, &plan.ids)?;
    if job == Job::Toggle && !toggle_runs_service(settings) {
        let path = match plan.domain_service {
            Some((d, svc)) => format!("/api/services/{d}/{svc}"),
            None => "/api/services/homeassistant/toggle".to_owned(),
        };
        return Ok((path, body));
    }
    let (mut domain, mut service) = (
        settings["domain"].as_str().unwrap_or(""),
        settings["service"].as_str().unwrap_or(""),
    );
    if !identifier(domain) || !identifier(service) {
        return Err("Service invalide");
    }
    if let Some((d, svc)) = plan.domain_service {
        (domain, service) = (d, svc);
    }
    Ok((format!("/api/services/{domain}/{service}"), body))
}
fn plan(
    job: Job,
    settings: &Value,
    groups: &[Group],
    lookup: &dyn Fn(&str) -> Lookup,
) -> Result<Plan, &'static str> {
    let ids = resolve_targets(settings, groups)?;
    let is_group = !settings["group"].as_str().unwrap_or("").is_empty();
    let toggle = match job {
        Job::Toggle => !toggle_runs_service(settings),
        Job::Service => {
            settings["domain"].as_str() == Some("homeassistant")
                && settings["service"].as_str() == Some("toggle")
        }
        Job::Test => false,
    };
    let domain_service = if is_group && toggle {
        let states: Vec<Lookup> = ids.iter().map(|id| lookup(id)).collect();
        Some(("homeassistant", group_toggle_service(&states)?))
    } else {
        None
    };
    Ok(Plan {
        ids,
        domain_service,
    })
}
/// Indices pour les actions prédéfinies de l'inspecteur (1.2.0) : options d'une liste, modes
/// de climatisation, bornes d'un nombre. Valeurs bornées et filtrées ; rien d'autre n'est relayé.
fn preset_hints(domain: &str, e: &Entity) -> Vec<(&'static str, Value)> {
    let texts = |key: &str| -> Option<Value> {
        let list: Vec<Value> = e.attributes[key]
            .as_array()?
            .iter()
            .filter_map(Value::as_str)
            .filter(|s| !s.is_empty() && s.chars().count() <= 128)
            .take(100)
            .map(|s| json!(s))
            .collect();
        (!list.is_empty()).then_some(Value::Array(list))
    };
    let number = |key: &str| {
        e.attributes[key]
            .as_f64()
            .filter(|n| n.is_finite())
            .map(|n| json!(n))
    };
    let mut out = Vec::new();
    match domain {
        "select" | "input_select" => out.extend(texts("options").map(|v| ("options", v))),
        "climate" => out.extend(texts("hvac_modes").map(|v| ("hvac_modes", v))),
        "number" | "input_number" => {
            out.extend(number("min").map(|v| ("min", v)));
            out.extend(number("max").map(|v| ("max", v)));
            out.extend(number("step").map(|v| ("step", v)));
        }
        _ => {}
    }
    out
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

/// URL et option TLS étaient par touche en 1.0.x : on collecte les origines vues, sans rien
/// appliquer. Une seule origine distincte devient une suggestion que seule `saveConnection`
/// (action de l'utilisateur) peut appliquer. Les réglages des touches restent intacts.
fn observed(key_settings: &Value) -> Option<(String, Config)> {
    let candidate = Config::from_value(key_settings);
    if !candidate.configured() || endpoint(&candidate.url, "/").is_err() {
        return None;
    }
    Some((origin_of(&candidate.url).ok()?, candidate))
}
fn suggestion(config: &Config, seen: &HashMap<String, Config>) -> Option<Config> {
    if config.configured() || seen.len() != 1 {
        return None;
    }
    seen.values().next().cloned()
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
fn request(config: &Config, secret: &Secret, path: &str, body: Value) -> Result<(), &'static str> {
    let url = endpoint(&config.url, path)?;
    // Refus avant tout accès réseau : le jeton ne part que vers son origine liée.
    let token = secret.token_for(&config.url)?;
    if config.insecure && !url_is_local(&config.url) {
        return Err(INSECURE_REFUSED);
    }
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

#[derive(Clone, Copy, Debug, PartialEq)]
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
    vault: Arc<dyn Vault>,
    /// Écriture de `config.json` (injectable : hors Windows les tests n'ont pas de dossier de données).
    persist: fn(&Config) -> Result<(), &'static str>,
    /// Groupes d'entités partagés par toutes les touches (`groups.json`).
    groups: Vec<Group>,
    persist_groups: fn(&[Group]) -> Result<(), &'static str>,
    has_token: bool,
    /// Jeton présent mais lié à une autre adresse que la connexion courante.
    mismatch: bool,
    /// Origines vues dans d'anciens réglages de touche (1.0.x), jamais appliquées seules.
    seen: HashMap<String, Config>,
    /// Dernier refus de `saveConnection`, affiché par l'inspecteur.
    save_error: String,
    monitor: Monitor,
    last_status: String,
    last_tick: Instant,
}
impl Home {
    fn with(config: Config, monitor: Monitor, vault: Arc<dyn Vault>) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            keys: HashMap::new(),
            inspectors: HashSet::new(),
            tx,
            rx,
            config,
            vault,
            persist: store_config,
            groups: Vec::new(),
            persist_groups: store_groups,
            has_token: false,
            mismatch: false,
            seen: HashMap::new(),
            save_error: String::new(),
            monitor,
            last_status: String::new(),
            last_tick: Instant::now(),
        }
    }
}
impl Default for Home {
    fn default() -> Self {
        let mut home = Self::with(load_config(), Monitor::spawn(), Arc::new(DpapiVault));
        home.groups = load_groups();
        home.reconfigure();
        home
    }
}

fn status_code(
    config: &Config,
    has_token: bool,
    mismatch: bool,
    suggested: bool,
    status: &Status,
) -> &'static str {
    if !config.configured() {
        if suggested {
            "suggestion"
        } else {
            "not_configured"
        }
    } else if mismatch {
        "token_mismatch"
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
        ("suggestion", _) => {
            "Adresse détectée dans d'anciens réglages : vérifiez-la puis enregistrez la connexion"
                .into()
        }
        ("token_mismatch", _) => MISMATCH.into(),
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
    /// Lit le secret ; un secrets.bin 1.1.0 (jeton seul) est lié une fois à l'origine courante.
    fn load_secret(&self) -> Option<Secret> {
        let mut secret = self.vault.load().ok().flatten()?;
        if secret.origin.is_none() && self.config.configured() {
            if let Ok(origin) = origin_of(&self.config.url) {
                secret.origin = Some(origin);
                let _ = self.vault.save(&secret);
            }
        }
        Some(secret)
    }
    /// Applique URL/jeton courants à la connexion unique (jamais d'accès réseau ici).
    fn reconfigure(&mut self) {
        let secret = self.load_secret();
        self.has_token = secret.is_some();
        self.mismatch = false;
        let mut settings = None;
        if let Some(secret) = secret {
            if self.config.configured() && endpoint(&self.config.url, "/").is_ok() {
                match secret.token_for(&self.config.url) {
                    Ok(token) => {
                        settings = Some(Settings {
                            url: self.config.url.clone(),
                            token: token.to_owned(),
                            insecure: self.config.insecure,
                            bound_origin: secret.origin.clone().unwrap_or_default(),
                        })
                    }
                    Err(_) => self.mismatch = true,
                }
            }
        }
        self.monitor.configure(settings);
    }
    /// Enregistre une origine vue dans un ancien réglage de touche (aucune application).
    fn observe(&mut self, key_settings: &Value) {
        if self.config.configured() {
            return;
        }
        if let Some((origin, config)) = observed(key_settings) {
            self.seen.entry(origin).or_insert(config);
        }
    }
    /// Action de l'utilisateur : enregistre l'adresse et lie/invalide le jeton en conséquence.
    fn save_connection(&mut self, payload: &Value, with_url: bool) -> Result<(), &'static str> {
        let config = if with_url {
            Config::from_value(payload)
        } else {
            self.config.clone()
        };
        if config.configured() && endpoint(&config.url, "/").is_err() {
            return Err("URL invalide");
        }
        if config.insecure && config.configured() && !url_is_local(&config.url) {
            return Err(INSECURE_REFUSED);
        }
        let value = payload["token"].as_str().unwrap_or("");
        let provided = (!value.is_empty()
            && value.len() <= 8_192
            && !value.contains('\n')
            && !value.contains('\r'))
        .then_some(value);
        let origin = if config.configured() {
            origin_of(&config.url).ok()
        } else {
            None
        };
        let current = self.vault.load().ok().flatten();
        match plan_secret(current.as_ref(), origin.as_deref(), provided) {
            SecretPlan::Keep => {}
            SecretPlan::Replace(secret) => self.vault.save(&secret)?,
            SecretPlan::Clear => self.vault.clear()?,
        }
        if config != self.config {
            (self.persist)(&config)?;
            self.config = config;
        }
        Ok(())
    }
    fn status_payload(&self) -> Value {
        let status = self.monitor.status();
        let suggested = suggestion(&self.config, &self.seen);
        let code = status_code(
            &self.config,
            self.has_token,
            self.mismatch,
            suggested.is_some(),
            &status,
        );
        let cleartext = if !is_cleartext(&self.config.url) {
            "none"
        } else if url_is_local(&self.config.url) {
            "local"
        } else {
            "public"
        };
        let mut payload = json!({"type":"status", "code":code, "message":status_message(code, &status), "connected":code == "connected",
            "configured":self.config.configured(), "hasToken":self.has_token && !self.mismatch,
            "tokenMismatch":self.mismatch, "cleartext":cleartext,
            "insecureRefused": self.config.insecure && self.config.configured() && !url_is_local(&self.config.url),
            "saveError":self.save_error,
            "url":self.config.url, "insecure":self.config.insecure});
        if let Some(s) = suggested {
            payload["suggestion"] = json!({"url":s.url, "insecure":s.insecure});
        }
        payload
    }
    fn send_status(&self, ctx: &str, sdk: &mut Client) -> Result<(), String> {
        sdk.inspector(ctx, self.status_payload())
    }
    fn list_payload(&self) -> Value {
        let status = self.monitor.status();
        let code = status_code(&self.config, self.has_token, self.mismatch, false, &status);
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
                let mut item = json!({"id":id, "name":name, "domain":domain, "state":e.state.chars().take(64).collect::<String>(), "unit":e.unit()});
                for (k, v) in preset_hints(&domain, &e) {
                    item[k] = v;
                }
                item
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
    fn groups_payload(&self, saved: Option<&str>, error: Option<&str>) -> Value {
        let list: Vec<Value> = self.groups.iter().map(Group::to_value).collect();
        let mut v = json!({"type":"groups", "groups":list});
        if let Some(id) = saved {
            v["saved"] = json!(id);
        }
        if let Some(e) = error {
            v["error"] = json!(e);
        }
        v
    }
    /// Crée ou modifie un groupe (identifiant généré à la création). Retourne son identifiant.
    fn save_group(&mut self, payload: &Value) -> Result<String, &'static str> {
        let given = payload["id"].as_str().unwrap_or("");
        let id = if given.is_empty() {
            if self.groups.len() >= MAX_GROUPS {
                return Err("Maximum 50 groupes");
            }
            let n = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            format!("g{n:x}")
        } else {
            given.to_owned()
        };
        let group = Group::from_value(&json!({
            "id": id, "name": payload["name"], "entities": payload["entities"]
        }))?;
        if self
            .groups
            .iter()
            .any(|g| g.id != group.id && g.name.to_lowercase() == group.name.to_lowercase())
        {
            return Err("Nom de groupe déjà utilisé");
        }
        let mut next = self.groups.clone();
        match next.iter_mut().find(|g| g.id == group.id) {
            Some(slot) => *slot = group.clone(),
            None if !given.is_empty() => return Err("Groupe introuvable"),
            None => next.push(group.clone()),
        }
        (self.persist_groups)(&next)?;
        self.groups = next;
        Ok(group.id)
    }
    fn delete_group(&mut self, id: &str) -> Result<(), &'static str> {
        if !self.groups.iter().any(|g| g.id == id) {
            return Err("Groupe introuvable");
        }
        let next: Vec<Group> = self.groups.iter().filter(|g| g.id != id).cloned().collect();
        (self.persist_groups)(&next)?;
        self.groups = next;
        Ok(())
    }
    fn group_label(&self, group: &str) -> String {
        let Some(g) = self.groups.iter().find(|g| g.id == group) else {
            return "Groupe introuvable".into();
        };
        let states: Vec<Lookup> = g
            .entities
            .iter()
            .map(|id| self.monitor.lookup(id))
            .collect();
        match group_state(&states) {
            GroupState::On => "on".into(),
            GroupState::Off => "off".into(),
            GroupState::Missing => "Introuvable".into(),
            GroupState::Offline => match self.monitor.status() {
                Status::AuthFailed(_) => "Accès refusé".into(),
                Status::Disabled => "Non configuré".into(),
                _ => "Hors ligne".into(),
            },
        }
    }
    fn live_label(&self, key: &Key) -> Option<String> {
        let group = key.settings["group"].as_str().unwrap_or("");
        if !group.is_empty() {
            return Some(self.group_label(group));
        }
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
    fn schedule(&mut self, ctx: &str, job: Job, test: Option<(Config, Option<String>)>) {
        let Some(key) = self.keys.get_mut(ctx) else {
            return;
        };
        if key.pending {
            return;
        }
        key.pending = true;
        let settings = key.settings.clone();
        let prepared = plan(job, &settings, &self.groups, &|id| self.monitor.lookup(id));
        let (config, provided) = test.unwrap_or_else(|| (self.config.clone(), None));
        let vault = self.vault.clone();
        let sender = self.tx.clone();
        let context = ctx.to_owned();
        thread::spawn(move || {
            let result = (|| -> Result<usize, String> {
                let stored = vault.load().map_err(str::to_owned)?;
                match job {
                    Job::Test => {
                        let secret = test_credentials(
                            &config.url,
                            config.insecure,
                            stored.as_ref(),
                            provided.as_deref(),
                        )?;
                        live::test_connection(
                            &Settings {
                                url: config.url,
                                token: secret.token,
                                insecure: config.insecure,
                                bound_origin: secret.origin.unwrap_or_default(),
                            },
                            Duration::from_secs(8),
                        )
                    }
                    Job::Toggle | Job::Service => {
                        let secret = stored.ok_or("Non connecté")?;
                        let (path, body) = service_call(job, &settings, &prepared?)?;
                        request(&config, &secret, &path, body)
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
                self.observe(event.settings());
                self.insert_key(&ctx, &event.action, event.settings());
                self.draw(&ctx, sdk)?;
            }
            "willDisappear" => {
                self.keys.remove(&ctx);
            }
            "didReceiveSettings" => {
                self.observe(event.settings());
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
                        self.save_error =
                            match self.save_connection(&event.payload, op == "saveConnection") {
                                Ok(()) => String::new(),
                                Err(e) => e.to_owned(),
                            };
                        self.reconfigure();
                        self.send_status(&ctx, sdk)?;
                    }
                    "status" => self.send_status(&ctx, sdk)?,
                    "groups" => sdk.inspector(&ctx, self.groups_payload(None, None))?,
                    "saveGroup" | "deleteGroup" => {
                        let result = if op == "saveGroup" {
                            self.save_group(&event.payload).map(Some)
                        } else {
                            self.delete_group(event.payload["id"].as_str().unwrap_or(""))
                                .map(|_| None)
                        };
                        match result {
                            Ok(saved) => {
                                let payload = self.groups_payload(saved.as_deref(), None);
                                for target in self.inspectors.clone() {
                                    sdk.inspector(&target, payload.clone())?;
                                }
                            }
                            Err(e) => sdk.inspector(&ctx, self.groups_payload(None, Some(e)))?,
                        }
                    }
                    "list" => sdk.inspector(&ctx, self.list_payload())?,
                    "test" => {
                        // Adresse saisie (même non enregistrée) : jeton enregistré seulement si son
                        // origine est celle liée, sinon jeton fourni dans ce message (non enregistré).
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
                            let provided = event.payload["token"]
                                .as_str()
                                .map(str::trim)
                                .filter(|t| !t.is_empty() && t.len() <= 8_192)
                                .map(str::to_owned);
                            self.schedule(&ctx, Job::Test, Some((candidate, provided)));
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
            } else if key.action != SERVICE
                || !key.settings["group"].as_str().unwrap_or("").is_empty()
            {
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
    fn preset_hints_expose_only_bounded_options_and_bounds() {
        let sel = Entity {
            state: "a".into(),
            attributes: json!({"options":["a","", "b", 3], "secret":"x"}),
        };
        assert_eq!(
            preset_hints("input_select", &sel),
            vec![("options", json!(["a", "b"]))]
        );
        assert!(preset_hints("light", &sel).is_empty());
        let num = Entity {
            state: "5".into(),
            attributes: json!({"min":0, "max":40.5, "step":0.5}),
        };
        assert_eq!(preset_hints("input_number", &num).len(), 3);
        let clim = Entity {
            state: "heat".into(),
            attributes: json!({"hvac_modes":["off","heat"]}),
        };
        assert_eq!(
            preset_hints("climate", &clim),
            vec![("hvac_modes", json!(["off", "heat"]))]
        );
        let many = Entity {
            state: "a".into(),
            attributes: json!({"options": (0..300).map(|i| format!("o{i}")).collect::<Vec<_>>()}),
        };
        assert_eq!(
            preset_hints("select", &many)[0].1.as_array().unwrap().len(),
            100
        );
    }
    fn st(state: &str) -> Lookup {
        Some(Some(ent(state)))
    }
    fn grp(ids: &[&str]) -> Group {
        Group::new(
            "g1",
            "Salon",
            &ids.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
        .unwrap()
    }
    #[test]
    fn single_entity_body_is_unchanged_and_group_body_is_an_array() {
        let one =
            service_body(&json!({"entity":"light.a", "data":"{\"brightness_pct\":40}"})).unwrap();
        assert_eq!(one, json!({"brightness_pct":40, "entity_id":"light.a"}));
        let ids = vec![
            "light.a".to_string(),
            "switch.b".to_string(),
            "light.a".to_string(),
        ];
        let many = service_body_for(&json!({"data":""}), &ids).unwrap();
        assert_eq!(many, json!({"entity_id":["light.a","switch.b"]}));
        // l'entity_id des données avancées ne contourne jamais la cible
        let forced = service_body_for(&json!({"data":"{\"entity_id\":\"lock.x\"}"}), &ids).unwrap();
        assert_eq!(forced["entity_id"], json!(["light.a", "switch.b"]));
        assert!(service_body_for(&json!({}), &["light.a".into(), "bad id".into()]).is_err());
        assert!(service_body_for(&json!({}), &[]).is_err());
        let big: Vec<String> = (0..51).map(|i| format!("light.l{i}")).collect();
        assert!(service_body_for(&json!({}), &big).is_err());
        assert!(service_body(&json!({"entity":"nope"})).is_err());
    }
    #[test]
    fn group_state_semantics() {
        assert_eq!(group_state(&[st("off"), st("on")]), GroupState::On);
        assert_eq!(
            group_state(&[st("off"), st("unavailable")]),
            GroupState::Off
        );
        assert_eq!(group_state(&[st("off"), Some(None)]), GroupState::Off);
        assert_eq!(group_state(&[Some(None), Some(None)]), GroupState::Missing);
        assert_eq!(group_state(&[st("on"), None]), GroupState::Offline);
        assert_eq!(group_state(&[]), GroupState::Offline);
        assert_eq!(group_toggle_service(&[st("on"), st("off")]), Ok("turn_off"));
        assert_eq!(group_toggle_service(&[st("on"), st("on")]), Ok("turn_off"));
        assert_eq!(group_toggle_service(&[st("off"), st("off")]), Ok("turn_on"));
        assert!(group_toggle_service(&[None]).is_err());
    }
    #[test]
    fn plans_for_entity_and_group_keys() {
        let groups = vec![grp(&["light.a", "switch.b"])];
        let seen = |id: &str| if id == "light.a" { st("on") } else { st("off") };
        // touche existante (entité seule) : aucune lecture d'état, bascule native
        let single = plan(Job::Toggle, &json!({"entity":"light.a"}), &groups, &seen).unwrap();
        assert_eq!(
            single,
            Plan {
                ids: vec!["light.a".into()],
                domain_service: None
            }
        );
        // une entité invalide reste refusée, groupe vide de sens inconnu aussi
        assert!(plan(Job::Toggle, &json!({"entity":"x"}), &groups, &seen).is_err());
        assert_eq!(
            plan(Job::Service, &json!({"group":"nope"}), &groups, &seen),
            Err("Groupe introuvable")
        );
        // groupe mixte : au moins une allumée → tout éteindre via homeassistant
        let g = json!({"group":"g1", "domain":"homeassistant", "service":"toggle"});
        let p = plan(Job::Service, &g, &groups, &seen).unwrap();
        assert_eq!(p.ids, vec!["light.a", "switch.b"]);
        assert_eq!(p.domain_service, Some(("homeassistant", "turn_off")));
        let all_off = |_: &str| st("off");
        assert_eq!(
            plan(Job::Toggle, &json!({"group":"g1"}), &groups, &all_off)
                .unwrap()
                .domain_service,
            Some(("homeassistant", "turn_on"))
        );
        // allumer / éteindre : pas de lecture d'état (états non requis)
        let offline = |_: &str| None;
        let on = json!({"group":"g1", "domain":"homeassistant", "service":"turn_on"});
        assert_eq!(
            plan(Job::Service, &on, &groups, &offline)
                .unwrap()
                .domain_service,
            None
        );
        // bascule sans connexion : refus, aucune écriture
        assert_eq!(
            plan(Job::Toggle, &json!({"group":"g1"}), &groups, &offline),
            Err("États indisponibles")
        );
        // un service d'entité unique nommé toggle n'est pas réinterprété
        let single_toggle =
            json!({"entity":"light.a", "domain":"homeassistant", "service":"toggle"});
        assert_eq!(
            plan(Job::Service, &single_toggle, &groups, &offline)
                .unwrap()
                .domain_service,
            None
        );
    }
    #[test]
    fn toggle_ignores_stale_service_fields_and_runs_chosen_command() {
        let groups = vec![grp(&["light.a", "switch.b"])];
        let off = |_: &str| None;
        let plan_of = |s: Value| plan(Job::Toggle, &s, &groups, &off).unwrap();
        // ancienne touche Toggle : domain/service saisis à tort sont ignorés
        let stale = json!({"entity":"light.a", "domain":"light", "service":"turn_off"});
        assert!(!toggle_runs_service(&stale));
        assert_eq!(plan_of(stale).domain_service, None);
        assert!(!toggle_runs_service(
            &json!({"entity":"light.a", "command":"toggle", "domain":"light", "service":"turn_off"})
        ));
        // commande explicite : service du préréglage, cible validée comme Job::Service
        let cmd =
            json!({"entity":"light.a", "command":"off", "domain":"light", "service":"turn_off"});
        assert!(toggle_runs_service(&cmd));
        let p = plan_of(cmd.clone());
        assert_eq!(
            (p.ids, p.domain_service),
            (vec!["light.a".to_string()], None)
        );
        assert_eq!(
            service_body_for(&cmd, &["light.a".into()]).unwrap(),
            json!({"entity_id":"light.a"})
        );
        assert!(plan(
            Job::Toggle,
            &json!({"entity":"bad", "command":"off"}),
            &groups,
            &off
        )
        .is_err());
        // groupe : Tout allumer sans lecture d'état ; Basculer garde la sémantique du groupe
        let all_on =
            json!({"group":"g1", "command":"on", "domain":"homeassistant", "service":"turn_on"});
        let p = plan_of(all_on);
        assert_eq!(p.ids, vec!["light.a", "switch.b"]);
        assert_eq!(p.domain_service, None);
        let seen = |id: &str| if id == "light.a" { st("on") } else { st("off") };
        let tg =
            json!({"group":"g1", "command":"toggle", "domain":"homeassistant", "service":"toggle"});
        assert_eq!(
            plan(Job::Toggle, &tg, &groups, &seen)
                .unwrap()
                .domain_service,
            Some(("homeassistant", "turn_off"))
        );
        assert_eq!(safe_settings(&json!({"command":"off"}))["command"], "off");
        // chemins d'API réellement appelés
        let call = |s: &Value| {
            let pl = plan(Job::Toggle, s, &groups, &seen).unwrap();
            service_call(Job::Toggle, s, &pl).unwrap().0
        };
        assert_eq!(
            call(&json!({"entity":"light.a", "domain":"light", "service":"turn_off"})),
            "/api/services/homeassistant/toggle"
        );
        assert_eq!(
            call(
                &json!({"entity":"light.a", "command":"off", "domain":"light", "service":"turn_off"})
            ),
            "/api/services/light/turn_off"
        );
        assert_eq!(
            call(
                &json!({"group":"g1", "command":"on", "domain":"homeassistant", "service":"turn_on"})
            ),
            "/api/services/homeassistant/turn_on"
        );
        assert_eq!(call(&tg), "/api/services/homeassistant/turn_off");
        let bad = json!({"entity":"light.a", "command":"off", "domain":"li ght", "service":"x"});
        let pl = plan(Job::Toggle, &bad, &groups, &seen).unwrap();
        assert_eq!(
            service_call(Job::Toggle, &bad, &pl),
            Err("Service invalide")
        );
    }
    #[test]
    fn groups_are_created_edited_and_deleted_with_persistence() {
        let (mut home, _) = home_with(cfg(""), None);
        home.persist_groups = |_| Ok(());
        let ids = json!(["light.a", "switch.b"]);
        let id = home
            .save_group(&json!({"name":"Salon", "entities":ids}))
            .unwrap();
        assert_eq!(home.groups.len(), 1);
        assert_eq!(
            home.save_group(&json!({"name":"salon", "entities":["light.c"]})),
            Err("Nom de groupe déjà utilisé")
        );
        assert_eq!(
            home.save_group(&json!({"id":"inconnu", "name":"X", "entities":["light.c"]})),
            Err("Groupe introuvable")
        );
        assert!(home
            .save_group(&json!({"name":"Vide", "entities":[]}))
            .is_err());
        home.save_group(&json!({"id":id, "name":"Salon 2", "entities":["light.c"]}))
            .unwrap();
        assert_eq!(home.groups[0].name, "Salon 2");
        assert_eq!(home.groups[0].entities, vec!["light.c"]);
        let payload = home.groups_payload(Some(&id), None);
        assert_eq!(payload["groups"][0]["name"], "Salon 2");
        assert_eq!(payload["saved"], json!(id));
        // un échec d'écriture ne modifie pas l'état en mémoire
        home.persist_groups = |_| Err("Stockage impossible");
        assert!(home
            .save_group(&json!({"name":"Autre", "entities":["light.d"]}))
            .is_err());
        assert!(home.delete_group(&id).is_err());
        assert_eq!(home.groups.len(), 1);
        home.persist_groups = |_| Ok(());
        home.delete_group(&id).unwrap();
        assert!(home.groups.is_empty());
        assert_eq!(home.delete_group(&id), Err("Groupe introuvable"));
        for i in 0..MAX_GROUPS {
            home.save_group(&json!({"name":format!("G{i}"), "entities":["light.a"]}))
                .unwrap();
        }
        assert_eq!(
            home.save_group(&json!({"name":"Trop", "entities":["light.a"]})),
            Err("Maximum 50 groupes")
        );
    }
    #[test]
    fn group_key_settings_keep_the_group_and_old_keys_have_none() {
        assert_eq!(safe_settings(&json!({"group":"g1"}))["group"], "g1");
        assert_eq!(safe_settings(&json!({"entity":"light.a"}))["group"], "");
    }
    #[test]
    fn label_and_settings_never_carry_a_token() {
        assert_eq!(state_label(&ent("19.5")), "19.5 °C");
        let safe = safe_settings(&json!({"token":"secret", "url":"http://ha:8123", "refresh":0}));
        assert!(safe.get("token").is_none());
        assert_eq!(safe["refresh"], 2);
        assert_eq!(safe["url"], "http://ha:8123");
    }
    #[derive(Default)]
    struct MemVault(std::sync::Mutex<Option<Secret>>);
    impl Vault for MemVault {
        fn load(&self) -> Result<Option<Secret>, &'static str> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn save(&self, s: &Secret) -> Result<(), &'static str> {
            *self.0.lock().unwrap() = Some(s.clone());
            Ok(())
        }
        fn clear(&self) -> Result<(), &'static str> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }
    fn home_with(config: Config, secret: Option<Secret>) -> (Home, Arc<MemVault>) {
        let vault = Arc::new(MemVault(std::sync::Mutex::new(secret)));
        let mut home = Home::with(config, Monitor::inert(), vault.clone());
        home.persist = |_| Ok(());
        (home, vault)
    }
    fn secret(origin: Option<&str>) -> Secret {
        Secret {
            token: "tok".into(),
            origin: origin.map(str::to_owned),
        }
    }
    fn cfg(url: &str) -> Config {
        Config {
            url: url.into(),
            insecure: false,
        }
    }

    #[test]
    fn migration_collects_origins_and_never_adopts() {
        let (mut home, _) = home_with(Config::default(), None);
        home.observe(&json!({"url":" http://ha.local:8123 ", "insecure":true, "entity":"light.a"}));
        home.observe(&json!({"entity":"light.b"}));
        home.observe(&json!({"url":"ftp://x"}));
        home.observe(&json!({"url":"http://HA.local:8123/", "entity":"light.c"}));
        assert_eq!(home.config, Config::default(), "rien n'est appliqué");
        let p = home.status_payload();
        assert_eq!(p["code"], "suggestion");
        assert_eq!(p["suggestion"]["url"], "http://ha.local:8123");
        assert_eq!(p["configured"], false);
        // Deux origines distinctes : aucune suggestion, aucune adoption.
        home.observe(&json!({"url":"http://autre.local:8123"}));
        let p = home.status_payload();
        assert_eq!(p["code"], "not_configured");
        assert!(p.get("suggestion").is_none());
        assert_eq!(home.config, Config::default());
        // Une configuration existante n'est jamais écrasée ni suggérée.
        let (mut set, _) = home_with(cfg("http://other:8123"), None);
        set.observe(&json!({"url":"http://ha.local:8123"}));
        assert!(set.seen.is_empty());
        // Les réglages de touche gardent leur ancienne URL.
        assert_eq!(
            safe_settings(&json!({"url":"http://a:1"}))["url"],
            "http://a:1"
        );
    }
    #[test]
    fn two_keys_with_different_origins_adopt_nothing() {
        let (mut home, vault) = home_with(Config::default(), Some(secret(None)));
        home.observe(&json!({"url":"http://a.local:8123"}));
        home.observe(&json!({"url":"http://b.local:8123"}));
        assert!(suggestion(&home.config, &home.seen).is_none());
        assert_eq!(vault.load().unwrap().unwrap().origin, None);
    }
    #[test]
    fn save_to_a_new_origin_without_token_invalidates_the_token() {
        let (mut home, vault) = home_with(
            cfg("http://192.168.1.2:8123"),
            Some(secret(Some("http://192.168.1.2:8123"))),
        );
        // Même origine : jeton conservé.
        assert_eq!(
            home.save_connection(&json!({"url":"http://192.168.1.2:8123/"}), true),
            Ok(())
        );
        assert!(vault.load().unwrap().is_some());
        // Nouvelle origine sans jeton : effacé.
        assert_eq!(
            home.save_connection(&json!({"url":"http://192.168.1.9:8123"}), true),
            Ok(())
        );
        assert_eq!(vault.load().unwrap(), None, "jeton invalidé");
        assert_eq!(home.config.url, "http://192.168.1.9:8123");
        // Avec jeton : lié à la nouvelle origine.
        assert_eq!(
            home.save_connection(
                &json!({"url":"http://192.168.1.7:8123", "token":"new"}),
                true
            ),
            Ok(())
        );
        let s = vault.load().unwrap().unwrap();
        assert_eq!(
            (s.token.as_str(), s.origin.as_deref()),
            ("new", Some("http://192.168.1.7:8123"))
        );
    }
    #[test]
    fn legacy_secret_is_bound_once_to_the_current_origin() {
        let (mut home, vault) = home_with(cfg("http://192.168.1.2:8123"), Some(secret(None)));
        home.reconfigure();
        assert!(home.has_token && !home.mismatch);
        assert_eq!(
            vault.load().unwrap().unwrap().origin.as_deref(),
            Some("http://192.168.1.2:8123")
        );
    }
    #[test]
    fn mismatched_secret_is_reported_and_not_used() {
        let (mut home, _) = home_with(
            cfg("http://192.168.1.2:8123"),
            Some(secret(Some("http://10.0.0.1:8123"))),
        );
        home.reconfigure();
        assert!(home.mismatch);
        let p = home.status_payload();
        assert_eq!(p["code"], "token_mismatch");
        assert_eq!(p["message"], MISMATCH);
        assert_eq!(p["hasToken"], false);
    }
    #[test]
    fn insecure_is_refused_on_a_public_host() {
        let (mut home, vault) = home_with(Config::default(), None);
        let r = home.save_connection(
            &json!({"url":"https://ha.example.org", "insecure":true, "token":"t"}),
            true,
        );
        assert_eq!(r, Err(INSECURE_REFUSED));
        assert_eq!(home.config, Config::default());
        assert_eq!(vault.load().unwrap(), None, "rien n'est enregistré");
        home.save_error = INSECURE_REFUSED.into();
        assert_eq!(home.status_payload()["saveError"], INSECURE_REFUSED);
        // Hôte local : accepté.
        assert_eq!(
            home.save_connection(
                &json!({"url":"https://192.168.1.2:8123", "insecure":true}),
                true
            ),
            Ok(())
        );
        assert!(home.config.insecure);
    }
    #[test]
    fn request_to_another_origin_is_refused_without_network() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let config = cfg(&format!("http://127.0.0.1:{port}"));
        let other = secret(Some("http://autre.local:8123"));
        assert_eq!(
            request(&config, &other, "/api/services/light/toggle", json!({})),
            Err(MISMATCH)
        );
        assert_eq!(
            request(
                &config,
                &secret(None),
                "/api/services/light/toggle",
                json!({})
            ),
            Err(MISMATCH)
        );
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            listener.accept().is_err(),
            "le serveur témoin ne reçoit aucune connexion"
        );
    }
    #[test]
    fn status_messages_explain_the_cause() {
        let none = Config::default();
        assert_eq!(
            status_code(&none, false, false, false, &Status::Disabled),
            "not_configured"
        );
        assert!(status_message("not_configured", &Status::Disabled)
            .contains("Configure l'URL et le jeton"));
        let cfg = Config {
            url: "http://ha:8123".into(),
            insecure: false,
        };
        assert_eq!(
            status_code(&cfg, false, false, false, &Status::Disabled),
            "no_token"
        );
        assert_eq!(
            status_code(&cfg, true, false, false, &Status::AuthFailed("x".into())),
            "auth_failed"
        );
        assert_eq!(
            status_code(
                &cfg,
                true,
                false,
                false,
                &Status::Reconnecting {
                    attempt: 1,
                    error: "e".into()
                }
            ),
            "reconnecting"
        );
        assert_eq!(
            status_code(&cfg, true, false, false, &Status::Connected),
            "connected"
        );
    }
    #[test]
    fn disconnected_list_is_an_explained_error_and_never_has_a_token() {
        let home = Home::with(
            Config::default(),
            Monitor::inert(),
            Arc::new(MemVault::default()),
        );
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

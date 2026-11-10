//! Règle « État automatique » : comparaison, validation et machine « depuis N secondes ».
//! Pure (aucun réseau, aucune horloge globale) : testable sans Home Assistant.
use serde_json::Value;
use std::time::{Duration, Instant};

pub const OPS: [&str; 4] = ["eq", "ne", "gt", "lt"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub entity: String,
    /// Vide = état principal de l'entité.
    pub attribute: String,
    /// `eq`, `ne`, `gt`, `lt`.
    pub op: String,
    pub value: String,
    pub hold_seconds: u32,
    /// Déclenche aussi (edge active, initial) si la condition est déjà vraie au démarrage.
    pub fire_at_start: bool,
}

fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
pub fn valid_entity_id(id: &str) -> bool {
    id.len() <= 255
        && id
            .split_once('.')
            .is_some_and(|(domain, name)| identifier(domain) && identifier(name))
}
fn number(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// `None` (attribut absent) ne vérifie aucune comparaison, sauf « différent ».
pub fn compare(actual: Option<&str>, op: &str, value: &str) -> bool {
    match (op, actual) {
        ("eq", Some(a)) => a.trim().eq_ignore_ascii_case(value.trim()),
        ("ne", a) => !a.is_some_and(|a| a.trim().eq_ignore_ascii_case(value.trim())),
        ("gt", Some(a)) => matches!((number(a), number(value)), (Some(a), Some(b)) if a > b),
        ("lt", Some(a)) => matches!((number(a), number(value)), (Some(a), Some(b)) if a < b),
        _ => false,
    }
}

impl Rule {
    /// Lit les réglages de l'action (contrat 0.19.1) ; erreur lisible si la règle est inutilisable.
    pub fn from_settings(settings: &Value) -> Result<Self, &'static str> {
        let text = |key: &str| settings[key].as_str().unwrap_or("").trim().to_owned();
        let hold = match &settings["hold_seconds"] {
            Value::Number(n) => n.as_u64().ok_or("Durée invalide")?,
            Value::String(s) if s.trim().is_empty() => 0,
            Value::String(s) => s.trim().parse::<u64>().map_err(|_| "Durée invalide")?,
            Value::Null => 0,
            _ => return Err("Durée invalide"),
        };
        if hold > 86_400 {
            return Err("Durée : 24 h au maximum");
        }
        let op = match text("op").as_str() {
            "" => "eq".to_owned(),
            other => other.to_owned(),
        };
        let rule = Self {
            entity: text("entity"),
            attribute: text("attribute"),
            op,
            // La valeur n'est pas rognée : seul le contrôle ci-dessous la borne.
            value: settings["value"].as_str().unwrap_or("").to_owned(),
            hold_seconds: hold as u32,
            fire_at_start: settings["fire_at_start"].as_bool().unwrap_or(false),
        };
        rule.validate()?;
        Ok(rule)
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_entity_id(&self.entity) {
            return Err("Entité invalide");
        }
        if self.attribute.len() > 128 || self.attribute.chars().any(char::is_control) {
            return Err("Attribut invalide");
        }
        if !OPS.contains(&self.op.as_str()) {
            return Err("Comparaison inconnue");
        }
        if self.value.len() > 256 || self.value.chars().any(char::is_control) {
            return Err("Valeur invalide");
        }
        if matches!(self.op.as_str(), "gt" | "lt") && number(&self.value).is_none() {
            return Err("Seuil numérique attendu");
        }
        Ok(())
    }
    pub fn matches(&self, actual: Option<&str>) -> bool {
        compare(actual, &self.op, &self.value)
    }
}

/// Ce que la machine voit de Home Assistant à cet instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading<'a> {
    /// Déconnecté, non configuré ou entité absente : état inconnu.
    Unknown,
    /// Valeur lue (état ou attribut) ; `None` si l'attribut n'existe pas.
    Known(Option<&'a str>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Active { initial: bool },
    Inactive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Unprimed,
    Idle,
    Pending { since: Instant, initial: bool },
    Active,
}

/// Front montant unique après N secondes, annulation si la condition retombe avant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    phase: Phase,
}
impl Default for Machine {
    fn default() -> Self {
        Self {
            phase: Phase::Unprimed,
        }
    }
}
impl Machine {
    /// Mode actif (conservé pendant une déconnexion, mais non publié : voir `state`).
    pub fn active(&self) -> bool {
        self.phase == Phase::Active
    }
    /// État à publier : `None` = inconnu (novydeckStateUnknown).
    pub fn state(&self, reading: Reading) -> Option<bool> {
        match reading {
            Reading::Unknown => None,
            Reading::Known(_) => Some(self.active()),
        }
    }
    pub fn step(&mut self, rule: &Rule, now: Instant, reading: Reading) -> Option<Edge> {
        let Reading::Known(actual) = reading else {
            // Inconnu : un compte à rebours en cours est annulé ; le mode actif est conservé.
            if matches!(self.phase, Phase::Pending { .. }) {
                self.phase = Phase::Idle;
            }
            return None;
        };
        let hold = Duration::from_secs(u64::from(rule.hold_seconds));
        let elapse = |since: Instant, initial: bool, phase: &mut Phase| {
            if now.saturating_duration_since(since) < hold {
                *phase = Phase::Pending { since, initial };
                None
            } else {
                *phase = Phase::Active;
                Some(Edge::Active { initial })
            }
        };
        match (self.phase, rule.matches(actual)) {
            // Déjà vrai au premier relevé : état 1 sans transition, sauf « déclencher au démarrage ».
            (Phase::Unprimed, true) if !rule.fire_at_start => {
                self.phase = Phase::Active;
                None
            }
            (Phase::Unprimed, true) => elapse(now, true, &mut self.phase),
            (Phase::Idle, true) => elapse(now, false, &mut self.phase),
            (Phase::Pending { since, initial }, true) => elapse(since, initial, &mut self.phase),
            (Phase::Active, true) => None,
            (Phase::Active, false) => {
                self.phase = Phase::Idle;
                Some(Edge::Inactive)
            }
            (_, false) => {
                self.phase = Phase::Idle;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rule(hold: u32, start: bool) -> Rule {
        Rule {
            entity: "binary_sensor.presence".into(),
            attribute: String::new(),
            op: "eq".into(),
            value: "off".into(),
            hold_seconds: hold,
            fire_at_start: start,
        }
    }
    struct Run {
        m: Machine,
        r: Rule,
        t0: Instant,
    }
    impl Run {
        fn new(r: Rule) -> Self {
            Self {
                m: Machine::default(),
                r,
                t0: Instant::now(),
            }
        }
        fn at(&mut self, secs: u64, actual: &str) -> Option<Edge> {
            self.m.step(
                &self.r,
                self.t0 + Duration::from_secs(secs),
                Reading::Known(Some(actual)),
            )
        }
        fn unknown(&mut self, secs: u64) -> Option<Edge> {
            self.m.step(
                &self.r,
                self.t0 + Duration::from_secs(secs),
                Reading::Unknown,
            )
        }
    }

    #[test]
    fn hold_then_single_active_edge_then_inactive_edge() {
        let mut r = Run::new(rule(30, false));
        assert_eq!(r.at(0, "on"), None);
        assert!(!r.m.active());
        assert_eq!(r.at(1, "off"), None);
        assert_eq!(r.at(30, "off"), None);
        assert!(!r.m.active());
        assert_eq!(r.at(31, "off"), Some(Edge::Active { initial: false }));
        assert!(r.m.active());
        for t in 32..40 {
            assert_eq!(r.at(t, "off"), None, "pas de répétition");
        }
        assert_eq!(r.at(41, "on"), Some(Edge::Inactive));
        assert!(!r.m.active());
        assert_eq!(r.at(42, "on"), None);
    }
    #[test]
    fn hold_is_cancelled_when_condition_drops_early() {
        let mut r = Run::new(rule(30, false));
        assert_eq!(r.at(0, "on"), None);
        assert_eq!(r.at(1, "off"), None);
        assert_eq!(r.at(20, "on"), None); // annulé
        assert_eq!(r.at(25, "off"), None); // le compte repart de zéro
        assert_eq!(r.at(54, "off"), None);
        assert_eq!(r.at(55, "off"), Some(Edge::Active { initial: false }));
    }
    #[test]
    fn zero_hold_fires_immediately_on_real_change() {
        let mut r = Run::new(rule(0, false));
        assert_eq!(r.at(0, "on"), None);
        assert_eq!(r.at(1, "off"), Some(Edge::Active { initial: false }));
    }
    #[test]
    fn already_true_at_start_is_active_without_transition() {
        let mut r = Run::new(rule(30, false));
        assert_eq!(r.at(0, "off"), None);
        assert!(r.m.active());
        assert_eq!(r.m.state(Reading::Known(Some("off"))), Some(true));
        assert_eq!(r.at(100, "off"), None);
        assert_eq!(r.at(101, "on"), Some(Edge::Inactive));
    }
    #[test]
    fn fire_at_start_sends_initial_active_edge_after_hold() {
        let mut r = Run::new(rule(0, true));
        assert_eq!(r.at(0, "off"), Some(Edge::Active { initial: true }));
        let mut r = Run::new(rule(5, true));
        assert_eq!(r.at(0, "off"), None);
        assert_eq!(r.at(4, "off"), None);
        assert_eq!(r.at(5, "off"), Some(Edge::Active { initial: true }));
        assert_eq!(r.at(6, "off"), None);
        // Ensuite un vrai changement n'est plus « initial ».
        assert_eq!(r.at(7, "on"), Some(Edge::Inactive));
        assert_eq!(r.at(8, "off"), None);
        assert_eq!(r.at(13, "off"), Some(Edge::Active { initial: false }));
    }
    #[test]
    fn unknown_publishes_unknown_cancels_countdown_and_keeps_active() {
        let mut r = Run::new(rule(30, false));
        assert_eq!(r.at(0, "on"), None);
        assert_eq!(r.at(1, "off"), None);
        assert_eq!(r.unknown(10), None);
        assert_eq!(r.m.state(Reading::Unknown), None);
        assert_eq!(r.at(12, "off"), None);
        assert_eq!(
            r.at(41, "off"),
            None,
            "le compte est reparti à la reconnexion"
        );
        assert_eq!(r.at(42, "off"), Some(Edge::Active { initial: false }));
        // Actif conservé pendant la coupure, aucune transition parasite au retour.
        assert_eq!(r.unknown(50), None);
        assert!(r.m.active());
        assert_eq!(r.at(51, "off"), None);
        // Changement réel survenu pendant la coupure : une seule transition.
        assert_eq!(r.unknown(60), None);
        assert_eq!(r.at(61, "on"), Some(Edge::Inactive));
    }
    #[test]
    fn comparisons() {
        assert!(compare(Some(" ON "), "eq", "on"));
        assert!(!compare(Some("off"), "eq", "on"));
        assert!(compare(Some("off"), "ne", "on"));
        assert!(compare(None, "ne", "on"));
        assert!(!compare(None, "eq", "on"));
        assert!(compare(Some("21.5"), "gt", "20"));
        assert!(!compare(Some("20"), "gt", "20"));
        assert!(compare(Some("19"), "lt", "20"));
        assert!(!compare(Some("unavailable"), "gt", "20"));
        assert!(!compare(Some("1"), "gt", "NaN"));
        assert!(!compare(Some("5"), "xx", "5"));
    }
    #[test]
    fn settings_parsing_and_validation() {
        let ok = Rule::from_settings(&json!({"entity":"sensor.temp","op":"gt","value":"20.5","hold_seconds":"15","fire_at_start":true})).unwrap();
        assert_eq!(
            (ok.hold_seconds, ok.fire_at_start, ok.op.as_str()),
            (15, true, "gt")
        );
        let default_op =
            Rule::from_settings(&json!({"entity":"light.salon","value":"on"})).unwrap();
        assert_eq!((default_op.op.as_str(), default_op.hold_seconds), ("eq", 0));
        for bad in [
            json!({"entity":"../x","value":"on"}),
            json!({"entity":"light.a","op":"regex","value":"x"}),
            json!({"entity":"sensor.a","op":"gt","value":"abc"}),
            json!({"entity":"light.a","value":"on","hold_seconds":86401}),
            json!({"entity":"light.a","value":"on","hold_seconds":-1}),
            json!({"entity":"light.a","value":"o\nn"}),
            json!({"entity":"light.a","attribute":"a\u{0}b","value":"x"}),
        ] {
            assert!(Rule::from_settings(&bad).is_err(), "{bad}");
        }
    }
}

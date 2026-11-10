# Changelog

## 1.1.0
- New action "Automatic state" (`com.novydeck.homeassistant.auto`) / Nouvelle action « État automatique » : rule on an entity or attribute (equals, not equal, >, <), optional "for N seconds", optional trigger at startup; the key image follows the rule (state 1 active / 0 inactive) and fires the "becomes active / inactive" branches. Requires NovyDeck 0.19.1+ / nécessite NovyDeck 0.19.1+.
- One live Home Assistant WebSocket connection shared by all keys (no per-key polling for the rule); on disconnect the last image is kept / Connexion WebSocket unique et partagée ; la dernière image est conservée en cas de déconnexion.
- URL and insecure-TLS option are now plugin-wide; the token stays encrypted (DPAPI) and is never shown again; existing per-key URLs are migrated, never overwritten / URL et option TLS désormais communes au plugin ; jeton chiffré, jamais réaffiché ; anciennes URL de touche migrées sans écrasement.
- Redesigned inspector: instant sensor search (name, entity_id, current state), domain filter, click to select, clear message when not configured or disconnected, value suggestions / Inspecteur : recherche instantanée de capteur, filtre par domaine, sélection au clic, message clair si non configuré ou déconnecté, suggestions de valeurs.
- Toggle entity, Call service and Display state keep their behaviour and settings / Les trois actions existantes gardent leur comportement et leurs réglages.

## 1.0.2
- Distinct house logo and matching action icons / Logo maison distinct et icônes d’action assorties.
- Redesigned bilingual settings inspector / Inspecteur repensé FR/EN.

## 1.0.1
- Key text is drawn over the background without a black box around letters / Texte des touches sans cadre noir autour des lettres.

## 1.0.0
- Toggle entity, call service and display state with visible-only polling.
- Local property inspector, encrypted Windows DPAPI token storage, no telemetry.

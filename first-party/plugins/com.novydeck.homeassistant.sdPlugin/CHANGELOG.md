# Changelog

## 1.2.0
- "Call service": new **Action** menu with ready-made choices that depend on the entity domain (light: turn on / off / toggle / brightness % / colour temperature; switch, input_boolean, fan, siren, automation; cover; media_player; scene, script, button, input_button; lock; climate; vacuum; input_number, number; input_select, select), instead of typing domain, service and JSON by hand. Numeric parameters are bounded fields (%, °C, K); list options and climate modes come from the entity / Nouvelle liste **Action** pour « Appeler un service », selon le domaine de l'entité : plus besoin de saisir domaine, service et JSON ; paramètres numériques bornés ; options de liste et modes du thermostat lus depuis l'entité.
- "Custom (advanced)" is always the last choice and shows the domain / service / JSON fields as before / « Personnalisé (avancé) » est toujours le dernier choix et affiche les champs domaine / service / JSON comme avant.
- Compatibility: a preset only fills the same `domain` / `service` / `data` settings, so existing keys and the plugin backend are unchanged. Opening an existing key finds its matching preset (or shows "Custom" with the values untouched) and never rewrites settings / Compatibilité : mêmes réglages `domain` / `service` / `data` ; l'ouverture d'une touche existante retrouve son préréglage ou affiche « Personnalisé » sans rien modifier. Changing the entity to another domain proposes the first action only if the key had no custom settings / changer d'entité de domaine ne propose la première action que si la touche n'avait pas de réglage personnalisé.
- Entities without presets (sensor, …) or no entity selected: custom mode directly / Entité sans préréglage ou aucune entité : mode personnalisé directement.
- The entity list now carries select options, climate modes and number bounds (bounded, read-only) / La liste d'entités transmet options de liste, modes du thermostat et bornes de nombre.
- Preset table in `presets.js` with node tests (`node --test first-party/homeassistant/tests`) / table des préréglages dans `presets.js`, testée par node.
- Groups / Groupes : tick several entities, create / rename / edit / delete a named group, and choose "A saved group" as the target of Toggle and Call service keys (Cible : entité ou groupe). Groups are stored once for the plugin in `groups.json` next to `config.json` (shared by all keys, independent from the connection) / enregistrés une fois pour le plugin dans `groups.json`, partagés par toutes les touches. 1–50 entities, 50 groups max. Group actions: turn all on, turn all off, toggle (any on → all off, otherwise all on) via `homeassistant.*`, so mixed domains (light + switch) work in one call; the key shows on/off (on if at least one is on) / Tout allumer, tout éteindre, basculer (au moins une allumée → tout éteindre, sinon tout allumer) ; la touche affiche on/off. Single-entity keys are unchanged.
- Not done: completing the list with every other service of the domain (would need a new REST call to `/api/services`) / Non fait : compléter avec tous les autres services du domaine (nécessiterait un nouvel appel REST `/api/services`).

## 1.1.1
- Security: the token is now bound to the address (scheme, host, port) it was entered for and is never sent to another one; changing the address without entering a token erases the saved token ("Re-enter the token") / Sécurité : le jeton est lié à l'adresse (schéma, hôte, port) pour laquelle il a été saisi et n'est jamais envoyé ailleurs ; changer l'adresse sans saisir de jeton efface le jeton enregistré (« Ressaisir le jeton »). A 1.1.0 token is bound once to the current address / Un jeton 1.1.0 est lié une fois à l'adresse courante.
- Test button: the saved token is used only for the bound address; otherwise the token typed in the field is used, without being saved / Bouton Tester : le jeton enregistré n'est utilisé que pour l'adresse liée ; sinon le jeton saisi est utilisé, sans être enregistré.
- 1.0.x migration: the address of the first key is no longer adopted. A single distinct address is only suggested in the inspector and applied by "Save connection"; key settings are never lost / Reprise 1.0.x : l'adresse de la première touche n'est plus adoptée ; une adresse unique est seulement suggérée et appliquée par « Enregistrer la connexion » ; les réglages des touches ne sont jamais perdus.
- Clear-text warning when the address is http:// or ws:// and not local (loopback, private, link-local, .local, .lan, .home.arpa, single-label names); discreet reminder on local addresses / Avertissement en clair pour une adresse http:// ou ws:// non locale ; simple rappel sur adresse locale.
- "Do not verify the certificate" moved under "Advanced" with a red warning and is refused for non-local hosts; it will be removed in favour of certificate pinning / L'option « Ne pas vérifier le certificat » passe sous « Avancé », avec avertissement rouge, et est refusée hors réseau local ; elle sera retirée au profit de l'épinglage du certificat.

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

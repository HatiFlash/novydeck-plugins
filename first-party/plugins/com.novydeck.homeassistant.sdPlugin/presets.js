/* Home Assistant : actions prédéfinies pour « Appeler un service » / predefined actions.
   Table pure (sans DOM) chargée par inspector.html et testée par node :
   first-party/homeassistant/tests/presets.test.js.
   Un préréglage se traduit toujours en {domain, service, data} : les mêmes clés de réglages
   qu'avant 1.2.0, donc le backend Rust et les touches existantes restent compatibles. */
(function (root) {
  'use strict';
  const L = (en, fr) => ({ en, fr });
  const pct = (key, en, fr, def) => ({ key, type: 'number', min: 1, max: 100, step: 1, int: true, unit: '%', label: L(en, fr), def });
  const onOffToggle = (extra) => [
    { id: 'on', service: 'turn_on', label: L('Turn on', 'Allumer') },
    { id: 'off', service: 'turn_off', label: L('Turn off', 'Éteindre') },
    { id: 'toggle', service: 'toggle', label: L('Toggle', 'Basculer') },
  ].concat(extra || []);
  const activate = (id, svc) => [{ id, service: svc, label: L('Activate', 'Activer') }];

  // svc absent = le domaine de l'entité ; param : paramètre unique (number | choice) rangé dans data[param.key].
  const TABLE = {
    light: onOffToggle([
      { id: 'brightness', service: 'turn_on', label: L('Turn on at brightness (%)', 'Allumer à une luminosité (%)'), param: pct('brightness_pct', 'Brightness (%)', 'Luminosité (%)', 50) },
      { id: 'color_temp', service: 'turn_on', label: L('Turn on at colour temperature (K)', 'Allumer à une température de couleur (K)'), param: { key: 'color_temp_kelvin', type: 'number', min: 1500, max: 10000, step: 100, int: true, unit: 'K', label: L('Colour temperature (K)', 'Température de couleur (K)'), def: 2700 } },
    ]),
    switch: onOffToggle(),
    input_boolean: onOffToggle(),
    fan: onOffToggle(),
    siren: onOffToggle(),
    automation: [
      { id: 'on', service: 'turn_on', label: L('Enable', 'Activer') },
      { id: 'off', service: 'turn_off', label: L('Disable', 'Désactiver') },
      { id: 'toggle', service: 'toggle', label: L('Toggle', 'Basculer') },
      { id: 'trigger', service: 'trigger', label: L('Trigger now', 'Déclencher') },
    ],
    cover: [
      { id: 'open', service: 'open_cover', label: L('Open', 'Ouvrir') },
      { id: 'close', service: 'close_cover', label: L('Close', 'Fermer') },
      { id: 'stop', service: 'stop_cover', label: L('Stop', 'Arrêter') },
      { id: 'position', service: 'set_cover_position', label: L('Position (%)', 'Position (%)'), param: { key: 'position', type: 'number', min: 0, max: 100, step: 1, int: true, unit: '%', label: L('Position (%)', 'Position (%)'), def: 50 } },
    ],
    media_player: [
      { id: 'playpause', service: 'media_play_pause', label: L('Play / Pause', 'Lecture / Pause') },
      { id: 'play', service: 'media_play', label: L('Play', 'Lecture') },
      { id: 'pause', service: 'media_pause', label: L('Pause', 'Pause') },
      { id: 'next', service: 'media_next_track', label: L('Next', 'Suivant') },
      { id: 'previous', service: 'media_previous_track', label: L('Previous', 'Précédent') },
      { id: 'volume', service: 'volume_set', label: L('Volume (%)', 'Volume (%)'), param: { key: 'volume_level', type: 'number', min: 0, max: 100, step: 1, int: true, unit: '%', scale: 0.01, label: L('Volume (%)', 'Volume (%)'), def: 30 } },
      { id: 'mute', service: 'volume_mute', fixed: { is_volume_muted: true }, label: L('Mute', 'Couper le son') },
      { id: 'unmute', service: 'volume_mute', fixed: { is_volume_muted: false }, label: L('Unmute', 'Rétablir le son') },
    ],
    scene: activate('activate', 'turn_on'),
    script: [{ id: 'run', service: 'turn_on', label: L('Run', 'Exécuter') }],
    button: [{ id: 'press', service: 'press', label: L('Press', 'Appuyer') }],
    input_button: [{ id: 'press', service: 'press', label: L('Press', 'Appuyer') }],
    lock: [
      { id: 'lock', service: 'lock', label: L('Lock', 'Verrouiller') },
      { id: 'unlock', service: 'unlock', label: L('Unlock', 'Déverrouiller') },
    ],
    climate: [
      { id: 'temperature', service: 'set_temperature', label: L('Target temperature (°C)', 'Température cible (°C)'), param: { key: 'temperature', type: 'number', min: 5, max: 35, step: 0.5, unit: '°C', label: L('Temperature (°C)', 'Température (°C)'), def: 20 } },
      { id: 'hvac_mode', service: 'set_hvac_mode', label: L('Mode', 'Mode'), param: { key: 'hvac_mode', type: 'choice', optionsFrom: 'hvac_modes', fallback: ['off', 'heat', 'cool', 'heat_cool', 'auto', 'dry', 'fan_only'], label: L('Mode', 'Mode') } },
    ],
    vacuum: [
      { id: 'start', service: 'start', label: L('Start', 'Démarrer') },
      { id: 'pause', service: 'pause', label: L('Pause', 'Pause') },
      { id: 'return', service: 'return_to_base', label: L('Return to base', 'Retour à la base') },
    ],
    input_number: [{ id: 'set', service: 'set_value', label: L('Set value', 'Définir la valeur'), param: { key: 'value', type: 'number', free: true, boundsFrom: true, step: 1, label: L('Value', 'Valeur'), def: 0 } }],
    number: [{ id: 'set', service: 'set_value', label: L('Set value', 'Définir la valeur'), param: { key: 'value', type: 'number', free: true, boundsFrom: true, step: 1, label: L('Value', 'Valeur'), def: 0 } }],
    input_select: [{ id: 'option', service: 'select_option', label: L('Choose an option', 'Choisir une option'), param: { key: 'option', type: 'choice', optionsFrom: 'options', label: L('Option', 'Option') } }],
    select: [{ id: 'option', service: 'select_option', label: L('Choose an option', 'Choisir une option'), param: { key: 'option', type: 'choice', optionsFrom: 'options', label: L('Option', 'Option') } }],
  };
  // Cible = groupe d'entités (domaines éventuellement mélangés) : services génériques
  // homeassistant.* ; « Basculer » est décidé par le plugin d'après l'état du groupe
  // (au moins une allumée → tout éteindre, sinon tout allumer).
  const GROUP = '__group';
  TABLE[GROUP] = [
    { id: 'on', domain: 'homeassistant', service: 'turn_on', label: L('Turn all on', 'Tout allumer') },
    { id: 'off', domain: 'homeassistant', service: 'turn_off', label: L('Turn all off', 'Tout éteindre') },
    { id: 'toggle', domain: 'homeassistant', service: 'toggle', label: L('Toggle (any on → all off, else all on)', 'Basculer (au moins une allumée → tout éteindre, sinon tout allumer)') },
  ];
  for (const dom of Object.keys(TABLE)) for (const p of TABLE[dom]) { p.domain = p.domain || dom; p.svc = p.svc || null; }

  const CUSTOM = '__custom';
  const domainOf = (entityId) => { const m = /^([a-z0-9_]+)\./.exec(String(entityId || '').trim()); return m ? m[1] : ''; };
  const forDomain = (dom) => TABLE[dom] || [];
  const byId = (dom, id) => forDomain(dom).find((p) => p.id === id) || null;
  const first = (dom) => forDomain(dom)[0] || null;
  const serviceDomain = (p) => p.svc || p.domain;
  const isEmpty = (domain, service, data) => !String(domain || '').trim() && !String(service || '').trim() && !String(data || '').trim();
  const label = (p, lang) => p.label[lang] || p.label.en;

  /** Valide la valeur d'un paramètre numérique/choix ; renvoie la valeur normalisée ou null. */
  function validate(p, value, bounds) {
    if (!p.param) return null;
    const q = p.param;
    if (q.type === 'choice') {
      const s = String(value === undefined || value === null ? '' : value).trim();
      return s && s.length <= 128 ? s : null;
    }
    if (value === '' || value === null || value === undefined) return null;
    const n = Number(value);
    if (!Number.isFinite(n)) return null;
    if (q.int && !Number.isInteger(n)) return null;
    const lo = q.boundsFrom && bounds && Number.isFinite(bounds.min) ? bounds.min : q.min;
    const hi = q.boundsFrom && bounds && Number.isFinite(bounds.max) ? bounds.max : q.max;
    if (lo !== undefined && n < lo) return null;
    if (hi !== undefined && n > hi) return null;
    return n;
  }

  /** Préréglage + valeur → {domain, service, data} (data = texte JSON, '' sans donnée) ; null si valeur invalide. */
  function build(p, value, bounds) {
    const out = { domain: serviceDomain(p), service: p.service, data: '' };
    const obj = Object.assign({}, p.fixed || {});
    if (p.param) {
      const v = validate(p, value, bounds);
      if (v === null) return null;
      obj[p.param.key] = p.param.type === 'choice' ? v : p.param.scale ? Math.round(v * p.param.scale * 1e6) / 1e6 : v;
    }
    if (Object.keys(obj).length) out.data = JSON.stringify(obj);
    return out;
  }

  /** Inverse : (domaine de l'entité, domain/service/data enregistrés) → {preset, value} ou null (= Personnalisé). */
  function match(entityDomain, domain, service, data) {
    const dom = String(entityDomain || '');
    const text = String(data === undefined || data === null ? '' : data).trim();
    let obj;
    if (!text) obj = {};
    else {
      try { obj = JSON.parse(text); } catch (_) { return null; }
      if (!obj || typeof obj !== 'object' || Array.isArray(obj)) return null;
    }
    for (const p of forDomain(dom)) {
      if (serviceDomain(p) !== String(domain || '').trim() || p.service !== String(service || '').trim()) continue;
      const rest = Object.assign({}, obj);
      let ok = true;
      for (const k of Object.keys(p.fixed || {})) { if (rest[k] !== p.fixed[k]) ok = false; delete rest[k]; }
      if (!ok) continue;
      if (!p.param) { if (Object.keys(rest).length === 0) return { preset: p, value: null }; continue; }
      const keys = Object.keys(rest);
      if (keys.length !== 1 || keys[0] !== p.param.key) continue;
      let v = rest[p.param.key];
      if (p.param.type === 'choice') { if (typeof v !== 'string' || !v.trim() || v.length > 128) continue; return { preset: p, value: v }; }
      if (typeof v !== 'number' || !Number.isFinite(v)) continue;
      if (p.param.scale) v = Math.round(v / p.param.scale);
      if (p.param.int && !Number.isInteger(v)) continue;
      if (!p.param.free && ((p.param.min !== undefined && v < p.param.min) || (p.param.max !== undefined && v > p.param.max))) continue;
      return { preset: p, value: v };
    }
    return null;
  }

  const api = { TABLE, CUSTOM, GROUP, domainOf, forDomain, byId, first, serviceDomain, isEmpty, label, validate, build, match };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.HAPresets = api;
})(typeof self !== 'undefined' ? self : this);

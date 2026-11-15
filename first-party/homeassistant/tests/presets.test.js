'use strict';
// node --test first-party/homeassistant/tests
const test = require('node:test');
const assert = require('node:assert/strict');
const P = require('../../plugins/com.novydeck.homeassistant.sdPlugin/presets.js');

test('chaque préréglage de la table fait l\'aller-retour build → match', () => {
  let n = 0;
  for (const dom of Object.keys(P.TABLE)) {
    for (const p of P.TABLE[dom]) {
      let value = null;
      if (p.param) value = p.param.type === 'choice' ? 'x' : p.param.def;
      const out = P.build(p, value, null);
      assert.ok(out, `${dom}/${p.id} build`);
      const back = P.match(dom, out.domain, out.service, out.data);
      assert.ok(back, `${dom}/${p.id} match`);
      assert.equal(back.preset.id, p.id, `${dom}/${p.id} identité`);
      assert.equal(back.value, value, `${dom}/${p.id} valeur`);
      assert.ok(p.label.en && p.label.fr, `${dom}/${p.id} libellés FR/EN`);
      n++;
    }
  }
  assert.ok(n >= 40, 'table non vide: ' + n);
});

test('lumière : actions et données exactes', () => {
  const L = (id) => P.byId('light', id);
  assert.deepEqual(P.build(L('on')), { domain: 'light', service: 'turn_on', data: '' });
  assert.deepEqual(P.build(L('off')), { domain: 'light', service: 'turn_off', data: '' });
  assert.deepEqual(P.build(L('toggle')), { domain: 'light', service: 'toggle', data: '' });
  assert.deepEqual(P.build(L('brightness'), 40), { domain: 'light', service: 'turn_on', data: '{"brightness_pct":40}' });
  assert.deepEqual(P.build(L('color_temp'), 3000), { domain: 'light', service: 'turn_on', data: '{"color_temp_kelvin":3000}' });
});

test('paramètres bornés : hors limites ou non entier refusés', () => {
  const b = P.byId('light', 'brightness');
  for (const bad of [0, 101, 12.5, '', 'abc', null, NaN]) assert.equal(P.build(b, bad), null, String(bad));
  assert.ok(P.build(b, 1) && P.build(b, 100));
  const t = P.byId('climate', 'temperature');
  assert.equal(P.build(t, 4.5), null);
  assert.equal(P.build(t, 21.5).data, '{"temperature":21.5}');
});

test('volume : pourcentage ↔ fraction Home Assistant', () => {
  const v = P.byId('media_player', 'volume');
  assert.equal(P.build(v, 30).data, '{"volume_level":0.3}');
  assert.equal(P.build(v, 7).data, '{"volume_level":0.07}');
  assert.equal(P.match('media_player', 'media_player', 'volume_set', '{"volume_level":0.07}').value, 7);
  assert.equal(P.match('media_player', 'media_player', 'volume_set', '{"volume_level":0.3}').value, 30);
});

test('couper / rétablir le son se distinguent par is_volume_muted', () => {
  assert.equal(P.match('media_player', 'media_player', 'volume_mute', '{"is_volume_muted":true}').preset.id, 'mute');
  assert.equal(P.match('media_player', 'media_player', 'volume_mute', '{"is_volume_muted":false}').preset.id, 'unmute');
  assert.equal(P.match('media_player', 'media_player', 'volume_mute', ''), null);
});

test('domaines de service : propre domaine, scene/script, cover, lock, vacuum', () => {
  assert.equal(P.build(P.byId('switch', 'toggle')).domain, 'switch');
  assert.equal(P.build(P.byId('fan', 'on')).domain, 'fan');
  assert.equal(P.build(P.byId('scene', 'activate')).domain, 'scene');
  assert.equal(P.build(P.byId('script', 'run')).service, 'turn_on');
  assert.equal(P.build(P.byId('automation', 'trigger')).service, 'trigger');
  assert.equal(P.build(P.byId('cover', 'position'), 0).data, '{"position":0}');
  assert.equal(P.build(P.byId('lock', 'unlock')).service, 'unlock');
  assert.equal(P.build(P.byId('vacuum', 'return')).service, 'return_to_base');
  assert.equal(P.build(P.byId('input_button', 'press')).domain, 'input_button');
});

test('choix (input_select, climate) : texte libre accepté, vide refusé', () => {
  const o = P.byId('input_select', 'option');
  assert.equal(P.build(o, 'Soirée').data, '{"option":"Soirée"}');
  assert.equal(P.build(o, '  '), null);
  assert.equal(P.match('input_select', 'input_select', 'select_option', '{"option":"Soirée"}').value, 'Soirée');
  assert.equal(P.match('select', 'select', 'select_option', '{"option":""}'), null);
  assert.equal(P.build(P.byId('climate', 'hvac_mode'), 'heat').data, '{"hvac_mode":"heat"}');
});

test('input_number : bornes issues de l\'entité', () => {
  const s = P.byId('input_number', 'set');
  assert.equal(P.build(s, 50, { min: 0, max: 40 }), null);
  assert.equal(P.build(s, 35, { min: 0, max: 40 }).data, '{"value":35}');
  assert.equal(P.match('input_number', 'input_number', 'set_value', '{"value":1234}').value, 1234);
});

test('compatibilité : réglages existants non reconnus restent personnalisés', () => {
  // données supplémentaires, clé inconnue, JSON invalide, entity_id dans les données
  assert.equal(P.match('light', 'light', 'turn_on', '{"brightness":128}'), null);
  assert.equal(P.match('light', 'light', 'turn_on', '{"brightness_pct":40,"transition":2}'), null);
  assert.equal(P.match('light', 'light', 'turn_on', '{"entity_id":"light.x"}'), null);
  assert.equal(P.match('light', 'light', 'turn_on', '{oops'), null);
  assert.equal(P.match('light', 'light', 'turn_on', '[]'), null);
  assert.equal(P.match('light', 'light', 'turn_on', '{"brightness_pct":150}'), null);
  assert.equal(P.match('light', 'light', 'turn_on', '{"brightness_pct":"40"}'), null);
  // service d'un autre domaine que l'entité
  assert.equal(P.match('light', 'switch', 'turn_on', ''), null);
  // domaine sans préréglage
  assert.equal(P.match('sensor', 'sensor', 'turn_on', ''), null);
  assert.equal(P.match('', 'light', 'turn_on', ''), null);
});

test('reconnaissance d\'une touche existante (données vides ou {})', () => {
  assert.equal(P.match('light', 'light', 'turn_off', '').preset.id, 'off');
  assert.equal(P.match('light', 'light', 'turn_off', ' {} ').preset.id, 'off');
  assert.equal(P.match('light', 'light', 'turn_on', '{"brightness_pct":40}').value, 40);
  assert.equal(P.match('light', ' light ', 'toggle', undefined).preset.id, 'toggle');
});

test('helpers : domaine d\'entité, vide, libellés', () => {
  assert.equal(P.domainOf('light.salon'), 'light');
  assert.equal(P.domainOf('../x'), '');
  assert.equal(P.domainOf(''), '');
  assert.equal(P.isEmpty('', ' ', ''), true);
  assert.equal(P.isEmpty('light', '', ''), false);
  assert.equal(P.first('light').id, 'on');
  assert.equal(P.first('sensor'), null);
  assert.equal(P.label(P.byId('light', 'on'), 'fr'), 'Allumer');
  assert.equal(P.label(P.byId('light', 'on'), 'en'), 'Turn on');
});

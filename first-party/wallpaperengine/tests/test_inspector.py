"""Offline contract checks for the Wallpaper Engine property inspector."""
import json
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
INSPECTOR = ROOT / 'plugins/com.novydeck.wallpaperengine.sdPlugin/inspector.html'
MANIFEST = ROOT / 'plugins/com.novydeck.wallpaperengine.sdPlugin/manifest.json'


class StopPlayInspectorTests(unittest.TestCase):
    def test_action_and_existing_images(self):
        manifest = json.loads(MANIFEST.read_text(encoding='utf-8'))
        actions = {a['UUID'].rsplit('.', 1)[-1]: a for a in manifest['Actions']}
        self.assertEqual(manifest['Version'], '1.0.6')
        self.assertTrue({'pause', 'play', 'stop', 'profile', 'stopplay'} <= actions.keys())
        toggle = actions['stopplay']
        self.assertEqual(len(toggle['States']), 2)
        self.assertEqual([s['Image'] for s in toggle['States']], ['images/stop', 'images/play'])
        for name in [toggle['Icon'], *(s['Image'] for s in toggle['States'])]:
            self.assertTrue((MANIFEST.parent / (name + '.png')).is_file())
            self.assertTrue((MANIFEST.parent / (name + '@2x.png')).is_file())

    def test_localized_help_and_global_only_fields(self):
        html = INSPECTOR.read_text(encoding='utf-8')
        self.assertIn("$('specific').hidden=['pause','stopplay','play','stop','mute','unmute'].includes(action)", html)
        self.assertIn("$('targetField').hidden=!['wallpaper','playlist','next'].includes(action)", html)
        self.assertIn("$('global').textContent=copy[lang][action==='stopplay'?'stopplayHelp':'global']", html)
        self.assertEqual(len(re.findall(r"stopplayHelp:'[^']+'", html)), 2)
        self.assertIn('The next command is shared across keys', html)
        self.assertIn('La prochaine commande est partagée entre les touches', html)
        self.assertIn('initial state is unknown', html)
        self.assertIn('état initial est inconnu', html)


if __name__ == '__main__':
    unittest.main()

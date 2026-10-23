"""Pure checks for untrusted archive processing, without running Defender."""
import io
import tempfile
import unittest
from pathlib import Path
import zipfile

import security_scan as scan


class SecurityScanTests(unittest.TestCase):
    def setUp(self):
        self.plugin = {'version': '1.0.2', 'homepage': 'https://github.com/Example/plugin',
                       'download': 'https://github.com/Example/plugin/releases/download/1.0.2/plugin.zip'}

    def archive(self, entries):
        blob = io.BytesIO()
        with zipfile.ZipFile(blob, 'w') as file:
            for name, data in entries.items():
                file.writestr(name, data)
        return blob.getvalue()

    def test_source_matching(self):
        self.assertTrue(scan.source_match(self.plugin))
        self.plugin['download'] = 'https://github.com/attacker/plugin/releases/download/1.0.2/plugin.zip'
        self.assertFalse(scan.source_match(self.plugin))

    def test_script_heuristics_and_license(self):
        data = self.archive({'x.sdPlugin/manifest.json': '{"Version":"1.0.2","CodePathWin":"run.js","OS":[{"Platform":"windows"}]}',
                             'x.sdPlugin/run.js': 'eval(atob("abc")); https://example.org/path',
                             'x.sdPlugin/LICENSE': 'MIT'})
        with tempfile.TemporaryDirectory() as folder:
            findings, urls, size = scan.inspect_archive(data, Path(folder), self.plugin)
        self.assertIn('obfuscated_eval', [f['rule'] for f in findings])
        self.assertEqual(urls, ['https://example.org/path'])
        self.assertGreater(size, 0)

    def test_codepath_without_exe_and_declared_license_are_not_blocking(self):
        self.plugin['license'] = 'MIT'
        data = self.archive({'x.sdPlugin/manifest.json': '{"Version":"1.0.2","CodePath":"x","OS":[{"Platform":"windows"}]}',
                             'x.sdPlugin/x.exe': 'MZ'})
        scan.signature = lambda path: 'NotSigned|'
        with tempfile.TemporaryDirectory() as folder:
            findings, _, _ = scan.inspect_archive(data, Path(folder), self.plugin)
        rules = {f['rule']: f['severity'] for f in findings}
        self.assertNotIn('manifest_codepath_missing', rules)
        self.assertEqual(rules['native_binary'], 'info')
        self.assertEqual(rules['missing_license'], 'info')

    def test_path_traversal_refused(self):
        data = self.archive({'../outside.txt': 'bad'})
        with tempfile.TemporaryDirectory() as folder:
            with self.assertRaises(ValueError):
                scan.inspect_archive(data, Path(folder), self.plugin)


if __name__ == '__main__':
    unittest.main()

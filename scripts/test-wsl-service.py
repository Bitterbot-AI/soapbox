import importlib.util
from pathlib import Path
import tempfile
import unittest

source = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('installer', source/'install-wsl-docker-service.py')
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


class Installation(unittest.TestCase):
    def test_replaces_reviewed_docker_hook_preserves_settings_and_is_repeatable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'etc').mkdir()
            conf = root/'etc/wsl.conf'
            original = '# keep comment\n[user]\ndefault=operator\n[boot]\ncommand=sh -c "exec dockerd"\n[network]\ngenerateResolvConf=false\n'
            conf.write_text(original)
            with self.assertRaises(RuntimeError):
                installer.install(source, root)
            backup = installer.install(source, root, replace_docker_command=True)
            self.assertEqual((backup/'wsl.conf.before').read_text(), original)
            result = conf.read_text()
            self.assertIn('# keep comment', result)
            self.assertIn('default=operator', result)
            self.assertIn('generateResolvConf=false', result)
            self.assertEqual(result.count('command='), 1)
            self.assertIn('command=/usr/local/sbin/soapbox-docker-service start', result)
            installer.install(source, root)
            self.assertEqual(conf.read_text(), result)
            self.assertEqual((root/'usr/local/sbin/soapbox-docker-service').read_bytes(), (source/'soapbox-docker-service').read_bytes())

    def test_unrelated_boot_hooks_and_systemd_are_not_replaced(self):
        for original in ['[boot]\ncommand=/opt/custom-start\n', '[boot]\nsystemd=true\n']:
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root/'etc').mkdir()
                conf = root/'etc/wsl.conf'
                conf.write_text(original)
                with self.assertRaises(RuntimeError):
                    installer.install(source, root, replace_docker_command=True)
                self.assertEqual(conf.read_text(), original)

    def test_adds_boot_section_to_existing_user_config(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'etc').mkdir()
            conf = root/'etc/wsl.conf'
            conf.write_text('[user]\ndefault=operator\n')
            installer.install(source, root)
            self.assertEqual(conf.read_text().count('[boot]'), 1)
            self.assertIn('default=operator', conf.read_text())


if __name__ == '__main__':
    unittest.main()

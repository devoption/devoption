"""Real apt simulation: refuse removals when offline package versions advance."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

from test_ci_vm_packages import SCRIPT, UBUNTU


@unittest.skipUnless(UBUNTU and shutil.which('dpkg-deb') and shutil.which('apt-get'),
                     'Ubuntu apt/dpkg solver fixture')
class GuestPackageSolverTests(unittest.TestCase):
    def test_incomplete_version_update_is_refused_and_matching_init_is_preserved(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            status = root / 'status'
            original = ('Package: fixture-runtime\nStatus: install ok installed\nArchitecture: all\n'
                        'Version: 1\nDescription: synthetic runtime\n\n'
                        'Package: fixture-init\nStatus: install ok installed\nArchitecture: all\n'
                        'Version: 1\nDepends: fixture-runtime (= 1)\nDescription: synthetic init\n\n')
            status.write_text(original)
            (root / 'sources.list').touch()
            (root / 'sources.list.d').mkdir()
            (root / 'lists').mkdir()
            tools = root / 'tools'; tools.mkdir()
            (tools / 'id').write_text('#!/bin/sh\necho 0\n')
            (tools / 'id').chmod(0o755)
            # Always simulate. Use only synthetic status and empty private sources
            # and caches, even though the production helper supplies install args.
            (tools / 'timeout').write_text('#!' + sys.executable + '\n' + '''import os, pathlib, subprocess, sys
root = pathlib.Path(os.environ['SOLVER_FIXTURE'])
args = sys.argv[sys.argv.index('apt-get') + 1:]
command = ['/usr/bin/apt-get', '--simulate', *args]
for key, value in [('Dir::State::status', root / 'status'),
                   ('Dir::State::lists', root / 'lists'),
                   ('Dir::State::extended_states', root / 'extended_states'),
                   ('Dir::Etc::sourcelist', root / 'sources.list'),
                   ('Dir::Etc::sourceparts', root / 'sources.list.d'),
                   ('Dir::Cache::pkgcache', ''), ('Dir::Cache::srcpkgcache', ''),
                   ('Debug::NoLocking', 'true')]:
    command += ['-o', str(key) + '=' + str(value)]
result = subprocess.run(command, capture_output=True, text=True, timeout=30)
(root / 'solver.log').write_text(result.stdout + result.stderr)
sys.exit(result.returncode)
''')
            (tools / 'timeout').chmod(0o755)
            packages = root / 'packages'; packages.mkdir()

            def archive(name, dependency=None):
                tree = root / name; (tree / 'DEBIAN').mkdir(parents=True)
                control = (f'Package: {name}\nVersion: 2\nArchitecture: all\n'
                           'Maintainer: Fixture <fixture@example.test>\nDescription: synthetic solver fixture\n')
                if dependency:
                    control += f'Depends: {dependency}\n'
                (tree / 'DEBIAN/control').write_text(control)
                subprocess.run(['dpkg-deb', '--root-owner-group', '--build', str(tree),
                                str(packages / (name + '_2_all.deb'))], check=True, capture_output=True, timeout=30)

            def install():
                (packages / 'SHA256SUMS').write_text(''.join(
                    hashlib.sha256(p.read_bytes()).hexdigest() + '  ' + p.name + '\n'
                    for p in sorted(packages.glob('*.deb'))))
                env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ['PATH'],
                           DEVOPTION_DISPOSABLE_VM_PACKAGES='1', SOLVER_FIXTURE=str(root), LC_ALL='C')
                result = subprocess.run(['bash', str(SCRIPT), 'installer-guest-offline', str(packages)],
                                        env=env, capture_output=True, text=True, timeout=40)
                self.assertEqual(status.read_text(), original)
                return result, (root / 'solver.log').read_text()

            archive('fixture-runtime')
            rejected, log = install()
            self.assertNotEqual(rejected.returncode, 0, log)
            self.assertIn('remove is disabled', log)
            archive('fixture-init', 'fixture-runtime (= 2)')
            accepted, log = install()
            self.assertEqual(accepted.returncode, 0, log)
            self.assertNotIn('Remv ', log)
            self.assertIn('Inst fixture-init', log)
            self.assertIn('Inst fixture-runtime', log)

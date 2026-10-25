"""Cover change-boundary and required-status regressions in installation CI."""

from itertools import product
from pathlib import Path
import subprocess
import tempfile
import unittest

from ci_installation import installation_required, safe_path, validate_results


class SelectionTests(unittest.TestCase):
    def test_allowlist_is_conservative(self):
        for path in ('README.md', 'docs/development.md', 'docs/screenshots/home.png',
                     'crates/devoption-ui/assets/theme.css'):
            self.assertTrue(safe_path(path), path)
        for path in ('Cargo.lock', 'Cargo.toml', 'rust-toolchain.toml', '.cargo/config.toml',
                     '.github/workflows/ci.yml', 'scripts/check_bundle_upgrade.py',
                     'examples/web-app/Cargo.toml', 'crates/devoption-ui/src/lib.rs',
                     'release-candidates/candidate.json', 'new-input', 'docs-other/file'):
            self.assertFalse(safe_path(path), path)

    def test_non_pr_and_reusable_calls_require_full_coverage(self):
        for event, force in (('push', ''), ('workflow_dispatch', ''),
                             ('workflow_call', ''), ('pull_request', 'true')):
            self.assertTrue(installation_required(event, force, '', '')[0])

    def test_missing_revisions_or_objects_require_full_coverage(self):
        for base, head in (('', ''), ('a'*40, 'b'*40), ('--help', 'c'*40)):
            with tempfile.TemporaryDirectory() as root:
                self.assertTrue(installation_required('pull_request', '', base, head, root)[0])

    def test_git_diff_handles_deletions_renames_large_sets_and_base_advancement(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            def git(*args):
                return subprocess.check_output(['git', *args], cwd=root).decode().strip()
            def write(path, contents='fixture'):
                target = root/path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(contents)
            def commit():
                git('add', '.')
                git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                    '-c', 'commit.gpgSign=false', 'commit', '--quiet', '-m', 'fixture')
                return git('rev-parse', 'HEAD')
            git('init', '--quiet')
            write('scripts/critical.py'); write('docs/old.md')
            base = commit()
            write('docs/one.md')
            safe = commit()
            self.assertFalse(installation_required('pull_request', '', base, safe, root)[0])
            # New base-only code must not contaminate a documentation PR's diff.
            git('checkout', '--quiet', '--detach', base)
            write('crates/shared/src/lib.rs')
            advanced_base = commit()
            self.assertFalse(installation_required('pull_request', '', advanced_base, safe, root)[0])
            git('checkout', '--quiet', '--detach', safe)
            # A move into the safe tree must still include the removed unsafe path.
            git('mv', 'scripts/critical.py', 'docs/moved.md')
            moved = commit()
            self.assertTrue(installation_required('pull_request', '', base, moved, root)[0])
            git('checkout', '--quiet', '--detach', base)
            (root/'scripts/critical.py').unlink()
            deleted = commit()
            self.assertTrue(installation_required('pull_request', '', base, deleted, root)[0])
            git('checkout', '--quiet', '--detach', base)
            for index in range(350):
                write(f'docs/{index}.md')
            write('docs/line\nbreak.md')
            large_safe = commit()
            self.assertFalse(installation_required('pull_request', '', base, large_safe, root)[0])
            write('z-unknown-input')
            large_unsafe = commit()
            self.assertTrue(installation_required('pull_request', '', base, large_unsafe, root)[0])
            self.assertTrue(installation_required('pull_request', '', base, base, root)[0])


class GateTests(unittest.TestCase):
    def test_all_job_results_and_decisions(self):
        names = ('selection', 'lint', 'tests', 'installation', 'scripts')
        for decision in ('true', 'false', '', 'invalid'):
            for results in product(('success', 'failure', 'cancelled', 'skipped'), repeat=5):
                needs = {name: {'result': result} for name, result in zip(names, results)}
                needs['selection']['outputs'] = {'installation': decision}
                should_pass = (decision in ('true', 'false')
                               and all(needs[name]['result'] == 'success'
                                       for name in names if name != 'installation')
                               and needs['installation']['result'] ==
                               ('success' if decision == 'true' else 'skipped'))
                if should_pass:
                    validate_results(needs)
                else:
                    with self.assertRaises(ValueError):
                        validate_results(needs)

    def test_missing_or_extra_jobs_cannot_pass(self):
        with self.assertRaises(ValueError):
            validate_results({})
        with self.assertRaises(ValueError):
            validate_results({name: {'result': 'success'} for name in
                              ('selection', 'lint', 'tests', 'installation', 'scripts', 'extra')})


if __name__ == '__main__':
    unittest.main()

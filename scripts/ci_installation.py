"""Conservative PR installation selection and the required CI result gate."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess

from release_bundle import SOURCE_FILES


SAFE_ROOT_FILES = {'README.md', 'CHANGELOG.md', 'CONTRIBUTING.md', 'AGENTS.md'}


def safe_path(path):
    # An allowlist makes new crates, fixtures and release/configuration inputs
    # require installation coverage without maintaining a dependency blacklist.
    # Shipped instructions and license are release inputs, even under docs/.
    # CHANGELOG.md is deliberately exempt: every ordinary PR must update it.
    if path in SOURCE_FILES and path != 'CHANGELOG.md':
        return False
    return (path in SAFE_ROOT_FILES or path.startswith('docs/')
            or path.startswith('crates/devoption-ui/assets/'))


def installation_required(event, force, base, head, repository=Path('.')):
    if event != 'pull_request' or force == 'true':
        return True, 'Full validation for main or a reusable workflow call.'
    try:
        if not all(re.fullmatch(r'[0-9a-f]{40}', value) for value in (base, head)):
            raise ValueError('Missing immutable PR revisions')
        def git(*args):
            return subprocess.check_output(['git', *args], cwd=repository, timeout=60)
        ancestor = git('merge-base', base, head).decode('ascii').strip()
        # Disable rename detection: both the removed and added paths must be safe.
        raw = git('diff', '--no-renames', '--name-only', '-z', ancestor, head, '--')
        paths = raw.decode('utf-8').split('\0')[:-1]
        if not paths or any(not safe_path(path) for path in paths):
            return True, 'Relevant, unknown or empty change set requires installation coverage.'
        return False, 'Only documentation and UI assets changed.'
    except (ValueError, UnicodeError, OSError, subprocess.SubprocessError):
        return True, 'PR diff unavailable; conservatively require installation coverage.'


def validate_results(needs):
    expected = {'selection', 'lint', 'tests', 'installation', 'scripts'}
    if set(needs) != expected:
        raise ValueError('Missing or unexpected validation jobs')
    for name in ('selection', 'lint', 'tests', 'scripts'):
        if needs[name]['result'] != 'success':
            raise ValueError(f'{name} did not succeed')
    required = needs['selection'].get('outputs', {}).get('installation')
    if required not in ('true', 'false'):
        raise ValueError('Missing or invalid installation decision')
    expected_result = 'success' if required == 'true' else 'skipped'
    if needs['installation']['result'] != expected_result:
        raise ValueError('Installation result did not match the successful selection decision')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=('select', 'gate'))
    args = parser.parse_args()
    if args.mode == 'gate':
        validate_results(json.loads(os.environ['NEEDS']))
        return
    required, reason = installation_required(
        os.environ['EVENT_NAME'], os.environ.get('FORCE_FULL', ''),
        os.environ.get('BASE_SHA', ''), os.environ.get('HEAD_SHA', ''))
    decision = str(required).lower()
    with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
        output.write(f'installation={decision}\n')
    with open(os.environ['GITHUB_STEP_SUMMARY'], 'a') as summary:
        summary.write(f'Installation checks required: **{decision}**. {reason}\n')
    print(reason)


if __name__ == '__main__':
    main()

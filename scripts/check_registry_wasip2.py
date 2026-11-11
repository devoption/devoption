"""Public static-site acceptance inside the guarded disposable library installation."""
import hashlib
import os
import re
import shutil
import tomllib

from check_bundle_runtime import smoke
from check_named_web_app import run
from release_plan import workspace


def check(repository, binary, root, registry, checksum, request, mutate, commit,
          prepare, build, decide, readers):
    example = repository / 'examples/static-site'
    # Public test archives are unchanged upstream bytes, independently pinned by
    # the example lockfile and the preparation policy. No downloads in this proof.
    archives = repository / 'crates/devoption-builds/tests/fixtures/wasip2-closure'
    public = registry / 'public-archives'
    public.mkdir(mode=0o700)
    for package in tomllib.loads((example / 'Cargo.lock').read_text())['package']:
        if 'checksum' in package:
            name = f'{package["name"]}-{package["version"]}.crate'
            raw = (archives / name).read_bytes()
            assert hashlib.sha256(raw).hexdigest() == package['checksum']
            (public / name).write_bytes(raw)
            (public / name).chmod(0o600)

    version, _ = workspace(repository)
    for mixed in (False, True):
        projects = set(re.findall(rb'href="/projects/([0-9a-f]{32})"', request('GET', '/projects')[2]))
        mutate('/projects', 'Mixed HTTP example' if mixed else 'Public HTTP example')
        project, = set(re.findall(rb'href="/projects/([0-9a-f]{32})"', request('GET', '/projects')[2])) - projects
        project = project.decode()
        if mixed:
            decide('0.1.0', {'action': 'readers', 'readers': readers + [project]})
        path = root / 'repositories' / ('mixed-http' if mixed else 'public-http')
        shutil.copytree(example, path, ignore=shutil.ignore_patterns('target', '.git'))
        if mixed:
            manifest = path / 'Cargo.toml'
            manifest.write_text(manifest.read_text() +
                '\nreusable-fixture={version="=0.1.0",registry="team"}\n')
            lock = path / 'Cargo.lock'
            lock.write_text(lock.read_text().replace(' "wasip2",', ' "wasip2",\n "reusable-fixture",') +
                '\n[[package]]\nname="reusable-fixture"\nversion="0.1.0"\n'
                'source="sparse+https://packages.example.test/index/"\n' + f'checksum="{checksum}"\n')
            source = path / 'src/lib.rs'
            # Exercise the internal library during each actual HTTP invocation.
            source.write_text(source.read_text().replace(
                'fn handle(request: IncomingRequest, output: ResponseOutparam) {',
                'fn handle(request: IncomingRequest, output: ResponseOutparam) {\n'
                '        assert_eq!(reusable_fixture::answer(), 42);'))
        run('git', '-C', path, 'init', '-q')
        commit(path)
        mutate(f'/projects/{project}/repositories', path.name)
        history = f'/projects/{project}/builds'
        repo, = set(re.findall(rb'/builds/([0-9a-f]{32})/run', request('GET', history)[2]))
        base = history + '/' + repo.decode()
        if os.environ.get('DEVOPTION_EXPECT_WASIP2_REFUSAL') == '1':
            mutate(base + '/prepare', expected=409)
            print('Reproduced registry-enabled public static-site preparation HTTP 409 before compilation.', flush=True)
            return
        # Ordinary source capture, review and approval path (no bypass runner).
        _, artifact = build(project, base)
        component = root / ('mixed-http.wasm' if mixed else 'public-http.wasm')
        component.write_bytes(artifact)
        assets = [(url, kind, (path / 'assets' / filename).read_bytes()) for url, kind, filename in
                  [('/assets/site.css', 'text/css; charset=utf-8', 'site.css'),
                   ('/assets/site.js', 'text/javascript; charset=utf-8', 'site.js')]]
        smoke(binary.with_name('devoption-runtime'), component, version, assets=assets)
        # An already reviewed public archive cannot change before approval.
        token = prepare(base)
        archive = next(public.glob('wit-bindgen-*.crate'))
        original = archive.read_bytes()
        archive.write_bytes(original[:-1] + bytes([original[-1] ^ 1]))
        try:
            mutate(base + '/approve', token, expected=409)
            mutate(base + '/prepare', expected=409)
            assert not (root / 'jobs' / token / 'artifact/component.wasm').exists()
        finally:
            archive.write_bytes(original)
        # A source edit makes a previous review stale.
        token = prepare(base)
        source = path / 'src/lib.rs'
        source.write_text(source.read_text() + '\n// changed after review\n')
        commit(path)
        mutate(base + '/approve', token, expected=409)
        if mixed:
            token = prepare(base)
            decide('0.1.0', {'action': 'readers', 'readers': readers})
            mutate(base + '/approve', token, expected=409)
            mutate(base + '/prepare', expected=409)
        assert artifact.startswith(b'\0asm\x0d\0\x01\0')
    print('Registry-aware public static-site and mixed published-library graph: owner review/approval, '
          'runtime validation, HTTP/assets, altered archive, stale source and revoked-reader refusal passed.', flush=True)

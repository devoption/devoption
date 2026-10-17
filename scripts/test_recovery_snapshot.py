import fcntl
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import recovery_snapshot as recovery


class RecoveryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.roots = {name: self.root/name for name in recovery.LOCKS}
        for path in self.roots.values(): path.mkdir(mode=0o700)
        (self.roots['owner']/'owner.bin').write_bytes(b'private owner fixture')
        (self.roots['history']/'build-history.bin').write_bytes(b'fixture history')
        (self.roots['jobs']/'receipt.toml').write_text('fixture = true')
        (self.roots['releases']/'component.wasm').write_bytes(b'fixture component')
        (self.roots['releases']/'desired.json').write_text('{"intent":"stopped"}')
        for name, root in self.roots.items():
            (root/recovery.LOCKS[name]).write_bytes(b'')
        for root in self.roots.values():
            for path in root.iterdir(): path.chmod(0o600)
        self.build = dict(uid=os.geteuid(), owner_data=str(self.roots['owner']), history=str(self.roots['history']), work=str(self.roots['jobs']), repositories=str(self.root/'repositories'), toolchain=str(self.root/'toolchain'), vendor=str(self.root/'vendor'))
        self.deployment = dict(uid=os.geteuid(), owner_data=self.build['owner_data'], history=self.build['history'], build_work=self.build['work'], repositories=self.build['repositories'], state=str(self.roots['releases']), worker=str(self.root/'worker'))
        self.build_path = self.root/'build.toml'; self.deploy_path = self.root/'deployment.toml'
        for path, data in ((self.build_path, self.build), (self.deploy_path, self.deployment)):
            path.write_text(''.join(f'{k} = {json.dumps(v)}\n' for k,v in data.items()))
        self.ref_path = self.root/'binary'; self.ref_path.write_bytes(b'compatible binary')
        self.ref_path.chmod(0o600)
        # Keep tests independent of installed root-only helpers/systemd. Real VM
        # validation exercises those external references and unit checks.
        original = recovery.references
        self.patch_refs = patch.object(recovery, 'references', side_effect=lambda paths: original({'binary': self.ref_path}))
        self.patch_refs.start(); self.addCleanup(self.patch_refs.stop)
        stopped = patch.object(recovery, 'assert_stopped')
        stopped.start(); self.addCleanup(stopped.stop)
        self.snapshot = self.root/'snapshot'

    def create(self, limit=1024*1024):
        recovery.create(self.build_path, self.deploy_path, self.ref_path, self.snapshot, 'fixture.service', 'a'*40, limit)

    def test_verified_copy_stages_metadata_and_never_overwrites_originals(self):
        nested = self.roots['jobs']/'source'; nested.mkdir(mode=0o775); nested.chmod(0o775)
        (nested/'lib.rs').write_text('trusted source'); (nested/'lib.rs').chmod(0o664)
        self.create()
        recovery.verify(self.snapshot)
        destination = self.root/'restored'
        recovery.stage(self.snapshot, destination)
        for name, root in self.roots.items():
            for source in root.iterdir():
                target = destination/name/source.name
                if source.name == recovery.LOCKS[name]:
                    self.assertFalse(target.exists())
                elif source.is_file():
                    self.assertEqual(source.read_bytes(), target.read_bytes())
                    self.assertEqual(recovery.metadata(source), recovery.metadata(target))
        with self.assertRaises(FileExistsError): recovery.stage(self.snapshot, destination)
        with self.assertRaises(ValueError): recovery.stage(self.snapshot, self.roots['owner'])
        with self.assertRaises(ValueError): recovery.stage(self.snapshot, self.snapshot/'..'/'owner'/'nested')
        with self.assertRaises(FileExistsError): self.create()

    def test_corruption_missing_extra_and_symlink_payloads_fail_before_staging(self):
        self.create()
        artifact = self.snapshot/'payload/releases/component.wasm'
        original = artifact.read_bytes()
        for mutation in ('corrupt', 'missing', 'link'):
            if artifact.exists(): artifact.unlink()
            if mutation == 'corrupt': artifact.write_bytes(b'corrupted')
            if mutation == 'link': artifact.symlink_to(self.ref_path)
            with self.subTest(mutation=mutation), self.assertRaises((ValueError, OSError)):
                recovery.stage(self.snapshot, self.root/'should-not-exist')
            self.assertFalse((self.root/'should-not-exist').exists())
        artifact.unlink(); artifact.write_bytes(original); artifact.chmod(0o600)
        extra = self.snapshot/'payload/owner/.projects.lock'; extra.write_bytes(b'')
        with self.assertRaises(ValueError): recovery.verify(self.snapshot)

    def test_limits_links_pending_and_running_intent_fail(self):
        with self.assertRaises(ValueError): self.create(limit=1)
        self.assertFalse(self.snapshot.exists())
        path = self.roots['jobs']/'unsafe'
        path.symlink_to(self.ref_path)
        with self.assertRaises(ValueError): self.create()
        path.unlink()
        path = self.roots['owner']/'.owner.tmp'; path.write_bytes(b'partial')
        with self.assertRaises(ValueError): self.create()
        path.unlink()
        (self.roots['releases']/'desired.json').write_text('{"intent":"running"}')
        with self.assertRaises(ValueError): self.create()

    def test_busy_writer_lock_refuses_snapshot(self):
        with (self.roots['history']/'build-history.lock').open('rb') as stream:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaises(BlockingIOError): self.create()
        self.assertFalse(self.snapshot.exists())

    def test_source_change_leaves_unverifiable_incomplete_snapshot(self):
        original = recovery.copy_records
        def changing(*args):
            original(*args)
            (self.roots['history']/'build-history.bin').write_bytes(b'changed')
        with patch.object(recovery, 'copy_records', side_effect=changing):
            with self.assertRaises(ValueError): self.create()
        with self.assertRaises(ValueError): recovery.verify(self.snapshot)

    def test_growth_after_inventory_cannot_exceed_copy_budget(self):
        records = recovery.inventory(self.roots, 1024*1024)
        source = self.roots['owner']/'owner.bin'
        original_size = source.stat().st_size
        source.write_bytes(source.read_bytes() + b'extra bytes')
        destination = self.root/'partial'; destination.mkdir(mode=0o700)
        with self.assertRaises(ValueError): recovery.copy_records(self.roots, destination, records)
        self.assertEqual((destination/'owner/owner.bin').stat().st_size, original_size)

    def test_changed_installation_refuses_stage(self):
        self.create()
        self.ref_path.write_bytes(b'new binary')
        with self.assertRaises(ValueError): recovery.stage(self.snapshot, self.root/'restored')
        self.assertFalse((self.root/'restored').exists())

    def test_active_unknown_or_populated_unit_fails_closed(self):
        for output in ('ActiveState=active\nControlGroup=/system.slice/test\n', 'ActiveState=inactive\nControlGroup=/still-populated\n', ''):
            with patch.object(recovery.subprocess, 'run') as run:
                run.return_value.stdout = output
                with self.assertRaises(ValueError): recovery.stopped('fixture.service')
        with patch.object(recovery.subprocess, 'run') as run:
            run.return_value.stdout = 'ActiveState=inactive\nControlGroup=\n'
            recovery.stopped('fixture.service')
        with self.assertRaises(ValueError): recovery.stopped('--all')

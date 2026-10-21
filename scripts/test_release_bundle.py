"""Exercise candidate identity and rejection boundaries without running payloads."""

from datetime import date
import hashlib
import io
import json
import os
import re
from pathlib import Path
import tarfile
from unittest.mock import patch

from check_release_bundle import check
from release_bundle import SOURCE_FILES, assemble, binary_bytes, probe, probe_runtime, sha256, RUNTIME
from release_inventory import SBOM, PROVENANCE
from release_metadata import load_metadata, render_metadata
from release_plan import git
from test_release_plan import Fixture, ROOT


class BundleTests(Fixture):
    def setUp(self):
        super().setUp()
        self.version = "0.2.1-preview.1"
        for name in SOURCE_FILES:
            if name in ("Cargo.lock", "CHANGELOG.md", "release-metadata.toml"):
                continue
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes((ROOT / name).read_bytes())
        path = self.root / "Cargo.toml"
        path.write_text(path.read_text().replace("0.2.0", self.version))
        (self.root / "Cargo.lock").write_text(re.sub(
            r'(?m)(^name = "devoption-[^"]+"\nversion = ")[^"]+("$)',
            lambda match: match[1] + self.version + match[2], (ROOT / "Cargo.lock").read_text()))
        metadata, digest = load_metadata(self.root, self.version)
        (self.root / "CHANGELOG.md").write_text(
            f"# Changelog\n\n## [Unreleased]\n\n## [{self.version}] - {date.today()}\n\n"
            "- Fixture ([#261](https://github.com/devoption/devoption/issues/261)).\n\n" +
            render_metadata(metadata, digest) + "\n")
        self.commit("chore: prepare bundle fixture")
        self.binary = self.root / "fixture-elf"
        data = bytearray(64)
        data[:7] = b"\x7fELF\x02\x01\x01"
        data[16:20] = b"\x03\x00\x3e\x00"
        self.binary.write_bytes(data)
        self.runtime = self.root / "devoption-runtime"
        self.runtime.write_bytes(data)
        # Build output is deliberately ignored, like target/ in the real repository.
        (self.root / ".gitignore").write_text("fixture-elf\ndevoption-runtime\n")
        self.commit("test: ignore compiled fixture")
        self.output = self.root.parent / (self.root.name + "-candidate")
        self.addCleanup(self.clean_output)

    def clean_output(self):
        import shutil
        if self.output.is_dir():
            shutil.rmtree(self.output)

    def assemble(self, output=None):
        with patch("release_bundle.require_host"), patch("release_bundle.probe") as check, patch("release_bundle.probe_runtime") as runtime_check:
            result = assemble(self.root, self.binary, output or self.output)
            self.assertEqual(check.call_count, 1)
            self.assertEqual(runtime_check.call_count, 1)
        return result

    def test_repeatable_complete_payload_and_manifest(self):
        first = self.assemble()
        manifest = json.loads((self.output / "manifest.json").read_bytes())
        self.assertFalse(manifest["publication_eligible"])
        self.assertEqual(manifest["source_commit"], git(self.root, "rev-parse", "HEAD").strip())
        self.assertEqual(set(manifest["components"].values()), {self.version})
        self.assertEqual(manifest["release_metadata"]["compatibility"]["upgrade_from"], [])
        with tarfile.open(self.output / first["archive"]) as archive:
            members = archive.getmembers()
            self.assertEqual({m.name for m in members}, {*SOURCE_FILES, "bin/devoption", RUNTIME, "RELEASE-NOTES.md", "manifest.json", SBOM})
            self.assertEqual(archive.extractfile("manifest.json").read(), (self.output / "manifest.json").read_bytes())
            for record in manifest["files"]:
                entry = archive.getmember(record["path"])
                content = archive.extractfile(entry).read()
                self.assertEqual(hashlib.sha256(content).hexdigest(), record["sha256"])
                self.assertEqual(len(content), record["size"])
                self.assertEqual(entry.mode, int(record["mode"], 8))
                self.assertEqual((entry.uid, entry.gid, entry.mtime), (0, 0, 0))
        original = {p.name: p.read_bytes() for p in self.output.iterdir()}
        self.clean_output()
        self.assertEqual(self.assemble(), first)
        self.assertEqual({p.name: p.read_bytes() for p in self.output.iterdir()}, original)
        self.assertEqual(check(self.root, self.binary, self.output), first["sha256"])

    def test_checker_rejects_tampering_even_with_recomputed_checksum(self):
        result = self.assemble()
        archive = self.output / result["archive"]
        with tarfile.open(archive) as original:
            entries = [(m, original.extractfile(m).read()) for m in original]
        # Match the declared executable size while changing its actual bytes.
        with tarfile.open(archive, mode="w:gz") as changed:
            for member, content in entries:
                if member.name == "bin/devoption":
                    content = content[:-1] + b"x"
                changed.addfile(member, io.BytesIO(content))
        (self.output / "SHA256SUMS").write_text(f"{sha256(archive.read_bytes())}  {archive.name}\n")
        with self.assertRaisesRegex(ValueError, "member differs"):
            check(self.root, self.binary, self.output)

    def test_checker_rejects_duplicate_and_missing_members(self):
        result = self.assemble()
        archive = self.output / result["archive"]
        with tarfile.open(archive) as original:
            member = original.getmember("bin/devoption")
            content = original.extractfile(member).read()
        for copies, expected in [(2, "repeated"), (1, "missing")]:
            with self.subTest(copies=copies):
                with tarfile.open(archive, mode="w:gz") as changed:
                    for _ in range(copies):
                        changed.addfile(member, io.BytesIO(content))
                (self.output / "SHA256SUMS").write_text(f"{sha256(archive.read_bytes())}  {archive.name}\n")
                with self.assertRaisesRegex(ValueError, expected):
                    check(self.root, self.binary, self.output)

    def test_checker_rejects_unsigned_statement_and_inventory_edits(self):
        result = self.assemble()
        statement_path = self.output / PROVENANCE
        statement = json.loads(statement_path.read_bytes())
        self.assertEqual(statement["subject"][0]["digest"]["sha256"], result["sha256"])
        self.assertEqual(statement["predicate"]["buildDefinition"]["externalParameters"]["authentication"], "unsigned")
        statement["subject"][0]["digest"]["sha256"] = "0" * 64
        statement_path.write_text(json.dumps(statement))
        with self.assertRaisesRegex(ValueError, "provenance subject"):
            check(self.root, self.binary, self.output)
        (self.output / SBOM).write_text("{}")
        with self.assertRaisesRegex(ValueError, "SBOM differs"):
            check(self.root, self.binary, self.output)

    def test_existing_candidate_is_preserved(self):
        first = self.assemble()
        before = (self.output / first["archive"]).read_bytes()
        with self.assertRaisesRegex(ValueError, "new output"):
            self.assemble()
        self.assertEqual((self.output / first["archive"]).read_bytes(), before)

    def test_dirty_checkout_and_output_alias_are_rejected(self):
        (self.root / "uncommitted").write_text("private fixture")
        with self.assertRaisesRegex(ValueError, "Commit changes"):
            self.assemble()
        (self.root / "uncommitted").unlink()
        alias = self.root.parent / (self.root.name + "-alias")
        alias.symlink_to(self.root, target_is_directory=True)
        self.addCleanup(alias.unlink)
        with self.assertRaisesRegex(ValueError, "outside"):
            self.assemble(alias / "output")
        self.assertFalse(self.output.exists())

    def test_source_symlink_is_rejected(self):
        path = self.root / "scripts/build_broker.py"
        path.unlink()
        path.symlink_to("/etc/passwd")
        self.commit("test: replace helper with symlink")
        with self.assertRaisesRegex(ValueError, "committed regular"):
            self.assemble()
        self.assertFalse(self.output.exists())

    def test_wrong_architecture_symlink_and_fifo_are_rejected(self):
        self.binary.write_bytes(b"MZ" + b"x" * 100)
        with self.assertRaisesRegex(ValueError, "ELF"):
            binary_bytes(self.binary)
        link = self.root / "binary-link"
        link.symlink_to(self.binary)
        with self.assertRaises(OSError):
            binary_bytes(link)
        link.unlink()
        os.mkfifo(link)
        with self.assertRaisesRegex(ValueError, "regular"):
            binary_bytes(link)

    def test_trusted_cargo_hardlink_is_snapshotted(self):
        link = self.root / "cargo-hardlink"
        os.link(self.binary, link)
        self.assertEqual(binary_bytes(link), self.binary.read_bytes())

    def test_missing_metadata_and_wrong_version_leave_no_output(self):
        path = self.root / "release-metadata.toml"
        path.write_text(path.read_text().replace(self.version, "0.3.0-preview.1"))
        self.commit("test: stale metadata")
        with self.assertRaisesRegex(ValueError, "match proposed"):
            self.assemble()
        self.assertFalse(self.output.exists())

    def test_probe_mismatch_leaves_no_candidate(self):
        with patch("release_bundle.require_host"), patch("release_bundle.probe", side_effect=ValueError("version mismatch")):
            with self.assertRaisesRegex(ValueError, "version mismatch"):
                assemble(self.root, self.binary, self.output)
        self.assertFalse(self.output.exists())

    def test_real_probe_rejects_wrong_embedded_version(self):
        fake = self.root / "wrong-version"
        fake.write_text("#!/bin/sh\nprintf 'devoption 9.9.9\\n'\n")
        fake.chmod(0o700)
        with self.assertRaisesRegex(ValueError, "selected version"):
            probe(fake, self.version)

    def test_worker_is_required_and_its_bytes_and_inventory_are_verified(self):
        first = self.assemble()
        manifest = json.loads((self.output / "manifest.json").read_bytes())
        self.assertEqual(manifest['schema_version'], 3)
        self.assertEqual(manifest['app_runtime']['path'], RUNTIME)
        self.assertEqual(manifest['app_runtime']['version'], self.version)
        self.assertEqual(manifest['embedded_components'], ['server', 'CLI', 'UI'])
        record = next(r for r in manifest['files'] if r['path'] == RUNTIME)
        self.assertEqual(record['mode'], '0755')
        self.assertEqual(record['sha256'], sha256(self.runtime.read_bytes()))
        statement = json.loads((self.output / PROVENANCE).read_bytes())
        materials = statement['predicate']['buildDefinition']['resolvedDependencies']
        self.assertIn({'name': RUNTIME, 'digest': {'sha256': record['sha256']}}, materials)
        self.runtime.write_bytes(self.runtime.read_bytes() + b'changed worker')
        with self.assertRaisesRegex(ValueError, 'manifest differs'):
            check(self.root, self.binary, self.output)
        self.clean_output(); self.runtime.unlink()
        with self.assertRaises(OSError):
            self.assemble()
        self.assertFalse(self.output.exists())

    def test_worker_probe_failure_never_publishes_candidate(self):
        with patch('release_bundle.require_host'), patch('release_bundle.probe'), \
                patch('release_bundle.probe_runtime', side_effect=ValueError('wrong worker version')):
            with self.assertRaisesRegex(ValueError, 'worker version'):
                assemble(self.root, self.binary, self.output)
        self.assertFalse(self.output.exists())

    def test_unsupported_host_is_rejected_before_reading_inputs(self):
        with patch("release_bundle.platform.system", return_value="Darwin"):
            with self.assertRaisesRegex(ValueError, "Ubuntu"):
                assemble(Path("/absent/source"), Path("/absent/binary"), self.output)


if __name__ == "__main__":
    import unittest
    unittest.main()

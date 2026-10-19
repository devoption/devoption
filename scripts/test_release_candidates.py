"""Exercise version reservation retries, transport trust and Git immutability."""

import hashlib
from concurrent.futures import ThreadPoolExecutor
import subprocess
import unittest
from unittest.mock import patch
import zipfile

from bundle_candidate import identity
from release_bundle import canonical
from release_candidates import audit, existing_bytes, load, reserve, validate
from release_plan import git
import test_bundle_provenance as fixtures


class ReservationTests(unittest.TestCase):
    repack = fixtures.VerificationTests.repack
    response = fixtures.VerificationTests.response

    def setUp(self):
        fixtures.VerificationTests.setUp(self)
        self.repository = self.root / "repository"
        self.repository.mkdir()
        git(self.repository, "init", "--quiet")
        git(self.repository, "config", "user.name", "Candidate fixture")
        git(self.repository, "config", "user.email", "fixture@example.invalid")
        git(self.repository, "config", "commit.gpgSign", "false")
        (self.repository / "Cargo.toml").write_text(f'[workspace.package]\nversion = "{self.version}"\n')
        self.commit()
        self.source = git(self.repository, "rev-parse", "HEAD").strip()
        self.manifest["source_commit"] = self.source
        self.repack()
        self.candidate = identity(self.source, self.version, self.digest,
                                  hashlib.sha256(self.proof.read_bytes()).hexdigest(), "12345", "1")
        self.files = {self.name: self.archive.read_bytes(), "attestation.json": self.proof.read_bytes(),
                      "candidate.json": canonical(self.candidate)}
        self.transport = self.root / "packet.zip"
        self.catalog = self.repository / "release-candidates"
        self.rezip()

    def commit(self):
        git(self.repository, "add", ".")
        git(self.repository, "commit", "--quiet", "-m", "test: candidate fixture", "-m", "Refs #261")

    def rezip(self, entries=None):
        with zipfile.ZipFile(self.transport, "w") as archive:
            for name, content in entries if entries is not None else self.files.items():
                archive.writestr(name, content)
        self.record = {"schema_version": 1, "candidate": self.candidate,
                       "artifact": {"id": "67890", "transport_sha256": hashlib.sha256(self.transport.read_bytes()).hexdigest()}}

    def reserve(self, record=None):
        with patch("verify_bundle_provenance.command", side_effect=[fixtures.GH_VERSION.encode(), self.response()]):
            return reserve(self.catalog, record or self.record, self.transport, self.gh)

    def test_identical_retry_is_a_noop_and_conflicting_locator_never_replaces(self):
        self.assertEqual(self.reserve(), "reserved")
        destination = self.catalog / (self.version + ".json")
        original = destination.read_bytes()
        inode = destination.stat().st_ino
        self.assertEqual(self.reserve(), "already_reserved")
        conflicting = self.record | {"artifact": self.record["artifact"] | {"id": "67891"}}
        with self.assertRaisesRegex(ValueError, "already reserved"):
            self.reserve(conflicting)
        self.assertEqual(destination.read_bytes(), original)
        self.assertEqual(destination.stat().st_ino, inode)

    def test_unavailable_tampered_or_unverifiable_transport_never_reserves(self):
        with patch("verify_bundle_provenance.command", side_effect=[fixtures.GH_VERSION.encode(), subprocess.CalledProcessError(1, "gh")]):
            with self.assertRaises(subprocess.CalledProcessError):
                reserve(self.catalog, self.record, self.transport, self.gh)
        self.assertFalse(self.catalog.exists())
        self.transport.write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "Transport ZIP differs"):
            self.reserve()
        self.transport.unlink()
        with self.assertRaises(FileNotFoundError):
            self.reserve()
        self.assertFalse(self.catalog.exists())

    def test_concurrent_identical_reservations_reconcile_atomically(self):
        def response(arguments, environment):
            return fixtures.GH_VERSION.encode() if arguments[-1] == "--version" else self.response()
        with patch("verify_bundle_provenance.command", side_effect=response), ThreadPoolExecutor(max_workers=2) as pool:
            results = list(pool.map(lambda _: reserve(self.catalog, self.record, self.transport, self.gh), range(2)))
        self.assertEqual(sorted(results), ["already_reserved", "reserved"])
        self.assertEqual([p.name for p in self.catalog.iterdir()], [self.version + ".json"])

    def test_partial_existing_record_is_never_repaired_or_overwritten(self):
        self.catalog.mkdir()
        destination = self.catalog / (self.version + ".json")
        destination.write_bytes(b"partial")
        with self.assertRaisesRegex(ValueError, "already reserved"):
            self.reserve()
        self.assertEqual(destination.read_bytes(), b"partial")

    def test_single_metadata_race_retries_but_continued_instability_fails(self):
        self.reserve()
        destination = self.catalog / (self.version + ".json")
        expected = destination.read_bytes()
        from verify_bundle_provenance import snapshot
        calls = 0
        def read(path, limit):
            nonlocal calls
            calls += 1
            if calls == 1:
                raise ValueError("Verification input changed while reading.")
            return snapshot(path, limit)
        with patch("release_candidates.snapshot", side_effect=read):
            self.assertEqual(existing_bytes(destination), expected)
        self.assertEqual(calls, 2)
        with patch("release_candidates.snapshot", side_effect=ValueError("unstable")) as read:
            with self.assertRaisesRegex(ValueError, "unstable"):
                existing_bytes(destination)
        self.assertEqual(read.call_count, 2)

    def test_zip_missing_unexpected_duplicate_or_traversal_paths_are_rejected(self):
        for entries in ([next(iter(self.files.items()))], list(self.files.items()) + [("../escape", b"bad")],
                        [("../escape", b"bad"), ("attestation.json", b"bad"), ("candidate.json", b"bad")]):
            self.rezip(entries)
            with self.assertRaisesRegex(ValueError, "exactly the bounded"):
                self.reserve()
        # Duplicate names can evade a set-only inventory comparison.
        with self.assertWarns(UserWarning):
            self.rezip([(self.name, b"x"), (self.name, b"x"), ("candidate.json", b"{}")])
        with self.assertRaises(ValueError):
            self.reserve()

    def test_repacked_receipt_cannot_redefine_the_reviewed_candidate(self):
        self.files["candidate.json"] = canonical(self.candidate | {"run_attempt": "2"})
        self.rezip()
        with self.assertRaisesRegex(ValueError, "receipt differs"):
            self.reserve()
        self.assertFalse(self.catalog.exists())

    def test_malformed_noncanonical_or_eligible_records_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "canonical"):
            load(canonical(self.record) + b" ")
        for changed in (self.record | {"extra": True}, self.record | {"schema_version": True},
                        self.record | {"candidate": self.candidate | {"publication_eligible": True}},
                        self.record | {"artifact": self.record["artifact"] | {"id": "../escape"}}):
            with self.assertRaises(ValueError):
                validate(changed)

    def test_git_audit_accepts_addition_but_rejects_edits_removal_and_reassignment(self):
        base = self.source
        self.reserve()
        self.commit()
        self.assertEqual(audit(self.repository, base), 1)
        reserved = git(self.repository, "rev-parse", "HEAD").strip()
        destination = self.catalog / (self.version + ".json")
        raw = destination.read_bytes()
        destination.write_bytes(raw.replace(b'"67890"', b'"67891"'))
        self.commit()
        with self.assertRaisesRegex(ValueError, "cannot be edited"):
            audit(self.repository, reserved)
        destination.unlink()
        self.commit()
        with self.assertRaisesRegex(ValueError, "cannot be edited"):
            audit(self.repository, reserved)

    def test_source_version_and_filename_must_match_and_source_must_exist(self):
        self.reserve()
        destination = self.catalog / (self.version + ".json")
        destination.rename(self.catalog / "other.json")
        self.commit()
        with self.assertRaisesRegex(ValueError, "filename"):
            audit(self.repository)
        (self.catalog / "other.json").unlink()
        changed = self.record | {"candidate": identity(self.source, "9.0.0-preview.1", self.digest,
                                                       self.candidate["proof_sha256"], "12345", "1")}
        (self.catalog / "9.0.0-preview.1.json").write_bytes(canonical(changed))
        self.commit()
        with self.assertRaisesRegex(ValueError, "embedded version"):
            audit(self.repository)
        (self.catalog / "9.0.0-preview.1.json").unlink()
        changed = self.record | {"candidate": self.candidate | {"source_commit": "f" * 40,
                  "artifact_name": "bundle-" + "f" * 40 + "-12345-1"}}
        (self.catalog / (self.version + ".json")).write_bytes(canonical(changed))
        self.commit()
        with self.assertRaises(subprocess.CalledProcessError):
            audit(self.repository)

    def test_git_audit_rejects_executable_or_linked_catalog_records(self):
        self.reserve()
        destination = self.catalog / (self.version + ".json")
        destination.chmod(0o755)
        self.commit()
        with self.assertRaisesRegex(ValueError, "nonexecutable"):
            audit(self.repository)
        destination.unlink()
        destination.symlink_to("../Cargo.toml")
        self.commit()
        with self.assertRaisesRegex(ValueError, "nonexecutable"):
            audit(self.repository)


if __name__ == "__main__":
    unittest.main()

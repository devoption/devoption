"""Exercise retained-byte identity and failure boundaries without signing or installing."""

import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import unittest
from unittest.mock import patch
import zipfile

from bundle_candidate import check, identity, stage
from release_bundle import canonical
import test_bundle_provenance as fixtures
from test_bundle_provenance import GH_VERSION, SOURCE


class CandidateTests(unittest.TestCase):
    def setUp(self):
        # Reuse a complete inert archive fixture, never a compiled executable.
        fixtures.VerificationTests.setUp(self)
        self.candidate = self.root / "assembled"
        self.candidate.mkdir()
        shutil.copyfile(self.archive, self.candidate / self.name)
        (self.candidate / "manifest.json").write_bytes(canonical(self.manifest))
        (self.candidate / "SHA256SUMS").write_text(f"{self.digest}  {self.name}\n")
        self.output = self.root / "retained"

    repack = fixtures.VerificationTests.repack
    response = fixtures.VerificationTests.response

    def stage(self):
        return stage(self.candidate, self.proof, self.output, SOURCE, "12345", "1", self.gh)

    def stage_fixture(self):
        with patch("verify_bundle_provenance.command", side_effect=[GH_VERSION.encode(), self.response()]):
            return self.stage()

    def test_transport_round_trip_preserves_bytes_and_rechecks_crypto(self):
        expected = self.stage_fixture()
        self.assertEqual(expected["artifact_name"], f"bundle-{SOURCE}-12345-1")
        self.assertFalse(expected["publication_eligible"])
        self.assertEqual((self.output / self.name).read_bytes(), self.archive.read_bytes())
        self.assertEqual((self.output / "attestation.json").read_bytes(), self.proof.read_bytes())
        transport = self.root / "transport.zip"
        with zipfile.ZipFile(transport, "w") as archive:
            for path in self.output.iterdir():
                archive.write(path, path.name)
        downloaded = self.root / "downloaded"
        with zipfile.ZipFile(transport) as archive:
            archive.extractall(downloaded)  # Only our three known local fixture paths.
        with patch("verify_bundle_provenance.command", side_effect=[GH_VERSION.encode(), self.response()]) as crypto:
            self.assertEqual(check(downloaded, expected, self.gh), expected)
        self.assertEqual(crypto.call_count, 2)

    def test_collisions_never_replace_existing_bytes(self):
        expected = self.stage_fixture()
        original = {p.name: p.read_bytes() for p in self.output.iterdir()}
        with self.assertRaisesRegex(ValueError, "already exists"):
            self.stage()
        self.assertEqual({p.name: p.read_bytes() for p in self.output.iterdir()}, original)
        second_attempt = identity(SOURCE, self.version, self.digest, expected["proof_sha256"], "12345", "2")
        self.assertNotEqual(expected["artifact_name"], second_attempt["artifact_name"])

    def test_unverified_packet_is_never_staged(self):
        with patch("verify_bundle_provenance.command", side_effect=[GH_VERSION.encode(), subprocess.CalledProcessError(1, "gh")]):
            with self.assertRaises(subprocess.CalledProcessError):
                self.stage()
        self.assertFalse(self.output.exists())

    def test_wrong_source_checksum_and_unsafe_version_stop_before_crypto(self):
        for change in ("source", "checksum", "version"):
            with self.subTest(change=change):
                manifest = self.manifest.copy()
                if change == "source":
                    manifest["source_commit"] = "b" * 40
                if change == "version":
                    manifest["platform_version"] = "../../escape"
                (self.candidate / "manifest.json").write_bytes(canonical(manifest))
                (self.candidate / "SHA256SUMS").write_text(
                    f"{'0' * 64 if change == 'checksum' else self.digest}  {self.name}\n")
                with patch("verify_bundle_provenance.command") as crypto:
                    with self.assertRaises(ValueError):
                        self.stage()
                crypto.assert_not_called()
                self.assertFalse(self.output.exists())

    def test_download_changes_cannot_redefine_expected_identity(self):
        expected = self.stage_fixture()
        original = {p.name: p.read_bytes() for p in self.output.iterdir()}
        for name in (self.name, "attestation.json", "candidate.json"):
            with self.subTest(name=name):
                (self.output / name).write_bytes(b"changed")
                # Recomputing a receipt from tampered bytes cannot alter job outputs.
                if name != "candidate.json":
                    forged = expected | {"archive_sha256" if name == self.name else "proof_sha256":
                                         hashlib.sha256(b"changed").hexdigest()}
                    (self.output / "candidate.json").write_bytes(canonical(forged))
                with patch("verify_bundle_provenance.command") as crypto:
                    with self.assertRaises(ValueError):
                        check(self.output, expected, self.gh)
                crypto.assert_not_called()
                for filename, raw in original.items():
                    (self.output / filename).write_bytes(raw)
        (self.output / "attestation.json").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "digest differs"):
            check(self.output, expected, self.gh)

    def test_missing_extra_symlink_and_directory_members_fail_closed(self):
        expected = self.stage_fixture()
        proof = self.output / "attestation.json"
        proof.unlink()
        with self.assertRaisesRegex(ValueError, "missing or unexpected"):
            check(self.output, expected, self.gh)
        proof.symlink_to(self.proof)
        with self.assertRaises(OSError):
            check(self.output, expected, self.gh)
        proof.unlink()
        proof.mkdir()
        with self.assertRaises((ValueError, OSError)):
            check(self.output, expected, self.gh)
        proof.rmdir()
        shutil.copyfile(self.proof, proof)
        (self.output / "unexpected").write_text("extra")
        with self.assertRaisesRegex(ValueError, "missing or unexpected"):
            check(self.output, expected, self.gh)

    def test_wrong_run_or_attempt_cannot_relabel_packet(self):
        expected = self.stage_fixture()
        for overrides in ({"run_id": "67890"}, {"run_attempt": "2"}, {"publication_eligible": True}):
            (self.output / "candidate.json").write_bytes(canonical(expected | overrides))
            with self.assertRaisesRegex(ValueError, "receipt differs"):
                check(self.output, expected, self.gh)

    @unittest.skipUnless(os.environ.get("DEVOPTION_ATTEST_GH"), "set DEVOPTION_ATTEST_GH for real forged-proof rejection")
    def test_real_cli_forgery_cannot_produce_a_retained_packet(self):
        self.gh = Path(os.environ["DEVOPTION_ATTEST_GH"])
        with self.assertRaises(subprocess.CalledProcessError) as caught:
            self.stage()
        self.assertIn(b"bundle content could not be parsed", caught.exception.stderr)
        self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()

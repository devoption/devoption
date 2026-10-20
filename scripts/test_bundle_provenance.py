"""Verification policy tests; mocked success is not a cryptographic acceptance claim."""

import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import tomllib
import unittest
from unittest.mock import patch

from release_bundle import SOURCE_FILES, TARGET, canonical, write_archive, RUNTIME, file_mode, runtime_versions
from release_inventory import SBOM
from verify_bundle_provenance import GH_VERSION, IDENTITY, PREDICATE, WORKFLOW, require_workflow, verify

ROOT = Path(__file__).resolve().parents[1]
SOURCE = "a" * 40


class VerificationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.gh = self.root / "gh"
        self.gh.write_text("fixture path only; command calls mocked")
        self.proof = self.root / "proof.json"
        self.proof.write_text('{"forged":true}')
        # Most policy tests follow the current candidate. Historical upgrade
        # tests can supply their explicitly versioned synthetic source policy.
        metadata_raw = getattr(self, 'fixture_metadata', None)
        metadata_raw = metadata_raw.encode() if metadata_raw is not None else (ROOT / 'release-metadata.toml').read_bytes()
        self.version = tomllib.loads(metadata_raw.decode())["version"]
        self.name = f"devoption-{self.version}-{TARGET}.tar.gz"
        self.archive = self.root / self.name
        self.files = {name: (ROOT / name).read_bytes() for name in SOURCE_FILES}
        self.files['release-metadata.toml'] = metadata_raw
        self.files |= {"bin/devoption": b"policy fixture; never executed", "RELEASE-NOTES.md": b"fixture", SBOM: b"{}"}
        metadata = tomllib.loads(self.files["release-metadata.toml"].decode())
        self.manifest = {"schema_version": 2, "source_commit": SOURCE, "platform_version": self.version,
                         "target": TARGET, "publication_eligible": False, "release_metadata": metadata,
                         "release_metadata_sha256": hashlib.sha256(self.files["release-metadata.toml"]).hexdigest(),
                         "files": [{"path": name, "size": len(content), "sha256": hashlib.sha256(content).hexdigest(),
                                    "mode": "0755" if name == "bin/devoption" else "0644"}
                                   for name, content in sorted(self.files.items())]}
        self.repack()

    def repack(self):
        self.archive.unlink(missing_ok=True)
        write_archive(self.archive, self.files, self.manifest)
        self.digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()

    def response(self):
        return canonical([{"verificationResult": {"statement": {
            "predicateType": PREDICATE, "subject": [{"name": self.name, "digest": {"sha256": self.digest}}]}}}])

    def verify(self):
        return verify(self.archive, self.proof, SOURCE, self.digest, self.version, self.gh)

    def schema_three(self):
        self.files[RUNTIME] = b'authenticated worker fixture; never executed'
        self.manifest.update(schema_version=3, embedded_components=['server', 'CLI', 'UI'],
            app_runtime={'path': RUNTIME, 'version': self.version,
                         'embedded_runtime_versions': runtime_versions(tomllib.loads(self.files['Cargo.lock'].decode()))},
            cargo_lock_sha256=hashlib.sha256(self.files['Cargo.lock']).hexdigest())
        self.manifest['files'] = [{'path': name, 'size': len(content), 'sha256': hashlib.sha256(content).hexdigest(),
                                  'mode': f'{file_mode(name):04o}'} for name, content in sorted(self.files.items())]
        self.repack()

    def test_dual_executable_manifest_verifies_without_running_payloads(self):
        self.schema_three()
        with patch('verify_bundle_provenance.command', side_effect=[GH_VERSION.encode(), self.response()]) as command:
            self.assertTrue(self.verify()['authenticated'])
        self.assertEqual(command.call_count, 2)  # Pinned verifier only, never a bundle executable.
        self.files[RUNTIME] += b'tampered'
        self.repack()
        with patch('verify_bundle_provenance.command', side_effect=[GH_VERSION.encode(), self.response()]):
            with self.assertRaisesRegex(ValueError, 'digests/modes'):
                self.verify()

    def test_worker_descriptor_missing_bytes_wrong_versions_and_schema_downgrades_fail(self):
        self.schema_three()
        for change in ('missing', 'version', 'lock', 'path', 'downgrade'):
            files, manifest = self.files.copy(), json.loads(json.dumps(self.manifest))
            if change == 'missing': self.files.pop(RUNTIME)
            elif change == 'version': self.manifest['app_runtime']['version'] = '9.0.0-preview.1'
            elif change == 'lock': self.manifest['app_runtime']['embedded_runtime_versions']['wasmtime'] = '0.0.0'
            elif change == 'path': self.manifest['app_runtime']['path'] = '../worker'
            else: self.manifest['schema_version'] = 2
            self.repack()
            with self.subTest(change=change), patch('verify_bundle_provenance.command', side_effect=[GH_VERSION.encode(), self.response()]):
                with self.assertRaises(ValueError): self.verify()
            self.files, self.manifest = files, manifest
        self.repack()

    def test_fixed_cryptographic_policy_precedes_manifest_processing(self):
        with patch("verify_bundle_provenance.command", side_effect=[GH_VERSION.encode(), self.response()]) as command:
            self.assertTrue(self.verify()["authenticated"])
        args, environment = command.call_args.args
        required = {"--repo": "devoption/devoption",
                    "--cert-identity": IDENTITY, "--source-ref": "refs/heads/main",
                    "--source-digest": SOURCE, "--signer-digest": SOURCE,
                    "--cert-oidc-issuer": "https://token.actions.githubusercontent.com",
                    "--predicate-type": PREDICATE, "--hostname": "github.com"}
        for flag, expected in required.items():
            self.assertEqual(args[args.index(flag) + 1], expected)
        self.assertIn("--deny-self-hosted-runners", args)
        self.assertNotIn("--signer-workflow", args)  # gh treats this as mutually exclusive with exact cert identity.
        self.assertNotIn("GH_TOKEN", environment)
        self.assertNotIn("GITHUB_TOKEN", environment)
        self.assertNotIn("GH_HOST", environment)

    def test_signature_failure_never_reads_the_manifest(self):
        with patch("verify_bundle_provenance.command", side_effect=[GH_VERSION.encode(), subprocess.CalledProcessError(1, "gh")]), \
                patch("verify_bundle_provenance.manifest_from_archive") as parse:
            with self.assertRaises(subprocess.CalledProcessError):
                self.verify()
        parse.assert_not_called()

    def test_wrong_digest_and_symlink_rejected_before_crypto(self):
        with patch("verify_bundle_provenance.command") as command:
            with self.assertRaisesRegex(ValueError, "reviewed digest"):
                verify(self.archive, self.proof, SOURCE, "0" * 64, self.version, self.gh)
        command.assert_not_called()
        alias = self.root / "alias"
        alias.symlink_to(self.archive)
        with self.assertRaises(OSError):
            verify(alias, self.proof, SOURCE, self.digest, self.version, self.gh)

    def test_wrong_tool_empty_results_wrong_subject_and_predicate_rejected(self):
        outputs = [b"[]", canonical([{"verificationResult": {"statement": {"predicateType": PREDICATE, "subject": []}}}]),
                   self.response().replace(PREDICATE.encode(), b"https://cyclonedx.org/bom"),
                   self.response().replace(self.digest.encode(), b"0" * 64)]
        for output in outputs:
            with self.subTest(output=output[:30]), patch("verify_bundle_provenance.command", side_effect=[GH_VERSION.encode(), output]):
                with self.assertRaises(ValueError):
                    self.verify()
        with patch("verify_bundle_provenance.command", return_value=b"gh version 0.0.1"):
            with self.assertRaisesRegex(ValueError, "pinned"):
                self.verify()

    def test_manifest_source_version_target_and_eligibility_must_match(self):
        for key, value in [("source_commit", "b" * 40), ("platform_version", "9.9.9-preview.1"),
                           ("target", "other"), ("publication_eligible", True)]:
            original = self.manifest[key]
            self.manifest[key] = value
            self.repack()
            with self.subTest(key=key), patch("verify_bundle_provenance.command", side_effect=[GH_VERSION.encode(), self.response()]):
                with self.assertRaisesRegex(ValueError, "reviewed preview"):
                    self.verify()
            self.manifest[key] = original

    def test_signed_but_inconsistent_payload_is_rejected(self):
        self.files["bin/devoption"] = b"changed bytes"
        self.repack()
        with patch("verify_bundle_provenance.command", side_effect=[GH_VERSION.encode(), self.response()]):
            with self.assertRaisesRegex(ValueError, "digests/modes"):
                self.verify()

    def test_unsafe_members_and_privileged_modes_are_rejected_without_extraction(self):
        for kind in ("traversal", "symlink", "duplicate", "setuid"):
            with self.subTest(kind=kind):
                member = tarfile.TarInfo("../escape" if kind == "traversal" else "file")
                member.mode = 0o4644 if kind == "setuid" else 0o644
                member.size = 3
                if kind == "symlink":
                    member.type = tarfile.SYMTYPE
                    member.linkname = "/etc/passwd"
                    member.size = 0
                with tarfile.open(self.archive, "w:gz") as archive:
                    archive.addfile(member, io.BytesIO(b"bad"))
                    if kind == "duplicate":
                        archive.addfile(member, io.BytesIO(b"bad"))
                self.digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
                with patch("verify_bundle_provenance.command", side_effect=[GH_VERSION.encode(), self.response()]):
                    with self.assertRaisesRegex(ValueError, "unsafe or repeated|not normalized"):
                        self.verify()

    def test_signing_context_denies_missing_opt_in_branch_and_failed_checks(self):
        context = {"GITHUB_ACTIONS": "true", "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REPOSITORY": "devoption/devoption",
                   "GITHUB_REF": "refs/heads/main", "GITHUB_SERVER_URL": "https://github.com", "GITHUB_SHA": SOURCE,
                   "GITHUB_WORKFLOW_REF": WORKFLOW + "@refs/heads/main", "SOURCE_CHECKS": "success", "SIGN_APPROVED": "true"}
        for field, bad in [("SIGN_APPROVED", "false"), ("SOURCE_CHECKS", "failure"), ("GITHUB_REF", "refs/pull/1/merge"),
                           ("GITHUB_EVENT_NAME", "pull_request"), ("GITHUB_SHA", "b" * 40)]:
            with self.subTest(field=field), patch.dict(os.environ, context | {field: bad}, clear=True), \
                    patch("verify_bundle_provenance.git") as git:
                with self.assertRaisesRegex(ValueError, "opted-in"):
                    require_workflow(SOURCE, self.root)
                git.assert_not_called()
        with patch.dict(os.environ, context, clear=True), patch("verify_bundle_provenance.git", side_effect=[SOURCE, ""]):
            require_workflow(SOURCE, self.root)
        with patch.dict(os.environ, context, clear=True), patch("verify_bundle_provenance.git", side_effect=[SOURCE, "dirty"]):
            with self.assertRaisesRegex(ValueError, "clean"):
                require_workflow(SOURCE, self.root)

    @unittest.skipUnless(os.environ.get("DEVOPTION_ATTEST_GH"), "set DEVOPTION_ATTEST_GH to pinned gh for forged-proof integration")
    def test_real_pinned_gh_rejects_a_forged_proof(self):
        with self.assertRaises(subprocess.CalledProcessError) as caught:
            verify(self.archive, self.proof, SOURCE, self.digest, self.version, Path(os.environ["DEVOPTION_ATTEST_GH"]))
        self.assertIn(b"bundle content could not be parsed", caught.exception.stderr)


if __name__ == "__main__":
    unittest.main()

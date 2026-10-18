"""Inventory graph and checksum validation using the real locked dependency matrix."""

from pathlib import Path
import tomllib
import unittest

from release_inventory import VENDOR_FILES, inventory

ROOT = Path(__file__).resolve().parents[1]


class InventoryTests(unittest.TestCase):
    def setUp(self):
        self.files = {name: (ROOT / name).read_bytes() for name in ("Cargo.lock", *VENDOR_FILES)}
        self.files["bin/devoption"] = b"fixed trusted binary fixture"
        self.lock = tomllib.loads(self.files["Cargo.lock"].decode())
        self.names = {p["name"] for p in self.lock["package"] if "source" not in p}
        self.version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]

    def inventory(self):
        return inventory(self.files, self.version, "a" * 40, self.names)

    def test_every_locked_identity_edge_and_source_checksum_is_preserved(self):
        bom = self.inventory()
        self.assertEqual(self.inventory(), bom)
        components = {p["bom-ref"]: p for p in bom["components"]}
        cargo = {ref: p for ref, p in components.items() if ref.startswith("pkg:cargo/")}
        self.assertEqual(len(cargo), len(self.lock["package"]))
        graph = {d["ref"]: d["dependsOn"] for d in bom["dependencies"]}
        for package in self.lock["package"]:
            matches = [ref for ref, component in cargo.items()
                       if (component["name"], component["version"]) == (package["name"], package["version"])]
            self.assertEqual(len(matches), 1)
            ref = matches[0]
            if "checksum" in package:
                self.assertEqual(cargo[ref]["hashes"][0]["content"], package["checksum"])
            self.assertEqual(len(graph[ref]), len(package.get("dependencies", [])))
            self.assertTrue(set(graph[ref]) <= set(components))
        self.assertIn("pkg:generic/highlight.js@11.12.0", components)
        self.assertEqual(bom["compositions"][0]["aggregate"], "incomplete")
        self.assertNotIn("/Users/", str(bom))

    def test_ambiguous_and_missing_dependencies_fail(self):
        original = self.files["Cargo.lock"]
        for replacement in (b'"cpufeatures"', b'"absent-crate"'):
            self.files["Cargo.lock"] = original.replace(b'"cpufeatures 0.2.17"', replacement)
            with self.subTest(replacement=replacement), self.assertRaisesRegex(ValueError, "missing or ambiguous"):
                self.inventory()

    def test_unsupported_source_missing_checksum_and_duplicate_identity_fail(self):
        original = self.files["Cargo.lock"]
        checksum = next(p["checksum"] for p in self.lock["package"] if "checksum" in p)
        cases = [original.replace(b'registry+https://github.com/rust-lang/crates.io-index', b'git+https://example.invalid/crate'),
                 original.replace(checksum.encode(), b"bad"),
                 original + b'\n[[package]]\nname = "addr2line"\nversion = "0.26.1"\n']
        for data in cases:
            self.files["Cargo.lock"] = data
            with self.assertRaises(ValueError):
                self.inventory()

    def test_vendor_version_disagreement_fails(self):
        self.files[VENDOR_FILES[0]] = self.files[VENDOR_FILES[0]].replace(b"11.12.0", b"99.0.0")
        with self.assertRaisesRegex(ValueError, "Highlight.js"):
            self.inventory()


if __name__ == "__main__":
    unittest.main()

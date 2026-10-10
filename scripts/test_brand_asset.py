"""Keep the reviewed logo static and preserve the approved path geometries."""

import hashlib
from pathlib import Path
import unittest
import xml.etree.ElementTree as ET

ASSET = Path(__file__).resolve().parents[1] / "crates/devoption-ui/assets/devoption.svg"
SVG = "{http://www.w3.org/2000/svg}"


class BrandAssetTests(unittest.TestCase):
    def test_svg_contains_only_reviewed_static_markup(self):
        source = ASSET.read_text()
        self.assertNotIn("<!DOCTYPE", source.upper())
        self.assertNotIn("<!ENTITY", source.upper())
        self.assertNotIn("<?", source)
        root = ET.fromstring(source)
        self.assertEqual(root.tag, SVG + "svg")
        self.assertEqual(root.attrib, {
            "viewBox": "0 0 380.943 60.0338", "class": "brand-mark",
            "aria-hidden": "true", "focusable": "false",
        })
        self.assertEqual(len(root), 2)
        for index, path in enumerate(root):
            self.assertEqual(path.tag, SVG + "path")
            self.assertEqual(len(path), 0)
            self.assertEqual(set(path.attrib), {"fill", "d", "class"} if index == 0 else {"fill", "d"})
            self.assertEqual(path.attrib["fill"], "currentColor")
        self.assertEqual(root[0].attrib["class"], "brand-dev")

    def test_geometries_match_the_owner_approved_original(self):
        paths = ET.fromstring(ASSET.read_text())
        self.assertEqual(
            [hashlib.sha256(path.attrib["d"].encode()).hexdigest() for path in paths],
            [
                "3d15be1657a61c7a2086c86e3add4624cae04bae4f88a514bd05d8f1a93d6399",
                "9b85a5c56f910f51d031c456fa49e7a5028f7834d29385c7912b08f1b094dc62",
            ],
        )


if __name__ == "__main__":
    unittest.main()

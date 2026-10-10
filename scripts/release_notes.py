"""Print reviewed release notes; never create tags, releases, or registry uploads."""

from datetime import date
from pathlib import Path
import re
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def extract_notes(version, package_version, changelog):
    if not re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", version):
        raise ValueError("Choose a stable version such as 0.1.0 (without a v prefix).")
    if version != package_version:
        raise ValueError("Release version must match workspace.package.version in Cargo.toml.")
    headings = list(re.finditer(r"^## .+$", changelog, re.M))
    matches = [i for i, heading in enumerate(headings)
               if heading.group().startswith(f"## [{version}]")]
    if len(matches) != 1:
        raise ValueError("Expected exactly one dated changelog section for this version.")
    index = matches[0]
    heading = headings[index]
    dated = re.fullmatch(rf"## \[{re.escape(version)}\] - (\d{{4}}-\d{{2}}-\d{{2}})", heading.group())
    if not dated or date.fromisoformat(dated[1]) > date.today():
        raise ValueError("Release heading must contain a valid date, not in the future.")
    end = headings[index + 1].start() if index + 1 < len(headings) else len(changelog)
    body = changelog[heading.end():end].strip()
    if not re.search(r"^- .+", body, re.M) or not re.search(
        r"https://github.com/devoption/devoption/issues/[1-9][0-9]*\b", body
    ):
        raise ValueError("Release notes need a change bullet and a repository issue link.")
    return f"{heading.group()}\n\n{body}\n"


def main():
    if len(sys.argv) != 2:
        raise ValueError("Usage: python3 scripts/release_notes.py VERSION")
    with (ROOT / "Cargo.toml").open("rb") as manifest:
        version = tomllib.load(manifest)["workspace"]["package"]["version"]
    print(extract_notes(sys.argv[1], version, (ROOT / "CHANGELOG.md").read_text()), end="")


if __name__ == "__main__":
    try:
        main()
    except ValueError as error:
        sys.exit(str(error))

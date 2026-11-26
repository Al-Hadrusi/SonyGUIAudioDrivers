#!/usr/bin/env python3
"""Write the versioned ebuild for the version in Cargo.toml.

Run from the repository root after bumping the version and before tagging:

    python3 packaging/gentoo/make-release-ebuild.py

It turns the live (9999) ebuild into a release ebuild: the sources come from
the GitHub tag archive, and every crate from Cargo.lock is listed in CRATES so
Portage can download them (builds run offline).
"""
import pathlib
import re
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]
PKG = ROOT / "packaging/gentoo/media-sound/headphones-control"
LIVE = PKG / "headphones-control-9999.ebuild"

version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
crates = sorted(
    f"{p['name']}@{p['version']}"
    for p in lock["package"]
    if p.get("source", "").startswith("registry+")
)
if not crates:
    sys.exit("no registry crates found in Cargo.lock")

text = LIVE.read_text()
text = text.replace("inherit cargo desktop git-r3 xdg", "inherit cargo desktop xdg")
text = re.sub(r'^EGIT_REPO_URI=.*\n', "", text, flags=re.M)
text = re.sub(r'^EGIT_BRANCH=.*\n', "", text, flags=re.M)
text = re.sub(r"\nsrc_unpack\(\) \{\n.*?\n\}\n", "", text, flags=re.S)
crate_block = "CRATES=\"\n" + "".join(f"\t{c}\n" for c in crates) + '"\n\n'
text = text.replace('RUST_MIN_VER=', crate_block + "RUST_MIN_VER=", 1)
text = text.replace(
    'HOMEPAGE="https://github.com/Al-Hadrusi/SonyGUIAudioDrivers"\n',
    'HOMEPAGE="https://github.com/Al-Hadrusi/SonyGUIAudioDrivers"\n'
    'SRC_URI="\n'
    '\thttps://github.com/Al-Hadrusi/SonyGUIAudioDrivers/archive/refs/tags/v${PV}.tar.gz -> ${P}.tar.gz\n'
    '\t${CARGO_CRATE_URIS}\n'
    '"\n'
    'S="${WORKDIR}/SonyGUIAudioDrivers-${PV}"\n',
)
text = text.replace('SLOT="0"\n', 'SLOT="0"\nKEYWORDS="~amd64"\n')

for needle in ("git-r3", "EGIT_", "cargo_live_src_unpack"):
    if needle in text:
        sys.exit(f"conversion incomplete: {needle!r} still present")

out = PKG / f"headphones-control-{version}.ebuild"
out.write_text(text)
print(f"wrote {out.relative_to(ROOT)} with {len(crates)} crates")
print(
    "Now update the Manifest (checksums Portage verifies downloads against) once the tag\n"
    "is pushed, check the tarball matches `git archive` of the tag, and commit it:\n"
    f"    ebuild {out.relative_to(ROOT)} manifest"
)

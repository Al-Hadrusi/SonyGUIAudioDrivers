#!/bin/sh
# Release gate, run by .github/workflows/release.yml before anything is built
# or published. Usage:
#
#   check-release.sh <version> <commit> [<protected branch ref>]
#
# Refuses (exit 1) unless ALL of these hold:
#   1. <version> is a plain semantic version (optionally prefixed with "v"),
#      checked as a whole string (no newlines, slashes or shell syntax).
#   2. <commit> is already on the protected branch (default origin/main), so
#      only reviewed, merged code can be released, never a side branch.
#   3. Cargo.toml at <commit> declares exactly that version.
#   4. If tag v<version> exists, it points at <commit>: re-running a release
#      for a published version from different code would otherwise replace
#      the binaries people already downloaded and trust.
set -eu

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
    echo "usage: $0 <version> <commit> [<protected branch ref>]" >&2
    exit 2
fi
VERSION=$1
COMMIT=$2
PROTECTED=${3:-origin/main}

fail() {
    echo "release check failed: $*" >&2
    exit 1
}

V=${VERSION#v}
# POSIX sh has no regex match; reject anything outside the allowed alphabet,
# then check the shape. Command substitution would hide a trailing newline,
# so test for one explicitly.
case "$V" in
    *"
"*) fail "version contains a newline" ;;
    "" | *[!0-9A-Za-z.-]*) fail "version has unexpected characters" ;;
esac
printf '%s' "$V" | grep -Eqx '[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?' ||
    fail "version must look like 1.2.3 or 1.2.3-rc.1, got '$V'"

SHA=$(git rev-parse --verify --quiet "$COMMIT^{commit}") || fail "unknown commit $COMMIT"
git rev-parse --verify --quiet "$PROTECTED^{commit}" >/dev/null ||
    fail "protected branch $PROTECTED not found (fetch full history first)"
git merge-base --is-ancestor "$SHA" "$PROTECTED" ||
    fail "commit $SHA is not on $PROTECTED; merge it there before releasing"

CARGO_VERSION=$(git show "$SHA:Cargo.toml" |
    awk '/^\[/{p=($0=="[package]")} p && /^version *=/{gsub(/^version *= *"|".*$/,""); print; exit}')
[ "$CARGO_VERSION" = "$V" ] ||
    fail "Cargo.toml at $SHA says version '$CARGO_VERSION', not '$V'"

if TAGGED=$(git rev-parse --verify --quiet "refs/tags/v$V^{commit}"); then
    [ "$TAGGED" = "$SHA" ] ||
        fail "tag v$V already exists on $TAGGED; refusing to republish it from $SHA"
fi

echo "release check passed: v$V from $SHA (on $PROTECTED)"

#!/bin/sh
# Refuse to publish into a release that already exists, run by the publish
# job of .github/workflows/release.yml right before uploading. Usage:
#
#   GH_TOKEN=... GITHUB_REPOSITORY=owner/repo check-not-published.sh <tag>
#
# Re-running a release, even for the same commit, would replace the assets
# people already downloaded (builds are not bit-for-bit reproducible, so
# their checksums and attestations would stop matching). This fails closed:
# only a definite "404 Not Found" from the GitHub API lets publishing go on;
# an existing release, an API error or no answer all stop it.
#
# The token is passed to curl on stdin, never as an argument, so it does not
# appear in the process list or in logs.
set -eu

[ "$#" -eq 1 ] || { echo "usage: $0 <tag>" >&2; exit 2; }
TAG=$1
case "$TAG" in
    v[0-9]*) ;;
    *) echo "refusing: unexpected tag '$TAG'" >&2; exit 1 ;;
esac
case "$TAG" in
    *[!0-9A-Za-z.v-]*) echo "refusing: unexpected tag '$TAG'" >&2; exit 1 ;;
esac
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY must be set}"
: "${GH_TOKEN:?GH_TOKEN must be set}"
API=${GITHUB_API_URL:-https://api.github.com}

STATUS=$(printf 'Authorization: Bearer %s\n' "$GH_TOKEN" |
    curl -sS -o /dev/null -w '%{http_code}' --max-time 30 -H @- \
        -H 'Accept: application/vnd.github+json' \
        "$API/repos/$GITHUB_REPOSITORY/releases/tags/$TAG") || STATUS=000

case "$STATUS" in
    404) echo "ok: no release for $TAG yet" ;;
    200) echo "refusing: release $TAG is already published; publish a new version instead" >&2
         exit 1 ;;
    *) echo "refusing: could not confirm $TAG is unpublished (HTTP $STATUS)" >&2
       exit 1 ;;
esac

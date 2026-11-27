# Security

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub: **Security → Report a vulnerability** on
this repository (private vulnerability reporting). Don't open a public issue.

## What this app is (and isn't)

Headphones Control is a local desktop app. It has no server, no user accounts, no passwords or
tokens, no payments and no network client. It talks only to the paired headset over Bluetooth
RFCOMM and stores one small settings file. So the controls that matter are:

| Asset / high-risk action | Who could abuse it | Control |
| --- | --- | --- |
| **Publishing a release** (binaries every user installs) | Anyone who can run the release workflow, push a `v*` tag, or push to `main`, including someone holding a maintainer's stolen session or token | Release gate in the repo (below) **plus** the GitHub settings in "Pending" |
| Commands to the headset (settings, power off) | A nearby device that pretends to be the headset, or answers in its place | Linux: only the selected, paired headset's authenticated link is accepted (`src/transport/bluetooth.rs`). Power off asks for confirmation |
| Text the headset or Bluetooth sends (names, firmware, labels) | Any nearby device | Parsers are bounds-checked and fuzzed. Text is cleaned and escaped before it reaches notifications or the tray |
| Settings file (last headset's Bluetooth address) | Other local users | Written owner-only (`0600`), atomically, without following symlinks |

## Controls in this repository (covered by tests)

- **Release gate:** `packaging/release/check-release.sh`, run first by `.github/workflows/release.yml`.
  It refuses unless the version is a plain semver matching `Cargo.toml`, the commit is already on
  `main`, and an existing `v<version>` tag points at that same commit (so a published release
  can't be overwritten from other code). Tests: `tests/release_gate.rs`.
- Release jobs get a read-only token by default. Only the publish job gets `contents: write`, and
  it runs in the `release` environment. Actions are pinned to commit SHAs and kept updated by
  Dependabot. Releases carry signed build provenance (`gh attestation verify`).
- CI runs `cargo audit` against the RustSec advisory database.
- No secrets are used by the workflows or stored in the repository.

## Pending: GitHub settings a maintainer must apply

These live in the repository and account settings, which can't be changed or inspected from a
pull request. On 2026-11-27 GitHub reported `main` as **not protected**, so the release gate's
"commit is on `main`" check is only as strong as whoever can push to `main`. Apply these, then
run the checks. Commands use the GitHub CLI as a repository admin.

> **Lock-out warning (sole maintainer):** don't require pull-request *approvals* or enable
> "prevent self-review" unless a second maintainer can approve. With one maintainer, require pull
> requests and passing checks but keep yourself on the bypass list.

1. **Maintainer account: phishing-resistant sign-in.** GitHub → Settings → Password and
   authentication: register a passkey or a hardware security key, keep the recovery codes offline,
   and remove SMS as a 2FA method. Review active sessions there and revoke unknown ones. Revoke
   unused personal access tokens (Settings → Developer settings) and OAuth/GitHub App grants
   (Settings → Applications).
   Verify: `gh api user --jq .two_factor_authentication` prints `true`. Confirm in that settings
   page that a passkey or security key is listed and SMS is not.
2. **Protect `main`** (Settings → Rules → Rulesets → New branch ruleset, target `main`): block
   force pushes and deletion, and require a pull request before merging. Require the status checks
   `Format, lint, test`, `Windows build and test` and `Dependency advisories`. Add only the
   maintainer role to the bypass list.
   Verify: `gh api repos/Al-Hadrusi/SonyGUIAudioDrivers/branches/main --jq .protected` prints `true`.
3. **Protect release tags** (new tag ruleset, target `v*`): restrict creation, block updates and
   deletion, with the same bypass list.
   Verify: `gh api repos/Al-Hadrusi/SonyGUIAudioDrivers/rulesets --jq '.[] | {name, target, enforcement}'`
   lists both rulesets as `active`.
4. **Gate the `release` environment** (Settings → Environments → `release`, created by the first
   release run or by hand): add yourself as a required reviewer. Under deployment branches and
   tags, allow only `main` and tags `v*`. Every publish then waits for an explicit approval, from
   a fresh sign-in if your session has expired.
   Verify: `gh api repos/Al-Hadrusi/SonyGUIAudioDrivers/environments/release --jq '.protection_rules'`
   shows a `required_reviewers` rule and a `branch_policy` rule.
5. **Actions defaults** (Settings → Actions → General): set workflow permissions to "Read repository
   contents", and turn off "Allow GitHub Actions to create and approve pull requests".
   Verify: `gh api repos/Al-Hadrusi/SonyGUIAudioDrivers/actions/permissions/workflow` shows
   `"default_workflow_permissions":"read"` and `"can_approve_pull_request_reviews":false`.
6. **Releases and reporting** (Settings → General): turn on release immutability if your plan
   offers it, so published assets can't be replaced. Turn on private vulnerability reporting
   (Settings → Code security).
   Verify: `gh api repos/Al-Hadrusi/SonyGUIAudioDrivers/private-vulnerability-reporting` shows
   `"enabled":true`.

## Pending: needs real hardware

The Bluetooth controls (accept only the selected headset, require an authenticated link) are
compiled and type-checked but haven't been exercised against a real headset. Verify with a
paired headset: it connects. Then unpair it in `bluetoothctl` (`remove <MAC>`) without deleting
the app's settings: the app must not get a control connection without a new pairing (expect a
connection error or a pairing prompt, never a silent connect).

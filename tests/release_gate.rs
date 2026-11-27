//! Regression tests for the release gate (`packaging/release/check-release.sh`),
//! the only thing standing between "someone can run the release workflow" and
//! "binaries are published to every user". Each test builds a throwaway git
//! repository with synthetic commits and runs the real script against it.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.dir)
            // Isolated from the developer's own git configuration.
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .output()
            .expect("git must be installed to run these tests");
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn commit(&self, version: &str, note: &str) -> String {
        // A dependency with its own `version` key must not be mistaken for
        // the package version.
        let cargo = format!(
            "[package]\nname = \"demo\"\nversion = \"{version}\"\n\n[dependencies]\nfoo = {{ version = \"9.9.9\" }}\n"
        );
        std::fs::write(self.dir.join("Cargo.toml"), cargo).unwrap();
        std::fs::write(self.dir.join("NOTES"), note).unwrap();
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", note]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn refs(&self) -> String {
        self.git(&["show-ref"])
    }

    fn check(&self, version: &str, commit: &str) -> Output {
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/release/check-release.sh");
        Command::new("sh")
            .arg(script)
            .args([version, commit, "main"])
            .current_dir(&self.dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap()
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// main: old (0.1.0, tag v0.1.0) -> released (0.2.0, tag v0.2.0)
///       -> later (0.2.0, untagged) -> next (0.3.0, untagged)
/// side branch from next: unmerged (0.3.0)
struct Fixture {
    repo: Repo,
    released: String,
    later: String,
    next: String,
    unmerged: String,
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("hpc-release-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let repo = Repo { dir };
    repo.git(&["init", "-q", "-b", "main"]);
    repo.commit("0.1.0", "old");
    repo.git(&["tag", "v0.1.0"]);
    let released = repo.commit("0.2.0", "released");
    repo.git(&["tag", "v0.2.0"]);
    let later = repo.commit("0.2.0", "later");
    let next = repo.commit("0.3.0", "next");
    repo.git(&["checkout", "-q", "-b", "side"]);
    let unmerged = repo.commit("0.3.0", "unreviewed change");
    repo.git(&["checkout", "-q", "main"]);
    Fixture {
        repo,
        released,
        later,
        next,
        unmerged,
    }
}

fn assert_denied(f: &Fixture, version: &str, commit: &str, why: &str) {
    let before = f.repo.refs();
    let out = f.repo.check(version, commit);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{why}: expected refusal, got {out:?}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("release check failed"), "{why}: {stderr}");
    // A refused release changes nothing: same tags and branches.
    assert_eq!(f.repo.refs(), before, "{why}: refs changed");
}

#[test]
fn allows_merged_commit_with_matching_version() {
    let f = fixture("allow");
    for v in ["0.3.0", "v0.3.0"] {
        let out = f.repo.check(v, &f.next);
        assert!(out.status.success(), "{v}: {out:?}");
    }
    // Re-running the tag-push case for an existing tag on the same commit.
    assert!(f.repo.check("0.2.0", &f.released).status.success());
}

#[test]
fn refuses_commits_not_on_the_protected_branch() {
    let f = fixture("branch");
    assert_denied(&f, "0.3.0", &f.unmerged, "unmerged side-branch commit");
}

#[test]
fn refuses_republishing_an_existing_version_from_other_code() {
    let f = fixture("republish");
    // v0.2.0 is already published from `released`; `later` also says 0.2.0.
    assert_denied(&f, "0.2.0", &f.later, "overwrite of published v0.2.0");
}

#[test]
fn refuses_version_not_matching_cargo_toml() {
    let f = fixture("mismatch");
    assert_denied(&f, "0.4.0", &f.next, "version not in Cargo.toml");
    assert_denied(&f, "9.9.9", &f.next, "dependency version, not package");
}

#[test]
fn refuses_malformed_versions() {
    let f = fixture("malformed");
    for bad in ["0.3.0\nEVIL=1", "0.3.0/../x", "0.3", "", "0.3.0 ", "$(id)"] {
        assert_denied(&f, bad, &f.next, &format!("malformed {bad:?}"));
    }
}

#[test]
fn refuses_unknown_commits() {
    let f = fixture("unknown");
    assert_denied(
        &f,
        "0.3.0",
        "0000000000000000000000000000000000000000",
        "unknown commit",
    );
}

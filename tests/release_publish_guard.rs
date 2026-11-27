//! Regression tests for `packaging/release/check-not-published.sh`: the
//! publish job must never replace the assets of an existing release, and it
//! must fail closed. A tiny local HTTP server stands in for the GitHub API;
//! the token is synthetic.
#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::mpsc;
use std::thread;

const TOKEN: &str = "synthetic-token-123";

/// Serve one request with `status`; returns what the client sent.
fn fake_api(status: u16) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let n = conn.read(&mut buf).unwrap_or(0);
        let _ = tx.send(String::from_utf8_lossy(&buf[..n]).into_owned());
        let _ = write!(
            conn,
            "HTTP/1.1 {status} X\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
        );
    });
    (url, rx)
}

fn run(api: &str, tag: &str) -> Output {
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/release/check-not-published.sh");
    Command::new("sh")
        .arg(script)
        .arg(tag)
        .env("GITHUB_API_URL", api)
        .env("GITHUB_REPOSITORY", "example/demo")
        .env("GH_TOKEN", TOKEN)
        .output()
        .unwrap()
}

fn assert_no_token_in_output(out: &Output) {
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!all.contains(TOKEN), "token leaked into output: {all}");
}

#[test]
fn allows_publishing_when_no_release_exists() {
    let (api, request) = fake_api(404);
    let out = run(&api, "v1.2.3");
    assert!(out.status.success(), "{out:?}");
    let req = request.recv().unwrap();
    assert!(
        req.starts_with("GET /repos/example/demo/releases/tags/v1.2.3 "),
        "{req}"
    );
    // The token is sent to the API, but never printed.
    assert!(
        req.contains(&format!("Authorization: Bearer {TOKEN}")),
        "{req}"
    );
    assert_no_token_in_output(&out);
}

#[test]
fn refuses_when_release_is_already_published() {
    let (api, _request) = fake_api(200);
    let out = run(&api, "v1.2.3");
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("already published"));
    assert_no_token_in_output(&out);
}

#[test]
fn fails_closed_on_api_errors_and_no_answer() {
    for status in [401, 403, 500, 502] {
        let (api, _request) = fake_api(status);
        let out = run(&api, "v1.2.3");
        assert_eq!(out.status.code(), Some(1), "HTTP {status}: {out:?}");
        assert_no_token_in_output(&out);
    }
    // Nothing listening at all.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let out = run(&format!("http://127.0.0.1:{port}"), "v1.2.3");
    assert_eq!(out.status.code(), Some(1), "unreachable API: {out:?}");
}

#[test]
fn refuses_unexpected_tags_without_calling_the_api() {
    for bad in ["1.2.3", "v1.2.3/../x", "v1.2.3?x=1", "main"] {
        // No server: a request would make curl fail, but the script must
        // refuse before that, with its own message.
        let out = run("http://127.0.0.1:9", bad);
        assert_eq!(out.status.code(), Some(1), "{bad}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("unexpected tag"),
            "{bad}: {out:?}"
        );
    }
}

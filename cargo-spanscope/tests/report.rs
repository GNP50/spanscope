//! End-to-end package-level report generation from the real runtime fixture.

use flate2::write::GzEncoder;
use flate2::Compression;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
fn workdir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "spanscope-cli-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../viewer/fixtures/example.json")
}
fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-spanscope"))
        .args(args)
        .env("SPANSCOPE_OPEN", "0")
        .output()
        .unwrap()
}

#[test]
fn inline_report_is_self_contained_and_escapes_hostile_names() {
    let dir = workdir();
    let mut profile: Value = serde_json::from_slice(&fs::read(fixture()).unwrap()).unwrap();
    profile["spans"][0]["name"] = "</script><script>window.__pwned=1</script>".into();
    let input = dir.join("hostile.json");
    fs::write(&input, serde_json::to_vec(&profile).unwrap()).unwrap();
    let report = dir.join("report");
    let output = run(&[
        "spanscope",
        input.to_str().unwrap(),
        "--output",
        report.to_str().unwrap(),
        "--no-open",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let html = fs::read_to_string(report.join("index.html")).unwrap();
    assert!(html.contains("\"mode\":\"inline\""));
    assert!(!html.contains("</script><script>window.__pwned"));
    assert!(html.contains("\\u003c/script\\u003e\\u003cscript\\u003ewindow.__pwned"));
    assert!(!html.contains("src=\"./assets/") && !html.contains("href=\"./assets/"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn gzip_inline_and_forced_sidecar() {
    let dir = workdir();
    let raw = fs::read(fixture()).unwrap();
    let input = dir.join("profile.json.gz");
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&raw).unwrap();
    fs::write(&input, encoder.finish().unwrap()).unwrap();
    let inline = dir.join("inline");
    let result = run(&[input.to_str().unwrap(), "-o", inline.to_str().unwrap()]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(fs::read_to_string(inline.join("index.html"))
        .unwrap()
        .contains("\"mode\":\"inline\""));
    let sidecar = dir.join("sidecar");
    let result = run(&[
        input.to_str().unwrap(),
        "--output",
        sidecar.to_str().unwrap(),
        "--inline-limit-mib",
        "0",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(fs::read_to_string(sidecar.join("index.html"))
        .unwrap()
        .contains("\"mode\":\"picker\""));
    assert_eq!(
        fs::read(sidecar.join("profile.json.gz")).unwrap(),
        fs::read(input).unwrap()
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unsupported_schema_fails_before_output() {
    let dir = workdir();
    let mut profile: Value = serde_json::from_slice(&fs::read(fixture()).unwrap()).unwrap();
    profile["schema_version"] = 2.into();
    let input = dir.join("future.json");
    fs::write(&input, serde_json::to_vec(&profile).unwrap()).unwrap();
    let report = dir.join("report");
    let output = run(&[
        input.to_str().unwrap(),
        "--output",
        report.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported schema_version"));
    assert!(!report.join("index.html").exists());
    fs::remove_dir_all(dir).unwrap();
}

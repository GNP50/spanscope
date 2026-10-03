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

#[test]
fn embeds_candidate_and_baseline_for_offline_comparison() {
    let dir = workdir();
    let report = dir.join("report");
    let fixture = fixture();
    let output = run(&[
        fixture.to_str().unwrap(),
        "--baseline",
        fixture.to_str().unwrap(),
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
    assert!(html.contains("id=\"spanscope-profile\""));
    assert!(html.contains("id=\"spanscope-baseline\""));
    assert!(html.contains("\"baseline_mode\":\"inline\""));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn regression_gate_writes_markdown_and_keeps_the_report() {
    let dir = workdir();
    let baseline = fixture();
    let mut candidate: Value = serde_json::from_slice(&fs::read(&baseline).unwrap()).unwrap();
    let original = candidate["chains"][1]["self_ns"].as_u64().unwrap();
    candidate["chains"][1]["self_ns"] = (original * 2).into();
    let candidate_path = dir.join("candidate.json");
    fs::write(&candidate_path, serde_json::to_vec(&candidate).unwrap()).unwrap();
    let report = dir.join("report");
    let summary = dir.join("summary.md");
    let output = run(&[
        candidate_path.to_str().unwrap(),
        "--baseline",
        baseline.to_str().unwrap(),
        "--fail-if",
        "self:+10%",
        "--summary",
        summary.to_str().unwrap(),
        "--output",
        report.to_str().unwrap(),
        "--no-open",
    ]);
    assert!(!output.status.success());
    assert!(report.join("index.html").exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("regression gate failed"));
    let markdown = fs::read_to_string(summary).unwrap();
    assert!(markdown.contains("**Result: FAIL**"));
    assert!(markdown.contains("demo::transform"));
    assert!(markdown.contains("self:+10%"));

    let passing = run(&[
        candidate_path.to_str().unwrap(),
        "--baseline",
        baseline.to_str().unwrap(),
        "--fail-if",
        "self:+200%",
        "--output",
        dir.join("passing").to_str().unwrap(),
        "--no-open",
    ]);
    assert!(
        passing.status.success(),
        "{}",
        String::from_utf8_lossy(&passing.stderr)
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn regression_options_require_a_baseline_and_valid_rule() {
    let dir = workdir();
    let fixture = fixture();
    let without_baseline = run(&[
        fixture.to_str().unwrap(),
        "--fail-if",
        "p99:+10%",
        "--output",
        dir.to_str().unwrap(),
    ]);
    assert!(!without_baseline.status.success());
    assert!(String::from_utf8_lossy(&without_baseline.stderr).contains("require --baseline"));
    let invalid = run(&[
        fixture.to_str().unwrap(),
        "--baseline",
        fixture.to_str().unwrap(),
        "--fail-if",
        "mean:+10%",
    ]);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("unknown comparison metric"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn writes_chrome_and_speedscope_exports() {
    let dir = workdir();
    let fixture = fixture();
    let chrome = dir.join("exports/trace.json");
    let speedscope = dir.join("exports/profile.speedscope.json");
    let output = run(&[
        fixture.to_str().unwrap(),
        "--export",
        &format!("chrome:{}", chrome.display()),
        "--export",
        &format!("speedscope:{}", speedscope.display()),
        "--output",
        dir.join("report").to_str().unwrap(),
        "--no-open",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let chrome: Value = serde_json::from_slice(&fs::read(chrome).unwrap()).unwrap();
    assert!(chrome["traceEvents"].as_array().unwrap().len() > 1);
    assert_eq!(chrome["traceEvents"][1]["ph"], "X");

    let speedscope: Value = serde_json::from_slice(&fs::read(speedscope).unwrap()).unwrap();
    assert_eq!(
        speedscope["$schema"],
        "https://www.speedscope.app/file-format-schema.json"
    );
    assert!(!speedscope["profiles"][0]["samples"]
        .as_array()
        .unwrap()
        .is_empty());
    fs::remove_dir_all(dir).unwrap();
}

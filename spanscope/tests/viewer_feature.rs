//! Diagnostics when an application requests opening without the optional viewer asset.
#![cfg(all(feature = "enabled", not(feature = "viewer")))]

use spanscope::config::ConfigBuilder;
use std::process::Command;

#[spanscope::trace(root)]
fn work() {
    std::hint::black_box(1);
}

#[test]
fn child_without_viewer_feature() {
    if std::env::var_os("SPANSCOPE_VIEWER_TEST_CHILD").is_none() {
        return;
    }
    let path = std::env::var("SPANSCOPE_VIEWER_TEST_PATH").unwrap();
    ConfigBuilder::new()
        .output(path)
        .auto_flush(false)
        .init()
        .unwrap();
    work();
    spanscope::export::flush().unwrap();
}

#[test]
fn opening_request_reports_missing_feature_but_keeps_profile() {
    let path =
        std::env::temp_dir().join(format!("spanscope-no-viewer-{}.json", std::process::id()));
    let output = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("child_without_viewer_feature")
        .arg("--nocapture")
        .env("SPANSCOPE_VIEWER_TEST_CHILD", "1")
        .env("SPANSCOPE_VIEWER_TEST_PATH", &path)
        .env("SPANSCOPE_OPEN", "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires the `viewer` feature"));
    assert!(path.exists());
    std::fs::remove_file(path).unwrap();
}

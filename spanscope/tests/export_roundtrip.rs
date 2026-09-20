//! Export round-trip, replacement, failure, and lifecycle tests.
#![cfg(feature = "enabled")]

use flate2::read::GzDecoder;
use spanscope::collection::snapshot;
use spanscope::config::{ConfigBuilder, OutputFormat};
use spanscope::export::{flush, write_snapshot};
use spanscope::profile::{EvidenceStatus, Profile};
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

#[spanscope::trace(root)]
fn root() {
    child();
}

#[spanscope::trace(tags("leaf", "leaf"))]
fn child() {
    std::hint::black_box(17);
}

fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "spanscope-{label}-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ))
}

fn config(path: PathBuf, format: OutputFormat) -> spanscope::config::Config {
    ConfigBuilder::new()
        .output(path)
        .format(format)
        .auto_flush(false)
        .program("roundtrip")
        .resolve()
        .unwrap()
}

#[test]
fn json_gzip_text_roundtrip_and_repeat() {
    root();
    let first = snapshot();
    let json = temp_path("roundtrip.json");
    write_snapshot(&first, &config(json.clone(), OutputFormat::Json)).unwrap();
    let first_profile: Profile = serde_json::from_slice(&std::fs::read(&json).unwrap()).unwrap();
    assert_eq!(first_profile.schema_version, 1);
    assert!(first_profile.meta.started_at.ends_with('Z'));
    assert!(first_profile
        .roots
        .iter()
        .all(|root| root.execution.status != EvidenceStatus::Complete));
    assert!(first_profile.chains.iter().all(|chain| chain
        .histogram
        .buckets
        .iter()
        .map(|bucket| bucket.count)
        .sum::<u64>()
        == chain.count));
    root();
    write_snapshot(&snapshot(), &config(json.clone(), OutputFormat::Json)).unwrap();
    let second: Profile = serde_json::from_slice(&std::fs::read(&json).unwrap()).unwrap();
    assert!(second.roots.len() > first_profile.roots.len());
    let gzip = temp_path("roundtrip.json.gz");
    write_snapshot(&snapshot(), &config(gzip.clone(), OutputFormat::Gzip)).unwrap();
    let mut contents = String::new();
    GzDecoder::new(std::fs::File::open(&gzip).unwrap())
        .read_to_string(&mut contents)
        .unwrap();
    let compressed: Profile = serde_json::from_str(&contents).unwrap();
    assert_eq!(compressed.chains.len(), second.chains.len());
    let text = temp_path("roundtrip.txt");
    write_snapshot(&snapshot(), &config(text.clone(), OutputFormat::Text)).unwrap();
    assert!(std::fs::read_to_string(&text)
        .unwrap()
        .contains("Calls  Total(ns)"));
    for path in [json, gzip, text] {
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn failed_output_preserves_existing_destination() {
    root();
    let path = temp_path("no-overwrite.json");
    std::fs::write(&path, "previous").unwrap();
    let mut configured = config(path.clone(), OutputFormat::Json);
    configured.overwrite = false;
    assert!(write_snapshot(&snapshot(), &configured).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous");
    std::fs::remove_file(path).unwrap();
    let missing = temp_path("missing-parent").join("profile.json");
    assert!(write_snapshot(&snapshot(), &config(missing, OutputFormat::Json)).is_err());
}

#[test]
fn remapped_ids_and_order_are_stable() {
    root();
    let original = snapshot();
    let mut shuffled = original.clone();
    let span_count = shuffled.spans.len() as u32;
    shuffled.spans.reverse();
    for (id, span) in shuffled.spans.iter_mut().enumerate() {
        span.id = id as u32;
    }
    let remap = |path: &mut Vec<u32>| {
        for id in path {
            *id = span_count - 1 - *id;
        }
    };
    for chain in &mut shuffled.chains {
        remap(&mut chain.path);
    }
    shuffled.chains.reverse();
    for root in &mut shuffled.roots {
        root.span = span_count - 1 - root.span;
        for chain in &mut root.chains {
            remap(&mut chain.path);
        }
        for invocation in &mut root.invocations {
            remap(&mut invocation.path);
        }
        root.chains.reverse();
        root.invocations.reverse();
    }
    shuffled.roots.reverse();
    shuffled.threads.reverse();
    let first = temp_path("ordered-a.json");
    let second = temp_path("ordered-b.json");
    write_snapshot(&original, &config(first.clone(), OutputFormat::Json)).unwrap();
    write_snapshot(&shuffled, &config(second.clone(), OutputFormat::Json)).unwrap();
    let left: Profile = serde_json::from_slice(&std::fs::read(&first).unwrap()).unwrap();
    let right: Profile = serde_json::from_slice(&std::fs::read(&second).unwrap()).unwrap();
    assert_eq!(left.spans, right.spans);
    assert_eq!(left.chains, right.chains);
    assert_eq!(left.graph, right.graph);
    assert_eq!(left.roots, right.roots);
    assert_eq!(left.threads, right.threads);
    std::fs::remove_file(first).unwrap();
    std::fs::remove_file(second).unwrap();
}

#[test]
fn normal_exit_child() {
    if std::env::var_os("SPANSCOPE_TEST_EXIT_CHILD").is_none() {
        return;
    }
    let path = std::env::var("SPANSCOPE_TEST_EXIT_PATH").unwrap();
    ConfigBuilder::new()
        .output(path)
        .auto_flush(true)
        .init()
        .unwrap();
    root();
}

#[test]
fn normal_exit_writes_report() {
    let path = temp_path("exit.json");
    let status = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("normal_exit_child")
        .arg("--nocapture")
        .env("SPANSCOPE_TEST_EXIT_CHILD", "1")
        .env("SPANSCOPE_TEST_EXIT_PATH", &path)
        .status()
        .unwrap();
    assert!(status.success());
    let profile: Profile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(!profile.roots.is_empty());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn args_child() {
    if std::env::var_os("SPANSCOPE_TEST_ARGS_CHILD").is_none() {
        return;
    }
    let output = std::env::var("SPANSCOPE_TEST_ARGS_PATH").unwrap();
    let remaining = spanscope::config::init_from_args([
        "demo",
        "--spanscope-output",
        &output,
        "--spanscope-sample-rate=1",
        "--spanscope-auto-flush",
        "false",
        "--application-flag",
        "--",
        "--spanscope-format=text",
    ])
    .unwrap();
    assert_eq!(
        remaining,
        [
            "demo",
            "--application-flag",
            "--",
            "--spanscope-format=text"
        ]
    );
    root();
    flush().unwrap();
    root();
    flush().unwrap();
}

#[test]
fn arguments_override_environment_and_explicit_flush_repeats() {
    let path = temp_path("args.json");
    let status = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("args_child")
        .env("SPANSCOPE_TEST_ARGS_CHILD", "1")
        .env("SPANSCOPE_TEST_ARGS_PATH", &path)
        .env("SPANSCOPE_SAMPLE_RATE", "0")
        .status()
        .unwrap();
    assert!(status.success());
    let profile: Profile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(profile.meta.capture.sample_rate, 1.0);
    assert!(profile.roots.len() >= 2);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn concurrent_flush_child() {
    if std::env::var_os("SPANSCOPE_TEST_CONCURRENT_CHILD").is_none() {
        return;
    }
    let path = std::env::var("SPANSCOPE_TEST_CONCURRENT_PATH").unwrap();
    ConfigBuilder::new()
        .output(path)
        .auto_flush(false)
        .init()
        .unwrap();
    let workers = (0..4)
        .map(|_| {
            std::thread::spawn(|| {
                for _ in 0..100 {
                    root();
                }
            })
        })
        .collect::<Vec<_>>();
    let flushers = (0..4)
        .map(|_| {
            std::thread::spawn(|| {
                for _ in 0..4 {
                    flush().unwrap();
                }
            })
        })
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().unwrap();
    }
    for flusher in flushers {
        flusher.join().unwrap();
    }
    flush().unwrap();
}

#[test]
fn concurrent_flushes_leave_valid_final_profile() {
    let path = temp_path("concurrent.json");
    let status = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("concurrent_flush_child")
        .env("SPANSCOPE_TEST_CONCURRENT_CHILD", "1")
        .env("SPANSCOPE_TEST_CONCURRENT_PATH", &path)
        .status()
        .unwrap();
    assert!(status.success());
    let profile: Profile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(profile.roots.len(), 400);
    assert_eq!(
        profile
            .chains
            .iter()
            .filter(|chain| chain.path.len() == 1)
            .map(|chain| chain.count)
            .sum::<u64>(),
        400
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn explicit_flush_requires_initialization_in_fresh_child() {
    if std::env::var_os("SPANSCOPE_TEST_NO_INIT").is_none() {
        return;
    }
    assert!(flush().is_err());
}

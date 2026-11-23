use spanscope::profile::Profile;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Format {
    Chrome,
    Speedscope,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExportSpec {
    pub(crate) format: Format,
    pub(crate) path: PathBuf,
}

impl ExportSpec {
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        let (format, path) = value
            .split_once(':')
            .ok_or("--export expects FORMAT:PATH, for example chrome:trace.json")?;
        if path.is_empty() {
            return Err("--export path cannot be empty".into());
        }
        let format = match format {
            "chrome" | "perfetto" => Format::Chrome,
            "speedscope" => Format::Speedscope,
            _ => return Err("--export format must be chrome, perfetto, or speedscope".into()),
        };
        Ok(Self {
            format,
            path: PathBuf::from(path),
        })
    }
}

fn ensure_parent(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn chrome(profile: &Profile) -> serde_json::Value {
    let chains = profile
        .chains
        .iter()
        .map(|chain| (chain.id, chain))
        .collect::<std::collections::HashMap<_, _>>();
    let spans = profile
        .spans
        .iter()
        .map(|span| (span.id, span))
        .collect::<std::collections::HashMap<_, _>>();
    let mut events = vec![serde_json::json!({
        "name": "process_name", "ph": "M", "pid": 1, "tid": 0,
        "args": { "name": profile.meta.program }
    })];
    for root in &profile.roots {
        for invocation in &root.execution.invocations {
            let chain = chains.get(&invocation.chain);
            let span = chain
                .and_then(|chain| chain.path.last())
                .and_then(|id| spans.get(id));
            events.push(serde_json::json!({
                "name": span.map_or("unknown", |span| span.name.as_str()),
                "cat": "spanscope", "ph": "X",
                "ts": invocation.t_start_ns as f64 / 1_000.0,
                "dur": invocation.t_end_ns.saturating_sub(invocation.t_start_ns) as f64 / 1_000.0,
                "pid": 1, "tid": invocation.thread,
                "args": {
                    "root": root.uid, "chain": invocation.chain,
                    "completion": format!("{:?}", root.completion).to_lowercase(),
                    "outcome": root.outcome, "attributes": root.attributes,
                }
            }));
        }
    }
    serde_json::json!({ "traceEvents": events, "displayTimeUnit": "ns" })
}

fn speedscope(profile: &Profile) -> serde_json::Value {
    let frame_index = profile
        .spans
        .iter()
        .enumerate()
        .map(|(index, span)| (span.id, index))
        .collect::<std::collections::HashMap<_, _>>();
    let frames = profile
        .spans
        .iter()
        .map(|span| {
            serde_json::json!({
                "name": span.name, "file": span.file, "line": span.line
            })
        })
        .collect::<Vec<_>>();
    let samples = profile
        .chains
        .iter()
        .filter(|chain| chain.self_ns > 0)
        .map(|chain| {
            chain
                .path
                .iter()
                .filter_map(|id| frame_index.get(id).copied())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let weights = profile
        .chains
        .iter()
        .filter(|chain| chain.self_ns > 0)
        .map(|chain| chain.self_ns)
        .collect::<Vec<_>>();
    let total = weights
        .iter()
        .fold(0u64, |sum, value| sum.saturating_add(*value));
    serde_json::json!({
        "$schema": "https://www.speedscope.app/file-format-schema.json",
        "shared": { "frames": frames },
        "profiles": [{ "type": "sampled", "name": profile.meta.program,
            "unit": "nanoseconds", "startValue": 0, "endValue": total,
            "samples": samples, "weights": weights }],
        "activeProfileIndex": 0, "exporter": "cargo-spanscope"
    })
}

pub(crate) fn write(
    profile: &Profile,
    spec: &ExportSpec,
) -> Result<(), Box<dyn std::error::Error>> {
    ensure_parent(&spec.path)?;
    let value = match spec.format {
        Format::Chrome => chrome(profile),
        Format::Speedscope => speedscope(profile),
    };
    let mut bytes = serde_json::to_vec(&value)?;
    bytes.push(b'\n');
    fs::write(&spec.path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_export_specs() {
        assert_eq!(
            ExportSpec::parse("chrome:target/trace.json").unwrap(),
            ExportSpec {
                format: Format::Chrome,
                path: PathBuf::from("target/trace.json")
            }
        );
        assert_eq!(
            ExportSpec::parse("perfetto:trace.json").unwrap().format,
            Format::Chrome
        );
        assert!(ExportSpec::parse("unknown:trace.json").is_err());
    }
}

//! Cargo subcommand that turns a spanscope profile into a single-file offline report.

mod compare;
mod interop;

use compare::{GateRule, Violation};
use flate2::read::GzDecoder;
use spanscope::profile::Profile;
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const TEMPLATE: &str = include_str!("../assets/viewer.html");
const DEFAULT_INLINE_LIMIT: usize = 50 * 1024 * 1024;

#[derive(Debug)]
struct Options {
    input: PathBuf,
    baseline: Option<PathBuf>,
    output: PathBuf,
    open: bool,
    inline_limit: usize,
    fail_if: Vec<GateRule>,
    summary: Option<PathBuf>,
    exports: Vec<interop::ExportSpec>,
}

struct ReportOutcome {
    path: PathBuf,
    violations: Vec<Violation>,
}

fn usage() -> &'static str {
    "cargo spanscope <candidate.json|candidate.json.gz> [--baseline PROFILE] [--fail-if METRIC:+PERCENT%] [--summary FILE] [--export FORMAT:FILE] [--output DIR] [--open] [--inline-limit-mib N]\n\nCreates DIR/index.html. --baseline embeds an earlier profile for comparison. Repeat --fail-if with self, p99, calls, or alloc to fail a regression gate. Repeat --export with chrome, perfetto, or speedscope for interoperable traces."
}

fn parse_args() -> Result<Option<Options>, String> {
    let mut args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args
        .first()
        .is_some_and(|arg| arg == OsStr::new("spanscope"))
    {
        args.remove(0);
    }
    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", usage());
        return Ok(None);
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("cargo-spanscope {}", env!("CARGO_PKG_VERSION"));
        return Ok(None);
    }
    let mut input = None;
    let mut baseline = None;
    let mut output = None;
    let mut open = std::env::var("SPANSCOPE_OPEN").is_ok_and(|value| value == "1");
    let mut inline_limit = DEFAULT_INLINE_LIMIT;
    let mut fail_if = Vec::new();
    let mut summary = None;
    let mut exports = Vec::new();
    let mut position = 0;
    while position < args.len() {
        let argument = &args[position];
        if argument == "--output" || argument == "-o" {
            position += 1;
            output = Some(PathBuf::from(
                args.get(position).ok_or("--output needs a directory")?,
            ));
        } else if argument == "--baseline" {
            position += 1;
            baseline = Some(PathBuf::from(
                args.get(position)
                    .ok_or("--baseline needs a profile path")?,
            ));
        } else if argument == "--fail-if" {
            position += 1;
            let value = args
                .get(position)
                .ok_or("--fail-if needs METRIC:+PERCENT%")?
                .to_string_lossy();
            fail_if.push(GateRule::parse(&value)?);
        } else if argument == "--summary" {
            position += 1;
            summary = Some(PathBuf::from(
                args.get(position).ok_or("--summary needs a file path")?,
            ));
        } else if argument == "--export" {
            position += 1;
            let value = args
                .get(position)
                .ok_or("--export needs FORMAT:PATH")?
                .to_string_lossy();
            exports.push(interop::ExportSpec::parse(&value)?);
        } else if argument == "--inline-limit-mib" {
            position += 1;
            let amount = args
                .get(position)
                .ok_or("--inline-limit-mib needs a number")?;
            let number = amount
                .to_string_lossy()
                .parse::<usize>()
                .map_err(|_| "invalid inline limit")?;
            inline_limit = number
                .checked_mul(1024 * 1024)
                .ok_or("inline limit is too large")?;
        } else if argument == "--open" {
            open = true;
        } else if argument == "--no-open" {
            open = false;
        } else if argument.to_string_lossy().starts_with('-') {
            return Err(format!("unknown option: {}", argument.to_string_lossy()));
        } else if input.replace(PathBuf::from(argument)).is_some() {
            return Err("provide exactly one profile path".into());
        }
        position += 1;
    }
    let input: PathBuf = input.ok_or("provide a profile path")?;
    if baseline.is_none() && (!fail_if.is_empty() || summary.is_some()) {
        return Err("--fail-if and --summary require --baseline".into());
    }
    let output = output.unwrap_or_else(|| {
        input
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("spanscope-report")
    });
    Ok(Some(Options {
        input,
        baseline,
        output,
        open,
        inline_limit,
        fail_if,
        summary,
        exports,
    }))
}

fn profile_bytes(input: &Path) -> Result<(Vec<u8>, bool), Box<dyn std::error::Error>> {
    let bytes = fs::read(input)?;
    let compressed = bytes.starts_with(&[0x1f, 0x8b]);
    if compressed {
        let mut decoded = Vec::new();
        GzDecoder::new(bytes.as_slice()).read_to_end(&mut decoded)?;
        Ok((decoded, true))
    } else {
        Ok((bytes, false))
    }
}

fn escape_embedded_json(bytes: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let source = std::str::from_utf8(bytes)?;
    let mut escaped = String::with_capacity(source.len());
    for character in source.chars() {
        match character {
            '<' => escaped.push_str("\\u003c"),
            '>' => escaped.push_str("\\u003e"),
            '&' => escaped.push_str("\\u0026"),
            '\u{2028}' => escaped.push_str("\\u2028"),
            '\u{2029}' => escaped.push_str("\\u2029"),
            _ => escaped.push(character),
        }
    }
    Ok(escaped)
}

fn render(options: &Options) -> Result<ReportOutcome, Box<dyn std::error::Error>> {
    let (decoded, compressed) = profile_bytes(&options.input)?;
    let profile: Profile = serde_json::from_slice(&decoded)?;
    if profile.schema_version != 1 {
        return Err(format!(
            "unsupported schema_version {}; expected 1",
            profile.schema_version
        )
        .into());
    }
    let baseline_bytes = options
        .baseline
        .as_ref()
        .map(|path| profile_bytes(path))
        .transpose()?;
    let baseline_profile = if let Some((bytes, _)) = &baseline_bytes {
        let baseline_profile: Profile = serde_json::from_slice(bytes)?;
        if baseline_profile.schema_version != 1 {
            return Err(format!(
                "unsupported baseline schema_version {}; expected 1",
                baseline_profile.schema_version
            )
            .into());
        }
        Some(baseline_profile)
    } else {
        None
    };
    fs::create_dir_all(&options.output)?;
    let (mut bootstrap, mut extra) = if decoded.len() <= options.inline_limit {
        let escaped = escape_embedded_json(&decoded)?;
        (
            serde_json::json!({"mode":"inline"}),
            format!(
                "<script id=\"spanscope-profile\" type=\"application/json\">{escaped}</script>"
            ),
        )
    } else {
        let name = if compressed {
            "profile.json.gz"
        } else {
            "profile.json"
        };
        let destination = options.output.join(name);
        if fs::canonicalize(&options.input).ok() != fs::canonicalize(&destination).ok() {
            fs::copy(&options.input, &destination)?;
        }
        (
            serde_json::json!({"mode":"picker","suggested":name}),
            String::new(),
        )
    };
    if let Some((baseline_bytes, baseline_compressed)) = baseline_bytes {
        if baseline_bytes.len() <= options.inline_limit {
            let escaped = escape_embedded_json(&baseline_bytes)?;
            bootstrap["baseline_mode"] = "inline".into();
            extra.push_str(&format!(
                "\n<script id=\"spanscope-baseline\" type=\"application/json\">{escaped}</script>"
            ));
        } else {
            let name = if baseline_compressed {
                "baseline.json.gz"
            } else {
                "baseline.json"
            };
            let destination = options.output.join(name);
            let source = options.baseline.as_ref().expect("baseline path is present");
            if fs::canonicalize(source).ok() != fs::canonicalize(&destination).ok() {
                fs::copy(source, &destination)?;
            }
            bootstrap["baseline_mode"] = "picker".into();
            bootstrap["baseline_suggested"] = name.into();
        }
    }
    if !TEMPLATE.contains("__SPANSCOPE_BOOTSTRAP__") || !TEMPLATE.contains("</body>") {
        return Err("packaged viewer template is invalid".into());
    }
    let safe_bootstrap = escape_embedded_json(bootstrap.to_string().as_bytes())?;
    let html = TEMPLATE
        .replace("__SPANSCOPE_BOOTSTRAP__", &safe_bootstrap)
        .replace("</body>", &format!("{extra}\n</body>"));
    let path = options.output.join("index.html");
    fs::write(&path, html)?;
    let violations = baseline_profile
        .as_ref()
        .map(|baseline| compare::evaluate(baseline, &profile, &options.fail_if))
        .unwrap_or_default();
    if let (Some(summary), Some(baseline)) = (&options.summary, &options.baseline) {
        let contents = compare::markdown(
            &baseline.to_string_lossy(),
            &options.input.to_string_lossy(),
            &options.fail_if,
            &violations,
        );
        fs::write(summary, contents)?;
    }
    for spec in &options.exports {
        interop::write(&profile, spec)?;
    }
    if options.open {
        open_browser(&path)?;
    }
    Ok(ReportOutcome { path, violations })
}

fn open_browser(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let path = fs::canonicalize(path)?;
    #[cfg(target_os = "linux")]
    let status = Command::new("xdg-open").arg(&path).status()?;
    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(&path).status()?;
    #[cfg(target_os = "windows")]
    let status = Command::new("explorer").arg(&path).status()?;
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    return Err("automatic opening is unsupported on this platform".into());
    if !status.success() {
        return Err("could not open the report in a browser".into());
    }
    Ok(())
}

fn main() -> ExitCode {
    match parse_args().and_then(|options| match options {
        Some(options) => render(&options)
            .map(Some)
            .map_err(|error| error.to_string()),
        None => Ok(None),
    }) {
        Ok(Some(outcome)) => {
            println!("{}", outcome.path.display());
            if outcome.violations.is_empty() {
                ExitCode::SUCCESS
            } else {
                eprintln!(
                    "cargo-spanscope: regression gate failed with {} violation(s)",
                    outcome.violations.len()
                );
                for violation in outcome.violations.iter().take(10) {
                    eprintln!(
                        "  {} {}: {} -> {} ({:+.2}%, limit +{:.2}%)",
                        violation.metric.name(),
                        violation.routine,
                        violation.baseline,
                        violation.candidate,
                        violation.delta_percent(),
                        violation.threshold_percent,
                    );
                }
                ExitCode::FAILURE
            }
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cargo-spanscope: {error}\n\n{}", usage());
            ExitCode::FAILURE
        }
    }
}

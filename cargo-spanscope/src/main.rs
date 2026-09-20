//! Cargo subcommand that turns a spanscope profile into a single-file offline report.

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
    output: PathBuf,
    open: bool,
    inline_limit: usize,
}

fn usage() -> &'static str {
    "cargo spanscope <profile.json|profile.json.gz> [--output DIR] [--open] [--inline-limit-mib N]\n\nCreates DIR/index.html. Profiles up to 50 MiB uncompressed are safely embedded. Larger profiles produce a sidecar and a file-picker report. --open (or SPANSCOPE_OPEN=1) launches the report in the default browser."
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
    let mut output = None;
    let mut open = std::env::var("SPANSCOPE_OPEN").is_ok_and(|value| value == "1");
    let mut inline_limit = DEFAULT_INLINE_LIMIT;
    let mut position = 0;
    while position < args.len() {
        let argument = &args[position];
        if argument == "--output" || argument == "-o" {
            position += 1;
            output = Some(PathBuf::from(
                args.get(position).ok_or("--output needs a directory")?,
            ));
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
    let output = output.unwrap_or_else(|| {
        input
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("spanscope-report")
    });
    Ok(Some(Options {
        input,
        output,
        open,
        inline_limit,
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

fn render(options: &Options) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let (decoded, compressed) = profile_bytes(&options.input)?;
    let profile: Profile = serde_json::from_slice(&decoded)?;
    if profile.schema_version != 1 {
        return Err(format!(
            "unsupported schema_version {}; expected 1",
            profile.schema_version
        )
        .into());
    }
    fs::create_dir_all(&options.output)?;
    let (bootstrap, extra) = if decoded.len() <= options.inline_limit {
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
    if !TEMPLATE.contains("__SPANSCOPE_BOOTSTRAP__") || !TEMPLATE.contains("</body>") {
        return Err("packaged viewer template is invalid".into());
    }
    let safe_bootstrap = escape_embedded_json(bootstrap.to_string().as_bytes())?;
    let html = TEMPLATE
        .replace("__SPANSCOPE_BOOTSTRAP__", &safe_bootstrap)
        .replace("</body>", &format!("{extra}\n</body>"));
    let path = options.output.join("index.html");
    fs::write(&path, html)?;
    if options.open {
        open_browser(&path)?;
    }
    Ok(path)
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
        Ok(Some(path)) => {
            println!("{}", path.display());
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cargo-spanscope: {error}\n\n{}", usage());
            ExitCode::FAILURE
        }
    }
}

//! Capture and output configuration. Call [`ConfigBuilder::init`] before the first span.

use std::fmt;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Output encoding for an explicit or automatic flush.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputFormat {
    /// Streaming UTF-8 JSON.
    Json,
    /// Gzip-compressed streaming JSON.
    Gzip,
    /// Human-readable aggregate report.
    Text,
}

impl OutputFormat {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value {
            "json" => Ok(Self::Json),
            "gzip" | "json.gz" => Ok(Self::Gzip),
            "text" | "txt" => Ok(Self::Text),
            _ => Err(ConfigError(format!("invalid output format: {value}"))),
        }
    }
}

/// Resolved collector and exporter configuration.
#[derive(Clone, Debug)]
pub struct Config {
    /// Destination used by [`crate::export::flush`].
    pub output: PathBuf,
    /// Output encoding.
    pub format: OutputFormat,
    /// Independent probability of retaining a root.
    pub sample_rate: f64,
    /// Number of retained completed roots.
    pub max_roots: usize,
    /// Register a best-effort normal-exit flush hook.
    pub auto_flush: bool,
    /// Atomically replace a previous output file.
    pub overwrite: bool,
    /// Program label in capture metadata.
    pub program: String,
    /// Optional host application version.
    pub version: Option<String>,
    /// Optional host application revision.
    pub git_sha: Option<String>,
}

/// Invalid setting or initialization after collection has started.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for ConfigError {}

/// Optional settings; specified values override environment variables.
#[derive(Default)]
pub struct ConfigBuilder {
    output: Option<PathBuf>,
    format: Option<OutputFormat>,
    sample_rate: Option<f64>,
    max_roots: Option<usize>,
    auto_flush: Option<bool>,
    overwrite: Option<bool>,
    program: Option<String>,
    version: Option<String>,
    git_sha: Option<String>,
}

static CONFIG: OnceLock<Config> = OnceLock::new();
static INIT_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn initialization_lock() -> MutexGuard<'static, ()> {
    INIT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn configured() -> Option<&'static Config> {
    CONFIG.get()
}

fn env_value(key: &str) -> Result<Option<String>, ConfigError> {
    match std::env::var(key) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(ConfigError(format!("{key} is not valid UTF-8"))),
    }
}

fn parse_bool(value: &str, key: &str) -> Result<bool, ConfigError> {
    match value {
        "1" | "true" | "yes" => Ok(true),
        "0" | "false" | "no" => Ok(false),
        _ => Err(ConfigError(format!("invalid boolean for {key}: {value}"))),
    }
}

impl ConfigBuilder {
    /// Creates a builder with no explicit overrides.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the destination path.
    pub fn output(mut self, output: impl Into<PathBuf>) -> Self {
        self.output = Some(output.into());
        self
    }

    /// Selects the output encoding.
    pub fn format(mut self, format: OutputFormat) -> Self {
        self.format = Some(format);
        self
    }

    /// Sets the probability of retaining each root.
    pub fn sample_rate(mut self, rate: f64) -> Self {
        self.sample_rate = Some(rate);
        self
    }

    /// Sets the bounded root history size.
    pub fn max_roots(mut self, limit: usize) -> Self {
        self.max_roots = Some(limit);
        self
    }

    /// Enables or disables normal-exit best-effort flushing.
    pub fn auto_flush(mut self, enabled: bool) -> Self {
        self.auto_flush = Some(enabled);
        self
    }

    /// Allows or refuses replacement of an existing destination.
    pub fn overwrite(mut self, enabled: bool) -> Self {
        self.overwrite = Some(enabled);
        self
    }

    /// Sets the program name in profile metadata.
    pub fn program(mut self, name: impl Into<String>) -> Self {
        self.program = Some(name.into());
        self
    }

    /// Sets the host application version.
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Sets the host application Git revision.
    pub fn git_sha(mut self, revision: impl Into<String>) -> Self {
        self.git_sha = Some(revision.into());
        self
    }

    /// Resolves builder overrides, environment settings, then defaults.
    pub fn resolve(self) -> Result<Config, ConfigError> {
        let format = match self.format {
            Some(value) => value,
            None => env_value("SPANSCOPE_FORMAT")?
                .map(|value| OutputFormat::parse(&value))
                .transpose()?
                .unwrap_or(OutputFormat::Json),
        };
        let sample_rate = match self.sample_rate {
            Some(value) => value,
            None => env_value("SPANSCOPE_SAMPLE_RATE")?
                .map(|value| {
                    value
                        .parse()
                        .map_err(|_| ConfigError("invalid SPANSCOPE_SAMPLE_RATE".into()))
                })
                .transpose()?
                .unwrap_or(1.0),
        };
        if !sample_rate.is_finite() || !(0.0..=1.0).contains(&sample_rate) {
            return Err(ConfigError(
                "sample_rate must be finite and in [0, 1]".into(),
            ));
        }
        let max_roots = match self.max_roots {
            Some(value) => value,
            None => env_value("SPANSCOPE_MAX_ROOTS")?
                .map(|value| {
                    value
                        .parse()
                        .map_err(|_| ConfigError("invalid SPANSCOPE_MAX_ROOTS".into()))
                })
                .transpose()?
                .unwrap_or(10_000),
        };
        let boolean = |override_value: Option<bool>, key, default| -> Result<bool, ConfigError> {
            match override_value {
                Some(value) => Ok(value),
                None => env_value(key)?
                    .map(|value| parse_bool(&value, key))
                    .transpose()
                    .map(|value| value.unwrap_or(default)),
            }
        };
        let output = match self.output {
            Some(value) => value,
            None => env_value("SPANSCOPE_OUTPUT")?
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(match format {
                        OutputFormat::Json => "spanscope.json",
                        OutputFormat::Gzip => "spanscope.json.gz",
                        OutputFormat::Text => "spanscope.txt",
                    })
                }),
        };
        if output.as_os_str().is_empty() {
            return Err(ConfigError("output path cannot be empty".into()));
        }
        Ok(Config {
            output,
            format,
            sample_rate,
            max_roots,
            auto_flush: boolean(self.auto_flush, "SPANSCOPE_AUTO_FLUSH", true)?,
            overwrite: boolean(self.overwrite, "SPANSCOPE_OVERWRITE", true)?,
            program: match self.program {
                Some(value) => value,
                None => env_value("SPANSCOPE_PROGRAM")?.unwrap_or_else(|| {
                    std::env::args_os()
                        .next()
                        .map(|value| value.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "unknown".into())
                }),
            },
            version: match self.version {
                Some(value) => Some(value),
                None => env_value("SPANSCOPE_VERSION")?,
            },
            git_sha: match self.git_sha {
                Some(value) => Some(value),
                None => env_value("SPANSCOPE_GIT_SHA")?,
            },
        })
    }

    /// Installs configuration before the first traced span or snapshot.
    pub fn init(self) -> Result<(), ConfigError> {
        let config = self.resolve()?;
        let _guard = initialization_lock();
        if CONFIG.get().is_some() || crate::runtime::is_initialized() {
            return Err(ConfigError("spanscope was already initialized".into()));
        }
        if config.auto_flush {
            crate::export::register_exit_hook()?;
        }
        CONFIG
            .set(config)
            .map_err(|_| ConfigError("spanscope was already initialized".into()))
    }
}

/// Parses `--spanscope-*` flags and returns all unrecognized arguments unchanged.
///
/// Supported flags are `output`, `format`, `sample-rate`, `max-roots`,
/// `auto-flush`, `overwrite`, `program`, `version`, and `git-sha`. Values may
/// follow `=` or the flag in the next argument. `--` ends profiler parsing.
pub fn init_from_args<I, S>(args: I) -> Result<Vec<String>, ConfigError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut remaining = Vec::new();
    let mut builder = ConfigBuilder::new();
    let mut args = args.into_iter().map(Into::into).peekable();
    let mut stopped = false;
    while let Some(arg) = args.next() {
        if stopped || arg == "--" {
            stopped |= arg == "--";
            remaining.push(arg);
            continue;
        }
        let Some(flag) = arg.strip_prefix("--spanscope-") else {
            remaining.push(arg);
            continue;
        };
        let (key, inline) = flag
            .split_once('=')
            .map_or((flag, None), |(key, value)| (key, Some(value)));
        let value = match inline {
            Some(value) => value.to_owned(),
            None => args
                .next()
                .ok_or_else(|| ConfigError(format!("missing value for --spanscope-{key}")))?,
        };
        builder = match key {
            "output" => builder.output(value),
            "format" => builder.format(OutputFormat::parse(&value)?),
            "sample-rate" => builder.sample_rate(
                value
                    .parse()
                    .map_err(|_| ConfigError("invalid sample-rate".into()))?,
            ),
            "max-roots" => builder.max_roots(
                value
                    .parse()
                    .map_err(|_| ConfigError("invalid max-roots".into()))?,
            ),
            "auto-flush" => builder.auto_flush(parse_bool(&value, key)?),
            "overwrite" => builder.overwrite(parse_bool(&value, key)?),
            "program" => builder.program(value),
            "version" => builder.version(value),
            "git-sha" => builder.git_sha(value),
            _ => {
                return Err(ConfigError(format!(
                    "unknown profiler flag: --spanscope-{key}"
                )))
            }
        };
    }
    builder.init()?;
    Ok(remaining)
}

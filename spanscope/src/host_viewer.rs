//! Optional host-side HTML report built from the same checked-in viewer asset.

use crate::config::OutputFormat;
use crate::export::ExportError;
use flate2::read::GzDecoder;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

const TEMPLATE: &str = include_str!("../assets/viewer.html");
const INLINE_LIMIT: usize = 50 * 1024 * 1024;

pub(crate) fn emit_viewer(profile: &Path, format: OutputFormat) -> Result<PathBuf, ExportError> {
    if format == OutputFormat::Text {
        return Err(ExportError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "viewer emission needs JSON or gzip output",
        )));
    }
    let bytes = fs::read(profile)?;
    let decoded = if format == OutputFormat::Gzip {
        let mut data = Vec::new();
        GzDecoder::new(bytes.as_slice()).read_to_end(&mut data)?;
        data
    } else {
        bytes
    };
    let (bootstrap, extra) = if decoded.len() <= INLINE_LIMIT {
        let source = std::str::from_utf8(&decoded).map_err(|error| {
            ExportError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
        })?;
        let escaped = source
            .replace('&', "\\u0026")
            .replace('<', "\\u003c")
            .replace('>', "\\u003e")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029");
        (
            "{\"mode\":\"inline\"}".to_owned(),
            format!(
                "<script id=\"spanscope-profile\" type=\"application/json\">{escaped}</script>"
            ),
        )
    } else {
        let suggested = profile.file_name().unwrap_or_default().to_string_lossy();
        (
            serde_json::json!({"mode":"picker", "suggested": suggested}).to_string(),
            String::new(),
        )
    };
    let safe_bootstrap = bootstrap
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e");
    let html = TEMPLATE
        .replace("__SPANSCOPE_BOOTSTRAP__", &safe_bootstrap)
        .replace("</body>", &format!("{extra}\n</body>"));
    let path = profile.with_extension("html");
    fs::write(&path, html)?;
    Ok(path)
}

pub(crate) fn open_browser(path: &Path) {
    let status = {
        #[cfg(target_os = "linux")]
        {
            Command::new("xdg-open").arg(path).status()
        }
        #[cfg(target_os = "macos")]
        {
            Command::new("open").arg(path).status()
        }
        #[cfg(target_os = "windows")]
        {
            Command::new("explorer").arg(path).status()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "automatic browser opening is unsupported",
            ))
        }
    };
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!(
            "spanscope: browser opener exited with {status}; report at {}",
            path.display()
        ),
        Err(error) => eprintln!(
            "spanscope: could not open report: {error}; report at {}",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emitted_html_escapes_profile_script_terminators() {
        let input =
            std::env::temp_dir().join(format!("spanscope-viewer-{}.json", std::process::id()));
        fs::write(
            &input,
            br#"{"name":"</script><script>window.pwned=true</script>"}"#,
        )
        .unwrap();
        let output = emit_viewer(&input, OutputFormat::Json).unwrap();
        let html = fs::read_to_string(&output).unwrap();
        assert!(html.contains("\\u003c/script\\u003e\\u003cscript\\u003ewindow.pwned"));
        assert!(!html.contains("</script><script>window.pwned"));
        fs::remove_file(input).unwrap();
        fs::remove_file(output).unwrap();
    }
}

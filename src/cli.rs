//! Command-line parsing. Two flags, hand-rolled: folio needs `--root` and
//! `--port` and nothing else, and pulling in an argument-parser crate for that
//! is the single most expensive dependency the project can carry.

use std::path::PathBuf;

/// The flags folio actually runs with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// Initial directory to browse. Only sets where the browser starts — the
    /// server itself is not confined to it.
    pub root: PathBuf,
    /// Port to bind on `127.0.0.1`.
    pub port: u16,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            port: 4000,
        }
    }
}

/// What the caller should do after parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Run the server with these arguments.
    Run(Args),
    /// Print [`USAGE`] and exit 0.
    Help,
    /// Print [`VERSION`] and exit 0.
    Version,
}

/// Usage text printed for `--help` and alongside a bad flag.
pub const USAGE: &str = "\
folio — local web file explorer

Usage: folio [OPTIONS]

Options:
  -r, --root <DIR>    Initial directory to browse (default: current directory)
  -p, --port <PORT>   Port to bind on 127.0.0.1 (default: 4000)
  -h, --help          Print this help and exit
  -V, --version       Print version and exit";

/// Package version, printed for `--version`.
pub const VERSION: &str = concat!("folio ", env!("CARGO_PKG_VERSION"));

/// Parses arguments (without the program name). Accepts both `--flag value` and
/// `--flag=value`. `--help`/`--version` win over everything else, including a
/// bad flag elsewhere on the line, so `folio --nonsense --help` still prints
/// help rather than an error.
pub fn parse_from<I, T>(args: I) -> Result<Outcome, String>
where
    I: IntoIterator<Item = T>,
    T: Into<String>,
{
    let args: Vec<String> = args.into_iter().map(Into::into).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Outcome::Help);
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        return Ok(Outcome::Version);
    }

    let mut out = Args::default();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        // `--flag=value` splits here; `--flag value` consumes the iterator.
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) => (f.to_string(), Some(v.to_string())),
            None => (arg, None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            match it.next() {
                // A following token that looks like a flag is not a value:
                // `--root --port 80` must not set the root to "--port".
                Some(v) if v.starts_with('-') && v.len() > 1 => {
                    Err(format!("{name} needs a value"))
                }
                Some(v) => Ok(v),
                None => Err(format!("{name} needs a value")),
            }
        };
        match flag.as_str() {
            "--root" | "-r" => out.root = PathBuf::from(value("--root")?),
            "--port" | "-p" => out.port = parse_port(&value("--port")?)?,
            other => return Err(format!("unknown flag: {other}")),
        }
    }
    Ok(Outcome::Run(out))
}

fn parse_port(raw: &str) -> Result<u16, String> {
    raw.parse::<u16>()
        .map_err(|_| format!("invalid port: {raw} (expected a number 0-65535)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Args {
        match parse_from(args.iter().copied()) {
            Ok(Outcome::Run(a)) => a,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn err(args: &[&str]) -> String {
        match parse_from(args.iter().copied()) {
            Err(e) => e,
            other => panic!("expected Err, got {other:?}"),
        }
    }

    #[test]
    fn no_arguments_yields_defaults() {
        assert_eq!(run(&[]), Args::default());
        assert_eq!(run(&[]).root, PathBuf::from("."));
        assert_eq!(run(&[]).port, 4000);
    }

    #[test]
    fn long_flags_accept_both_forms() {
        let spaced = run(&["--root", "/tmp", "--port", "9000"]);
        let joined = run(&["--root=/tmp", "--port=9000"]);
        assert_eq!(spaced, joined);
        assert_eq!(spaced.root, PathBuf::from("/tmp"));
        assert_eq!(spaced.port, 9000);
    }

    #[test]
    fn short_flags_accept_both_forms() {
        let spaced = run(&["-r", "/var", "-p", "80"]);
        let joined = run(&["-r=/var", "-p=80"]);
        assert_eq!(spaced, joined);
        assert_eq!(spaced.root, PathBuf::from("/var"));
        assert_eq!(spaced.port, 80);
    }

    #[test]
    fn flags_may_be_given_in_any_order_and_alone() {
        assert_eq!(run(&["--port", "1"]).root, PathBuf::from("."));
        assert_eq!(run(&["--root", "/a"]).port, 4000);
        assert_eq!(run(&["--port", "2", "--root", "/b"]).port, 2);
    }

    #[test]
    fn help_and_version_are_recognized_in_both_forms() {
        assert_eq!(parse_from(["--help"]), Ok(Outcome::Help));
        assert_eq!(parse_from(["-h"]), Ok(Outcome::Help));
        assert_eq!(parse_from(["--version"]), Ok(Outcome::Version));
        assert_eq!(parse_from(["-V"]), Ok(Outcome::Version));
    }

    #[test]
    fn help_and_version_win_over_everything_else() {
        // Including a malformed port and an unknown flag, and each other.
        assert_eq!(parse_from(["--port", "nope", "--help"]), Ok(Outcome::Help));
        assert_eq!(parse_from(["--bogus", "--version"]), Ok(Outcome::Version));
        assert_eq!(parse_from(["--version", "--help"]), Ok(Outcome::Help));
    }

    #[test]
    fn unknown_flags_are_rejected() {
        assert!(err(&["--bogus"]).contains("unknown flag"));
        assert!(err(&["-x"]).contains("unknown flag"));
        assert!(err(&["--root", "/tmp", "--bogus"]).contains("unknown flag"));
    }

    #[test]
    fn missing_values_are_rejected() {
        assert!(err(&["--root"]).contains("needs a value"));
        assert!(err(&["--port"]).contains("needs a value"));
        assert!(err(&["--root", "--port"]).contains("needs a value"));
    }

    #[test]
    fn invalid_ports_are_rejected() {
        assert!(err(&["--port", "nope"]).contains("invalid port"));
        assert!(err(&["--port", ""]).contains("invalid port"));
        assert!(err(&["--port", "65536"]).contains("invalid port"));
        assert!(err(&["--port=1.5"]).contains("invalid port"));
        // A dash-prefixed value is flag-shaped, so it is read as a missing
        // value rather than a bad port.
        assert!(err(&["--port", "-1"]).contains("needs a value"));
        // 65535 is the largest valid port.
        assert_eq!(run(&["--port", "65535"]).port, 65535);
    }

    #[test]
    fn usage_text_documents_every_flag() {
        for flag in [
            "--root",
            "--port",
            "--help",
            "--version",
            "-r",
            "-p",
            "-h",
            "-V",
        ] {
            assert!(USAGE.contains(flag), "usage must mention {flag}");
        }
        assert!(VERSION.starts_with("folio "));
    }
}

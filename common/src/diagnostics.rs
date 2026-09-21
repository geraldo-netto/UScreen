//! Dependency evidence and backend implementation are independent of readiness.
use crate::platform::Capabilities;
use std::path::{Path, PathBuf};

mod native;
pub use native::collect;

pub trait Probe {
    fn find(&mut self, name: &str) -> Option<PathBuf>;
    fn run(&mut self, path: &Path, argument: &str) -> Result<Vec<u8>, String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Adb,
    Ffmpeg,
}
impl Tool {
    fn command(self) -> (&'static str, &'static str) {
        match self {
            Self::Adb => ("adb", "version"),
            Self::Ffmpeg => ("ffmpeg", "-version"),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Adb => "ADB",
            Self::Ffmpeg => "FFmpeg",
        }
    }
    fn version(self, bytes: &[u8]) -> Option<String> {
        if bytes.len() > 32768 {
            return None;
        }
        let text = std::str::from_utf8(bytes).ok()?;
        let first = text.lines().next()?;
        match self {
            Self::Adb => adb_version(first, text),
            Self::Ffmpeg => {
                version_token(first.strip_prefix("ffmpeg version ")?).map(str::to_owned)
            }
        }
    }
}

fn version_token(text: &str) -> Option<&str> {
    let token = text.split_whitespace().next()?;
    let valid = token.len() <= 128
        && token.bytes().any(|byte| byte.is_ascii_digit())
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-_+:".contains(&byte));
    valid.then_some(token)
}

fn adb_version(first: &str, text: &str) -> Option<String> {
    let protocol = version_token(first.strip_prefix("Android Debug Bridge version ")?)?;
    match text.lines().find_map(|line| line.strip_prefix("Version ")) {
        Some(package) => Some(format!("{} (protocol {protocol})", version_token(package)?)),
        None => Some(protocol.into()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Missing,
    Available(String),
    Unverified(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependency {
    pub tool: Tool,
    pub path: Option<PathBuf>,
    pub state: State,
}
impl Dependency {
    pub fn verified(&self) -> bool {
        matches!(self.state, State::Available(_))
    }
    fn line(&self) -> String {
        let status = match &self.state {
            State::Missing => "missing".into(),
            State::Available(version) => format!("available (version {version})"),
            State::Unverified(reason) => format!("unverified ({reason})"),
        };
        let path = self
            .path
            .as_ref()
            .map(|path| format!("; {}", path.display()))
            .unwrap_or_default();
        format!("{}: {status}{path}", self.tool.label())
    }
}

fn inspect(probe: &mut impl Probe, tool: Tool) -> Dependency {
    let (name, argument) = tool.command();
    let path = probe.find(name);
    let state = match &path {
        None => State::Missing,
        Some(path) => match probe.run(path, argument) {
            Err(error) => State::Unverified(error),
            Ok(bytes) => tool
                .version(&bytes)
                .map(State::Available)
                .unwrap_or_else(|| State::Unverified("unrecognized version output".into())),
        },
    };
    Dependency { tool, path, state }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub tools: [Dependency; 2],
    pub capabilities: Capabilities,
}
impl Report {
    pub fn lines(&self) -> Vec<String> {
        let mut lines: Vec<_> = self.tools.iter().map(Dependency::line).collect();
        lines.extend(backend_lines(self.capabilities));
        lines.push("Tablet connection: unverified (not checked)".into());
        lines.push("Encoder/decoder compatibility: unverified (not probed)".into());
        lines
    }
}

pub fn collect_with(probe: &mut impl Probe, capabilities: Capabilities) -> Report {
    Report {
        tools: [inspect(probe, Tool::Adb), inspect(probe, Tool::Ffmpeg)],
        capabilities,
    }
}

pub fn backend_lines(caps: Capabilities) -> Vec<String> {
    [
        ("Display", caps.display),
        ("Input", caps.input),
        ("Daemon lifecycle", caps.daemon),
        ("Cameras", caps.camera),
        ("Autostart", caps.autostart),
        ("System setup", caps.system_setup),
        ("Pipe capacity", caps.pipe_capacity),
        ("Conversion pool", caps.conversion_pool),
    ]
    .into_iter()
    .map(|(name, implemented)| {
        let status = if implemented {
            "unverified (backend implemented; runtime not checked)"
        } else {
            "unavailable (unsupported)"
        };
        format!("{name}: {status}")
    })
    .collect()
}

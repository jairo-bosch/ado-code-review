use std::{ffi::OsString, path::Path};

use copilot_sdk::{
    CliProgram, ClientMode, ClientOptions, LogLevel, RuntimeConnection, SpawnedProcessOptions,
};
use secrecy::{ExposeSecret, SecretString};

use super::CopilotError;

const CHILD_ENV: &[&str] = &[
    "PATH",
    "LANG",
    "TMPDIR",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "https_proxy",
    "http_proxy",
    "all_proxy",
    "no_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

pub(in crate::app::review) struct Credentials {
    token: SecretString,
    host: String,
}

impl Credentials {
    #[cfg(test)]
    pub(super) fn fixture() -> Self {
        Self {
            token: "synthetic-token".into(),
            host: validate_host("GitHub.Example").unwrap(),
        }
    }

    pub(in crate::app::review) fn from_env() -> Result<Self, CopilotError> {
        let token = std::env::var("COPILOT_GITHUB_TOKEN").map_err(|_| CopilotError::Token)?;
        if token.trim().is_empty() || token.contains("$(") || token.chars().any(char::is_control) {
            return Err(CopilotError::Token);
        }
        let host = match std::env::var("COPILOT_GH_HOST") {
            Ok(host) => validate_host(&host)?,
            Err(std::env::VarError::NotPresent) => "github.com".into(),
            Err(std::env::VarError::NotUnicode(_)) => return Err(CopilotError::Host),
        };
        Ok(Self {
            token: token.into(),
            host,
        })
    }

    pub(super) fn options(&self, program: &Path, home: &Path) -> ClientOptions {
        let process = SpawnedProcessOptions::new()
            .with_program(CliProgram::Path(program.to_owned()))
            .with_working_directory(home)
            .with_owned(|owned| owned.with_base_directory(home))
            .with_env(child_env(home, &self.host));
        ClientOptions::new(ClientMode::Empty)
            .with_runtime(RuntimeConnection::SpawnedStdio(process))
            .with_github(|github| {
                github
                    .with_token(self.token.expose_secret())
                    .with_use_logged_in_user(false)
            })
            .with_observability(|options| {
                options
                    .with_log_level(LogLevel::None)
                    .with_disable_client_telemetry(true)
            })
    }
}

fn validate_host(host: &str) -> Result<String, CopilotError> {
    if host.is_empty()
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
        || host.split('.').any(str::is_empty)
    {
        return Err(CopilotError::Host);
    }
    Ok(host.to_ascii_lowercase())
}

fn child_env(home: &Path, host: &str) -> Vec<(OsString, OsString)> {
    let mut env: Vec<_> = CHILD_ENV
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| ((*name).into(), value)))
        .collect();
    env.extend([
        ("HOME".into(), home.as_os_str().to_owned()),
        ("COPILOT_GH_HOST".into(), host.into()),
    ]);
    env
}

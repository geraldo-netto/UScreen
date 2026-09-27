//! Shared USB attachment policy. Native command/process behavior stays in adapters.
use anyhow::{ensure, Context, Result};
use blent_config::{
    adb::{transport_of, Transport},
    commands::AsyncCommandExt,
};
use std::{future::Future, path::PathBuf, pin::Pin, process::Output, time::Duration};

pub mod connection;
pub mod monitor;
mod preview;
mod routes;
pub use routes::Routes;

pub type CommandFuture<'a> = Pin<Box<dyn Future<Output = Result<Output>> + Send + 'a>>;
pub trait Commands: Send + Sync {
    fn execute(&self, arguments: Vec<String>, input: Option<Vec<u8>>) -> CommandFuture<'_>;
}

/// Uses the common native adapter: argument arrays and Windows job ownership,
/// never a host shell. Only Android shell commands are sent through stdin.
pub struct NativeCommands(pub PathBuf);
impl Commands for NativeCommands {
    fn execute(&self, arguments: Vec<String>, input: Option<Vec<u8>>) -> CommandFuture<'_> {
        Box::pin(async move {
            Ok(tokio::process::Command::new(&self.0)
                .args(arguments)
                .output_input_timeout(input.as_deref(), Duration::from_secs(5))
                .await?)
        })
    }
}

pub struct Adb<C>(pub C);
impl<C: Commands> Adb<C> {
    pub async fn inventory(&self) -> Option<Vec<String>> {
        let output = self.0.execute(vec!["devices".into()], None).await.ok()?;
        if !output.status.success() || output.stdout.len() > 65536 {
            return None;
        }
        crate::adb_inventory::parse(std::str::from_utf8(&output.stdout).ok()?).map(|devices| {
            devices
                .into_iter()
                .filter(|serial| transport_of(serial) == Transport::Usb)
                .collect()
        })
    }
    pub async fn installed(&self, serial: &str) -> Option<bool> {
        let output = self
            .0
            .execute(
                args(
                    serial,
                    &["shell", "pm", "path", blent_config::android::PACKAGE],
                ),
                None,
            )
            .await
            .ok()?;
        crate::adb_inventory::package_presence(&output)
    }
    pub async fn checked(&self, arguments: Vec<String>, input: Option<Vec<u8>>) -> Result<Vec<u8>> {
        let output = self.0.execute(arguments, input).await?;
        // Android diagnostics may contain the credential command. Never log it.
        ensure!(
            output.status.success(),
            "ADB command failed (exit {:?})",
            output.status.code()
        );
        ensure!(output.stdout.len() <= 65536, "ADB reply exceeds 64 KiB");
        Ok(output.stdout)
    }
    pub async fn routes(&self, serial: &str) -> Result<String> {
        String::from_utf8(
            self.checked(args(serial, &["reverse", "--list"]), None)
                .await?,
        )
        .context("ADB reverse listing is not UTF-8")
    }
    pub async fn deliver(&self, serial: &str, token: &str, launch: bool) -> Result<()> {
        ensure!(
            token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "Invalid attachment credential"
        );
        let command = if launch {
            blent_config::android::app_launch_command(Some(token))
        } else {
            blent_config::android::token_delivery_command(Some(token))
        };
        self.checked(args(serial, &["shell", "-T"]), Some(command.into_bytes()))
            .await?;
        Ok(())
    }
}

pub fn args(serial: &str, arguments: &[&str]) -> Vec<String> {
    ["-s", serial]
        .into_iter()
        .chain(arguments.iter().copied())
        .map(str::to_owned)
        .collect()
}

fn valid_serial(serial: &str) -> bool {
    !serial.is_empty()
        && serial.len() <= 1024
        && !serial.chars().any(char::is_control)
        && transport_of(serial) == Transport::Usb
}

#[cfg(test)]
mod tests;

//! A dedicated, temporary ADB reverse mapping and shell-protected invitation.
use anyhow::{ensure, Context, Result};
use blent_config::{camera::CameraOptions, commands::AsyncCommandExt};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::process::Command;

pub struct Bridge {
    pub adb: PathBuf,
    pub serial: String,
    pub remote: String,
    pub local: String,
}

pub async fn output(adb: &Path, args: &[&str]) -> Result<String> {
    let result = Command::new(adb)
        .args(args)
        .output_timeout(Duration::from_secs(5))
        .await?;
    ensure!(
        result.status.success(),
        "ADB camera command failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}

impl Bridge {
    pub async fn create(adb: PathBuf, serial: Option<&str>, port: u16) -> Result<Self> {
        let serial = match serial {
            Some(serial) => serial.to_owned(),
            None => output(&adb, &["get-serialno"])
                .await
                .context("select one authorized tablet with --serial")?,
        };
        ensure!(!serial.is_empty(), "missing tablet serial");
        let local = format!("tcp:{port}");
        let allocated = output(&adb, &["-s", &serial, "reverse", "tcp:0", &local]).await?;
        let port: u16 = allocated
            .parse()
            .context("ADB did not return the allocated camera port")?;
        ensure!(port != 0, "ADB returned camera port zero");
        Ok(Self {
            adb,
            serial,
            remote: format!("tcp:{port}"),
            local,
        })
    }

    pub async fn invite(&self, token: &str, options: &CameraOptions) -> Result<()> {
        // Only hex and typed numbers enter the remote shell command. The serial
        // stays a distinct local argv element, including spaces/metacharacters.
        ensure!(
            token.len() == 64
                && token
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid camera token"
        );
        let port: u16 = self
            .remote
            .strip_prefix("tcp:")
            .context("invalid camera endpoint")?
            .parse()?;
        ensure!(port != 0, "invalid camera port");
        let lens = match options.lens {
            blent_config::camera::Lens::Front => 0,
            blent_config::camera::Lens::Rear => 1,
        };
        let command = format!(
            "am broadcast -n io.github.geraldo_netto.blent/com.blent.CameraReceiver --es token {token} --ei port {port} --ei width {} --ei height {} --ei fps {} --ei lens {lens} --ez background {} --ei bitrate {} --ei freshness_ms {}\n",
            options.width, options.height, options.fps, options.background, options.bitrate, options.freshness_ms,
        );
        let result = Command::new(&self.adb)
            .args(["-s", &self.serial, "shell"])
            .output_input_timeout(Some(command.as_bytes()), Duration::from_secs(5))
            .await?;
        // ADB may echo the shell input in diagnostics; never surface it.
        ensure!(result.status.success(), "ADB camera invitation failed");
        let result = String::from_utf8_lossy(&result.stdout);
        ensure!(
            result.contains("result=1"),
            "tablet did not accept camera invitation; install updated Blent APK and open it"
        );
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        let mappings = output(&self.adb, &["-s", &self.serial, "reverse", "--list"]).await?;
        if owns_mapping(&mappings, &self.remote, &self.local) {
            output(
                &self.adb,
                &["-s", &self.serial, "reverse", "--remove", &self.remote],
            )
            .await?;
        }
        Ok(())
    }
}

fn owns_mapping(mappings: &str, remote: &str, local: &str) -> bool {
    mappings.lines().any(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        fields.len() == 3 && fields[1] == remote && fields[2] == local
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t539_cleanup_preserves_other_reverse_mappings() {
        assert!(owns_mapping(
            "Usb tcp:9999 tcp:1234\nUsb tcp:8890 tcp:8890",
            "tcp:9999",
            "tcp:1234"
        ));
        assert!(!owns_mapping(
            "Usb tcp:9999 tcp:1235",
            "tcp:9999",
            "tcp:1234"
        ));
        for line in ["", "tcp:9999 tcp:1234", "Usb tcp:9999 tcp:1234 extra"] {
            assert!(!owns_mapping(line, "tcp:9999", "tcp:1234"));
        }
    }
}

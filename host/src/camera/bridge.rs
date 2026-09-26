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
        let port = &self.remote[4..];
        let result = output(
            &self.adb,
            &[
                "-s",
                &self.serial,
                "shell",
                "am",
                "broadcast",
                "-n",
                "io.github.geraldo_netto.blent/com.blent.CameraReceiver",
                "--es",
                "token",
                token,
                "--ei",
                "port",
                port,
                "--ei",
                "width",
                &options.width.to_string(),
                "--ei",
                "height",
                &options.height.to_string(),
                "--ei",
                "fps",
                &options.fps.to_string(),
                "--ei",
                "lens",
                match options.lens {
                    blent_config::camera::Lens::Front => "0",
                    blent_config::camera::Lens::Rear => "1",
                },
                "--ez",
                "background",
                if options.background { "true" } else { "false" },
                "--ei",
                "bitrate",
                &options.bitrate.to_string(),
            ],
        )
        .await?;
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

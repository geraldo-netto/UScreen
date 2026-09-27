//! Installed fork identity is distinct from the retained Kotlin namespace.
pub const PACKAGE: &str = "io.github.geraldo_netto.blent";

pub enum Component {
    MainActivity,
    TokenActivity,
    TokenReceiver,
    CodecReportReceiver,
}
impl Component {
    pub fn adb_name(&self) -> String {
        let class = match self {
            Self::MainActivity => "MainActivity",
            Self::TokenActivity => "TokenActivity",
            Self::TokenReceiver => "TokenReceiver",
            Self::CodecReportReceiver => "CodecReportReceiver",
        };
        format!("{PACKAGE}/com.blent.{class}")
    }
}

/// Commands travel over ADB stdin, so the token never enters host argv.
pub fn app_launch_command(token: Option<&str>) -> String {
    use crate::android::Component;
    let component = if token.is_some() {
        Component::TokenActivity
    } else {
        Component::MainActivity
    };
    let mut cmd = format!("am start -n {}", component.adb_name());
    if let Some(t) = token {
        // Hex only, so no quoting is needed and nothing can break out.
        cmd.push_str(" --es token ");
        cmd.push_str(t);
    }
    cmd.push_str(" >/dev/null 2>&1; exit\n");

    cmd
}

pub fn token_delivery_command(token: Option<&str>) -> String {
    let mut command = format!(
        "am broadcast -n {}",
        crate::android::Component::TokenReceiver.adb_name()
    );
    if let Some(token) = token {
        command.push_str(" --es token ");
        command.push_str(token);
    }
    command.push_str(" >/dev/null 2>&1; exit\n");
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t250_fork_components_keep_original_class_namespace() {
        for (component, class) in [
            (Component::MainActivity, "MainActivity"),
            (Component::TokenActivity, "TokenActivity"),
            (Component::TokenReceiver, "TokenReceiver"),
            (Component::CodecReportReceiver, "CodecReportReceiver"),
        ] {
            assert_eq!(
                component.adb_name(),
                format!("io.github.geraldo_netto.blent/com.blent.{class}")
            );
        }
    }
}

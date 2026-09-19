//! Installed fork identity is distinct from the retained Kotlin namespace.
pub const PACKAGE: &str = "io.github.geraldo_netto.uscreen";

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
        format!("{PACKAGE}/com.uscreen.{class}")
    }
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
                format!("io.github.geraldo_netto.uscreen/com.uscreen.{class}")
            );
        }
    }
}

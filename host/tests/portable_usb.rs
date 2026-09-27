//! T525: real native argument/stdin transport on each supported host platform.
use blent::usb::{Adb, NativeCommands, Routes};
#[path = "support/usb_adb.rs"]
mod usb_fixture;
#[tokio::test]
async fn t525_native_adb_argv_stdin_bounds_and_owned_routes() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("ADB café 東京 & spaces");
    std::fs::create_dir(&directory).unwrap();
    usb_fixture::fixture(&directory);
    let adb = Adb(NativeCommands(
        directory.join(blent_config::platform::executable_name("adb")),
    ));
    // T650: exercise the shared query entry point with the same native fixture.
    assert_eq!(
        blent::adb_inventory::query(adb.0 .0.to_str().unwrap()).await,
        Some(vec!["USB".into()])
    );
    assert_eq!(adb.inventory().await, Some(vec!["USB".into()]));
    assert_eq!(adb.installed("USB").await, Some(true));
    let mut routes = Routes::new("USB", (9000, 9001)).unwrap();
    routes.prepare(&adb).await.unwrap();
    assert_eq!(adb.routes("USB").await.unwrap().lines().count(), 2);
    let token = "a".repeat(64);
    adb.deliver("USB", &token, true).await.unwrap();
    let invoked = std::fs::read_to_string(directory.join("invoked")).unwrap();
    assert_eq!(invoked, "-s\nUSB\nshell\n-T");
    assert!(!invoked.contains(&token));
    assert!(std::fs::read_to_string(directory.join("delivered"))
        .unwrap()
        .contains(&token));
    adb.deliver("USB", &token, false).await.unwrap();
    assert!(std::fs::read_to_string(directory.join("delivered"))
        .unwrap()
        .contains("am broadcast"));
    routes.retire(&adb).await.unwrap();
    assert!(adb.routes("USB").await.unwrap().is_empty());
    for text in [vec![255], vec![b' '; 65537]] {
        std::fs::write(directory.join("inventory"), text).unwrap();
        assert_eq!(adb.inventory().await, None);
    }
    let missing = Adb(NativeCommands(directory.join("missing.exe")));
    assert!(missing.inventory().await.is_none());
    assert!(missing.installed("USB").await.is_none());
    assert!(missing.routes("USB").await.is_err());
}

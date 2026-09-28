use super::*;
#[path = "../tests/support/usb_adb.rs"]
mod fixture;
#[tokio::test]
async fn t691_native_wifi_action_updates_only_injected_store() {
    let root = tempfile::tempdir().unwrap();
    let store = blent_config::storage::ConfigStore::new(root.path().join("config.toml"));
    assert!(setup_wifi(false, None, &store).await.is_err());
    assert!(!store.path().unwrap().exists());
    fixture::fixture(root.path());
    let adb = Some(root.path().join("adb.exe"));
    setup_wifi(false, adb.clone(), &store).await.unwrap();
    assert_eq!(store.load().wifi_address, "192.0.2.1:5555");
    setup_wifi(true, adb, &store).await.unwrap();
    assert!(store.load().wifi_address.is_empty());
}

#[tokio::test]
async fn t691_wifi_off_forgets_address_even_without_adb() {
    let root = tempfile::tempdir().unwrap();
    let store = blent_config::storage::ConfigStore::new(root.path().join("config.toml"));
    store
        .update(|c| {
            c.wifi_address = "192.0.2.1:5555".into();
            Ok(())
        })
        .unwrap();
    setup_wifi(true, None, &store)
        .await
        .expect("T691: forgetting saved Wi-Fi must not require ADB installation");
    assert!(store.load().wifi_address.is_empty());
}

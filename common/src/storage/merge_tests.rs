//! T703: merge independent nested preferences through real locked transactions.
use super::*;

#[test]
fn t703_two_camera_snapshots_preserve_independent_edits_in_both_orders() {
    for reverse in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(root.path().join("config.toml"));
        let baseline = store.update(|_| Ok(())).unwrap();
        let mut mirror = baseline.clone();
        mirror.camera.options.mirror = true;
        let mut fps = baseline.clone();
        fps.camera.options.fps = 15;
        let edits = if reverse {
            [&fps, &mirror]
        } else {
            [&mirror, &fps]
        };
        for edit in edits {
            store.save_edits(edit, &baseline).unwrap();
        }
        let mut expected = baseline.clone();
        expected.camera.options.mirror = true;
        expected.camera.options.fps = 15;
        assert_eq!(
            store.load(),
            expected,
            "T703: lost an independent camera edit"
        );
        assert_eq!(store.save_edits(&baseline, &baseline).unwrap(), expected);
    }
}

#[test]
fn t703_same_camera_field_keeps_last_edited_save_wins() {
    for values in [[15, 20], [20, 15]] {
        let root = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(root.path().join("config.toml"));
        let baseline = store.update(|_| Ok(())).unwrap();
        for value in values {
            let mut edited = baseline.clone();
            edited.camera.options.fps = value;
            store.save_edits(&edited, &baseline).unwrap();
        }
        assert_eq!(store.load().camera.options.fps, values[1]);
    }
}

#[test]
fn t703_optional_serial_removal_and_addition_preserve_other_edits() {
    for original in [None, Some("USB-A".to_owned())] {
        for reverse in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let store = ConfigStore::new(root.path().join("config.toml"));
            let baseline = store
                .update(|config| {
                    config.camera.options.serial = original.clone();
                    Ok(())
                })
                .unwrap();
            let mut serial = baseline.clone();
            serial.camera.options.serial = if original.is_some() {
                None
            } else {
                Some("USB-B".into())
            };
            let mut other = baseline.clone();
            other.camera.options.rotation = 90;
            other.profile_cache = true;
            let edits = if reverse {
                [&other, &serial]
            } else {
                [&serial, &other]
            };
            for edit in edits {
                store.save_edits(edit, &baseline).unwrap();
            }
            let mut expected = other;
            expected.camera.options.serial = serial.camera.options.serial;
            assert_eq!(
                store.load(),
                expected,
                "T703: optional field and independent edit interfered"
            );
        }
    }
}

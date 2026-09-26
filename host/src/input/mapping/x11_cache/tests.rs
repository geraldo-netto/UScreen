use super::*;
use std::os::unix::fs::PermissionsExt;

const PROPERTIES: &str = "Device Node (123): \"/dev/input/event10\"\nCoordinate Transformation Matrix (124): 1, 0, 0, 0, 1, 0, 0, 0, 1\n";

#[test]
fn t625_readback_requires_bounded_node_and_finite_nine_value_matrix() {
    assert!(valid_properties(PROPERTIES));
    for text in ["", "Device Node (123): \"\"", "Coordinate Transformation Matrix (124): 1", &"a".repeat(16_385)] {
        assert!(!valid_properties(text));
    }
    for bad in ["NaN", "inf", "-inf", "1e999", "🦀", ""] {
        assert!(!valid_properties(&PROPERTIES.replace("1, 0", &format!("{bad}, 0"))));
    }
    for count in 0..=32 {
        let text = format!("Device Node (123): \"/dev/input/event10\"\nCoordinate Transformation Matrix (124): {}\n", vec!["0"; count].join(","));
        assert_eq!(valid_properties(&text), count == 9);
    }
    for bad in ["Device Nodes", "Device Node (bad):", "Device Node (123): /dev/input/event10", "Device Node (123): \"/tmp/event10\""] {
        assert!(!valid_properties(&PROPERTIES.replace(PROPERTIES.lines().next().unwrap(), bad)));
    }
    for byte in 0..=255u8 {
        let text = format!("{}{}", char::from(byte), PROPERTIES);
        let _ = valid_properties(&text);
    }
}

#[test]
fn t625_topology_missing_device_and_reused_identity_invalidate_readback() {
    let ident = DeviceIdentity::for_instance(0);
    let list = "Blent Touch id=10 [slave pointer]";
    let mut cache = Mappings::default();
    cache.prepare("initial topology", list, &ident);
    let insert = |cache: &mut Mappings| { cache.devices.insert("10".into(), ("Blent Touch".into(), PROPERTIES.into())); };
    insert(&mut cache);
    cache.prepare("initial topology", list, &ident);
    assert_eq!(cache.devices.len(), 1);
    for (topology, devices) in [
        ("new geometry, EDID or primary", list),
        ("new geometry, EDID or primary", ""),
        ("new geometry, EDID or primary", "Blent Touch 2 id=10 [slave pointer]"),
        ("new geometry, EDID or primary", "Blent Pointer id=10 [slave pointer]"),
    ] {
        insert(&mut cache);
        cache.prepare(topology, devices, &ident);
        assert!(cache.devices.is_empty());
    }
}

#[tokio::test]
async fn t625_changed_failed_and_missing_property_reads_remap_without_trusting_old_state() {
    let root = tempfile::tempdir().unwrap();
    let program = root.path().join("xinput");
    let source = format!(r#"#!/usr/bin/python3
import pathlib,sys
root=pathlib.Path({root:?})
if sys.argv[1]=='list-props':
 if (root/'read-failed').exists(): sys.exit(2)
 sys.stdout.buffer.write((root/'properties').read_bytes())
else:
 p=root/'calls'; p.write_text(str(int(p.read_text())+1 if p.exists() else 1))
 if (root/'map-failed').exists(): sys.exit(3)
"#, root=root.path());
    std::fs::write(&program, source).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(root.path().join("properties"), PROPERTIES).unwrap();
    let mut cache = Mappings::default();
    let program = program.to_str().unwrap();
    assert!(cache.map(program, "10", "Blent Touch", "DVI-I-2-1").await);
    assert!(cache.map(program, "10", "Blent Touch", "DVI-I-2-1").await);
    assert_eq!(std::fs::read_to_string(root.path().join("calls")).unwrap(), "1");
    for properties in [PROPERTIES.replace("1, 0", "0.5, 0").into_bytes(), vec![255], b"unavailable".to_vec()] {
        std::fs::write(root.path().join("properties"), &properties).unwrap();
        assert!(cache.map(program, "10", "Blent Touch", "DVI-I-2-1").await);
    }
    std::fs::write(root.path().join("read-failed"), "").unwrap();
    assert!(cache.map(program, "10", "Blent Touch", "DVI-I-2-1").await);
    assert!(cache.devices.is_empty());
    assert_eq!(std::fs::read_to_string(root.path().join("calls")).unwrap(), "5");
    std::fs::write(root.path().join("map-failed"), "").unwrap();
    assert!(!cache.map(program, "10", "Blent Touch", "DVI-I-2-1").await);
    assert!(!cache.map("/nonexistent-t625", "10", "Blent Touch", "DVI-I-2-1").await);
}

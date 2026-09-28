use super::*;
fn monitor(id: &str, left: i32, scale_percent: u32) -> Monitor {
    Monitor {
        id: id.into(),
        name: id.into(),
        bounds: Rect {
            left,
            top: -1080,
            right: left + 1920,
            bottom: 0,
        },
        rotation: Rotation::Identity,
        scale_percent,
        primary: left == 0,
    }
}
#[test]
fn t672_mapping_bounds_rotation_and_mixed_dpi_property() {
    for scale in [100, 125, 150, 200, 300] {
        let panel = monitor("left", -1920, scale);
        for xi in 0..=16 {
            for yi in 0..=16 {
                let (x, y) = panel
                    .project(f64::from(xi) / 16.0, f64::from(yi) / 16.0)
                    .unwrap();
                assert!((-1920..0).contains(&x));
                assert!((-1080..0).contains(&y));
                panel.bounds.normalize(x, y).unwrap();
            }
        }
        assert_eq!(panel.project(0.0, 0.0).unwrap(), (-1920, -1080));
        assert_eq!(panel.project(1.0, 1.0).unwrap(), (-1, -1));
        assert_eq!(panel.bounds.normalize(-1, -1).unwrap(), (65535, 65535));
        for (rotation, expected) in [
            (Rotation::Identity, (-1920, -1080)),
            (Rotation::Clockwise90, (-1, -1080)),
            (Rotation::Clockwise180, (-1, -1)),
            (Rotation::Clockwise270, (-1920, -1)),
        ] {
            let rotated = Monitor {
                rotation,
                ..panel.clone()
            };
            assert_eq!(rotated.project_panel(0.0, 0.0).unwrap(), expected);
            assert_eq!(
                rotated.project(0.0, 0.0).unwrap(),
                (-1920, -1080),
                "T672: displayed coordinates must not rotate twice"
            );
        }
    }
}
#[test]
fn t672_invalid_coordinates_geometry_identity_and_ranges_are_rejected() {
    let panel = monitor("a", 0, 100);
    for bad in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.0,
        -f64::EPSILON,
        1.0 + f64::EPSILON,
        f64::MAX,
    ] {
        assert!(panel.project(bad, 0.5).is_err());
        assert!(panel.project(0.5, bad).is_err());
        assert!(panel.project_panel(bad, 0.5).is_err());
    }
    for id in ["".into(), "a\n".into(), "x".repeat(1025)] {
        assert!(Monitor {
            id,
            ..panel.clone()
        }
        .validate()
        .is_err());
    }
    for scale_percent in [0, 1001, u32::MAX] {
        assert!(Monitor {
            scale_percent,
            ..panel.clone()
        }
        .validate()
        .is_err());
    }
    for delta in [0, -1, -100] {
        let bounds = Rect {
            right: delta,
            ..panel.bounds
        };
        assert!(bounds.project(0.0, 0.0).is_err());
        assert!(bounds.normalize(0, 0).is_err());
    }
    for (x, y) in [
        (-1, -1),
        (1920, -1),
        (0, 0),
        (0, -1081),
        (i32::MIN, i32::MAX),
    ] {
        assert!(panel.bounds.normalize(x, y).is_err());
    }
    let single = Rect {
        left: 0,
        top: 0,
        right: 1,
        bottom: 1,
    };
    assert_eq!(single.project(1.0, 1.0).unwrap(), (0, 0));
    assert_eq!(single.normalize(0, 0).unwrap(), (0, 0));
    let extreme = Rect {
        left: i32::MIN,
        top: i32::MIN,
        right: i32::MAX,
        bottom: i32::MAX,
    };
    assert_eq!(
        extreme.project(1.0, 1.0).unwrap(),
        (i32::MAX - 1, i32::MAX - 1)
    );
}
#[test]
fn t672_selection_requires_exact_identity_and_current_topology() {
    let a = monitor("a", -1920, 100);
    let b = monitor("b", 0, 150);
    assert!(Snapshot::new(vec![]).is_err());
    assert!(Snapshot::new(vec![a.clone(); 129]).is_err());
    assert!(Snapshot::new(vec![a.clone(), a.clone()]).is_err());
    let snapshot = Snapshot::new(vec![a.clone(), b.clone()]).unwrap();
    assert_eq!(snapshot.monitors().len(), 2);
    assert_eq!(snapshot.desktop().left, -1920);
    assert!(snapshot.select("unknown").is_err());
    let selected = snapshot.select("a").unwrap();
    assert!(selected.current(&Snapshot::new(vec![b.clone(), a.clone()]).unwrap()));
    assert_eq!(selected.absolute(&snapshot, 0.0, 0.0).unwrap(), (0, 0));
    assert_eq!(selected.project(&snapshot, 1.0, 1.0).unwrap(), (-1, -1));
    for changed in [
        vec![b.clone()],
        vec![
            a.clone(),
            Monitor {
                scale_percent: 200,
                ..b.clone()
            },
        ],
        vec![
            Monitor {
                rotation: Rotation::Clockwise90,
                ..a.clone()
            },
            b.clone(),
        ],
    ] {
        assert!(selected
            .project(&Snapshot::new(changed).unwrap(), 0.5, 0.5)
            .is_err());
    }
}

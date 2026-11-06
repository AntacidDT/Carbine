//! Reading a real authored asset, rather than the fixtures built in `reader.rs`.
//!
//! The file is not in the repository — it is a licensed model asset kept beside it — so the tests
//! are ignored by default and run by hand, pointing at wherever the asset lives:
//!
//! ```text
//! CARBINE_ASSET=/home/ant/Documents/carbinevehicleassetsMITlicensed.glb \
//!     cargo test -p carbine-glb -- --ignored --test-threads=1
//! ```
//! Without the variable set there is nothing to measure, and the tests say so instead of passing.

use carbine_glb::{Model, is_glb, read, read_scene, write};
use glam::{Mat4, Vec3};

/// The asset the environment names, as the bytes it holds.
fn bytes() -> Vec<u8> {
    let path = std::env::var("CARBINE_ASSET")
        .unwrap_or_else(|_| panic!("set CARBINE_ASSET to the model asset to run this test"));
    std::fs::read(&path).unwrap_or_else(|why| panic!("cannot read {path}: {why}"))
}

/// The asset the environment names, read as a Z-up tree.
fn asset() -> Model {
    let bytes = bytes();
    assert!(is_glb(&bytes), "the asset does not open with the GLB magic");
    read(&bytes).unwrap_or_else(|why| panic!("{why}, for an asset that starts with glTF"))
}

/// The box one branch occupies once every node under it is folded into its parent.
fn bounds_from(model: &Model, root: usize) -> Option<(Vec3, Vec3)> {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    let mut found = false;
    let mut stack = vec![(root, Mat4::IDENTITY)];
    while let Some((index, parent)) = stack.pop() {
        let node = &model.nodes[index];
        let world = parent * node.transform.matrix();
        for part in &node.parts {
            for position in &part.positions {
                let place = world.transform_point3(*position);
                min = min.min(place);
                max = max.max(place);
                found = true;
            }
        }
        for child in &node.children {
            stack.push((*child, world));
        }
    }
    found.then_some((min, max))
}

/// The extent of one named branch, folded into place.
fn extent(model: &Model, name: &str) -> Vec3 {
    let index = model
        .find(name)
        .unwrap_or_else(|| panic!("no {name} branch"));
    let (min, max) = bounds_from(model, index).unwrap();
    max - min
}

#[test]
#[ignore]
fn reads_the_vehicle_asset_as_a_tree_of_named_nodes() {
    let model = asset();

    // What the file declares: ten vehicles in one scene, sixty-eight primitives, and the
    // hierarchy intact — a turret is still under its tank rather than welded into its hull.
    assert_eq!(model.nodes.len(), 26);
    assert_eq!(model.roots.len(), 10);
    assert_eq!(model.part_count(), 68);
    assert_eq!(model.vertex_count(), 55_643);
    assert_eq!(model.triangle_count(), 33_689);

    assert_eq!(
        model
            .roots
            .iter()
            .map(|root| model.nodes[*root].name.clone())
            .collect::<Vec<_>>(),
        [
            Some("Battleship".into()),
            Some("Bomber Plane".into()),
            Some("Fighter Jet".into()),
            Some("Transport Plane".into()),
            Some("Transport Ship".into()),
            Some("Medium Tank".into()),
            Some("DualRocketTransportPlane".into()),
            Some("Transport vehicle".into()),
            Some("Heavy Tank".into()),
            Some("Infantry Vehicle.014".into()),
        ]
    );

    // The branches the game code hangs seats and turrets on are still addressable by name.
    for name in [
        "HT Turret",
        "MT Turret",
        "driver/driverSeat.001",
        "backleft/seat",
        "Rocket1",
    ] {
        assert!(model.find(name).is_some(), "{name} is missing");
    }
    assert_eq!(
        model.nodes[model.find("Heavy Tank").unwrap()]
            .children
            .len(),
        1
    );

    // The named seat nodes are children of their vehicles, standing where the file put them.
    let tank = &model.nodes[model.find("Heavy Tank").unwrap()];
    let turret = &model.nodes[tank.children[0]];
    assert_eq!(turret.name.as_deref(), Some("HT Turret"));
    assert_eq!(turret.parts.len(), 1);
}

#[test]
#[ignore]
fn stands_the_asset_up_without_moving_anything_it_should_not() {
    let model = asset();

    // glTF's own POSITION bounds for the widest primitive in this file are
    // x [-37.4, 35.6], y [-6.29, 12.8], z [-25.72, 36.82]. Reading it into a Z-up world
    // keeps x, and swaps the other two: (x, y, z) becomes (x, -z, y).
    let plane = extent(&model, "Transport Plane");
    assert!(
        plane.distance(Vec3::new(73.0, 62.55, 19.09)) < 0.02,
        "Transport Plane came out {plane}"
    );
    // A tank that is 2.2 m wide, 6.8 m along its nose, and a metre tall is a tank in metres.
    let heavy = extent(&model, "Heavy Tank");
    assert!(
        heavy.distance(Vec3::new(2.21, 6.78, 1.03)) < 0.02,
        "Heavy Tank came out {heavy}"
    );
    // The jet is the small one, not the 73 metre thing — which is the whole file's box.
    assert!(extent(&model, "Fighter Jet").z < 2.0);
}

#[test]
#[ignore]
fn takes_one_named_vehicle_out_of_the_hangar() {
    let bytes = bytes();
    let tank = read_scene(&bytes, Some("Heavy Tank")).unwrap();

    // Just the tank and its turret, renumbered from zero, standing where it stood before.
    assert_eq!(tank.nodes.len(), 2);
    assert_eq!(tank.roots, vec![0]);
    assert_eq!(tank.part_count(), 3);
    assert_eq!(tank.nodes[0].name.as_deref(), Some("Heavy Tank"));
    assert_eq!(tank.nodes[1].name.as_deref(), Some("HT Turret"));
    assert_eq!(tank.nodes[0].children, vec![1]);
    assert!(tank.nodes[1].children.is_empty());

    // A name that is not in the file is a refusal, not an empty model.
    let why = read_scene(&bytes, Some("Silo")).unwrap_err();
    assert_eq!(why.to_string(), "no node is named \"Silo\"");
}

#[test]
#[ignore]
fn writes_the_asset_out_and_reads_the_same_asset_back() {
    let model = asset();
    let again = read(&write(&model)).unwrap();

    assert_eq!(again.nodes.len(), model.nodes.len());
    assert_eq!(again.roots, model.roots);
    assert_eq!(again.triangle_count(), model.triangle_count());
    assert_eq!(again.vertex_count(), model.vertex_count());
    for (index, node) in model.nodes.iter().enumerate() {
        let other = &again.nodes[index];
        assert_eq!(other.name, node.name);
        assert_eq!(other.children, node.children);
        assert_eq!(other.parts.len(), node.parts.len());
        assert!(
            node.transform
                .matrix()
                .abs_diff_eq(other.transform.matrix(), 1e-5),
            "{} moved",
            node.name.as_deref().unwrap_or("-")
        );
        for (part, other) in node.parts.iter().zip(&other.parts) {
            assert_eq!(part.positions, other.positions);
            assert_eq!(part.indices, other.indices);
            assert_eq!(part.uvs, other.uvs);
            assert!(part.material.name == other.material.name);
            // A colour goes linear -> displayed on the way in and back on the way out, so
            // what survives is the picture, not the exact bits.
            for (channel, other) in part.material.color.iter().zip(other.material.color) {
                assert!((channel - other).abs() < 1e-5, "{channel} != {other}");
            }
        }
    }
}

"""`carbine.procedural.map()` - the generator that lays ground to walk on instead of
a flat plane.

A map is a node, not a switch: it hangs in the tree, is parented, moved, coloured,
skinned, hidden and destroyed the way any object is, and the knobs it adds are about
the ground - how far it reaches, how tall its hills stand, how finely it is counted
and the dice it was rolled from. It is scenery rather than a body, so the solver never
shoves it; anything the floor catches walks on it without it being cut into a collider.

The hills come out of a seed, so the same word lays the same ground for anybody. These
tests read the same numbers the editor will, and step the world to prove a walker is
held up by the hill it stands on rather than falling through to the flat plane.
"""

import carbine
import pytest

SECOND = 1.0 / 60.0


@pytest.fixture
def ground(headless):
    """A world with nothing authored in it. The scene is one per process, so a map -
    or a cube a map was hung on - left standing would hold up the next test's walker
    and paint its frame."""

    def sweep():
        carbine.destroy("all")
        carbine.player().enabled = False

    sweep()
    yield
    sweep()


def test_map_is_a_terrain_node(ground):
    ground_map = carbine.procedural.map(name="Valley")
    assert isinstance(ground_map, carbine.Terrain)
    assert ground_map.kind == "terrain"
    assert ground_map.name == "Valley"
    assert ground_map in carbine.scene.root.children
    assert ground_map.parent == carbine.scene.root


def test_map_defaults(ground):
    ground_map = carbine.procedural.map()
    assert ground_map.name == "Terrain"
    assert tuple(ground_map.size) == (64.0, 64.0)
    assert ground_map.relief == pytest.approx(6.0)
    assert ground_map.quality == "medium"
    assert isinstance(ground_map.seed, int)
    assert 0 <= ground_map.seed <= 18446744073709551615


def test_map_qualities_list(ground):
    assert carbine.procedural.mapQualities() == ["low", "medium", "high"]


def test_map_named_knobs(ground):
    ground_map = carbine.procedural.map(
        size=(128.0, 96.0), relief=12.0, quality="high", seed="valley"
    )
    assert tuple(ground_map.size) == (128.0, 96.0)
    assert ground_map.relief == pytest.approx(12.0)
    assert ground_map.quality == "high"
    assert isinstance(ground_map.seed, int)


def test_map_seed_round_trips(ground):
    ground_map = carbine.procedural.map(seed=12345)
    assert ground_map.seed == 12345
    ground_map.seed = 18446744073709551615
    assert ground_map.seed == 18446744073709551615
    ground_map.seed = "valley"
    worded = ground_map.seed
    assert isinstance(worded, int)
    # The same word lays the same dice; a different word lays a different one.
    again = carbine.procedural.map(seed="valley")
    other = carbine.procedural.map(seed="vallez")
    assert again.seed == worded
    assert other.seed != worded


def test_map_size_accepts_vec3(ground):
    ground_map = carbine.procedural.map(size=carbine.Vec3(100.0, 80.0, 0.0))
    assert tuple(ground_map.size) == (100.0, 80.0)
    ground_map.size = carbine.Vec3(40.0, 20.0, 99.0)
    assert tuple(ground_map.size) == (40.0, 20.0)


def test_map_knobs_relayout(ground):
    ground_map = carbine.procedural.map(seed="knobs")
    ground_map.relief = 2.0
    assert ground_map.relief == pytest.approx(2.0)
    ground_map.size = (16.0, 16.0)
    assert tuple(ground_map.size) == (16.0, 16.0)
    ground_map.quality = "low"
    assert ground_map.quality == "low"
    ground_map.seed = "elsewhere"
    assert isinstance(ground_map.seed, int)


def test_map_bad_quality_refused(ground):
    with pytest.raises(ValueError, match="ultra"):
        carbine.procedural.map(quality="ultra")
    ground_map = carbine.procedural.map()
    with pytest.raises(ValueError, match="available"):
        ground_map.quality = "ultra"


def test_map_bad_size_refused(ground):
    with pytest.raises(TypeError):
        carbine.procedural.map(size="big")
    ground_map = carbine.procedural.map()
    with pytest.raises(TypeError):
        ground_map.size = 5.0


def test_new_terrain_returns_terrain(ground):
    assert "terrain" in carbine.kinds()
    ground_map = carbine.new("terrain")
    assert isinstance(ground_map, carbine.Terrain)
    assert ground_map.kind == "terrain"
    # The class the node is comes back through the tree as well.
    assert isinstance(carbine.scene.root.children[-1], carbine.Terrain)


def test_map_handle_equality_and_hash(ground):
    ground_map = carbine.procedural.map(name="Same")
    found = next(n for n in carbine.scene.root.children if n.name == "Same")
    assert found == ground_map
    assert hash(found) == hash(ground_map)
    assert len({ground_map, found}) == 1


def test_map_parenting(ground):
    holder = carbine.new("cube", name="Holder")
    ground_map = carbine.procedural.map(parent=holder)
    assert ground_map.parent == holder
    assert ground_map in holder.children
    holder.removeChild(ground_map)
    assert ground_map.parent == carbine.scene.root


def test_map_skin_and_colour(ground, skin):
    ground_map = carbine.procedural.map()
    ground_map.color = "#3a7d44"
    assert ground_map.colorHex == "#3a7d44"
    ground_map.texture = skin
    assert ground_map.texture == skin
    ground_map.textureTiling = 2.0
    assert ground_map.textureTiling == pytest.approx(2.0)
    ground_map.transparent = 128.0
    assert ground_map.transparent == pytest.approx(128.0)
    ground_map.visible = False
    assert ground_map.visible is False


def test_map_destroy(ground):
    ground_map = carbine.procedural.map()
    node_id = ground_map.id
    ground_map.destroy()
    assert all(n.id != node_id for n in carbine.scene.root.children)
    with pytest.raises(carbine.DestroyedError):
        _ = ground_map.relief


def test_duplicate_map_is_ground(ground):
    ground_map = carbine.procedural.map(seed="copy", relief=8.0)
    copies = carbine.duplicate(ground_map, 2)
    assert len(copies) == 2
    for copy in copies:
        assert isinstance(copy, carbine.Terrain)
        assert copy.relief == pytest.approx(8.0)
        assert copy.seed == ground_map.seed
        assert copy != ground_map


def test_linked_maps_share_knobs(ground):
    ground_map = carbine.procedural.map(seed="twins")
    twins = carbine.linkDuplicate(ground_map, 1)
    twin = twins[0]
    assert isinstance(twin, carbine.Terrain)
    ground_map.relief = 3.0
    assert twin.relief == pytest.approx(3.0)
    twin.position = (10.0, 0.0, 0.0)
    assert tuple(ground_map.position) == (0.0, 0.0, 0.0)


def test_walker_is_held_up_by_the_map(ground):
    # Lift the whole map five metres so the floor under its centre is clearly the
    # map's and not the flat plane the world is born with.
    ground_map = carbine.procedural.map(size=(32.0, 32.0), relief=4.0, seed="walk")
    ground_map.position = (0.0, 0.0, 5.0)

    player = carbine.player()
    player.enabled = True
    player.position = (0.0, 0.0, 30.0)
    for _ in range(300):
        carbine.step(SECOND)

    assert player.grounded
    assert player.position.z >= 5.0 - 1e-2, "the walker stands on the lifted map"
    assert abs(player.velocity.z) < 1e-2
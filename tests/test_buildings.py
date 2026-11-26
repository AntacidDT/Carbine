"""`carbine.procedural.building()` — a seeded node tree standing on the ground.

A building is not one welded mesh: floors, walls, windows and a roof hang under a
root as ordinary nodes, so every piece can be moved, skinned or hidden like anything
else in the scene. Everything is rolled from the seed before the first piece is laid.
"""

import carbine
import pytest


@pytest.fixture
def quiet():
    def sweep():
        carbine.destroy("all")
        carbine.player().enabled = False

    sweep()
    yield
    sweep()


def windows(building):
    found = []
    for floor in building.children:
        for piece in floor.children:
            if piece.name.startswith("Window"):
                found.append((piece.name, tuple(round(v, 3) for v in piece.position)))
    return found


def test_building_styles_are_listed(quiet):
    assert carbine.procedural.buildingStyles() == ["house", "tower", "cabin"]


def test_a_building_is_a_tree_of_nodes(quiet):
    root = carbine.procedural.building(
        name="Home", position=(4.0, 5.0, 0.0), floors=3, width=8.0, depth=6.0,
        style="cabin", seed="home",
    )

    assert isinstance(root, carbine.Object)
    assert root.name == "Home"
    assert tuple(root.position) == (4.0, 5.0, 0.0)
    assert root.anchored is True
    names = [child.name for child in root.children]
    assert "Floor 1" in names and "Floor 2" in names and "Floor 3" in names
    assert "Roof" in names
    first = next(child for child in root.children if child.name == "Floor 1")
    sides = [piece.name for piece in first.children]
    assert {"Front 1", "Back 1", "Left 1", "Right 1"}.issubset(set(sides))
    assert any(name.startswith("Window 1.") for name in sides)
    assert first.anchored is True


def test_the_same_seed_raises_the_same_windows(quiet):
    first = carbine.procedural.building(seed="windows", floors=2)
    again = carbine.procedural.building(seed="windows", floors=2)
    other = carbine.procedural.building(seed="windowz", floors=2)

    assert windows(first) == windows(again)
    assert windows(first) != windows(other)


def test_a_bad_style_is_refused(quiet):
    with pytest.raises(ValueError, match="castle"):
        carbine.procedural.building(style="castle")


def test_destroy_takes_the_whole_building(quiet):
    root = carbine.procedural.building(name="Doomed")
    node_id = root.id
    root.destroy()
    assert all(node.id != node_id for node in carbine.scene.root.children)

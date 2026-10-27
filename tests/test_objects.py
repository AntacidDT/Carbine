import math

import carbine
import pytest


def test_version():
    assert carbine.__version__ == "0.1.0"


def test_kinds_are_advertised():
    assert carbine.kinds() == ["rectangle", "square"]


def test_new_rectangle_defaults():
    card = carbine.new("rectangle")
    try:
        assert card.kind == "rectangle"
        assert card.name == "Rectangle"
        assert card.parent == carbine.scene.root
        assert card.mesh.vertexCount == 4
        assert card.mesh.triangleCount == 2
        assert card.mesh.indices == [0, 1, 2, 0, 2, 3]

        low, high = card.mesh.bounds
        assert low.toTuple() == (-0.5, -0.5, 0.0)
        assert high.toTuple() == (0.5, 0.5, 0.0)
    finally:
        card.destroy()


def test_transform_handles_write_through():
    card = carbine.new("rectangle")
    try:
        card.position.x = 2.0
        assert tuple(card.position) == (2.0, 0.0, 0.0)

        card.position = (1, 2, 3)
        assert card.position.toTuple() == (1.0, 2.0, 3.0)

        card.scale.y = 4.0
        assert card.scale == carbine.Vec3(1.0, 4.0, 1.0)

        card.rotationEuler.z = 90.0
        assert math.isclose(card.rotationEuler.z, 90.0, abs_tol=1e-4)
        assert math.isclose(card.rotation.toEuler()[2], 90.0, abs_tol=1e-4)

        card.rotation = carbine.Quat.fromEuler(0.0, 0.0, 45.0)
        assert math.isclose(card.rotationEuler.z, 45.0, abs_tol=1e-4)
    finally:
        card.destroy()


def test_size_is_meters_and_scale_is_the_factor():
    card = carbine.new("rectangle", name="Card")
    try:
        card.size = (3.0, 1.0, 1.0)
        assert card.size.toTuple() == (3.0, 1.0, 1.0)
        assert card.scale.toTuple() == (3.0, 1.0, 1.0)

        card.size.x = 5.0
        assert card.size.toTuple() == (5.0, 1.0, 1.0)
    finally:
        card.destroy()


def test_colour_defaults_to_carbine_orange():
    card = carbine.new("rectangle")
    try:
        red, green, blue = card.color.toTuple()
        assert (round(red, 3), round(green, 3), round(blue, 3)) == (0.921, 0.478, 0.204)

        card.color = (0.0, 1.0, 0.0)
        assert card.color.toTuple() == (0.0, 1.0, 0.0)
    finally:
        card.destroy()


def test_hierarchy():
    parent = carbine.new("rectangle", name="Parent")
    child = carbine.new("square", parent=parent, name="Child")
    try:
        assert child.parent == parent
        assert parent.children == [child]

        child.parent = None
        assert child.parent == carbine.scene.root
        assert parent.children == []

        child.parent = parent
        with pytest.raises(ValueError):
            parent.parent = child
    finally:
        parent.destroy()


def test_recursive_destroy():
    parent = carbine.new("rectangle")
    child = carbine.new("rectangle", parent=parent)

    parent.destroy()

    with pytest.raises(carbine.DestroyedError):
        parent.name
    with pytest.raises(carbine.DestroyedError):
        child.name


def test_root_cannot_be_destroyed():
    with pytest.raises(ValueError):
        carbine.scene.root.destroy()


def test_unknown_kind():
    with pytest.raises(ValueError, match="rectangle"):
        carbine.new("triangle")


def test_node_count_and_identity():
    before = carbine.scene.nodeCount
    first = carbine.new("rectangle")
    second = carbine.new("rectangle", parent=first)
    try:
        assert carbine.scene.nodeCount == before + 2
        assert first.id != second.id
        assert first != second
        assert len({first, second}) == 2
    finally:
        first.destroy()
    assert carbine.scene.nodeCount == before

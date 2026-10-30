import math

import carbine
import pytest


def test_version():
    assert carbine.__version__ == "0.1.0"


def test_kinds_are_advertised():
    assert carbine.kinds() == ["cube", "plane", "sphere", "rectangle", "square"]


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


def test_an_axis_left_out_keeps_the_value_it_had():
    card = carbine.new("cube", name="Card")
    try:
        card.size = (2.0, 3.0, 4.0)

        card.size = (..., ..., 8.0)
        assert card.size.toTuple() == (2.0, 3.0, 8.0)

        card.size = (None, 5.0, None)
        assert card.size.toTuple() == (2.0, 5.0, 8.0)

        card.position = (1.0, 1.0, ...)
        assert card.position.toTuple() == (1.0, 1.0, 0.0)

        with pytest.raises(TypeError, match="three values"):
            card.size = (1.0, 2.0)
        with pytest.raises(TypeError, match="number"):
            card.size = (1.0, "wide", 3.0)
    finally:
        card.destroy()


def test_rotation_takes_a_quat_or_euler_degrees():
    card = carbine.new("cube")
    try:
        card.rotation = (0.0, 0.0, 90.0)
        assert math.isclose(card.rotationEuler.z, 90.0, abs_tol=1e-4)
        assert math.isclose(card.rotation.toEuler()[2], 90.0, abs_tol=1e-4)

        card.rotation = (10.0, ..., ...)
        assert math.isclose(card.rotationEuler.x, 10.0, abs_tol=1e-4)
        assert math.isclose(card.rotationEuler.z, 90.0, abs_tol=1e-4)

        card.rotation = carbine.Quat.fromEuler(0.0, 0.0, 45.0)
        assert math.isclose(card.rotationEuler.z, 45.0, abs_tol=1e-4)
    finally:
        card.destroy()


def test_a_skin_is_remembered_as_a_path(skin):
    card = carbine.new("cube")
    try:
        assert card.texture is None

        card.texture = skin
        assert card.texture == skin

        card.texture = None
        assert card.texture is None
    finally:
        card.destroy()


def test_a_skin_in_any_other_format_is_refused(tmp_path):
    """Carbine reads one image format, so a picture authored as a PNG is refused when
    an object is skinned rather than quietly drawn in a flat colour - and the refusal
    names the tool that turns it into the format the engine takes."""
    card = carbine.new("cube")
    try:
        picture = tmp_path / "wood.png"
        picture.write_bytes(b"\x89PNG\r\n\x1a\n" + b"\x00" * 24)

        with pytest.raises(ValueError, match="carbine.packager.image") as error:
            card.texture = str(picture)
        assert "not a KTX2 file" in str(error.value), error

        assert card.texture is None, "a refused path leaves the object unskinned"

        with pytest.raises(ValueError, match="no file at that path"):
            card.texture = "assets/nowhere.ktx2"
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


def test_a_child_list_can_be_written_whole():
    panel = carbine.new("rectangle", name="Panel")
    first = carbine.new("square", name="First", parent=panel)
    second = carbine.new("square", name="Second", parent=panel)
    stranger = carbine.new("square", name="Stranger")
    try:
        panel.children = [second, stranger]
        assert panel.children == [second, stranger], "the order written is the order read back"
        assert stranger.parent == panel, "a node named here but hanging elsewhere is adopted"
        assert first.parent == carbine.scene.root
        assert first.name == "First", "left out of the list, so detached rather than destroyed"

        panel.children = first
        assert panel.children == [first], "one node is a one-child list"

        panel.children = []
        assert panel.children == []
        assert first.parent == carbine.scene.root
    finally:
        panel.children = []
        for node in (first, second, stranger, panel):
            node.destroy()


def test_children_are_added_and_removed_one_at_a_time():
    parent = carbine.new("rectangle", name="Parent")
    first = carbine.new("square", name="First", parent=parent)
    newcomer = carbine.new("square", name="Newcomer")
    try:
        parent.addChild(newcomer)
        assert parent.children == [first, newcomer], "a fresh child goes last"

        parent.removeChild(newcomer)
        assert parent.children == [first]
        assert newcomer.parent == carbine.scene.root
        assert newcomer.name == "Newcomer", "a child removed is still in the world"

        with pytest.raises(ValueError):
            parent.removeChild(newcomer)
        with pytest.raises(ValueError):
            parent.addChild(parent)
    finally:
        parent.children = []
        for node in (first, newcomer, parent):
            node.destroy()


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
    with pytest.raises(ValueError, match="dodecahedron"):
        carbine.new("dodecahedron")


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

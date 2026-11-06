import math

import carbine
import pytest


def test_version():
    assert carbine.__version__ == "0.1.0"


def test_kinds_are_advertised():
    # The shapes the geometry knows, and the kinds it does not: a light is a node `new`
    # writes like any other and draws nothing, a model is an anchor a `.glb` hangs on, a
    # vehicle is a box wearing a drive, and the two seats are somewhere a node can sit.
    assert carbine.kinds() == [
        "cube",
        "plane",
        "sphere",
        "rectangle",
        "square",
        "light",
        "model",
        "vehicle",
        "seat",
        "driverSeat",
    ]


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


def test_a_vec3_written_on_is_the_value_the_node_holds():
    card = carbine.new("cube", name="Card")
    copy = carbine.new("cube", name="Copy")
    try:
        card.position = (1.0, 2.0, 3.0)

        copy.position = card.position
        assert copy.position.toTuple() == (1.0, 2.0, 3.0), "a Vec3 writes like a tuple"

        # A Vec3 read off a node is that node's own value rather than a photograph of
        # the moment it was read, which is what makes copying one across mean anything.
        moved = card.position
        card.position = (9.0, ..., ...)
        copy.position = moved
        assert copy.position.toTuple() == (9.0, 2.0, 3.0)

        copy.size = carbine.Vec3(2.0, 3.0, 4.0)
        assert copy.size.toTuple() == (2.0, 3.0, 4.0)

        copy.color = card.color
        assert copy.color == card.color

        copy.rotationEuler = card.rotationEuler
        assert tuple(copy.rotationEuler) == pytest.approx((0.0, 0.0, 0.0), abs=1e-4)
    finally:
        copy.destroy()
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


def test_a_body_weighs_the_box_it_was_made_from():
    box = carbine.new("cube", name="Box")
    try:
        assert box.mass == pytest.approx(200.0), "a metre cube of the world's stuff"

        box.size = (2.0, 3.0, 1.0)
        assert box.mass == pytest.approx(1200.0), "six of them"

        box.mass = 42.0
        assert box.mass == pytest.approx(42.0)

        box.size = (1.0, 1.0, 1.0)
        assert box.mass == pytest.approx(42.0), "a written weight outranks the box"

        box.mass = -8.0
        assert box.mass == 0.0, "nothing weighs less than nothing"
    finally:
        box.destroy()


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


def test_a_skin_lies_one_metre_across_until_a_script_says_otherwise():
    """A tile is measured in metres of the object, so a picture keeps its size from a
    floor tile to a six metre wall instead of being pulled to fit each face."""
    card = carbine.new("cube")
    try:
        assert card.textureTiling == 1.0

        card.textureTiling = 2.5
        assert card.textureTiling == 2.5

        # Nil is the other ask: the whole picture smeared across each face, which is
        # how a model's own UVs already lie.
        card.textureTiling = 0.0
        assert card.textureTiling == 0.0

        card.textureTiling = -4.0
        assert card.textureTiling == 0.0, "a negative reach is no reach at all"
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


@pytest.mark.parametrize(
    "make, has_alpha",
    [
        pytest.param(lambda: carbine.new("cube"), True, id="world"),
        pytest.param(lambda: carbine.ui.new("square"), True, id="ui"),
        pytest.param(lambda: carbine.spawnEntity("cube"), False, id="entity"),
        pytest.param(lambda: carbine.procedural.particles(count=1), True, id="particle"),
    ],
)
def test_named_colours_on_nodes_preserve_rgb_and_alpha(make, has_alpha):
    node = make()
    try:
        node.color = "#1234"
        for name, hex_value, rgb in [
            ("Red", "#ff0000", (255, 0, 0)),
            (" dArK\t RED ", "#8b0000", (139, 0, 0)),
            ("Light   Orange", "#ffb366", (255, 179, 102)),
            ("Banana", "#ffe135", (255, 225, 53)),
            ("\u2003Skin\u00a0color\n", "#f1c27d", (241, 194, 125)),
            ("LIGHT GRAY", "#d3d3d3", (211, 211, 211)),
        ]:
            node.color = name
            assert tuple(node.color) == pytest.approx(tuple(c / 255 for c in rgb))
            if has_alpha:
                assert node.colorHex == hex_value
                assert node.transparent == 68

        node.color = (0.1, 0.2, 0.3)
        assert tuple(node.color) == pytest.approx((0.1, 0.2, 0.3))
        node.color = (..., 0.4, ...)
        assert tuple(node.color) == pytest.approx((0.1, 0.4, 0.3))
        node.color = carbine.Vec3(0.25, 0.5, 0.75)
        assert tuple(node.color) == pytest.approx((0.25, 0.5, 0.75))
        if has_alpha:
            assert node.transparent == 68

        node.color = "12AbCd40"
        assert tuple(node.color) == pytest.approx((18 / 255, 171 / 255, 205 / 255))
        node.color = "Blue"
        assert tuple(node.color) == (0.0, 0.0, 1.0)
        if has_alpha:
            assert node.transparent == 64

        for invalid in ("reddish", "light red extra", "light-red", "", " \t "):
            with pytest.raises(ValueError, match="not a colour"):
                node.color = invalid
            assert tuple(node.color) == (0.0, 0.0, 1.0)
            if has_alpha:
                assert node.transparent == 64
    finally:
        node.destroy()


def test_a_particle_accepts_a_named_colour_at_creation():
    particle = carbine.procedural.particles(count=1, color=" Light Red ")
    try:
        assert particle.colorHex == "#ff7f7f"
        assert particle.transparent == 255
    finally:
        particle.destroy()


@pytest.mark.parametrize("field", ["zenith", "horizon", "fogColor"])
def test_named_sky_and_fog_colours_preserve_rgb_and_ignore_alpha(field):
    target = carbine.atmosphere if field == "fogColor" else carbine.atmosphere.sky()
    before = tuple(getattr(target, field))
    fog = carbine.atmosphere.fog
    try:
        for written, rgb in [
            ("  dArK\tBlue ", (0.0, 0.0, 139 / 255)),
            ("Light Yellow", (1.0, 1.0, 224 / 255)),
            ("#1230", (17 / 255, 34 / 255, 51 / 255)),
            ("aBcDeF00", (171 / 255, 205 / 255, 239 / 255)),
            ((0.1, 0.2, 0.3), (0.1, 0.2, 0.3)),
            (carbine.Vec3(0.25, 0.5, 0.75), (0.25, 0.5, 0.75)),
        ]:
            setattr(target, field, written)
            assert tuple(getattr(target, field)) == pytest.approx(rgb)
            assert carbine.atmosphere.fog == fog

        for invalid in ("bluish", "dark blue extra", " \t "):
            with pytest.raises(ValueError, match="not a colour"):
                setattr(target, field, invalid)
            assert tuple(getattr(target, field)) == (0.25, 0.5, 0.75)
    finally:
        setattr(target, field, before)


def test_named_sky_colour_pairs_reject_unknown_names_atomically():
    sky = carbine.atmosphere.sky()
    before = (tuple(sky.zenith), tuple(sky.horizon))
    try:
        sky.colors(" Navy ", "Light  Orange")
        expected = ((0.0, 0.0, 128 / 255), (1.0, 179 / 255, 102 / 255))
        assert tuple(sky.zenith) == pytest.approx(expected[0])
        assert tuple(sky.horizon) == pytest.approx(expected[1])
        for zenith, horizon in (("reddish", "Red"), ("Red", "reddish")):
            with pytest.raises(ValueError, match="not a colour"):
                sky.colors(zenith, horizon)
            assert tuple(sky.zenith) == pytest.approx(expected[0])
            assert tuple(sky.horizon) == pytest.approx(expected[1])
    finally:
        sky.zenith, sky.horizon = before


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

import carbine
import pytest

WIDTH = 64
HEIGHT = 64

# UI is drawn in pixels, so the camera never enters these assertions: a 40 by 40
# element centred on a 64 by 64 target covers pixels 12 through 51.
ORANGE = (235, 122, 52)
CLEAR = (22, 21, 20)


def pixel(data, x, y):
    offset = (y * WIDTH + x) * 4
    return tuple(data[offset : offset + 3])


def button(**kwargs):
    element = carbine.ui.new("square", **kwargs)
    element.position = (32, 32, 0)
    element.size = (40, 40, 1)
    element.color = "#eb7a34"
    return element


def test_a_ui_element_is_measured_in_pixels(flat):
    element = button(name="Button")
    try:
        assert element.name == "Button"
        assert element.polygon == 4
        assert element.corner == 0
        assert element.outline == 0
        assert element.transparent == 255
        assert element.colorHex == "#eb7a34"

        pixels = carbine.readPixels()
        assert pixel(pixels, 14, 14) == ORANGE
        assert pixel(pixels, 49, 49) == ORANGE
        assert pixel(pixels, 9, 9) == CLEAR
    finally:
        element.destroy()


def test_corners_round_off_as_the_radius_grows(flat):
    element = button()
    try:
        element.corner = 49
        assert element.corner == 49

        pixels = carbine.readPixels()
        assert pixel(pixels, 14, 14) == CLEAR, "the corner is cut away"
        assert pixel(pixels, 32, 14) == ORANGE, "the straight edge stays"
        assert pixel(pixels, 32, 32) == ORANGE, "the middle stays"
    finally:
        element.destroy()


def test_an_outline_strokes_the_edge(flat):
    element = button()
    try:
        element.outline = 4
        element.transparent = 0

        pixels = carbine.readPixels()
        assert pixel(pixels, 14, 32) == ORANGE, "the ring"
        assert pixel(pixels, 32, 32) == CLEAR, "the hollow middle"
        assert pixel(pixels, 11, 32) == CLEAR, "outside"
    finally:
        element.destroy()


def test_transparency_blends_the_element_into_the_frame(flat):
    element = button()
    try:
        element.transparent = 128

        blended = pixel(carbine.readPixels(), 14, 14)
        assert (126 <= blended[0] <= 132), f"expected a blended orange, got {blended}"
        assert 68 <= blended[1] <= 75, f"expected a blended orange, got {blended}"
        assert 33 <= blended[2] <= 39, f"expected a blended orange, got {blended}"
        assert pixel(carbine.readPixels(), 9, 9) == CLEAR

        element.transparent = 300
        assert element.transparent == 255
    finally:
        element.destroy()


def test_the_polygon_count_trades_the_box_for_another_shape(flat):
    triangle = carbine.ui.new("triangle")
    # The chip is parked in a corner so that only the shape under test paints a pixel.
    chip = carbine.ui.new("square", name="Chip")
    try:
        triangle.position = (32, 32, 0)
        triangle.size = (40, 40, 1)
        triangle.color = "#00ff00"
        chip.position = (6, 6, 0)
        chip.size = (4, 4, 1)
        chip.color = "#eb7a34"

        assert chip.polygon == 4
        assert triangle.polygon == 3

        pixels = carbine.readPixels()
        assert pixel(pixels, 20, 32) == (0, 255, 0), "inside the point"
        assert pixel(pixels, 44, 32) == CLEAR, "past the flat edge"
        assert pixel(pixels, 32, 14) == CLEAR, "above the flat edge"

        chip.polygon = 64
        assert chip.polygon == 64
    finally:
        chip.destroy()
        triangle.destroy()


def test_a_polygon_must_be_a_whole_number_of_sides(flat):
    element = button()
    try:
        with pytest.raises(TypeError):
            element.polygon = 3.5
        assert element.polygon == 4

        element.polygon = 5
        assert element.polygon == 5
    finally:
        element.destroy()


def test_colour_accepts_hex_and_rejects_other_text(flat):
    element = button()
    try:
        element.color = "#0000ff"
        assert element.colorHex == "#0000ff"
        assert element.color.toTuple() == (0.0, 0.0, 1.0)

        with pytest.raises(ValueError, match="not a colour"):
            element.color = "reddish"

        element.color = (1.0, 0.0, 0.0)
        assert element.colorHex == "#ff0000"
    finally:
        element.destroy()


def test_world_objects_have_no_ui_to_configure(flat):
    card = carbine.new("rectangle")
    try:
        assert card.kind == "rectangle"
        with pytest.raises(ValueError, match="not a UI element"):
            card.corner = 8
        with pytest.raises(ValueError, match="not a UI element"):
            card.outline = 2
        with pytest.raises(ValueError, match="not a UI element"):
            card.polygon = 3

        # Transparency is the one overlay knob a world object does share.
        card.transparent = 100
        assert card.transparent == 100
    finally:
        card.destroy()


def test_the_two_layers_only_take_their_own_shapes(flat):
    with pytest.raises(ValueError, match="triangle"):
        carbine.new("triangle")
    with pytest.raises(ValueError, match="cube"):
        carbine.ui.new("cube")


def test_ui_draws_over_the_world(flat):
    card = carbine.new("rectangle")
    element = carbine.ui.new("square")
    try:
        card.size = (1, 1, 1)
        card.color = (1, 0, 0)
        element.position = (32, 32, 0)
        element.size = (16, 16, 1)
        element.color = "#0000ff"

        pixels = carbine.readPixels()
        assert pixel(pixels, 32, 32) == (0, 0, 255), "the overlay is on top"
        assert pixel(pixels, 20, 32) == (255, 0, 0), "the world is behind it"
    finally:
        element.destroy()
        card.destroy()


def test_a_branch_never_crosses_between_the_overlay_and_the_world(flat):
    cube = carbine.new("cube", name="Cube")
    panel = carbine.ui.new("rectangle", name="Panel")
    try:
        with pytest.raises(ValueError, match="branch"):
            carbine.ui.new("circle", name="Badge", parent=cube)
        with pytest.raises(ValueError, match="branch"):
            carbine.new("cube", name="Hidden", parent=panel)
        with pytest.raises(ValueError, match="branch"):
            cube.parent = panel
        with pytest.raises(ValueError, match="branch"):
            panel.addChild(cube)
        with pytest.raises(ValueError, match="branch"):
            panel.children = [cube]
        assert cube.parent == carbine.scene.root, "a refused write moved nothing"
        assert panel.children == []

        # Both kinds still hang off the root, and each keeps its own branch.
        badge = carbine.ui.new("circle", name="Badge", parent=panel)
        assert panel.children == [badge]
        badge.destroy()
    finally:
        panel.destroy()
        cube.destroy()


def test_the_child_written_last_is_the_one_painted_on_top(flat):
    panel = carbine.ui.new("rectangle", name="Panel")
    panel.position = (32, 32, 0)
    panel.size = (48, 48, 1)
    panel.color = "#000000"

    under = carbine.ui.new("square", name="Under", parent=panel)
    under.size = (32, 32, 1)
    under.color = "#ff0000"
    over = carbine.ui.new("square", name="Over", parent=panel)
    over.size = (32, 32, 1)
    over.color = "#00ff00"
    try:
        assert under.position.toTuple() == over.position.toTuple()
        assert pixel(carbine.readPixels(), 32, 32) == (0, 255, 0), "the later child sits on top"

        panel.children = [over, under]
        assert panel.children == [over, under]
        assert pixel(carbine.readPixels(), 32, 32) == (255, 0, 0), "so the list is the paint order"
    finally:
        panel.destroy()


def test_an_element_hangs_off_its_panel(flat):
    panel = carbine.ui.new("rectangle", name="Panel")
    try:
        panel.position = (16, 16, 0)
        panel.size = (32, 32, 1)
        panel.color = "#000000"

        badge = carbine.ui.new("circle", name="Badge", parent=panel)
        try:
            badge.position = (8, 8, 0)
            badge.size = (16, 16, 1)
            badge.color = "#ffffff"
            assert badge.polygon == 64

            pixels = carbine.readPixels()
            assert pixel(pixels, 24, 24) == (255, 255, 255), "the badge follows its panel"
            assert pixel(pixels, 6, 6) == (0, 0, 0), "the panel under it"
        finally:
            badge.destroy()
    finally:
        panel.destroy()


@pytest.fixture
def text():
    label = carbine.ui.new("Text", name="Label")
    try:
        yield label
    finally:
        label.destroy()


@pytest.mark.parametrize("kind", ["text", "Text", " TEXT ", "\tTeXt\n"])
def test_text_is_an_ordinary_object_with_scalar_size_and_boolean_outline(kind):
    label = carbine.ui.new(kind)
    try:
        assert type(label) is carbine.Object
        assert label.kind == "text"
        assert label.name == "Text"
        assert label.parent == carbine.scene.root
        assert label.content == ""
        assert type(label.size) is float
        assert label.size == 24
        assert label.wordWrap is False
        assert label.width == 400
        assert label.outline is False
        assert label.outlineWidth == 1
        assert label.transparent == 255
        assert label.anchored is True
    finally:
        label.destroy()


@pytest.mark.parametrize("content", ["", "FPS: 60", "Hello, 世界\nCafé\t42", "first\n\nlast\n"])
def test_text_content_round_trips_without_rewriting_whitespace(text, content):
    text.content = content
    assert text.content == content


def test_text_properties_round_trip_without_changing_transform(text):
    text.position = (12, 34, 0)
    text.scale = (2, 3, 1)
    position = text.position
    scale = text.scale
    text.content = "Measured FPS"
    text.size = 31.5
    text.wordWrap = True
    text.width = 125.5
    text.outline = True
    text.outlineWidth = 2.5
    text.transparent = 128
    assert text.content == "Measured FPS"
    assert text.size == 31.5
    assert text.wordWrap is True
    assert text.width == 125.5
    assert text.outline is True
    assert text.outlineWidth == 2.5
    assert text.transparent == 128
    assert position.toTuple() == (12, 34, 0)
    assert scale.toTuple() == (2, 3, 1)
    text.wordWrap = False
    text.outline = False
    assert text.wordWrap is False
    assert text.outline is False


@pytest.mark.parametrize(
    "property_name, value, expected",
    [
        ("size", -20, 1),
        ("size", 0, 1),
        ("size", 1, 1),
        ("size", 24.5, 24.5),
        ("size", 400, 400),
        ("size", 401, 400),
        ("size", 100000, 400),
        ("width", -10, 1),
        ("width", 0, 1),
        ("width", 1, 1),
        ("width", 400.5, 400.5),
        ("width", 16384, 16384),
        ("width", 100000, 16384),
        ("outlineWidth", -1, 0),
        ("outlineWidth", 0, 0),
        ("outlineWidth", 1.5, 1.5),
        ("outlineWidth", 32, 32),
        ("outlineWidth", 100000, 32),
        ("transparent", -10, 0),
        ("transparent", 300, 255),
    ],
)
def test_text_numeric_properties_clamp_to_their_bounds(text, property_name, value, expected):
    setattr(text, property_name, value)
    assert getattr(text, property_name) == expected


@pytest.mark.parametrize("property_name", ["size", "width", "outlineWidth", "transparent"])
@pytest.mark.parametrize("value", [float("nan"), float("inf"), -float("inf"), 1e100, -1e100])
def test_nonfinite_text_numbers_do_not_change_any_linked_member(text, property_name, value):
    twin, = carbine.linkDuplicate(text)
    try:
        before = getattr(text, property_name)
        with pytest.raises(ValueError, match="finite"):
            setattr(twin, property_name, value)
        assert getattr(text, property_name) == before
        assert getattr(twin, property_name) == before
    finally:
        twin.destroy()


@pytest.mark.parametrize(
    "property_name, value",
    [
        ("content", None),
        ("content", 42),
        ("size", (12, 12, 1)),
        ("size", carbine.Vec3(12, 12, 1)),
        ("size", "large"),
        ("size", None),
        ("width", (10, 20, 30)),
        ("width", "wide"),
        ("outlineWidth", None),
        ("outlineWidth", "thick"),
        ("wordWrap", 1),
        ("wordWrap", "yes"),
        ("wordWrap", None),
        ("outline", 1),
        ("outline", 2.5),
        ("outline", "yes"),
        ("outline", None),
    ],
)
def test_text_properties_reject_wrong_types_without_mutation(text, property_name, value):
    before = getattr(text, property_name)
    with pytest.raises(TypeError):
        setattr(text, property_name, value)
    assert getattr(text, property_name) == before


@pytest.mark.parametrize("factory, kind", [(carbine.ui.new, "square"), (carbine.new, "cube")])
def test_text_only_properties_refuse_shape_and_world_nodes(factory, kind):
    node = factory(kind)
    try:
        for property_name, value in [
            ("content", "text"), ("wordWrap", True), ("width", 100), ("outlineWidth", 2)
        ]:
            with pytest.raises(ValueError):
                getattr(node, property_name)
            with pytest.raises(ValueError):
                setattr(node, property_name, value)
    finally:
        node.destroy()


def test_destroyed_text_properties_raise_destroyed_error():
    label = carbine.ui.new("Text")
    label.destroy()
    for property_name, value in [
        ("content", "gone"), ("size", 20), ("wordWrap", True), ("width", 120),
        ("outline", True), ("outlineWidth", 2), ("transparent", 128),
    ]:
        with pytest.raises(carbine.DestroyedError):
            getattr(label, property_name)
        with pytest.raises(carbine.DestroyedError):
            setattr(label, property_name, value)


def test_text_colours_use_shared_names_hex_and_rgb_without_extra_nodes(text):
    before = carbine.scene.nodeCount
    text.color = "#1234"
    assert text.colorHex == "#112233"
    assert text.transparent == 68
    text.color = "  Light   Blue  "
    assert text.colorHex == "#add8e6"
    assert text.transparent == 68
    text.color = (0.5, 0.25, 1)
    assert text.color.toTuple() == (0.5, 0.25, 1)
    text.color = carbine.Vec3(1, 0, 0)
    assert text.colorHex == "#ff0000"
    with pytest.raises(ValueError, match="not a colour"):
        text.color = "reddish"
    assert text.colorHex == "#ff0000"
    assert carbine.scene.nodeCount == before


def test_text_duplicates_copy_configuration_and_subtree_independently(text):
    settings = {
        "content": "original\nlabel", "size": 37.5, "wordWrap": True,
        "width": 220, "outline": True, "outlineWidth": 2.5, "transparent": 100,
    }
    for property_name, value in settings.items():
        setattr(text, property_name, value)
    text.position = (10, 20, 0)
    text.scale = (2, 2, 1)
    child = carbine.ui.new("Text", name="Child", parent=text)
    child.content = "child"
    copy, = carbine.duplicate(text)
    try:
        assert type(copy) is carbine.Object
        assert copy != text
        assert copy.parent == text.parent
        assert copy.position.toTuple() == text.position.toTuple()
        assert copy.scale.toTuple() == text.scale.toTuple()
        for property_name, value in settings.items():
            assert getattr(copy, property_name) == value
        copied_child, = copy.children
        assert type(copied_child) is carbine.Object
        assert copied_child != child
        assert copied_child.parent == copy
        assert copy.find("Child") == copied_child
        assert copied_child.content == "child"
        copied_child.content = "independent"
        assert child.content == "child"
        copy.content = "copy"
        copy.size = 80
        assert text.content == "original\nlabel"
        assert text.size == 37.5
    finally:
        copy.destroy()


def test_linked_text_shares_font_configuration_but_not_placement(text):
    twin, = carbine.linkDuplicate(text)
    third, = carbine.linkDuplicate(twin)
    plain, = carbine.duplicate(twin)
    try:
        for property_name, value in {
            "content": "shared", "size": 42, "wordWrap": True, "width": 250,
            "outline": True, "outlineWidth": 3, "transparent": 100,
        }.items():
            setattr(twin, property_name, value)
            assert getattr(text, property_name) == value
            assert getattr(third, property_name) == value
        assert plain.content == ""
        assert plain.size == 24
        twin.position = (70, 80, 0)
        twin.scale = (2, 3, 1)
        twin.visible = False
        assert text.position.toTuple() == third.position.toTuple() == (0, 0, 0)
        assert text.scale.toTuple() == third.scale.toTuple() == (1, 1, 1)
        assert text.visible is third.visible is True
    finally:
        plain.destroy()
        third.destroy()
        twin.destroy()


def test_text_hierarchy_stays_in_the_overlay_and_destroy_follows_the_branch():
    panel = carbine.ui.new("rectangle", name="Panel")
    other = carbine.ui.new("square")
    cube = carbine.new("cube")
    try:
        label = carbine.ui.new("Text", name="Caption", parent=panel)
        nested = carbine.ui.new("Text", name="Nested", parent=label)
        assert panel.children == [label]
        assert label.children == [nested]
        assert label.parent == panel
        assert panel.find("Nested") == nested
        before = carbine.scene.nodeCount
        with pytest.raises(ValueError, match="branch"):
            carbine.ui.new("Text", parent=cube)
        assert carbine.scene.nodeCount == before
        with pytest.raises(ValueError, match="branch"):
            label.parent = cube
        with pytest.raises(ValueError, match="branch"):
            label.addChild(cube)
        assert label.parent == panel
        with pytest.raises(ValueError, match="cycle"):
            label.parent = nested
        other.addChild(label)
        assert panel.children == []
        assert other.children == [label]
        other.removeChild(label)
        assert label.parent == carbine.scene.root
        panel.children = [label]
        panel.destroy()
        with pytest.raises(carbine.DestroyedError):
            _ = label.content
        with pytest.raises(carbine.DestroyedError):
            _ = nested.size
    finally:
        if panel in carbine.scene.root.children:
            panel.destroy()
        other.destroy()
        cube.destroy()


@pytest.mark.parametrize("factory, kind", [(carbine.ui.new, "square"), (carbine.new, "cube")])
def test_nontext_size_remains_a_live_vector_with_partial_assignment(factory, kind):
    node = factory(kind)
    try:
        size = node.size
        assert type(size) is carbine.Vec3
        node.size = (20, 30, 40)
        assert size.toTuple() == (20, 30, 40)
        size.x = 50
        assert node.size.toTuple() == (50, 30, 40)
        node.size = (..., None, 60)
        assert size.toTuple() == (50, 30, 60)
        node.size = carbine.Vec3(4, 5, 6)
        assert size.toTuple() == (4, 5, 6)
        with pytest.raises(TypeError):
            node.size = 24
        assert size.toTuple() == (4, 5, 6)
    finally:
        node.destroy()


def test_shape_outline_remains_float_thickness_and_is_not_a_text_toggle():
    shape = carbine.ui.new("square")
    try:
        assert type(shape.outline) is float
        for thickness in (0, 3.5, -2, 500):
            shape.outline = thickness
            assert type(shape.outline) is float
            assert shape.outline == thickness
        with pytest.raises(TypeError):
            shape.outline = "thick"
        assert shape.outline == 500
    finally:
        shape.destroy()

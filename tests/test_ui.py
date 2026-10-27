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

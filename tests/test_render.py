import os

import carbine
import pytest

WIDTH = 64
HEIGHT = 64

# The camera shows two metres of world across these pixels, so one metre is 32
# pixels and the origin lands dead centre.
CLEAR = (22, 21, 20)


def pixel(data, x, y):
    offset = (y * WIDTH + x) * 4
    return tuple(data[offset : offset + 3])


@pytest.fixture
def headless():
    try:
        adapter = carbine.init(WIDTH, HEIGHT, window=False)
    except RuntimeError as error:
        pytest.skip(f"no GPU renderer available: {error}")
    try:
        yield adapter
    finally:
        carbine.close()
        carbine.camera().viewHeight = 2.0


def test_a_card_lands_where_the_camera_points(headless):
    card = carbine.new("rectangle", name="Card")
    try:
        card.size = (0.5, 0.5, 1.0)
        card.position = (0.5, 0.0, 0.0)
        card.color = (1.0, 0.0, 0.0)

        pixels = carbine.readPixels()

        assert len(pixels) == WIDTH * HEIGHT * 4
        assert pixel(pixels, 52, 32) == (255, 0, 0)
        assert pixel(pixels, 4, 32) == CLEAR
    finally:
        card.destroy()


def test_an_empty_scene_is_only_the_clear_colour(headless):
    pixels = carbine.readPixels()
    assert pixel(pixels, 32, 32) == CLEAR
    assert pixel(pixels, 0, 0) == CLEAR


def test_a_child_moves_with_its_parent(headless):
    pedestal = carbine.new("rectangle", name="Pedestal")
    try:
        rider = carbine.new("rectangle", parent=pedestal, name="Rider")
        try:
            rider.size = (0.5, 0.5, 1.0)
            rider.position = (0.5, 0.0, 0.0)
            rider.color = (0.0, 1.0, 0.0)

            assert pixel(carbine.readPixels(), 52, 32) == (0, 255, 0)

            pedestal.position = (-1.0, 0.0, 0.0)
            pixels = carbine.readPixels()
            assert pixel(pixels, 20, 32) == (0, 255, 0)
            assert pixel(pixels, 52, 32) == CLEAR
        finally:
            rider.destroy()
    finally:
        pedestal.destroy()


def test_view_height_zooms_out(headless):
    card = carbine.new("rectangle")
    try:
        card.size = (0.5, 0.5, 1.0)
        card.position = (0.5, 0.0, 0.0)
        card.color = (1.0, 0.0, 0.0)

        assert pixel(carbine.readPixels(), 52, 32) == (255, 0, 0)

        carbine.camera().viewHeight = 8.0
        pixels = carbine.readPixels()
        assert pixel(pixels, 52, 32) == CLEAR
        assert pixel(pixels, 36, 32) == (255, 0, 0)
    finally:
        card.destroy()


def test_an_entity_draws_its_body(headless):
    actor = carbine.spawnEntity("rectangle", position=(0.5, 0.0, 0.0))
    try:
        actor.size = (0.5, 0.5, 1.0)
        actor.color = (0.0, 0.0, 1.0)
        assert pixel(carbine.readPixels(), 52, 32) == (0, 0, 255)

        carbine.killEntity(actor)
        assert pixel(carbine.readPixels(), 52, 32) == (0, 0, 255)
    finally:
        actor.destroy()


def test_viewport_and_running(headless):
    assert carbine.viewport() == (WIDTH, HEIGHT)
    assert headless
    assert carbine.running() is True


def test_a_real_window_renders():
    if not (os.environ.get("WAYLAND_DISPLAY") or os.environ.get("DISPLAY")):
        pytest.skip("no compositor to open a window on")
    try:
        adapter = carbine.init(320, 240, title="Carbine test window")
    except RuntimeError as error:
        pytest.skip(f"cannot open a window here: {error}")
    try:
        assert adapter
        assert carbine.running() is True
        card = carbine.new("rectangle")
        try:
            carbine.render()
        finally:
            card.destroy()
        width, height = carbine.viewport()
        assert width >= 320 and height >= 240
    finally:
        carbine.close()
    assert carbine.running() is False


def test_the_renderer_is_off_until_init():
    assert carbine.running() is False
    with pytest.raises(RuntimeError, match="carbine.init"):
        carbine.render()
    with pytest.raises(RuntimeError, match="carbine.init"):
        carbine.readPixels()

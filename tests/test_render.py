import os
import time

import carbine
import pytest

WIDTH = 64
HEIGHT = 64

CLEAR = (22, 21, 20)


def pixel(data, x, y, width=WIDTH):
    offset = (y * width + x) * 4
    return tuple(data[offset : offset + 3])


def near(found, want, tolerance=6):
    """Whether a pixel is the colour it was asked for, allowing for the couple of
    steps BC7 spends and for whatever the light in the scene decided to do to it."""
    return all(abs(a - b) <= tolerance for a, b in zip(found, want))


def painted(data):
    """Every pixel brighter than the clear colour, that is everything the world
    objects actually painted."""
    return [tuple(data[i : i + 3]) for i in range(0, len(data), 4) if data[i] > 40]


def aim(mode, position, target):
    camera = carbine.camera()
    camera.mode = mode
    camera.position = position
    camera.lookAt(target)


def test_the_scene_opens_on_a_3d_world(headless):
    camera = carbine.camera()
    sky = carbine.atmosphere.sky()

    assert camera.mode == "perspective", "a new scene renders in depth"
    assert camera.position.z > 0.0, "the world is z-up, so the camera floats above it"
    assert sky.enabled is True
    assert sky.zenith.z > sky.zenith.x, "the default backdrop is blue overhead"
    assert "cube" in carbine.kinds()


def test_a_card_lands_where_the_camera_points(flat):
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


def test_an_invisible_object_leaves_the_frame(flat):
    card = carbine.new("rectangle", name="Card")
    rider = carbine.new("rectangle", name="Rider", parent=card)
    try:
        card.size = (0.5, 0.5, 1.0)
        card.position = (0.5, 0.0, 0.0)
        card.color = (1.0, 0.0, 0.0)
        # A child is measured in the parent's size, so this one stands two of the
        # card's heights above its middle.
        rider.size = (0.2, 0.2, 1.0)
        rider.position = (0.0, 0.0, 2.0)
        rider.color = (0.0, 1.0, 0.0)

        assert pixel(carbine.readPixels(), 52, 32) == (255, 0, 0)
        assert pixel(carbine.readPixels(), 48, 32) == (0, 255, 0)

        card.visible = False
        pixels = carbine.readPixels()
        assert pixel(pixels, 52, 32) == CLEAR
        assert pixel(pixels, 48, 32) == CLEAR, "hiding a parent hides its children"

        card.visible = True
        pixels = carbine.readPixels()
        assert pixel(pixels, 52, 32) == (255, 0, 0)
        assert pixel(pixels, 48, 32) == (0, 255, 0)
    finally:
        rider.destroy()
        card.destroy()


def test_an_empty_scene_is_only_the_clear_colour(flat):
    pixels = carbine.readPixels()
    assert pixel(pixels, 32, 32) == CLEAR
    assert pixel(pixels, 0, 0) == CLEAR


def test_a_child_moves_with_its_parent(flat):
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


def test_view_height_zooms_out(flat):
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


def test_an_entity_draws_its_body(flat):
    actor = carbine.spawnEntity("rectangle", position=(0.5, 0.0, 0.0))
    try:
        actor.size = (0.5, 0.5, 1.0)
        actor.color = (0.0, 0.0, 1.0)
        assert pixel(carbine.readPixels(), 52, 32) == (0, 0, 255)

        carbine.killEntity(actor)
        assert pixel(carbine.readPixels(), 52, 32) == (0, 0, 255)
    finally:
        actor.destroy()


def test_the_nearer_cube_hides_the_one_behind_it(flat):
    # Drawn first, so only a real depth buffer can keep the second cube from
    # painting over it.
    near = carbine.new("cube", name="Near")
    far = carbine.new("cube", name="Far")
    try:
        near.size = (1, 1, 1)
        near.position = (0, 0, 1)
        near.color = (1, 0, 0)
        far.size = (1, 1, 1)
        far.position = (0, 0, 0)
        far.color = (0, 1, 0)

        assert pixel(carbine.readPixels(), 32, 32) == (255, 0, 0)
    finally:
        far.destroy()
        near.destroy()


def test_a_cube_shows_a_lit_top_and_a_wall_in_ambient(headless):
    carbine.atmosphere.sky().enabled = False
    carbine.atmosphere.sky().sun = (0.0, 0.0, 1.0)
    aim("perspective", (0.0, -3.0, 3.0), (0.0, 0.0, 0.5))

    cube = carbine.new("cube")
    try:
        cube.color = (1.0, 0.0, 0.0)
        reds = [rgb[0] for rgb in painted(carbine.readPixels()) if rgb[0] > rgb[1] + 20]

        # The face pointing into the light keeps the full colour; a wall facing
        # away from it falls to the scene's ambient level.
        assert max(reds) > 250, reds
        assert 70 <= min(reds) <= 82, min(reds)
    finally:
        cube.destroy()


def test_perspective_shrinks_what_is_farther(headless):
    carbine.atmosphere.sky().enabled = False
    aim("perspective", (0.0, -4.0, 0.5), (0.0, 0.0, 0.0))

    cube = carbine.new("cube")
    try:
        cube.size = (1, 1, 1)
        cube.color = (1, 0, 0)
        cube.position = (0, 0, 0)
        near = len(painted(carbine.readPixels()))

        cube.position = (0, 6.0, 0)
        far = len(painted(carbine.readPixels()))

        assert near > far * 2, f"near covered {near}, far {far}"
    finally:
        cube.destroy()


def test_an_orthographic_camera_keeps_the_same_size_at_any_distance(headless):
    carbine.atmosphere.sky().enabled = False
    carbine.atmosphere.sky().sun = (0.0, 0.0, 1.0)
    # Straight down the y axis, so sliding the cube along y only changes how far
    # away it is.
    aim("orthographic", (0.0, -4.0, 0.0), (0.0, 4.0, 0.0))
    carbine.camera().viewHeight = 4.0

    cube = carbine.new("cube")
    try:
        cube.size = (1, 1, 1)
        cube.color = (1, 0, 0)
        cube.position = (0, 0, 0)
        near = len(painted(carbine.readPixels()))

        cube.position = (0, 3.0, 0)
        far = len(painted(carbine.readPixels()))

        assert near == far and near > 0, f"{near} then {far}"
    finally:
        cube.destroy()


def test_the_sky_graduates_from_horizon_to_zenith(headless):
    sky = carbine.atmosphere.sky()
    sky.enabled = True
    sky.noise = 0.0
    sky.colors("#0000ff", "#ffffff")
    aim("perspective", (0.0, -4.0, 0.0), (0.0, 4.0, 0.0))

    pixels = carbine.readPixels()
    # Looking along the ground: the top of the frame is sky, the bottom is under
    # the horizon and stays at the horizon colour.
    assert pixel(pixels, 32, 2)[0] < 120, pixel(pixels, 32, 2)
    assert pixel(pixels, 32, 62) == (255, 255, 255)


def test_grain_roughens_the_backdrop_without_hiding_it(headless):
    sky = carbine.atmosphere.sky()
    sky.enabled = True
    sky.colors("#0000ff", "#0000ff")
    aim("perspective", (0.0, -4.0, 1.0), (0.0, 4.0, 1.0))

    def backdrop():
        pixels = carbine.readPixels()
        return {pixel(pixels, x, y) for x in range(0, WIDTH, 4) for y in range(0, HEIGHT, 4)}

    sky.noise = 0.0
    smooth = backdrop()
    sky.noise = 0.6
    grainy = backdrop()

    assert len(smooth) == 1, smooth
    assert len(grainy) > 4, grainy


def test_a_texture_paints_the_object(flat, skin):
    tile = carbine.new("plane")
    try:
        tile.size = (1, 1, 1)
        tile.color = (1.0, 1.0, 1.0)
        tile.texture = skin
        assert tile.texture == skin

        # A packed skin is BC7, which spends a couple of steps even on a flat block,
        # so the ask is the colour the picture carries, not the byte it was drawn with.
        pixels = carbine.readPixels()
        assert near(pixel(pixels, 24, 24), (255, 0, 0)), "top left"
        assert near(pixel(pixels, 40, 24), (0, 255, 0)), "top right"
        assert near(pixel(pixels, 24, 40), (0, 0, 255)), "bottom left"
        assert near(pixel(pixels, 40, 40), (255, 255, 255)), "bottom right"

        tile.texture = None
        assert tile.texture is None
        assert near(pixel(carbine.readPixels(), 24, 24), (255, 255, 255))
    finally:
        tile.destroy()


def test_a_skin_shown_from_far_still_wears_its_picture(authored, flat):
    """The whole mip chain goes up with the picture, not just the first level: a skin
    small on screen is sampled from a reduced level, so a flat red image has to come
    back red from a distance instead of the black of a level nobody filled."""
    packed = carbine.packager.image(authored("red.bmp", [[(255, 0, 0)] * 8 for _ in range(8)]))
    tile = carbine.new("plane")
    camera = carbine.camera()
    try:
        tile.size = (1, 1, 1)
        tile.color = (1.0, 1.0, 1.0)
        tile.texture = packed

        camera.mode = "perspective"
        camera.position = (0.0, 0.0, 24.0)
        camera.target = (0.0, 0.0, 0.0)
        pixels = carbine.readPixels()

        window = [pixel(pixels, 32 + dx, 32 + dy) for dx in range(-3, 4) for dy in range(-3, 4)]
        brightest = max(window, key=sum)
        assert brightest[0] > 150 and brightest[1] < 60 and brightest[2] < 60, (
            f"a two pixel skin reads its own picture, not an unfilled level: {window}"
        )
    finally:
        tile.destroy()


def test_a_translucent_object_blends_over_what_is_behind_it(flat):
    floor = carbine.new("plane")
    veil = carbine.new("plane")
    try:
        floor.size = (1, 1, 1)
        floor.color = (1, 0, 0)
        veil.size = (1, 1, 1)
        veil.position = (0, 0, 1)
        veil.color = (0, 1, 0)
        veil.transparent = 128

        blended = pixel(carbine.readPixels(), 32, 32)
        assert 122 <= blended[0] <= 132, blended
        assert 122 <= blended[1] <= 132, blended
        assert blended[2] < 4, blended
    finally:
        veil.destroy()
        floor.destroy()


def test_camera_numbers_stay_sane(headless):
    camera = carbine.camera()
    camera.fov = 0.0
    assert camera.fov == 1.0
    camera.near = -1.0
    assert camera.near > 0.0

    with pytest.raises(ValueError, match="not a camera mode"):
        camera.mode = "fisheye"


def test_viewport_and_running(flat):
    assert carbine.viewport() == (WIDTH, HEIGHT)
    assert flat
    assert carbine.running() is True


def test_a_real_window_renders_and_holds_the_lower_limit():
    """One window per process is all the desktop hands out, so this is where both
    stories get told: a scene drawn to a real screen, and the frame limit rule
    measured against that screen's actual refresh rate.

    The window opens holding nothing — no cap, no sync — and the panel's rate is only a
    limit once sync is asked for. A cap the panel cannot reach then lets the panel win;
    a cap below it is what the frames hold. See `tests/test_pacing.py` for everything
    the limits do with no window to ask."""
    if not (os.environ.get("WAYLAND_DISPLAY") or os.environ.get("DISPLAY")):
        pytest.skip("no compositor to open a window on")
    try:
        adapter = carbine.init(320, 240, title="Carbine test window")
    except RuntimeError as error:
        pytest.skip(f"cannot open a window here: {error}")
    try:
        assert adapter
        assert carbine.running() is True
        assert (carbine.fpsCap, carbine.monitorSync) == (
            None,
            False,
        ), "a window starts held by nothing"
        card = carbine.new("cube")
        try:
            carbine.render()
        finally:
            card.destroy()
        width, height = carbine.viewport()
        assert width >= 320 and height >= 240

        for _ in range(4):
            carbine.render()
        hz = carbine.monitorHz
        if hz == 0:
            pytest.skip("this compositor will not name its refresh rate")

        carbine.fpsCap = hz * 4
        assert carbine.fpsLimit == hz * 4, "unsynced, so only the cap holds"

        carbine.monitorSync = True
        assert carbine.fpsLimit == hz, "synced, and the screen is the lower limit"

        carbine.fpsCap = 15
        limit = carbine.fpsLimit
        assert limit and 0 < limit <= 15, "the cap holds on a screen that could show more"

        start = time.perf_counter()
        for _ in range(10):
            carbine.render()
        took = time.perf_counter() - start
        assert took >= 8 / limit, f"{limit} fps let ten frames through in {took:.3f}s"

        carbine.monitorSync = False
        carbine.fpsCap = None
    finally:
        carbine.close()
    assert carbine.running() is False


def test_the_renderer_is_off_until_init():
    assert carbine.running() is False
    with pytest.raises(RuntimeError, match="carbine.init"):
        carbine.render()
    with pytest.raises(RuntimeError, match="carbine.init"):
        carbine.readPixels()

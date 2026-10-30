"""The player controller: what the keyboard and mouse do to the camera.

These drive the scene with injected input and `carbine.step`, so no window is
needed until the last tests, which check that a real frame folds the input in.
"""

import time

import carbine
import pytest

WIDTH = 64
HEIGHT = 64


def walk_forward(paces=60, second=1.0 / 60.0):
    carbine.injectKey("w", True)
    for _ in range(paces):
        carbine.step(second)


# The scene is one per process, so a key a test left down would still be down in
# the next one, and a jump would fire on its own.
HANDS = ("w", "a", "s", "d", "space", "escape")


def let_go():
    for name in HANDS:
        carbine.injectKey(name, False)
    carbine.injectButton("left", False)
    carbine.injectButton("right", False)


@pytest.fixture
def walker():
    """An enabled player on an empty world, put back the way the engine started."""
    player = carbine.player()
    let_go()
    player.enabled = False
    player.body = None
    player.mode = "first"
    player.position = (0.0, 0.0, 0.0)
    player.yaw = 0.0
    player.pitch = 0.0
    player.speed = 5.0
    player.sensitivity = 0.12
    player.eyeHeight = 1.6
    player.distance = 4.0
    player.gravityPull = 10.0
    player.jumpStrength = 5.0
    player.enabled = True
    yield player
    let_go()
    player.enabled = False
    player.body = None
    player.position = (0.0, 0.0, 0.0)
    player.yaw = 0.0
    player.pitch = 0.0


def pace_of(player):
    velocity = player.velocity
    return (velocity.x**2 + velocity.y**2) ** 0.5


def test_the_keyboard_walks_the_player_forward(walker):
    walker.speed = 6.0
    walk_forward()

    where = walker.position
    # One second at six metres a second, less the short ramp up to speed.
    assert 5.4 <= where.x <= 6.1, where
    assert where.y == pytest.approx(0.0, abs=0.01)
    assert where.z == 0.0
    assert walker.grounded


def test_turning_around_walks_the_other_way(walker):
    walker.speed = 6.0
    walker.yaw = 90.0
    walk_forward()

    where = walker.position
    assert where.x == pytest.approx(0.0, abs=0.01), where
    assert 5.4 <= where.y <= 6.1, where


def test_letting_go_of_a_key_slides_to_a_stop(walker):
    walk_forward(30)
    carbine.injectKey("w", False)
    for _ in range(120):
        carbine.step(1.0 / 60.0)

    assert pace_of(walker) < 0.02


def test_a_teleport_arrives_at_rest(walker):
    walk_forward()
    assert walker.velocity.x > 4.0

    walker.position = (0.0, 20.0, 0.0)
    still = walker.velocity
    assert (still.x, still.y, still.z) == (0.0, 0.0, 0.0), "no momentum carried in"
    assert walker.grounded, "and it lands where it was put"


def test_a_strafe_moves_sideways_not_forward(walker):
    carbine.injectKey("d", True)
    for _ in range(60):
        carbine.step(1.0 / 60.0)

    where = walker.position
    assert where.y < -3.5, "right of the way the player faces is -y"
    assert abs(where.x) < 0.05, "a strafe does not walk forward"


def test_the_mouse_turns_the_player_and_the_pitch_refuses_to_flip(walker):
    walker.sensitivity = 0.2
    carbine.injectMouse(50.0, 0.0)
    assert walker.yaw == pytest.approx(350.0, abs=1e-4), "mouse right turns right"

    for _ in range(20):
        carbine.injectMouse(0.0, -100.0)
    assert walker.pitch == 89.0

    for _ in range(40):
        carbine.injectMouse(0.0, 100.0)
    assert walker.pitch == -89.0


def test_the_camera_rides_the_player(walker):
    carbine.step(0.0)
    camera = carbine.camera()
    assert camera.position.z == pytest.approx(1.6, abs=1e-4)
    offset = camera.target - camera.position
    distance = (offset.x**2 + offset.y**2 + offset.z**2) ** 0.5
    assert distance == pytest.approx(1.0, abs=1e-4), "the lens looks where the head does"

    walk_forward(40)
    assert camera.position.x > 2.5, "the eyes go where the feet go"


def test_a_step_leaves_a_camera_the_script_owns_alone():
    """No player, so no eyes to put the lens back on: the camera a script placed is
    its own, and stepping the world must not take it away."""
    player = carbine.player()
    player.enabled = False
    let_go()

    camera = carbine.camera()
    camera.position = (0.0, -4.0, 1.0)
    camera.target = (0.0, 0.0, 1.0)
    for _ in range(30):
        carbine.step(1.0 / 60.0)

    assert tuple(camera.position) == (0.0, -4.0, 1.0)
    assert tuple(camera.target) == (0.0, 0.0, 1.0)


def test_third_person_parks_a_camera_behind_the_body(walker):
    walker.mode = "third"
    walker.distance = 4.0
    carbine.step(0.0)

    camera = carbine.camera()
    assert camera.position.x == pytest.approx(-4.0, abs=1e-3)
    assert camera.target.x == pytest.approx(0.0, abs=1e-3)
    assert camera.position.z > 0.0


def test_a_camera_looking_straight_down_stays_above_the_floor(walker):
    walker.mode = "third"
    walker.pitch = -89.0
    carbine.step(0.0)

    assert carbine.camera().position.z > 0.0


def test_the_mode_only_accepts_the_two_names(walker):
    for name, written in [("first", "first"), ("third", "third"), ("fps", "first")]:
        walker.mode = name
        assert walker.mode == written

    with pytest.raises(ValueError):
        walker.mode = "side"


def test_the_body_is_worn_and_follows_the_walk(walker):
    body = carbine.new("cube", name="Suit")
    try:
        body.size = (0.6, 0.6, 1.8)
        walker.body = body
        assert walker.body == body

        # The eyes are inside the body, so nothing of it should be drawn.
        assert body.visible is False

        walker.mode = "third"
        assert body.visible is True

        walk_forward(60)
        placed = body.position
        assert placed.x > 4.0, placed
        assert placed.z == pytest.approx(0.9, abs=1e-3), "a head stands on its bottom face"
    finally:
        walker.body = None
        body.destroy()


def test_a_ui_element_refuses_to_be_a_body(walker):
    button = carbine.ui.new("square", name="Button")
    try:
        with pytest.raises(ValueError):
            walker.body = button
    finally:
        button.destroy()


def test_no_body_is_worn_after_a_clear(walker):
    body = carbine.new("cube", name="Suit")
    walker.body = body
    assert walker.body == body
    walker.body = None
    assert walker.body is None
    body.destroy()


def test_space_jumps_and_gravity_lands_the_player(walker):
    carbine.injectKey("space", True)
    carbine.step(0.1)
    assert walker.grounded is False
    assert walker.position.z > 0.3, "one tenth of a jump"

    for _ in range(150):
        carbine.step(1.0 / 60.0)
    assert walker.position.z == 0.0
    assert walker.grounded is True


def test_there_is_no_second_jump_in_mid_air(walker):
    walker.position = (0.0, 0.0, 4.0)
    carbine.injectKey("space", True)
    carbine.step(0.1)

    assert walker.velocity.z <= 0.0, "no ground to push off"


def test_jump_strength_and_gravity_pull_are_the_players_own(walker):
    walker.gravityPull = 1.6
    walker.jumpStrength = 8.0
    carbine.injectKey("space", True)
    carbine.step(0.1)

    assert walker.position.z == pytest.approx(0.78, abs=0.05)
    assert walker.velocity.z == pytest.approx(7.8, abs=0.2), "the moon is slower"

    walker.gravityPull = 10.0
    carbine.step(0.1)
    assert walker.velocity.z == pytest.approx(6.84, abs=0.2), "Earth pulls harder"


def test_escape_hands_the_mouse_back_and_a_click_asks_again(walker):
    assert walker.grabMouse is True

    carbine.injectKey("escape", True)
    assert walker.grabMouse is False

    carbine.injectButton("left", True)
    assert walker.grabMouse is True


def test_the_raw_input_is_readable_too(walker):
    carbine.injectKey("e", True)
    assert carbine.key("e") is True
    assert carbine.pressed("e") is True
    assert carbine.released("e") is False

    # The edge belongs to the frame that carried it; a later frame only knows the
    # key is still down.
    carbine.injectKey("r", False)
    assert carbine.key("e") is True
    assert carbine.pressed("e") is False

    carbine.injectButton("left", True)
    assert carbine.mouse().left is True

    carbine.injectMouse(6.0, -2.0)
    mouse = carbine.mouse()
    assert (mouse.dx, mouse.dy) == (6.0, -2.0)
    assert mouse.grabbed is False, "nothing is holding the pointer without a window"


def test_a_key_is_named_by_where_it_sits():
    assert "w" in carbine.keys()
    assert "space" in carbine.keys()
    with pytest.raises(ValueError):
        carbine.key("f1")


def test_grabbing_the_pointer_needs_a_player_to_look_with():
    carbine.player().enabled = False
    assert carbine.grabMouse(True) is False
    assert carbine.player().grabMouse is False


def test_a_step_cannot_be_bigger_than_a_frame(walker):
    walker.position = (0.0, 0.0, 20.0)
    carbine.step(1000.0)

    assert walker.position.z > 9.0, "one clamped step, not a thousand seconds of fall"
    assert walker.position.z < 20.0


def test_the_solver_drops_a_loose_crate(headless):
    crate = carbine.new("cube", name="Crate")
    try:
        crate.size = (1.0, 1.0, 1.0)
        crate.position = (0.0, 0.0, 5.0)
        crate.gravity = True

        for _ in range(120):
            carbine.step(1.0 / 60.0)

        assert crate.position.z == pytest.approx(0.5, abs=1e-3)
        assert crate.position.x == pytest.approx(0.0, abs=1e-6)

        crate.gravity = False
        crate.anchored = True
        crate.position = (0.0, 0.0, 3.0)
        carbine.step(1.0)
        assert crate.position.z == 3.0, "an anchored node stays where it was left"
    finally:
        crate.destroy()


def test_render_folds_the_keyboard_in_and_walks(walker):
    walker.speed = 5.0
    walker.mode = "first"
    carbine.injectKey("w", True)

    try:
        carbine.init(WIDTH, HEIGHT, window=False)
        for _ in range(3):
            carbine.render()
            time.sleep(0.05)
    finally:
        carbine.close()

    assert carbine.dt() > 0.0, "a frame measured its own length"
    assert walker.position.x > 0.3, walker.position
    assert carbine.camera().position.x > 0.3, "the eyes went with the feet"

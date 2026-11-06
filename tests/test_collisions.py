"""Colliders: the bodies a script put in the solver, and what the solver does.

Everything here runs through `carbine.step`, and the scene is one per process, so
each test destroys what it made and leaves no player walking about.
"""

import carbine
import pytest

SECOND = 1.0 / 60.0


def settle(seconds=5.0):
    for _ in range(round(seconds / SECOND)):
        carbine.step(SECOND)


def scenery(*, at, size, kind="cube"):
    """A body the world holds still: it collides and nothing pushes it anywhere."""
    table = carbine.new(kind)
    table.size = size
    table.position = at
    table.collision = True
    return table


def crate(*, at, collide=True, falls=True):
    """A one metre cube with its collider switched on and gravity owning it."""
    box = carbine.new("cube")
    box.size = (1.0, 1.0, 1.0)
    box.position = at
    box.collision = collide
    box.gravity = falls
    return box


@pytest.fixture(autouse=True)
def no_player():
    """A player in the scene would be shoved by the same colliders these tests
    are checking, and would shove back."""
    player = carbine.player()
    player.enabled = False
    player.body = None
    yield player
    player.enabled = False
    player.body = None


def test_nothing_collides_until_a_script_says_so():
    box = carbine.new("cube")
    try:
        assert box.collision is False
        assert box.collisionType == "box", "the type the body would be cut as"

        box.collision = True
        assert box.collision is True
        box.collision = False
        assert box.collision is False
        assert box.collisionType == "box", "switching off keeps the type"
    finally:
        box.destroy()


def test_a_type_is_cut_from_the_body_it_was_given():
    ball = carbine.new("sphere")
    try:
        assert ball.collisionType == "sphere", "a ball reads as round"

        for name in carbine.collisionTypes():
            ball.collisionType = name
            assert ball.collisionType == name
            assert ball.collision is True, "writing a type puts the body in the solver"
    finally:
        ball.destroy()


def test_a_type_is_written_the_way_a_script_types_it():
    box = carbine.new("cube")
    try:
        box.collisionType = "  Ball "
        assert box.collisionType == "sphere"

        with pytest.raises(ValueError, match="box, sphere, mesh"):
            box.collisionType = "octahedron"
    finally:
        box.destroy()


def test_carbine_lists_the_types_it_accepts():
    assert carbine.collisionTypes() == ["box", "sphere", "mesh"]


def test_a_ui_element_stays_out_the_solver():
    button = carbine.ui.new("square")
    try:
        with pytest.raises(ValueError, match="UI element"):
            button.collision = True
    finally:
        button.destroy()


def test_a_body_that_never_asked_falls_straight_through():
    table = scenery(at=(0.0, 0.0, 1.0), size=(2.0, 2.0, 0.2))
    ghost = crate(at=(0.0, 0.0, 3.0), collide=False)
    settle()

    assert ghost.position.z == pytest.approx(0.5, abs=1e-3), "a ghost reaches the floor"
    ghost.destroy()
    table.destroy()


def test_a_crate_lands_on_a_table_and_names_it():
    table = scenery(at=(0.0, 0.0, 1.0), size=(2.0, 2.0, 0.2))
    box = crate(at=(0.0, 0.0, 3.0))
    settle()

    # The table's top face is 1.1 up, so a one metre cube rests half a metre on.
    assert box.position.z == pytest.approx(1.6, abs=1e-3), box.position
    assert box.touching() == [table]
    assert type(box.touching()[0]) is carbine.Object, "scenery comes back as scenery"
    assert table.touching() == [box]

    box.destroy()
    table.destroy()


def test_two_crates_settle_one_on_top_of_the_other():
    lower = crate(at=(0.0, 0.0, 1.0))
    upper = crate(at=(0.0, 0.0, 2.2))
    settle(7.0)

    assert lower.position.z == pytest.approx(0.5, abs=1e-2), lower.position
    assert upper.position.z == pytest.approx(1.5, abs=1e-2), upper.position
    assert upper.touching() == [lower]

    upper.destroy()
    lower.destroy()


def test_an_anvil_barely_gives_way_to_a_bolt():
    anvil = crate(at=(0.0, 0.0, 5.0))
    bolt = crate(at=(0.8, 0.0, 5.0))
    anvil.mass = 2000.0
    bolt.mass = 0.05

    # Twenty centimetres the two boxes share, handed out between them.
    carbine.step(SECOND)

    assert bolt.position.x - 0.8 > 0.19, "the bolt takes nearly all of it"
    assert abs(anvil.position.x) < 1e-3, "and the anvil the rest"

    bolt.destroy()
    anvil.destroy()


def test_weight_does_not_change_how_fast_a_body_falls():
    anvil = crate(at=(0.0, 0.0, 8.0))
    bolt = crate(at=(4.0, 0.0, 8.0))
    anvil.mass = 2000.0
    bolt.mass = 0.05
    settle(1.0)

    assert anvil.position.z == pytest.approx(bolt.position.z, abs=1e-6)

    bolt.destroy()
    anvil.destroy()


def test_touching_hands_back_an_entity_for_an_actor():
    walker = carbine.spawnEntity("cube", name="Walker", position=(0.0, 0.0, 1.0))
    pushed = carbine.spawnEntity("cube", name="Pushed", position=(0.0, 0.0, 3.0))
    for actor in (walker, pushed):
        actor.collision = True
        actor.gravity = True
    settle(7.0)

    try:
        assert pushed.touching() == [walker]
        assert type(pushed.touching()[0]) is carbine.Entity, "an actor comes back an actor"
    finally:
        pushed.destroy()
        walker.destroy()


def test_touching_is_empty_until_something_has_met_it():
    box = crate(at=(0.0, 0.0, 20.0), collide=True, falls=False)
    try:
        assert box.touching() == []
    finally:
        box.destroy()


def test_a_destroyed_body_leaves_the_solver():
    table = scenery(at=(0.0, 0.0, 1.0), size=(2.0, 2.0, 0.2))
    box = crate(at=(0.0, 0.0, 3.0))
    settle()
    assert box.touching() == [table]

    table.destroy()
    settle(1.0)
    assert box.position.z == pytest.approx(0.5, abs=1e-2), "the crate finishes the drop"
    assert box.touching() == []
    box.destroy()

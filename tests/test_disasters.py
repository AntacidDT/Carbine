"""Disasters: `carbine.physics.new("earthquake")`.

A disaster is an acceleration the solver asks about every step, not a body, so it
has no place in the tree and nothing to draw. It is written switched off and
`start()` is what turns it on; `magnitude` behaves the way a news report says it
does, doubling with every step up the scale, and only bodies that asked for
gravity and were not anchored ever feel it.
"""

import carbine
import pytest

SECOND = 1.0 / 60.0
HANDS = ("w", "a", "s", "d", "space", "escape")


def let_go():
    for name in HANDS:
        carbine.injectKey(name, False)


def run(event, seconds):
    """Steps the world with the event switched on, then leaves it off."""
    event.start()
    for _ in range(round(seconds / SECOND)):
        carbine.step(SECOND)
    event.stop()


@pytest.fixture
def calm():
    """A scene with no weather in it and the player handed back disabled: the
    whole engine is one scene per process, so a storm left running would leak."""
    carbine.physics.stopAll()
    for event in carbine.physics.running():
        event.destroy()
    player = carbine.player()
    let_go()
    player.enabled = False
    yield player
    carbine.physics.stopAll()
    player.enabled = False
    let_go()


def crate(*, at=(0.0, 0.0, 0.0), falls=True):
    """A metre cube, either loose enough for the weather to move or scenery that
    holds its ground."""
    body = carbine.new("cube", name="Crate")
    body.size = (1.0, 1.0, 1.0)
    body.position = at
    body.gravity = falls
    return body


def test_the_register_lists_the_weather_it_can_write(calm):
    assert carbine.physics.kinds() == ["blast", "earthquake", "tornado", "wind"]


def test_a_disaster_is_written_switched_off(calm):
    quake = carbine.physics.new("earthquake")
    try:
        assert quake.kind == "earthquake"
        assert quake.name == "earthquake"
        assert quake.running is False
        assert quake.magnitude == 5.0
        assert quake.reach == 0.0, "zero means the whole world"
        assert quake.duration == 8.0
        assert quake.elapsed == 0.0
        assert tuple(quake.epicentre) == (0.0, 0.0, 0.0)
        assert quake.node is None
        assert quake not in carbine.physics.running()

        quake.start()
        assert quake.running
        assert carbine.physics.running() == [quake]

        quake.stop()
        assert quake not in carbine.physics.running()
    finally:
        quake.destroy()


def test_capital_letters_and_short_names_are_forgiven(calm):
    asked = [
        ("Earthquake", "earthquake"),
        ("QUAKE", "earthquake"),
        ("twister", "tornado"),
    ]
    for name, kind in asked:
        event = carbine.physics.new(name)
        try:
            assert event.kind == kind
        finally:
            event.destroy()

    with pytest.raises(ValueError, match="hurricane"):
        carbine.physics.new("hurricane")


def test_a_step_up_the_scale_doubles_the_shove(calm):
    blast = carbine.physics.new("blast")
    blast.reach = 100.0
    try:
        blast.magnitude = 5.0
        blast.start()
        five = blast.accelerationAt((1.0, 0.0, 0.0))
        blast.magnitude = 7.0
        seven = blast.accelerationAt((1.0, 0.0, 0.0))
        assert five.x > 0.0, "a blast throws whatever it reaches outward"
        assert five.z > 0.0, "and a little up"
        assert abs(seven.x - 4.0 * five.x) < 1e-3, "two steps up is four times the shove"
    finally:
        blast.destroy()


def test_a_switched_off_disaster_shoves_nothing(calm):
    blast = carbine.physics.new("blast")
    try:
        assert tuple(blast.accelerationAt((1.0, 0.0, 0.0))) == (0.0, 0.0, 0.0)
    finally:
        blast.destroy()


def test_a_reach_makes_it_quiet_far_away(calm):
    gale = carbine.physics.new("wind")
    gale.direction = (1.0, 0.0, 0.0)
    gale.reach = 10.0
    try:
        gale.start()
        at_centre = gale.accelerationAt((0.0, 0.0, 0.0))
        halfway = gale.accelerationAt((5.0, 0.0, 0.0))
        assert at_centre.x > 0.0
        assert abs(halfway.x - 0.5 * at_centre.x) < 1e-3, "half the distance, half the shove"
        assert tuple(gale.accelerationAt((20.0, 0.0, 0.0))) == (0.0, 0.0, 0.0)
    finally:
        gale.destroy()


def test_a_quake_shoves_a_loose_crate_and_the_world_settles_after(calm):
    body = crate()
    scenery = crate(falls=False)
    quake = carbine.physics.new("earthquake")
    quake.magnitude = 8.0
    try:
        run(quake, 2.0)

        moved = tuple(body.position)
        assert moved != (0.0, 0.0, 0.0), f"the floor moved under the crate: {moved}"
        assert tuple(scenery.position) == (0.0, 0.0, 0.0), "scenery never asked to be moved"

        # Four seconds of quiet is enough to spend the speed the quake left behind,
        # and a crate that has come to rest stays there rather than sliding on.
        for _ in range(240):
            carbine.step(SECOND)
        settled = tuple(body.position)
        for _ in range(60):
            carbine.step(SECOND)
        after = tuple(body.position)
        drift = max(abs(one - both) for one, both in zip(settled, after))
        assert drift < 1e-3, f"a stopped quake stops shoving: {settled} -> {after}"
    finally:
        quake.destroy()
        body.destroy()
        scenery.destroy()


def test_a_funnel_bolted_to_a_fan_follows_the_fan(calm):
    fan = crate(at=(2.0, 0.0, 0.0), falls=False)
    twister = carbine.physics.new("tornado", name="Fune")
    twister.magnitude = 7.0
    twister.reach = 10.0
    twister.node = fan
    try:
        twister.start()
        assert twister.node == fan
        inside = twister.accelerationAt((2.5, 0.0, 0.0))
        assert inside.x < 0.0, f"drawn towards the funnel: {inside}"
        assert inside.z > 0.0, f"and thrown up inside it: {inside}"

        fan.position = (20.0, 0.0, 0.0)
        beside = twister.accelerationAt((20.5, 0.0, 0.0))
        assert beside.x < 0.0, f"the funnel came with the fan: {beside}"
        assert tuple(twister.accelerationAt((0.0, 0.0, 0.0))) == (0.0, 0.0, 0.0)

        twister.node = None
        assert twister.node is None
    finally:
        twister.destroy()
        fan.destroy()


def test_a_funnel_lifts_the_player_off_the_floor(calm):
    player = carbine.player()
    player.position = (0.0, 0.0, 0.0)
    player.enabled = True
    twister = carbine.physics.new("tornado")
    twister.magnitude = 7.0
    try:
        run(twister, 0.5)
        assert player.position.z > 0.0, "a magnitude 7 updraft beats a magnitude 10 world"
    finally:
        twister.destroy()
        player.position = (0.0, 0.0, 0.0)


def test_a_wind_you_stand_in_drifts_you_without_lifting_you(calm):
    player = carbine.player()
    player.position = (0.0, 0.0, 0.0)
    player.enabled = True
    gale = carbine.physics.new("wind")
    gale.direction = (1.0, 0.0, 0.0)
    gale.magnitude = 6.0
    try:
        run(gale, 2.0)
        assert player.position.x > 0.1, f"a steady wind drifts you: {player.position}"
        assert player.position.z == 0.0, "a horizontal wind never lifts you"
    finally:
        gale.destroy()
        player.position = (0.0, 0.0, 0.0)


def test_an_event_whose_time_is_up_stops_by_itself(calm):
    blast = carbine.physics.new("blast")
    blast.magnitude = 7.0
    blast.duration = 0.2
    blast.start()
    try:
        for _ in range(30):
            carbine.step(SECOND)
        assert blast.elapsed == pytest.approx(0.2, abs=1e-6)
        assert blast.running is False
        assert carbine.physics.running() == []
    finally:
        blast.destroy()


def test_stop_all_leaves_what_the_weather_already_did(calm):
    body = crate(at=(0.0, 0.0, 5.0))
    # Hang it in the air, so the only thing that can move it is the weather, and
    # nothing can slow it down once the weather stops.
    body.gravityPull = 0.0
    quake = carbine.physics.new("earthquake")
    quake.magnitude = 8.0
    quake.epicentre = (0.0, 0.0, 5.0)
    try:
        quake.start()
        for _ in range(60):
            carbine.step(SECOND)
        first = tuple(body.position)
        assert first != (0.0, 0.0, 5.0), f"the quake took the crate: {first}"

        carbine.physics.stopAll()
        assert carbine.physics.running() == []
        assert quake.running is False

        # With the rule gone the crate coasts in the straight line it was left in,
        # covering the same ground every second.
        for _ in range(60):
            carbine.step(SECOND)
        second = tuple(body.position)
        for _ in range(60):
            carbine.step(SECOND)
        third = tuple(body.position)
        one = [b - a for a, b in zip(first, second)]
        two = [c - b for b, c in zip(second, third)]
        assert one == pytest.approx(two, abs=1e-4), "the calm takes the rule, not the shove"
    finally:
        quake.destroy()
        body.destroy()


def test_a_destroyed_disaster_is_gone_not_resting(calm):
    event = carbine.physics.new("wind")
    event.start()
    event.destroy()

    assert carbine.physics.running() == []
    for read in (lambda: event.magnitude, lambda: event.running, lambda: event.kind):
        with pytest.raises(ValueError, match="no such disaster"):
            read()
    with pytest.raises(ValueError, match="no such disaster"):
        event.start()


def test_a_shock_that_ended_can_be_begun_again(calm):
    """A hand that switches a finished disaster on again means another one, not a
    frame of nothing: the aftershock a demo queues has to actually shake the ground.
    A disaster that is only paused keeps its clock, so tapping the switch does not
    jump the shaking to a new phase."""
    quake = carbine.physics.new("earthquake", name="Main shock")
    quake.magnitude = 7.0
    quake.duration = 0.5
    try:
        quake.start()
        for _ in range(round(1.0 / SECOND)):
            carbine.step(SECOND)
        assert quake.running is False, "it ended on its own"
        assert quake.elapsed == pytest.approx(0.5)

        quake.start()
        assert quake.running, "a restarted shock is a shock"
        assert quake.elapsed == 0.0, "and it starts from the beginning"
        for _ in range(round(0.25 / SECOND)):
            carbine.step(SECOND)
        assert quake.elapsed == pytest.approx(0.25, abs=0.02)

        quake.stop()
        paused = quake.elapsed
        quake.start()
        assert quake.elapsed == paused, "a paused one keeps the phase it stopped on"
    finally:
        quake.destroy()

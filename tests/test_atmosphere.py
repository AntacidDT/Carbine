"""The environment: `carbine.atmosphere`.

Three things make a frame feel like a place: where the sun is, how far the air lets
you see, and what is falling. Only the last of them is generated, and it is generated
by the particle generator - `atmosphere.new("rain")` is the fast path onto
`procedural.particles()`, so what comes back is a `Particle` wearing every particle
knob, hung on the sky so its box rides with whatever is looking.

Fog is not a particle: it is the air refusing to show far things, so it is a number on
the register with nothing to start or stop. Weather pushes no body however hard it
falls, and leans only when a `wind` or a `tornado` leans it.
"""

import carbine
import pytest

SECOND = 1.0 / 60.0
WIDTH = 64

CLEAR = (22, 21, 20)
ORANGE = (235, 122, 52)

NIGHT_ZENITH = (0.024, 0.031, 0.071)
DAY_ZENITH = (0.184, 0.424, 0.702)


def pixel(data, x, y):
    offset = (y * WIDTH + x) * 4
    return tuple(data[offset : offset + 3])


def frame():
    """The framing the `flat` fixture sets up, written out again so a test can
    restate it without asking the fixture to do it a second time."""
    sky = carbine.atmosphere.sky()
    sky.enabled = False
    sky.sun = (0.0, 0.0, 1.0)

    camera = carbine.camera()
    camera.mode = "orthographic"
    camera.position = (0.0, 0.0, 10.0)
    camera.target = (0.0, 0.0, 0.0)
    camera.viewHeight = 2.0


EYES = (0.0, -4.0, 1.0)
LOOKED_AT = (0.0, 0.0, 1.0)


def from_the_side():
    """`frame()`, turned to look along the ground. A veil hung on the sky stands
    over the eyes rather than through them, so a lens pointed down at the world has
    nothing in front of it left to show but the top of the rain."""
    frame()
    camera = carbine.camera()
    camera.position = EYES
    camera.target = LOOKED_AT


def close_the_sky_over(system):
    """A real veil is spread over the whole map, and a 64 pixel frame only ever
    holds a two metre box of it. Closing the box in on the eyes - and giving its
    drops a size the sampler can resolve - is the only way a test can count them."""
    system.area = (2.0, 2.0, 2.0)
    system.count = 300
    system.size = 0.1
    system.restart()


def flecks(pixels, dry):
    """How much of the frame the weather has taken up from the dry one."""
    return sum(1 for i in range(0, len(pixels), 4) if pixels[i + 1] > dry[i + 1])


def fall(system, seconds):
    """Steps the world with the crowd flying, then leaves it switched off."""
    system.active = True
    for _ in range(round(seconds / SECOND)):
        carbine.step(SECOND)
    system.active = False


def card(at, color):
    """A forty centimetre plate at whatever height the eyes are looking down past,
    so the only thing that can change its colour is the air in front of it."""
    plate = carbine.new("rectangle")
    plate.size = (0.4, 0.4, 1.0)
    plate.position = at
    plate.color = color
    return plate


@pytest.fixture
def clear_air():
    """The air as the engine found it. The whole engine is one scene per process, so
    a test that rains, fogs or moves the clock has to hand it back, or the next
    test's frame is still dark."""
    sky = carbine.atmosphere.sky()
    found = (
        carbine.atmosphere.time(),
        carbine.atmosphere.fog,
        tuple(sky.zenith),
        tuple(sky.horizon),
        sky.ambient,
    )

    carbine.atmosphere.stopAll()
    carbine.physics.stopAll()
    player = carbine.player()
    player.enabled = False
    yield player

    carbine.atmosphere.stopAll()
    carbine.physics.stopAll()
    carbine.atmosphere.fog = found[1]
    sky.zenith = found[2]
    sky.horizon = found[3]
    sky.ambient = found[4]
    carbine.atmosphere.time(found[0])
    player.enabled = False


def test_the_register_lists_what_can_fall(clear_air):
    assert carbine.atmosphere.kinds() == ["ash", "dust", "rain", "snow"]


def test_a_preset_is_a_particle_system_hung_on_the_sky(clear_air):
    rain = carbine.atmosphere.new("rain")
    try:
        assert isinstance(rain, carbine.Particle)
        assert rain.kind == "particles"
        assert rain.name == "Rain"
        assert rain.onSky is True
        assert rain.active is True

        assert rain.shape == "streak"
        assert rain.count == 1400
        assert rain.size == pytest.approx(0.035)
        assert rain.speed == pytest.approx(9.0)
        assert tuple(rain.area) == (70.0, 70.0, 45.0)
        assert rain.lifetime == pytest.approx(5.5)
        assert rain.splash == 2
        assert tuple(rain.color) == pytest.approx((0.725, 0.827, 0.902), abs=1e-3)

        assert carbine.atmosphere.running() == [rain]
    finally:
        rain.destroy()


def test_each_kind_is_written_its_own_way(clear_air):
    written = {kind: carbine.atmosphere.new(kind) for kind in carbine.atmosphere.kinds()}
    try:
        assert written["rain"].shape == "streak"
        assert written["snow"].shape == "circle"
        assert written["dust"].shape == "circle"
        assert written["ash"].shape == "square"

        # Rain is the heaviest and ash the lightest, which is the order a person
        # standing in both would feel it.
        assert written["rain"].speed > written["snow"].speed
        assert written["snow"].speed > written["dust"].speed
        assert written["dust"].speed > written["ash"].speed

        assert tuple(written["snow"].color) == pytest.approx(
            (0.949, 0.965, 0.980), abs=1e-3
        ), "flakes are nearly white"
        assert written["rain"].splash == 2
        assert written["ash"].splash == 0, "a chip of ash does not splash"
    finally:
        for system in written.values():
            system.destroy()


def test_capital_letters_and_the_names_a_report_uses_are_forgiven(clear_air):
    asked = [
        ("Rain", "Rain"),
        ("Downpour", "Rain"),
        ("BLIZZARD", "Snow"),
        (" sand ", "Dust"),
        ("cinder", "Ash"),
    ]
    for word, name in asked:
        system = carbine.atmosphere.new(word)
        try:
            assert system.name == name
        finally:
            system.destroy()

    with pytest.raises(ValueError, match="unknown atmosphere"):
        carbine.atmosphere.new("hail")


def test_fog_is_a_knob_and_not_something_that_falls(clear_air):
    assert carbine.atmosphere.fog == 0.0

    with pytest.raises(ValueError, match="fog is a knob"):
        carbine.atmosphere.new("fog")

    carbine.atmosphere.fog = 40.0
    assert carbine.atmosphere.fog == 40.0

    carbine.atmosphere.fog = 500.0
    assert carbine.atmosphere.fog == 100.0, "a hundred is all of it"

    carbine.atmosphere.fog = -3.0
    assert carbine.atmosphere.fog == 0.0, "and clear air is the other end"


def test_a_fog_colour_can_be_written_the_way_an_object_writes_one(clear_air):
    carbine.atmosphere.fogColor = "#eb7a34"
    assert tuple(carbine.atmosphere.fogColor) == pytest.approx(
        (235 / 255, 122 / 255, 52 / 255), abs=1e-3
    )

    carbine.atmosphere.fogColor = (0.1, 0.2, 0.3)
    assert tuple(carbine.atmosphere.fogColor) == pytest.approx((0.1, 0.2, 0.3), abs=1e-6)

    carbine.atmosphere.fogColor = carbine.Vec3(0.25, 0.5, 0.75)
    assert tuple(carbine.atmosphere.fogColor) == pytest.approx((0.25, 0.5, 0.75), abs=1e-6)

    with pytest.raises(ValueError, match="not a colour"):
        carbine.atmosphere.fogColor = "nope"


def test_the_clock_is_a_clock_a_person_can_read(clear_air):
    assert carbine.atmosphere.time("07:30") == 7.5
    assert carbine.atmosphere.time() == 7.5
    assert carbine.atmosphere.time("12:00") == 12.0
    assert carbine.atmosphere.time(18) == 18.0
    assert carbine.atmosphere.time("9") == 9.0

    # Past midnight wraps rather than running off the world.
    assert carbine.atmosphere.time("25.5") == 1.5
    assert carbine.atmosphere.time(26.0) == 2.0


def test_a_time_that_is_not_a_time_is_refused(clear_air):
    for clock in ("half past six", "07:75", "25:00", ""):
        with pytest.raises(ValueError, match="is not a time"):
            carbine.atmosphere.time(clock)

    with pytest.raises(TypeError, match="a time is a clock"):
        carbine.atmosphere.time([7, 30])


def test_the_clock_moves_the_sun_and_colours_the_sky(clear_air):
    sky = carbine.atmosphere.sky()

    carbine.atmosphere.time("12:00")
    assert tuple(sky.sun)[2] > 0.5, "midday is overhead"
    assert tuple(sky.zenith) == pytest.approx(DAY_ZENITH, abs=1e-3)
    noon = sky.ambient

    carbine.atmosphere.time("00:00")
    assert tuple(sky.sun)[2] < -0.5, "midnight is underfoot"
    assert tuple(sky.zenith) == pytest.approx(NIGHT_ZENITH, abs=1e-3)
    assert sky.ambient < noon, "a night is darker than a day"

    # The sky keeps its own knobs afterwards, because a script may want a red noon.
    sky.zenith = "#ff0000"
    assert tuple(sky.zenith) == pytest.approx((1.0, 0.0, 0.0), abs=1e-3)


def test_a_weather_system_can_be_taken_off_the_sky(clear_air):
    rain = carbine.atmosphere.new("rain")
    try:
        rain.onSky = False
        assert rain.onSky is False
        assert carbine.atmosphere.running() == [], "a local crowd is not the sky"

        rain.onSky = True
        assert carbine.atmosphere.running() == [rain]
    finally:
        rain.destroy()


def test_stop_all_clears_the_sky_and_leaves_the_weather_startable(clear_air):
    rain = carbine.atmosphere.new("rain")
    snow = carbine.atmosphere.new("snow")
    try:
        assert carbine.atmosphere.running() == [rain, snow], "in the order written"

        carbine.atmosphere.stopAll()
        assert carbine.atmosphere.running() == []
        assert rain.active is False
        assert snow.active is False

        rain.active = True
        assert carbine.atmosphere.running() == [rain], "stopAll leaves it written"
    finally:
        rain.destroy()
        snow.destroy()


def test_a_destroyed_system_is_gone_not_resting(clear_air):
    rain = carbine.atmosphere.new("dust")
    rain.destroy()

    assert carbine.atmosphere.running() == []
    for read in (lambda: rain.onSky, lambda: rain.count, lambda: rain.active):
        with pytest.raises(carbine.DestroyedError):
            read()


def test_the_weather_paints_but_never_pushes(clear_air):
    body = carbine.new("cube", name="Crate")
    body.size = (1.0, 1.0, 1.0)
    body.position = (0.0, 0.0, 5.0)
    body.gravity = True
    # Hang it where nothing can hold it, so the only thing that could move it is
    # the weather - and a downpour, however thick, is only a look.
    body.gravityPull = 0.0
    rain = carbine.atmosphere.new("rain")
    try:
        fall(rain, 2.0)
        assert tuple(body.position) == (0.0, 0.0, 5.0), body.position
    finally:
        rain.destroy()
        body.destroy()


def test_a_shower_brightens_the_frame_it_falls_through(flat, clear_air):
    rain = carbine.atmosphere.new("rain", name="Shower")
    rain.color = "#ffffff"
    from_the_side()
    close_the_sky_over(rain)
    try:
        wet = carbine.readPixels()
        rain.active = False
        dry = carbine.readPixels()

        assert pixel(dry, 32, 32) == CLEAR
        assert flecks(wet, dry) > 400, f"only {flecks(wet, dry)} pixels caught a drop"

        assert carbine.readPixels() == dry, "a stopped shower leaves the frame clear"
    finally:
        rain.destroy()


def test_a_sky_veil_follows_where_the_eyes_are(flat, clear_air):
    """The whole point of hanging a crowd on the sky: it is laid around whatever is
    looking, so walking across the map takes the rain with you."""
    rain = carbine.atmosphere.new("rain")
    rain.color = "#ffffff"
    from_the_side()
    close_the_sky_over(rain)
    camera = carbine.camera()
    try:
        rain.active = False
        dry = carbine.readPixels()
        rain.active = True
        assert pixel(dry, 32, 32) == CLEAR
        assert flecks(carbine.readPixels(), dry) > 400, "no drops over these eyes"

        # The next crowd is laid where the eyes have got to, five hundred metres on.
        camera.position = (500.0, -4.0, 1.0)
        camera.target = (500.0, 0.0, 1.0)
        rain.restart()
        assert flecks(carbine.readPixels(), dry) > 400, "the veil did not come with the eyes"

        # The same crowd hung on the node instead stays back at the origin.
        rain.onSky = False
        assert carbine.readPixels() == dry, "a local crowd left the frame behind"
    finally:
        rain.onSky = True
        rain.destroy()


def test_the_weather_slides_as_the_world_steps(flat, clear_air):
    snow = carbine.atmosphere.new("snow")
    from_the_side()
    close_the_sky_over(snow)
    try:
        early = carbine.readPixels()
        for _ in range(60):
            carbine.step(SECOND)
        late = carbine.readPixels()

        # No player took these steps, so the eyes are exactly where they were put
        # and the only thing that has moved is the fall itself.
        assert tuple(carbine.camera().position) == EYES
        assert early != late, "the flakes never moved"
    finally:
        snow.destroy()


def test_a_fog_takes_the_far_side_of_a_field_away(flat, clear_air):
    # The eyes look down from ten meters, so one card is two meters away and the
    # other is the full ten.
    near = card((0.0, 0.0, 8.0), (1.0, 0.0, 0.0))
    far = card((0.5, 0.0, 0.0), (1.0, 0.0, 0.0))
    try:
        carbine.atmosphere.fogColor = "#ffffff"
        assert pixel(carbine.readPixels(), 48, 32) == (255, 0, 0), "far, and clear air"

        carbine.atmosphere.fog = 60.0
        pixels = carbine.readPixels()
        beside = pixel(pixels, 32, 32)
        away = pixel(pixels, 48, 32)

        assert beside[0] == away[0] == 255, "fog never dims the colour it is mixed to"
        assert away[1] - beside[1] > 40, f"the far card should grey out first: {beside} vs {away}"

        carbine.atmosphere.fog = 0.0
        assert pixel(carbine.readPixels(), 48, 32) == (255, 0, 0), "the clear air came back"
    finally:
        near.destroy()
        far.destroy()


def test_a_shower_and_a_fog_share_the_frame(flat, clear_air):
    """Two different misbehavings of the air in one picture: a shower is things
    falling in it, fog is the air itself, so the far wall goes pale while drops
    still cut across in front of it."""
    from_the_side()
    # The eyes look along the ground, so the sun has to come down the same line for
    # the face they see to keep the colour it was given.
    carbine.atmosphere.sky().sun = (0.0, -1.0, 0.0)

    wall = carbine.new("cube", name="Far wall")
    wall.size = (1.0, 1.0, 1.0)
    wall.position = (0.0, 6.0, 1.0)
    wall.color = (1.0, 0.0, 0.0)
    wall.gravity = False

    rain = carbine.atmosphere.new("rain")
    rain.color = "#ffffff"
    close_the_sky_over(rain)
    try:
        rain.active = False
        dry = carbine.readPixels()
        assert pixel(dry, 32, 32) == (255, 0, 0), "ten metres of clear air, red wall"

        rain.active = True
        falling = flecks(carbine.readPixels(), dry)
        assert falling > 400, f"only {falling} pixels caught a drop"

        carbine.atmosphere.fogColor = "#ffffff"
        carbine.atmosphere.fog = 60.0
        rain.active = False
        only_fog = carbine.readPixels()
        assert pixel(only_fog, 32, 32)[0] == 255, "fog never dims the colour it is mixed to"
        assert pixel(only_fog, 32, 32)[1] > 150, "the fog has swallowed the far wall"

        rain.active = True
        assert carbine.readPixels() != only_fog, "the shower still cuts across foggy air"

        rain.active = False
        carbine.atmosphere.fog = 0.0
        assert carbine.readPixels() == dry, "the clear air came back"
    finally:
        rain.destroy()
        wall.destroy()


def test_the_weather_falls_behind_the_interface(flat, clear_air):
    element = carbine.ui.new("square")
    element.position = (32, 32, 0)
    element.size = (40, 40, 1)
    element.color = "#eb7a34"
    rain = carbine.atmosphere.new("rain")
    rain.color = "#ffffff"
    from_the_side()
    close_the_sky_over(rain)
    try:
        wet = carbine.readPixels()
        rain.active = False
        dry = carbine.readPixels()

        assert dry != wet, "the weather is on"
        # A widget is measured in pixels, so no weather is allowed to wash one out.
        for x, y in ((32, 32), (14, 14), (49, 49)):
            assert pixel(wet, x, y) == pixel(dry, x, y) == ORANGE
    finally:
        rain.destroy()
        element.destroy()

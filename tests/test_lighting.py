import carbine
import pytest

WIDTH = 64
HEIGHT = 64

LADDER = ["low", "medium", "high", "extra", "auto"]
LOOKS = ["blocky", "straight"]
GIB = 2**30


def pixel(data, x, y, width=WIDTH):
    offset = (y * width + x) * 4
    return tuple(data[offset : offset + 3])


def ending(column):
    """The stretch of a column on which a look can be read at all: from the edge of
    the umbra to the floor the sun reaches. Anywhere else a column is open floor or
    the shadow of the thing standing on it, and both are one colour whichever way the
    edge is drawn, so a run measured down a whole column counts the floor and not the
    look."""
    darkest, lightest = min(column), max(column)
    first = column.index(darkest)
    last = len(column) - 1 - column[::-1].index(darkest)
    if column[0] > column[-1]:
        back = (index for index in range(first, -1, -1) if column[index] + 1 >= lightest)
        return column[next(back, 0) : first + 1]
    on = (index for index in range(last, len(column)) if column[index] + 1 >= lightest)
    return column[last : next(on, len(column) - 1) + 1]


def reversals(column):
    """How much brightness a column gives back on its way into the shade. A fade
    answers with nothing; a weave that lays a different handful of readings in every
    pixel climbs again where its neighbour fell, and that swing is the grain a person
    sees. A column can only ever give back less than it was paid, so the smaller of
    its two totals is the give-back, which way ever it runs."""
    steps = list(zip(column, column[1:]))
    return min(
        sum(later - earlier for earlier, later in steps if later > earlier),
        sum(earlier - later for earlier, later in steps if earlier > later),
    )


@pytest.fixture
def afternoon(headless):
    """A floor with a wall standing in a sun that is up and off to one side, seen
    from directly above by an orthographic lens eight metres across. The backdrop is
    off because the light is what is under test here: with a sky drawn the faces the
    sun leaves out would take its colour as well, and that is another test's job.

    The wall is six metres tall, so its shade lies over a metre and a half of floor
    before it and metres beyond it: both samples sit well inside one or the other,
    away from the blurred edge."""
    sky = carbine.atmosphere.sky()
    camera = carbine.camera()
    found = (
        sky.enabled,
        tuple(sky.sun),
        camera.mode,
        tuple(camera.position),
        tuple(camera.target),
        camera.viewHeight,
    )

    sky.enabled = False
    sky.sun = (0.0, -1.0, 1.0)
    camera.mode = "orthographic"
    camera.position = (0.0, 0.0, 10.0)
    camera.lookAt((0.0, 0.0, 0.0))
    camera.viewHeight = 8.0

    floor = carbine.new("rectangle", name="Floor")
    wall = carbine.new("cube", name="Wall")
    for card, size, place in ((floor, (10.0, 10.0, 1.0), (0.0, 0.0, 0.0)),
                              (wall, (4.0, 0.4, 6.0), (0.0, -1.0, 3.0))):
        card.size = size
        card.position = place
        card.color = (0.8, 0.8, 0.8)

    yield (20, 60)

    wall.destroy()
    floor.destroy()
    (
        sky.enabled,
        sky.sun,
        camera.mode,
        camera.position,
        camera.target,
        camera.viewHeight,
    ) = found


@pytest.fixture
def shade_end(headless):
    """A cube standing in a forty five degree sun, framed close on the far end of its
    shade by an orthographic lens a metre and a half across, so one metre of the world
    is forty-two pixels here. The blur a rung lays is only a few centimetres of that
    world, which is a handful of pixels at any wider framing and nothing to read a way
    of drawing an edge off; at this framing it is tens of pixels and either a fade or a
    grain. The backdrop is off for the reason it is off in `afternoon`: the light is
    what is under test."""
    sky = carbine.atmosphere.sky()
    camera = carbine.camera()
    found = (
        sky.enabled,
        tuple(sky.sun),
        camera.mode,
        tuple(camera.position),
        tuple(camera.target),
        camera.viewHeight,
    )

    sky.enabled = False
    sky.sun = (0.0, -1.0, 1.0)
    camera.mode = "orthographic"
    camera.position = (0.0, 2.0, 10.0)
    camera.lookAt((0.0, 2.0, 0.0))
    camera.viewHeight = 1.5

    floor = carbine.new("rectangle", name="Floor")
    cube = carbine.new("cube", name="Cube")
    for card, size, place in ((floor, (40.0, 40.0, 1.0), (0.0, 0.0, 0.0)),
                              (cube, (1.0, 1.0, 2.0), (0.0, 0.0, 1.0))):
        card.size = size
        card.position = place
        card.color = (0.8, 0.8, 0.8)

    yield

    cube.destroy()
    floor.destroy()
    (
        sky.enabled,
        sky.sun,
        camera.mode,
        camera.position,
        camera.target,
        camera.viewHeight,
    ) = found


def test_the_ladder_is_offered_cheapest_first(headless):
    assert carbine.atmosphere.lightQualities() == LADDER
    assert carbine.atmosphere.lightQuality == "low", "a test target is pinned to the bare world"


def test_a_written_rung_is_the_one_read_back(headless):
    for rung in LADDER:
        carbine.atmosphere.lightQuality = rung
        assert carbine.atmosphere.lightQuality == rung


def test_auto_answers_a_rung_and_never_itself(headless):
    carbine.atmosphere.lightQuality = "auto"

    light = carbine.atmosphere.light()

    assert light.quality in ("low", "medium", "high"), "the dearest rung is only ever asked for"
    assert carbine.atmosphere.lightQuality == "auto", "reading the light does not move the knob"


def test_auto_stands_on_the_card_it_drew(headless):
    carbine.atmosphere.lightQuality = "auto"

    light = carbine.atmosphere.light()

    assert light.device in ("discrete", "integrated", "cpu", "virtual", "other")
    if light.device in ("integrated", "cpu", "virtual", "other"):
        # A card that borrows the machine's memory owns none of its own.
        assert light.quality == "low"
    elif light.memory and light.memory > 4 * GIB:
        assert light.quality == "high", "a big card has room for a big cast"


def test_a_rung_above_the_floor_of_it_casts(headless):
    carbine.atmosphere.lightQuality = "low"
    bare = carbine.atmosphere.light()
    assert bare.shadows is False
    assert bare.shadowMap == 0
    assert bare.softness == 0
    assert bare.reach == 0.0
    assert bare.gloss == 0.0

    carbine.atmosphere.lightQuality = "medium"
    casting = carbine.atmosphere.light()
    assert casting.shadows is True
    assert casting.shadowMap > 0
    assert casting.softness > 1, "a shade that fades where it ends costs more than a hard edge"
    assert casting.reach > 0.0
    assert casting.gloss > 0.0


def test_every_rung_costs_at_least_the_one_below_it(headless):
    seen = []
    for rung in ("low", "medium", "high", "extra"):
        carbine.atmosphere.lightQuality = rung
        light = carbine.atmosphere.light()
        seen.append((light.shadowMap, light.softness, light.reach, light.gloss))

    for cheaper, dearer in zip(seen, seen[1:]):
        assert dearer >= cheaper, f"{dearer} should not cost less than {cheaper}"
    assert seen[3][0] > seen[1][0], "the top of the ladder is not the middle of it"


def test_the_sky_only_fills_when_there_is_a_sky(headless):
    sky = carbine.atmosphere.sky()
    carbine.atmosphere.lightQuality = "extra"
    found = sky.enabled
    try:
        sky.enabled = False
        assert carbine.atmosphere.light().skyFill is False
        sky.enabled = True
        assert carbine.atmosphere.light().skyFill is True
    finally:
        sky.enabled = found


def test_the_rung_refuses_a_name_the_engine_does_not_know(headless):
    with pytest.raises(ValueError) as raised:
        carbine.atmosphere.lightQuality = "sparkly"

    said = str(raised.value)
    assert "sparkly" in said
    assert "medium" in said, "a refused name is told what the knob does take"


def test_the_light_handle_has_no_knobs(headless):
    light = carbine.atmosphere.light()

    with pytest.raises(AttributeError):
        light.quality = "low"
    with pytest.raises(AttributeError):
        light.shadowMap = 4096
    with pytest.raises(AttributeError):
        light.style = "blocky"


def test_the_light_reads_as_itself(headless):
    carbine.atmosphere.lightQuality = "high"

    said = repr(carbine.atmosphere.light())

    assert said.startswith("<Light high "), said
    assert "cast=2048" in said, said


@pytest.fixture
def look(headless):
    """The light type is one knob for the whole process, like the frame limits, so a
    test that turns it hands back the look it found."""
    found = carbine.lightType
    yield found
    carbine.lightType = found


def test_the_world_starts_with_a_woven_edge(look):
    assert look == "straight", "the plain world is a woven one; blocks are asked for"
    assert carbine.lightTypes() == LOOKS
    assert carbine.atmosphere.light().style == "straight"


def test_a_written_look_is_the_one_read_back(look):
    for style in carbine.lightTypes():
        carbine.lightType = style
        assert carbine.lightType == style
        assert carbine.atmosphere.light().style == style

    # A name that means the same thing is taken as the name it stands for.
    carbine.lightType = "pixel"
    assert carbine.lightType == "blocky", "an alias is answered with the name the engine keeps"


def test_the_light_type_refuses_a_name_the_engine_does_not_know(look):
    with pytest.raises(ValueError) as raised:
        carbine.lightType = "wobbly"

    said = str(raised.value)
    assert "wobbly" in said
    assert "blocky" in said and "straight" in said, "a refused name is told what the knob takes"


def test_the_look_of_an_edge_costs_what_the_rung_said(look):
    def cast():
        light = carbine.atmosphere.light()
        return (light.quality, light.shadowMap, light.softness, light.reach, light.gloss,
                light.skyFill)

    for rung in ("medium", "high", "extra"):
        carbine.atmosphere.lightQuality = rung
        carbine.lightType = "straight"
        woven = cast()
        carbine.lightType = "blocky"
        assert cast() == woven, f"{rung} is priced by the rung, not by the look: {woven}"


def test_the_bare_world_is_the_bare_world_whatever_the_look_says(afternoon, look):
    shade_row, lit_row = afternoon

    carbine.atmosphere.lightQuality = "low"
    carbine.lightType = "straight"
    woven = carbine.readPixels()
    carbine.lightType = "blocky"
    stepped = carbine.readPixels()

    # The lowest rung draws no edge at all, so there is nothing for a look to change
    # and every colour the pixel tests were measured against stays where it was.
    assert woven == stepped, "a rung that casts nothing has no edge to lay either way"
    assert pixel(woven, 32, shade_row) == pixel(woven, 32, lit_row)


def test_a_wall_throws_a_shade_only_once_the_light_starts_casting(afternoon):
    shade_row, lit_row = afternoon

    carbine.atmosphere.lightQuality = "low"
    bare = pixel(carbine.readPixels(), 32, lit_row)
    assert pixel(carbine.readPixels(), 32, shade_row) == bare, (
        "a rung that casts nothing has no shade to throw"
    )

    carbine.atmosphere.lightQuality = "medium"
    cast = carbine.readPixels()
    shade = pixel(cast, 32, shade_row)
    lit = pixel(cast, 32, lit_row)
    assert all(lit[channel] > shade[channel] + 40 for channel in range(3)), (
        f"the same floor beyond the wall should be in shade: {shade} against {lit}"
    )
    # The rung may add a sheen to the floor the sun reaches, but it cannot take the
    # sun off it: a cast that shaded the open floor would be a cast read upside down.
    assert lit[0] + 16 >= bare[0], (
        f"the sun that still reaches the near floor should not have been lost: {lit} vs {bare}"
    )


def test_a_woven_shade_end_fades_instead_of_speckling(shade_end, look):
    for rung in ("medium", "high", "extra"):
        carbine.atmosphere.lightQuality = rung
        carbine.lightType = "straight"
        frame = carbine.readPixels()

        # Three columns, so that no one place the shade's end happens to fall in the
        # cast carries the answer for the rung.
        for column in (16, 32, 48):
            lit = [pixel(frame, column, row)[0] for row in range(HEIGHT)]
            end = ending(lit)
            assert max(end) - min(end) > 40, f"{rung} left no shade ending in frame: {end}"
            assert reversals(end) <= 2, (
                f"{rung} wove {reversals(end)} levels of brightness back into a shade "
                f"that should only be fading: {end}"
            )


@pytest.fixture
def daylight(flat):
    """The sun the world is lit by, which is a node like any other and therefore a knob
    a test can turn. One sun serves the whole process, so everything written on it is
    handed back the way it was found.

    `flat` puts it overhead with the backdrop out of the frame, which is what a test
    that reads one exact colour needs: a floor facing straight up into a white sun is
    the colour of the floor, and with the sun's own strength at nil the only light left
    in the frame is whatever a lamp is throwing."""
    sun = carbine.scene.sun
    found = (sun.brightness, tuple(sun.color), sun.visible)

    yield sun

    sun.brightness, sun.color, sun.visible = found


@pytest.fixture
def floor(flat):
    """Two metres of white boards filling the frame `flat` shows, so every pixel in it
    is floor and the light falling on it is the only thing that can change its colour."""
    boards = carbine.new("rectangle", name="Floor")
    boards.size = (2.0, 2.0, 1.0)
    boards.color = (1.0, 1.0, 1.0)

    yield boards

    boards.destroy()


def test_a_light_is_a_node_you_can_hang(headless):
    assert "light" in carbine.kinds(), "the kind list is how a script learns the way to write one"

    box = carbine.new("cube", name="Box")
    porch = carbine.new("Light", name="Porch")
    street = carbine.new("Light")
    try:
        assert porch.kind == "light"
        assert porch.shape == "point", "a light a script writes shines out of one place"
        assert porch.brightness == 1.0
        assert porch.reach > 0.0
        assert street.name == "Light", "a light left unnamed is still called what it is"

        # A light stands on its own on the root, or hangs under a thing that carries it.
        assert street.parent == carbine.scene.root
        box.addChild(porch)
        assert [child.name for child in box.children] == ["Porch"]
        assert porch.parent == box

        # Taking it off the box leaves it in the world, only standing on its own.
        box.removeChild(porch)
        assert porch.parent == carbine.scene.root
    finally:
        street.destroy()
        porch.destroy()
        box.destroy()


def test_a_lamp_carries_what_it_gives_with_it(headless):
    lamp = carbine.new("Light")
    try:
        lamp.brightness = 2.5
        assert lamp.brightness == 2.5
        lamp.brightness = -1.0
        assert lamp.brightness == 0.0, "a lamp cannot carry less light than none at all"

        lamp.reach = 7.5
        assert lamp.reach == 7.5
        lamp.reach = -3.0
        assert lamp.reach == 0.0, "and it cannot reach less than nowhere"

        lamp.color = (1.0, 0.0, 0.0)
        assert tuple(lamp.color) == (1.0, 0.0, 0.0), "the colour of a lamp is the light it throws"
    finally:
        lamp.destroy()


def test_a_lamp_lays_its_edge_the_way_it_was_told(look):
    lamp = carbine.new("Light")
    try:
        for style in carbine.lightTypes():
            lamp.type = style
            assert lamp.type == style
            assert carbine.lightType == look, "a lamp's own look does not move the world's"

        lamp.type = "pixel"
        assert lamp.type == "blocky", "an alias is answered with the name the engine keeps"

        with pytest.raises(ValueError) as raised:
            lamp.type = "wobbly"
        said = str(raised.value)
        assert "wobbly" in said
        assert "blocky" in said and "straight" in said, "a refused name is told what it takes"
    finally:
        lamp.destroy()


def test_a_lamp_that_never_named_a_look_follows_the_worlds(look):
    for style in carbine.lightTypes():
        carbine.lightType = style
        lamp = carbine.new("Light")
        try:
            assert lamp.type == style, f"a {style} world should carry an unwritten lamp"
        finally:
            lamp.destroy()


def test_the_shape_of_a_light_is_answered_and_not_written(headless):
    sun = carbine.scene.sun
    assert sun.shape == "sun", "daylight arrives from one direction, not out of one place"
    assert sun.name == "Sun"
    assert sun.kind == "light"
    assert sun in carbine.scene.root.children, "the world's sun is a node hung on the root"
    assert carbine.scene.nodeCount == 0, "and it is furniture the world came with, not content"

    lamp = carbine.new("Light")
    try:
        assert lamp.shape == "point"
        assert carbine.scene.nodeCount == 1, "a light a script writes is content like any other"

        # A second sun would need a second picture of the world to cast out of, so a
        # lamp is never made one and the shape a light answers with is read, not set.
        for light in (sun, lamp):
            with pytest.raises(AttributeError):
                light.shape = "sun"
    finally:
        lamp.destroy()


def test_the_light_knobs_refuse_a_thing_that_is_not_one(headless):
    box = carbine.new("cube", name="Box")
    panel = carbine.ui.new("rectangle", name="Panel")
    lamp = carbine.new("Light")
    try:
        for knob in ("shape", "brightness", "reach", "type"):
            with pytest.raises(ValueError) as raised:
                getattr(box, knob)
            said = str(raised.value)
            assert "not a light" in said, f"{knob} on a box: {said}"
            assert "carbine.new" in said, f"{knob} should be told how a light is written: {said}"

        with pytest.raises(ValueError):
            box.brightness = 1.0
        with pytest.raises(ValueError):
            box.type = "blocky"

        # A light is a world node, so the UI knobs refuse it the way the light knobs
        # refuse a box - and the same message answers a read as a write.
        with pytest.raises(ValueError) as raised:
            lamp.corner = 8
        assert "not a UI element" in str(raised.value)

        with pytest.raises(ValueError):
            panel.brightness
    finally:
        panel.destroy()
        lamp.destroy()
        box.destroy()


def test_the_daylights_colour_and_strength_are_what_the_world_is_lit_by(daylight, floor):
    white = pixel(carbine.readPixels(), 32, 32)
    assert white == (255, 255, 255), f"a floor facing a whole white sun is the floor: {white}"

    daylight.brightness = 0.0
    dark = pixel(carbine.readPixels(), 32, 32)
    assert all(channel < 100 for channel in dark), (
        f"switch the sun off and only the light nobody directed is left: {dark}"
    )
    assert dark[0] == dark[1] == dark[2], f"what is left has no colour to give: {dark}"

    daylight.brightness = 1.0
    daylight.color = (1.0, 0.0, 0.0)
    red = pixel(carbine.readPixels(), 32, 32)
    assert red == (255, dark[0], dark[0]), (
        f"a sun painted red should light red and leave the rest to the dark: {red} vs {dark}"
    )


def test_a_lamp_lights_the_world_it_reaches_and_none_of_the_rest(daylight, floor):
    daylight.brightness = 0.0
    alone = carbine.readPixels()

    lamp = carbine.new("Light", name="Lamp")
    try:
        lamp.position = (0.0, 0.0, 0.2)
        lamp.reach = 0.5

        lit = carbine.readPixels()
        middle = pixel(lit, 32, 32)
        edge = pixel(lit, 4, 32)
        assert middle[0] > pixel(alone, 32, 32)[0] + 40, (
            f"a lamp standing on the floor should light the floor under it: {middle}"
        )
        assert edge == pixel(alone, 4, 32), (
            f"a metre away stands past the half metre of reach: {edge} vs {pixel(alone, 4, 32)}"
        )

        lamp.reach = 4.0
        wider = pixel(carbine.readPixels(), 4, 32)
        assert wider[0] > edge[0], "a longer reach takes in what the short one left dark"

        # A light draws nothing of itself, and switched off it gives nothing back.
        lamp.reach = 0.5
        lamp.visible = False
        assert carbine.readPixels() == alone, "a lamp nobody can see is a lamp nobody is lit by"
    finally:
        lamp.destroy()


def test_an_unwritten_lamp_lays_its_falloff_as_the_world_lays_its_edges(daylight, floor, look):
    daylight.brightness = 0.0
    lamp = carbine.new("Light", name="Lamp")
    try:
        lamp.position = (0.0, 0.0, 0.2)
        lamp.reach = 0.5

        carbine.lightType = "straight"
        woven = pixel(carbine.readPixels(), 32, 32)[0]
        carbine.lightType = "blocky"
        stepped = pixel(carbine.readPixels(), 32, 32)[0]

        # A blocky falloff takes the same run in four steps, so a fifth of the way down
        # it is already at the step below where the straight run still stands.
        assert stepped < woven, (
            f"the world's look should reach the lamp that never named one: {stepped} vs {woven}"
        )
    finally:
        lamp.destroy()

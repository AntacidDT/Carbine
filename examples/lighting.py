"""How hard the sun is allowed to work: `carbine.atmosphere.lightQuality`.

    python examples/lighting.py

The atmosphere register holds two things. The weather is particles and is written by
`atmosphere.new(...)`; the light is a rung, and it is what the world is shaded by.
`atmosphere.lightQuality` names the rung and `atmosphere.light()` says what that rung
turned out to be - whether the sun pre-renders the world to read a shade out of, how
big that picture is, how many readings a pixel takes around its own place in it, how
far around the eyes it reaches, how much of the sun a face turned square to it throws
back, and whether the light the sun leaves out is borrowed from the backdrop.

A fresh world is on "auto", which is the engine reading the card it draws on and
choosing for itself. The rule lives in Rust and this file only prints the answer: a
discrete card owning more than 4 GB takes the high rung, one from a tenth of a GB up
to 4 GB the middle, and anything smaller - or anything that shares the machine's
memory - the lowest. Nothing ever auto-picks "extra", because that is the rung a
person asks for on purpose.

    1 2 3 4   low, medium, high, extra
    0         hand the choice back to the machine
    e         lay the edge the other way: woven straight, or stepped blocks
    g         the daylight off or on, which is the only way to see the lamps plainly
    r         the reach of the two lamps, nine metres or three
    n / b     run the clock on, or back: the sun moves and the shades swing after it
    t         the backdrop on or off, which is the only thing sky fill depends on
    WASD walk - mouse look - space jump - ESC releases the mouse - close to quit

The scene stands at mid-afternoon, so the wall's shade lies across the ground between
the eyes and it. `1` is the world with nothing added: a face lights by the angle it
holds to the sun and nothing else, so the ground under the wall is as bright as the
ground in the open and the pillars throw no shade at all. Each rung above it adds its
own thing - `2` the shade, a sheen along the two balls, and the backdrop filling in
where the sun is not; `3` the same over a bigger and softer picture; `4` the biggest
and softest picture of them all. The bar in the corner is the weight of the rung, with
the reach of the cast under it.

Look at the far edge of the wall's shade, because that is what the rungs are priced
on: four readings cannot make an edge any crisper than it is, and sixteen can fade it
out instead of ending it. `t` is the other half of the same picture - the light the
sun leaves out is borrowed from the backdrop, so switching the backdrop off takes the
colour out of the shade as well as the blue out of the sky.

The backdrop carries its own dust, and it is worth knowing which way that is laid
because it covers more of the frame than any shade does: `sky.noise` says how much, and
the haze is a dome of soft blobs the view turns through, a degree and a half wide, so a
pixel and the pixels beside it stand in the same blob and the sky settles instead of
boiling. Turned up it reads as thin weather; a film of dice thrown on the window would
only ever read as grain, and would sit still while the world moved past it.

How that edge is *drawn* is a second knob, and it costs nothing: `carbine.lightType`.
"straight", which a fresh world starts on, weaves the readings together - the cast is
read between its texels, the readings lie about a texel apart, so the blur is woven out
of the picture it was drawn from rather than out of a handful of dice thrown on it, and
the whole weave turns slowly as the cast goes by instead of jumping between one pixel
and the next. A pixel and its neighbours therefore weigh nearly the same readings, which
is what lets a shade fade instead of grain. The readings far out in the blur count for
less than the one under the pixel, and the shade is taken off along the border of the
cast's picture instead of cut by it. "blocky" does none of
that: one texel per pixel, the same handful of texels in every pixel, weighed alike,
and the shade stops dead at the edge of the picture. Blocks are a look some games want,
so `e` hands it to them; `1` has no cast either way and so no look either.

The cast is a picture of the world centred on wherever the eyes stand, drawn out to the
reach the rung gives it; a thing further off than that is lit and casts nothing, which
is why a heavier rung buys distance as well as a softer edge.

Lights are nodes, too. `carbine.new("Light")` writes one: an object that draws nothing,
hangs in the tree like anything else - on the root, or under a pillar that carries it
along - and lights the world within its reach. It carries a `brightness` (1 is what a
lamp is born with), a `color`, which is the colour of the light and not of a surface, a
`reach` in metres, and a `type`, either `"straight"` or `"blocky"`, which is how it lays
its own falloff. Left alone a lamp lays its edge the way the world does, so `e` moves the
lamps with the sun without anyone writing on them.

The daylight is one of these nodes as well, and `carbine.scene.sun` hands it back: the
world is born with it, it is the only light that throws a shade, and painting or dimming
it is the same two knobs. It shines from one direction rather than out of one place, so
its `shape` reads `"sun"` and stays that way - a second sun would need a second picture
of the world to cast out of. `g` switches the daylight off, which is the fastest way to
see what the two lamps by the pillars are actually doing, and `r` takes their reach from
nine metres to three and back.

A lamp gives light and hides nothing: the rungs above "low" cast the *sun's* shade, and a
lamp standing in a doorway lights the floor in front of it without throwing the door
behind it. That is the next thing to buy, not something this file can turn a knob for.

The light is only a look: nothing in this file pushes anything, so the world stays
where it was put until a disaster says otherwise.
"""

import time

import carbine

ORANGE = "#eb7a34"
NAVY = "#2f6cb3"

# An hour with the sun low enough to make a shade worth looking at, and far enough
# round that it lies on the ground the eyes are standing on rather than behind it.
AFTERNOON = 15.5

# How much of a day the clock moves in a second while a key is held down.
CLOCK_PER_SECOND = 0.7

# (x, y, height) - four pillars in a row so their shades can be seen lying side by
# side, and one wall further off to lay a shade across the whole field.
PILLARS = [(-6.0, -6.0, 3.0), (-2.0, -6.0, 5.0), (2.0, -6.0, 7.0), (6.0, -6.0, 4.0)]

# (x, y, height, colour) - two lamps stood in front of the ends of that row, one warm
# and one cold, so their pools lie apart and one can be seen ending while the other is
# still lit. A lamp's colour is the colour of the light it throws, not of a surface.
LAMPS = [(-6.0, -4.4, 1.6, ORANGE), (6.0, -4.4, 1.6, NAVY)]

# What `r` hands the lamps between: far enough to reach the ground the eyes stand on,
# near enough to leave it in the dark halfway there.
NEAR, FAR = 3.0, 9.0

GIB = 2**30


def clock_face(hours: float) -> str:
    """The clock back the way a person reads it, so the printout matches the hour the
    key press asked for."""
    wrapped = hours % 24.0
    return f"{int(wrapped):02d}:{int(round(wrapped % 1.0 * 60.0)) % 60:02d}"


def as_text(light) -> str:
    """The whole rung on one line, so a printout says what the engine decided and not
    only what it was asked for."""
    if light.shadows:
        cast = f"cast {light.shadowMap} texels, {light.softness} taps over {light.reach:g} m"
    else:
        cast = "no cast"
    return (
        f"{cast} - sheen {light.gloss:g} - sky fill "
        f"{'on' if light.skyFill else 'off'}"
    )


def main() -> None:
    adapter = carbine.init(1280, 720, title="Carbine Light")
    width, height = carbine.viewport()
    print(
        f"{adapter} at {width}x{height}\n"
        "WASD walk - mouse look - space jump\n"
        "1 low - 2 medium - 3 high - 4 extra - 0 back to auto\n"
        "e lay the edge the other way - g the daylight - r the reach of the lamps\n"
        "n/b run the clock - t the backdrop\n"
        "ESC releases the mouse - close the window to quit"
    )

    # The daylight is a light node like any other, which is what makes it configurable.
    sun = carbine.scene.sun

    rungs = [rung for rung in carbine.atmosphere.lightQualities() if rung != "auto"]
    print("the ladder, cheapest first:", ", ".join(rungs))
    print("the edge, as the engine names it:", ", ".join(carbine.lightTypes()))
    print(
        f"the daylight is a node as well: {sun.name} the {sun.kind}, shining as a "
        f"{sun.shape} - g switches it, r moves the reach of the two lamps"
    )

    ground = carbine.new("plane", name="Ground")
    ground.size = (80.0, 80.0, 1.0)
    ground.color = "#3d4a3a"

    wall = carbine.new("cube", name="Wall")
    wall.size = (14.0, 0.8, 5.0)
    wall.position = (0.0, -16.0, 2.5)
    wall.color = "#9a958c"
    wall.gravity = False

    for index, (across, away, tall) in enumerate(PILLARS):
        pillar = carbine.new("cube", name=f"Pillar {index + 1}")
        pillar.size = (1.6, 1.6, tall)
        pillar.position = (across, away, tall / 2.0)
        pillar.color = "#8a6d3b"
        pillar.gravity = False

    # Two faces curved to the sun, so the sheen a rung above low adds has somewhere
    # to show itself and the ground under it has a shade with a round edge.
    balls = []
    for index, (across, away) in enumerate([(1.8, -1.5), (-2.4, -3.0)]):
        ball = carbine.new("sphere", name=f"Ball {index + 1}")
        ball.size = (1.2, ..., 1.2)
        ball.position = (across, away, 0.6)
        ball.color = NAVY
        ball.gravity = False
        balls.append(ball)

    # Two lamps, written like any other node and hung on the root. Neither one draws
    # itself: what they do is give the ground and the pillars a colour the sun did not.
    lamps = []
    for index, (across, away, high, tint) in enumerate(LAMPS):
        lamp = carbine.new("Light", name=f"Lamp {index + 1}")
        lamp.position = (across, away, high)
        lamp.color = tint
        lamp.brightness = 1.6
        lamp.reach = FAR
        lamps.append(lamp)

    carbine.atmosphere.time(AFTERNOON)

    panel = carbine.ui.new("rectangle", name="Hud")
    panel.position = (120, height - 44, 0)
    panel.size = (300, 36, 1)
    panel.color = "#221e1b"
    panel.corner = 10
    panel.transparent = 190

    # The weight of the rung, and under it how far the cast reaches.
    rung_bar = carbine.ui.new("rectangle", name="Rung", parent=panel)
    rung_bar.position = (-90, -8, 0)
    rung_bar.size = (4.0, 10.0, 1)
    rung_bar.color = ORANGE
    rung_bar.corner = 5

    cast_bar = carbine.ui.new("rectangle", name="Cast", parent=panel)
    cast_bar.position = (-90, 8, 0)
    cast_bar.size = (4.0, 10.0, 1)
    cast_bar.color = NAVY
    cast_bar.corner = 5

    crosshair = carbine.ui.new("circle", name="Crosshair")
    crosshair.position = (width / 2.0, height / 2.0, 0)
    crosshair.size = (10.0, 10.0, 1)
    crosshair.color = ORANGE
    crosshair.transparent = 170

    player = carbine.player()
    player.position = (0.0, 0.0, 0.0)
    player.enabled = True

    light = carbine.atmosphere.light()
    print(
        f"auto read the machine as {light.quality} on a "
        f"{light.device} card of {(light.memory or 0) / GIB:.1f} GiB - {as_text(light)}"
    )

    keys = dict(zip("1234", rungs))
    frame = 0
    try:
        while carbine.running():
            carbine.render()
            step = carbine.dt()

            for key, rung in keys.items():
                if carbine.pressed(key):
                    carbine.atmosphere.lightQuality = rung
                    print(f"{rung} asked for - {as_text(light)}")

            if carbine.pressed("0"):
                carbine.atmosphere.lightQuality = "auto"
                print(f"auto asked for - {light.quality} is what the card gives")

            if carbine.pressed("e"):
                carbine.lightType = "blocky" if carbine.lightType == "straight" else "straight"
                print(f"the edge is laid {carbine.lightType} - {as_text(light)}")

            if carbine.pressed("g"):
                # The one knob the daylight has that the rungs do not: the sun is a node,
                # so it is switched off by writing on the node and not by a setting.
                sun.brightness = 0.0 if sun.brightness > 0.0 else 1.0
                if sun.brightness > 0.0:
                    print("daylight on - the wall has its shade back")
                else:
                    print("daylight off - the two lamps are all the floor has left")

            if carbine.pressed("r"):
                reach = NEAR if lamps[0].reach > NEAR else FAR
                for lamp in lamps:
                    lamp.reach = reach
                print(f"the lamps reach {reach:g} m - what stands past that is dark")

            if carbine.key("n") or carbine.key("b"):
                hours = carbine.atmosphere.time()
                moving = CLOCK_PER_SECOND * step
                if carbine.key("b"):
                    moving = -moving
                print(f"{clock_face(carbine.atmosphere.time(hours + moving))}")

            if carbine.pressed("t"):
                sky = carbine.atmosphere.sky()
                sky.enabled = not sky.enabled
                print(
                    f"backdrop {'on' if sky.enabled else 'off'} - sky fill "
                    f"{'on' if light.skyFill else 'off'}"
                )

            # The bars are the rung's own numbers, so a heavier rung moves them by
            # itself and nothing here has to remember what each rung was given.
            weight = rungs.index(light.quality) / (len(rungs) - 1)
            rung_bar.size = (4.0 + 110.0 * weight, ..., ...)
            rung_bar.color = ORANGE if light.shadows else NAVY
            cast_bar.size = (4.0 + 110.0 * (light.reach / 160.0), ..., ...)
            cast_bar.color = NAVY if light.shadows else ORANGE

            if frame % 120 == 0:
                print(
                    f"at ({player.position.x:.1f}, {player.position.y:.1f}, "
                    f"{player.position.z:.1f}) m - {clock_face(carbine.atmosphere.time())} "
                    f"- {light.quality} {light.style} - {as_text(light)}"
                )

            frame += 1
            time.sleep(1.0 / 120.0)
    finally:
        # One call takes the whole world with it: every node and rung emptied out of
        # the scene, the player handing its camera back, the window gone.
        carbine.end()


if __name__ == "__main__":
    main()

"""The environment: `carbine.atmosphere`.

    python examples/atmosphere.py

Every key here is the engine's own. `carbine.atmosphere.new(kind)` is the fast path
onto the particle generator: it writes a crowd and hangs it on the sky, so the handle
it hands back is a `Particle` and `count`, `shape`, `speed`, `color` and `active` are
all its ordinary knobs. The maths - how many streaks make a downpour, where the sun
stands at seven in the morning - happens in Rust, so this file only says what the
weather is doing.

    1 2 3 4   rain, snow, dust, ash     each one again switches it off
    up/down   a tenth less or more of whichever weathers are falling
    n / b     run the clock on, or back: the sun moves and the sky changes colour
    f / v     thicken the fog, or clear it
    G  gale   a wind down -x: the streaks lean by themselves, because the engine
              already knows which way the air is moving
    E         an earthquake, to shake the eyes the weather is drawn past
    X         stop the air and the storms both

Fog is the knob to walk into. It is not made of particles, so it never falls: it is
the air refusing to show far things, which is why the row of pillars at 12, 18, 24, 30
and 36 metres goes away one at a time while the crate beside you stays the same colour.
Rain, snow, dust and ash are the other way round - they fill the sky around wherever
the eyes stand, and they lean when a wind runs.

The weather is only a look, so nothing it paints pushes anything: the crates and the
player stay exactly where the solver left them until a disaster says otherwise.
"""

import time

import carbine

ORANGE = "#eb7a34"
NAVY = "#2f6cb3"

# The kinds the register can write, on the keys that ask for them.
KEYS = dict(zip("1234", ["rain", "snow", "dust", "ash"]))

# How much of a day the clock moves in a second while a key is held down. An hour in
# a second and a half is fast enough to see a dawn pass and slow enough to read it.
CLOCK_PER_SECOND = 0.7
FOG_PER_SECOND = 40.0

# Pillars down the -y axis, so the fog has distances to be rude about.
DISTANCES = [12.0, 18.0, 24.0, 30.0, 36.0]


def clock_face(hours: float) -> str:
    """The clock back the way a person reads it, so the printout matches the key
    press that asked for `"07:30"` in the first place."""
    wrapped = hours % 24.0
    return f"{int(wrapped):02d}:{int(round(wrapped % 1.0 * 60.0)) % 60:02d}"


def main() -> None:
    adapter = carbine.init(1280, 720, title="Carbine Air")
    width, height = carbine.viewport()
    print(
        f"{adapter} at {width}x{height}\n"
        "WASD walk - mouse look - space jump\n"
        "1 rain - 2 snow - 3 dust - 4 ash - up/down thickness\n"
        "n/b run the clock - f/v fog in and out - G gale - E quake - X calm\n"
        "ESC releases the mouse - close the window to quit"
    )
    print("weather the sky can wear:", ", ".join(carbine.atmosphere.kinds()))

    ground = carbine.new("plane", name="Ground")
    ground.size = (80.0, 80.0, 1.0)
    ground.color = "#3d4a3a"

    # One crate within arm's reach and a row of pillars going off into the distance:
    # near things keep their colour while the far end of the field is taken away.
    near = carbine.new("cube", name="Crate")
    near.size = (1.0, 1.0, 1.0)
    near.position = (2.0, -3.0, 0.5)
    near.color = "#8a6d3b"

    pillars = []
    for index, away in enumerate(DISTANCES):
        pillar = carbine.new("cube", name=f"Pillar {away:g}m")
        pillar.size = (1.6, 1.6, 3.0 + index)
        pillar.position = (0.0, -away, (3.0 + index) / 2.0)
        pillar.color = NAVY
        pillar.gravity = False
        pillars.append(pillar)

    # Four weathers written once and hung on the sky, each remembered at its own
    # thickness so switching one back on gives it the crowd it was last raining.
    written = {}
    for key, kind in KEYS.items():
        weather = carbine.atmosphere.new(kind, name=kind.capitalize())
        weather.active = False
        written[key] = (weather, weather.count)

    gale = carbine.physics.new("wind", name="Gale")
    gale.direction = (-1.0, 0.0, 0.0)
    gale.magnitude = 6.0
    gale.reach = 60.0

    quake = carbine.physics.new("earthquake", name="Earthquake")
    quake.magnitude = 6.0
    quake.epicentre = (0.0, -12.0, 0.5)

    panel = carbine.ui.new("rectangle", name="Hud")
    panel.position = (120, height - 44, 0)
    panel.size = (300, 36, 1)
    panel.color = "#221e1b"
    panel.corner = 10
    panel.transparent = 190

    # How thick the air is, and what the weather is being shoved by.
    thickness = carbine.ui.new("rectangle", name="Air", parent=panel)
    thickness.position = (-90, -8, 0)
    thickness.size = (4.0, 10.0, 1)
    thickness.color = ORANGE
    thickness.corner = 5

    blown = carbine.ui.new("rectangle", name="Wind", parent=panel)
    blown.position = (-90, 8, 0)
    blown.size = (4.0, 10.0, 1)
    blown.color = NAVY
    blown.corner = 5

    crosshair = carbine.ui.new("circle", name="Crosshair")
    crosshair.position = (width / 2.0, height / 2.0, 0)
    crosshair.size = (10.0, 10.0, 1)
    crosshair.color = ORANGE
    crosshair.transparent = 170

    player = carbine.player()
    player.position = (0.0, 0.0, 0.0)
    player.enabled = True

    frame = 0
    try:
        while carbine.running():
            carbine.render()
            step = carbine.dt()

            # One key per kind, so a second press is the one that stops it.
            for key, (weather, _) in written.items():
                if carbine.pressed(key):
                    weather.active = not weather.active
                    print(
                        f"{weather.name} {'on' if weather.active else 'off'} - "
                        f"{weather.count} drops at {weather.speed:g} m/s"
                    )

            falling = carbine.atmosphere.running()
            if carbine.pressed("up") or carbine.pressed("down"):
                ratio = 1.1 if carbine.pressed("up") else 1.0 / 1.1
                for weather in falling:
                    weather.count = max(1, round(weather.count * ratio))
                    print(f"{weather.name} thickness {weather.count} drops")

            # The clock is the one knob that has no weather in it: it moves the sun
            # and recolours the sky, and nothing is born or destroyed.
            if carbine.key("n") or carbine.key("b"):
                hours = carbine.atmosphere.time()
                moving = CLOCK_PER_SECOND * step
                if carbine.key("b"):
                    moving = -moving
                print(f"{clock_face(carbine.atmosphere.time(hours + moving))}")

            if carbine.key("f") or carbine.key("v"):
                air = carbine.atmosphere.fog
                moving = FOG_PER_SECOND * step
                if carbine.key("v"):
                    moving = -moving
                carbine.atmosphere.fog = air + moving

            for key, storm in (("g", gale), ("e", quake)):
                if carbine.pressed(key):
                    if storm.running:
                        storm.stop()
                    else:
                        storm.start()
                    print(f"{storm.name} {'on' if storm.running else 'off'}")

            if carbine.pressed("x"):
                carbine.atmosphere.stopAll()
                carbine.physics.stopAll()
                print("clear air")

            storms = carbine.physics.running()
            # The same shove the solver lays on the eyes is what leans the streaks,
            # so the bar is the reason the rain is slanted.
            push = 0.0
            for storm in storms:
                shove = storm.accelerationAt(player.position)
                push = max(push, (shove.x**2 + shove.y**2 + shove.z**2) ** 0.5)

            heaviest = max(
                (weather.count / written[k][1] for k, (weather, _) in written.items() if weather.active),
                default=0.0,
            )
            thickness.size = (4.0 + 110.0 * min(1.0, heaviest), ..., ...)
            thickness.color = ORANGE if falling else NAVY
            blown.size = (4.0 + 110.0 * min(1.0, push / 12.0), ..., ...)
            blown.color = NAVY if push > 0.0 else ORANGE

            if frame % 120 == 0:
                air = ", ".join(
                    f"{weather.name} {weather.count} drops at "
                    f"{tuple(round(channel, 2) for channel in weather.color)}"
                    for weather in falling
                )
                print(
                    f"at ({player.position.x:.1f}, {player.position.y:.1f}, "
                    f"{player.position.z:.1f}) m - {clock_face(carbine.atmosphere.time())} "
                    f"- fog {carbine.atmosphere.fog:g}% - {push:.1f} m/s^2 at the eyes - "
                    f"weather: {air or 'clear'}"
                )

            frame += 1
            time.sleep(1.0 / 120.0)
    finally:
        carbine.atmosphere.stopAll()
        carbine.physics.stopAll()
        for weather, _ in written.values():
            weather.destroy()
        gale.destroy()
        quake.destroy()
        player.enabled = False
        crosshair.destroy()
        panel.destroy()
        for pillar in pillars:
            pillar.destroy()
        near.destroy()
        ground.destroy()
        carbine.close()


if __name__ == "__main__":
    main()

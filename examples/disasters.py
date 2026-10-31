"""Out of the box natural disasters.

    python examples/disasters.py

Keys switch the weather on and under, and every one of them is the engine's own:
`carbine.physics.new("earthquake")` writes a rule the solver asks about once a
step, and `start()` is what makes the world feel it. Nothing here does the maths -
the script only says what is happening, where, and how hard.

    E  earthquake        the ground shove sideways and up, and the camera with it
    W  wind              one way, with gusts, pointed down -x
    T  tornado           a funnel bolted to the Spinner: drawn in, spun around,
                        and thrown up inside the tube
    B  blast             outward from one point, once, and over in half a second
    1 / 2                a step down or up the scale, on whatever is running
    X  stop everything   the crates keep the speed the weather left them with, and
                        the floor's grip spends it

The weather only shoves a body the solver is allowed to move, so the crates ask
for gravity and the ground, the spinner and the player's feet hold their ground.
Wind resists the walk instead of throwing the player about, because the controller
chases its own target speed. An updraft is different: nothing argues with the
height axis, so stand inside the funnel and go up.
"""

import time

import carbine

ORANGE = "#eb7a34"
NAVY = "#2f6cb3"
CRATE = "#8a6d3b"

# The nine places the crates were put down, which is what "how much has the weather
# done to the yard" is measured against.
HOME = [(-6.0 + 3.0 * column, -4.0 + 3.0 * row) for row in range(3) for column in range(3)]

# kind, name, how many meters away it stops being felt, and which way it points.
# A reach of zero is the whole world.
WEATHER = [
    ("earthquake", "Earthquake", 0.0, None),
    ("wind", "Gale", 30.0, (-1.0, 0.0, 0.0)),
    ("tornado", "Fune", 24.0, None),
    ("blast", "Blast", 20.0, None),
]

KEYS = dict(zip("ewtb", WEATHER))


def how_hard(at, storms):
    """The strongest shove any running disaster lays on one point, in metres per
    second squared - asked the way the solver asks it."""
    loudest = 0.0
    for storm in storms:
        shove = storm.accelerationAt(at)
        loudest = max(loudest, (shove.x**2 + shove.y**2 + shove.z**2) ** 0.5)
    return loudest


def main() -> None:
    adapter = carbine.init(1280, 720, title="Carbine Weather")
    width, height = carbine.viewport()
    print(
        f"{adapter} at {width}x{height}\n"
        "WASD walk - mouse look - space jump\n"
        "E earthquake - W wind - T tornado - B blast - 1/2 magnitude - X stop all\n"
        "ESC releases the mouse - close the window to quit"
    )
    print("weather the engine can write:", ", ".join(carbine.physics.kinds()))

    ground = carbine.new("plane", name="Ground")
    ground.size = (48.0, 48.0, 1.0)
    ground.color = "#3d4a3a"

    crates = []
    for index, (east, north) in enumerate(HOME):
        crate = carbine.new("cube", name=f"Crate {index}")
        crate.size = (1.0, 1.0, 1.0)
        crate.position = (east, north, 0.5)
        crate.color = CRATE
        # The invitation the weather answers: gravity on, nothing anchored.
        crate.gravity = True
        crates.append(crate)

    # A fan to bolt a funnel to. A disaster that rides a node takes its place in the
    # world as its own centre, so wherever the fan stands is where the tube stands.
    fan = carbine.new("sphere", name="Spinner")
    fan.size = (1.4, 1.4, 1.4)
    fan.position = (7.0, 4.0, 0.7)
    fan.color = NAVY

    written = {}
    for key, (kind, name, reach, direction) in KEYS.items():
        event = carbine.physics.new(kind, name=name)
        event.magnitude = 6.0
        event.reach = reach
        event.epicentre = (0.0, 0.0, 0.5)
        if kind == "tornado":
            event.node = fan
        if direction is not None:
            event.direction = direction
        written[key] = event

    panel = carbine.ui.new("rectangle", name="Hud")
    panel.position = (120, height - 44, 0)
    panel.size = (300, 36, 1)
    panel.color = "#221e1b"
    panel.corner = 10
    panel.transparent = 190

    # One bar for the weather and one for what it has done: the shove at the
    # player's feet, and the mean distance the crates have travelled from home.
    dial = carbine.ui.new("rectangle", name="Shove", parent=panel)
    dial.position = (-90, -8, 0)
    dial.size = (4.0, 10.0, 1)
    dial.color = ORANGE
    dial.corner = 5

    damage = carbine.ui.new("rectangle", name="Yard", parent=panel)
    damage.position = (-90, 8, 0)
    damage.size = (4.0, 10.0, 1)
    damage.color = NAVY
    damage.corner = 5

    crosshair = carbine.ui.new("circle", name="Crosshair")
    crosshair.position = (width / 2.0, height / 2.0, 0)
    crosshair.size = (10.0, 10.0, 1)
    crosshair.color = ORANGE
    crosshair.transparent = 170

    player = carbine.player()
    player.position = (0.0, -9.0, 0.0)
    player.enabled = True

    frame = 0
    try:
        while carbine.running():
            carbine.render()

            # One switch per kind, so a second press is the one that stops it.
            for key, event in written.items():
                if carbine.pressed(key):
                    if event.running:
                        event.stop()
                    else:
                        event.start()
                    print(f"{event.name} {'on' if event.running else 'off'}")

            storms = carbine.physics.running()
            for key in ("1", "2"):
                if not carbine.pressed(key):
                    continue
                steps = -1.0 if key == "1" else 1.0
                for event in storms:
                    event.magnitude = event.magnitude + steps
                    print(
                        f"{event.name} magnitude {event.magnitude:g} - "
                        f"{event.accelerationAt((0.0, 0.0, 0.5)).x:.1f} m/s^2 at the epicentre"
                    )
            if carbine.pressed("x"):
                carbine.physics.stopAll()
                print("calm")

            shove = how_hard(player.position, storms)
            dial.size = (4.0 + 110.0 * min(1.0, shove / 12.0), ..., ...)
            dial.color = ORANGE if storms else NAVY

            travelled = sum(
                ((box.position.x - east) ** 2 + (box.position.y - north) ** 2) ** 0.5
                for box, (east, north) in zip(crates, HOME)
            ) / len(crates)
            damage.size = (4.0 + min(110.0, 11.0 * travelled), ..., ...)

            if frame % 120 == 0:
                weather = ", ".join(
                    f"{event.name} {event.magnitude:g} at {event.elapsed:.1f}s"
                    for event in storms
                )
                print(
                    f"at ({player.position.x:.1f}, {player.position.y:.1f},"
                    f" {player.position.z:.1f}) m, "
                    f"{shove:.1f} m/s^2 at the feet, crates {travelled:.1f} m from home, "
                    f"weather: {weather or 'none'}"
                )

            frame += 1
            time.sleep(1.0 / 120.0)
    finally:
        # The weather, the props and the window all leave together.
        carbine.end()


if __name__ == "__main__":
    main()

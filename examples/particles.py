"""The particle generator: `carbine.procedural.particles()`.

    python examples/particles.py

Three crowds written by the same generator, and none of them is a weather switch.
Each one is a node: the rain and the snow stand in the sky by themselves, and the
embers hang off a chimney, so the box they fly out of is the chimney's place in the
world rather than a coordinate someone typed.

The knobs are the ones a script gets - shape, count, size, speed, direction, area,
lifetime, seed, colour, image, collide, splash, recycle - and the maths behind them
lives in Rust: where a drop is born, how far it travels in a step, what stops it and
what it throws back when it does. So this file only says what is flying.

    1 2 3      rain, snow, embers      each key again switches that crowd off
    b          a burst: one throw of sparks that runs out on its own
    up/down    a hundred more or fewer drops in the rain
    c          the rain through a collider, or straight past everything
    s          splash on and off: what a drop does when it lands
    d          a new dice, so the same weather comes back differently
    x          every crowd out of the air

Walk the rain behind the chimney and the drops go out of the frame one by one,
because they are drawn in the world and not over it. Turn the collider off and they
fall straight through the floor instead of bringing up short on it, which is the
number the bar at the bottom of the screen is counting.
"""

import time

import carbine

ORANGE = "#eb7a34"
ICE = "#bad3e6"
SNOW = "#fff8f0"

# The chimney the embers ride, and the height their box is centred at: a cube of six
# metres standing on the ground has its top at six.
CHIMNEY_TOP = 6.0


def main() -> None:
    adapter = carbine.init(1280, 720, title="Carbine Crowds")
    width, height = carbine.viewport()
    print(
        f"{adapter} at {width}x{height}\n"
        "WASD walk - mouse look - space jump\n"
        "1 rain - 2 snow - 3 embers - b burst - up/down count - c collider - s splash "
        "- d dice - x clear\n"
        "ESC releases the mouse - close the window to quit"
    )
    print("cuts a drop can take:", ", ".join(carbine.procedural.shapes()))

    ground = carbine.new("plane", name="Ground")
    ground.size = (80.0, 80.0, 1.0)
    ground.color = "#3d4a3a"

    chimney = carbine.new("cube", name="Chimney")
    chimney.size = (1.6, 1.6, CHIMNEY_TOP)
    chimney.position = (0.0, -14.0, CHIMNEY_TOP / 2.0)
    chimney.color = "#6b5545"

    crate = carbine.new("cube", name="Crate")
    crate.size = (1.4, 1.4, 1.4)
    crate.position = (3.0, -6.0, 0.7)
    crate.color = "#8a6d3b"

    rain = carbine.procedural.particles(
        name="Rain",
        count=1600,
        shape="streak",
        size=0.06,
        speed=9.0,
        area=(40.0, 40.0, 18.0),
        lifetime=4.0,
        color=ICE,
        splash=3,
        seed="a wet october",
    )
    rain.position = (0.0, -10.0, 12.0)

    snow = carbine.procedural.particles(
        name="Snow",
        count=700,
        shape="circle",
        size=0.09,
        speed=1.2,
        area=(36.0, 36.0, 14.0),
        lifetime=14.0,
        color=SNOW,
        seed="slow february",
    )
    snow.position = (0.0, -10.0, 10.0)
    snow.active = False

    # A child, so the box moves with the branch: embers are born at the top of the
    # chimney and rise along it rather than along the world's down.
    embers = carbine.procedural.particles(
        name="Embers",
        parent=chimney,
        shape="circle",
        count=160,
        size=0.07,
        speed=1.8,
        direction=(0.0, 0.0, 1.0),
        area=(1.2, 1.2, 0.4),
        lifetime=3.0,
        color=ORANGE,
        collide=False,
        recycle=True,
        seed="hearth",
    )
    embers.position = (0.0, 0.0, CHIMNEY_TOP / 2.0 + 0.4)

    crowds = {"1": rain, "2": snow, "3": embers}
    bursts = []

    panel = carbine.ui.new("rectangle", name="Hud")
    panel.position = (140, height - 44, 0)
    panel.size = (320, 52, 1)
    panel.color = "#221e1b"
    panel.corner = 10
    panel.transparent = 190

    flying = carbine.ui.new("rectangle", name="In the air", parent=panel)
    flying.position = (-100, 10, 0)
    flying.size = (4.0, 10.0, 1)
    flying.color = ICE
    flying.corner = 5

    hits = carbine.ui.new("rectangle", name="Brought up short", parent=panel)
    hits.position = (-100, -10, 0)
    hits.size = (4.0, 10.0, 1)
    hits.color = ORANGE
    hits.corner = 5

    crosshair = carbine.ui.new("circle", name="Crosshair")
    crosshair.position = (width / 2.0, height / 2.0, 0)
    crosshair.size = (10.0, 10.0, 1)
    crosshair.color = ORANGE
    crosshair.transparent = 170

    player = carbine.player()
    player.position = (0.0, 0.0, 0.0)
    player.enabled = True

    burst = 0
    frame = 0
    try:
        while carbine.running():
            carbine.render()

            for key, crowd in crowds.items():
                if carbine.pressed(key):
                    crowd.active = not crowd.active
                    print(
                        f"{crowd.name} {'on' if crowd.active else 'off'} - "
                        f"{crowd.count} {crowd.shape} drops of {crowd.size:g} m "
                        f"at {crowd.speed:g} m/s"
                    )

            if carbine.pressed("b"):
                # One crowd, thrown once: recycle is off, so it runs out by itself.
                burst += 1
                sparks = carbine.procedural.particles(
                    name=f"Burst {burst}",
                    parent=chimney,
                    shape="square",
                    count=120,
                    size=0.08,
                    speed=5.0,
                    direction=(0.0, 0.0, 1.0),
                    area=(0.6, 0.6, 0.3),
                    lifetime=1.2,
                    color=ORANGE,
                    splash=2,
                    recycle=False,
                    seed=burst,
                )
                sparks.position = (..., ..., CHIMNEY_TOP / 2.0 + 0.5)
                bursts.append(sparks)
                print(f"burst {burst} thrown - {sparks.inAir} sparks, and then none")

            if carbine.pressed("up"):
                rain.count = rain.count + 100
                print(f"rain now {rain.count} drops")
            if carbine.pressed("down"):
                rain.count = max(0, rain.count - 100)
                print(f"rain now {rain.count} drops")

            if carbine.pressed("c"):
                rain.collide = not rain.collide
                rain.restart()
                print(f"rain {'stopped by' if rain.collide else 'through'} the world")

            if carbine.pressed("s"):
                rain.splash = 0 if rain.splash else 3
                print(f"splash {rain.splash} droplets back per hit")

            if carbine.pressed("d"):
                rain.seed = rain.seed + 1
                print(f"rain re-diced to seed {rain.seed}")

            if carbine.pressed("x"):
                for crowd in crowds.values():
                    crowd.active = False
                print("clear air")

            air = sum(crowd.inAir for crowd in crowds.values())
            hits_seen = max(crowd.impacts for crowd in crowds.values())
            flying.size = (4.0 + 200.0 * min(1.0, air / 2400.0), ..., ...)
            hits.size = (4.0 + 200.0 * min(1.0, hits_seen / 60.0), ..., ...)

            if frame % 120 == 0:
                print(
                    f"at ({player.position.x:.1f}, {player.position.y:.1f}, "
                    f"{player.position.z:.1f}) m, {air} things in the air, "
                    f"{hits_seen} brought up short last step, "
                    f"rain falls {'onto the world' if rain.collide else 'straight through it'}"
                )

            frame += 1
            time.sleep(1.0 / 120.0)
    finally:
        # Crowds, bursts, props, weather and the window: one call takes all of it.
        carbine.end()


if __name__ == "__main__":
    main()

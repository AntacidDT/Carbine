"""Walk around a 3D world under your own control.

    python examples/player.py

WASD walks, the mouse looks, space jumps. None of that is scripted here: the
engine reads the keyboard and drives the camera during every `carbine.render()`.
The script only says who the player is and how fast they may go.

Escape hands the pointer back to the desktop, a click asks for it again, and F
switches between the player's eyes and a camera behind them.
"""

import time

import carbine

ORANGE = "#eb7a34"
NAVY = "#2f6cb3"

# Where the pillars stand and how tall they are, in meters. Nothing collides with
# them yet: the world has one floor and these are for depth.
PILLARS = [(-4.0, 3.5, 2.4), (2.5, 6.0, 1.2), (7.0, -2.0, 3.6), (-6.5, -5.0, 1.8)]


def main() -> None:
    adapter = carbine.init(1280, 720, title="Carbine Player")
    width, height = carbine.viewport()
    print(
        f"{adapter} at {width}x{height}\n"
        "WASD walk - mouse look - space jump - F camera - ESC releases the mouse - close to quit"
    )

    ground = carbine.new("plane", name="Ground")
    ground.size = (48.0, 48.0, 1.0)
    ground.color = "#3d4a3a"

    pillars = []
    for index, (x, y, tall) in enumerate(PILLARS):
        pillar = carbine.new("cube", name=f"Pillar {index}")
        pillar.size = (1.2, 1.2, tall)
        pillar.position = (x, y, tall / 2.0)
        pillar.color = NAVY if index % 2 else "#6b7280"
        pillars.append(pillar)

    # A crate the solver actually owns: gravity is on, so each frame drops it and
    # rests it on the ground plane.
    crate = carbine.new("cube", name="Crate")
    crate.size = (1.0, 1.0, 1.0)
    crate.position = (3.0, -3.0, 6.0)
    crate.color = "#8a6d3b"
    crate.gravity = True

    body = carbine.new("cube", name="Body")
    body.size = (0.6, 0.6, 1.7)
    body.color = ORANGE

    player = carbine.player()
    player.body = body
    player.position = (0.0, -3.0, 0.0)
    player.speed = 5.0
    player.enabled = True

    panel = carbine.ui.new("rectangle", name="Hud")
    panel.position = (110, height - 44, 0)
    panel.size = (200, 36, 1)
    panel.color = "#221e1b"
    panel.corner = 10
    panel.transparent = 190

    # A bar that fills as the player picks up speed, which is the engine chasing
    # the target velocity rather than snapping to it.
    bar = carbine.ui.new("rectangle", name="Speed", parent=panel)
    bar.position = (-80, 0, 0)
    bar.size = (4.0, 12.0, 1)
    bar.color = ORANGE
    bar.corner = 6

    crosshair = carbine.ui.new("circle", name="Crosshair")
    crosshair.position = (width / 2.0, height / 2.0, 0)
    crosshair.size = (10.0, 10.0, 1)
    crosshair.color = ORANGE
    crosshair.transparent = 170

    frame = 0
    try:
        while carbine.running():
            carbine.render()

            # Input the engine does not own: F trades the eyes for a camera behind
            # the body, and the crosshair only means something in the eyes.
            if carbine.pressed("f"):
                player.mode = "third" if player.mode == "first" else "first"
            crosshair.visible = player.mode == "first"

            velocity = player.velocity
            pace = (velocity.x**2 + velocity.y**2) ** 0.5
            bar.size = (4.0 + 110.0 * min(1.0, pace / player.speed), ..., ...)

            if frame % 120 == 0:
                where = player.position
                print(
                    f"at ({where.x:.1f}, {where.y:.1f}, {where.z:.1f}) m, "
                    f"{pace:.1f} m/s, {player.mode} person, "
                    f"mouse {'held' if player.grabMouse else 'free'}"
                )

            frame += 1
            time.sleep(1.0 / 120.0)
    finally:
        player.enabled = False
        crosshair.destroy()
        panel.destroy()
        body.destroy()
        crate.destroy()
        for pillar in pillars:
            pillar.destroy()
        ground.destroy()
        carbine.close()


if __name__ == "__main__":
    main()

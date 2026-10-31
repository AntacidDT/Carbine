"""A real 3D scene: a ground plane, solid objects and a camera that walks around
them, with a HUD element pinned over the top.

    python examples/scene3d.py

The world is z-up, so `size` is (length, width, height) and `position.z` is how
high something sits. Any axis of a vector can be left out with `...`, which keeps
the value the object already has.
"""

import math
import time

import carbine

ORANGE = "#eb7a34"


def main() -> None:
    adapter = carbine.init(1280, 720, title="Carbine 3D")
    width, height = carbine.viewport()
    print(f"{adapter} at {width}x{height} - close the window to quit")

    ground = carbine.new("plane", name="Ground")
    ground.size = (24.0, 24.0, 1.0)
    ground.color = "#3d4a3a"

    tower = carbine.new("cube", name="Tower")
    tower.size = (1.6, 1.6, 3.2)
    tower.position = (0.0, 0.0, 1.6)
    tower.color = ORANGE

    # A translucent slab, so depth is visible through it as well as around it.
    slab = carbine.new("cube", name="Slab")
    slab.size = (4.0, 0.6, 0.4)
    slab.position = (0.0, -2.6, 1.4)
    slab.color = "#7fb2ff"
    slab.transparent = 110

    ball = carbine.new("sphere", name="Ball")
    ball.size = (1.2, ..., 1.2)
    ball.position = (-3.0, 1.4, 0.6)

    camera = carbine.camera()
    camera.fov = 55.0
    camera.near = 0.1
    camera.far = 200.0

    hud = carbine.ui.new("rectangle", name="Hud")
    hud.position = (110, height - 60, 0)
    hud.size = (180, 44, 1)
    hud.color = "#221e1b"
    hud.corner = 12
    hud.transparent = 200

    marker = carbine.ui.new("square", name="Marker", parent=hud)
    marker.position = (-70, 0, 0)
    marker.size = (20, 20, 1)
    marker.color = ORANGE
    marker.corner = 6

    frame = 0
    try:
        while carbine.running():
            second = frame / 60.0

            # The camera circles the tower and looks slightly down at it.
            angle = second * 0.35
            camera.position = (math.sin(angle) * 9.0, -math.cos(angle) * 9.0, 4.5)
            camera.lookAt((0.0, 0.0, 1.4))

            tower.rotation = (..., ..., (tower.rotationEuler.z + 0.4) % 360.0)
            ball.position = (..., ..., 0.6 + 0.5 * math.sin(second * 1.6))
            marker.position = (-70 + 40.0 * math.sin(second * 2.0), 0, 0)

            carbine.render()
            frame += 1
            time.sleep(1.0 / 60.0)
    finally:
        carbine.end()


if __name__ == "__main__":
    main()

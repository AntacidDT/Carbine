"""The smallest thing Carbine can draw: one orange rectangle, slowly turning.

    python examples/rectangle.py

The engine's camera defaults to a perspective one standing out in the world, so
this flat demo aims it straight down the shape and switches it to orthographic,
which is the other thing the camera can do.
"""

import time

import carbine

adapter = carbine.init(1280, 720, title="Carbine 0.01")
print(f"rendering on {adapter} at {carbine.viewport()}")

camera = carbine.camera()
camera.mode = "orthographic"
camera.position = (0.0, 0.0, 6.0)
camera.target = (0.0, 0.0, 0.0)
camera.viewHeight = 4.0

card = carbine.new("rectangle", name="Card")
card.size = (2.0, 1.0, 1.0)

try:
    while carbine.running():
        card.rotationEuler.z = (card.rotationEuler.z + 0.6) % 360.0
        carbine.render()
        time.sleep(1.0 / 60.0)
finally:
    card.destroy()
    carbine.close()

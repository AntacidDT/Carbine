"""The smallest thing Carbine can draw: one orange rectangle, slowly turning.

    python examples/rectangle.py
"""

import time

import carbine

adapter = carbine.init(1280, 720, title="Carbine 0.01")
print(f"rendering on {adapter} at {carbine.viewport()}")

carbine.camera().viewHeight = 4.0

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

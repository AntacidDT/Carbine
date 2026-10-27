"""The UI overlay: rounded corners, polygons, outlines and transparency.

Every position and size here is in pixels from the window's top left corner, so
the camera plays no part in where anything lands.
"""

import time

import carbine

ORANGE = "#eb7a34"


def main() -> None:
    adapter = carbine.init(960, 540, title="Carbine UI")
    width, height = carbine.viewport()
    print(f"{adapter} at {width}x{height} - close the window to quit")

    panel = carbine.ui.new("rectangle", name="Panel")
    panel.position = (width * 0.5, height * 0.5, 0)
    panel.size = (440, 300, 1)
    panel.color = "#221e1b"
    panel.cornerRadius = 24

    button = carbine.ui.new("square", name="Button", parent=panel)
    button.position = (-120, 0, 0)
    button.size = (150, 150, 1)
    button.color = ORANGE

    ring = carbine.ui.new("square", name="Ring", parent=panel)
    ring.position = (100, -45, 0)
    ring.size = (110, 110, 1)
    ring.color = "#7fb2ff"
    ring.outline = 6
    ring.transparent = 0

    badge = carbine.ui.new("circle", name="Badge", parent=panel)
    badge.position = (100, 60, 0)
    badge.size = (64, 64, 1)
    badge.color = "#34d17f"
    badge.transparent = 130

    triangle = carbine.ui.new("triangle", name="Triangle")
    triangle.position = (84, height - 84, 0)
    triangle.size = (96, 96, 1)
    triangle.color = "#ffffff"
    triangle.cornerRadius = 14

    frame = 0
    try:
        while carbine.running():
            # A corner radius that walks from soft to fully round and back.
            pulse = abs(1.0 - 2.0 * ((frame % 150) / 150.0))
            button.cornerRadius = 16.0 + 58.0 * pulse
            triangle.rotationEuler = (0.0, 0.0, (triangle.rotationEuler.z + 0.8) % 360.0)
            carbine.render()
            frame += 1
            time.sleep(1 / 60)
    finally:
        triangle.destroy()
        panel.destroy()
        carbine.close()


if __name__ == "__main__":
    main()

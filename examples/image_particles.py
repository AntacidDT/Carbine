"""Image particles: any picture on disk, worn by a crowd.

    python examples/image_particles.py path/to/any/image.png

Carbine loads one image format, so an authored picture is packed on the way in:
`carbine.packager.image()` turns it into a `.ktx2` of BC7 blocks beside the file it came
from, and that path is what the crowds wear - the same import Enginebay runs when a file
is dropped into a project, done here in real time. A `.ktx2` is already in the format the
engine reads and is used as it stands.

`carbine.procedural.particles(image=...)` hands the path to the core and the renderer puts
it on the GPU once, then draws every drop of that crowd from it - so the picture is in the
world, depth tested against the walls, dimmed by its own shade and blown away by a gust,
rather than pasted over the window.

Three crowds wear the same image differently: chips falling, a fountain thrown up, and a
slow drift. The drops are squares, because a square shows four corners of a picture; a
streak would stretch it along the way it falls.

    1 2 3      chips, fountain, drift      each key again switches that crowd off
    s          the image on, or the crowd back to its own colour
    up/down    bigger or smaller drops
    f          faster or slower through the air
    d          a new dice, so the same crowds come back differently
    x          every crowd out of the air

A file the Packager cannot read as a picture stops the demo with the reason: an image in
any other format is refused when it is named, not silently drawn flat.
"""

import os
import sys
import time

import carbine

ORANGE = "#eb7a34"
SKY = "#bad3e6"
CREAM = "#fff8f0"


def skin_of(path: str) -> str:
    """The packed image a crowd wears, converting an authored picture first and
    reporting what the bytes saved."""
    if path.endswith(".ktx2"):
        print(f"already packed: {path} in {os.path.getsize(path):,} bytes")
        return path

    packed = carbine.packager.image(path)
    before, after = os.path.getsize(path), os.path.getsize(packed)
    print(
        f"packed {path} -> {packed}: {before:,} bytes to {after:,} "
        f"({before / after:.1f}x smaller, BC7 with a full mip chain)"
    )
    return packed


def main(source: str) -> None:
    skin = skin_of(source)

    adapter = carbine.init(1280, 720, title="Carbine Image Particles")
    width, height = carbine.viewport()
    print(f"{adapter} at {width}x{height}")
    print(f"skin: {skin}")
    print(
        "WASD walk - mouse look - space jump\n"
        "1 chips - 2 fountain - 3 drift - s skin - up/down size - f speed - d dice "
        "- x clear\n"
        "ESC releases the mouse - close the window to quit"
    )

    ground = carbine.new("plane", name="Ground")
    ground.size = (80.0, 80.0, 1.0)
    ground.color = "#3d4a3a"

    plinth = carbine.new("cube", name="Plinth")
    plinth.size = (1.2, 1.2, 1.2)
    plinth.position = (0.0, -8.0, 0.6)
    plinth.color = "#6b5545"

    chips = carbine.procedural.particles(
        name="Chips",
        image=skin,
        shape="square",
        count=600,
        size=0.22,
        speed=4.0,
        area=(30.0, 30.0, 12.0),
        lifetime=5.0,
        color="#ffffff",
        collide=True,
        seed="a picture of rain",
    )
    chips.position = (0.0, -6.0, 9.0)

    fountain = carbine.procedural.particles(
        name="Fountain",
        image=skin,
        shape="square",
        count=400,
        size=0.18,
        speed=6.5,
        direction=(0.0, 0.0, 1.0),
        area=(1.0, 1.0, 0.4),
        lifetime=2.5,
        color="#ffffff",
        collide=True,
        splash=2,
        seed="a picture of water",
    )
    fountain.position = (0.0, -8.0, 1.3)
    fountain.active = False

    drift = carbine.procedural.particles(
        name="Drift",
        image=skin,
        shape="circle",
        count=300,
        size=0.3,
        speed=0.7,
        area=(26.0, 26.0, 10.0),
        lifetime=18.0,
        color=CREAM,
        seed="a picture of drift",
    )
    drift.position = (0.0, -8.0, 7.0)
    drift.active = False

    crowds = {"1": chips, "2": fountain, "3": drift}

    panel = carbine.ui.new("rectangle", name="Hud")
    panel.position = (150, height - 40, 0)
    panel.size = (300, 44, 1)
    panel.color = "#221e1b"
    panel.corner = 10
    panel.transparent = 190

    air = carbine.ui.new("rectangle", name="In the air", parent=panel)
    air.position = (-95, 10, 0)
    air.size = (4.0, 10.0, 1)
    air.color = SKY
    air.corner = 5

    skinned = carbine.ui.new("rectangle", name="Skinned", parent=panel)
    skinned.position = (-95, -10, 0)
    skinned.size = (4.0 + 200.0, 10.0, 1)
    skinned.color = ORANGE
    skinned.corner = 5

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

            for key, crowd in crowds.items():
                if carbine.pressed(key):
                    crowd.active = not crowd.active
                    print(
                        f"{crowd.name} {'on' if crowd.active else 'off'} - "
                        f"{crowd.count} {crowd.shape} drops of {crowd.size:g} m "
                        f"at {crowd.speed:g} m/s, skin {crowd.texture or 'its own colour'}"
                    )

            if carbine.pressed("s"):
                worn = chips.texture
                for crowd in crowds.values():
                    crowd.texture = None if worn else skin
                print(
                    "crowds painted by the image"
                    if chips.texture
                    else "crowds painted in their own colours"
                )

            if carbine.pressed("up") or carbine.pressed("down"):
                step = 0.02 if carbine.pressed("up") else -0.02
                for crowd in crowds.values():
                    crowd.size = max(0.02, crowd.size + step)
                print(f"drops now {chips.size:g} m across")

            if carbine.pressed("f"):
                for crowd in crowds.values():
                    crowd.speed = crowd.speed * (1.6 if crowd.speed < 3.0 else 0.4)
                print(f"chips fall at {chips.speed:g} m/s, the fountain at {fountain.speed:g}")

            if carbine.pressed("d"):
                for crowd in crowds.values():
                    crowd.seed = crowd.seed + 1
                    crowd.restart()
                print("every crowd re-diced")

            if carbine.pressed("x"):
                for crowd in crowds.values():
                    crowd.active = False
                print("clear air")

            flying = sum(crowd.inAir for crowd in crowds.values())
            air.size = (4.0 + 200.0 * min(1.0, flying / 1300.0), ..., ...)
            skinned.size = (4.0 + 200.0 * (1.0 if chips.texture else 0.0), ..., ...)

            if frame % 120 == 0:
                print(
                    f"at ({player.position.x:.1f}, {player.position.y:.1f}, "
                    f"{player.position.z:.1f}) m, {flying} things in the air, "
                    f"{chips.impacts} brought up short last step"
                )

            frame += 1
            time.sleep(1.0 / 120.0)
    finally:
        for crowd in crowds.values():
            crowd.destroy()
        for node in carbine.scene.root.children:
            if isinstance(node, carbine.Particle):
                node.destroy()
        player.enabled = False
        crosshair.destroy()
        panel.destroy()
        plinth.destroy()
        ground.destroy()
        carbine.close()


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("give the engine a picture: python examples/image_particles.py path/to/image.png")
        raise SystemExit(2)
    main(sys.argv[1])

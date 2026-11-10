"""Procedural maps: ground that is somewhere to stand instead of a flat plane.

    python examples/maps.py

The usual way to get a hill into a game is to model it, export it, and hope the
collider matches the picture. This is the same job done inside the engine:
`carbine.procedural.map()` sums a heightfield out of a seed and hangs it in the tree
as one node wearing the mesh it draws. The ground a walker or a falling body meets is
the height of the map under that spot, asked of the grid analytically - so the hills
are walked on without the map being cut into thousands of collider pieces.

A map is scenery: it holds its place and the solver never shoves it. Skin it the way
any object is skinned - here with a grass tile the texture generator paints - and the
tile is laid by the metre across the hills rather than stretched over them.

The same seed lays the same ground on this machine and on the four others in a
session, so a world named "carbine" is the world anyone who types "carbine" stands on.

    r            a new dice, so the same reach comes back as different hills
    q            low - medium - high, the grid the ground is counted on
    x / z        the hills stand taller / settle lower
    h            the grass skin off, or back on
    t            the reach of one tile: half a metre, a metre, two, or smeared

WASD walks, the mouse looks, space jumps. The crates take gravity and rest on the
hills they land on. Close the window to quit.
"""

import random
from pathlib import Path

import carbine

GENERATED = Path(__file__).resolve().parent / "generated"
ORANGE = "#eb7a34"
QUALITIES = tuple(carbine.procedural.mapQualities())
TILES = (1.0, 0.5, 2.0, 0.0)
REACH = (160.0, 160.0)


def main() -> None:
    carbine.fpsCap = 90
    carbine.monitorSync = True
    adapter = carbine.init(1280, 720, title="Carbine Maps")
    width, height = carbine.viewport()
    print(
        f"{adapter} at {width}x{height}\n"
        "WASD walk - mouse look - space jump\n"
        "R new seed - Q quality - X/Z relief - H skin - T tiling - close to quit"
    )

    # Morning light so the slopes read by their shade rather than flat overhead.
    carbine.atmosphere.time("07:30")
    carbine.atmosphere.sky().zenith = "#2f6cb3"

    grass = carbine.procedural.texture(
        "grass", quality="medium", seed="meadow", out=GENERATED
    )

    seed = "carbine"
    ground = carbine.procedural.map(
        name="Map", size=REACH, relief=12.0, quality="high", seed=seed
    )
    ground.texture = grass
    ground.textureTiling = 1.0

    # A handful of crates the solver owns: gravity drops each onto whichever hill is
    # under it, which is the map's floor and not the flat plane the world is born with.
    for index in range(6):
        crate = carbine.new("cube", name=f"Crate {index}")
        crate.size = (1.0, 1.0, 1.0)
        crate.position = (
            random.uniform(-20.0, 20.0),
            random.uniform(-20.0, 20.0),
            20.0 + index * 2.0,
        )
        crate.color = "#8a6d3b"
        crate.gravity = True
        crate.collisionType = "box"

    player = carbine.player()
    player.position = (0.0, -10.0, 24.0)
    player.speed = 6.0
    player.enabled = True

    fps_text = carbine.ui.new("Text", name="FPS")
    fps_text.position = (18, 12, 0)
    fps_text.size = 26
    fps_text.color = "Light Orange"
    fps_text.outline = True
    fps_text.outlineWidth = 2
    fps_text.content = "FPS --"

    info_text = carbine.ui.new("Text", name="Map")
    info_text.position = (18, 48, 0)
    info_text.size = 18
    info_text.color = "White"
    info_text.outline = True
    info_text.outlineWidth = 1
    info_text.content = ""

    help_text = carbine.ui.new("Text", name="Controls")
    help_text.position = (18, height - 30, 0)
    help_text.size = 16
    help_text.color = "White"
    help_text.outline = True
    help_text.outlineWidth = 1
    help_text.content = "WASD walk | R seed | Q quality | X/Z relief | H skin | T tiling"

    quality_index = QUALITIES.index("high")
    tile_index = 0
    skinned = True
    frame = 0
    try:
        while carbine.running():
            carbine.render()

            if carbine.pressed("r"):
                seed = random.getrandbits(64)
                ground.seed = seed
                print(f"new dice: {seed}")

            if carbine.pressed("q"):
                quality_index = (quality_index + 1) % len(QUALITIES)
                ground.quality = QUALITIES[quality_index]
                print(f"quality: {ground.quality}")

            if carbine.pressed("x"):
                ground.relief = ground.relief + 2.0
            if carbine.pressed("z"):
                ground.relief = max(0.0, ground.relief - 2.0)

            if carbine.pressed("h"):
                skinned = not skinned
                ground.texture = grass if skinned else None
                ground.color = "#3d5a2c" if not skinned else "#ffffff"

            if carbine.pressed("t"):
                tile_index = (tile_index + 1) % len(TILES)
                ground.textureTiling = TILES[tile_index]
                print(f"tiling: {TILES[tile_index]} m")

            fps_text.content = f"FPS {carbine.fps:.0f}"
            info_text.content = (
                f"seed {seed} | relief {ground.relief:.0f} m | "
                f"{ground.quality} | {ground.size[0]:.0f}x{ground.size[1]:.0f} m"
            )

            if frame % 120 == 0:
                where = player.position
                print(
                    f"at ({where.x:.1f}, {where.y:.1f}, {where.z:.1f}) m, "
                    f"{'grounded' if player.grounded else 'airborne'}, "
                    f"relief {ground.relief:.0f} m, {ground.quality}"
                )
            frame += 1
    finally:
        carbine.end()


if __name__ == "__main__":
    main()
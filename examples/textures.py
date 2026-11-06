"""Procedural textures: surfaces the engine paints, worn the moment they land.

    python examples/textures.py

The usual way to get a wall or a stretch of grass into a game is to find a website,
sort the pictures by licence, download the one that tiles, and hope the size suits.
This is the same job done inside the engine: `carbine.procedural.texture()` rolls a
surface from a seed, bakes its own relief into the colour, packs it as a `.ktx2` of
BC7 blocks and answers with the path - which is exactly what `obj.texture` takes.

Four surfaces are on offer and each has its own ways of going wrong: `grass` dries to
straw or takes moss, `sand` gathers dust or is crowded with pebbles, `rock` cracks or
greens over, `brick` is dusty or mossy. Nothing here is measured in pixels: `quality`
is one word for both the size of the tile and how far its detail is summed, so `low`
is 256 texels over three doublings and `high` is 1024 over seven, and six courses of
brick are six courses either way - a finer tile is the same wall seen closer.

All twelve variations start on two-metre cubes in a six-metre circle around you.
Plain, dusty and mossy brick are directly ahead, left to right; turn right for grass,
then rock, then sand. `o` shows only the plain cubes and `g` restores the full set.
Quality and dice refresh every visible sample together, and repeated requests reuse
the tile already baked at that quality and seed.

The tiles land in `examples/generated/`, named for everything asked for, so the same
calling twice writes over its own picture. A new dice means a new file, which is also
what makes the renderer read it again: a skin already uploaded is kept under the path
it came from. A tile is laid at a size rather than stretched to fit: it covers one
metre of whatever wears it, so the same brick sits on a two-metre cube and a ten-metre
wall at one scale. `t` cycles half a metre, a metre, two metres, or one picture across
the whole face. Smooth UV and tone variation reduce visible repetition on larger
faces; a face holding exactly one tile is left the way it was painted.

    1 2 3 4      the surface on the wall and the monolith
    o            only the plain comparison cubes
    g            all twelve comparison cubes (shown at startup)
    v            the next way that surface goes wrong, and back to plain
    q            low - medium - high
    r            a new dice, so the same surfaces come back differently
    h            the skins off, or back on
    t            the reach of one tile: half a metre, a metre, two, or smeared

WASD walks, the mouse looks, space jumps. Close the window to quit.
"""

import math
import os
import time
from pathlib import Path

import carbine

GENERATED = Path(__file__).resolve().parent / "generated"
SURFACES = tuple(carbine.procedural.textureTypes())
QUALITIES = ("low", "medium", "high")
# The reach of one tile, in metres of the thing wearing it. Nil is the other ask: the
# whole picture pulled across each face, the way a model's own UVs already are.
TILES = (1.0, 0.5, 2.0, 0.0)
ORANGE = "#eb7a34"
CREAM = "#fff8f0"

# The colour each surface wears when it is not skinned, so `h` shows what the picture
# was doing: a flat patch of it against the painted one.
FLAT = {"grass": "#3d5a2c", "sand": "#c8b183", "rock": "#6f6f72", "brick": "#964434"}

# Every tile this program has asked the engine for. A tile is wholly what its name
# says, so remembering the path is enough to never paint the same one twice.
baked: dict[tuple[str, str | None, str, int], str] = {}


def paint(surface: str, variation: str | None, quality: str, dice: int) -> str:
    """Asks the engine for one tile, or hands back the one it asked for already."""
    asked = (surface, variation, quality, dice)
    if asked in baked:
        return baked[asked]
    GENERATED.mkdir(exist_ok=True)
    started = time.monotonic()
    written = carbine.procedural.texture(
        surface, variation=variation, quality=quality, seed=dice, out=str(GENERATED)
    )
    baked[asked] = written
    print(
        f"{surface} {'plain' if variation is None else variation} {quality}: "
        f"{os.path.basename(written)} - {os.path.getsize(written):,} bytes "
        f"in {time.monotonic() - started:.3f} s"
    )
    return written


def main() -> None:
    adapter = carbine.init(1280, 720, title="Carbine Procedural Textures")
    carbine.fpsCap = 120
    width, height = carbine.viewport()
    print(f"{adapter} at {width}x{height}")
    wears = {
        surface: [None, *carbine.procedural.textureVariations(surface)]
        for surface in SURFACES
    }
    for surface in SURFACES:
        offered = ", ".join("plain" if wear is None else wear for wear in wears[surface])
        print(f"  {surface}: {offered}")
    print(
        "1 2 3 4 the surface - o each plain - g every variation - v variation\n"
        "q quality - r dice - h skins - t tile reach\n"
        "WASD walk - mouse look - space jump - close the window to quit"
    )

    floor = carbine.new("plane", name="Gallery Floor")
    floor.size = (20.0, 20.0, 1.0)
    floor.position = (0.0, 0.0, -0.02)
    floor.color = "#41403b"

    cells = []
    display_order = ("brick", *(surface for surface in SURFACES if surface != "brick"))
    for surface in display_order:
        for variation in wears[surface]:
            sample = carbine.new(
                "cube",
                name=f"{surface.title()} {'plain' if variation is None else variation}",
            )
            angle = math.radians(-60.0 - 30.0 * len(cells))
            sample.size = (2.0, 2.0, 2.0)
            sample.position = (6.0 * math.cos(angle), 6.0 * math.sin(angle), 1.0)
            sample.color = FLAT[surface]
            cells.append((surface, variation, sample))

    wall = carbine.new("cube", name="Wall")
    wall.size = (10.0, 0.5, 3.0)
    wall.position = (0.0, -9.0, 1.5)

    monolith = carbine.new("cube", name="Monolith")
    monolith.size = (1.6, 1.6, 4.0)
    monolith.position = (-5.0, -6.0, 2.0)

    worn = [wall, monolith, *(sample for _, _, sample in cells)]
    state = {
        "surface": "brick",
        "variation": None,
        "quality": "medium",
        "dice": 7,
        "tile": 1.0,
        "shown": True,
    }
    # The path each thing was last skinned with, so `h` can put the same tiles back
    # rather than asking the generator for a second copy of a surface it already made.
    skins: dict[int, str] = {}

    def wear(node, skin: str) -> None:
        node.texture = skin
        skins[node.id] = skin

    def show(everything: bool, quiet: bool = False) -> None:
        state["shown"] = everything
        wanted = [
            (surface, variation, sample)
            for surface, variation, sample in cells
            if everything or variation is None
        ]
        for _, variation, sample in cells:
            sample.visible = everything or variation is None
        if not quiet:
            print(
                f"{'every variation' if everything else 'each surface plain'}, "
                f"{state['quality']}, dice {state['dice']}: {len(wanted)} cubes"
            )
        started = time.monotonic()
        for surface, variation, sample in wanted:
            sample.color = FLAT[surface]
            wear(sample, paint(surface, variation, state["quality"], state["dice"]))
        print(
            f"  {len(wanted)} cubes up in {time.monotonic() - started:.2f} s, "
            f"clockwise from ahead: {', '.join(display_order)}"
        )

    def relayer() -> None:
        skin = paint(
            state["surface"], state["variation"], state["quality"], state["dice"]
        )
        for node in (wall, monolith):
            node.color = FLAT[state["surface"]]
            wear(node, skin)
        show(everything=state["shown"], quiet=True)

    relayer()

    panel = carbine.ui.new("rectangle", name="Hud")
    panel.position = (178, height - 46, 0)
    panel.size = (356, 56, 1)
    panel.color = "#221e1b"
    panel.corner = 10
    panel.transparent = 190

    bars = []
    for row, (label, color) in enumerate(
        [("Quality", ORANGE), ("Ruined", CREAM), ("Skinned", ORANGE)]
    ):
        bar = carbine.ui.new("rectangle", name=label, parent=panel)
        bar.position = (-118, 12 - 16 * row, 0)
        bar.size = (4.0, 10.0, 1)
        bar.color = color
        bar.corner = 5
        bars.append(bar)

    crosshair = carbine.ui.new("circle", name="Crosshair")
    crosshair.position = (width / 2.0, height / 2.0, 0)
    crosshair.size = (10.0, 10.0, 1)
    crosshair.color = ORANGE
    crosshair.transparent = 170

    fps_text = carbine.ui.new("Text", name="FPS")
    fps_text.position = (18, 12, 0)
    fps_text.size = 26
    fps_text.color = "Light Orange"
    fps_text.outline = True
    fps_text.outlineWidth = 2
    fps_text.content = "FPS --"

    help_text = carbine.ui.new("Text", name="Controls")
    help_text.position = (18, 48, 0)
    help_text.size = 17
    help_text.color = "White"
    help_text.outline = True
    help_text.outlineWidth = 1
    help_text.wordWrap = True
    help_text.width = width - 36
    help_text.content = "WASD walk | Mouse look | R new seed | H textures | Q quality | T tiling"

    player = carbine.player()
    player.position = (0.0, 0.0, 0.0)
    player.yaw = 270.0
    player.enabled = True

    frame = 0
    try:
        while carbine.running():
            carbine.render()

            for index, surface in enumerate(SURFACES):
                if carbine.pressed(str(index + 1)):
                    state["surface"] = surface
                    state["variation"] = None
                    relayer()

            if carbine.pressed("o"):
                show(everything=False)

            if carbine.pressed("g"):
                show(everything=True)

            if carbine.pressed("v"):
                asking = wears[state["surface"]]
                state["variation"] = asking[
                    (asking.index(state["variation"]) + 1) % len(asking)
                ]
                relayer()

            if carbine.pressed("q"):
                state["quality"] = QUALITIES[(QUALITIES.index(state["quality"]) + 1) % 3]
                relayer()

            if carbine.pressed("r"):
                state["dice"] += 1
                relayer()

            if carbine.pressed("h"):
                if wall.texture:
                    for node in worn:
                        node.texture = None
                    print("surfaces painted by their own colour")
                else:
                    for node in worn:
                        node.texture = skins.get(node.id)
                    print("surfaces skinned from the tiles already on disk")

            if carbine.pressed("t"):
                state["tile"] = TILES[(TILES.index(state["tile"]) + 1) % len(TILES)]
                for node in worn:
                    node.textureTiling = state["tile"]
                reach = state["tile"]
                print(
                    "one picture pulled across each face"
                    if reach == 0.0
                    else f"one tile every {reach:g} m, whatever the face is"
                )

            bars[0].size = (
                4.0 + 240.0 * (QUALITIES.index(state["quality"]) + 1) / 3.0,
                ...,
                ...,
            )
            bars[1].size = (4.0 + 240.0 * (state["variation"] is not None), ..., ...)
            bars[2].size = (4.0 + 240.0 * (1.0 if wall.texture else 0.0), ..., ...)

            if frame % 180 == 0:
                on_show = {False: "plain cubes", True: "all variation cubes"}
                print(
                    f"{state['surface']} "
                    f"{'plain' if state['variation'] is None else state['variation']} "
                    f"{state['quality']}, dice {state['dice']}, "
                    f"{state['tile']:g} m per tile, "
                    f"{on_show[state['shown']]} on show, at "
                    f"({player.position.x:.1f}, {player.position.y:.1f}, "
                    f"{player.position.z:.1f}) m"
                )

            fps_text.content = f"FPS {carbine.fps:.0f}"
            width, height = carbine.viewport()
            help_text.width = max(1, width - 36)
            crosshair.position = (width / 2.0, height / 2.0, 0)
            panel.position = (178, height - 46, 0)
            frame += 1
    finally:
        carbine.end()


if __name__ == "__main__":
    main()

"""An earthquake rig: a city, a fault you can walk, and a damage model.

    python examples/quakesim.py

The engine owns the ground motion. `carbine.physics.new("earthquake")` writes a rule
the solver asks about once a step, and `quake.node = fault` makes the epicentre
wherever the marker stands - so jogging the fault with WASD jogs the shake with it.
What this script owns is the other half: what a building is made of, how hard it can
be shoved before it loses storeys, and what happens when it loses them. That is a game
rule, not a mechanic, so it lives here rather than in the core.

The damage numbers were measured against the engine rather than invented, which is how
the dial ends up reading: a magnitude 5 or 6 is shrugged off, a 6.8 takes the thin
towers nearest the fault over a few seconds, a 7.5 guts the plan, and a 9.0 flattens a
retrofitted city along with everything else. Damage chases the peak of the shake the way
a seismograph does, because the ground only moves hard for instants at a time.

What you see of a quake is three things at once, and only one of them is the solver.
Loose bodies are shoved by the ground themselves; the towers also *lean* into the shove
here, the taller and thinner the further, and run back upright when the ground goes
quiet; and the view is thrown by it too - in the eyes the engine does both halves of it,
sliding them and turning them aside by whatever the ground is doing under your feet,
because a head that only slides leaves the horizon almost where it was. The orbit ring
is thrown by this script instead, since a script that owns a camera owns its shake.

    R  rupture            starts a shock and arms an aftershock sequence
    P  patrol / walk      patrol drives the fault; walk hands the camera to the player
                          (the rig opens on foot, in the eyes - ESC frees the mouse)
    M  fault under me     drops the epicentre at the player's feet
    V  first / third      the eyes, or a camera behind the body
    1 / 2                a step down or up the Richter dial
    3 / 4                a step in or out of the distance it is felt
    T  retrofit           every standing block gains its code check
    G  anchor             bedrock piles: the solver is told not to move them
    B  new city           carbine.destroy("all") and a fresh plan in its place
    X  still air          stops the ground and drops the aftershock queue

Nothing sleeps to pace the loop: `carbine.fpsCap` and `carbine.monitorSync` are the
frame limit, held by the engine inside `carbine.render()`. The whole run leaves with
one call, `carbine.end()`.
"""

import math
import random

import carbine

GRID = 5              # a five by five plan of lots
LOT = 9.0             # metres between the aisles
DRUM = 44             # samples on the seismograph
SECOND = 1.0 / 60.0

STANDING = "#8b93a6"
RETROFIT = "#79a3b2"
RUBBLE = "#4f4a44"
FAULT = "#eb7a34"
DUST = "#c9bfa8"
DARK = "#221e1b"

# The damage rule, measured against the engine's own ground motion rather than
# guessed: one step on the dial doubles the shove, so these are the numbers that
# make a magnitude 6 shrug, a 6.8 take the thin towers over a few seconds, a 7.5
# gut the plan, and a 9.0 flatten a retrofitted city too.
BRITTLE = 3.0         # the shove a stub shrugs off before storeys start going
LENDER = 5.0          # what being tall and thin costs on top of that
UPGRADE = 3.5         # what a retrofit buys back
FATIGUE = 5.0         # integrity points a second, per metre per second past the check
BLEED = 3.0           # how fast a remembered peak is forgotten once the ground is quiet

# What the shove is worth on screen. The core only ever answers how hard the ground
# pushes at one metre; how that looks is a rule of this rig, so it is written here.
LEAN_PER_ACCEL = 0.22     # degrees a tower leans, per metre per second squared
LEAN_MAX = 7.0            # past this a tower is pancaked rather than swayed
LEAN_SETTLE = 14.0        # how fast the lean chases the ground, per second
RING_PER_ACCEL = 0.09     # metres the patrol ring is thrown, per metre per second squared
RING_MAX = 1.6            # the most the orbit view is ever thrown off the fault


def shove(storms, at):
    """The hardest shove any running disaster lays on one point, as the vector the
    solver works in - which way as well as how hard, which is what a lean needs."""
    loudest = (0.0, 0.0, 0.0)
    for storm in storms:
        vector = storm.accelerationAt(at).toTuple()
        if sum(axis**2 for axis in vector) > sum(axis**2 for axis in loudest):
            loudest = vector
    return loudest


def felt(storms, at):
    """The same shove, measured: how many metres per second squared the ground is
    laying on one point, whichever way it is laying it."""
    return sum(axis**2 for axis in shove(storms, at)) ** 0.5


def code(block):
    """A building's own check: the hardest shove it may stand for a whole second
    before it starts losing storeys. Slenderness is what costs - a thin tower sways
    where a low slab does not - and a retrofit buys back a fixed amount of it."""
    slender = block["height"] / max(2.0 * block["footprint"], 0.5)
    return BRITTLE + (UPGRADE if block["retrofitted"] else 0.0) + LENDER / (1.0 + slender)


def shake(block, storms, dt):
    """One step of the damage rule, asked the way a seismograph reads a shake: the
    hardest instant so far, not the value under the feet right now. A building keeps
    that peak while the ground goes on moving and forgets it over a few quiet
    seconds, so an aftershock starts where the main shock left off and calm air
    spends nothing. Answers whether this is the step the block lost its storeys on."""
    if block["gone"]:
        return False
    remembered = max(0.0, block["peak"] - BLEED * dt)
    block["peak"] = max(felt(storms, block["node"].position), remembered)
    beaten = max(0.0, block["peak"] - code(block))
    block["integrity"] -= FATIGUE * beaten * dt
    if block["integrity"] > 0.0:
        return False
    block["integrity"] = 0.0
    return topple(block)


def sway(block, storms, dt):
    """The visible half of the ground motion: a tower leans into the shove, the
    taller and thinner it is the further, and the lean chases the ground a step
    behind rather than snapping to it - which is what makes it a sway instead of a
    tilt. When the ground goes quiet the lean runs back to nothing on its own, so
    the city that stood the shock up stands it up straight."""
    if block["gone"]:
        return
    push = shove(storms, block["node"].position)
    slender = block["height"] / max(2.0 * block["footprint"], 0.5)
    gain = LEAN_PER_ACCEL * (1.0 + slender)
    rate = min(1.0, LEAN_SETTLE * dt)
    for axis, want in enumerate((gain * push[0], gain * push[1])):
        limit = max(-LEAN_MAX, min(LEAN_MAX, want))
        block["lean"][axis] += (limit - block["lean"][axis]) * rate

    # Leaning east is a turn about the north axis and leaning north about the east
    # one; the tower still pivots on its own middle, which the solver owns.
    block["node"].rotationEuler = (block["lean"][1], -block["lean"][0], 0.0)


def topple(block):
    """What a failure looks like: the tower is pancaked to a stump, takes on the
    colour of rubble, and throws a dust crowd from its own footprint."""
    node = block["node"]
    stump = max(0.8, 0.18 * block["height"])
    node.rotationEuler = (0.0, 0.0, 0.0)
    block["lean"] = [0.0, 0.0]
    node.size = (block["footprint"], block["depth"], stump)
    node.position = (node.position.x, node.position.y, stump / 2.0)
    node.color = RUBBLE
    block["gone"] = True
    block["dust"] = carbine.procedural.particles(
        name=f"Dust {block['label']}",
        parent=node,
        shape="circle",
        count=140,
        size=0.55,
        speed=1.5,
        area=(block["footprint"] + 2.0, block["depth"] + 2.0, 3.0),
        lifetime=2.6,
        color=DUST,
        recycle=False,
    )
    return True


def build_rng(seed):
    return random.Random(seed)


def standing(blocks):
    """How many of the plan's blocks still have all their storeys."""
    return sum(1 for block in blocks if not block["gone"])


def main() -> None:
    adapter = carbine.init(1440, 810, title="Carbine Quake Rig")
    width, height = carbine.viewport()
    carbine.fpsCap = 60
    carbine.monitorSync = True
    print(
        f"{adapter} at {width}x{height}, held to {carbine.fpsLimit} fps\n"
        "on foot in first person: WASD walk, mouse look, space jump, ESC frees the mouse\n"
        "R rupture - 1/2 magnitude - 3/4 reach - M fault under me - V first or third\n"
        "P patrol ring - T retrofit - G anchor - B new city - X still air\n"
        "close the window to quit"
    )

    camera = carbine.camera()
    player = carbine.player()
    # The rig opens on foot, in the eyes: the ground is then under the camera rather
    # than 38 metres away from it, so a rupture is felt as well as read off the drum.
    # Standing in the street rather than on a lot - the aisles run between the tower
    # lines at 4.5 metres - and ten metres from the fault, which is close enough to
    # take most of the shove rather than the thin end of it.
    player.mode = "first"
    player.position = (LOT / 2.0, -LOT, 0.0)
    player.yaw = 90.0
    player.enabled = True
    sky = carbine.atmosphere.sky()
    carbine.atmosphere.time("18:10")
    carbine.atmosphere.fog = 12.0
    sky.colors("#1f3557", "#c76b3d")
    sky.ambient = 0.35

    dial = 6.8
    reach = 34.0
    patrol = False
    plan = 0
    angle = 0.0
    frame = 0
    trace = [0.0] * DRUM

    def city(number):
        """The plan, the fault marker, the weather and the instruments. All of it is
        content, so `carbine.destroy("all")` is what takes a city away."""
        carbine.destroy("all")

        ground = carbine.new("plane", name="Ground")
        ground.size = (GRID * LOT + 12.0, GRID * LOT + 12.0, 1.0)
        ground.color = "#3d4a3a"
        ground.anchored = True

        rng = build_rng(f"city {number}")
        blocks = []
        edge = (GRID - 1) / 2.0
        for row in range(GRID):
            for column in range(GRID):
                # The centre lot is left as a plaza, which is where the fault stands.
                if row == column == GRID // 2:
                    continue
                east = (column - edge) * LOT
                north = (row - edge) * LOT
                footprint = rng.choice([3.0, 3.6, 4.4])
                depth = rng.choice([3.0, 4.2])
                tower = rng.choice([4.0, 7.0, 10.0, 14.0])
                block = {
                    "label": f"{chr(65 + row)}{column + 1}",
                    "footprint": footprint,
                    "depth": depth,
                    "height": tower,
                    "integrity": 100.0,
                    "peak": 0.0,
                    "lean": [0.0, 0.0],
                    "retrofitted": False,
                    "gone": False,
                    "dust": None,
                    "node": None,
                }
                node = carbine.new("cube", name=f"{block['label']}")
                node.size = (footprint, depth, tower)
                node.position = (east, north, tower / 2.0)
                node.color = STANDING
                # The invitation the ground answers: a body the solver may move, cut
                # as a box so a toppled tower leans on its neighbour instead of
                # through it.
                node.gravity = True
                node.collisionShape = "box"
                block["node"] = node
                blocks.append(block)

        fault = carbine.new("sphere", name="Fault")
        fault.size = (1.6, 1.6, 1.6)
        fault.position = (0.0, 0.0, 0.8)
        fault.color = FAULT

        quake = carbine.physics.new("earthquake", name="Main shock")
        quake.magnitude = dial
        quake.reach = reach
        quake.duration = 6.0
        # The shock rides the marker, so moving the fault moves the epicentre.
        quake.node = fault

        panel = carbine.ui.new("rectangle", name="Readout")
        panel.position = (150, 44, 0)
        panel.size = (280, 56, 1)
        panel.color = DARK
        panel.corner = 10
        panel.transparent = 200

        # One bar on the plate: the shove the ground is laying on the player's feet
        # right now, the same number the solver is working with.
        needle = carbine.ui.new("rectangle", name="Shove", parent=panel)
        needle.position = (-90, 0, 0)
        needle.size = (4.0, 8.0, 1)
        needle.color = FAULT
        needle.corner = 4

        drum = carbine.ui.new("rectangle", name="Seismograph")
        drum.position = (180, height - 100, 0)
        drum.size = (DRUM * 8 + 24, 84, 1)
        drum.color = DARK
        drum.corner = 8
        drum.transparent = 200

        bars = []
        for index in range(DRUM):
            bar = carbine.ui.new("rectangle", name=f"Trace {index}", parent=drum)
            bar.position = (-DRUM * 4 + 4 + 8 * index, 0, 0)
            bar.size = (5.0, 4.0, 1)
            bar.color = FAULT
            bar.corner = 2
            bars.append(bar)

        return {
            "blocks": blocks,
            "fault": fault,
            "quake": quake,
            "bars": bars,
            "needle": needle,
        }

    rig = city(plan)
    print(f"city of {len(rig['blocks'])} blocks; weather:", ", ".join(carbine.physics.kinds()))

    queue = []
    try:
        while carbine.running():
            carbine.render()
            dt = max(carbine.dt(), SECOND)
            storms = carbine.physics.running()
            shaking = felt(storms, rig["fault"].position)
            underfoot = felt(storms, player.position)

            # --- the instruments: the drum is the last DRUM samples of the fault ---
            trace = trace[1:] + [shaking]
            for index, (bar, sample) in enumerate(zip(rig["bars"], trace)):
                bar.size = (..., 4.0 + 70.0 * min(1.0, sample / 14.0), ...)
                bar.position = (..., 2.0 + 35.0 * min(1.0, sample / 14.0), ...)
            rig["needle"].size = (4.0 + 150.0 * min(1.0, underfoot / 12.0), ..., ...)

            # --- the city, one step of fatigue at a time ---
            for block in rig["blocks"]:
                lost = shake(block, storms, dt)
                sway(block, storms, dt)
                if lost:
                    print(
                        f"{block['label']} went: {shaking:.1f} m/s^2 at the fault, "
                        f"{standing(rig['blocks'])} still standing"
                    )

            # --- the aftershock sequence, which is a rule of this script ---
            if queue:
                wait, size = queue[0]
                queue[0] = (wait - dt, size)
                if queue[0][0] <= 0.0:
                    _, magnitude = queue.pop(0)
                    rig["quake"].magnitude = magnitude
                    rig["quake"].duration = 2.5
                    rig["quake"].start()
                    print(f"aftershock M {magnitude:.1f}")

            # --- controls ---
            if carbine.pressed("r"):
                rig["quake"].magnitude = dial
                rig["quake"].duration = 3.0 + 1.2 * (dial - 5.0)
                rig["quake"].start()
                queue = [
                    (6.0 + 7.0 * number, max(3.0, dial - 1.1 - 0.4 * number))
                    for number in range(3)
                ]
                print(
                    f"rupture M {dial:g} over a {reach:g} m reach, "
                    f"{shaking:.1f} m/s^2 at the fault"
                )
            if carbine.pressed("x"):
                carbine.physics.stopAll()
                queue = []
                print("still air")
            if carbine.pressed("1") or carbine.pressed("2"):
                dial = min(9.4, max(3.5, dial + (0.5 if carbine.pressed("2") else -0.5)))
                for storm in storms:
                    storm.magnitude = dial
                print(f"dial set to M {dial:g}")
            if carbine.pressed("3") or carbine.pressed("4"):
                widen = 8.0 if carbine.pressed("4") else -8.0
                reach = min(160.0, max(8.0, reach + widen))
                rig["quake"].reach = reach
                print(f"felt within {reach:g} m")
            if carbine.pressed("t"):
                for block in rig["blocks"]:
                    if not block["gone"]:
                        block["retrofitted"] = True
                        block["node"].color = RETROFIT
                print("the city is retrofitted")
            if carbine.pressed("g"):
                loose = any(not block["node"].anchored for block in rig["blocks"])
                for block in rig["blocks"]:
                    block["node"].anchored = loose
                print("anchored to bedrock" if loose else "let loose again")
            if carbine.pressed("b"):
                plan += 1
                rig = city(plan)
                trace = [0.0] * DRUM
                queue = []
                print(f"a fresh city: {len(rig['blocks'])} blocks")
            if carbine.pressed("v"):
                player.mode = "third" if player.mode == "first" else "first"
                print(f"{player.mode} person")
            if carbine.pressed("p"):
                patrol = not patrol
                player.enabled = not patrol
                if patrol:
                    print("patrol: WASD drives the fault, the camera orbits it")
                else:
                    print("walk: the player owns the camera; M parks the fault at your feet")
            if carbine.pressed("m") and not patrol:
                rig["fault"].position = (player.position.x, player.position.y, 0.8)
                print("the fault moved under you")

            # --- the two cameras ---
            if patrol:
                where = rig["fault"].position
                nudge = 9.0 * dt
                if carbine.key("w"):
                    rig["fault"].position = (where.x, where.y + nudge, 0.8)
                if carbine.key("s"):
                    rig["fault"].position = (where.x, where.y - nudge, 0.8)
                if carbine.key("d"):
                    rig["fault"].position = (where.x + nudge, where.y, 0.8)
                if carbine.key("a"):
                    rig["fault"].position = (where.x - nudge, where.y, 0.8)
                angle = (angle + 8.0 * dt) % 360.0
                ring = math.radians(angle)

                # The engine shakes the player's own eyes; out here on the orbit the
                # script is the camera, so it is the script that throws the ring. The
                # fault is dragged along with it, which is what makes the whole frame
                # rattle rather than the city slide past a still lens.
                at = rig["fault"].position
                push = shove(storms, at)
                farthest = max(abs(axis) for axis in push) or 1.0
                scale = min(RING_PER_ACCEL, RING_MAX / farthest)
                off = tuple(scale * axis for axis in push)
                camera.position = (
                    38.0 * math.sin(ring) + off[0],
                    -38.0 * math.cos(ring) + off[1],
                    30.0 + off[2],
                )
                camera.lookAt((at.x + off[0] * 0.5, at.y + off[1] * 0.5, 0.8))

            if frame % 150 == 0:
                print(
                    f"M {dial:g} - {shaking:.1f} m/s^2 at the fault - "
                    f"{standing(rig['blocks'])}/{len(rig['blocks'])} standing - "
                    f"{len(queue)} aftershock(s) queued - "
                    f"player feels {underfoot:.1f}"
                )
            frame += 1
    finally:
        # The city, the dust, the weather and the window all leave on this one call.
        carbine.end()


if __name__ == "__main__":
    main()

"""Models: a whole `.glb` file read into the scene as nodes, and then driven.

    python examples/models.py
    CARBINE_ASSET=/path/to/anything.glb python examples/models.py

Carbine reads the GLB container itself - no assimp, no Python loader, no decoder of
anybody else's at runtime - and what the file names arrives as nodes of the scene: one
per node, plus one more for every extra surface a node carries that it cannot wear
itself. So a turret is still a node under its tank, a seat marker is still a place inside
it, and a script can name either one and move it. A file written in Y-up is stood up into
this Z-up world by the same read, and a base colour is encoded on the way in - both done
once, in the reader, rather than in every script that loads something.

The asset this file asks for is a hangar rather than a model: ten vehicles in one file,
of which four have a floor to stand on. `#Heavy Tank` on the end of the path pulls one
branch out of it, which is how a level author wears a single thing from a shared file;
the whole file, asked for with no `#`, brings all ten and leaves them stacked on top of
one another, because a modelling program lays objects out about their own origins. The
first print below reads the file through the scene, and the parked row asks the same file
one branch at a time.

A loaded vehicle is not a car: it is a node wearing a file. `carbine.new("Model")` makes
an anchor with no body of its own, the file hangs under it, and the anchor's `size` is
then read as the whole model - which is also where its `mass` comes from, so a Heavy Tank
weighs three tonnes because that is how much tank the file describes, not because a
script said so. Three more writes make it driveable: `driveable` hangs a drive on a node
that was not born `new("Vehicle")` and asks nothing else of it, and the `collision` and
`gravity` the car spawn path turns on for you are left here for the script to ask for.
The drive is the same arcade model either way, so a wall stops a tank exactly as it stops
a hatchback, and a crate that weighs less than it gets shoved out of the way.

The floor holds a body by the lowest point it is drawn with rather than by its pivot, and
for a loaded model those are not the same place: every vehicle in this hangar hangs most of
a metre of bodywork under the origin the file stood it on, so the pivot is left that far
clear of the ground and the tyres are what rests on it.

Each parked vehicle is collided as `collisionType = "mesh"` rather than left as the box its
`size` describes. A box around a tank is a brick with the space between its wheels filled
in, so a crate stands on air above the deck and a fender reaches further than the fender is.
Mesh hands the solver one solid per surface the file drew, and the vehicle keeps its hollows.

The one place this file has to correct the modeller is the nose. A drive steers along the
node's own `+X`; every vehicle in this hangar was laid out along `+Y`, with the gun
barrel pointing the same way. So each parked row gets a quarter turn on the *file's* root
node - a write that is only possible because the import stayed a tree - and the anchor
keeps pointing wherever the script put it.

    WASD wheel (throttle and steer) - space brake - E get in or out
    T lift the turret clear of the hull and swing it - press again and it goes back
    F eyes or chase lens - close the window to quit

The turret is the point of the whole file. A welded mesh cannot be lifted forty-five
degrees above its tank and put back exactly where it was; a node can, and this one is
still named what the person who made the file called it.
"""

import os
import sys

import carbine

ASSET = os.environ.get(
    "CARBINE_ASSET", os.path.expanduser("~/Documents/carbinevehicleassetsMITlicensed.glb")
)

# The four things in the hangar with a floor to stand on. The other six are ships and
# aeroplanes, and the reader brings those along just the same.
PARKED = ["Medium Tank", "Heavy Tank", "Transport vehicle", "Infantry Vehicle.014"]

# The quarter turn that points a vehicle from this file at the way a drive asks to go.
NOSE = (0.0, 0.0, -90.0)

# Where the four stand, and how far the player has to be to climb into one.
PLACES = [(-9.0, 6.5 * index, 0.6) for index in range(len(PARKED))]
REACH = 5.0

# What a turret is lifted and swung by when a script takes it off.
OFF = (0.0, 0.0, 1.2)
TURNED = (0.0, 0.0, 45.0)

ORANGE = "#eb7a34"
NAVY = "#2f6cb3"

DISTANCE = 11.0
EYES = 2.2

# The crates are there for the weight a file brings: a three tonne tank at pace takes a
# two hundred kilogramme crate with it, and does not stop for it.
CRATES = [(14.0, 3.0), (17.0, -3.0), (20.0, 1.0)]

# Where a seat goes on a loaded vehicle. A model gives no handle on its own top, so this
# is a share of the height the file measures - which is the kind of thing a script is
# allowed to guess at, and to change the moment the modelling program is opened. These
# four hang their body under the pivot, so the pivot is already a fraction of a metre off
# the floor when this share of the measured height is taken above it.
ROOF = 0.6


def counted(node):
    """How many nodes of the scene a branch is, the file's and any the script hung on."""
    return 1 + sum(counted(child) for child in node.children)


def metres(where):
    return " x ".join(f"{value:.2f}" for value in where)


def surveyed(path):
    """What the file holds, printed once from the scene itself: one node per root, each
    with the tree the file gave it. The anchor is thrown away as soon as it has been
    read, because this is only the survey."""
    hangar = carbine.new("Model", name="Hangar")
    hangar.model = path
    print(f"{os.path.basename(path)}: {len(hangar.children)} vehicles in one file")
    for root in hangar.children:
        print(
            f"  {root.name}: {counted(root)} nodes, {metres(root.size)} m, "
            f"{root.mass:,.0f} kg"
        )
    hangar.destroy()
    print(f"  taken off again: {len(carbine.scene.root.children)} nodes left in the world")


def parked(asset, label, at):
    """One vehicle out of the file, standing on the floor and wearing a drive.

    The anchor is the node a script moves, collides and drives; everything the file
    named hangs under it and goes with it. Three writes make it a car - `driveable` for
    the drive, and the collider and gravity the vehicle spawn path would have turned on
    by itself - and a fourth turns the file's own root so its nose is the nose.

    The collider is cut as "mesh" rather than left as the box the anchor's `size`
    describes, because a tank is not a brick: a box around the whole vehicle fills in
    the space between its wheels and leaves a crate standing on air above the deck. A
    mesh hands the solver one solid per surface the file drew - hull, turret, wheels -
    and the room inside the vehicle stays room."""
    tank = carbine.new("Model", name=label)
    tank.model = f"{asset}#{label}"
    tank.position = at
    tank.collisionType = "mesh"
    tank.gravity = True
    tank.driveable = True

    hull = tank.children[0]
    hull.rotationEuler = NOSE

    wheel = carbine.new("DriverSeat", name=f"{label} wheel", parent=tank)
    wheel.position = (0.0, 0.0, tank.size.z * ROOF)

    gun = next((child for child in hull.children if "Turret" in child.name), None)
    return tank, wheel, gun


def marked(tank, label="driver/driverSeat.001"):
    """A plain seat hung at the place the modeller marked as one. The file's own seat
    node is a node of the scene, so its place can be read and copied beside it - which
    is where it goes, and where it dies with the branch when the file is taken off."""
    mark = tank.find(label)
    if mark is None:
        return None
    seat = carbine.new("Seat", name=f"{mark.name} seat", parent=mark.parent)
    seat.position = mark.position
    return seat


def wheel_of(body):
    """The car this node sits in, and whether the seat it sits in drives."""
    seat = body.seatedIn
    if seat is None:
        return None, False
    return seat.parent, seat.drives


def nearest(fleet, at):
    """The nearest car of the row, with its driver seat, if it is within reach."""
    best, best_distance = None, None
    for car, wheel, _ in fleet:
        where = car.position
        distance = ((where.x - at.x) ** 2 + (where.y - at.y) ** 2) ** 0.5
        if best_distance is None or distance < best_distance:
            best, best_distance = (car, wheel), distance
    if best is None or best_distance > REACH:
        return None, best_distance or 0.0
    return best, best_distance


def wish(held, forwards, back):
    """One of the three knobs, read off the keyboard: +1, -1, or 0 for neither."""
    return 1.0 if (held(forwards) and not held(back)) else (-1.0 if held(back) else 0.0)


def swung(turret, homes, tank):
    """Take a turret off the car, or put it back exactly where the file had it.

    The first press remembers the position and the turn the import gave the node and
    writes those numbers back on the second, so the branch is what the person who made
    the file left - nothing is destroyed and nothing is re-made. The anchor's `size` is
    read from every surface hanging under it, which is also where its `mass` comes from,
    so a turret lifted above the roofline measurably lengthens the hull: the last print
    is the car's own answer, not a number this file kept."""
    if turret.id in homes:
        home, back = homes.pop(turret.id)
        turret.position = home
        turret.rotationEuler = back
        word = "back where the file had it"
    else:
        at, turned = tuple(turret.position), tuple(turret.rotationEuler)
        homes[turret.id] = (at, turned)
        turret.position = (at[0] + OFF[0], at[1] + OFF[1], at[2] + OFF[2])
        turret.rotationEuler = (turned[0], turned[1], turned[2] + TURNED[2])
        word = f"lifted {OFF[2]:g} m off the hull and swung {TURNED[2]:g} deg"
    print(f"{turret.name} {word} - the {tank.name} measures {metres(tank.size)} m now")


def main() -> None:
    if not os.path.exists(ASSET):
        sys.exit(f"no model to read at {ASSET}\nput one there or set CARBINE_ASSET to it")

    adapter = carbine.init(1280, 720, title="Carbine Models")
    width, height = carbine.viewport()
    print(
        f"{adapter} at {width}x{height}\n"
        "WASD wheel - space brake - E get in or out - T lift the turret - "
        "F eyes or chase lens - close the window to quit"
    )
    print("the kinds a model needs:", ", ".join(carbine.kinds()))

    surveyed(ASSET)

    ground = carbine.new("plane", name="Ground")
    ground.size = (160.0, 160.0, 1.0)
    ground.color = "#3d4a3a"

    for index, (across, away) in enumerate(CRATES):
        crate = carbine.new("cube", name=f"Crate {index}")
        crate.size = (1.0, 1.0, 1.0)
        crate.position = (across, away, 0.5)
        crate.color = ORANGE
        crate.collision = True
        crate.gravity = True

    fleet = []
    for label, at in zip(PARKED, PLACES):
        tank, wheel, gun = parked(ASSET, label, at)
        seat = marked(tank)
        print(
            f"{label}: {metres(tank.size)} m at {tank.mass:,.0f} kg, "
            f"{counted(tank)} nodes on the branch, "
            f"gun {'found as ' + repr(gun.name) if gun else 'not in this one'}, "
            f"seat {'copied from the file' if seat else 'left to the roof'}"
        )
        fleet.append((tank, wheel, gun))

    suit = carbine.spawnEntity("cube", name="Suit")
    suit.size = (0.6, 0.6, 1.7)
    suit.color = NAVY

    player = carbine.player()
    player.body = suit
    player.position = (-16.0, -2.0, 0.0)
    player.mode = "third"
    player.distance = DISTANCE
    player.eyeHeight = EYES
    player.enabled = True

    panel = carbine.ui.new("rectangle", name="Hud")
    panel.position = (120, height - 44, 0)
    panel.size = (300, 36, 1)
    panel.color = "#221e1b"
    panel.corner = 10
    panel.transparent = 190

    pace_bar = carbine.ui.new("rectangle", name="Pace", parent=panel)
    pace_bar.position = (-90, -8, 0)
    pace_bar.size = (4.0, 10.0, 1)
    pace_bar.color = NAVY
    pace_bar.corner = 5

    # Which turrets are currently standing off their car, and where the file had them.
    homes = {}
    last = None
    pace = 0.0
    frame = 0
    try:
        while carbine.running():
            carbine.render()
            step = carbine.dt()

            car, driving = wheel_of(suit)

            if carbine.pressed("e"):
                if car is not None:
                    suit.stand()
                    print("out - and its speed goes with you for a moment")
                    car, driving = None, False
                else:
                    reached, away = nearest(fleet, player.position)
                    if reached is None:
                        print(f"nothing to climb into within {REACH:g} m")
                    elif reached[1].seated is not None:
                        print(f"the {reached[0].name} wheel is already taken")
                    else:
                        suit.sit(reached[1])
                        print(
                            f"in the {reached[0].name} - {away:.1f} m away, "
                            f"{reached[0].mass:,.0f} kg of what the file describes"
                        )

            if carbine.pressed("f"):
                player.mode = "first" if player.mode == "first" else "third"
                print(f"{'eyes' if player.mode == 'first' else 'chase'} lens")

            if carbine.pressed("t"):
                turret = next((gun for each, _, gun in fleet if each == car), None)
                if car is None:
                    print("no turret to take off - walk up to a tank and get in")
                elif turret is None:
                    print(f"the {car.name} has no node named for a turret")
                else:
                    swung(turret, homes, car)

            if carbine.pressed("g"):
                for tank, _, _ in fleet:
                    tank.gravity = not tank.gravity
                print(f"gravity {'on' if fleet[0][0].gravity else 'off'} the row")

            if car is not None and driving:
                car.throttle = wish(carbine.key, "w", "s")
                car.steer = wish(carbine.key, "d", "a")
                car.brake = 1.0 if carbine.key("space") else 0.0
                gone = car.position
                if last is not None and step > 0.0:
                    pace = ((gone.x - last[0]) ** 2 + (gone.y - last[1]) ** 2) ** 0.5 / step
                last = (gone.x, gone.y)
                pace_bar.size = (4.0 + 110.0 * min(1.0, pace / car.topSpeed), 10.0, 1)
            else:
                if last is not None:
                    print(f"left it at ({last[0]:.1f}, {last[1]:.1f}) m")
                last, pace = None, 0.0
                for each, _, _ in fleet:
                    each.throttle = each.steer = each.brake = 0.0
                pace_bar.size = (4.0, 10.0, 1)

            if frame % 180 == 0 and car is not None:
                print(
                    f"{car.name} at ({car.position.x:.1f}, {car.position.y:.1f}) m - "
                    f"pivot {car.position.z:.2f} m off the floor - {pace:.1f} m/s - "
                    f"throttle {car.throttle:+g} steer {car.steer:+g}"
                )

            frame += 1
    finally:
        carbine.end()


if __name__ == "__main__":
    main()

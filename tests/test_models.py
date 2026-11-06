"""Models: a `.glb` file hung onto the scene as nodes of its own.

Carbine reads the GLB container itself, and an import keeps the file's hierarchy: the
tank comes in as a node with a turret node under it, not as one welded mesh, so a script
can still name either part, move either one and hide either one. Everything here writes
a small GLB by hand - the suite has no decoder and does not need one - and one test reads
the real asset when the machine has it.
"""

import json
import os
import struct

import carbine
import pytest

WIDTH = 64
HEIGHT = 64

CLEAR = (22, 21, 20)

# The three primaries. glTF writes a base colour as linear light, and the encode of 1.0 is
# 1.0 and of 0.0 is 0.0, so a lit face of these paints exactly its own colour.
RED = (1.0, 0.0, 0.0, 1.0)
GREEN = (0.0, 1.0, 0.0, 1.0)
BLUE = (0.0, 0.0, 1.0, 1.0)


def pixel(data, x, y, width=WIDTH):
    offset = (y * width + x) * 4
    return tuple(data[offset : offset + 3])


def near(found, want, tolerance=6):
    return all(abs(a - b) <= tolerance for a, b in zip(found, want))


def glb(header, bin_bytes):
    """The container: a twelve byte head, then the JSON chunk and the binary chunk, each
    padded out to a whole four bytes and each declared at its padded length."""
    text = json.dumps(header).encode()

    def pad(length):
        return (length + 3) // 4 * 4

    total = 12 + 8 + pad(len(text)) + 8 + pad(len(bin_bytes))
    out = bytearray()
    out += b"glTF"
    out += (2).to_bytes(4, "little")
    out += total.to_bytes(4, "little")
    out += pad(len(text)).to_bytes(4, "little")
    out += b"JSON"
    out += text
    out += b" " * (pad(len(text)) - len(text))
    out += pad(len(bin_bytes)).to_bytes(4, "little")
    out += b"BIN\x00"
    out += bin_bytes
    out += b"\x00" * (pad(len(bin_bytes)) - len(bin_bytes))
    assert len(out) == total
    return bytes(out)


def written(meshes, nodes):
    """A glTF document for these meshes and this node tree, over one binary buffer.

    A mesh is a list of parts and a part is `(corners, triangles, colour)`, its corners
    already in glTF's own Y-up metres."""
    bin_bytes = bytearray()
    views, accessors, materials = [], [], []

    def floats(rows):
        start = len(bin_bytes)
        for corner in rows:
            bin_bytes.extend(struct.pack("<3f", *corner))
        views.append({"buffer": 0, "byteOffset": start, "byteLength": len(rows) * 12})
        accessors.append(
            {
                "bufferView": len(views) - 1,
                "componentType": 5126,
                "count": len(rows),
                "type": "VEC3",
            }
        )
        return len(accessors) - 1

    def indexes(triangles):
        start = len(bin_bytes)
        for triangle in triangles:
            bin_bytes.extend(struct.pack("<3H", *triangle))
        while len(bin_bytes) % 4:
            bin_bytes.append(0)
        views.append(
            {"buffer": 0, "byteOffset": start, "byteLength": len(triangles) * 6}
        )
        accessors.append(
            {
                "bufferView": len(views) - 1,
                "componentType": 5123,
                "count": len(triangles) * 3,
                "type": "SCALAR",
            }
        )
        return len(accessors) - 1

    def paint(colour):
        materials.append({"pbrMetallicRoughness": {"baseColorFactor": list(colour)}})
        return len(materials) - 1

    built = []
    for parts in meshes:
        primitives = []
        for corners, triangles, colour in parts:
            primitives.append(
                {
                    "attributes": {"POSITION": floats(corners)},
                    "indices": indexes(triangles),
                    "material": paint(colour),
                }
            )
        built.append({"primitives": primitives})

    header = {
        "asset": {"version": "2.0", "generator": "Carbine's own test suite"},
        "buffers": [{"byteLength": len(bin_bytes)}],
        "bufferViews": views,
        "accessors": accessors,
        "materials": materials,
        "meshes": built,
        "nodes": nodes,
        "scenes": [{"nodes": [0]}],
        "scene": 0,
    }
    return header, bytes(bin_bytes)


# Corners in glTF's XZ ground plane, wound so their faces stand up. The reader carries
# every one of them through (x, y, z) -> (x, -z, y), so a sheet authored flat here lies
# flat in Carbine, facing the sky.
HULL = [(-1.0, 0.0, -1.0), (0.0, 0.0, 1.0), (1.0, 0.0, -1.0)]
SKIRT = [(0.6, 0.0, 0.6), (0.6, 0.0, 0.9), (0.9, 0.0, 0.6)]
TURRET = [(-0.3, 0.0, -0.3), (0.0, 0.0, 0.3), (0.3, 0.0, -0.3)]
ONE = [(0, 1, 2)]


@pytest.fixture
def hangar(tmp_path):
    """A tank of two surfaces with a turret standing on it, written into the test's own
    folder. The hull node wears both of its surfaces, so one of them has to arrive as a
    node of its own; the turret is the file's own child."""
    meshes = [
        [(HULL, ONE, RED), (SKIRT, ONE, GREEN)],
        [(TURRET, ONE, BLUE)],
    ]
    nodes = [
        {"name": "Hull", "mesh": 0, "translation": [0.0, 0.5, 0.0], "children": [1]},
        {"name": "Turret", "mesh": 1, "translation": [0.0, 1.0, 0.0]},
    ]
    header, bin_bytes = written(meshes, nodes)
    path = tmp_path / "tank.glb"
    path.write_bytes(glb(header, bin_bytes))
    return str(path)


def brought(path, name="Tank"):
    """An object wearing the file, with the file's own hull one step under it."""
    tank = carbine.new("Model", name=name)
    tank.model = path
    return tank


def names(handles):
    return [node.name for node in handles]


SECOND = 1.0 / 60.0


def rolled(seconds=1.0):
    for _ in range(round(seconds / SECOND)):
        carbine.step(SECOND)


def test_carbine_lists_the_model_kind():
    assert "model" in carbine.kinds()
    anchor = carbine.new("Model", name="Anchor")
    try:
        assert anchor.kind == "model"
        assert anchor.model is None
        assert anchor.children == []
    finally:
        anchor.destroy()


def test_a_file_arrives_as_the_nodes_it_named(hangar):
    tank = brought(hangar)
    try:
        assert tank.model == hangar
        (hull,) = tank.children
        assert hull.name == "Hull"
        assert hull.kind == "model"
        assert hull.parent == tank

        turret = tank.find("Turret")
        assert turret is not None
        assert turret.parent == hull
        assert tank.find("Nowhere") is None

        # The file's own places survive: half a metre up for the hull, a metre more for
        # the turret standing on it.
        assert near(tuple(hull.position), (0.0, 0.0, 0.5), 1e-4)
        assert near(tuple(turret.position), (0.0, 0.0, 1.0), 1e-4)

        # The anchor stays the node the script wrote on: the branch is under it, not it.
        assert tank.name == "Tank"
        assert names(hull.children) == ["Hull.1", "Turret"]
    finally:
        tank.destroy()


def test_a_second_surface_becomes_a_node_of_its_own(hangar):
    tank = brought(hangar)
    try:
        (hull,) = tank.children
        skirt = hull.children[0]
        # A node carries one mesh, so the file's second surface hangs beside the file's
        # children under a name made from its own.
        assert skirt.name == "Hull.1"
        assert hull.find("Hull.1") == skirt
        assert near(tuple(skirt.color), (0.0, 1.0, 0.0), 0.01)
        assert near(tuple(hull.color), (1.0, 0.0, 0.0), 0.01)
        assert near(tuple(skirt.position), (0.0, 0.0, 0.0), 1e-4)

        # A file's picture is already laid out the way the modelling program drew it, so
        # a hung part arrives unscaled and stays that way until a script asks for tiles.
        assert hull.textureTiling == 0.0
        assert tank.textureTiling == 1.0, "the anchor is Carbine's own node"

        hull.textureTiling = 2.0
        assert hull.textureTiling == 2.0
        assert skirt.textureTiling == 0.0, "and a part is measured on its own"
    finally:
        tank.destroy()


def test_the_anchor_measures_the_whole_thing(hangar):
    tank = brought(hangar)
    try:
        # Two metres each way of hull, and a metre of turret above it.
        assert near(tuple(tank.size), (2.0, 2.0, 1.0), 0.01)

        # Which is what a weight is read from: two by two by one at the world's own
        # two hundred kilograms to the cubic metre.
        tank.collision = True
        tank.collisionType = "box"
        assert tank.mass == pytest.approx(800.0, abs=1.0)

        tank.scale = (2.0, 2.0, 2.0)
        assert near(tuple(tank.size), (4.0, 4.0, 2.0), 0.01)
        tank.size = (2.0, 2.0, 1.0)
        assert near(tuple(tank.scale), (1.0, 1.0, 1.0), 0.01)
    finally:
        tank.destroy()


def test_the_parts_stay_separately_drawable(hangar, flat):
    tank = brought(hangar)
    try:
        turret = tank.find("Turret")

        # The turret stands between the lens and the hull, so it paints the centre.
        assert near(pixel(carbine.readPixels(), 32, 32), (0, 0, 255))

        turret.visible = False
        assert near(pixel(carbine.readPixels(), 32, 32), (255, 0, 0))

        # The anchor itself draws nothing: it is only where the file hangs.
        tank.children[0].visible = False
        assert pixel(carbine.readPixels(), 32, 32) == CLEAR
    finally:
        tank.destroy()


def test_taking_the_file_off_leaves_what_the_script_wrote(hangar):
    tank = brought(hangar)
    try:
        flag = carbine.new("cube", name="Flag", parent=tank)
        assert names(tank.children) == ["Hull", "Flag"]

        tank.model = None

        assert tank.model is None
        assert tank.children == [flag]
        assert tank.find("Turret") is None
        assert flag.name == "Flag"
        flag.destroy()

        # And the file can go back on, once, without leaving a second copy of the branch.
        tank.model = hangar
        tank.model = hangar
        assert names(tank.children) == ["Hull"]
    finally:
        tank.destroy()


def test_one_named_branch_of_a_hangar_is_enough(hangar):
    tank = brought(hangar)
    try:
        tank.model = f"{hangar}#Turret"

        assert tank.model == f"{hangar}#Turret"
        assert names(tank.children) == ["Turret"]
        assert tank.find("Hull") is None
        # The branch alone: sixty centimetres of turret, a metre above the anchor.
        assert near(tuple(tank.size), (0.6, 0.6, 1.0), 0.01)
        assert near(tuple(tank.find("Turret").position), (0.0, 0.0, 1.0), 1e-4)
    finally:
        tank.destroy()


def test_an_actor_can_wear_a_file_too(hangar):
    rig = carbine.spawnEntity(name="Rig")
    try:
        rig.model = hangar
        assert rig.model == hangar
        assert names(rig.children) == ["Hull"]
        assert rig.find("Turret") is not None

        # A script's own children ride along beside the file's and stay after it.
        hat = carbine.new("cube", name="Hat", parent=rig)
        rig.model = None
        assert rig.children == [hat]
        hat.destroy()
    finally:
        rig.destroy()


def test_a_file_carbine_does_not_read_is_refused(tmp_path, hangar):
    tank = carbine.new("Model", name="Refused")
    try:
        assert tank.model is None

        obj = tmp_path / "old.obj"
        obj.write_text("v 0 0 0\n")
        with pytest.raises(ValueError, match="not a GLB"):
            tank.model = str(obj)

        with pytest.raises(ValueError, match="export the model as glTF"):
            tank.model = str(tmp_path / "dance.fbx")

        with pytest.raises(ValueError, match="no file"):
            tank.model = str(tmp_path / "nowhere.glb")

        assert tank.model is None
        assert tank.children == [], "a refused import hangs nothing"

        # A name no node of the file carries is refused the same way.
        with pytest.raises(ValueError, match='no node is named "Silo"'):
            tank.model = f"{hangar}#Silo"
        assert tank.children == []
    finally:
        tank.destroy()


def test_a_copy_of_an_import_carries_its_branch(hangar):
    tank = brought(hangar)
    try:
        (twin,) = carbine.duplicate(tank, 1)
        assert twin.model == hangar
        assert names(twin.children) == ["Hull"]
        assert twin.find("Turret") is not None
        assert twin.find("Turret") != tank.find("Turret")
        assert near(tuple(twin.size), tuple(tank.size), 0.01)

        twin.destroy()
        assert names(tank.children) == ["Hull"]
    finally:
        tank.destroy()


def test_an_imported_vehicle_can_be_driven(hangar):
    """A drive is a component a script hangs on a node, so a tank somebody drew in a
    modelling program answers the wheel the same way `new("Vehicle")` does - and keeps the
    size, weight and hierarchy the file brought instead of the hatchback a kind word buys."""
    tank = carbine.new("Model", name="Tank")
    wheel = carbine.new("DriverSeat", parent=tank)
    hero = carbine.spawnEntity("cube", name="Hero", position=(0.0, 4.0, 1.0))
    hero.size = (0.6, 0.6, 1.6)
    hero.collision = True
    hero.gravity = True
    try:
        tank.model = hangar
        assert tank.driveable is False

        tank.driveable = True
        tank.collision = True
        tank.gravity = True

        # The body is the file's own: two metres of hull and turret at the world's two
        # hundred kilograms to the cubic metre, not a 4.4 by 2 by 1.2 car.
        assert near(tuple(tank.size), (2.0, 2.0, 1.0), 0.01)
        assert tank.mass == pytest.approx(800.0, abs=1.0)

        hero.sit(wheel)
        assert tank.driver == hero, "a seat under a driveable model drives it"

        tank.throttle = 1.0
        rolled()

        assert tank.position.x > 3.5, str(tuple(tank.position))
        assert hero.position.x == pytest.approx(tank.position.x, abs=1e-4)
        assert tank.find("Turret") is not None, "and the branch came along with it"

        hero.stand()
    finally:
        hero.destroy()
        wheel.destroy()
        tank.destroy()


@pytest.fixture
def skid(tmp_path):
    """One plate hanging half a metre under the origin its file gave it, which is where a
    real vehicle's tyres are relative to the point a modelling program dropped at its
    centre: the body is below the pivot, not around it."""
    meshes = [[(HULL, ONE, RED)]]
    nodes = [{"name": "Plate", "mesh": 0, "translation": [0.0, -0.5, 0.0]}]
    header, bin_bytes = written(meshes, nodes)
    path = tmp_path / "skid.glb"
    path.write_bytes(glb(header, bin_bytes))
    return str(path)


def test_a_loaded_model_rests_on_its_lowest_point(skid):
    """The floor holds a body by the lowest thing drawn on it rather than by its pivot, so
    a file that hangs its bodywork under the origin stands on that bodywork and leaves the
    pivot as far above the ground as the file hung it below."""
    plate = brought(skid, name="Plate")
    try:
        assert near(tuple(plate.size), (2.0, 2.0, 1.0), 0.01)
        plate.collision = True
        plate.gravity = True
        plate.position = (0.0, 0.0, 4.0)

        rolled(5.0)

        # Not on 0.0, which would put the plate itself under the floor.
        assert plate.position.z == pytest.approx(0.5, abs=2e-3), str(tuple(plate.position))
    finally:
        plate.destroy()


def test_a_mesh_collider_leaves_the_room_inside_a_file_open(hangar):
    """"mesh" hands the solver one solid per surface the file drew rather than a brick
    around the whole vehicle, so the metre of air between a tank's deck and its turret
    stays air and a neighbour stood in it is left there."""
    tank = brought(hangar)
    tank.collisionType = "mesh"
    tank.anchored = True
    crate = carbine.new("cube", name="Crate")
    try:
        crate.size = (0.2, 0.2, 0.2)
        # Between the deck the file hung at half a metre and the turret at one and a half.
        crate.position = (0.0, 0.0, 1.0)
        crate.collision = True
        crate.gravity = True

        rolled(SECOND)

        assert tuple(crate.position) == pytest.approx((0.0, 0.0, 1.0), abs=1e-2), str(
            tuple(crate.position)
        )

        # The same metre of air cut as a box is inside the brick around the model, and the
        # solver spends the step pulling the crate back out of it.
        tank.collisionType = "box"
        rolled(3.0 * SECOND)

        assert abs(crate.position.z - 1.0) > 0.1, str(tuple(crate.position))
    finally:
        crate.destroy()
        tank.destroy()


ASSET = os.environ.get("CARBINE_ASSET", "")


@pytest.mark.skipif(not os.path.exists(ASSET), reason="no model asset on this machine")
def test_a_real_asset_keeps_its_hierarchy():
    """The whole file, read through the scene rather than through the reader: a hangar of
    vehicles comes in as vehicles, each with its own turret and its own seat."""
    tank = brought(ASSET, name="Hangar")
    try:
        assert tank.model == ASSET
        assert len(tank.children) == 10, "one node per root the file names"
        assert names(tank.children) == [
            "Battleship",
            "Bomber Plane",
            "Fighter Jet",
            "Transport Plane",
            "Transport Ship",
            "Medium Tank",
            "DualRocketTransportPlane",
            "Transport vehicle",
            "Heavy Tank",
            "Infantry Vehicle.014",
        ]
        for part in ("HT Turret", "MT Turret", "driver/driverSeat.001", "Rocket1"):
            assert tank.find(part) is not None, part

        # Only the vehicle a script asked for, standing where it stood in the file.
        tank.model = f"{ASSET}#Heavy Tank"
        (hull,) = tank.children
        assert hull.name == "Heavy Tank"
        turret = hull.find("HT Turret")
        assert turret is not None
        assert turret.parent == hull, "the turret is still under its tank"
        assert tank.find("Fighter Jet") is None

        # Two metres wide, near seven along the nose and a metre of turret above the
        # hull: a tank measured in metres, the same box the file's own bounds say.
        assert near(tuple(tank.size), (2.21, 6.78, 1.03), 0.05)
    finally:
        tank.destroy()

"""Vehicles, the seats hanging on them, and the riders the step holds in those seats.

A car is one object wearing a drive, so it is drawn, collides and falls like any other
body. The drive is a thing a script can hang on any node - `box.driveable = True` - which
is how a loaded model or a horse comes to answer the same three knobs, and it puts nothing
else on the node it is written on. A seat is a node of its own pinned to that object, and a
rider stays the node the script made - the step only holds it at the seat's place while it
sits. Everything here runs through `carbine.step`, and the scene is one per process, so
each test destroys what it made and leaves nobody driving.
"""

import carbine
import pytest

SECOND = 1.0 / 60.0

HANDS = ("w", "a", "s", "d", "space")

# The hatchback a script is given for writing nothing about size, and the weight of a box
# that big at the world's own two hundred kilograms to the cubic metre.
CHASSIS = (4.4, 2.0, 1.2)
WEIGHT = 2112.0

# What the drive model is set to give a car: the nose swung a hundred and twenty degrees
# a second at full lock, and a full throttle chases twenty metres a second.
TURN_RATE = 120.0


def settle(seconds=2.0):
    for _ in range(round(seconds / SECOND)):
        carbine.step(SECOND)


def park():
    """A car dropped onto the floor at the origin and left there. Half its height is all
    that stands between it and the ground, so that is the height of the seat hanging on
    it too."""
    auto = carbine.new("Vehicle")
    settle()
    return auto


def rider(at, **made):
    """A cube with feet, stood where a script puts it before it has anywhere to sit: in
    the solver and held by gravity, so it is a body the engine would move about if it
    were not sitting down."""
    box = carbine.new("cube", **made)
    box.size = (1.0, 1.0, 1.0)
    box.position = at
    box.collision = True
    box.gravity = True
    return box


@pytest.fixture(autouse=True)
def nobody_driving():
    """A player walking about would be shoved by the cars these tests park, and a key
    one test leaves down is still down when the next one starts."""
    player = carbine.player()
    player.enabled = False
    player.body = None
    for name in HANDS:
        carbine.injectKey(name, False)
    yield player
    player.enabled = False
    player.body = None
    for name in HANDS:
        carbine.injectKey(name, False)


def test_carbine_lists_the_kinds_a_vehicle_needs():
    assert carbine.handlingTypes() == ["arcade", "drift"]
    for name in ("vehicle", "seat", "driverSeat"):
        assert name in carbine.kinds(), name


def test_a_car_is_born_a_hatchback_with_a_drive_on_it():
    auto = carbine.new("Vehicle")
    try:
        assert auto.kind == "vehicle"
        assert auto.name == "Vehicle"
        assert auto.driveable is True, "the drive is what the kind word asked for"
        assert tuple(auto.size) == pytest.approx(CHASSIS, abs=1e-5)
        assert auto.mass == pytest.approx(WEIGHT), "the weight of all that box"
        assert auto.type == "arcade", "sliding is something a script has to want"
        assert auto.topSpeed == 20.0
        assert auto.turnRate == TURN_RATE
        assert auto.grip == 0.5
        assert (auto.throttle, auto.steer, auto.brake) == (0.0, 0.0, 0.0)
        assert auto.collision is True, "a car you walk through is no use to anyone"
        assert auto.gravity is True, "and only a body the solver moves can answer a wheel"

        auto.size = (2.0, 1.0, 1.0)
        assert auto.mass == pytest.approx(400.0), "two cubic metres of the world's stuff"
    finally:
        auto.destroy()


def test_the_handling_is_a_word_a_script_types():
    auto = carbine.new("Vehicle")
    try:
        for word, read in (
            ("drift", "drift"),
            ("  Slide ", "drift"),
            ("RALLY", "drift"),
            ("arcade", "arcade"),
            ("grip", "arcade"),
            ("Street", "arcade"),
        ):
            auto.type = word
            assert auto.type == read, word

        with pytest.raises(ValueError, match='unknown handling type "bouncy"'):
            auto.type = "bouncy"
        assert auto.type == "arcade", "a refused word leaves the car as it stood"
    finally:
        auto.destroy()


def test_a_wish_is_kept_between_the_limits_it_has():
    auto = carbine.new("Vehicle")
    try:
        auto.throttle = 2.0
        assert auto.throttle == 1.0
        auto.throttle = -3.0
        assert auto.throttle == -1.0, "the far end of an accelerator is reverse"
        auto.steer = -3.0
        assert auto.steer == -1.0
        auto.brake = 9.0
        assert auto.brake == 1.0
        auto.brake = -9.0
        assert auto.brake == 0.0, "and a brake has no backwards"

        # A number the maths cannot read is thrown away rather than carried into a solver.
        for knob in ("throttle", "steer", "brake", "topSpeed", "turnRate", "grip"):
            setattr(auto, knob, float("nan"))
            assert getattr(auto, knob) == 0.0, knob
        for knob in ("throttle", "steer", "topSpeed", "turnRate"):
            setattr(auto, knob, float("inf"))
            assert getattr(auto, knob) == 0.0, knob

        auto.topSpeed = -5.0
        assert auto.topSpeed == 0.0, "a ceiling below the ground is no ceiling"
        auto.turnRate = -1.0
        assert auto.turnRate == 0.0
        auto.grip = 9.0
        assert auto.grip == 1.0, "the most the tyres can bite is holding on"
        auto.grip = -1.0
        assert auto.grip == 0.0, "and the least is ice"
    finally:
        auto.destroy()


def test_a_plain_box_refuses_what_only_a_car_or_a_seat_answers():
    box = carbine.new("cube")
    try:
        for knob in ("topSpeed", "turnRate", "grip", "throttle", "steer", "brake", "driver"):
            with pytest.raises(ValueError, match="not a vehicle"):
                getattr(box, knob)
        with pytest.raises(ValueError, match="not a vehicle"):
            box.throttle = 1.0

        for knob in ("drives", "seated"):
            with pytest.raises(ValueError, match="not a seat"):
                getattr(box, knob)
        with pytest.raises(ValueError, match="not a seat"):
            box.sit(box)

        assert box.seatedIn is None, "which seat a node rides is asked of any node"
        box.stand()
    finally:
        box.destroy()


def test_a_drive_is_something_a_script_can_hang_on_any_node():
    box = carbine.new("cube")
    try:
        assert box.driveable is False
        with pytest.raises(ValueError, match="not a vehicle"):
            box.throttle = 1.0

        box.driveable = True
        assert box.driveable is True
        assert box.kind == "cube", "a drive goes on a node, not into its kind word"
        assert box.type == "arcade"
        assert box.topSpeed == 20.0, "the same drive a car is born with"

        # A drive asks nothing else of the body it is hung on, because that body arrives
        # with its own size, weight and place in the solver. A car is only different
        # because `new("Vehicle")` has nothing to read but the kind word, so it is born a
        # hatchback and inside the solver for want of anything better to write.
        box.topSpeed = 5.0
        box.driveable = True
        assert box.topSpeed == 5.0, "writing it again is not a reset"
        assert box.collision is False
        assert box.gravity is False

        box.driveable = False
        assert box.driveable is False
        with pytest.raises(ValueError, match="not a vehicle"):
            box.throttle
    finally:
        box.destroy()


def test_a_node_that_was_not_born_a_car_answers_the_wheel():
    box = carbine.new("cube")
    try:
        box.driveable = True
        box.collision = True
        box.gravity = True
        settle(2.0)
        assert box.position.x == pytest.approx(0.0, abs=1e-3)

        box.throttle = 1.0
        settle(1.0)
        assert box.position.x > 3.5, str(tuple(box.position))
        assert abs(box.position.y) < 1e-3, "along the nose, the way a car goes"

        # Take the drive off and the wheels stop with it: the box is only a body the
        # floor drags again, so it coasts on what it had and asks for nothing more.
        box.driveable = False
        gone = box.position.x
        settle(1.0)
        assert box.position.x > gone + 1.0, "still rolling, and never driven"
    finally:
        box.destroy()


def test_a_seat_reads_whatever_a_script_calls_it():
    for word, drives in (
        ("Seat", False),
        ("chair", False),
        ("Passenger", False),
        ("DriverSeat", True),
        ("  driver's Seat ", True),
        ("Driver", True),
        ("Pilot seat", True),
    ):
        place = carbine.new(word)
        try:
            assert place.drives is drives, word
            assert place.kind == ("driverSeat" if drives else "seat"), word
        finally:
            place.destroy()


def test_a_seat_hangs_on_the_car_it_belongs_to():
    auto = park()
    wheel = carbine.new("DriverSeat", parent=auto)
    bench = carbine.new("Seat", parent=auto)
    try:
        assert wheel.parent == auto
        assert auto.children == [wheel, bench]
        assert wheel.drives is True and bench.drives is False
        assert wheel.seated is None, "and both of them are empty"
        assert auto.driver is None, "a car nobody has sat in has nobody at the wheel"

        # A seat draws nothing: it is somewhere to sit, not a shape.
        with pytest.raises(RuntimeError, match="no such engine component"):
            wheel.mesh
    finally:
        wheel.destroy()
        bench.destroy()
        auto.destroy()


def test_sitting_puts_a_rider_at_the_seat_and_standing_leaves_it_there():
    auto = park()
    wheel = carbine.new("DriverSeat", parent=auto)
    who = rider((0.0, 3.0, 1.0), name="Driver")
    try:
        who.sit(wheel)

        assert wheel.seated == who
        assert who.seatedIn == wheel
        assert type(wheel.seated) is carbine.Object, "scenery comes back as scenery"
        assert tuple(who.position) == pytest.approx(tuple(auto.position), abs=1e-4), (
            "at the seat in the same frame it was asked"
        )
        assert who.parent == carbine.scene.root, "still a node of its own, not a piece of the car"

        who.stand()

        assert wheel.seated is None
        assert who.seatedIn is None
        assert tuple(who.position) == pytest.approx(tuple(auto.position), abs=1e-4), (
            "left standing exactly where the seat had it"
        )
        who.stand()
    finally:
        who.destroy()
        wheel.destroy()
        auto.destroy()


def test_an_actor_rides_the_same_way_a_box_does():
    auto = park()
    wheel = carbine.new("DriverSeat", parent=auto)
    hero = carbine.spawnEntity("cube", name="Hero", position=(0.0, 3.0, 1.0))
    hero.collision = True
    hero.gravity = True
    try:
        hero.sit(wheel)

        assert type(wheel.seated) is carbine.Entity, "an actor comes back an actor"
        assert hero.seatedIn == wheel
        assert auto.driver == hero, "and the driver seat has lent the car its hands"

        hero.stand()
    finally:
        hero.destroy()
        wheel.destroy()
        auto.destroy()


def test_a_car_driven_off_takes_its_driver_with_it():
    auto = park()
    wheel = carbine.new("DriverSeat", parent=auto)
    who = rider((0.0, 3.0, 1.0), name="Driver")
    who.sit(wheel)
    try:
        auto.throttle = 1.0
        auto.steer = 1.0
        settle(1.0)

        assert auto.position.x > 3.5, auto.position
        assert tuple(who.position) == pytest.approx(tuple(auto.position), abs=1e-4)
        assert who.rotationEuler.z < 0.0, "the nose swung right, and the rider went with it"
        assert tuple(who.rotationEuler) == pytest.approx(
            tuple(auto.rotationEuler), abs=1e-4
        )

        settle(1.0)
        assert tuple(who.position) == pytest.approx(tuple(auto.position), abs=1e-4), (
            "a second further on the rider has still not fallen behind"
        )
    finally:
        who.destroy()
        wheel.destroy()
        auto.destroy()


def test_a_rider_in_its_seat_neither_falls_nor_takes_up_room():
    auto = carbine.new("Vehicle")
    auto.position = (0.0, 0.0, 5.0)
    auto.anchored = True
    wheel = carbine.new("DriverSeat", parent=auto)
    who = rider((0.0, 3.0, 1.0), name="Passenger")
    who.sit(wheel)
    try:
        settle(2.0)

        assert who.position.z == pytest.approx(5.0, abs=1e-3), (
            "gravity had hold of it and it stayed in the car"
        )
        assert who.touching() == [], "and it pushed against nothing on the way"
        assert auto.position.x == 0.0, "the car it sits in was not shoved either"
    finally:
        who.destroy()
        wheel.destroy()
        auto.destroy()


def test_one_seat_holds_one_rider_and_a_rider_holds_one_seat():
    auto = park()
    wheel = carbine.new("DriverSeat", parent=auto)
    bench = carbine.new("Seat", parent=auto)
    first = rider((0.0, 3.0, 1.0), name="First")
    second = rider((0.0, 6.0, 1.0), name="Second")
    try:
        first.sit(wheel)
        second.sit(wheel)

        assert wheel.seated == second
        assert first.seatedIn is None, "and whoever was in it before got out"
        assert auto.driver == second

        second.sit(bench)

        assert second.seatedIn == bench
        assert wheel.seated is None, "the seat it left is empty again"
        assert auto.driver is None, "a passenger lends no hands"
    finally:
        for place in (first, second, wheel, bench, auto):
            place.destroy()


def test_a_seat_is_not_something_that_sits():
    auto = park()
    wheel = carbine.new("DriverSeat", parent=auto)
    bench = carbine.new("Seat", parent=auto)
    try:
        with pytest.raises(ValueError, match="not a node that sits"):
            wheel.sit(bench)
        assert wheel.seatedIn is None
    finally:
        wheel.destroy()
        bench.destroy()
        auto.destroy()


def test_getting_out_leaves_a_rider_running_with_the_car():
    auto = park()
    wheel = carbine.new("DriverSeat", parent=auto)
    who = rider((0.0, 3.0, 1.0), name="Driver")
    who.sit(wheel)
    auto.throttle = 1.0
    settle(1.0)
    gone = auto.position.x
    assert gone > 3.5, auto.position

    try:
        who.stand()
        settle(0.5)

        assert who.position.x > gone, "a body that stops being carried keeps going"
    finally:
        who.destroy()
        wheel.destroy()
        auto.destroy()


def test_a_wreck_throws_its_rider_clear():
    wreck = carbine.spawnEntity("cube", name="Wreck", position=(0.0, 0.0, 5.0))
    bench = carbine.new("Seat", name="bench", parent=wreck)
    who = carbine.spawnEntity("cube", name="Rider", position=(0.0, 3.0, 1.0))
    who.collision = True
    who.gravity = True
    killer = carbine.spawnEntity("cube", name="Killer", damage=10.0)
    who.sit(bench)
    try:
        assert bench.seated == who

        landed = killer.hit(wreck)

        assert landed == pytest.approx(10.0)
        assert who.seatedIn is None, "the death of what it rode in stood it up"
        assert bench.seated is None
        assert wreck.health < 0.0, "and the engine has already dealt with the wreck"

        settle(0.25)
        assert who.position.z > 5.3, who.position
    finally:
        for place in (killer, who, bench, wreck):
            place.destroy()


def test_destroying_a_car_leaves_nobody_welded_to_nothing():
    auto = park()
    wheel = carbine.new("DriverSeat", parent=auto)
    who = rider((0.0, 3.0, 1.0), name="Driver")
    who.sit(wheel)

    auto.destroy()

    assert who.name == "Driver", "the rider survived its car"
    assert who.seatedIn is None
    with pytest.raises(carbine.DestroyedError):
        wheel.seated
    settle(1.0)
    assert who.position.z == pytest.approx(0.5, abs=1e-3), "and it has the floor again"
    who.destroy()


def test_a_car_the_solver_cannot_move_has_no_way_to_answer_the_wheel():
    auto = carbine.new("Vehicle")
    auto.position = (0.0, 0.0, 20.0)
    auto.throttle = 1.0
    try:
        auto.anchored = True
        settle(1.0)

        assert tuple(auto.position) == pytest.approx((0.0, 0.0, 20.0), abs=1e-4), (
            "pinned to the world, so nothing to drive"
        )

        auto.anchored = False
        auto.gravity = False
        settle(1.0)

        assert tuple(auto.position) == pytest.approx((0.0, 0.0, 20.0), abs=1e-4), (
            "and a car gravity has no claim on is left where it was"
        )
    finally:
        auto.destroy()

    # The same refusal on a car that is on the floor rather than above it, since a
    # body touching the ground is where the solver actually has contacts to work with.
    resting = park()
    resting.throttle = 1.0
    try:
        resting.anchored = True
        settle(1.0)

        assert tuple(resting.position) == pytest.approx((0.0, 0.0, 0.6), abs=1e-4), (
            "a throttled car pinned on the floor has still not gone anywhere"
        )

        resting.anchored = False
        resting.gravity = False
        settle(1.0)

        assert tuple(resting.position) == pytest.approx((0.0, 0.0, 0.6), abs=1e-4), (
            "let go with no gravity holding it down, and it neither creeps nor climbs"
        )
    finally:
        resting.destroy()


def test_a_player_at_the_wheel_watches_the_car_it_rides_in():
    auto = park()
    wheel = carbine.new("DriverSeat", parent=auto)
    avatar = rider((0.0, 30.0, 1.0), name="Avatar")
    player = carbine.player()
    player.mode = "third"
    player.eyeHeight = 1.6
    player.distance = 4.0
    player.body = avatar
    player.enabled = True
    try:
        avatar.sit(wheel)
        auto.throttle = 1.0
        settle(2.0)

        assert auto.position.x > 10.0, auto.position
        assert auto.driver == avatar
        assert tuple(player.position) == pytest.approx(tuple(auto.position), abs=1e-4)

        # The lens rides behind the chassis it is watching, at the eye height the
        # player was given - the car, not the walk, decides where the eyes are.
        eye = (auto.position.x, auto.position.y, auto.position.z + 1.44)
        assert tuple(carbine.camera().target) == pytest.approx(eye, abs=1e-3)
        assert tuple(carbine.camera().position) == pytest.approx(
            (eye[0] - 4.0, eye[1], eye[2]), abs=1e-3
        )

        # A rider has no feet to walk with: the keys still read, the legs do not.
        auto.throttle = 0.0
        carbine.injectKey("w", True)
        settle(1.0)

        assert tuple(player.velocity) == (0.0, 0.0, 0.0)
        assert tuple(player.position) == pytest.approx(tuple(auto.position), abs=1e-4)
    finally:
        player.enabled = False
        player.body = None
        carbine.injectKey("w", False)
        avatar.destroy()
        wheel.destroy()
        auto.destroy()


def test_a_car_ploughs_a_prop_lighter_than_it_instead_of_stopping_at_it():
    """Weight has to be worth something to a moving body, or `mass` is only a number a
    script reads back. A two thousand kilogram car riding into a twenty two kilogram cone
    gives way a fortieth of the gap between them, so it keeps nearly all of its own way
    and the cone is left travelling at it."""
    auto = park()
    cone = carbine.new("cube", name="Cone")
    cone.size = (0.4, 0.4, 0.7)
    cone.position = (6.0, 0.0, 0.35)
    cone.collision = True
    cone.gravity = True
    try:
        assert cone.mass == pytest.approx(22.4, abs=0.1)
        assert auto.mass == pytest.approx(WEIGHT)

        auto.throttle = 1.0
        settle(2.0)

        # Past where the cone stood, and still moving: the halt below belongs to scenery,
        # which keeps its ground and takes a car's whole speed for it.
        assert auto.position.x > 8.0, f"the cone stopped a car twice its weight: {auto.position}"
        assert cone.position.x > 10.0, f"and nothing was knocked: {cone.position}"
    finally:
        auto.throttle = 0.0
        cone.destroy()
        auto.destroy()


def test_a_car_cannot_budge_an_anchored_prop():
    """Anchored is the world holding a body, not a body that is merely heavy: another
    body may press on it as hard and as long as it likes, and all it gets back for the
    trouble is its own speed taken away."""
    auto = park()
    bollard = carbine.new("cube", name="Bollard")
    bollard.size = (0.8, 6.0, 3.0)
    bollard.position = (10.0, 0.0, 1.5)
    bollard.collision = True
    bollard.anchored = True
    try:
        auto.throttle = 1.0
        settle(4.0)

        assert tuple(bollard.position) == (10.0, 0.0, 1.5), f"it was shoved: {bollard.position}"
        # Half the chassis plus half the bollard is the closest the two may stand: the
        # car is stopped with its nose on the face rather than driven through it.
        nose = auto.position.x + CHASSIS[0] * 0.5
        assert nose == pytest.approx(9.6, abs=0.05), f"not stopped at the face: {auto.position}"
    finally:
        auto.throttle = 0.0
        bollard.destroy()
        auto.destroy()


def test_a_car_drives_a_block_as_heavy_as_itself_along_at_its_own_pace():
    """A shove is worth more than the few centimetres the two bodies happen to share. A
    block nearly the weight of the car takes half the pace the car arrived with, which is
    what leaves the pair travelling together instead of the car crawling past a body the
    solver had to drag a step at a time."""
    auto = park()
    block = carbine.new("cube", name="Block")
    block.size = (1.0, 1.0, 1.0)
    block.position = (8.0, 0.0, 0.5)
    block.collision = True
    block.gravity = True
    block.mass = 2_000.0
    try:
        auto.throttle = 1.0
        settle(3.0)

        assert block.position.x > 12.0, f"the block was left behind: {block.position}"
        # Half a metre of the block and 2.2 of the chassis is the gap they hold while
        # they ride together.
        gap = block.position.x - auto.position.x
        assert gap == pytest.approx(2.7, abs=0.2), f"the car drove off from it: {gap}"
    finally:
        auto.throttle = 0.0
        block.destroy()
        auto.destroy()

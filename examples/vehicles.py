"""Driving: a car is one object wearing a drive, and a seat is a node hung on it.

    python examples/vehicles.py

`carbine.new("Vehicle")` writes a hatchback - 4.4 by 2 by 1.2 metres, 2112 kilograms at
the world's own two hundred to the cubic metre - already in the solver, already held by
gravity, because only a body the engine moves can answer a wheel. Everything about how
it handles is a knob written on that one node: `type` ("arcade" or "drift", and
`carbine.handlingTypes()` lists them), `topSpeed`, `turnRate`, `grip`. What a driver
asks for is three more knobs, each a wish between -1 and 1: `throttle` forwards, `steer`
across, `brake` down. The engine reads none of those off the keyboard. This file does,
every frame, and that is the whole of the script side of driving - which is also why the
same three knobs are what a controller, an AI or a replay would write.

A seat is a second node, not a switch on the first. `new("Seat", parent=car)` is
somewhere to sit and rides along; `new("DriverSeat", parent=car)` is the seat that
drives, and it drives the object directly above it. The kind word said it, so there is
no `driveParent` to remember to throw. A seat draws nothing and collides with nothing -
it is a place in the car the solver can hold a body at. Its position is written in the
car's own metres, so the parent's size scales it: the two below land 1.1 m ahead of the
centre, 0.44 m to one side and 0.24 m up, where a driver and a stowaway read as being in
the chassis rather than stuck through the nose of it.

`E` puts you in the nearest empty driver seat and takes you out again. While you are in
one the engine stands its walking maths down: the same W and A that step you off on foot
are the ones this file reads for the wheel, and the seat holds you up, so a rider neither
falls nor shoves. Get out of a moving car and you keep its speed for a moment, because
`stand()` puts you down with the speed the seat was going.

Two handling types are parked side by side because they are the only way to feel the
difference. An arcade car's tyres are glued: the nose decides the line and any speed
sideways is gone inside a step, so `2` at speed and a flick of `A` turns it on the spot.
A drift car keeps what it has: throw the same nose across the turn and the car carries on
along its old line while the nose points somewhere else, and `grip` - 0.5 by default, and
the knob scales it - decides how long that lasts. `1` and `2` write the type on whichever
car you are sitting in, so the same corner can be taken twice, and with it the grip that
type is played at: the type only decides what the tyres do with a sideways speed, while
`grip` decides how much of that speed they keep hold of. The wall at the far end is
there because the drive model is not: the car is an ordinary collider, so the solver
stops it and not the engine - and the wall is anchored, which means it gives way none
of it. A car at full throttle stops with its nose on the face and the face stays where
it was put. The cones are there for the other half of the same weight: a pair that
rides into each other trades pace, so a cone gets nearly the whole of the speed the car
came with and the car keeps the part its own weight is worth. A shove moves a body by
giving it the speed of the shove, not by dragging it a few centimetres a step.

The orange car also carries a stowaway in its passenger seat - an entity, sitting in a
plain seat, riding along and driving nothing - which is the difference between the two
seats made visible. The engine never ejects anyone here, and it is worth saying why:
only an entity has hit points, and `new` writes objects, so a car cannot be wrecked and
the death that throws a rider clear has nothing in this scene to fire on. That is covered
in `tests/test_vehicles.py`, on a seat hung under something that can die.

    WASD wheel (throttle and steer) - space brake - E get in or out
    1 arcade - 2 drift, on the car you are in - F eyes or chase lens
    close the window to quit

Frame after frame this file prints where the car you are in actually got to, because
`type`, `grip` and `turnRate` are worth nothing until a car has been driven into a corner
and somebody has read back what it did.
"""

import carbine

ORANGE = "#eb7a34"
NAVY = "#2f6cb3"
GREY = "#6b7280"

# (name, colour, handling) for the two cars, parked side by side facing down the track.
CARS = [("Arcade", ORANGE, "arcade"), ("Slide", NAVY, "drift")]

# Where the two seats sit inside the chassis. A child's position is in its parent's
# own meters, so it is scaled by the parent's size: 0.25 of a 4.4 m hatchback is 1.1 m
# ahead of the centre, and 0.22 of a 2 m wide one is 0.44 m to the left of it.
DRIVER_SEAT = (0.25, 0.22, 0.2)
PASSENGER_SEAT = (0.25, -0.22, 0.2)

# How near a car you have to be to climb into it, and how long the slalom runs for.
REACH = 3.5
CONES = [(x, (2.6 if index % 2 else -2.6)) for index, x in enumerate(range(8, 34, 6))]

# What a chase lens sits behind the body, and how high above the floor the eyes are.
DISTANCE = 7.0
EYES = 1.9


def wheel_of(body):
    """The car this node sits in, and whether the seat it sits in drives. The seat is
    a node like any other, so its parent is the chassis and `drives` is the kind word
    the script typed when it was made."""
    seat = body.seatedIn
    if seat is None:
        return None, False
    return seat.parent, seat.drives


def nearest(parked, at):
    """The car whose centre is closest to `at`, with its driver seat, and how far off it
    is. A car is a body the solver moves, so asking it where it got to is the whole of
    what a person walking up to a row of them does."""
    best, best_distance = None, None
    for car, wheel, _ in parked:
        where = car.position
        distance = ((where.x - at.x) ** 2 + (where.y - at.y) ** 2) ** 0.5
        if best_distance is None or distance < best_distance:
            best, best_distance = (car, wheel), distance
    if best is None or best_distance > REACH:
        return None, best_distance or 0.0
    return best, best_distance


def wish(held, forwards, back):
    """One of the three knobs, read off the keyboard: +1 for the first key, -1 for the
    second, 0 for neither and both, since two feet on two pedals ask for nothing."""
    return 1.0 if (held(forwards) and not held(back)) else (-1.0 if held(back) else 0.0)


def main() -> None:
    adapter = carbine.init(1280, 720, title="Carbine Vehicles")
    width, height = carbine.viewport()
    print(
        f"{adapter} at {width}x{height}\n"
        "WASD wheel - space brake - E get in or out - F eyes or chase lens\n"
        "1 arcade - 2 drift, on the car you are in - close the window to quit"
    )
    print("the kinds a vehicle needs:", ", ".join(carbine.kinds()))
    print("the handlings a car answers:", ", ".join(carbine.handlingTypes()))

    ground = carbine.new("plane", name="Ground")
    ground.size = (140.0, 140.0, 1.0)
    ground.color = "#3d4a3a"

    # Something to stop a car, and the reason the drive model needs no wall of its own:
    # the collider is the engine's and the halt is the solver's.
    wall = carbine.new("cube", name="Wall")
    wall.size = (0.8, 40.0, 3.0)
    wall.position = (40.0, 0.0, 1.5)
    wall.color = GREY
    wall.collision = True
    wall.anchored = True

    cones = []
    for index, (across, away) in enumerate(CONES):
        cone = carbine.new("cube", name=f"Cone {index}")
        cone.size = (0.4, 0.4, 0.7)
        cone.position = (across, away, 0.35)
        cone.color = ORANGE
        # In the solver, so a car that arrives sideways at speed takes them with it.
        cone.collision = True
        cone.gravity = True
        cones.append(cone)

    cars = []
    for index, (label, tint, handling) in enumerate(CARS):
        car = carbine.new("Vehicle", name=label)
        car.position = (0.0, 2.5 * (1 if index else -1), 0.6)
        car.color = tint
        car.type = handling
        car.grip = 0.35 if handling == "drift" else 1.0
        wheel = carbine.new("DriverSeat", name=f"{label} wheel", parent=car)
        wheel.position = DRIVER_SEAT
        bench = carbine.new("Seat", name=f"{label} seat", parent=car)
        bench.position = PASSENGER_SEAT
        cars.append((car, wheel, bench))
        print(
            f"{label}: {car.mass:g} kg, {car.type}, up to {car.topSpeed:g} m/s, "
            f"nose swings {car.turnRate:g} deg/s, grip {car.grip:g}"
        )

    # One rider that is not you, in the plain seat of the first car, to make the two
    # kinds of seat visible: it rides along and drives nothing.
    stowaway = carbine.spawnEntity("cube", name="Stowaway")
    stowaway.size = (0.7, 0.7, 1.2)
    stowaway.color = NAVY
    stowaway.sit(cars[0][2])

    suit = carbine.spawnEntity("cube", name="Suit")
    suit.size = (0.6, 0.6, 1.7)
    suit.color = ORANGE

    player = carbine.player()
    player.body = suit
    player.position = (-4.0, -6.0, 0.0)
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

    wheel_bar = carbine.ui.new("rectangle", name="Wheel", parent=panel)
    wheel_bar.position = (90, 8, 0)
    wheel_bar.size = (4.0, 10.0, 1)
    wheel_bar.color = ORANGE
    wheel_bar.corner = 5

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
                    car = None
                    driving = False
                else:
                    reached, away = nearest(cars, player.position)
                    if reached is None:
                        print(f"nothing to climb into within {REACH:g} m")
                    elif reached[1].seated is not None:
                        print(f"the {reached[0].name} wheel is already taken")
                    else:
                        suit.sit(reached[1])
                        print(
                            f"in the {reached[0].name} - {away:.1f} m away, "
                            f"{reached[0].type}, grip {reached[0].grip:g}"
                        )

            if carbine.pressed("f"):
                player.mode = "first" if player.mode == "first" else "third"
                print(f"{'eyes' if player.mode == 'first' else 'chase'} lens")

            if carbine.pressed("1") or carbine.pressed("2"):
                asked = "arcade" if carbine.pressed("1") else "drift"
                if car is None:
                    print(f"{asked} asked for, but nobody is sitting in a car")
                else:
                    car.type = asked
                    # The type alone only changes what the tyres do with a sideways speed.
                    # How much of it they keep hold of is the second knob, and a script
                    # that asks for a slide has to write it too.
                    car.grip = 0.35 if asked == "drift" else 1.0
                    print(f"{car.name} handles {car.type} now, grip {car.grip:g}")

            # The three wishes, and only while somebody is at a wheel that drives. On
            # foot the keys belong to the engine's own walker, and in a passenger seat
            # they belong to nobody.
            if car is not None and driving:
                car.throttle = wish(carbine.key, "w", "s")
                car.steer = wish(carbine.key, "d", "a")
                car.brake = 1.0 if carbine.key("space") else 0.0
                gone = car.position
                if last is not None and step > 0.0:
                    pace = ((gone.x - last[0]) ** 2 + (gone.y - last[1]) ** 2) ** 0.5 / step
                last = (gone.x, gone.y)
                # Two bars, in pixels: how far the pace has filled up against the car's
                # own top speed, and which way and how hard the wheel is turned.
                pace_bar.size = (4.0 + 110.0 * min(1.0, pace / car.topSpeed), 10.0, 1)
                wheel_bar.size = (4.0 + 110.0 * abs(car.steer), 10.0, 1)
                wheel_bar.color = ORANGE if car.steer >= 0.0 else NAVY
            else:
                if last is not None:
                    print(f"left it at ({last[0]:.1f}, {last[1]:.1f}) m")
                last, pace = None, 0.0
                for each, _, _ in cars:
                    each.throttle = each.steer = each.brake = 0.0
                pace_bar.size = (4.0, 10.0, 1)
                wheel_bar.size = (4.0, 10.0, 1)

            if frame % 120 == 0 and car is not None:
                print(
                    f"{car.name} at ({car.position.x:.1f}, {car.position.y:.1f}) m - "
                    f"{car.position.z:.2f} off the floor - {pace:.1f} m/s - "
                    f"throttle {car.throttle:+g} steer {car.steer:+g} brake {car.brake:+g}"
                )

            frame += 1
    finally:
        carbine.end()


if __name__ == "__main__":
    main()

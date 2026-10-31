"""Walk around a 3D world under your own control.

    python examples/player.py

WASD walks, the mouse looks, space jumps. None of that is scripted here: the
engine reads the keyboard and drives the camera during every `carbine.render()`.
The script only says who the player is and how fast they may go.

Everything with a collider switched on is solid, so the pillars, the wall, the
boulder and the barrel stop the player dead. The barrel is drawn as a box but
collides as a capsule, because the shape is the solver's business and not the
picture's. The lava plate and the two actors hurt on contact, and `E` lands a
scripted hit on whichever actor is closest. Health running out is the engine's
ending to write: the Golem is left as a corpse that falls and stops blocking
anything, the Wisp is asked to leave nothing behind. When it is the body the player
wears that runs out, the engine only takes it off and leaves it on the floor - that
it stops the feet and waits for `R` is this script's decision, where another could
end the run or park the camera on the corpse instead.

Escape hands the pointer back to the desktop, a click asks for it again, and F
switches between the player's eyes and a camera behind them.

The loop has no sleep in it, and the engine holds nothing by default: no cap, no sync, so
the frames run as fast as the machine will have them. This game asks for both - 90 so the
GPU does not scream, and the panel's own rate as a second limit - and `V` trades the
second one off again.
"""

import carbine

ORANGE = "#eb7a34"
NAVY = "#2f6cb3"
LAVA = "#c93b2b"

HEALTH = 30.0
SPEED = 5.0

# kind, name, where its centre sits, how big, and the shape the solver cuts it as.
SCENERY = [
    ("cube", "Pillar", (-4.0, 3.5, 1.2), (1.2, 1.2, 2.4), "box"),
    ("cube", "Wall", (2.5, 6.0, 1.5), (5.0, 0.6, 3.0), "box"),
    ("sphere", "Boulder", (7.0, -2.0, 1.0), (2.0, 2.0, 2.0), "sphere"),
    ("cube", "Barrel", (-6.5, -5.0, 0.7), (1.0, 1.0, 1.4), "capsule"),
]


def closest(where, actors):
    """The living actor nearest `where` on the floor, or nothing once they are all
    down. The engine has no query for entities, so the script keeps its own list -
    and forgets the actors whose bodies were removed when they died."""
    living = []
    for actor in actors:
        try:
            standing = actor.alive
        except carbine.DestroyedError:
            continue
        if standing:
            living.append(actor)
    return min(
        living,
        key=lambda a: (a.position.x - where.x) ** 2 + (a.position.y - where.y) ** 2,
        default=None,
    )


def status(actor):
    """What is left to say about an actor once the engine has finished with it: the
    hit points still standing, a corpse still in the world, or nothing at all."""
    try:
        return f"{actor.health:g} hp" if actor.alive else "corpse"
    except carbine.DestroyedError:
        return "gone"


def main() -> None:
    # Left alone the engine holds the frames to nothing at all, so this game names
    # both limits itself, before there is a window to name them to: 90 so the GPU
    # does not scream, and the panel's own rate in case it turns out to be less.
    print(
        f"the engine starts held by nothing: cap {carbine.fpsCap}, "
        f"sync {carbine.monitorSync}, limit {carbine.fpsLimit} fps"
    )
    carbine.fpsCap = 90
    carbine.monitorSync = True
    adapter = carbine.init(1280, 720, title="Carbine Player")
    width, height = carbine.viewport()
    print(
        f"{adapter} at {width}x{height}\n"
        "WASD walk - mouse look - space jump - E hit - R get up - F camera\n"
        "V trades the frame cap for the monitor's own rate - close the window to quit\n"
        "ESC releases the mouse"
    )
    print("shapes a collider can be cut as:", ", ".join(carbine.collisionShapes()))

    ground = carbine.new("plane", name="Ground")
    ground.size = (48.0, 48.0, 1.0)
    ground.color = "#3d4a3a"

    scenery = []
    for index, (kind, label, where, across, shape) in enumerate(SCENERY):
        block = carbine.new(kind, name=f"{label} {index}")
        block.position = where
        block.size = across
        block.color = NAVY if index % 2 else "#6b7280"
        # The invitation to the solver, and the only reason the player cannot
        # stroll through any of these. Asking for a shape switches it on.
        block.collisionShape = shape
        scenery.append(block)

    # A crate the solver actually owns: gravity is on, so each frame drops it and
    # rests it on the ground plane. No collider, so nothing walks around it.
    crate = carbine.new("cube", name="Crate")
    crate.size = (1.0, 1.0, 1.0)
    crate.position = (3.0, -7.0, 6.0)
    crate.color = "#8a6d3b"
    crate.gravity = True

    # A hazard in the player's way: an entity, because scenery has no damage to
    # give, left without gravity so the solver counts it as ground that does not
    # budge. Standing in it costs six hit points every half second, so a suit of
    # thirty burns out in under three - long enough to walk out of it.
    lava = carbine.spawnEntity("cube", name="Lava", health=1000.0)
    lava.size = (2.4, 2.4, 0.2)
    lava.position = (2.5, -3.0, 0.1)
    lava.color = LAVA
    lava.collision = True
    lava.damage = 6.0

    golem = carbine.spawnEntity("cube", name="Golem", health=30.0, damage=2.0)
    golem.size = (0.9, 0.9, 1.8)
    golem.position = (6.0, -3.0, 0.9)
    golem.color = NAVY
    golem.collision = True

    # The one actor told not to stay: its node, and the hat pinned under it, go
    # with it.
    wisp = carbine.spawnEntity("sphere", name="Wisp", health=4.0, damage=1.0)
    wisp.size = (1.0, 1.0, 1.0)
    wisp.position = (-2.5, -1.0, 0.9)
    wisp.color = "#9a7bd6"
    wisp.collision = True
    wisp.stayAfterDeath = False
    hat = carbine.new("cube", name="Hat", parent=wisp.object)
    hat.size = (0.6, 0.6, 0.2)
    hat.position = (0.0, 0.0, 0.6)
    hat.color = "#221e1b"
    actors = [golem, wisp]

    # The body the player wears is an entity, which is what makes it hurt-able:
    # contact damage lands on the node the player is drawn as, and its `damage` is
    # what a scripted hit is measured in.
    suit = carbine.spawnEntity("cube", name="Suit", health=HEALTH, damage=3.0)
    suit.size = (0.6, 0.6, 1.7)
    suit.color = ORANGE

    player = carbine.player()
    player.body = suit
    player.position = (0.0, -3.0, 0.0)
    player.speed = SPEED
    player.enabled = True

    panel = carbine.ui.new("rectangle", name="Hud")
    panel.position = (120, height - 44, 0)
    panel.size = (240, 36, 1)
    panel.color = "#221e1b"
    panel.corner = 10
    panel.transparent = 190

    # Two bars: health, and a pace that fills as the player picks up speed, which
    # is the engine chasing the target velocity rather than snapping to it.
    life = carbine.ui.new("rectangle", name="Health", parent=panel)
    life.position = (-60, -8, 0)
    life.size = (4.0 + 110.0, 10.0, 1)
    life.color = ORANGE
    life.corner = 5

    bar = carbine.ui.new("rectangle", name="Speed", parent=panel)
    bar.position = (-60, 8, 0)
    bar.size = (4.0, 10.0, 1)
    bar.color = NAVY
    bar.corner = 5

    crosshair = carbine.ui.new("circle", name="Crosshair")
    crosshair.position = (width / 2.0, height / 2.0, 0)
    crosshair.size = (10.0, 10.0, 1)
    crosshair.color = ORANGE
    crosshair.transparent = 170

    frame = 0
    told = False
    try:
        while carbine.running():
            carbine.render()

            # Said as soon as the desktop names the screen, which on Wayland is
            # after a frame is up rather than at `init` - the pace is only true
            # once there is a panel to compare the cap against.
            if not told and carbine.monitorHz:
                told = True
                print(
                    f"holding {carbine.fpsLimit} fps: cap {carbine.fpsCap}, "
                    f"screen {carbine.monitorHz} Hz, "
                    f"monitor sync {carbine.monitorSync}"
                )

            # Input the engine does not own: F trades the eyes for a camera behind
            # the body, and the crosshair only means something in the eyes.
            if carbine.pressed("f"):
                player.mode = "third" if player.mode == "first" else "first"

            # The other half of the same wish: unlocked, the cap is the only thing
            # holding the frames; synced, the panel is - and the lower of the two
            # is what runs either way.
            if carbine.pressed("v"):
                carbine.monitorSync = not carbine.monitorSync
                print(
                    f"sync {carbine.monitorSync}: cap {carbine.fpsCap}, "
                    f"screen {carbine.monitorHz} Hz, holding {carbine.fpsLimit} fps"
                )

            # What a death means is the script's call, not the engine's. By the time
            # this reads false the engine has already taken the body off the player
            # and left it on the floor, so this decides to stand still and wait for
            # R - where another script could end the run, or park the camera on the
            # corpse and let the player watch.
            down = not suit.alive
            player.speed = 0.0 if down else SPEED
            if down and carbine.pressed("r"):
                suit.health = HEALTH
                suit.gravity = False
                player.position = (0.0, -3.0, 0.0)
                player.body = suit
                player.speed = SPEED
                print("back up")
            crosshair.visible = player.mode == "first" and not down

            # A hit from anywhere, with no contact needed. It costs the target the
            # suit's damage, and the target's own `hitPause` decides whether it
            # actually lands.
            if carbine.pressed("e"):
                target = closest(suit.position, actors)
                if target is None:
                    print("nothing left standing")
                else:
                    name = target.name
                    dealt = suit.hit(target)
                    print(f"hit {name} for {dealt:g} - {status(target)}")

            velocity = player.velocity
            pace = (velocity.x**2 + velocity.y**2) ** 0.5
            bar.size = (4.0 + 110.0 * min(1.0, pace / SPEED), ..., ...)
            remaining = max(0.0, suit.health / HEALTH)
            life.size = (4.0 + 110.0 * remaining, ..., ...)
            life.color = LAVA if remaining < 0.35 else ORANGE

            if frame % 120 == 0:
                where = player.position
                # The pace the frames come at, next to the pace the player has reached.
                shown = f"{1.0 / carbine.dt():.0f} fps" if carbine.dt() else "starting"
                if down:
                    print(
                        f"down at ({where.x:.1f}, {where.y:.1f}) m - R to get back up"
                    )
                else:
                    met = ", ".join(node.name for node in suit.touching()) or "nothing"
                    print(
                        f"at ({where.x:.1f}, {where.y:.1f}, {where.z:.1f}) m, "
                        f"{pace:.1f} m/s, {shown}, {player.mode} person, "
                        f"mouse {'held' if player.grabMouse else 'free'}, "
                        f"{suit.health:.0f} hp, touching {met}"
                    )

            frame += 1
    finally:
        # The suit, the actors, the lava and the ground are all nodes the world was
        # asked to hold, so one call returns them - and the window - at once.
        carbine.end()


if __name__ == "__main__":
    main()

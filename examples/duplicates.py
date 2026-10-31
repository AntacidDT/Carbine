"""Copies of a node: `carbine.duplicate()` and `carbine.linkDuplicate()`.

    python examples/duplicates.py

A street of lamp posts and a yard of crates, both written from one prototype each.

The posts are linked duplicates: twelve nodes tied to one configuration, so painting
one paints all twelve and a wound stays with the one that took it. The crates are
plain duplicates: forty bodies that copied the prototype's look and settings and share
nothing with it after that, so one can be repainted, moved or dropped out of the sky
without the rest noticing.

That is the bargain Blender strikes with a linked duplicate - the data block is common,
the object hanging off it is not - and the split is one sentence: what a script writes
about what a thing *is* goes to the whole group, and what happens *to* one of its
members - where it stands, how it is turned, how big it is, how much of it is left -
stays with that member. Every copy starts where the prototype stood, still, and hangs
under the same parent; a branch comes across whole, as copies of its own.

    p          paint the lamp group - one write, twelve posts
    k          knock the next lamp over - its own turn, its twins stay upright
    h          bite the nearest lamp - its own health, and its twins keep theirs
    n          link six more lamps into the same group
    d          drop a plain duplicate of a lamp: it takes the look, not the sharing
    c          duplicate twenty more crates into the yard
    x          clear the world and lay the street back out from one prototype

The fifty-odd bodies are all cut from the same mesh, so the GPU is told about that
geometry once however many copies there are; only the transforms differ per frame.
"""

import carbine

ORANGE = "#eb7a34"
GLASS = "#d8c8a8"
STEEL = "#8b93a6"

POST_HEIGHT = 6.0
LAMP_SIDE = -6.0

# Where the lamps stand, and where the crates are dropped from. Both are written out
# rather than rolled, so the same run lays the same street twice.
ALONG = [(-14.0 + 4.0 * step, LAMP_SIDE, POST_HEIGHT / 2.0) for step in range(8)]
SPOTS = [
    (-10.0 + 2.4 * (index % 7), 5.0 + 2.4 * (index // 7), 1.0 + 1.6 * (index % 5))
    for index in range(20)
]


def build():
    """The street, from two prototypes: one lamp, one crate."""
    lamp = carbine.spawnEntity("cube", name="Lamp", health=40.0)
    lamp.size = (0.35, 0.35, POST_HEIGHT)
    lamp.color = STEEL
    lamp.collisionShape = "box"
    lamp.position = ALONG[0]

    # The bulb hangs off the post before the post is copied, so the branch comes
    # across with it - each lamp gets a bulb of its own, and the bulb is not in the
    # lamp's group: only the node that was named joins the sharing.
    bulb = carbine.new("sphere", name="Bulb", parent=lamp)
    bulb.size = (0.7, 0.7, 0.7)
    bulb.position = (0.0, 0.0, POST_HEIGHT / 2.0)
    bulb.color = GLASS

    lamps = [lamp, *carbine.linkDuplicate(lamp, len(ALONG) - 1)]
    for post, at in zip(lamps, ALONG):
        post.position = at

    crate = carbine.new("cube", name="Crate")
    crate.size = (0.9, 0.9, 0.9)
    crate.color = "#8a6d3b"
    crate.gravity = True
    crate.collisionShape = "box"
    crate.position = SPOTS[0]
    crates = [crate, *carbine.duplicate(crate, len(SPOTS) - 1)]
    for box, at in zip(crates, SPOTS):
        box.position = at

    ground = carbine.new("plane", name="Ground")
    ground.size = (80.0, 80.0, 1.0)
    ground.color = "#3d4a3a"

    hammer = carbine.spawnEntity(name="Hammer", health=1.0, damage=13.0)
    return lamps, crates, hammer


def main() -> None:
    adapter = carbine.init(1280, 720, title="Carbine Duplicates")
    width, height = carbine.viewport()
    carbine.fpsCap = 60
    print(
        f"{adapter} at {width}x{height}, held at {carbine.fpsLimit} fps\n"
        "WASD walk - mouse look - space jump\n"
        "p paint the lamps - k knock one over - h bite one - n link six more "
        "- d plain copy a lamp - c twenty more crates - x rebuild\n"
        "ESC releases the mouse - close the window to quit"
    )

    lamps, crates, hammer = build()
    fallen = 0
    loose = 0
    frame = 0

    player = carbine.player()
    player.position = (0.0, -18.0, 0.0)
    player.enabled = True

    try:
        while carbine.running():
            carbine.render()

            if carbine.pressed("p"):
                lamps[0].color = ORANGE
                print(
                    f"{len(lamps)} posts share one colour now: "
                    f"{set(post.colorHex for post in lamps)}"
                )

            if carbine.pressed("k") and fallen < len(lamps):
                post = lamps[fallen]
                post.rotationEuler = (0.0, 90.0, 0.0)
                post.position = (post.position.x, post.position.y, 0.4)
                fallen += 1
                print(
                    f"lamp {fallen} is on its side at "
                    f"{[round(value) for value in post.rotationEuler.toTuple()]}, "
                    f"the other {len(lamps) - fallen} still stand up"
                )

            if carbine.pressed("h") and lamps:
                # The nearest post is the one the player is standing by; a bite is
                # dealt through the engine, and only the target's health moves.
                target = min(
                    lamps,
                    key=lambda post: sum(
                        (axis - here) ** 2
                        for axis, here in zip(post.position, player.position)
                    ),
                )
                landed = hammer.hit(target)
                print(
                    f"bit for {landed} hp - "
                    f"{', '.join(str(int(post.health)) for post in lamps[:6])} left"
                )

            if carbine.pressed("n"):
                more = carbine.linkDuplicate(lamps[0], 6)
                for post, step in zip(more, range(len(lamps), len(lamps) + 6)):
                    post.position = (-14.0 + 4.0 * step, LAMP_SIDE, POST_HEIGHT / 2.0)
                lamps.extend(more)
                print(f"the group is {len(lamps)} posts now, and one write covers them all")

            if carbine.pressed("d"):
                at = (2.0 * loose, LAMP_SIDE - 3.0, POST_HEIGHT / 2.0)
                # A plain duplicate: it copies what the lamps are, then belongs to
                # nothing. Repainting it leaves the street its own colour.
                copy = carbine.duplicate(lamps[0])[0]
                copy.position = at
                copy.color = "#3f6d4e"
                loose += 1
                print(
                    f"copy {loose} is {copy.colorHex}, the group is {lamps[0].colorHex}, "
                    f"and neither moved the other"
                )

            if carbine.pressed("c"):
                base = len(crates)
                more = carbine.duplicate(crates[0], 20)
                for step, box in enumerate(more):
                    index = (base + step) % len(SPOTS)
                    box.position = (
                        SPOTS[index][0] + 0.3 * step,
                        SPOTS[index][1],
                        1.0 + 2.0 * (step % 6),
                    )
                crates.extend(more)
                print(f"{len(crates)} crates in the yard, all falling their own way")

            if carbine.pressed("x"):
                carbine.destroy("all")
                lamps, crates, hammer = build()
                fallen = 0
                loose = 0
                print("rebuilt from one lamp and one crate, and nothing remembers the fire")

            if frame % 120 == 0:
                upright = sum(1 for post in lamps if abs(post.rotationEuler.y) < 1.0)
                print(
                    f"at ({player.position.x:.1f}, {player.position.y:.1f}, "
                    f"{player.position.z:.1f}) m: {carbine.scene.nodeCount} nodes, "
                    f"{len(lamps)} linked lamps with {upright} still upright, "
                    f"{len(crates)} plain crates"
                )

            frame += 1
    finally:
        carbine.end()


if __name__ == "__main__":
    main()

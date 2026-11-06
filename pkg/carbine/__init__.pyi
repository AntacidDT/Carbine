import os

__version__: str

class DestroyedError(RuntimeError): ...

class Vec3Iterator:
    def __iter__(self) -> Vec3Iterator: ...
    def __next__(self) -> float: ...

class Vec3:
    def __init__(self, x: float = 0.0, y: float = 0.0, z: float = 0.0) -> None: ...
    x: float
    y: float
    z: float
    def __len__(self) -> int: ...
    def __iter__(self) -> Vec3Iterator: ...
    def __add__(self, other: Vec3) -> Vec3: ...
    def __sub__(self, other: Vec3) -> Vec3: ...
    def __mul__(self, other: float) -> Vec3: ...
    def __rmul__(self, other: float) -> Vec3: ...
    def toTuple(self) -> tuple[float, float, float]: ...

class Quat:
    def __init__(
        self, x: float = 0.0, y: float = 0.0, z: float = 0.0, w: float = 1.0
    ) -> None: ...
    x: float
    y: float
    z: float
    w: float
    @staticmethod
    def identity() -> Quat: ...
    @staticmethod
    def fromEuler(x: float, y: float, z: float) -> Quat: ...
    def toEuler(self) -> tuple[float, float, float]: ...

class Mesh:
    @property
    def positions(self) -> list[Vec3]: ...
    @property
    def normals(self) -> list[Vec3]: ...
    @property
    def uvs(self) -> list[tuple[float, float]]: ...
    @property
    def indices(self) -> list[int]: ...
    @property
    def vertexCount(self) -> int: ...
    @property
    def triangleCount(self) -> int: ...
    @property
    def bounds(self) -> tuple[Vec3, Vec3] | None: ...

class Object:
    """A world or UI node; Text has scalar size and boolean outline, other sizes are live Vec3 values."""

    id: int
    name: str
    kind: str
    # The node this one hangs under, which carries it along with its own transform.
    # Only the scene root has no parent. Writing None moves the node to the root.
    # An actor's parent reads back as an `Entity`, plain scenery as an `Object` and
    # a particle system as a `Particle`.
    parent: Object | Entity | Particle | None
    # What hangs under this node, in the order it is walked - which is also the
    # order UI elements are painted in, so the last one sits on top.
    # Writing a list, or a single node, replaces the branch: the order given
    # becomes the order walked, children left out move to the scene root, and a
    # node listed here but hanging elsewhere is adopted. Nothing is destroyed.
    children: list[Object | Entity | Particle]
    # Hangs another node on the end of this one's children.
    def addChild(self, child: Object | Entity | Particle) -> None: ...

    # Takes a child off this node and hangs it on the scene root. It leaves the
    # branch, not the world - `destroy` is the call that means that.
    def removeChild(self, child: Object | Entity | Particle) -> None: ...
    # Vector writes accept Vec3 or (x, y, z); None or ... preserves an axis.
    position: Vec3
    # Text font size is 1..400 pixels; other nodes return live (length, width, height) Vec3.
    size: Vec3 | float
    scale: Vec3
    # Read as a Quat, written as a Quat or as euler degrees (x, y, z).
    rotation: Quat
    rotationEuler: Vec3
    # Colours accept Vec3, RGB tuples, named colours or hex strings with optional alpha.
    # Presets include red, light blue, dark green, banana, skin color, cream, navy and gold.
    color: Vec3 | str
    @property
    def colorHex(self) -> str: ...
    # Light knobs, for a node made with `carbine.new("Light")`. A world object raises
    # on them, the way it does on the UI knobs further down.
    # "point" for a lamp giving light out of one place, "sun" for daylight thrown from
    # everywhere in one direction. Read, not written: the sun the world is lit by is
    # the node `carbine.scene.sun` answers with, and a lamp is not made a second one.
    shape: str
    # How strongly the light lights. 1 is what a light is born with, past that it is
    # brighter, and 0 leaves it dark.
    brightness: float
    # Metres of world a lamp reaches: past the end of it it lights nothing at all.
    reach: float
    # How this light lays what it gives: "straight" or "blocky". A light nobody wrote
    # on follows `carbine.lightType`, the world's own answer.
    # On a vehicle the same word is how it handles instead: "arcade", where the nose
    # decides the line, or "drift", where the tyres can be beaten and the car slides
    # across its old line. `carbine.handlingTypes()` lists those.
    type: str
    # Vehicle knobs. `new("Vehicle")` is one from the first moment - a hatchback 4.4 by 2
    # by 1.2 metres, in the solver and held by gravity - and anything else becomes one by
    # writing `driveable`, which hangs a drive on the node and asks nothing else of it: a
    # loaded tank keeps the size, mass and collider it arrived with. The rest below is
    # refused on a node that is not a vehicle, the way it refuses the light knobs above.
    driveable: bool
    # Metres per second a full throttle is chasing.
    topSpeed: float
    # Degrees per second the nose turns at full lock.
    turnRate: float
    # How much of a sideways shove the tyres keep hold of, 0 for ice to 1 for gripper
    # tarmac. An arcade car reads it as 1 and never lets the car slide at all.
    grip: float
    # The three things a driver asks for, each between -1 and 1: the accelerator
    # forwards, the wheel across, the brake down. They are wishes rather than speeds -
    # the handling works out from them where the car goes - and a script that reads the
    # keyboard writes them every frame. Unlike the three above they are one car's own
    # business, so a write does not travel through a `linkDuplicate` group.
    throttle: float
    steer: float
    brake: float
    # Whoever is at the wheel: the first of this car's seats that drives and has
    # somebody in it. None for a car nobody has sat in; anything that is not a vehicle
    # refuses, the way the knobs above do.
    @property
    def driver(self) -> Object | Entity | Particle | None: ...
    # Seat knobs. True for a `new("DriverSeat")`, which drives whatever vehicle it hangs
    # on, and false for a plain `new("Seat")`, which only rides along. Read, not written:
    # the kind word said it when the node was made. A node that is not a seat refuses.
    drives: bool
    # Whoever is riding in this seat, as the handle the rider really is. None while it
    # is empty; a node that is not a seat refuses, the way `drives` does.
    @property
    def seated(self) -> Object | Entity | Particle | None: ...
    # Which seat this node is riding in, or None for one standing on its own feet.
    @property
    def seatedIn(self) -> Object | Entity | Particle | None: ...
    # Sits this node in a seat. It stays on its own branch, with its own name and
    # children, and the step holds it at the seat's place for as long as it is in;
    # nothing is re-parented. Whoever was in that seat before gets out.
    def sit(self, seat: Object) -> None: ...

    # Gets out of whatever this node was sitting in, left exactly where the seat had it
    # and keeping the speed the seat was going - so stepping out of a moving car is a
    # moment of running. Asking while on your own feet is not an error, which makes the
    # same key safe for both ways.
    def stand(self) -> None: ...
    # The packed image (`.ktx2`) painted over the object's colour, as a path. None
    # paints it flat, and an image in any other format is refused - pack it with
    # `carbine.packager.image(...)` and use the path that answers.
    texture: str | None
    # How many metres of the object one tile of its skin covers. A tile stays the size
    # it was painted for: the same picture is as big on the six metre wall as on the
    # floor tile beside it, because every face is measured along its own two axes and
    # laid once per metre of each, so nothing stretches to fit a longer face. One metre
    # left alone; `0` smears one whole picture across each face instead, which is how a
    # model's own UVs already reach the screen. A face holding more than one tile is run
    # in rather than stamped - each repeat nudged off its lattice and worn to a tone of
    # its own - so a long wall reads as one surface instead of a row of copies.
    textureTiling: float
    # The model file worn here, as the path it was asked for. Assign a `.glb` and the
    # file's own nodes come in as children of this one - a whole vehicle, with every
    # turret, door and seat still a node of its own, named what the file named it -
    # and `None` takes that branch off again, leaving whatever the script hung here
    # itself in place. `path#Name` hangs one named branch out of a file holding a
    # whole hangar. Carbine reads `.glb` alone, so a model written in another format
    # is refused until it has been turned into one.
    model: str | None
    # The first node under this one carrying the name, searched down the whole branch
    # rather than one level at a time. An import names what it brings, so this is how
    # a script gets hold of a turret, a door or a seat again. None if nothing is named
    # that, here or below.
    def find(self, name: str) -> Object | Entity | Particle | None: ...
    # False leaves the object, and everything pinned under it, out of the frame.
    visible: bool
    # How the object takes part in `step`. Gravity is off by default.
    anchored: bool
    gravity: bool
    # Metres per second squared: 10 is Earth, 1.6 the Moon, 0 lets it hang.
    gravityPull: float
    # Kilograms. Left alone, an object weighs the box it is drawn from at two
    # hundred kilograms to the cubic metre, so a metre cube is 200. Weight decides
    # how much of a push a body takes, never how fast gravity pulls it.
    mass: float
    # In the solver, or out of it. Nothing collides until a script says so, and a
    # body that never asked falls straight through everything.
    collision: bool
    # How the solver reads the body: "box", "sphere" or "mesh". A box is the `size`
    # drawn around it, a sphere a ball as wide as its widest way, and a mesh the
    # body's own surfaces, each one a solid the solver asks about on its own - so
    # the room inside a loaded vehicle stays empty instead of filled in by a brick.
    # A write switches collisions on; a read before that gives the type the body
    # would be cut as. `carbine.collisionTypes()` lists them.
    collisionType: str
    # UI overlay knobs; a world object raises on them. Sizes are pixels.
    corner: float
    polygon: int
    # Text takes a bool for its black outline; shapes take pixel thickness as a float.
    outline: bool | float
    transparent: float
    # Text-only properties; numeric Text inputs reject NaN and infinity with ValueError.
    # The bundled font covers ASCII and Latin-1; unsupported characters display '?'.
    content: str
    wordWrap: bool
    # Wrapping width in pixels, clamped to 1..16384.
    width: float
    # Outline pixels, stored in 0..32 and drawn up to 0.6 times the font size.
    outlineWidth: float
    @property
    def mesh(self) -> Mesh: ...
    def touching(self) -> list[Object | Entity]: ...
    def destroy(self) -> None: ...

class Entity:
    id: int
    name: str
    kind: str
    @property
    def object(self) -> Object | None: ...

    # An actor is a node like any other, so it hangs in the same tree and rides
    # along with whatever it is pinned to - body or no body.
    parent: Object | Entity | Particle | None
    children: list[Object | Entity | Particle]
    def addChild(self, child: Object | Entity | Particle) -> None: ...
    def removeChild(self, child: Object | Entity | Particle) -> None: ...
    # Hit points remaining. At zero the engine has already dealt with the death.
    # Writing a positive number back is the only revive there is, and what a death
    # means for the run - end it, spectate the corpse, let the player up - is the
    # script's decision, not the engine's.
    health: float
    # Hit points this actor deals, on contact and through `hit`.
    damage: float
    position: Vec3
    size: Vec3
    rotation: Quat
    rotationEuler: Vec3
    # Reads as Vec3; accepts Vec3, RGB tuples, named colours or hex strings.
    color: Vec3 | str
    # A packed `.ktx2` path, or None for the actor's own colour.
    texture: str | None
    # Metres of the actor's body one tile of its skin covers, as on `Object`; `0` lays
    # one whole picture across each face.
    textureTiling: float
    # The `.glb` this actor's body wears. Its nodes come in as children of the actor,
    # named what the file names them, so an imported rig is still a set of parts a
    # script can reach; None takes them off. Same rules as `Object.model`.
    model: str | None
    # The first node under this actor carrying the name, anywhere down the branch.
    def find(self, name: str) -> Object | Entity | Particle | None: ...
    visible: bool
    anchored: bool
    gravity: bool
    gravityPull: float
    # Kilograms, or what the actor's box weighs unwritten.
    mass: float
    collision: bool
    collisionType: str
    # Whether there is any health left. A corpse answers false, takes no more
    # hits, and cannot hurt anything.
    @property
    def alive(self) -> bool: ...
    # Seconds one hit holds the next off, contact or scripted. 0.5 by default.
    hitPause: float
    # True leaves the body lying in the world when it dies, inert but still
    # falling. False removes the node, and everything pinned under it.
    stayAfterDeath: bool
    def touching(self) -> list[Object | Entity]: ...
    # Which seat this actor is riding in, or None standing on its own feet.
    @property
    def seatedIn(self) -> Object | Entity | Particle | None: ...
    # Sits this actor in a seat, and the step holds it there: an actor in a car neither
    # falls nor shoves. Whoever was in that seat before gets out.
    def sit(self, seat: Object) -> None: ...

    # Gets out, where the seat had this actor and still carrying the seat's speed. A
    # dying actor is stood out by the engine, with a shove, whether or not anybody asks.
    def stand(self) -> None: ...
    # Bites `other` for this actor's `damage` and answers with the hit points that
    # actually landed: zero when the target was already dead or still hurting, or
    # when this actor is a corpse with nothing to give.
    def hit(self, other: Entity) -> float: ...
    def kill(self) -> None: ...
    def destroy(self) -> None: ...

class Camera:
    """Meters, z-up. `position` floats above the world and `target` is the point
    the lens aims at, which is what gives the frame its depth."""

    position: Vec3
    target: Vec3
    # "perspective" (the default) or "orthographic".
    mode: str
    fov: float
    near: float
    far: float
    # Orthographic only: the vertical size of the visible world in meters.
    viewHeight: float
    def lookAt(self, target: Vec3 | tuple[float, float, float]) -> None: ...

class Sky:
    """Backdrop colours read as Vec3; writes accept Vec3, RGB tuples, named colours or hex strings."""

    enabled: bool
    zenith: Vec3 | str
    horizon: Vec3 | str
    noise: float
    sun: Vec3
    ambient: float
    def colors(self, zenith: str, horizon: str) -> None: ...

class Light:
    """The light the world is being shaded by, once the rung `atmosphere.lightQuality`
    names has been resolved. Every number here is the engine's own, so a script or an
    editor can show what `auto` picked without repeating the rule it picked by.
    Nothing on this handle is a knob."""

    # "low", "medium", "high" or "extra" - never "auto", because by the time anyone
    # asks, the machine has been read.
    @property
    def quality(self) -> str: ...

    # How the edge of a shade is laid, which is `carbine.lightType` and nothing else:
    # "straight" weaves the readings, "blocky" keeps them in steps.
    @property
    def style(self) -> str: ...

    # Whether the sun pre-renders the world to read a shade out of.
    @property
    def shadows(self) -> bool: ...

    # Texels on a side of that picture, or 0 when there is none.
    @property
    def shadowMap(self) -> int: ...

    # Readings a pixel takes around its own place in the cast. One is a hard edge;
    # more is a shade that fades where it ends.
    @property
    def softness(self) -> int: ...

    # Metres around the eyes the cast reaches. Past it a thing is lit and casts
    # nothing.
    @property
    def reach(self) -> float: ...

    # How much of the sun a face square to it throws back as a sheen, 0..1.
    @property
    def gloss(self) -> float: ...

    # Whether the light the sun leaves out comes from the backdrop itself.
    @property
    def skyFill(self) -> bool: ...

    # The machine the rung was read off: "discrete", "integrated", "cpu", "virtual"
    # or "other". None until a renderer has started.
    @property
    def device(self) -> str | None: ...

    # Bytes the card owns outright. None for a card that borrows the machine's
    # memory, and for one that will not say how big it is.
    @property
    def memory(self) -> int | None: ...

class Mouse:
    """The pointer as the last frame saw it. `dx` and `dy` are the pixels moved
    since the frame before, which is only useful while the pointer is grabbed."""

    @property
    def dx(self) -> float: ...
    @property
    def dy(self) -> float: ...
    @property
    def x(self) -> float: ...
    @property
    def y(self) -> float: ...
    @property
    def left(self) -> bool: ...
    @property
    def right(self) -> bool: ...
    @property
    def middle(self) -> bool: ...
    @property
    def wheel(self) -> float: ...
    @property
    def grabbed(self) -> bool: ...

class Player:
    """The controller `carbine.player()` returns. Turn it on and the engine drives
    the camera: WASD walks, the mouse looks, gravity pulls, and the camera rides
    the player. Meters and seconds throughout."""

    enabled: bool
    # "first" puts the camera in the eyes; "third" parks it behind the body.
    mode: str
    # The feet, in meters. z is height above the ground plane. A write arrives at
    # rest, so putting the player somewhere does not throw it.
    position: Vec3
    # Degrees. Yaw turns about the vertical; pitch never passes the vertical.
    yaw: float
    pitch: float
    # Meters per second at full tilt.
    speed: float
    # Degrees turned per pixel the mouse moves.
    sensitivity: float
    eyeHeight: float
    # Third person only: how far behind the body the camera sits, in meters.
    distance: float
    # Meters per second squared. 10 is Earth, 1.6 the Moon.
    gravityPull: float
    jumpStrength: float
    @property
    def grounded(self) -> bool: ...
    @property
    def velocity(self) -> Vec3: ...
    # The player, which the engine hides in first person and moves with every
    # step. A write takes the Object or the Entity to draw; a read answers with the
    # node as an Object. Whatever is worn, that node is what contact damage lands
    # on, so a player a script wants hurt wears an Entity here. When that actor
    # dies the engine takes it off, leaves it in the world and gives its visibility
    # back; the controller itself has no hit points, so it walks on until the
    # script says otherwise.
    @property
    def body(self) -> Object | None: ...
    @body.setter
    def body(self, value: Object | Entity | None) -> None: ...
    # Escape hands the pointer back to the desktop; a click asks for it again.
    grabMouse: bool
    def jump(self) -> None: ...
    def isKeyDown(self, name: str) -> bool: ...

class Scene:
    @property
    def root(self) -> Object: ...
    # The light the daylight is thrown by, which every world is born with. It is a
    # node like any other: colour it, turn it, brighten it, or switch it off with
    # `brightness = 0`. Turning it is aiming it, and `atmosphere.time(...)` turns it.
    @property
    def sun(self) -> Object | None: ...
    @property
    def nodeCount(self) -> int: ...
    @property
    def entityCount(self) -> int: ...

class Ui:
    """Pixel overlay; Text uses top-left position and scalar font size, shapes use size vectors."""

    # Presets: square, rectangle, triangle, hexagon, circle, Text (case-insensitive).
    def new(
        self,
        kind: str = "square",
        *,
        name: str | None = None,
        parent: Object | Entity | Particle | None = None,
    ) -> Object: ...

class Disaster:
    """One weather event: an acceleration the whole world is asked about, once a
    step. Not a node - it has no place in the tree and nothing to draw, so a
    destroyed one answers ValueError rather than DestroyedError."""

    id: int
    # "earthquake", "wind", "tornado" or "blast" - which rule the world is asked
    # by. It never changes after new(); write another one instead.
    kind: str
    # A label to tell two storms apart. Defaults to the kind.
    name: str
    # How hard it shoves, where every step up the scale doubles it. 5 is the
    # weather the world has every day, 7 is what the news reports, and past 10 the
    # engine stops listening.
    magnitude: float
    # Seconds of it. Zero - which a wind and a funnel start with - keeps going
    # until stop(); a blast comes ready-made with less than half a second.
    duration: float
    # Seconds of it the solver has already run. Reading only.
    elapsed: float
    # Where it is worst, in meters. Ignored while `node` is set.
    epicentre: Vec3
    # Meters before it stops being felt. Zero means the whole world.
    reach: float
    # Which way a wind blows. The other kinds find their own direction.
    direction: Vec3
    # The object this one rides: its place in the world becomes the centre, so a
    # funnel bolted to a fan follows the fan about. None puts the epicentre back.
    node: Object | Entity | Particle | None
    # Whether the solver is asking it how hard to shove right now.
    running: bool

    def start(self) -> None: ...
    def stop(self) -> None: ...
    def destroy(self) -> None: ...

    # How hard, and which way, this event shoves at one point in meters - zero
    # while it is switched off. A script may ask the weather a question.
    def accelerationAt(
        self, at: Vec3 | tuple[float, float, float]
    ) -> Vec3: ...

class Physics:
    """The weather register. Disasters are written here, in meters and seconds
    like the rest of the world, and they only ever shove bodies that asked for
    gravity and were not anchored."""

    def new(self, kind: str, *, name: str | None = None) -> Disaster: ...
    def kinds(self) -> list[str]: ...
    def running(self) -> list[Disaster]: ...
    # Switches everything off at once. What the weather already did stays done.
    def stopAll(self) -> None: ...

class Atmosphere:
    """The environment register, `carbine.atmosphere`. What time it is, how far the
    air lets you see, how much work the light is allowed to take and what the sky
    looks like all live here, and the weather that
    falls is written by the particle generator - `atmosphere.new("rain")` is the fast
    way to lay a crowd and hang it on the sky, which is why its handle is a
    `Particle` with every particle knob on it."""

    # The backdrop and the light the world is shaded by:
    # `atmosphere.sky().zenith = "#2f6cb3"`.
    def sky(self) -> Sky: ...

    # How much work the engine is allowed to do lighting a frame: "auto", "low",
    # "medium", "high" or "extra". A fresh world is on "auto", which is the engine
    # reading the card it draws on: a card owning more than 4 GB takes the high rung,
    # one from 0.1 GB up to 4 GB the middle, and anything smaller - or anything that
    # shares the machine's memory - the lowest.
    lightQuality: str
    # The rungs `lightQuality` takes, cheapest first.
    def lightQualities(self) -> list[str]: ...

    # What the light turned out to be on this machine. Read-only: the knob above is
    # the only way to move it.
    def light(self) -> Light: ...

    # Where the sun is, in hours past midnight. Write a clock the way a person reads
    # one ("07:30") or a count of hours (18); either way the answer is the hours and
    # reading the clock is `atmosphere.time()`. A time past midnight wraps round.
    def time(self, clock: str | float | None = None) -> float: ...

    # How far the air lets you see, 0 (a bright day) to 100 (not seeing your hand).
    # Fog is the air, not a crowd, so there is nothing to start or stop.
    fog: float
    # Fog colour accepts Vec3, RGB tuples, named colours or hex strings.
    fogColor: Vec3 | str

    # Writes one of the weathers the engine knows and hands back its handle, already
    # flying and hung on the sky, so its box rides with wherever the eyes are.
    def new(self, kind: str, *, name: str | None = None) -> Particle: ...
    def kinds(self) -> list[str]: ...
    # The weather systems falling over the view right now. A local crowd is not on
    # the sky, so it does not answer here.
    def running(self) -> list[Particle]: ...
    # Switches every sky system off at once. Each stays written and startable; this
    # clears the air, not the fog.
    def stopAll(self) -> None: ...

class Particle:
    """One crowd: a node with a system of drops flying in a box around it. It is a
    node and not a switch, so it hangs in the tree, moves, takes a colour, skins
    its drops and is destroyed like any object - and `parent` can be a chimney, in
    which case the embers fly from the top of the chimney.

    Every vector write also takes an `(x, y, z)` tuple, in which `...` or None
    keeps the axis as it already is."""

    id: int
    name: str
    # Always "particles", which is what tells this node from a body.
    kind: str
    parent: Object | Entity | Particle | None
    children: list[Object | Entity | Particle]
    def addChild(self, child: Object | Entity | Particle) -> None: ...
    def removeChild(self, child: Object | Entity | Particle) -> None: ...
    # Meters from the scene origin, and the middle of the box the crowd flies in.
    # Moving a system leaves what is already in the air where it was, so call
    # `restart()` to lay the whole veil at the new place.
    position: Vec3
    # Turning the node turns the fall with it: on its side it sprays sideways.
    rotation: Quat
    rotationEuler: Vec3
    # How the drops are cut: "streak", "square" or "circle". `procedural.shapes()`.
    shape: str
    # How many things the system keeps in the air. Growing it lays the new drops
    # and leaves the ones already flying alone; shrinking it takes the extras out.
    count: int
    # Meters across a drop. A streak is six of these long along its own way.
    size: float
    # Meters a second along the way the system points.
    speed: float
    # The way the crowd travels, in world meters. Zero means straight down.
    direction: Vec3
    # The box new drops are placed in, meters, centred on the node.
    area: Vec3
    # Seconds a drop lives for.
    lifetime: float
    # The dice the crowd was laid out with. Writing a new one throws the veil away
    # and lays the one that seed names. A word is as good as a number.
    seed: int | str
    # Whether the system is doing anything. Off leaves the node, its settings and
    # its place in the tree, and takes every drop out of the air.
    active: bool
    # Whether the drops are stopped by bodies. Off they fall straight through the
    # world; on they land on the floor and on anything else with a collider.
    collide: bool
    # How many droplets an impact throws back, 0 for none. Only means something
    # while the system collides.
    splash: int
    # Whether a spent drop is born again at the top of the box. Off, the system
    # runs out and the sky clears on its own.
    recycle: bool
    # Whether the crowd is hung on the sky. On, its box rides with the eyes rather
    # than with the node, which is what makes a weather veil.
    onSky: bool
    # Drop colour accepts Vec3, RGB tuples, named colours or hex strings.
    # Each drop keeps a random part of it, which stops a thousand drops reading as
    # one flat sheet.
    color: Vec3 | str
    @property
    def colorHex(self) -> str: ...
    # The packed `.ktx2` a drop is cut from, as a path. None paints them flat.
    texture: str | None
    # Opacity from 0 (invisible) to 255 (opaque), shared by every drop.
    transparent: float
    # Whether the node is drawn at all. Hidden, its crowd goes with it.
    visible: bool
    # How many of them are in the air right now.
    @property
    def inAir(self) -> int: ...
    # How many drops this system brought up short during the last step.
    @property
    def impacts(self) -> int: ...
    # Throws the crowd away and lays a new one at where the node stands now, which
    # is how a script moves a veil after moving, turning or re-hanging it.
    def restart(self) -> None: ...
    def destroy(self) -> None: ...

class Procedural:
    """The content generators, `carbine.procedural`. These make things rather than
    asking for them: the particle generator takes a shape, a colour or an image and
    turns it into a crowd flying through the world, and the texture generator paints
    a surface and writes it as the `.ktx2` an object is skinned with."""

    # Writes a system into the world and hands back its handle. Every knob is
    # optional; one left out keeps the value the engine gives a fresh system, so
    # `particles()` alone is a rain veil and
    # `particles(shape="circle", color="#fff8f0", speed=1.2)` is snow.
    def particles(
        self,
        *,
        name: str | None = None,
        parent: Object | Entity | Particle | None = None,
        shape: str | None = None,
        count: int | None = None,
        size: float | None = None,
        speed: float | None = None,
        direction: Vec3 | tuple[float, float, float] | None = None,
        area: Vec3 | tuple[float, float, float] | None = None,
        lifetime: float | None = None,
        seed: int | str | None = None,
        color: Vec3 | str | None = None,
        image: str | None = None,
        active: bool | None = None,
        collide: bool | None = None,
        splash: int | None = None,
        recycle: bool | None = None,
    ) -> Particle: ...
    # The shapes a `Particle.shape` accepts.
    def shapes(self) -> list[str]: ...

    # Paints a surface and writes it as a `.ktx2`, answering with the path it wrote,
    # which is the picture an object wants:
    # `ground.texture = carbine.procedural.texture("grass", variation="moss")`.
    # `surface` is what the thing is made of, from `textureTypes()`; `variation` is
    # what has gone wrong with it, from `textureVariations(surface)`; `quality` is one
    # word for both the size of the tile and how far its detail is summed, off
    # `textureQualities()`, and is `medium` left alone. The three names are read
    # forgivingly - `lawn`, `hi`, `small` all land somewhere - and a name the engine
    # does not recognise raises ValueError, which says what it does offer. The picture
    # comes out of `seed`, so the same word paints the same surface for anybody. An
    # `out` that names a folder writes into it and anything else is the exact file;
    # left alone the tile lands in the folder the game is run from, named for
    # everything asked for so a rerun overwrites its own picture instead of adding a
    # second one. A folder that cannot be made raises OSError.
    def texture(
        self,
        surface: str,
        *,
        variation: str | None = None,
        quality: str | None = None,
        seed: int | str | None = None,
        out: str | os.PathLike[str] | None = None,
    ) -> str: ...
    # The surfaces the generator paints.
    def textureTypes(self) -> list[str]: ...
    # The ways one surface can be ruined, which is what a `variation` may be named for
    # it. A surface that is not one of the four raises ValueError.
    def textureVariations(self, surface: str) -> list[str]: ...
    # The qualities, from the smallest tile to the largest.
    def textureQualities(self) -> list[str]: ...

class Packager:
    """The importer, `carbine.packager`. Carbine loads one format per class of
    thing — images are `.ktx2`, objects `.glb`, audio `.ogg` — so the engine carries
    one small loader each instead of forty decoders. This turns an authored file into
    the format the engine reads; Enginebay does the same work when a file is dropped
    into a project, and a script can do it in real time."""

    # Converts a picture to `.ktx2` — BC7 blocks with a full mip chain, the bytes a
    # GPU reads without decoding — and returns the path it wrote. Left alone the file
    # lands beside its source under the same stem, so `assets/wood.png` answers
    # `assets/wood.ktx2`; an `out` that names a folder packs into it, and anything
    # else is the exact file to write. Skin objects with the path it hands back:
    # `carbine.new("cube").texture = carbine.packager.image("assets/wood.png")`.
    # Reading the source takes png, jpg, webp, tga and bmp; anything else raises
    # ValueError, and a folder it cannot make raises OSError. A `pathlib.Path` is as
    # good as a string for either name.
    def image(
        self, path: str | os.PathLike[str], *, out: str | os.PathLike[str] | None = None
    ) -> str: ...

scene: Scene
ui: Ui
physics: Physics
atmosphere: Atmosphere
procedural: Procedural
packager: Packager

# The frame limits, both absent until a game asks for them. `fpsCap` is a ceiling the
# window is told not to pass — `None`, or `0`, for no ceiling — and `monitorSync` hands
# the pace to the screen the window stands on, off by default so nothing but a cap the
# game named holds the frames. The lower of the two is what runs, so one game holds 90
# on a 144 Hz desktop and 60 on the laptop at the café without anyone noticing the
# difference. `monitorHz` is what the screen asks for — zero on some desktops until the
# first `render()` puts a frame up, and then re-asked every second, so unplugging is
# noticed — and `fpsLimit` is the rate the two settle at, `None` when neither holds.
fpsCap: int | None
monitorSync: bool
monitorHz: int
fpsLimit: int | None
# Read-only measured FPS, refreshed every 0.25s; zero until sampled and after init/end.
fps: float

# How the edge of a shade is laid over the ground. `carbine.lightType = "blocky"`
# keeps the steps a single cast texel per pixel makes, which is a look of its own;
# "straight" — what the world starts with — weaves the readings of the cast together,
# turns the weave by where the pixel stands, lets the far readings count for less, and
# takes the cut off the border of the cast's picture. It costs the same either way: how
# much light is worked at all is `atmosphere.lightQuality`, and `lightTypes()` lists
# both looks.
lightType: str

def init(
    width: int = 1280,
    height: int = 720,
    *,
    window: bool = True,
    title: str = "Carbine",
) -> str: ...
def render() -> None: ...
def running() -> bool: ...
def viewport() -> tuple[int, int]: ...
def readPixels() -> bytes: ...
def close() -> None: ...

# Throws away what the script made. A handle takes that node and everything under it;
# "all" empties the scene of every node — objects, actors, UI, particle crowds, weather —
# and removes the disasters with it, which is the one call a teardown wants instead of
# one per object. The camera, the backdrop and the light are settings the game tuned, so
# `destroy("all")` leaves them alone. `end()` is `init()` in reverse: the scene comes back
# brand new, the player gives its camera and the pointer up, and the window closes. The
# frame limits are engine settings rather than scene content, so those stay as named.
def destroy(target: Object | Entity | Particle | str) -> None: ...
def end() -> None: ...

def new(
    kind: str,
    *,
    name: str | None = None,
    parent: Object | Entity | Particle | None = None,
) -> Object: ...

# More of the same thing. Each copy hangs beside the original, starts where it stood
# and stands still from there, and comes back as the class the node is - a body's
# copies are `Object`s, an actor's are `Entity`s, a crowd's are `Particle`s. Left
# alone `count` is 1, and 0 makes none. Whatever the node carries is carried: size,
# colour, skin, whether it takes gravity, what shape it is cut as, a UI element's
# corners, a crowd's knobs and its air, and the branch under it as copies of their
# own. Each copy then has its own place, turning, size and hit points.
def duplicate(
    target: Object | Entity | Particle,
    count: int = 1,
) -> list[Object | Entity | Particle]: ...

# Like `duplicate`, and the copies tied to the original in one configuration, the way
# Blender's linked duplicates share a data block. A write about what the thing *is*
# lands on every member - colour, `transparent`, `texture`, `textureTiling`, `gravity`,
# `gravityPull`, `mass`, `anchored`, `collision`, `collisionType`, an actor's
# `hitPause` and `stayAfterDeath`, a UI element's `corner`, `polygon` and `outline`, a
# vehicle's `type`, `topSpeed`, `turnRate` and `grip`, and every particle knob except
# `active`.
# A write about where a member is, or how much of it is left, stays with that member:
# `position`, `rotation`, `size`, `scale`, `health`,
# `damage`, `visible`, a crowd's `restart()` - so twins can stand at fifty different
# windows and die one at a time. A copy of a member joins the group it was copied
# from; `duplicate` of one leaves the group, and the original only picks up a group
# when it is first copied by link.
# Text content, font size, wrapping and outline are shared; position and scale stay local.
def linkDuplicate(
    target: Object | Entity | Particle,
    count: int = 1,
) -> list[Object | Entity | Particle]: ...

def spawnEntity(
    kind: str | None = None,
    *,
    name: str | None = None,
    parent: Object | Entity | Particle | None = None,
    position: Vec3 | tuple[float, float, float] | None = None,
    health: float = 1.0,
    damage: float = 0.0,
) -> Entity: ...

def killEntity(entity: Entity) -> None: ...
def camera() -> Camera: ...
def player() -> Player: ...
def mouse() -> Mouse: ...
# What `new` accepts: the geometry kinds, plus "light", which lights rather than fills,
# "vehicle", which is a body wearing a drive, and the two seats - "seat" to ride along
# in and "driverSeat", which drives the vehicle it hangs on. A seat reads whatever a
# script feels like calling it: "chair", "passenger" and "Driver's Seat" all work.
def kinds() -> list[str]: ...
# How the solver reads a body: "box", "sphere" and "mesh".
def collisionTypes() -> list[str]: ...
def lightTypes() -> list[str]: ...
# What a vehicle's `type` takes: "arcade" and "drift".
def handlingTypes() -> list[str]: ...
def dt() -> float: ...
def step(seconds: float) -> None: ...

# Keys are named by the position they sit in, so "w" means the key a French
# keyboard labels "z". `pressed` and `released` describe the last frame only.
def key(name: str) -> bool: ...
def pressed(name: str) -> bool: ...
def released(name: str) -> bool: ...
def keys() -> list[str]: ...

# Takes the pointer from the desktop, or hands it back. Reports what the window
# actually did, since a compositor can refuse. Only a player holds it.
def grabMouse(grab: bool) -> bool: ...

# For a test or a replay: input that did not come from a window.
def injectKey(name: str, down: bool) -> None: ...
def injectMouse(dx: float, dy: float) -> None: ...
def injectButton(name: str, down: bool) -> None: ...

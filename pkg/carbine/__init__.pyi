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
    """A body in the world. Reads give the values back as `Vec3`; every vector
    write below also takes an `(x, y, z)` tuple, in which `...` or None keeps the
    axis as it already is. The world is z-up: `size` is (length, width, height)."""

    id: int
    name: str
    kind: str
    parent: Object | None
    @property
    def children(self) -> list[Object]: ...
    position: Vec3
    size: Vec3
    scale: Vec3
    # Read as a Quat, written as a Quat or as euler degrees (x, y, z).
    rotation: Quat
    rotationEuler: Vec3
    color: Vec3 | str
    @property
    def colorHex(self) -> str: ...
    # The image painted over the object's colour, as a path. None paints it flat.
    texture: str | None
    # False leaves the object, and everything pinned under it, out of the frame.
    visible: bool
    # How the object takes part in `step`. Gravity is off by default.
    anchored: bool
    gravity: bool
    # Metres per second squared: 10 is Earth, 1.6 the Moon, 0 lets it hang.
    gravityPull: float
    # UI overlay knobs; a world object raises on them. Sizes are pixels.
    corner: float
    polygon: int
    outline: float
    transparent: float
    @property
    def mesh(self) -> Mesh: ...
    def destroy(self) -> None: ...

class Entity:
    id: int
    name: str
    kind: str
    @property
    def object(self) -> Object | None: ...
    health: float
    damage: float
    position: Vec3
    size: Vec3
    rotation: Quat
    rotationEuler: Vec3
    color: Vec3
    texture: str | None
    visible: bool
    anchored: bool
    gravity: bool
    gravityPull: float
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
    """The backdrop behind the world and the one light every object is shaded by.
    Colours read back as `Vec3` and are written as a `Vec3`, a tuple, or hex."""

    enabled: bool
    zenith: Vec3 | str
    horizon: Vec3 | str
    noise: float
    sun: Vec3
    ambient: float
    def colors(self, zenith: str, horizon: str) -> None: ...

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
    # The object drawn as the player, which the engine hides in first person.
    body: Object | None
    # Escape hands the pointer back to the desktop; a click asks for it again.
    grabMouse: bool
    def jump(self) -> None: ...
    def isKeyDown(self, name: str) -> bool: ...

class Scene:
    @property
    def root(self) -> Object: ...
    @property
    def nodeCount(self) -> int: ...
    @property
    def entityCount(self) -> int: ...

class Ui:
    """Screen overlay elements. Their position and size are pixels measured from
    the viewport's top left corner, not meters."""

    def new(
        self,
        kind: str = "square",
        *,
        name: str | None = None,
        parent: Object | Entity | None = None,
    ) -> Object: ...

scene: Scene
ui: Ui

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

def new(
    kind: str,
    *,
    name: str | None = None,
    parent: Object | Entity | None = None,
) -> Object: ...

def spawnEntity(
    kind: str | None = None,
    *,
    name: str | None = None,
    parent: Object | Entity | None = None,
    position: Vec3 | tuple[float, float, float] | None = None,
    health: float = 1.0,
    damage: float = 0.0,
) -> Entity: ...

def killEntity(entity: Entity) -> None: ...
def camera() -> Camera: ...
def sky() -> Sky: ...
def player() -> Player: ...
def mouse() -> Mouse: ...
def kinds() -> list[str]: ...
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

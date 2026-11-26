"""`swbtools pw-private`: a private PipeWire daemon for swb's audio measurements
(M14 feature 8, docs/testing.md "Audio output").

The owner's PipeWire daemon drives a sound card. A run of swb that plays sound
must never reach it, so every measurement with the PipeWire output connects to
a daemon that this tool starts:

- its own runtime directory (`tmp/pw-private/`): `PIPEWIRE_RUNTIME_DIR` and
  `XDG_RUNTIME_DIR` point there for the daemon and for every client, so a
  client finds the socket `pipewire-0` of the private daemon only;
- its own configuration file with a name that no system or user drop-in
  directory has (`swb-private.conf`), and `HOME` and `XDG_CONFIG_HOME` inside
  the runtime directory for the daemon, so no file of the owner applies;
- the modules that a client stream and links need, the Dummy-Driver, and one
  null sink (`support.null-audio-sink`, stereo, 48 kHz) with a monitor;
- no session manager (no WirePlumber, no `pipewire-pulse`), no ALSA, Bluetooth
  or V4L2 device: such a daemon cannot open a sound card.

Without a session manager nothing links by itself: `watch` links swb's output
ports to the null sink (and the sink's monitor to the recorder), and `record`
records the monitor to a WAV file, so that the output can be checked by
numbers.

`start` proves the isolation before it starts the daemon, and again after:
(a) with the daemon not running, `pw-cli info 0` under the private environment
fails (the client does not fall back to the owner's daemon); (b) `pw-dump` of
the private daemon shows no node or device with an `api.alsa`, `api.bluez5` or
`api.v4l2` property, and exactly one `Audio/Sink` (the null sink). If (a) does
not fail, `start` stops and starts nothing. Nothing here uses the default
runtime directory.
"""

import json
import logging
import os
import signal
import subprocess
import time
from pathlib import Path
from typing import Any

from swbtools import paths

log = logging.getLogger(__name__)

CONFIG_NAME = "swb-private.conf"
CLIENT_CONFIG_NAME = "swb-private-client.conf"
SINK_NAME = "swb-null-sink"
RECORDER_NAME = "swb-recorder"
# The socket name inside the private runtime directory: the clients' default,
# so that the two environment variables are all that a client needs.
SOCKET_NAME = "pipewire-0"
STATE_NAME = "daemon.json"
# Property names that show a hardware device (or a node of one).
FORBIDDEN_PREFIXES = ("api.alsa", "api.bluez5", "api.v4l2")
# Values of `device.api` that show one.
FORBIDDEN_APIS = ("alsa", "bluez5", "v4l2")

_CONFIG = """\
# A private PipeWire daemon for swb's measurements (tools/swbtools/pw_private.py).
# No session manager, no device: a null sink only.
context.properties = {
    core.daemon = true
    core.name   = %(socket)s
    link.max-buffers = 16
    default.clock.rate = 48000
    default.clock.allowed-rates = [ 48000 ]
    default.clock.quantum = 512
    default.clock.min-quantum = 512
    default.clock.max-quantum = 512
    log.level = 2
}

context.spa-libs = {
    audio.convert.* = audioconvert/libspa-audioconvert
    audio.adapt     = audioconvert/libspa-audioconvert
    support.*       = support/libspa-support
}

context.modules = [
    { name = libpipewire-module-protocol-native }
    { name = libpipewire-module-access }
    { name = libpipewire-module-metadata }
    { name = libpipewire-module-client-node }
    { name = libpipewire-module-spa-node-factory }
    { name = libpipewire-module-adapter }
    { name = libpipewire-module-link-factory }
]

context.objects = [
    { factory = spa-node-factory
        args = {
            factory.name    = support.node.driver
            node.name       = Dummy-Driver
            node.group      = pipewire.dummy
            priority.driver = 20000
        }
    }
    { factory = adapter
        args = {
            factory.name     = support.null-audio-sink
            node.name        = %(sink)s
            node.description = "swb private null sink"
            media.class      = Audio/Sink
            audio.channels   = 2
            audio.position   = "FL,FR"
            audio.rate       = 48000
            object.linger    = true
            node.driver      = true
            monitor.channel-volumes = false
            node.param.PortConfig = {
                direction = Input
                mode = dsp
                monitor = true
                format = {
                    mediaType = audio
                    mediaSubtype = raw
                    format = F32
                    rate = 48000
                    channels = 2
                    position = [ FL FR ]
                }
            }
        }
    }
]
"""


class IsolationError(RuntimeError):
    """The private daemon is not isolated from the owner's: nothing starts."""


def config_text() -> str:
    """The configuration file of the private daemon."""
    return _CONFIG % {"socket": SOCKET_NAME, "sink": SINK_NAME}


# The configuration of the clients. Without a session manager nothing gives a
# stream its ports; `adapter.auto-port-config` (the setting that the system's
# daemon configuration gives its own streams) makes a client's stream make
# them (`output_FL`, `output_FR`), so that `watch` can link them. The product
# does not set it: with a session manager the manager configures the ports.
_CLIENT_CONFIG = """\
# The clients of the private PipeWire daemon (tools/swbtools/pw_private.py).
context.properties = {
    log.level = 0
}
context.spa-libs = {
    audio.convert.* = audioconvert/libspa-audioconvert
    audio.adapt     = audioconvert/libspa-audioconvert
    support.*       = support/libspa-support
}
context.modules = [
    { name = libpipewire-module-protocol-native }
    { name = libpipewire-module-client-node }
    { name = libpipewire-module-adapter }
    { name = libpipewire-module-metadata }
]
stream.properties = {
    adapter.auto-port-config = {
        mode = dsp
        monitor = false
        control = false
        position = preserve
    }
}
"""


def client_config_text() -> str:
    """The configuration file of the clients (swb, `pw-record`)."""
    return _CLIENT_CONFIG


def default_dir() -> Path:
    """The private runtime directory: `tmp/pw-private/` of the checkout."""
    return paths.repo_root() / "tmp" / "pw-private"


def check_dir(path: Path) -> Path:
    """The resolved runtime directory `path`, or an error if it could be the
    owner's: it must lie inside the checkout and must not be the owner's
    default runtime directory."""
    resolved = path.resolve()
    root = paths.repo_root().resolve()
    if root not in resolved.parents:
        raise IsolationError(f"{resolved} is not inside the checkout {root}")
    for owner in (Path(f"/run/user/{os.getuid()}"), Path(os.environ.get("XDG_RUNTIME_DIR", "/"))):
        if resolved == owner.resolve():
            raise IsolationError(f"{resolved} is the owner's runtime directory")
    return resolved


def client_env(directory: Path) -> dict[str, str]:
    """The environment of a client of the private daemon (swb, `pw-link`,
    `pw-record`): both variables point to the private runtime directory, and
    nothing that names another server survives."""
    env = dict(os.environ)
    for name in ("PIPEWIRE_REMOTE", "PIPEWIRE_CORE", "PIPEWIRE_CONFIG_DIR", "PIPEWIRE_CONFIG_NAME"):
        env.pop(name, None)
    env["PIPEWIRE_RUNTIME_DIR"] = str(directory)
    env["XDG_RUNTIME_DIR"] = str(directory)
    env["PIPEWIRE_CONFIG_NAME"] = str(directory / CLIENT_CONFIG_NAME)
    return env


def daemon_env(directory: Path) -> dict[str, str]:
    """The environment of the daemon: the client's, with `HOME` and the
    configuration directories inside the runtime directory (no file of the
    owner applies) and no D-Bus session. It gets its configuration file from
    `-c`."""
    env = client_env(directory)
    env.pop("PIPEWIRE_CONFIG_NAME", None)
    home = directory / "home"
    env["HOME"] = str(home)
    env["XDG_CONFIG_HOME"] = str(home / ".config")
    env["XDG_CONFIG_DIRS"] = str(home / ".config-dirs")
    env["XDG_DATA_HOME"] = str(home / ".local" / "share")
    env.pop("DBUS_SESSION_BUS_ADDRESS", None)
    return env


def env_lines(directory: Path) -> list[str]:
    """`NAME=VALUE` lines for a client's environment (`env -u ... NAME=VALUE swb`)."""
    return [
        f"PIPEWIRE_RUNTIME_DIR={directory}",
        f"XDG_RUNTIME_DIR={directory}",
        f"PIPEWIRE_CONFIG_NAME={directory / CLIENT_CONFIG_NAME}",
    ]


def check_dump(objects: list[dict[str, Any]]) -> list[str]:
    """The problems that `pw-dump` shows about isolation: a node or device with
    an `api.alsa`, `api.bluez5` or `api.v4l2` property, a device object at all,
    and a number of `Audio/Sink` nodes other than one. Empty when the daemon
    is as isolated as the proof (b) demands."""
    problems: list[str] = []
    sinks = 0
    for obj in objects:
        kind = str(obj.get("type", ""))
        props = (obj.get("info") or {}).get("props") or {}
        for key, value in props.items():
            if str(key).startswith(FORBIDDEN_PREFIXES):
                problems.append(f"{kind} {obj.get('id')} has the property {key}")
            if str(value).startswith(FORBIDDEN_PREFIXES) or (
                key in ("device.api", "api.name") and str(value) in FORBIDDEN_APIS
            ):
                problems.append(f"{kind} {obj.get('id')} has {key} = {value}")
        if kind.endswith(":Device"):
            problems.append(f"{kind} {obj.get('id')}: the daemon has a device object")
        if kind.endswith(":Node") and props.get("media.class") == "Audio/Sink":
            sinks += 1
    if sinks != 1:
        problems.append(f"{sinks} Audio/Sink nodes (exactly 1 expected)")
    return problems


def _run(
    command: list[str], directory: Path, timeout: float = 5.0
) -> subprocess.CompletedProcess[str]:
    """Runs a PipeWire tool as a client of the private daemon."""
    return subprocess.run(
        command,
        env=client_env(directory),
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )


def prove_no_fallback(directory: Path) -> str:
    """Proof (a): with the private daemon not running, `pw-cli info 0` under
    the private environment fails. Returns the tool's message. Raises
    `IsolationError` when it does not fail (the client found some server:
    never run swb with the output then)."""
    result = _run(["pw-cli", "info", "0"], directory)
    message = (result.stderr or result.stdout).strip().splitlines()
    if result.returncode == 0:
        raise IsolationError(
            "pw-cli info 0 works with the private runtime directory and no private daemon: "
            "the client reaches another daemon. Stop; run nothing with the PipeWire output."
        )
    return message[-1] if message else f"exit {result.returncode}"


def read_state(directory: Path) -> dict[str, Any] | None:
    """The state file of a started daemon, if there is one."""
    try:
        return json.loads((directory / STATE_NAME).read_text())
    except (OSError, ValueError):
        return None


def _ours(pid: int, directory: Path) -> bool:
    """Whether `pid` is the private daemon of `directory` (its command line
    names our configuration file)."""
    try:
        command = (Path("/proc") / str(pid) / "cmdline").read_bytes().split(b"\0")
    except OSError:
        return False
    return str(directory / CONFIG_NAME).encode() in command


def is_running(directory: Path) -> bool:
    """Whether the private daemon of `directory` runs."""
    state = read_state(directory)
    return state is not None and _ours(int(state["pid"]), directory)


def _wait_ready(directory: Path, process: subprocess.Popen[bytes], seconds: float) -> None:
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"the daemon exited with {process.returncode}; see pipewire.log")
        if (directory / SOCKET_NAME).exists() and _run(
            ["pw-cli", "info", "0"], directory
        ).returncode == 0:
            return
        time.sleep(0.05)
    raise RuntimeError("the daemon did not answer in time; see pipewire.log")


def start(directory: Path) -> dict[str, Any]:
    """Proves (a), starts the private daemon, proves (b). Returns the report:
    the pid, the proof messages and the node names. On a failed proof it stops
    the daemon that it started and raises `IsolationError`."""
    directory = check_dir(directory)
    if is_running(directory):
        raise RuntimeError(f"the private daemon already runs (see {directory / STATE_NAME})")
    directory.mkdir(parents=True, exist_ok=True)
    directory.chmod(0o700)
    (directory / "home").mkdir(exist_ok=True)
    (directory / CLIENT_CONFIG_NAME).write_text(client_config_text())
    stale = directory / SOCKET_NAME
    for leftover in (stale, directory / f"{SOCKET_NAME}.lock"):
        leftover.unlink(missing_ok=True)
    proof_a = prove_no_fallback(directory)
    config = directory / CONFIG_NAME
    config.write_text(config_text())
    log_file = (directory / "pipewire.log").open("wb")
    process = subprocess.Popen(
        ["pipewire", "-c", str(config)],
        env=daemon_env(directory),
        stdout=log_file,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    try:
        _wait_ready(directory, process, 10.0)
        dump = json.loads(_run(["pw-dump"], directory, 10.0).stdout)
        problems = check_dump(dump)
        if problems:
            raise IsolationError("; ".join(problems))
    except BaseException:
        _terminate(process.pid)
        raise
    nodes = [
        (o.get("info") or {}).get("props", {}).get("node.name")
        for o in dump
        if str(o.get("type", "")).endswith(":Node")
    ]
    state = {"pid": process.pid, "dir": str(directory), "config": str(config)}
    (directory / STATE_NAME).write_text(json.dumps(state))
    return {
        "pid": process.pid,
        "dir": str(directory),
        "proof_a": proof_a,
        "proof_b": f"{len(dump)} objects, no alsa/bluez5/v4l2 property, 1 Audio/Sink",
        "nodes": nodes,
    }


def _terminate(pid: int) -> None:
    """Stops the process `pid` (SIGTERM, then SIGKILL after 3 s)."""
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    deadline = time.monotonic() + 3.0
    while time.monotonic() < deadline:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return
        # A child that we started is a zombie until it is waited for.
        try:
            if os.waitpid(pid, os.WNOHANG)[0] == pid:
                return
        except ChildProcessError:
            pass
        time.sleep(0.02)
    os.kill(pid, signal.SIGKILL)


def stop(directory: Path) -> bool:
    """Stops the private daemon of `directory` by its pid, after it checked
    that the process is ours. Returns whether a daemon was stopped."""
    directory = check_dir(directory)
    state = read_state(directory)
    if state is None or not _ours(int(state["pid"]), directory):
        (directory / STATE_NAME).unlink(missing_ok=True)
        return False
    _terminate(int(state["pid"]))
    (directory / STATE_NAME).unlink(missing_ok=True)
    return True


def _port_list(directory: Path, flag: str) -> list[str]:
    result = _run(["pw-link", flag], directory, 3.0)
    return [line.strip() for line in result.stdout.splitlines() if line.strip()]


def _links(directory: Path) -> set[tuple[str, str]]:
    """The links of the private daemon as (output port, input port)."""
    result = _run(["pw-link", "-l"], directory, 3.0)
    links: set[tuple[str, str]] = set()
    current = ""
    for line in result.stdout.splitlines():
        if not line.startswith((" ", "\t")):
            current = line.strip()
        elif "|->" in line:
            links.add((current, line.split("|->", 1)[1].strip()))
        elif "|<-" in line:
            links.add((line.split("|<-", 1)[1].strip(), current))
    return links


def wanted_links(outputs: list[str], inputs: list[str]) -> list[tuple[str, str]]:
    """The links that make swb's stream reach the null sink and the sink's
    monitor reach the recorder, for the ports that exist now."""
    wanted = []
    for channel in ("FL", "FR"):
        pairs = [
            (f"swb:output_{channel}", f"{SINK_NAME}:playback_{channel}"),
            (f"{SINK_NAME}:monitor_{channel}", f"{RECORDER_NAME}:input_{channel}"),
        ]
        wanted.extend((out, inp) for out, inp in pairs if out in outputs and inp in inputs)
    return wanted


def link_once(directory: Path) -> list[tuple[str, str]]:
    """Makes the links that are possible now and are not there. Returns the
    links that it made."""
    outputs = _port_list(directory, "-o")
    inputs = _port_list(directory, "-i")
    existing = _links(directory)
    made = []
    for out, inp in wanted_links(outputs, inputs):
        if (out, inp) in existing:
            continue
        result = _run(["pw-link", out, inp], directory, 3.0)
        if result.returncode == 0:
            made.append((out, inp))
    return made


def watch(directory: Path, seconds: float, interval: float = 0.02) -> int:
    """Links for `seconds` (0: until a SIGTERM): at each `interval` the links
    that are possible and missing (swb's stream within the 2 s rule, the
    recorder). Returns the number of links made."""
    directory = check_dir(directory)
    stopped = False

    def on_term(_signum: int, _frame: object) -> None:
        nonlocal stopped
        stopped = True

    signal.signal(signal.SIGTERM, on_term)
    deadline = time.monotonic() + seconds if seconds > 0 else None
    total = 0
    while not stopped and (deadline is None or time.monotonic() < deadline):
        made = link_once(directory)
        for out, inp in made:
            log.info("linked %s -> %s", out, inp)
        total += len(made)
        time.sleep(interval)
    return total


def record(directory: Path, out: Path, seconds: float) -> int:
    """Records the null sink's monitor to the WAV file `out` (32-bit float,
    stereo, 48 kHz) for `seconds`; the links come from `watch`. Returns the
    exit code of `pw-record`."""
    directory = check_dir(directory)
    out.parent.mkdir(parents=True, exist_ok=True)
    process = subprocess.Popen(
        [
            "pw-record",
            "--rate",
            "48000",
            "--channels",
            "2",
            "--format",
            "f32",
            "-P",
            f"{{ node.name = {RECORDER_NAME} node.autoconnect = false }}",
            str(out),
        ],
        env=client_env(directory),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        time.sleep(seconds)
    finally:
        # SIGINT makes pw-record finish the WAV header.
        process.send_signal(signal.SIGINT)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
    return process.returncode or 0


def dump(directory: Path) -> list[dict[str, Any]]:
    """`pw-dump` of the private daemon."""
    directory = check_dir(directory)
    return json.loads(_run(["pw-dump"], directory, 10.0).stdout)


def status(directory: Path) -> dict[str, Any]:
    """What runs: the daemon, its nodes, the isolation problems."""
    directory = check_dir(directory)
    if not is_running(directory):
        return {"running": False, "dir": str(directory)}
    objects = dump(directory)
    return {
        "running": True,
        "dir": str(directory),
        "pid": (read_state(directory) or {}).get("pid"),
        "problems": check_dump(objects),
        "nodes": [
            (o.get("info") or {}).get("props", {}).get("node.name")
            for o in objects
            if str(o.get("type", "")).endswith(":Node")
        ],
        "links": sorted(f"{a} -> {b}" for a, b in _links(directory)),
    }

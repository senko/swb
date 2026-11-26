"""Tests of `swbtools pw-private` that need no PipeWire: the configuration, the
environments, the isolation checks and the parsing. The daemon itself is not
started here (the owner's machine has its own; see docs/testing.md "Audio
output" for the run by hand)."""

import subprocess
from pathlib import Path

import pytest

from swbtools import paths, pw_private


def test_the_configuration_has_a_null_sink_and_no_device_or_session_manager() -> None:
    text = pw_private.config_text()
    assert "support.null-audio-sink" in text
    assert "Audio/Sink" in text
    for forbidden in ("alsa", "bluez", "v4l2", "wireplumber", "protocol-pulse", "udev", "jack"):
        assert forbidden not in text.lower(), forbidden
    # Only the modules that a client stream and links need.
    modules = [line for line in text.splitlines() if "libpipewire-module-" in line]
    assert [m.split("libpipewire-module-")[1].split()[0] for m in modules] == [
        "protocol-native",
        "access",
        "metadata",
        "client-node",
        "spa-node-factory",
        "adapter",
        "link-factory",
    ]
    assert "Dummy-Driver" in text
    # The socket of the private directory is the client's default name.
    assert f"core.name   = {pw_private.SOCKET_NAME}" in text


def _node(node_id: int, **props: str) -> dict:
    return {"id": node_id, "type": "PipeWire:Interface:Node", "info": {"props": props}}


def test_the_dump_check_accepts_one_null_sink() -> None:
    dump = [
        {"id": 0, "type": "PipeWire:Interface:Core", "info": {"props": {}}},
        _node(1, **{"node.name": "Dummy-Driver"}),
        _node(2, **{"node.name": "swb-null-sink", "media.class": "Audio/Sink"}),
    ]
    assert pw_private.check_dump(dump) == []


def test_the_dump_check_finds_devices_and_wrong_sink_counts() -> None:
    sink = _node(2, **{"media.class": "Audio/Sink"})
    alsa = _node(3, **{"media.class": "Audio/Sink", "api.alsa.path": "hw:0"})
    problems = pw_private.check_dump([sink, alsa])
    assert any("api.alsa.path" in p for p in problems)
    assert any("2 Audio/Sink" in p for p in problems)
    assert any("0 Audio/Sink" in p for p in pw_private.check_dump([]))
    bluez = _node(4, **{"device.api": "bluez5"})
    assert any("bluez5" in p for p in pw_private.check_dump([sink, bluez]))
    device = {"id": 5, "type": "PipeWire:Interface:Device", "info": {"props": {}}}
    assert any("device object" in p for p in pw_private.check_dump([sink, device]))
    v4l2 = _node(6, **{"api.v4l2.path": "/dev/video0"})
    assert any("api.v4l2.path" in p for p in pw_private.check_dump([sink, v4l2]))


def test_a_client_environment_names_only_the_private_directory(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("PIPEWIRE_REMOTE", "somewhere")
    monkeypatch.setenv("PIPEWIRE_CONFIG_DIR", "/etc/pipewire")
    monkeypatch.setenv("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus")
    env = pw_private.client_env(tmp_path)
    assert env["PIPEWIRE_RUNTIME_DIR"] == env["XDG_RUNTIME_DIR"] == str(tmp_path)
    assert "PIPEWIRE_REMOTE" not in env and "PIPEWIRE_CONFIG_DIR" not in env
    # The clients' configuration is the private one (it makes their streams
    # make ports); the daemon's comes from `-c`.
    assert env["PIPEWIRE_CONFIG_NAME"] == str(tmp_path / pw_private.CLIENT_CONFIG_NAME)
    assert "adapter.auto-port-config" in pw_private.client_config_text()
    assert "PIPEWIRE_CONFIG_NAME" not in pw_private.daemon_env(tmp_path)
    daemon = pw_private.daemon_env(tmp_path)
    assert daemon["PIPEWIRE_RUNTIME_DIR"] == str(tmp_path)
    assert daemon["HOME"] == str(tmp_path / "home")
    assert daemon["XDG_CONFIG_HOME"].startswith(str(tmp_path))
    assert "DBUS_SESSION_BUS_ADDRESS" not in daemon
    lines = pw_private.env_lines(tmp_path)
    assert lines == [
        f"PIPEWIRE_RUNTIME_DIR={tmp_path}",
        f"XDG_RUNTIME_DIR={tmp_path}",
        f"PIPEWIRE_CONFIG_NAME={tmp_path / pw_private.CLIENT_CONFIG_NAME}",
    ]


def test_the_directory_must_be_in_the_checkout_and_not_the_owners(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    assert pw_private.check_dir(pw_private.default_dir()) == pw_private.default_dir().resolve()
    with pytest.raises(pw_private.IsolationError):
        pw_private.check_dir(Path("/run/user/1000"))
    with pytest.raises(pw_private.IsolationError):
        pw_private.check_dir(Path("/tmp/elsewhere"))
    # The checkout itself is not a directory inside it.
    with pytest.raises(pw_private.IsolationError):
        pw_private.check_dir(paths.repo_root())
    monkeypatch.setenv("XDG_RUNTIME_DIR", str(paths.repo_root() / "tmp" / "owner"))
    with pytest.raises(pw_private.IsolationError):
        pw_private.check_dir(paths.repo_root() / "tmp" / "owner")


def _fake_run(returncode: int, stdout: str = "", stderr: str = "") -> object:
    def run(*_args: object, **_kwargs: object) -> subprocess.CompletedProcess[str]:
        return subprocess.CompletedProcess([], returncode, stdout, stderr)

    return run


def test_proof_a_fails_when_a_client_finds_a_server(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(pw_private, "_run", _fake_run(0, "id: 0\n"))
    with pytest.raises(pw_private.IsolationError, match="reaches another daemon"):
        pw_private.prove_no_fallback(tmp_path)
    monkeypatch.setattr(
        pw_private, "_run", _fake_run(1, stderr="pw_context_connect() failed: Host is down\n")
    )
    assert "Host is down" in pw_private.prove_no_fallback(tmp_path)


def test_the_links_that_are_wanted_follow_the_ports_that_exist() -> None:
    outputs = [
        "swb:output_FL",
        "swb:output_FR",
        "swb-null-sink:monitor_FL",
        "swb-null-sink:monitor_FR",
    ]
    inputs = ["swb-null-sink:playback_FL", "swb-null-sink:playback_FR"]
    assert pw_private.wanted_links(outputs, inputs) == [
        ("swb:output_FL", "swb-null-sink:playback_FL"),
        ("swb:output_FR", "swb-null-sink:playback_FR"),
    ]
    inputs += ["swb-recorder:input_FL", "swb-recorder:input_FR"]
    wanted = pw_private.wanted_links(outputs, inputs)
    assert ("swb-null-sink:monitor_FR", "swb-recorder:input_FR") in wanted
    assert len(wanted) == 4
    assert pw_private.wanted_links([], inputs) == []


def test_the_link_list_is_parsed(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    text = (
        "swb:output_FL\n"
        "  |-> swb-null-sink:playback_FL\n"
        "swb-null-sink:monitor_FR\n"
        "  |-> swb-recorder:input_FR\n"
        "swb-null-sink:playback_FR\n"
        "  |<- swb:output_FR\n"
    )
    monkeypatch.setattr(pw_private, "_run", _fake_run(0, text))
    assert pw_private._links(tmp_path) == {
        ("swb:output_FL", "swb-null-sink:playback_FL"),
        ("swb-null-sink:monitor_FR", "swb-recorder:input_FR"),
        ("swb:output_FR", "swb-null-sink:playback_FR"),
    }


def test_only_our_own_process_is_stopped(tmp_path: Path) -> None:
    # A pid that is alive but whose command line does not name our
    # configuration file is not ours: `stop` never signals it.
    import os

    assert not pw_private._ours(os.getpid(), tmp_path)
    assert not pw_private._ours(2**30, tmp_path)
    assert not pw_private.is_running(tmp_path)

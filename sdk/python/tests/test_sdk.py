import pytest

from ambar_sdk import AmbarError, Client, NotRunning


def manual(root, wid):
    return f'''
id = "{wid}"
name = "{wid}"
[trigger]
type = "manual"
[[actions]]
type = "write_file"
path = "{root}/out/{wid}.txt"
content = "ran"
append = true
[permissions]
write = ["{root}/out"]
'''


def test_owner_workflow_lifecycle(ambard):
    data, root = ambard
    with Client(data) as c:
        assert c.welcome["protocol"] == 1
        assert c.health()["status"] == "ok"
        c.register(manual(root, "hello"))
        c.enable("hello")
        run = c.run_now("hello")
        assert run["status"] == "succeeded"
        assert (root / "out" / "hello.txt").read_text() == "ran\n"
        wf = c.list_workflows()[0]
        assert (wf["id"], wf["state"], wf["granted"]) == ("hello", "active", True)
        c.pause("hello")
        c.resume("hello")
        c.disable("hello")
        assert c.history("hello")[0]["status"] == "succeeded"
        assert c.whoami()["actor"] == "owner"


def test_agent_scopes_and_proposals(ambard):
    data, root = ambard
    with Client(data) as owner:
        agent, token = owner.agent_create("py-agent", ["read", "propose", "memory"])
        assert token.startswith("ambar_agent_")
        with Client(data, token=token) as a:
            assert a.whoami()["name"] == "py-agent"
            a.memory_put("seen", "1")
            assert a.memory_get("seen") == "1"
            assert a.memory_list() == ["seen"]
            p = a.propose(manual(root, "tidy"), reason="tidy notes")
            assert p["status"] == "pending"
            with pytest.raises(AmbarError) as e:
                a.register(manual(root, "sneaky"))
            assert e.value.code == "forbidden"
            with pytest.raises(AmbarError) as e:
                a.shutdown()
            assert e.value.code == "forbidden"
        owner.approve(p["id"], enable=True)
        assert owner.list_workflows()[0]["state"] == "active"
        owner.agent_revoke("py-agent")
        with pytest.raises(AmbarError) as e:
            Client(data, token=token)
        assert e.value.code == "unauthenticated"


def test_not_running(tmp_path):
    with pytest.raises(NotRunning):
        Client(tmp_path / "nothing")


def test_bad_token(ambard):
    data, _ = ambard
    with pytest.raises(AmbarError) as e:
        Client(data, token="nope")
    assert e.value.code == "unauthenticated"

"""End-to-end checks against a temporary workspace.

Each check builds a small project, drives the server over stdio, and asserts on the response. Run
with the path to a built server:

    python3 check_features.py ../../../target/debug/sqformat-lsp
"""

import json
import os
import shutil
import sys
import tempfile

from lsp_client import LspClient, decode_tokens, labels

COMMENT_TOKEN_TYPE = 13


def write(root, name, text):
    path = os.path.join(root, name)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as handle:
        handle.write(text)
    return path


def check_branch_slot_join(server):
    """A slot every branch inserts is available afterwards; a one-sided insertion is not."""
    root = tempfile.mkdtemp()
    source = """class A { void function OnlyA() {} }
void function Example(bool condition) {
\tlocal holder = {}
\tif ( condition ) holder.both <- A(); else holder.both <- A()
\tif ( condition ) holder.oneSide <- A()
\tholder.
}
"""
    path = write(root, "slots.gnut", source)
    client = LspClient(server, root)
    client.open(path, source)
    offered = labels(client.completion(path, source, source.rindex("holder.") + len("holder.")))
    client.shutdown()
    shutil.rmtree(root)
    assert offered == ["both"], offered


def check_callable_signatures(server):
    """Reassigned function values and declared functionref returns both give signature help."""
    prelude = """functionref(int damage) function MakeCallback() {
\treturn void function(int damage) {}
}
"""
    cases = [
        (
            prelude
            + "void function Example() {\n\tlocal cb = function(int first) {}\n\tcb = function(float second) {}\n\tcb(\n}\n",
            "\tcb(",
            ["function(float second)"],
        ),
        (
            prelude + "void function Example() {\n\tlocal returned = MakeCallback()\n\treturned(\n}\n",
            "\treturned(",
            ["function(int damage)"],
        ),
    ]
    root = tempfile.mkdtemp()
    path = os.path.join(root, "callables.gnut")
    client = LspClient(server, root)
    for index, (text, needle, expected) in enumerate(cases):
        client.open(path, text, version=index + 1)
        found = client.signature_help(path, text, text.index(needle) + len(needle))
        client.close(path)
        assert found == expected, (needle, found)
    client.shutdown()
    shutil.rmtree(root)


def check_post_initializer(server):
    """A call post-initializer contributes slots beside the called type's own members."""
    root = tempfile.mkdtemp()
    source = """global class Weapon {
\tvoid function Fire() {}
}
void function Example() {
\tlocal weapon = Weapon() {
\t\tammo = 1
\t}
\tweapon.
}
"""
    path = write(root, "weapon.gnut", source)
    client = LspClient(server, root)
    client.open(path, source)
    offered = labels(client.completion(path, source, source.rindex("weapon.") + len("weapon.")))
    client.shutdown()
    shutil.rmtree(root)
    assert offered == ["Fire", "ammo"], offered


def check_return_inference(server):
    """An undeclared return type is inferred across files from the values the function returns."""
    root = tempfile.mkdtemp()
    write(
        root,
        "returns.gnut",
        "global class Pilot {\n\tvoid function Respawn() {}\n}\nglobal function MakeLocal\n"
        "function MakeLocal() {\n\tlocal pilot = Pilot()\n\treturn pilot\n}\n",
    )
    source = "void function Other() {\n\tMakeLocal().\n}\n"
    path = write(root, "using.gnut", source)
    client = LspClient(server, root)
    client.open(path, source)
    offset = source.rindex("MakeLocal().") + len("MakeLocal().")
    offered = labels(client.completion(path, source, offset))
    client.shutdown()
    shutil.rmtree(root)
    assert offered == ["Respawn"], offered


def check_vm_targets(server):
    """Completion inside a guard skips globals that only exist in another VM."""
    root = tempfile.mkdtemp()
    write(
        root,
        "library.gnut",
        "#if SERVER\nglobal function ServerSideThing\nvoid function ServerSideThing() {}\n#endif\n"
        "#if CLIENT\nglobal function ClientSideThing\nvoid function ClientSideThing() {}\n#endif\n"
        "global function SharedThing\nvoid function SharedThing() {}\n",
    )
    source = "void function Caller() {\n#if CLIENT\n\tSideThing\n#endif\n}\n"
    path = write(root, "caller.gnut", source)
    client = LspClient(server, root)
    client.open(path, source)
    offset = source.index("SideThing") + len("SideThing")
    offered = [name for name in labels(client.completion(path, source, offset)) if "Thing" in name]
    client.shutdown()
    shutil.rmtree(root)
    assert offered == ["ClientSideThing", "SharedThing"], offered


def check_manifest_targets_and_dimming(server):
    """A manifest's RunOn narrows a whole file, and unreachable guards come back as comments."""
    root = tempfile.mkdtemp()
    write(
        root,
        "Example.Mod/mod.json",
        json.dumps(
            {
                "Name": "Example",
                "LoadPriority": 0,
                "Scripts": [{"Path": "ui_thing.nut", "RunOn": "UI"}],
            }
        ),
    )
    source = """global function UiThing
void function UiThing() {}
#if SERVER
void function ServerOnly() {}
int deadValue = 3
#endif
void function AlsoUi() {}
"""
    path = write(root, "Example.Mod/mod/scripts/vscripts/ui_thing.nut", source)
    client = LspClient(server, root)
    client.open(path, source)
    dimmed = [
        line
        for line, _, _, token_type, _ in decode_tokens(client.semantic_tokens(path))
        if token_type == COMMENT_TOKEN_TYPE
    ]
    client.shutdown()
    shutil.rmtree(root)
    assert dimmed == [3, 4], dimmed


def check_api_source_root(server):
    """A Northstar-style dependency supplies VM-aware symbols but remains read-only."""
    root = tempfile.mkdtemp()
    workspace = os.path.join(root, "workspace")
    dependency = os.path.join(root, "NorthstarMods")
    write(
        dependency,
        "Api.Mod/mod.json",
        json.dumps(
            {
                "Name": "API",
                "Scripts": [{"Path": "api.gnut", "RunOn": "UI"}],
            }
        ),
    )
    api_source = """global function ApiCall
void function ApiCall() {
	WorkspaceOwned()
	wait 0
}
"""
    api_path = write(
        dependency,
        "Api.Mod/mod/scripts/vscripts/api.gnut",
        api_source,
    )
    source = """global function WorkspaceOwned
void function WorkspaceOwned() {}
void function Example() {
#if SERVER
	Api
#endif
#if UI
	Api
#endif
	ApiCall()
}
"""
    path = write(workspace, "caller.gnut", source)
    client = LspClient(
        server,
        workspace,
        initialization_options={"apiSourceRoots": [dependency]},
    )
    client.open(path, source)
    server_offset = source.index("\tApi") + len("\tApi")
    ui_offset = source.index("\tApi", server_offset) + len("\tApi")
    assert "ApiCall" not in labels(client.completion(path, source, server_offset))
    assert "ApiCall" in labels(client.completion(path, source, ui_offset))

    api_call_offset = source.rindex("ApiCall")
    assert client.prepare_rename(path, source, api_call_offset) is None
    owned_offset = source.index("WorkspaceOwned")
    assert client.prepare_rename(path, source, owned_offset) is None

    client.open(api_path, api_source)
    assert client.diagnostics(api_path) == []
    client.shutdown()
    shutil.rmtree(root)


def check_duplicate_declarations(server):
    """A redeclared local is published as a warning; exclusive `#if` branches are not."""
    root = tempfile.mkdtemp()
    source = """void function Example( entity player )
{
\tlocal value = 1
\tlocal value = 2
\tentity player = value
#if SP
\tlocal build = 1
#else
\tlocal build = 2
#endif
}
"""
    path = write(root, "duplicates.gnut", source)
    client = LspClient(server, root)
    client.open(path, source)
    published = client.diagnostics(path)
    client.shutdown()
    shutil.rmtree(root)
    reported = [
        (item["range"]["start"]["line"], item["severity"], item["message"]) for item in published
    ]
    assert reported == [
        (3, 2, "`value` is already declared in this scope"),
        (4, 2, "`player` shadows the parameter of the same name"),
    ], reported
    related = published[0]["relatedInformation"][0]
    assert related["location"]["range"]["start"]["line"] == 2, related


def check_cross_file_lint_refresh(server):
    """An unsaved thread call adds and removes a lint warning in another open document."""
    root = tempfile.mkdtemp()
    definition = "void function Poll() {\n\twhile ( true ) { DoWork() }\n}\n"
    definition_path = write(root, "definition.gnut", definition)
    caller_path = write(root, "caller.gnut", "Poll()\n")
    client = LspClient(server, root)
    client.open(definition_path, definition)
    assert client.diagnostics(definition_path) == []

    client.open(caller_path, "thread Poll()\n")
    published = client.diagnostics(definition_path)
    assert len(published) == 1, published
    assert published[0]["code"] == "threaded-loop-without-wait", published

    client.change(caller_path, "Poll()\n", version=2)
    assert client.diagnostics(definition_path) == []
    client.shutdown()
    shutil.rmtree(root)


def check_unopened_workspace_lint(server):
    """Workspace lint findings are published without opening the file that contains the loop."""
    root = tempfile.mkdtemp()
    definition = "void function Poll() {\n\twhile ( true ) { DoWork() }\n}\n"
    definition_path = write(root, "definition.gnut", definition)
    caller_path = write(root, "caller.gnut", "thread Poll()\n")
    client = LspClient(server, root)

    published = client.diagnostics(definition_path)
    assert len(published) == 1, published
    assert published[0]["code"] == "threaded-loop-without-wait", published

    client.open(caller_path, "Poll()\n")
    assert client.diagnostics(definition_path) == []
    client.shutdown()
    shutil.rmtree(root)


def check_invalid_members(server):
    """A name missing from a fully known struct or class is reported, open owners stay silent."""
    root = tempfile.mkdtemp()
    write(
        root,
        "types.gnut",
        "global struct Loadout {\n\tstring name\n}\n"
        "global class Base {\n\tvoid function Respawn() {}\n}\n"
        "global class Pilot extends Base {}\n",
    )
    source = """void function Example( Loadout loadout, Pilot pilot, entity player )
{
\tprintt( loadout.name )
\tprintt( loadout.nmae )
\tpilot.Respawn()
\tplayer.GetTeam()
\tlocal holder = {}
\tholder.anything = 1
}
"""
    path = write(root, "example.gnut", source)
    client = LspClient(server, root)
    client.open(path, source)
    published = client.diagnostics(path)
    client.shutdown()
    shutil.rmtree(root)
    reported = [
        (item["range"]["start"]["line"], item["severity"], item["message"]) for item in published
    ]
    assert reported == [(3, 2, "`nmae` is not a member of `Loadout`")], reported


def check_call_arity(server):
    """A wrong argument count is reported across files; overridable names stay silent."""
    root = tempfile.mkdtemp()
    write(
        root,
        "shared.gnut",
        "global function Exact\n\nvoid function Exact( int first, float second ) {}\n",
    )
    write(
        root,
        "vanilla.gnut",
        "untyped\n\nglobalize_all_functions\n\nfunction Hud_Hide( __t__, __tt__ )\n{\n}\n",
    )
    source = """void function Example()
{
\tExact( 1, 2.0 )
\tExact( 1 )
\tHud_Hide( 1 )
\tNative( 1, 2, 3 )
}
"""
    path = write(root, "example.gnut", source)
    client = LspClient(server, root)
    client.open(path, source)
    published = client.diagnostics(path)
    client.shutdown()
    shutil.rmtree(root)
    reported = [
        (item["range"]["start"]["line"], item["severity"], item["message"]) for item in published
    ]
    assert reported == [
        (
            3,
            2,
            "`void function Exact(int first, float second)` takes 2 arguments, but 1 argument is given",
        )
    ], reported


def check_type_mismatch(server):
    """A value of the wrong declared type is reported; a subclass and open types stay silent."""
    root = tempfile.mkdtemp()
    write(
        root,
        "types.gnut",
        "global struct Loadout {\n\tstring name\n}\n"
        "global class Base {}\n"
        "global class Pilot extends Base {}\n"
        "global function MakePilot\n\nPilot function MakePilot() {\n\treturn Pilot()\n}\n",
    )
    source = """void function Example( entity player )
{
\tBase base = MakePilot()
\tLoadout wrong = MakePilot()
\tentity native = player
}
"""
    path = write(root, "example.gnut", source)
    client = LspClient(server, root)
    client.open(path, source)
    published = client.diagnostics(path)
    client.shutdown()
    shutil.rmtree(root)
    reported = [
        (item["range"]["start"]["line"], item["severity"], item["message"]) for item in published
    ]
    assert reported == [(3, 2, "`Pilot` is not a `Loadout`")], reported


def check_formatting_uses_the_discovered_config(server):
    """The server formats with the nearest `.sqformat.toml`, the same file the CLI would find."""
    root = tempfile.mkdtemp()
    write(root, ".sqformat.toml", 'indent_style = "space"\nindent_width = 2\n')
    source = "void function Example()\n{\nif ( ready )\n{\nWait( 1 )\n}\n}\n"
    path = write(root, "nested/example.gnut", source)
    client = LspClient(server, root)
    client.open(path, source)
    edits = client.formatting(path)
    client.shutdown()
    shutil.rmtree(root)
    assert len(edits) == 1, edits
    formatted = edits[0]["newText"]
    assert "\n  if ( ready )" in formatted, formatted
    assert "\t" not in formatted, formatted


def check_exit_without_dynamic_registration(server):
    """A client that never answers `client/registerCapability` must still be able to exit."""
    root = tempfile.mkdtemp()
    client = LspClient(server, root, capabilities={})
    code = client.shutdown(timeout=5)
    shutil.rmtree(root)
    assert code == 0, f"server did not exit cleanly: {code}"


def check_completion_trigger(server):
    """Member completion is advertised to open automatically after a dot."""
    root = tempfile.mkdtemp()
    client = LspClient(server, root)
    completion = client.initialize_result["capabilities"]["completionProvider"]
    client.shutdown()
    shutil.rmtree(root)
    assert completion["triggerCharacters"] == ["."], completion


CHECKS = [
    check_branch_slot_join,
    check_callable_signatures,
    check_post_initializer,
    check_return_inference,
    check_vm_targets,
    check_manifest_targets_and_dimming,
    check_api_source_root,
    check_duplicate_declarations,
    check_cross_file_lint_refresh,
    check_unopened_workspace_lint,
    check_invalid_members,
    check_call_arity,
    check_type_mismatch,
    check_formatting_uses_the_discovered_config,
    check_completion_trigger,
    check_exit_without_dynamic_registration,
]


def main():
    if len(sys.argv) < 2:
        server = os.path.abspath("../../../target/debug/sqformat-lsp")
    else:
        server = os.path.abspath(sys.argv[1])
    failures = 0
    for check in CHECKS:
        try:
            check(server)
            print(f"ok   {check.__name__}")
        except AssertionError as error:
            failures += 1
            print(f"FAIL {check.__name__}: {error}")
    print(f"{len(CHECKS) - failures}/{len(CHECKS)} passed")
    raise SystemExit(1 if failures else 0)


if __name__ == "__main__":
    main()

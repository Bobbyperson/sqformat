"""Checks against a real NorthstarMods checkout.

These pin behavior that only shows up at project scale: name collisions across hundreds of files,
VM guards in shared scripts, and manifest-declared targets. Run with a built server and, if the
corpus is not the sibling `../NorthstarMods`, its path:

    python3 check_corpus.py ../../../target/debug/sqformat-lsp [/path/to/NorthstarMods]

The corpus evolves independently of the server, so inspect a changed source file before treating a
failed check as a regression.
"""

import json
import os
import re
import sys

from lsp_client import LspClient, decode_tokens, labels

COMMENT_TOKEN_TYPE = 13
REPO = os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../..")
# The corpus is normally checked out beside the repo, or beside its parent.
DEFAULT_CORPUS_PATHS = [
    os.path.join(REPO, "../NorthstarMods"),
    os.path.join(REPO, "../../NorthstarMods"),
]


def check_file_local_references(server, corpus):
    """`OnPlayerKilled` is defined in eight gamemodes without `global`, so it stays file-local."""
    path = f"{corpus}/Northstar.Custom/mod/scripts/vscripts/gamemodes/_gamemode_gg.gnut"
    client = LspClient(server, corpus)
    text = client.open(path)
    offset = text.index("void function OnPlayerKilled") + len("void function ")
    references = client.references(path, text, offset)
    client.shutdown()
    files = {location["uri"].rsplit("/", 1)[-1] for location in references}
    assert files == {"_gamemode_gg.gnut"}, files


def check_exported_references(server, corpus):
    """An exported callback registrar is still found project-wide."""
    path = f"{corpus}/Northstar.Custom/mod/scripts/vscripts/gamemodes/_gamemode_gg.gnut"
    client = LspClient(server, corpus)
    text = client.open(path)
    offset = text.index("AddCallback_OnPlayerKilled")
    references = client.references(path, text, offset)
    client.shutdown()
    files = {location["uri"].rsplit("/", 1)[-1] for location in references}
    assert len(files) > 1, f"expected use outside the declaring file, got {len(files)} file"


def check_guarded_completion(server, corpus):
    """A shared script guards client and server helpers; each guard sees only its own."""
    path = f"{corpus}/Northstar.Custom/mod/scripts/vscripts/sh_custom_scoreboard_columns.gnut"
    original = open(path).read()
    client = LspClient(server, corpus)
    seen = {}
    for index, guard in enumerate(["#if CLIENT\n", "#if SERVER\n"]):
        text = original.replace(guard, guard + "\tCustom\n", 1)
        client.open(path, text, version=index + 1)
        offset = text.index(guard) + len(guard) + len("\tCustom")
        seen[guard.strip()] = set(labels(client.completion(path, text, offset)))
        client.close(path)
    client.shutdown()
    client_side = seen["#if CLIENT"]
    server_side = seen["#if SERVER"]
    assert "Client_CustomScoreboardColumns_Init" in client_side
    assert "Server_AddCustomScoreboardColumn" not in client_side
    assert "Server_AddCustomScoreboardColumn" in server_side
    assert "Client_CustomScoreboardColumns_Init" not in server_side
    # Shared helpers stay available in both.
    assert "Shared_GetCustomScoreboardColumns" in client_side & server_side


def check_manifest_narrowing(server, corpus):
    """An unguarded global in a `RunOn: SERVER` script is hidden from a `RunOn: UI` script."""
    ui = f"{corpus}/Northstar.Client/mod/scripts/vscripts/ui/menu_ns_modmenu.nut"
    server_script = f"{corpus}/Northstar.Custom/mod/scripts/vscripts/sh_northstar_custom_precache.gnut"
    client = LspClient(server, corpus)
    offered = {}
    for index, path in enumerate([ui, server_script]):
        text = open(path).read() + "\nvoid function __probe() {\n\tNorthstar\n}\n"
        client.open(path, text, version=index + 1)
        offset = text.rindex("\tNorthstar") + len("\tNorthstar")
        offered[path] = set(labels(client.completion(path, text, offset)))
        client.close(path)
    client.shutdown()
    assert "NorthstarCustomPrecache" not in offered[ui]
    assert "NorthstarCustomPrecache" in offered[server_script]


def check_semantic_tokens(server, corpus):
    """Tokens for real files must be ordered, non-overlapping, and single-line."""
    path = f"{corpus}/Northstar.Custom/mod/scripts/vscripts/gamemodes/_gamemode_gg.gnut"
    client = LspClient(server, corpus)
    client.open(path)
    tokens = decode_tokens(client.semantic_tokens(path))
    client.shutdown()
    assert tokens, "expected a populated token list"
    previous = (0, 0)
    for line, character, length, _, _ in tokens:
        assert length > 0
        assert (line, character) >= previous, f"tokens out of order at line {line}"
        previous = (line, character + length)


def check_invalid_members(server, corpus):
    """The member check fires on a real file: `ServerInfo` is a struct this corpus declares."""
    path = f"{corpus}/Northstar.Client/mod/scripts/vscripts/ui/menu_ns_serverbrowser.nut"
    original = open(path).read()
    text = original.replace("server.playerCount", "server.playerCounts", 1)
    assert text != original, "the corpus no longer has the field access this check edits"
    client = LspClient(server, corpus)
    client.open(path, original, version=1)
    clean = [
        item
        for item in client.diagnostics(path, version=1)
        if "is not a member of" in item["message"]
    ]
    client.open(path, text, version=2)
    typo = [
        item["message"]
        for item in client.diagnostics(path, version=2)
        if "is not a member of" in item["message"]
    ]
    client.shutdown()
    assert not clean, clean
    assert typo == ["`playerCounts` is not a member of `ServerInfo`"], typo


def check_call_arity(server, corpus):
    """The arity check fires on a real call to a function declared in the same file."""
    path = f"{corpus}/Northstar.Client/mod/scripts/vscripts/ui/menu_ns_serverbrowser.nut"
    original = open(path).read()
    text = original.replace(
        "DisplayFocusedServerInfo( file.serverButtonFocusedID )",
        "DisplayFocusedServerInfo( file.serverButtonFocusedID, 1 )",
        1,
    )
    assert text != original, "the corpus no longer has the call this check edits"
    client = LspClient(server, corpus)
    client.open(path, original, version=1)
    clean = [item for item in client.diagnostics(path, version=1) if "takes" in item["message"]]
    client.open(path, text, version=2)
    extra = [
        item["message"]
        for item in client.diagnostics(path, version=2)
        if "takes" in item["message"]
    ]
    client.shutdown()
    assert not clean, clean
    assert extra == [
        "`void function DisplayFocusedServerInfo(int scriptID)` takes 1 argument, but 2 arguments are given"
    ], extra


def check_type_mismatch(server, corpus):
    """The mismatch check fires when a real declared struct is given another declared struct."""
    path = f"{corpus}/Northstar.Client/mod/scripts/vscripts/ui/menu_ns_serverbrowser.nut"
    original = open(path).read()
    probe = "\nvoid function __probe() {\n\tserverStruct source\n\tServerInfo info = source\n}\n"
    assert "struct serverStruct" in original, "the corpus no longer declares this file's struct"
    client = LspClient(server, corpus)
    client.open(path, original, version=1)
    clean = [item for item in client.diagnostics(path, version=1) if "is not a" in item["message"]]
    client.open(path, original + probe, version=2)
    mismatch = [
        item["message"]
        for item in client.diagnostics(path, version=2)
        if "is not a" in item["message"]
    ]
    client.shutdown()
    assert not clean, clean
    assert mismatch == ["`serverStruct` is not a `ServerInfo`"], mismatch


def check_warnings_over_the_corpus(server, corpus):
    """Shipping code compiles, so every warning over the whole corpus must be explainable."""
    lint_rules = {
        "threaded-loop-without-wait",
        "invalid-entity-use",
        "wait-zero",
        "unregistered-signal",
        "unchecked-encoded-ehandle",
        "entity-use-after-yield",
        "unsafe-array-index",
        "remote-function-contract-mismatch",
        "thread-spawned-inside-polling-loop",
        "find-used-as-boolean",
    }
    scripts = []
    for directory, _, names in os.walk(corpus):
        scripts.extend(
            os.path.join(directory, name)
            for name in names
            if name.endswith((".nut", ".gnut"))
        )
    scripts.sort()
    client = LspClient(server, corpus)
    reported = []
    for index, path in enumerate(scripts):
        client.open(path, version=index + 1)
        for item in client.diagnostics(path, version=index + 1):
            if item.get("severity") == 2:
                reported.append(
                    (
                        path[len(corpus) + 1 :],
                        item["range"]["start"]["line"] + 1,
                        item["message"],
                        item.get("code"),
                    )
                )
        client.close(path)
    client.shutdown()
    # Every duplicate is a body local that reuses its own parameter's name, which compiles but
    # hides the argument. Anything else here is a false positive until proven otherwise.
    shadowed = [item for item in reported if "shadows the parameter" in item[2]]
    unexpected = [item for item in reported if item not in shadowed and item[3] not in lint_rules]
    for item in unexpected:
        print(f"     {item[0]}:{item[1]} {item[2]}")
    lint_findings = [item for item in reported if item[3] in lint_rules]
    print(f"     {len(lint_findings)} lint findings, {len(shadowed)} parameter shadows")
    assert not unexpected, unexpected
    # Member checks only fire on types whose members are all known, so working code must be clean.
    assert not [item for item in reported if "is not a member of" in item[2]]
    # The same holds for arity and declared types: what these check, this corpus ships.
    assert not [item for item in reported if "takes" in item[2]]
    assert not [item for item in reported if re.search(r"is not a `", item[2])]


def report_dimmed_regions(server, corpus):
    """Informational: how many manifest-listed scripts have a provably unreachable region."""
    listed = []
    for mod in ["Northstar.Client", "Northstar.Custom", "Northstar.CustomServers"]:
        manifest = f"{corpus}/{mod}/mod.json"
        if not os.path.exists(manifest):
            continue
        for entry in json.load(open(manifest)).get("Scripts", []):
            path = os.path.join(corpus, mod, "mod/scripts/vscripts", entry["Path"])
            if os.path.exists(path) and re.search(r"^\s*#if", open(path).read(), re.M):
                listed.append(path)
    client = LspClient(server, corpus)
    dimmed = 0
    for index, path in enumerate(listed):
        client.open(path, version=index + 1)
        if any(
            token_type == COMMENT_TOKEN_TYPE
            for _, _, _, token_type, _ in decode_tokens(client.semantic_tokens(path))
        ):
            dimmed += 1
        client.close(path)
    client.shutdown()
    print(f"     {dimmed}/{len(listed)} listed scripts with #if have an unreachable region")


CHECKS = [
    check_file_local_references,
    check_exported_references,
    check_guarded_completion,
    check_manifest_narrowing,
    check_semantic_tokens,
    check_invalid_members,
    check_call_arity,
    check_type_mismatch,
    check_warnings_over_the_corpus,
    report_dimmed_regions,
]


def main():
    if len(sys.argv) < 2:
        raise SystemExit("usage: check_corpus.py <path-to-sqformat-lsp> [corpus-path]")
    server = os.path.abspath(sys.argv[1])
    if len(sys.argv) > 2:
        candidates = [sys.argv[2]]
    else:
        candidates = DEFAULT_CORPUS_PATHS
    corpus = next((os.path.abspath(path) for path in candidates if os.path.isdir(path)), None)
    if corpus is None:
        print("skipped: no NorthstarMods checkout found at " + " or ".join(candidates))
        return
    failures = 0
    for check in CHECKS:
        try:
            check(server, corpus)
            print(f"ok   {check.__name__}")
        except AssertionError as error:
            failures += 1
            print(f"FAIL {check.__name__}: {error}")
    print(f"{len(CHECKS) - failures}/{len(CHECKS)} passed")
    raise SystemExit(1 if failures else 0)


if __name__ == "__main__":
    main()

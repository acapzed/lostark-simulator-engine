"""Export a committed engine snapshot, or verify its recorded file hashes."""
import argparse
import hashlib
import json
import posixpath
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "engine-source-manifest.json"
INCLUDE = re.compile(r'(include_(?:str|bytes)!\(\s*")([^"]+)(")')


def git(folder, *args):
    return subprocess.check_output(["git", "-C", str(folder), *args])


def output_path(name):
    path = (ROOT / name).resolve()
    if not path.is_relative_to(ROOT) or path == ROOT:
        raise ValueError(f"Unsafe output path: {name}")
    return path


def sha(data):
    return hashlib.sha256(data).hexdigest()


def verify():
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    for row in manifest["files"]:
        data = output_path(row["path"]).read_bytes()
        if len(data) != row["bytes"] or sha(data) != row["sha256"]:
            raise ValueError(f"Snapshot mismatch: {row['path']}")
    print(f"Verified {len(manifest['files'])} snapshot files from {manifest['source_revision']}")


def export(app, revision):
    revision = git(app, "rev-parse", "--verify", f"{revision}^{{commit}}").decode().strip()
    tracked = set(git(app, "ls-tree", "-r", "--name-only", revision).decode().splitlines())
    selected = {p for p in tracked if p.startswith(("simulator/src/", "simulator/fixtures/"))}
    selected.update(["simulator/Cargo.toml", "simulator/Cargo.lock"])
    blobs = {}

    def read(name):
        if name not in tracked:
            raise ValueError(f"Missing committed input: {name}")
        if name not in blobs:
            blobs[name] = git(app, "show", f"{revision}:{name}")
        return blobs[name]

    def dependency(source, relative):
        name = posixpath.normpath(posixpath.join(posixpath.dirname(source), relative))
        if name.startswith("../") or name.startswith("/"):
            raise ValueError(f"External include: {source}: {relative}")
        return name

    for source in sorted(selected):
        if source.endswith(".rs"):
            for match in INCLUDE.finditer(read(source).decode()):
                selected.add(dependency(source, match[2]))

    def destination(source):
        if source.startswith("simulator/"):
            return source.removeprefix("simulator/")
        if source.startswith("skill-data/"):
            return source
        if source.startswith("docs/"):
            return "fixtures/app-docs/" + source.removeprefix("docs/")
        raise ValueError(f"Unexpected input family: {source}")

    # Refuse to overwrite edits in the exported tree. Repository docs/tools are independent.
    managed = ["src", "fixtures", "skill-data", "Cargo.toml", "Cargo.lock"]
    if git(ROOT, "status", "--porcelain", "--", *managed).strip():
        raise ValueError("Exported paths have local changes; commit or preserve them before syncing")
    outputs = {}
    rows = []
    for source in sorted(selected):
        original = read(source)
        name = destination(source)
        data = original
        if source.endswith(".rs"):
            def rewrite(match):
                target = destination(dependency(source, match[2]))
                relative = posixpath.relpath(target, posixpath.dirname(name))
                return match[1] + relative + match[3]
            data = INCLUDE.sub(rewrite, original.decode()).encode()
        elif name == "Cargo.toml":
            data = original.decode().replace('edition = "2021"', 'edition = "2021"\nlicense = "GPL-3.0-only"').encode()
        outputs[name] = data
        rows.append({"source_path": source, "source_sha256": sha(original), "path": name,
                     "bytes": len(data), "sha256": sha(data)})

    # Prune only previously tracked export files, never ignored or untracked local artifacts.
    previous = git(ROOT, "ls-files", "--", *managed).decode().splitlines()
    for name in previous:
        if name not in outputs:
            output_path(name).unlink()
    for name, data in outputs.items():
        target = output_path(name)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    manifest = {"schema_version": 1, "source_repository": "https://github.com/acapzed/lostark-simulator",
                "source_revision": revision, "adaptations": ["crate-relative include paths", "GPL-3.0-only package metadata"],
                "files": rows}
    MANIFEST.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    verify()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--app", type=Path)
    parser.add_argument("--revision", default="HEAD")
    args = parser.parse_args()
    if args.check:
        verify()
    elif args.app:
        export(args.app.resolve(), args.revision)
    else:
        parser.error("Pass --check, or --app with an optional --revision")

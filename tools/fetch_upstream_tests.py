#!/usr/bin/env python3
"""Fetches upstream's own Python tests and reports whether ours have drifted.

The point of running these is that they are *not* ours: a test we would have to edit
to pass is a compatibility gap, not a test problem. Vendored copies satisfy that only
until upstream changes one, which they do silently. So CI fetches them fresh every run
and compares against the copies in `tests/upstream*`.

Two outcomes matter and are reported separately:

* a fetched test *fails* -- we are not compatible with upstream as it stands today;
* a fetched test *differs* from our vendored copy -- upstream changed it, and our copy
  needs updating deliberately rather than drifting.

The fetched copies must be run in their own pytest invocation. They share basenames
with the vendored ones -- that is the point of comparing them -- and pytest refuses to
import two modules of the same name from different directories.

`test_projection.py` from the Autoware extension is skipped: it locates its input map
through `ament_index_python`, so the ROS dependency is in the test's plumbing rather
than in anything it exercises.
"""

import argparse
import filecmp
import pathlib
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent

SOURCES = [
    {
        "name": "lanelet2",
        "repo": "https://github.com/fzi-forschungszentrum-informatik/Lanelet2.git",
        "path": "lanelet2_python/test",
        "vendored": "tests/upstream",
        "skip": set(),
    },
    {
        "name": "autoware_lanelet2_extension",
        "repo": "https://github.com/autowarefoundation/autoware_lanelet2_extension.git",
        "path": "autoware_lanelet2_extension_python/test",
        "vendored": "tests/upstream-awext",
        # Needs ament_index_python to find its map; see the module docstring.
        "skip": {"test_projection.py"},
    },
]


def fetch(source, ref, into):
    """Shallow, sparse clone of just the test directory."""
    clone = into / source["name"]
    subprocess.run(
        ["git", "clone", "--depth", "1", "--filter=blob:none", "--sparse",
         "--branch", ref, source["repo"], str(clone)],
        check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    subprocess.run(
        ["git", "-C", str(clone), "sparse-checkout", "set", source["path"]],
        check=True, stdout=subprocess.DEVNULL,
    )
    sha = subprocess.run(
        ["git", "-C", str(clone), "rev-parse", "HEAD"],
        check=True, capture_output=True, text=True,
    ).stdout.strip()
    return clone / source["path"], sha


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ref", default="main", help="branch or tag to fetch")
    parser.add_argument("--out", default="tests/upstream-fresh")
    args = parser.parse_args()

    out = ROOT / args.out
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)

    drifted = []
    with tempfile.TemporaryDirectory(prefix="ll2-upstream-") as workdir:
        for source in SOURCES:
            for ref in (args.ref, "master", "main"):
                try:
                    tests, sha = fetch(source, ref, pathlib.Path(workdir))
                    break
                except subprocess.CalledProcessError:
                    continue
            else:
                raise SystemExit("could not clone %s" % source["repo"])

            target = out / source["name"]
            target.mkdir()
            print("%s @ %s (%s)" % (source["name"], sha[:12], ref))
            vendored = ROOT / source["vendored"]
            for path in sorted(tests.glob("test_*.py")):
                if path.name in source["skip"]:
                    print("  skip    %s" % path.name)
                    continue
                shutil.copy(path, target / path.name)
                mine = vendored / path.name
                if not mine.exists():
                    drifted.append("%s is new upstream" % path.name)
                    print("  NEW     %s" % path.name)
                elif not filecmp.cmp(path, mine, shallow=False):
                    drifted.append("%s changed upstream" % path.name)
                    print("  CHANGED %s" % path.name)
                else:
                    print("  same    %s" % path.name)

    if drifted:
        print("\nupstream has moved:", *drifted, sep="\n  - ")
        print("\nThe fetched copies in %s are what CI runs. Update the vendored ones "
              "deliberately." % args.out)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

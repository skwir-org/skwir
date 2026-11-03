#!/usr/bin/env python3
"""Fetch the checksum-pinned AppImage inputs, rechecking cached files too.

Release tags and checksums are both pinned. A changed checksum is an
error, never permission to use a new build. Update downloads.json deliberately.
"""

import hashlib
import http.client
import json
import pathlib
import sys
import time
import urllib.error
import urllib.request

ATTEMPTS = 4


def download(url, partial):
    """Fetch url into partial, trying again when the connection fails.

    A server that resets a connection or stalls once (musl's cgit did, on
    0.3.0's build) is no reason to stop a release. A refusal is: a
    missing or forbidden file stays so however often it is asked.
    """
    for attempt in range(1, ATTEMPTS + 1):
        try:
            with urllib.request.urlopen(url, timeout=60) as source, partial.open("wb") as out:
                while chunk := source.read(1024 * 1024):
                    out.write(chunk)
            return
        except urllib.error.HTTPError as error:
            if error.code < 500 and error.code != 429:
                sys.exit(f"{url}: {error}")
            failure = error
        except (OSError, http.client.HTTPException) as error:
            failure = error
        if attempt == ATTEMPTS:
            sys.exit(f"{url}: {failure} ({ATTEMPTS} attempts)")
        wait = 5 * attempt
        print(f"  {failure}; trying again in {wait} s", flush=True)
        time.sleep(wait)


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            result.update(chunk)
    return result.hexdigest()


def main():
    dest = pathlib.Path(sys.argv[1])
    dest.mkdir(parents=True, exist_ok=True)
    inputs = json.loads(pathlib.Path(__file__).with_name("downloads.json").read_text())
    for name, spec in inputs.items():
        path = dest / name
        if not path.exists():
            partial = path.with_suffix(path.suffix + ".part")
            print(f"Downloading {name}", flush=True)
            download(spec["url"], partial)
            if digest(partial) != spec["sha256"]:
                partial.unlink()
                sys.exit(f"{name}: checksum differs from downloads.json")
            partial.replace(path)
        if digest(path) != spec["sha256"]:
            sys.exit(f"{name}: cached file differs from downloads.json; remove it and retry")
        executable = name.endswith(".AppImage") or name == "runtime-x86_64"
        path.chmod(0o755 if executable else 0o644)


if __name__ == "__main__":
    main()

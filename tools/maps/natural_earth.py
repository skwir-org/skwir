#!/usr/bin/env python3
"""Pack Natural Earth's land, lakes and borders for the map-tiles plugin.

Natural Earth (https://www.naturalearthdata.com) is in the public domain.
The layers are read from its GitHub release as GeoJSON and written to
crates/plugins/map-tiles/data/natural-earth.bin, which the plugin embeds
and draws as the default, offline background (ADR-020).

Format, little-endian where it matters:
  b"BNE1", u8 layer count, then for each layer:
    u8 kind (0 land, 1 lakes, 2 borders), u8 scale (110 or 50: 1:110m, 1:50m),
    varint ring count, then for each ring (or line):
      varint point count, then each point as two zigzag varints, the change
      in longitude then latitude, in 1e-5 degrees, from the point before
      (the first from 0, 0).

Usage: tools/maps/natural_earth.py [directory with the .geojson files]
Without a directory, the files are downloaded from the release below.
"""
import json
import os
import sys
import urllib.request

RELEASE = "v5.1.2"
URL = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/{}/geojson/{}.geojson"
LAYERS = [
    (0, 110, "ne_110m_land"),
    (1, 110, "ne_110m_lakes"),
    (2, 110, "ne_110m_admin_0_boundary_lines_land"),
    (0, 50, "ne_50m_land"),
    (1, 50, "ne_50m_lakes"),
    (2, 50, "ne_50m_admin_0_boundary_lines_land"),
]
OUT = os.path.join(
    os.path.dirname(__file__), "..", "..", "crates", "plugins", "map-tiles", "data", "natural-earth.bin"
)


def varint(n, out):
    while True:
        byte = n & 0x7F
        n >>= 7
        if n:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return


def zigzag(n):
    return (n << 1) ^ (n >> 63)


def rings(geometry):
    kind, coordinates = geometry["type"], geometry["coordinates"]
    if kind == "Polygon":
        yield from coordinates
    elif kind == "MultiPolygon":
        for polygon in coordinates:
            yield from polygon
    elif kind == "LineString":
        yield coordinates
    elif kind == "MultiLineString":
        yield from coordinates


def load(name, directory):
    if directory:
        with open(os.path.join(directory, name + ".geojson")) as f:
            return json.load(f)
    with urllib.request.urlopen(URL.format(RELEASE, name)) as response:
        return json.load(response)


def main():
    directory = sys.argv[1] if len(sys.argv) > 1 else None
    out = bytearray(b"BNE1")
    out.append(len(LAYERS))
    for kind, scale, name in LAYERS:
        data = load(name, directory)
        all_rings = [
            ring
            for feature in data["features"]
            if feature.get("geometry")
            for ring in rings(feature["geometry"])
            if len(ring) >= 2
        ]
        out.append(kind)
        out.append(scale)
        varint(len(all_rings), out)
        points = 0
        for ring in all_rings:
            varint(len(ring), out)
            last = (0, 0)
            for position in ring:
                lon, lat = round(position[0] * 1e5), round(position[1] * 1e5)
                varint(zigzag(lon - last[0]), out)
                varint(zigzag(lat - last[1]), out)
                last = (lon, lat)
                points += 1
        print(f"{name}: {len(all_rings)} rings, {points} points", file=sys.stderr)
    with open(OUT, "wb") as f:
        f.write(out)
    print(f"{os.path.normpath(OUT)}: {len(out)} bytes", file=sys.stderr)


if __name__ == "__main__":
    main()

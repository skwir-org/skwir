#!/usr/bin/env python3
"""Pack place names for the geo plugin (ADR-022, "Place names offline").

Two sources, one format:

- Natural Earth's populated places at 1:10m (public domain,
  https://www.naturalearthdata.com), about 7 300 cities and towns. This is
  the default: the layer is read from Natural Earth's GitHub release as
  GeoJSON (or from a directory given) and written to
  crates/plugins/geo/data/places.bin, which the plugin embeds.

- GeoNames' `cities1000` (CC BY 4.0, https://www.geonames.org), about
  150 000 places of more than 1 000 people, for those who want names at the
  scale of a walk. skwir does not ship it and makes no request for it: the
  user downloads `cities1000.zip` from https://download.geonames.org/export/dump/
  (or lets this script do it with --download) and packs it with --geonames.
  Its licence requires attribution: skwir shows "GeoNames (CC BY 4.0)" beside
  the names it gives while the file is in use.

  skwir looks for the packed file at $XDG_DATA_HOME/skwir/places/geonames.bin
  (~/.local/share/skwir/places/geonames.bin by default), then at
  <dir>/skwir/places/geonames.bin for each directory of $XDG_DATA_DIRS
  (/usr/local/share:/usr/share by default). When it finds one it can read,
  it uses it instead of the embedded Natural Earth places, of which it is a
  finer superset; a damaged file is ignored and the embedded places used.

  Kept from GeoNames: the populated places of feature class P, except
  sections of a populated place (PPLX: a city's neighbourhoods, which would
  otherwise name the middle of every city) and historical, abandoned or
  destroyed places (PPLH, PPLQ, PPLW, PPLCH). The `name` column is kept
  (the name in the language the place uses, romanized when GeoNames does);
  alternate and localized names are left out.

Format, little-endian where it matters (nothing is wider than a byte):
  b"BPL1", u8 source (0 Natural Earth, 1 GeoNames),
  u8 country count, then each country as its two-letter ISO 3166 code
    (two ASCII bytes; the first entry is b"\\0\\0", no known country),
  varint place count, then for each place:
    varint name length, the name in UTF-8 (at most 255 bytes),
    two zigzag varints: the change in latitude then longitude, in 1e-5
      degrees, from the place before (the first from 0, 0),
    u8 rank (below), u8 index into the country table.
  Places are in order of their 1-degree cell, then of latitude and
  longitude, so the changes stay small. Varints are LEB128 (seven bits a
  byte, low first); zigzag maps 0, -1, 1, -2... to 0, 1, 2, 3...

The rank is the class of the place's population, as Natural Earth's
rank_max counts it: 14 for 10 million people or more, 13 for 5 million,
12 for 1 million, 11 for 500 000, 10 for 200 000, 9 for 100 000,
8 for 50 000, 7 for 20 000, 6 for 10 000, 5 for 5 000, 4 for 2 000,
3 for 1 000, 2 for 200, 1 for fewer or an unknown population.

Usage:
  tools/maps/places.py [directory with ne_10m_populated_places_simple.geojson]
  tools/maps/places.py --geonames cities1000.zip [--out PATH]
  tools/maps/places.py --geonames --download [--out PATH]
--geonames takes the .zip or the .txt inside it; --out defaults to the first
place skwir looks, under $XDG_DATA_HOME.
"""
import argparse
import io
import json
import math
import os
import sys
import urllib.request
import zipfile

RELEASE = "v5.1.2"
NE_URL = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/{}/geojson/{}.geojson"
NE_LAYER = "ne_10m_populated_places_simple"
GEONAMES_URL = "https://download.geonames.org/export/dump/cities1000.zip"
NE_OUT = os.path.join(
    os.path.dirname(__file__), "..", "..", "crates", "plugins", "geo", "data", "places.bin"
)
SOURCE_NATURAL_EARTH = 0
SOURCE_GEONAMES = 1
# Lower bounds of the population classes, rank 14 first.
RANKS = [10_000_000, 5_000_000, 1_000_000, 500_000, 200_000, 100_000, 50_000,
         20_000, 10_000, 5_000, 2_000, 1_000, 200]
LEFT_OUT = {"PPLX", "PPLH", "PPLQ", "PPLW", "PPLCH"}
NAME_MAX = 255


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


def rank(population):
    for i, bound in enumerate(RANKS):
        if population >= bound:
            return 14 - i
    return 1


def clip_name(name):
    data = name.strip().encode("utf-8")
    if len(data) <= NAME_MAX:
        return data
    return data[:NAME_MAX].decode("utf-8", "ignore").encode("utf-8")


def country(code):
    code = (code or "").strip().upper()
    return code if len(code) == 2 and code.isascii() and code.isalpha() else ""


def pack(source, places):
    """places: (name, lat, lon, rank, country) tuples."""
    countries = [""] + sorted({p[4] for p in places if p[4]})
    assert len(countries) <= 255, "more countries than a byte counts"
    index = {c: i for i, c in enumerate(countries)}
    rows = []
    for name, lat, lon, place_rank, code in places:
        data = clip_name(name)
        if not data:
            continue
        lat, lon = round(lat * 1e5), round(lon * 1e5)
        if abs(lat) > 90 * 10**5 or abs(lon) > 180 * 10**5:
            continue
        rows.append((math.floor(lat / 1e5), math.floor(lon / 1e5), lat, lon, data, place_rank, index[code]))
    rows.sort()
    out = bytearray(b"BPL1")
    out.append(source)
    out.append(len(countries))
    for code in countries:
        out += code.encode("ascii") if code else b"\0\0"
    varint(len(rows), out)
    last = (0, 0)
    for _, _, lat, lon, data, place_rank, code in rows:
        varint(len(data), out)
        out += data
        varint(zigzag(lat - last[0]), out)
        varint(zigzag(lon - last[1]), out)
        last = (lat, lon)
        out.append(place_rank)
        out.append(code)
    return out, len(rows)


def natural_earth(directory):
    if directory:
        with open(os.path.join(directory, NE_LAYER + ".geojson")) as f:
            data = json.load(f)
    else:
        with urllib.request.urlopen(NE_URL.format(RELEASE, NE_LAYER)) as response:
            data = json.load(response)
    places = []
    for feature in data["features"]:
        geometry, props = feature.get("geometry"), feature["properties"]
        if not geometry or geometry.get("type") != "Point":
            continue
        lon, lat = geometry["coordinates"][:2]
        population = max(props.get("pop_max") or 0, props.get("pop_min") or 0)
        places.append((props["name"], lat, lon, rank(population), country(props.get("iso_a2"))))
    return places


def geonames_lines(path, download):
    if download:
        print(f"downloading {GEONAMES_URL}", file=sys.stderr)
        with urllib.request.urlopen(GEONAMES_URL) as response:
            archive = zipfile.ZipFile(io.BytesIO(response.read()))
        return io.TextIOWrapper(archive.open("cities1000.txt"), encoding="utf-8")
    if path.endswith(".zip"):
        archive = zipfile.ZipFile(path)
        name = next(n for n in archive.namelist() if n.endswith(".txt"))
        return io.TextIOWrapper(archive.open(name), encoding="utf-8")
    return open(path, encoding="utf-8")


def geonames(path, download):
    # readme.txt: geonameid, name, asciiname, alternatenames, latitude,
    # longitude, feature class, feature code, country code, cc2, admin1..4,
    # population, elevation, dem, timezone, modification date.
    places = []
    for line in geonames_lines(path, download):
        fields = line.rstrip("\n").split("\t")
        if len(fields) < 15 or fields[6] != "P" or fields[7] in LEFT_OUT:
            continue
        try:
            lat, lon = float(fields[4]), float(fields[5])
            population = int(fields[14] or 0)
        except ValueError:
            continue
        places.append((fields[1], lat, lon, rank(population), country(fields[8])))
    return places


def default_out():
    home = os.environ.get("XDG_DATA_HOME") or os.path.join(os.path.expanduser("~"), ".local", "share")
    return os.path.join(home, "skwir", "places", "geonames.bin")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("directory", nargs="?", help="where ne_10m_populated_places_simple.geojson is")
    parser.add_argument("--geonames", nargs="?", const="", metavar="FILE",
                        help="pack GeoNames' cities1000 (.zip or .txt) instead")
    parser.add_argument("--download", action="store_true", help="with --geonames: download cities1000.zip")
    parser.add_argument("--out", help="with --geonames: where to write (default: under $XDG_DATA_HOME)")
    args = parser.parse_args()
    if args.geonames is not None:
        if not args.geonames and not args.download:
            parser.error("--geonames needs a file, or --download")
        out, count = pack(SOURCE_GEONAMES, geonames(args.geonames, args.download))
        path = args.out or default_out()
        os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    else:
        if args.download or args.out:
            parser.error("--download and --out go with --geonames")
        out, count = pack(SOURCE_NATURAL_EARTH, natural_earth(args.directory))
        path = NE_OUT
    with open(path, "wb") as f:
        f.write(out)
    print(f"{os.path.normpath(path)}: {count} places, {len(out)} bytes", file=sys.stderr)


if __name__ == "__main__":
    main()

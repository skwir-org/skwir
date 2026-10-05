#!/usr/bin/env python3
"""Synthetic tile archives for the map-tiles plugin's tests: invented tiles
over an invented box, written as MBTiles by sqlite3 (rows from the south, as
the spec says), for the reference `pmtiles` tool to convert to PMTiles:

    tools/maps/test_archives.py OUT 5 9     # the raster zooms
    pmtiles convert OUT/hills.mbtiles OUT/hills.pmtiles
    pmtiles convert OUT/roads.mbtiles OUT/roads.pmtiles

(`pmtiles` 1.31.2, nixpkgs#pmtiles). Its output is what
crates/plugins/map-tiles/src/testdata/ keeps: files bibli did not write.

Each raster tile is a north-to-south gradient across the whole box (blue
at the north, red at the south) with a dark frame, so tiles placed in the
wrong row or column break the gradient, and a dark square in its
north-west corner, so a tile drawn upside down shows it."""
import math, sqlite3, struct, sys, zlib, gzip, os

def png(width, height, pixel):
    raw = bytearray()
    for y in range(height):
        raw.append(0)
        for x in range(width):
            raw.extend(pixel(x, y))
    def chunk(kind, data):
        c = struct.pack('>I', len(data)) + kind + data
        return c + struct.pack('>I', zlib.crc32(kind + data) & 0xffffffff)
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(bytes(raw), 9)) + chunk(b'IEND', b''))

def tile_of(lat, lon, z):
    n = 2 ** z
    x = int((lon + 180) / 360 * n)
    y = int((1 - math.log(math.tan(math.radians(lat)) + 1 / math.cos(math.radians(lat))) / math.pi) / 2 * n)
    return x, y

def lat_of(y, z):
    n = 2 ** z
    return math.degrees(math.atan(math.sinh(math.pi * (1 - 2 * y / n))))

WEST, SOUTH, EAST, NORTH = 5.0, 44.0, 6.0, 45.0

def raster(path, zooms, fmt='png'):
    db = sqlite3.connect(path)
    db.executescript("""
        CREATE TABLE metadata (name TEXT, value TEXT);
        CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB);
        CREATE UNIQUE INDEX tile_index ON tiles (zoom_level, tile_column, tile_row);""")
    meta = {
        'name': 'Invented Hills', 'format': fmt, 'type': 'baselayer', 'version': '1.0',
        'description': 'Synthetic tiles for a reader test',
        'attribution': '<a href="https://example.org/">&copy; Example Maps</a>',
        'bounds': f'{WEST},{SOUTH},{EAST},{NORTH}',
        'center': f'{(WEST+EAST)/2},{(SOUTH+NORTH)/2},{zooms[0]}',
        'minzoom': str(zooms[0]), 'maxzoom': str(zooms[-1]),
    }
    db.executemany('INSERT INTO metadata VALUES (?, ?)', meta.items())
    side = 256
    for z in zooms:
        x0, y0 = tile_of(NORTH, WEST, z)
        x1, y1 = tile_of(SOUTH, EAST, z)
        for x in range(x0, x1 + 1):
            for y in range(y0, y1 + 1):
                def pixel(px, py, x=x, y=y, z=z):
                    if px < 3 or py < 3 or px >= side - 3 or py >= side - 3:
                        return (30, 30, 30)
                    if px < 40 and py < 40:
                        return (20, 20, 20)
                    lat = lat_of(y + py / side, z)
                    t = min(max((NORTH - lat) / (NORTH - SOUTH), 0), 1)
                    return (int(40 + 200 * t), 90, int(240 - 200 * t))
                data = png(side, side, pixel)
                db.execute('INSERT INTO tiles VALUES (?, ?, ?, ?)', (z, x, 2 ** z - 1 - y, data))
    db.commit()
    db.close()

def vector(path):
    db = sqlite3.connect(path)
    db.executescript("""
        CREATE TABLE metadata (name TEXT, value TEXT);
        CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB);""")
    meta = {
        'name': 'Invented Roads', 'format': 'pbf', 'minzoom': '0', 'maxzoom': '6',
        'bounds': f'{WEST},{SOUTH},{EAST},{NORTH}',
        'json': '{"vector_layers":[{"id":"roads","fields":{}},{"id":"water","fields":{}}]}',
    }
    db.executemany('INSERT INTO metadata VALUES (?, ?)', meta.items())
    for z in range(0, 7):
        x, y = tile_of((SOUTH + NORTH) / 2, (WEST + EAST) / 2, z)
        db.execute('INSERT INTO tiles VALUES (?, ?, ?, ?)', (z, x, 2 ** z - 1 - y, gzip.compress(b'\x1a\x00')))
    db.commit()
    db.close()

if __name__ == '__main__':
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    for name in ('hills.mbtiles', 'roads.mbtiles'):
        if os.path.exists(os.path.join(out, name)):
            os.remove(os.path.join(out, name))
    raster(os.path.join(out, 'hills.mbtiles'), list(range(int(sys.argv[2]), int(sys.argv[3]) + 1)))
    vector(os.path.join(out, 'roads.mbtiles'))

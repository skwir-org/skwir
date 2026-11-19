# Test data

- `hills.pmtiles`, `roads.pmtiles`: archives for `pmtiles.rs`, written by
  the reference `pmtiles` tool (1.31.2, `pmtiles convert`) from MBTiles
  files that `tools/maps/test_archives.py` makes, so the reader is checked
  against a writer other than its own. Invented tiles over an invented box:
  PNG tiles of a north-to-south gradient at zooms 5 to 9 (tiles in one row
  are identical, so the tool shares their bytes in runs), and placeholder
  vector tiles at zooms 0 to 6.

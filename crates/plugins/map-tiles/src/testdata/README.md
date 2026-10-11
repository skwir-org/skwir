# Test data

- `ca.pem`, `server.pem`, `server.key`: throwaway certificates for the
  tests of `http.rs` only. A CA (its key discarded) and a server certificate
  for `localhost` and `127.0.0.1` signed by it, valid a hundred years. Only
  the tests trust this CA, through the client configuration they build;
  nothing at runtime can be told to.
- `hills.pmtiles`, `roads.pmtiles`: archives for `pmtiles.rs`, written by
  the reference `pmtiles` tool (1.31.2, `pmtiles convert`) from MBTiles
  files that `tools/maps/test_archives.py` makes, so the reader is checked
  against a writer other than its own. Invented tiles over an invented box:
  PNG tiles of a north-to-south gradient at zooms 5 to 9 (tiles in one row
  are identical, so the tool shares their bytes in runs), and placeholder
  vector tiles at zooms 0 to 6.

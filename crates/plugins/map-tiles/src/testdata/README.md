Throwaway certificates for the tests of `http.rs` only: a CA (its key
discarded) and a server certificate for `localhost` and `127.0.0.1` signed
by it, valid a hundred years. Only the tests trust this CA, through the
client configuration they build; nothing at runtime can be told to.

`hills.pmtiles` and `roads.pmtiles`, for `pmtiles.rs`: archives written by
the reference `pmtiles` tool (1.31.2, `pmtiles convert`) from MBTiles files
`tools/maps/test_archives.py` makes, so the reader is checked against a
writer that is not its own. Invented tiles over an invented box: PNG tiles
of a gradient from north to south at zooms 5 to 9 (tiles in one row are
the same, so the tool shares their bytes in runs), and placeholder vector
tiles at zooms 0 to 6.

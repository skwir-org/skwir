Throwaway certificates for the tests of `http.rs` only: a CA (its key
discarded) and a server certificate for `localhost` and `127.0.0.1` signed
by it, valid a hundred years. Only the tests trust this CA, through the
client configuration they build; nothing at runtime can be told to.

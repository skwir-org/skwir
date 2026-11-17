# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# An error an agent reads; the path is the file's, as it was given.
checksums_changed_ask_again = { $path } changed while it was read: ask again

checksums_changed_while_computed = { $name } changed while its checksum was computed: compute it again

# A link after a checksum in Properties: says whether the checksum on the clipboard is the same.
checksums_compare = Compare with copied

checksums_compute = Compute a checksum

# Read by agents and in F1's list of actions; `sha256`, `md5` and `blake3` are the values the command takes, as written.
checksums_compute_description = The `sha256`, `md5` or `blake3` checksum of a file, read whole: answered where it is called, or from the window shown in Properties when done

# A link in Properties: computes the checksum named on its row.
checksums_compute_link = Compute

# A checksum's value in Properties while the file is read.
checksums_computing = Computing…

# A link after a checksum in Properties: puts it on the clipboard.
checksums_copy = Copy

# The error says why, as the system or Skwir said it.
checksums_could_not_compute = The checksum of { $name } could not be computed: { $error }

# What a folded Checksums group says of a checksum computed: the algorithm's name (SHA-256), then the checksum's first digits, as written.
checksums_gist = { $algorithm } { $digest }

# A group's heading in Properties: the file's SHA-256, MD5 and BLAKE3, each computed when asked.
checksums_group = Checksums

# Why a checksum could not be computed: what was named is a folder, a pipe or a device, or is gone.
checksums_it_is_not_a_file = it is not a file

# An error an agent reads: the command was given several files, or none.
checksums_name_one_file = name one file

# An error an agent reads; the path is the file's, as it was given.
checksums_not_a_file = { $path } is not a file

checksums_plugin_description = A file's SHA-256, MD5 and BLAKE3 in Properties, computed when asked, and compared with one copied

checksums_plugin_title = Checksums

checksums_properties = Checksums in Properties

# Read by agents and in F1's list of actions.
checksums_properties_description = Properties' rows for one file: each checksum computed, being computed, or the way to compute it

checksums_stop = Stop computing a checksum

checksums_stop_description = Stop computing a file's checksum

# A link after a checksum being computed, in Properties.
checksums_stop_link = Stop

# An error an agent reads: the checksum was stopped before it was done.
checksums_stopped = stopped

# A checksum's value in Properties while it waits for another file's to be computed.
checksums_waiting = Waiting…

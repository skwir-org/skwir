/* A grammar's symbol whose language says an ABI no tree-sitter reads, for
   skwir-tree-sitter-worker's tests. The fields after it are zeros, so what
   reads one reads nothing. */
static const unsigned int language[256] = {99};

const void *tree_sitter_wrong_abi(void) { return language; }

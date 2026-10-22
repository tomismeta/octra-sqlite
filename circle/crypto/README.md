# Circle Crypto

This directory contains the small public-domain TweetNaCl C implementation used
by the Circle WASM to verify Ed25519 signed write intents.

Signatures authorize the SQLite write gateway. SQLite still executes through
the normal page VFS after that gate.

The fixed 16×16 multiplication loop in `M` has Clang full-unroll directives;
its arithmetic and operation order are unchanged. The build compiles this
translation unit at `-O3` without LTO so owner-signature verification fits
Octra's metered native runtime. SQLite and the VFS retain size-oriented flags.

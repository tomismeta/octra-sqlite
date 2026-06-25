# Circle Crypto

This directory contains the small public-domain TweetNaCl C implementation used
by the Circle WASM to verify Ed25519 signed write intents.

Signatures authorize the SQLite write gateway. SQLite still executes through
the normal page VFS after that gate.

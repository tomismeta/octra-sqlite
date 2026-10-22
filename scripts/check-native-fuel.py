#!/usr/bin/env python3
"""Optional offline fuel audit against a separately built LiteNode native host.

Requires Python 3, Node (Ed25519 fixture signing), a C compiler, and the native
host shared library. Never reads wallets or contacts a network.
"""
import argparse
import base64
import ctypes
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile


def frame(method, params):
    method = method.encode()
    result = b"OCWR1" + struct.pack(">H", len(method)) + method + struct.pack(">H", len(params))
    for param in params:
        param = param.encode()
        result += b"\x04" + struct.pack(">I", len(param)) + param
    return result


def decode(result):
    value = base64.b64decode(result["response_b64"])
    assert value[:6] == b"OCWS1\x04" and len(value) == 10 + struct.unpack(">I", value[6:10])[0]
    return json.loads(value[10:])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native-library", type=Path, required=True)
    parser.add_argument("--wasm", type=Path, default=Path(__file__).resolve().parents[1] / "circle/wasm/octra_sqlite_circle.wasm")
    parser.add_argument("--fuel", type=int, default=15_000_000)
    args = parser.parse_args()
    if sys.flags.optimize:
        parser.error("run without -O: this audit requires assertions")
    if not 1 <= args.fuel <= 20_000_000:
        parser.error("standard-profile fuel must be between 1 and 20,000,000")

    # The standalone host lacks OCaml's HFHE service. Abort if it is ever called:
    # this audit exercises only SQLite's non-HFHE imports, never a fake response.
    with tempfile.TemporaryDirectory(prefix="octra-native-fuel-") as temporary:
        temporary = Path(temporary)
        guard_source = temporary / "guard.c"
        guard_source.write_text("#include <stdint.h>\n#include <stddef.h>\n#include <stdlib.h>\nint32_t octra_circle_wasm_host_hfhe_call_json(const uint8_t *i, size_t n, uint8_t **o, size_t *ol, uint8_t **e, size_t *el) { (void)i; (void)n; (void)o; (void)ol; (void)e; (void)el; abort(); }\n")
        guard_path = temporary / ("guard.dylib" if sys.platform == "darwin" else "guard.so")
        flags = ["-dynamiclib"] if sys.platform == "darwin" else ["-shared", "-fPIC"]
        subprocess.run([os.environ.get("CC", "cc"), *flags, str(guard_source), "-o", str(guard_path)], check=True)
        guard = ctypes.CDLL(str(guard_path), mode=ctypes.RTLD_GLOBAL)
        native = ctypes.CDLL(str(args.native_library.resolve()))
        execute = native.octra_circle_wasm_host_run_json
        execute.argtypes = [ctypes.c_char_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_size_t), ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_size_t), ctypes.POINTER(ctypes.c_void_p)]
        execute.restype = ctypes.c_int32
        native.octra_circle_wasm_host_free_bytes.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        native.octra_circle_wasm_host_free_bytes.restype = None
        native.octra_compute_input_drop.argtypes = [ctypes.c_void_p]
        native.octra_compute_input_drop.restype = None

        original = args.wasm.read_bytes()
        db_id = hashlib.sha256(b"octra-sqlite-native-fuel-fixture-v1").digest()
        sql = "create table compat(id integer primary key, body text default (lower('HELLO')));\ninsert into compat default values;\ninsert into compat(body) values ('héllo ✓');\n"
        method = b"exec"
        encoded_sql = sql.encode()
        message = b"octra-sqlite.osw1.v1\0" + db_id + struct.pack(">QH", 1, len(method)) + method + struct.pack(">I", len(encoded_sql)) + encoded_sql
        # Public, never-funded deterministic test seed; no user key material.
        signer = """const c = require('node:crypto');
const input = JSON.parse(require('node:fs').readFileSync(0, 'utf8'));
const key = c.createPrivateKey({key:Buffer.concat([Buffer.from('302e020100300506032b657004220420','hex'),Buffer.alloc(32,7)]),format:'der',type:'pkcs8'});
const publicKey = c.createPublicKey(key).export({format:'der',type:'spki'}).subarray(-32);
console.log(JSON.stringify({public_key:publicKey.toString('hex'),signature:c.sign(null,Buffer.from(input.message,'base64'),key).toString('hex')}));"""
        signed = json.loads(subprocess.check_output(["node", "-e", signer], input=json.dumps({"message": base64.b64encode(message).decode()}).encode()))
        wasm = original
        for marker, replacement in [(b"OSQL_OWNER_PUBKEY_V1_PLACEHOLDER", bytes.fromhex(signed["public_key"])), (b"OSQL_DATABASE_ID_V1_PLACEHOLDER0", db_id)]:
            marker = marker.ljust(32, b"\0")
            assert wasm.count(marker) == 1, "personalization marker must be unique"
            wasm = wasm.replace(marker, replacement)

        def invoke(method, params, pairs=None):
            payload = dict(action="execute", execution_profile="standard", code_b64=base64.b64encode(wasm).decode(), request_b64=base64.b64encode(frame(method, params)).decode(), export_name="octra_update" if method == "exec" else "octra_query", is_view=method != "exec", storage_pairs=pairs or [], hfhe_strict=False, fuel_limit=args.fuel)
            raw = json.dumps(payload).encode()
            out, error, owner = ctypes.c_void_p(), ctypes.c_void_p(), ctypes.c_void_p()
            out_len, error_len = ctypes.c_size_t(), ctypes.c_size_t()
            rc = execute(raw, len(raw), ctypes.byref(out), ctypes.byref(out_len), ctypes.byref(error), ctypes.byref(error_len), ctypes.byref(owner))
            try:
                if rc != 0:
                    raise RuntimeError(ctypes.string_at(error, error_len.value).decode())
                return json.loads(ctypes.string_at(out, out_len.value))
            finally:
                if out.value:
                    native.octra_circle_wasm_host_free_bytes(out, out_len.value)
                if error.value:
                    native.octra_circle_wasm_host_free_bytes(error, error_len.value)
                if owner.value:
                    native.octra_compute_input_drop(owner)

        params = [sql, signed["public_key"], "1", signed["signature"]]
        written = invoke("exec", params)
        assert written["success"], written.get("error")
        assert decode(written)["ok"] is True
        pairs = written["storage_pairs"]
        queried = invoke("query", ["select id, body from compat order by id;"], pairs)
        assert queried["success"], queried.get("error")
        assert decode(queried)["rows"] == [[1, "hello"], [2, "héllo ✓"]]
        bad_params = params.copy()
        bad_signature = bytearray.fromhex(bad_params[3])
        bad_signature[0] ^= 1
        bad_params[3] = bad_signature.hex()
        denied = invoke("exec", bad_params)
        assert decode(denied)["error"] == "auth_bad_signature"
        assert denied["storage_pairs"] == []
        replay = invoke("exec", params, pairs)
        assert decode(replay)["error"] == "auth_replay"
        assert replay["storage_pairs"] == pairs
        print(json.dumps({"wasm_sha256": hashlib.sha256(original).hexdigest(), "native_library_sha256": hashlib.sha256(args.native_library.read_bytes()).hexdigest(), "execution_profile": "standard", "fuel_limit": args.fuel, "write_effort": written["effort_used"], "bad_signature_effort": denied["effort_used"], "rows": decode(queried)["rows"], "bad_signature": "rejected_without_storage", "replay": "rejected_without_changes"}, indent=2))
        # Keep guard loaded until all native calls and buffer releases are done.
        del native, guard


if __name__ == "__main__":
    main()

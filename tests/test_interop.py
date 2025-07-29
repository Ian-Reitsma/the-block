import struct
import the_block


def encode_py(payload):
    def enc_str(s):
        data = s.encode("utf-8")
        return struct.pack("<Q", len(data)) + data

    return b"".join(
        [
            enc_str(getattr(payload, "from")),
            enc_str(payload.to),
            struct.pack("<Q", payload.amount_consumer),
            struct.pack("<Q", payload.amount_industrial),
            struct.pack("<Q", payload.fee),
            struct.pack("<B", payload.fee_token),
            struct.pack("<Q", payload.nonce),
            struct.pack("<Q", len(payload.memo)) + bytes(payload.memo),
        ]
    )


def test_deterministic_bytes():
    p = the_block.RawTxPayload(
        from_="alice",
        to="bob",
        amount_consumer=1,
        amount_industrial=2,
        fee=3,
        fee_token=0,
        nonce=42,
        memo=b"hi",
    )
    rust_bytes = bytes(the_block.canonical_payload(p))
    py_bytes = encode_py(p)
    assert rust_bytes == py_bytes

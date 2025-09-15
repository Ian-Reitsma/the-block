# Identity Registry

Handles are normalized to lowercase NFC form and must not start with `sys/` or `admin/`.

## Signed message

Clients sign the BLAKE3 hash of the concatenation:

```
"register:" || handle_norm || pubkey || nonce_le
```

The resulting 32-byte digest is signed with Ed25519. The server verifies this
signature when binding a handle to the public key's hex address.

## Error codes

| Code           | Meaning                  |
|----------------|--------------------------|
| `E_DUP_HANDLE` | handle already registered |
| `E_BAD_SIG`    | signature verification failed |
| `E_LOW_NONCE`  | nonce not greater than previous |
| `E_RESERVED`   | handle uses a reserved prefix |

## Decentralized Identifier Anchors

DID documents may be anchored on-chain by submitting the document hash via `did anchor`. The registry records the latest hash per identity, preventing replay of older documents.

`did resolve` returns the currently anchored document for verification. Each successful anchor increments the `did_anchor_total` counter and is visible in explorer views for auditing.

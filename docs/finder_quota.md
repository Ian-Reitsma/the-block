# Finder/WebDAV Quota Enforcement

Finder and other WebDAV clients consume credits when uploading data.  Each
credit authorizes one kilobyte of storage.  A client's **logical quota** is
derived from its credit balance and equals `credits.balance * 1024` bytes.

When a client attempts to write beyond its quota, the node returns an
`ENOSPC` ("no space left on device") error.  This allows SMB/WebDAV clients
such as Finder to surface a familiar "disk full" dialog while keeping the
ledger unchanged.  Read operations remain free.

See `node/tests/finder_quota.rs` for an example of quota calculation and
`node/tests/storage_os_errors.rs` for OS-level error mapping.

# The Block Platform Integration Guide

## 200-Year Extensibility Architecture

This guide explains how future platforms can integrate with The Block's runtime architecture without modifying core code. Designed for long-term adaptability (200+ years), this system allows new platforms to plug into existing capabilities through configuration and feature flags.

## Philosophy

The Block's architecture separates **capabilities** from **implementations**:
- **Capabilities** are abstract features (e.g., "reactor-based file watching")
- **Implementations** are platform-specific code (e.g., Linux inotify, macOS kqueue)

Built-in platforms (Linux, macOS, Windows) automatically get the correct implementation based on `target_os`. Custom platforms explicitly signal their capabilities via Cargo features.

## File Watching Integration

### Built-in Implementations

| Platform | Implementation | Capability |
|----------|---------------|------------|
| Linux | inotify | `reactor-based-fs-watching` |
| macOS/BSD | kqueue | Direct event polling |
| Windows | Directory Change Notifications | Event-based watching |
| Other | Polling | Periodic filesystem scanning |

### For Custom Platforms

#### Option 1: Use Reactor-Based Watching (Like Linux)

If your platform provides file watching APIs that integrate with an async reactor:

1. **Enable the capability** in `Cargo.toml`:
   ```toml
   [dependencies]
   runtime = { path = "../runtime", features = ["reactor-based-fs-watching"] }
   ```

2. **Implement your watcher module** in `crates/runtime/src/fs/watch.rs`:
   ```rust
   #[cfg(feature = "inhouse-backend")]
   mod inhouse {
       // ... existing code ...

       #[cfg(feature = "your-platform-feature")]
       mod your_platform {
           use super::{register_fd, BaseWatcher, RecursiveMode, WatchEvent, WatchEventKind};

           pub(crate) struct Watcher {
               base: BaseWatcher,
               // your platform-specific fields
           }

           impl Watcher {
               pub(crate) async fn next_event(&mut self) -> io::Result<WatchEvent> {
                   loop {
                       if let Some(event) = self.base.pop_event() {
                           return Ok(event);
                       }
                       // Use reactor integration like Linux does
                       self.base.wait_ready().await?;
                       let events = self.read_platform_events()?;
                       self.base.push_events(events);
                   }
               }
           }
       }
   }
   ```

3. **Export your watcher**:
   ```rust
   #[cfg(feature = "your-platform-feature")]
   pub(super) use your_platform::Watcher;
   ```

#### Option 2: Direct Event Polling (Like macOS kqueue)

If your platform provides event-based file watching but reactor integration isn't ideal:

1. Implement a watcher that polls events directly
2. Use `BaseWatcher` for event queuing: `push_events()` and `pop_event()`
3. Don't use `wait_ready()` - poll your platform APIs directly

#### Option 3: Polling Fallback

For platforms without native file watching:

1. Implement periodic filesystem scanning
2. Use content hashing for reliable change detection
3. Reference the `polling` module as a template

### Available Helper APIs

When implementing file watching for your platform, you have access to:

#### BaseWatcher (for event-based platforms)

```rust
impl BaseWatcher {
    fn new(registration: IoRegistration) -> Self;
    fn push_events<I>(&mut self, events: I);  // Add events to queue
    fn pop_event(&mut self) -> Option<WatchEvent>;  // Get next event
    fn registration(&self) -> &IoRegistration;  // Access reactor registration

    // Only available with reactor-based-fs-watching feature:
    async fn wait_ready(&self) -> io::Result<()>;  // Wait for reactor readiness
}
```

#### Helper Functions

```rust
fn register_fd(
    runtime: &InHouseRuntime,
    fd: ReactorRaw,
    interest: ReactorInterest,
) -> io::Result<IoRegistration>;
```

## Network Integration

[Documentation for custom network implementations - TBD]

## Reactor Integration

[Documentation for custom reactor implementations - TBD]

## Process Monitoring Integration

[Documentation for custom process monitoring - TBD]

## Testing Your Integration

### Unit Tests

Add tests in `crates/runtime/tests/`:

```rust
#[cfg(feature = "your-platform-feature")]
mod your_platform_tests {
    use runtime::fs::Watcher;

    #[test]
    fn file_creation_detected() {
        // Test file watching works on your platform
    }
}
```

### Stress Tests

Add long-running stability tests:

```rust
#[test]
#[ignore]  // Run with --ignored for stress testing
fn stress_test_file_watching_stability() {
    // Simulate hours of file system activity
}
```

## Feature Flag Naming Conventions

- Use lowercase with hyphens: `your-platform-feature`
- Prefix platform-specific features with platform name: `custom-os-fs-watching`
- Suffix with capability: `-reactor-based`, `-polling`, etc.

## Long-Term Compatibility Guarantee

The Block guarantees:

1. **Capability features** (e.g., `reactor-based-fs-watching`) will remain stable
2. **Helper APIs** (`BaseWatcher`, `register_fd`) will maintain backward compatibility
3. **Module structure** will support new platforms without breaking existing ones

When in doubt, open an issue or refer to the built-in implementations as reference.

## Examples

See existing implementations:
- **Reactor-based**: `crates/runtime/src/fs/watch.rs` → `linux` module (lines 235-403)
- **Direct polling**: `crates/runtime/src/fs/watch.rs` → `kqueue` module (lines 405-751)
- **Polling fallback**: `crates/runtime/src/fs/watch.rs` → `polling` module (lines 921-1133)

## Getting Help

- Open an issue on GitHub with the tag `platform-integration`
- Reference this guide and the existing implementations
- The Block is designed to be extended - we're here to help!

---

*This architecture is designed to last 200+ years. As platforms evolve, The Block evolves with them.*

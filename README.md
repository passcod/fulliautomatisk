# Fulliautomatisk

An **internal tool** to **safely** watch the **internal database** of a running
Asterisk server.

 - **internal tool**: it's meant to run internally
   - HTTP compression is disabled
   - Certification verification is disabled
   - Request timeout is set to 5 seconds
   - It needs to run on the Asterisk box
 - **safely**:
   - It opens a read-only connection to SQLite
   - It does not write anywhere
 - **internal database**:
   - This is not about the CDR
   - Think: call forwarding settings
   - defaults to reading from `/var/lib/asterisk/astdb.sqlite3`

Why this and not an Asterisk API?

 - Can handle really old Asterisks that don't have the right APIs
 - Doesn't need to reload configuration to change settings
 - Pushes changes rather than having to pull them
 - Can be very reactive (instant notice for changes)

```
USAGE:
    fulliautomatisk [OPTIONS] <URL>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -d, --db <ASTDB>         Sets a custom location for the astdb file
    -f, --filter <FILTER>    Regex filter of keys to watch

ARGS:
    <URL>    URL to deliver payloads to (JSON via POST)
```

Payloads look like:

```js
{
    "instance": "UUID generated when program is started",
    "full_state": { // not always present
        "key": "value",
        ...
    },
    "changes": [
      {"Modified": ["key", "new value"]},
      {"Added": ["key", "value"]},
      {"Removed": "key"},
      ...
    ],
}
```

Also:

 - Will re-read the entire state from DB every 10 minutes or so, to catch
   anything that might have been missed.
 - You can trigger a re-read manually by sending the SIGUSR1 signal.
 - When it starts it sends a full state, which will also have a full changes array.

Needed to build on Ubuntu:

 - `libssl-dev`
 - `pkg-config`

#[macro_use]
extern crate clap;
#[macro_use]
extern crate lazy_static;
extern crate notify;
extern crate regex;
extern crate reqwest;
extern crate rusqlite;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;
extern crate signal;
extern crate time;
extern crate uuid;

use notify::{Op, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use rusqlite::{Connection, NO_PARAMS, OpenFlags};
use signal::{Signal::SIGUSR1, trap::Trap};
use std::collections::{BTreeMap, HashSet};
use std::sync::mpsc::channel;
use std::thread;
use std::time::{Duration, Instant};
use time::now;
use uuid::Uuid;

#[derive(Debug, Serialize)]
enum Change {
    Added(String, String),
    Removed(String),
    Modified(String, String),
}

type State = BTreeMap<String, String>;

fn read_state(conn: &Connection, filter: &Option<Regex>) -> rusqlite::Result<State> {
    let mut fetch = conn.prepare_cached("SELECT key, value FROM astdb")?;

    let iter = fetch.query_map(
        NO_PARAMS,
        |row| -> (String, String) { (row.get(0), row.get(1)) }
    )?;

    let mut state = BTreeMap::new();
    for ast in iter {
        let a = ast?;
        match filter {
            None => {},
            Some(r) => if !r.is_match(&a.0) {
                continue;
            }
        }

        state.insert(a.0, a.1);
    }

    Ok(state)
}

fn compare_state(initial: &State, current: &State) -> Vec<Change> {
    let mut changes = Vec::new();
    let mut seen = HashSet::new();

    for (key, ival) in initial.iter() {
        seen.insert(key);
        match current.get(key) {
            None => { changes.push(Change::Removed(key.clone())); },
            Some(cval) => if ival != cval {
                changes.push(Change::Modified(key.clone(), cval.clone()));
            },
        }
    }

    for (key, cval) in current.iter() {
        if !seen.contains(key) {
            changes.push(Change::Added(key.clone(), cval.clone()));
        }
    }

    changes
}

fn ts() -> String {
    format!("{}", now().strftime("%F %T.%f").unwrap())
}

fn id() -> &'static str {
    lazy_static! {
        static ref ID: String = format!("{}", Uuid::new_v4());
    }

    &ID
}

fn main() -> MainResult {
    let cli = clap_app!(fulliautomatisk =>
        (version: env!("CARGO_PKG_VERSION"))
        (author: env!("CARGO_PKG_AUTHORS"))
        (about: "Monitors an Asterisk internal database for changes")
        (@arg ASTDB: -d --db +takes_value "Sets a custom location for the astdb file")
        (@arg FILTER: -f --filter +takes_value "Regex filter of keys to watch")
        (@arg URL: +required "URL to deliver payloads to (JSON via POST)")
    ).get_matches();

    let url: String = cli.value_of("URL").unwrap().into();
    let path = cli.value_of("ASTDB").unwrap_or("/var/lib/asterisk/astdb.sqlite3");
    let filter = match cli.value_of("FILTER") {
        None => None,
        Some(f) => Some(Regex::new(f)?),
    };

    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    println!("{} [{}] Read-only connection opened to {}", ts(), id(), path);

    let mut state: State = BTreeMap::new(); //read_state(&conn, &filter)?;
    println!("{} [{}] Fetched initial state", ts(), id());

    // HTTP channel
    let (cutx, curx) = channel();

    // Fetch changes from DB
    let (dbtx, dbrx) = channel();
    let db = thread::spawn(move || {
        loop {
            let full = dbrx.recv().expect("Internal communication error");

            let new_state = match read_state(&conn, &filter) {
                Ok(s) => s,
                Err(e) => {
                    println!("Error reading state, ignoring\n{:?}", e);
                    continue;
                }
            };

            let diff = compare_state(&state, &new_state);
            if diff.len() > 0 {
                let mut a = 0;
                let mut r = 0;
                let mut m = 0;
                for dif in diff.iter() {
                    match dif {
                        Change::Added(_, _) => { a += 1; },
                        Change::Removed(_) => { r += 1; },
                        Change::Modified(_, _) => { m += 1; },
                    }
                }

                println!("{} [{}] Change detected: +{}, -{}, ~{}", ts(), id(), a, r, m);

                if full {
                    cutx.send(json!({
                        "instance": id(),
                        "full_state": new_state,
                        "changes": diff
                    })).expect("Internal communication error");
                } else {
                    cutx.send(json!({
                        "instance": id(),
                        "changes": diff
                    })).expect("Internal communication error");
                }
            }

            state = new_state;
        }
    });

    // Send changes through http
    let http = thread::spawn(move || {
        let client = reqwest::Client::builder()
            .gzip(false)
            .timeout(Duration::from_secs(5))
            .danger_accept_invalid_certs(true)
            .build().expect("HTTP client failed to initialise");

        loop {
            let json = curx.recv().expect("Internal communication error");
            match client.post(&url).body(format!("{}", json)).send() {
                Err(e) => println!("Failed to send HTTP notice: {:?}", e),
                Ok(_) => {},
            }
        }
    });

    let fsdbtx = dbtx.clone();
    let (fstx, fsrx) = channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(fstx)?;
    watcher.watch(path, RecursiveMode::NonRecursive)?; // it's a file anyway
    println!("{} [{}] Watching the database through filesystem", ts(), id());

    // Watch DB for changes
    let fs = thread::spawn(move || {
        loop {
            match fsrx.recv() {
                Ok(event) => {
                    if event.op.is_err() { continue; }
                    if !event.op.unwrap().contains(Op::WRITE) { continue; }
                    fsdbtx.send(false).expect("Internal communication error");
                },
                Err(e) => println!("{} [{}] Watcher error: {:?}", ts(), id(), e),
            }
        }
    });

    // Watch signal (USR1) for full-reload, and also do a full-reload ~every ten minutes
    let sigdbtx = dbtx.clone();
    let sig = thread::spawn(move || {
        loop {
            let timeout = Instant::now() + Duration::from_secs(600);
            Trap::trap(&[SIGUSR1]).wait(timeout);
            sigdbtx.send(true).expect("Internal communication error");
        }
    });

    db.join().unwrap();
    http.join().unwrap();
    fs.join().unwrap();
    sig.join().unwrap();
    Ok(())
}

type MainResult = Result<(), MainError>;

#[derive(Debug)]
enum MainError {
    Sql(rusqlite::Error),
    Notify(notify::Error),
    Regex(regex::Error),
}

impl From<notify::Error> for MainError {
    fn from(err: notify::Error) -> Self {
        MainError::Notify(err)
    }
}

impl From<regex::Error> for MainError {
    fn from(err: regex::Error) -> Self {
        MainError::Regex(err)
    }
}

impl From<rusqlite::Error> for MainError {
    fn from(err: rusqlite::Error) -> Self {
        MainError::Sql(err)
    }
}

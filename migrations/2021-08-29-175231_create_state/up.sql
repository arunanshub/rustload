-- Holds our metadata information. Any value must be inserted only once.
CREATE TABLE states (
    id INTEGER NOT NULL PRIMARY KEY CHECK (id = 1),
    version TEXT NOT NULL UNIQUE,
    time INTEGER NOT NULL UNIQUE
);

CREATE TABLE maps (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    seq INTEGER NOT NULL,
    update_time INTEGER NOT NULL,
    offset INTEGER NOT NULL,
    length INTEGER NOT NULL,
    uri TEXT NOT NULL
);

CREATE TABLE badexes (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    update_time INTEGER NOT NULL,
    uri TEXT NOT NULL
);

CREATE TABLE exes (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    seq INTEGER NOT NULL,
    update_time INTEGER NOT NULL,
    time INTEGER NOT NULL,
    uri TEXT NOT NULL
);

CREATE TABLE exemaps (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    seq INTEGER NOT NULL,
    map_seq INTEGER NOT NULL,
    prob REAL NOT NULL
);

CREATE TABLE markovstates (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    a_seq INTEGER NOT NULL,
    b_seq INTEGER NOT NULL,
    time INTEGER NOT NULL,
    time_to_leave BLOB NOT NULL, -- serialize as `msgpack`
    weight BLOB NOT NULL         -- serialize as `msgpack`
);

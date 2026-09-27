CREATE TABLE accounts (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    currency TEXT NOT NULL,
    kind TEXT NOT NULL,
    opening_balance TEXT NOT NULL,
    archived INTEGER NOT NULL,
    current_value TEXT
);

CREATE TABLE entries (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id),
    date TEXT NOT NULL,
    time TEXT,
    amount TEXT NOT NULL,
    currency TEXT NOT NULL,
    description TEXT NOT NULL,
    note TEXT,
    category TEXT,
    pot_id TEXT,
    transfer_account_id TEXT,
    source TEXT NOT NULL,
    bank_state TEXT NOT NULL,
    confirmed INTEGER NOT NULL,
    voided_reason TEXT
);

CREATE INDEX entries_account_id ON entries(account_id);

CREATE TABLE entry_tags (
    entry_id TEXT NOT NULL REFERENCES entries(id),
    tag TEXT NOT NULL,
    PRIMARY KEY (entry_id, tag)
);

CREATE TABLE entry_parts (
    id TEXT PRIMARY KEY,
    entry_id TEXT NOT NULL REFERENCES entries(id),
    amount TEXT NOT NULL,
    category TEXT,
    transfer_account_id TEXT
);

CREATE INDEX entry_parts_entry_id ON entry_parts(entry_id);

CREATE TABLE pots (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    currency TEXT NOT NULL,
    target TEXT,
    priority INTEGER
);

CREATE TABLE allocations (
    id TEXT PRIMARY KEY,
    pot_id TEXT NOT NULL REFERENCES pots(id),
    amount TEXT NOT NULL,
    date TEXT NOT NULL,
    note TEXT
);

CREATE INDEX allocations_pot_id ON allocations(pot_id);

CREATE TABLE valuations (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id),
    date TEXT NOT NULL,
    old_value TEXT NOT NULL,
    new_value TEXT NOT NULL,
    category TEXT NOT NULL
);

CREATE INDEX valuations_account_id ON valuations(account_id);

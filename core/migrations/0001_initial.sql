CREATE TABLE accounts (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    currency TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('own', 'outside', 'person', 'investment')),
    opening_balance NUMERIC NOT NULL,
    archived BOOLEAN NOT NULL,
    current_value NUMERIC
);

CREATE TABLE entries (
    id UUID PRIMARY KEY,
    account_id UUID NOT NULL REFERENCES accounts(id),
    date DATE NOT NULL,
    time TIME,
    amount NUMERIC NOT NULL,
    currency TEXT NOT NULL,
    description TEXT NOT NULL,
    note TEXT,
    category TEXT,
    pot_id UUID,
    transfer_account_id UUID,
    source TEXT NOT NULL CHECK (source IN ('manual', 'imported')),
    bank_state TEXT NOT NULL CHECK (bank_state IN ('completed', 'pending', 'reverted')),
    confirmed BOOLEAN NOT NULL,
    voided_reason TEXT
);

CREATE INDEX entries_account_id ON entries(account_id);

CREATE TABLE entry_tags (
    entry_id UUID NOT NULL REFERENCES entries(id),
    tag TEXT NOT NULL,
    PRIMARY KEY (entry_id, tag)
);

CREATE TABLE entry_parts (
    id UUID PRIMARY KEY,
    entry_id UUID NOT NULL REFERENCES entries(id),
    amount NUMERIC NOT NULL,
    category TEXT,
    transfer_account_id UUID
);

CREATE INDEX entry_parts_entry_id ON entry_parts(entry_id);

CREATE TABLE pots (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    currency TEXT NOT NULL,
    target NUMERIC,
    priority INTEGER
);

CREATE TABLE allocations (
    id UUID PRIMARY KEY,
    pot_id UUID NOT NULL REFERENCES pots(id),
    amount NUMERIC NOT NULL,
    date DATE NOT NULL,
    note TEXT
);

CREATE INDEX allocations_pot_id ON allocations(pot_id);

CREATE TABLE valuations (
    id UUID PRIMARY KEY,
    account_id UUID NOT NULL REFERENCES accounts(id),
    date DATE NOT NULL,
    old_value NUMERIC NOT NULL,
    new_value NUMERIC NOT NULL,
    category TEXT NOT NULL
);

CREATE INDEX valuations_account_id ON valuations(account_id);

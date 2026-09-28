CREATE TABLE imports (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id),
    currency TEXT NOT NULL,
    file_name TEXT NOT NULL,
    uploaded_at TEXT NOT NULL,
    rows_read INTEGER NOT NULL,
    opening_balance TEXT NOT NULL,
    closing_balance TEXT NOT NULL,
    completed INTEGER NOT NULL
);

CREATE INDEX imports_account_id ON imports(account_id);

CREATE TABLE import_queue_rows (
    id TEXT PRIMARY KEY,
    import_id TEXT NOT NULL REFERENCES imports(id),
    kind TEXT NOT NULL,
    date TEXT NOT NULL,
    time TEXT,
    amount TEXT NOT NULL,
    currency TEXT NOT NULL,
    description TEXT NOT NULL,
    bank_state TEXT NOT NULL,
    category TEXT
);

CREATE INDEX import_queue_rows_import_id ON import_queue_rows(import_id);

-- Exactly one of matched_entry_id and matched_queue_row_id is set.
CREATE TABLE import_queue_row_matches (
    id TEXT PRIMARY KEY,
    queue_row_id TEXT NOT NULL REFERENCES import_queue_rows(id),
    matched_entry_id TEXT REFERENCES entries(id),
    matched_queue_row_id TEXT REFERENCES import_queue_rows(id),
    CHECK ((matched_entry_id IS NULL) <> (matched_queue_row_id IS NULL))
);

CREATE INDEX import_queue_row_matches_queue_row_id ON import_queue_row_matches(queue_row_id);
CREATE INDEX import_queue_row_matches_matched_queue_row_id ON import_queue_row_matches(matched_queue_row_id);

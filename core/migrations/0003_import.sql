CREATE TABLE imports (
    id UUID PRIMARY KEY,
    account_id UUID NOT NULL REFERENCES accounts(id),
    currency TEXT NOT NULL,
    file_name TEXT NOT NULL,
    uploaded_at TIMESTAMPTZ NOT NULL,
    rows_read BIGINT NOT NULL,
    opening_balance NUMERIC NOT NULL,
    closing_balance NUMERIC NOT NULL,
    completed BOOLEAN NOT NULL
);

CREATE INDEX imports_account_id ON imports(account_id);

CREATE TABLE import_queue_rows (
    id UUID PRIMARY KEY,
    import_id UUID NOT NULL REFERENCES imports(id),
    kind TEXT NOT NULL CHECK (kind IN ('normal', 'reverted_candidate')),
    date DATE NOT NULL,
    time TIME,
    amount NUMERIC NOT NULL,
    currency TEXT NOT NULL,
    description TEXT NOT NULL,
    bank_state TEXT NOT NULL CHECK (bank_state IN ('completed', 'pending', 'reverted')),
    category TEXT
);

CREATE INDEX import_queue_rows_import_id ON import_queue_rows(import_id);

-- Exactly one of matched_entry_id and matched_queue_row_id is set.
CREATE TABLE import_queue_row_matches (
    id UUID PRIMARY KEY,
    queue_row_id UUID NOT NULL REFERENCES import_queue_rows(id),
    matched_entry_id UUID REFERENCES entries(id),
    matched_queue_row_id UUID REFERENCES import_queue_rows(id),
    CHECK ((matched_entry_id IS NULL) <> (matched_queue_row_id IS NULL))
);

CREATE INDEX import_queue_row_matches_queue_row_id ON import_queue_row_matches(queue_row_id);
CREATE INDEX import_queue_row_matches_matched_queue_row_id ON import_queue_row_matches(matched_queue_row_id);

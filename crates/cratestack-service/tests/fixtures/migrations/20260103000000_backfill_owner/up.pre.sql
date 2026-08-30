-- Backfill so the NOT NULL promotion in up.sql can succeed.
UPDATE widgets SET owner = 'unknown' WHERE owner IS NULL;

-- Why the system cancelled a league (too few entries at the start date). NULL for leagues an
-- admin cancelled and for every league that was not cancelled.
ALTER TABLE leagues ADD COLUMN cancel_reason text;

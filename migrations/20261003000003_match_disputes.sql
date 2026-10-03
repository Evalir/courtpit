-- Who disputed a reported score and why (the admin's decision goes in resolution_note).
ALTER TABLE matches
    ADD COLUMN disputed_by  uuid,
    ADD COLUMN dispute_note text;

-- Unanswered reports are auto-confirmed by a job; this finds overdue ones cheaply.
CREATE INDEX matches_confirm_deadline_idx ON matches (confirm_deadline_at)
    WHERE status = 'reported';

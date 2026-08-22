-- Custom Agent avatars remain local to the collaboration database. The
-- browser normalizes uploads before this value reaches the domain boundary;
-- Rust still validates the MIME, base64 payload, byte cap, and file signature.
ALTER TABLE actors ADD COLUMN avatar_data_url TEXT;

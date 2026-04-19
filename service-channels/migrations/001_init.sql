CREATE TABLE IF NOT EXISTS channels (
    id          UUID PRIMARY KEY,
    title       VARCHAR(255) NOT NULL UNIQUE,
    description VARCHAR(300),
    avatar_url  VARCHAR(500),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS ix_channels_title ON channels(title);

CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger WHERE tgname = 'trg_channels_updated_at'
    ) THEN
        CREATE TRIGGER trg_channels_updated_at
        BEFORE UPDATE ON channels
        FOR EACH ROW EXECUTE FUNCTION set_updated_at();
    END IF;
END;
$$;

CREATE TABLE IF NOT EXISTS channel_subscribers (
    id          UUID PRIMARY KEY,
    user_id     UUID NOT NULL,
    channel_id  UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    is_owner    BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_channel_subscribers_user_channel
    ON channel_subscribers(user_id, channel_id);
CREATE INDEX IF NOT EXISTS ix_channel_subscribers_user_id
    ON channel_subscribers(user_id);
CREATE INDEX IF NOT EXISTS ix_channel_subscribers_channel_id
    ON channel_subscribers(channel_id);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger WHERE tgname = 'trg_channel_subscribers_updated_at'
    ) THEN
        CREATE TRIGGER trg_channel_subscribers_updated_at
        BEFORE UPDATE ON channel_subscribers
        FOR EACH ROW EXECUTE FUNCTION set_updated_at();
    END IF;
END;
$$;

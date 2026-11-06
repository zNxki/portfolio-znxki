CREATE TABLE users (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    github_id bigint NOT NULL UNIQUE CHECK (github_id > 0),
    login text NOT NULL CHECK (char_length(login) BETWEEN 1 AND 39),
    display_name text,
    avatar_url text,
    role text NOT NULL DEFAULT 'USER' CHECK (role IN ('ADMIN', 'USER')),
    created_at timestamptz NOT NULL DEFAULT now(),
    last_login_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE sessions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash bytea NOT NULL UNIQUE,
    prev_token_hash bytea,
    created_at timestamptz NOT NULL DEFAULT now(),
    rotated_at timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    ip text,
    user_agent text
);

CREATE INDEX sessions_user_idx ON sessions (user_id);
CREATE INDEX sessions_prev_hash_idx ON sessions (prev_token_hash) WHERE prev_token_hash IS NOT NULL;
CREATE INDEX sessions_expiry_idx ON sessions (expires_at);

CREATE TABLE oauth_states (
    state_hash bytea PRIMARY KEY,
    code_verifier text NOT NULL,
    ip text,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL
);

CREATE INDEX oauth_states_expiry_idx ON oauth_states (expires_at);

CREATE TABLE audit_log (
    id bigserial PRIMARY KEY,
    at timestamptz NOT NULL DEFAULT now(),
    severity text NOT NULL DEFAULT 'info' CHECK (severity IN ('info', 'warning', 'security')),
    actor_id uuid,
    actor_login text,
    action text NOT NULL,
    entity text,
    entity_id text,
    ip text,
    details jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX audit_log_at_idx ON audit_log (at DESC, id DESC);
CREATE INDEX audit_log_action_idx ON audit_log (action text_pattern_ops);
CREATE INDEX audit_log_severity_idx ON audit_log (severity);

CREATE TABLE profile (
    id smallint PRIMARY KEY CHECK (id = 1),
    display_name text NOT NULL,
    name_parts jsonb NOT NULL,
    first_name text,
    last_name text,
    job_title jsonb NOT NULL DEFAULT '{}'::jsonb,
    headline jsonb NOT NULL DEFAULT '{}'::jsonb,
    bio_lead jsonb NOT NULL DEFAULT '{}'::jsonb,
    bio_paragraphs jsonb NOT NULL DEFAULT '{}'::jsonb,
    email text NOT NULL,
    availability text NOT NULL DEFAULT 'available' CHECK (availability IN ('available', 'limited', 'unavailable')),
    availability_text jsonb NOT NULL DEFAULT '{}'::jsonb,
    updated_at timestamptz NOT NULL DEFAULT now(),
    updated_by uuid
);

CREATE TABLE contacts (
    id bigserial PRIMARY KEY,
    icon text NOT NULL CHECK (icon IN ('email', 'telegram', 'github', 'discord')),
    label text NOT NULL,
    value text NOT NULL,
    href text NOT NULL,
    position integer NOT NULL DEFAULT 0,
    visible boolean NOT NULL DEFAULT true,
    updated_at timestamptz NOT NULL DEFAULT now(),
    updated_by uuid
);

CREATE UNIQUE INDEX contacts_single_email ON contacts (icon) WHERE icon = 'email';

CREATE TABLE technologies (
    id bigserial PRIMARY KEY,
    name text NOT NULL UNIQUE,
    marquee_label text,
    icon text,
    level integer NOT NULL CHECK (level BETWEEN 0 AND 100),
    description jsonb NOT NULL DEFAULT '{}'::jsonb,
    in_marquee boolean NOT NULL DEFAULT true,
    visible boolean NOT NULL DEFAULT true,
    position integer NOT NULL DEFAULT 0,
    updated_at timestamptz NOT NULL DEFAULT now(),
    updated_by uuid
);

CREATE TABLE experiences (
    id bigserial PRIMARY KEY,
    slug text NOT NULL UNIQUE,
    kind text NOT NULL CHECK (kind IN ('work', 'personal')),
    title jsonb NOT NULL,
    subtitle jsonb NOT NULL DEFAULT '{}'::jsonb,
    role jsonb NOT NULL DEFAULT '{}'::jsonb,
    organization text,
    description jsonb NOT NULL DEFAULT '{}'::jsonb,
    start_date date,
    end_date date,
    visible boolean NOT NULL DEFAULT true,
    position integer NOT NULL DEFAULT 0,
    updated_at timestamptz NOT NULL DEFAULT now(),
    updated_by uuid,
    CHECK (end_date IS NULL OR start_date IS NULL OR end_date >= start_date)
);

CREATE TABLE media (
    id uuid PRIMARY KEY,
    filename text NOT NULL UNIQUE,
    original_name text,
    mime text NOT NULL CHECK (mime IN ('image/png', 'image/jpeg', 'image/webp', 'image/gif')),
    size_bytes bigint NOT NULL CHECK (size_bytes > 0),
    sha256 text NOT NULL,
    uploaded_by uuid,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE projects (
    id bigserial PRIMARY KEY,
    slug text NOT NULL UNIQUE,
    name text,
    title jsonb NOT NULL,
    subtitle jsonb NOT NULL DEFAULT '{}'::jsonb,
    description jsonb NOT NULL DEFAULT '{}'::jsonb,
    year_label text,
    hue integer NOT NULL DEFAULT 200 CHECK (hue BETWEEN 0 AND 359),
    status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'completed', 'paused', 'archived')),
    github_url text,
    demo_url text,
    started_on date,
    ended_on date,
    visible boolean NOT NULL DEFAULT true,
    position integer NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    created_by uuid,
    updated_by uuid,
    CHECK (ended_on IS NULL OR started_on IS NULL OR ended_on >= started_on)
);

CREATE TABLE project_technologies (
    project_id bigint NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    technology_id bigint NOT NULL REFERENCES technologies (id) ON DELETE CASCADE,
    PRIMARY KEY (project_id, technology_id)
);

CREATE TABLE project_images (
    id bigserial PRIMARY KEY,
    project_id bigint NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    media_id uuid NOT NULL REFERENCES media (id) ON DELETE RESTRICT,
    alt jsonb NOT NULL DEFAULT '{}'::jsonb,
    position integer NOT NULL DEFAULT 0
);

CREATE INDEX project_images_project_idx ON project_images (project_id, position);
CREATE INDEX project_images_media_idx ON project_images (media_id);

CREATE TABLE site_settings (
    key text PRIMARY KEY,
    value jsonb NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    updated_by uuid
);

CREATE TABLE ui_texts (
    lang text NOT NULL CHECK (char_length(lang) = 2),
    key text NOT NULL,
    value text NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (lang, key)
);
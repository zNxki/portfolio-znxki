-- Esegui come superuser: sudo -u postgres psql -f deploy/db.sql
-- Sostituisci le due password PRIMA di eseguire (es. openssl rand -hex 24).

-- owner: esegue le migrazioni (usato solo all'avvio dal backend)
CREATE ROLE portfolio_owner LOGIN PASSWORD 'CAMBIA_PASSWORD_OWNER';
-- app: privilegi minimi (solo DML), usato a runtime
CREATE ROLE portfolio_app LOGIN PASSWORD 'CAMBIA_PASSWORD_APP';

CREATE DATABASE portfolio OWNER portfolio_owner;
\connect portfolio

REVOKE ALL ON DATABASE portfolio FROM PUBLIC;
GRANT CONNECT ON DATABASE portfolio TO portfolio_app;

REVOKE ALL ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA public TO portfolio_app;
GRANT USAGE, CREATE ON SCHEMA public TO portfolio_owner;

ALTER DEFAULT PRIVILEGES FOR ROLE portfolio_owner IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO portfolio_app;
ALTER DEFAULT PRIVILEGES FOR ROLE portfolio_owner IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO portfolio_app;

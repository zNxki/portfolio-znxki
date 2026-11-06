# Portfolio — guida di installazione su VPS

Un unico servizio Rust (Axum + PostgreSQL) serve il sito pubblico, l'API e il pannello admin (`/admin/`).
Il frontend non ha un processo separato: lo avvia il backend. Dopo il primo avvio **tutto** (profilo, progetti,
esperienze, tecnologie, contatti, testi, SEO) vive nel database e si modifica dal pannello.
I file `data/site.json` e `lang/*.json` servono solo come seed iniziale.

Esempi sotto: Ubuntu/Debian, dominio `example.com`, utente `portfolio`, cartella `/opt/portfolio`.

## 1. Dipendenze

```bash
sudo apt update
sudo apt install -y build-essential pkg-config git curl nginx certbot python3-certbot-nginx postgresql
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y   # serve Rust >= 1.89
source ~/.cargo/env && rustc --version
sudo adduser --system --group --home /opt/portfolio portfolio
```

## 2. PostgreSQL

Modifica le due password in `deploy/db.sql`, poi:

```bash
sudo -u postgres psql -f deploy/db.sql
```

Crea due ruoli: `portfolio_owner` (migrazioni) e `portfolio_app` (solo DML, usato a runtime).
PostgreSQL resta in ascolto solo su `localhost` (default): non aprire la porta 5432 nel firewall.

## 3. GitHub OAuth

1. GitHub → Settings → Developer settings → OAuth Apps → *New OAuth App*.
2. *Homepage URL*: `https://example.com` — *Authorization callback URL*: `https://example.com/auth/github/callback`.
3. Copia *Client ID* e genera un *Client Secret*.
4. Il tuo **ID numerico** GitHub: `curl -s https://api.github.com/users/IL_TUO_USERNAME | grep '"id"'`.

Solo gli ID in `ADMIN_GITHUB_IDS` possono entrare; chiunque altro viene rifiutato. Il Client Secret resta solo sul server.

## 4. Codice e configurazione

```bash
sudo git clone <URL-DEL-TUO-REPO> /opt/portfolio      # oppure copia i file con rsync
sudo mkdir -p /opt/portfolio/backend/uploads /etc/portfolio
sudo cp /opt/portfolio/backend/.env.example /etc/portfolio/portfolio.env
sudo nano /etc/portfolio/portfolio.env
```

Valori da impostare in `/etc/portfolio/portfolio.env`:

| Variabile | Valore |
|---|---|
| `PUBLIC_URL` | `https://example.com` |
| `DATABASE_URL` / `MIGRATION_DATABASE_URL` | con le password scelte al punto 2 |
| `GITHUB_CLIENT_ID` / `GITHUB_CLIENT_SECRET` | dal punto 3 |
| `ADMIN_GITHUB_IDS` | il tuo ID, es. `12345678` (più admin: separati da virgola) |
| `APP_SECRET` | `openssl rand -hex 32` |
| `FRONTEND_DIR` / `ADMIN_DIR` / `UPLOAD_DIR` | `/opt/portfolio` / `/opt/portfolio/backend/admin` / `/opt/portfolio/backend/uploads` |
| `TRUST_PROXY` | `true` (dietro Nginx) |

```bash
sudo chown -R portfolio:portfolio /opt/portfolio
sudo chown root:portfolio /etc/portfolio/portfolio.env && sudo chmod 640 /etc/portfolio/portfolio.env
```

## 5. Compilazione e avvio

```bash
cd /opt/portfolio/backend
sudo -u portfolio -H bash -c 'source ~/.cargo/env 2>/dev/null; cargo build --release'
```

Se `cargo` non è disponibile per l'utente `portfolio`, compila con il tuo utente e poi `sudo chown -R portfolio:portfolio /opt/portfolio`.

Servizio systemd (l'avvio applica le migrazioni e importa il seed alla prima esecuzione):

```bash
sudo cp /opt/portfolio/deploy/portfolio.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now portfolio
sudo systemctl status portfolio
journalctl -u portfolio -f          # log strutturati (JSON)
curl -s http://127.0.0.1:8080/healthz
```

## 6. Nginx e HTTPS

```bash
sudo cp /opt/portfolio/deploy/nginx.conf /etc/nginx/sites-available/portfolio
sudo sed -i 's/example.com/IL-TUO-DOMINIO/' /etc/nginx/sites-available/portfolio
sudo ln -s /etc/nginx/sites-available/portfolio /etc/nginx/sites-enabled/
sudo nginx -t && sudo systemctl reload nginx
sudo certbot --nginx -d IL-TUO-DOMINIO        # certificato + redirect HTTP→HTTPS, rinnovo automatico
sudo ufw allow 'Nginx Full' && sudo ufw allow OpenSSH && sudo ufw enable
```

Apri `https://IL-TUO-DOMINIO/admin/` e accedi con GitHub.

## 7. Aggiornare dopo un nuovo deploy

```bash
cd /opt/portfolio && sudo git pull
cd backend && sudo -u portfolio -H bash -c 'source ~/.cargo/env 2>/dev/null; cargo build --release'
sudo systemctl restart portfolio      # applica le nuove migrazioni
```

## Manutenzione

- **Backup**: `sudo -u postgres pg_dump portfolio > portfolio-$(date +%F).sql` e copia `/opt/portfolio/backend/uploads`.
- **Cambiare admin**: modifica `ADMIN_GITHUB_IDS` e riavvia; chi viene rimosso perde ruolo e sessioni.
- **Sessioni e audit**: dal pannello, sezioni *Sicurezza* e *Audit log*.
- **Test** (richiedono PostgreSQL con un ruolo che può creare database):
  `cd backend && TEST_DATABASE_URL=postgres://utente:pass@localhost/postgres cargo test`

## Problemi comuni

| Sintomo | Causa |
|---|---|
| Il servizio non parte: `ADMIN_GITHUB_IDS è obbligatorio` | Variabile vuota o non numerica |
| Login → `/admin/?e=auth` | ID GitHub non in allowlist, callback URL diverso da `PUBLIC_URL`, o cookie di stato scaduto (riprova) |
| Ogni modifica dà 403 | `PUBLIC_URL` diverso dall'indirizzo con cui apri il sito (controllo Origin/CSRF) |
| Rate limit sbagliato / IP sempre uguale | `TRUST_PROXY=true` mancante dietro Nginx |
| Upload rifiutato (413) | `client_max_body_size` in Nginx o `MAX_UPLOAD_BYTES` troppo bassi |

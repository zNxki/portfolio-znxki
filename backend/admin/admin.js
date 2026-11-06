"use strict";
(() => {
    const $ = (s, r = document) => r.querySelector(s);

    const h = (tag, props, ...kids) => {
        const el = document.createElement(tag);
        if (props) {
            for (const [k, v] of Object.entries(props)) {
                if (v === null || v === undefined || v === false) continue;
                if (k === "class") el.className = v;
                else if (k === "text") el.textContent = v;
                else if (k.startsWith("on") && typeof v === "function") el.addEventListener(k.slice(2), v);
                else if (v === true) el.setAttribute(k, "");
                else el.setAttribute(k, String(v));
            }
        }
        for (const c of kids.flat(Infinity)) {
            if (c === null || c === undefined || c === false) continue;
            el.append(c instanceof Node ? c : document.createTextNode(String(c)));
        }
        return el;
    };

    const clear = (el) => {
        while (el.firstChild) el.removeChild(el.firstChild);
        return el;
    };

    const fmtDate = (v) => {
        if (!v) return "—";
        const d = new Date(v);
        return Number.isNaN(d.getTime()) ? "—" : d.toLocaleString("it-IT");
    };

    const state = { me: null, csrf: "", general: null, techs: null };

    class ApiError extends Error {
        constructor(status, body) {
            const inner = body && body.error ? body.error : {};
            super(inner.message || "Errore " + status);
            this.status = status;
            this.code = inner.code || "";
            this.fields = Array.isArray(inner.fields) ? inner.fields : [];
        }
    }

    const api = async (method, path, body) => {
        const init = {
            method,
            credentials: "same-origin",
            headers: { Accept: "application/json" },
        };
        if (method !== "GET") init.headers["X-CSRF-Token"] = state.csrf;
        if (body instanceof FormData) {
            init.body = body;
        } else if (body !== undefined) {
            init.headers["Content-Type"] = "application/json";
            init.body = JSON.stringify(body);
        }
        let res;
        try {
            res = await fetch(path, init);
        } catch (e) {
            throw new ApiError(0, { error: { message: "Server non raggiungibile" } });
        }
        if (res.status === 204) return null;
        let data = null;
        try {
            data = await res.json();
        } catch (e) {
            data = null;
        }
        if (res.status === 401) {
            state.me = null;
            renderLogin();
            throw new ApiError(401, data);
        }
        if (!res.ok) throw new ApiError(res.status, data);
        return data;
    };

    const toast = (msg, kind) => {
        const t = h("div", { class: "toast " + (kind || ""), role: "status", text: msg });
        $("#toasts").append(t);
        setTimeout(() => t.remove(), kind === "err" ? 8000 : 3500);
    };

    const errText = (e) => {
        if (!(e instanceof ApiError)) return "Errore imprevisto";
        const lines = [e.message];
        for (const f of e.fields) lines.push("• " + f.field + ": " + f.message);
        return lines.join("\n");
    };

    const guard = async (fn) => {
        try {
            return await fn();
        } catch (e) {
            if (!(e instanceof ApiError && e.status === 401)) toast(errText(e), "err");
            return undefined;
        }
    };

    const dialog = (title, body, actions) =>
        new Promise((resolve) => {
            const dlg = h("dialog", null,
                h("div", { class: "dh" }, h("h3", { text: title }),
                    h("button", { class: "btn sm", type: "button", text: "✕", "aria-label": "Chiudi", onclick: () => dlg.close() })),
                body);
            if (actions) dlg.append(h("div", { class: "da" }, actions(dlg)));
            dlg.addEventListener("close", () => {
                dlg.remove();
                resolve(dlg.returnValue);
            });
            document.body.append(dlg);
            dlg.showModal();
        });

    const confirmBox = async (title, msg) => {
        const r = await dialog(title, h("p", { text: msg }), (d) => [
            h("button", { class: "btn", type: "button", text: "Annulla", onclick: () => d.close("no") }),
            h("button", { class: "btn dng", type: "button", text: "Conferma", onclick: () => d.close("yes") }),
        ]);
        return r === "yes";
    };

    const field = (label, control, hint) =>
        h("div", { class: "field" }, h("label", { text: label }), control, hint ? h("small", { class: "hint", text: hint }) : null);

    const text = (value, attrs) => {
        const i = h("input", Object.assign({ type: "text", autocomplete: "off" }, attrs || {}));
        i.value = value === null || value === undefined ? "" : String(value);
        return i;
    };

    const area = (value, rows, max) => {
        const t = h("textarea", { rows: rows || 4, maxlength: max });
        t.value = value || "";
        return t;
    };

    const select = (options, value) => {
        const s = h("select", null, options.map(([v, l]) => h("option", { value: v, text: l })));
        s.value = value;
        return s;
    };

    const check = (label, value) => {
        const i = h("input", { type: "checkbox" });
        i.checked = !!value;
        return { input: i, el: h("label", { class: "chk" }, i, label) };
    };

    const nul = (s) => {
        const v = s.trim();
        return v === "" ? null : v;
    };

    const activeLangs = () => (state.general ? state.general.languages : ["it", "en"]);

    const i18n = (label, value, opts) => {
        const o = opts || {};
        const val = value || {};
        const langs = activeLangs();
        const inputs = {};
        const rows = langs.map((l) => {
            const f = o.area ? area(val[l], o.rows || 3, o.max) : text(val[l], { maxlength: o.max });
            inputs[l] = f;
            return h("div", { class: "lrow" }, h("span", { class: "lang", text: l.toUpperCase() }), f);
        });
        const extra = {};
        for (const [l, v] of Object.entries(val)) if (!langs.includes(l)) extra[l] = v;
        return {
            el: field(label, rows, o.hint),
            get: () => {
                const out = Object.assign({}, extra);
                for (const l of langs) out[l] = inputs[l].value;
                return out;
            },
        };
    };

    const colorList = (label, values, max) => {
        const box = h("div", { class: "inl" });
        const items = [];
        const add = (v) => {
            const c = h("input", { type: "color" });
            c.value = /^#[0-9a-fA-F]{6}$/.test(v) ? v : "#9fd3ff";
            const w = h("span", { class: "inl" }, c,
                h("button", { class: "btn sm", type: "button", text: "✕", onclick: () => { items.splice(items.indexOf(c), 1); w.remove(); } }));
            items.push(c);
            box.insertBefore(w, addBtn);
        };
        const addBtn = h("button", { class: "btn sm", type: "button", text: "+", onclick: () => { if (items.length < (max || 8)) add("#9fd3ff"); } });
        box.append(addBtn);
        (values || []).forEach(add);
        return { el: field(label, box), get: () => items.map((c) => c.value) };
    };

    const submitBar = (label, onSave) => {
        const b = h("button", { class: "btn pri", type: "button", text: label || "Salva" });
        b.addEventListener("click", async () => {
            b.disabled = true;
            try {
                await onSave();
            } finally {
                b.disabled = false;
            }
        });
        return b;
    };

    const loadMedia = () => api("GET", "/api/admin/media");

    const uploadMedia = async (file) => {
        const fd = new FormData();
        fd.append("file", file);
        return api("POST", "/api/admin/media", fd);
    };

    const fileButton = (label, onDone) => {
        const input = h("input", { type: "file", accept: "image/png,image/jpeg,image/webp,image/gif", hidden: true });
        input.addEventListener("change", async () => {
            const f = input.files && input.files[0];
            input.value = "";
            if (!f) return;
            const m = await guard(() => uploadMedia(f));
            if (m) {
                toast("Immagine caricata", "ok");
                await onDone(m);
            }
        });
        return h("span", null, input, h("button", { class: "btn", type: "button", text: label, onclick: () => input.click() }));
    };

    const pickMedia = async () => {
        let chosen = null;
        const grid = h("div", { class: "mgrid" });
        const dlgRef = {};
        const fill = async () => {
            const items = (await guard(loadMedia)) || [];
            clear(grid);
            if (!items.length) grid.append(h("div", { class: "empty", text: "Nessuna immagine caricata." }));
            for (const m of items) {
                grid.append(h("div", {
                    class: "m pick", tabindex: "0", role: "button",
                    onclick: () => { chosen = m; dlgRef.d.close("ok"); },
                    onkeydown: (e) => { if (e.key === "Enter") { chosen = m; dlgRef.d.close("ok"); } },
                }, h("img", { src: m.url, alt: m.original_name || "", loading: "lazy" }), h("small", { text: m.original_name || m.filename })));
            }
        };
        const body = h("div", null,
            h("p", { class: "inl" }, fileButton("Carica nuova immagine", fill)),
            grid);
        const p = dialog("Scegli un'immagine", body, (d) => {
            dlgRef.d = d;
            return [h("button", { class: "btn", type: "button", text: "Annulla", onclick: () => d.close("no") })];
        });
        await fill();
        const r = await p;
        return r === "ok" ? chosen : null;
    };

    const app = () => $("#app");

    const logout = async () => {
        await guard(() => api("POST", "/auth/logout"));
        state.me = null;
        renderLogin();
    };

    function renderLogin(message) {
        const params = new URLSearchParams(location.search);
        const err = message || (params.get("e") ? "Accesso non riuscito o non autorizzato." : "");
        clear(app()).append(
            h("div", { class: "login" },
                h("div", { class: "card" },
                    h("h1", null, "Admin ", h("em", { text: "console" })),
                    h("p", { text: "Accesso riservato. L'autenticazione avviene esclusivamente tramite GitHub." }),
                    err ? h("div", { class: "err", text: err }) : null,
                    h("a", { class: "btn pri", href: "/auth/github/login", text: "Accedi con GitHub" }))));
    }

    const NAV = [
        ["dashboard", "Dashboard"],
        ["projects", "Progetti"],
        ["profile", "Informazioni"],
        ["contacts", "Contatti"],
        ["experiences", "Esperienze"],
        ["technologies", "Tecnologie"],
        ["media", "Media"],
        ["settings", "Impostazioni"],
        ["security", "Sicurezza"],
        ["audit", "Audit log"],
    ];

    const views = {};

    const parseHash = () => {
        const parts = location.hash.replace(/^#\/?/, "").split("/").filter(Boolean);
        return { name: parts[0] || "dashboard", arg: parts[1] || "" };
    };

    const go = (name, arg) => {
        location.hash = "#/" + name + (arg ? "/" + arg : "");
    };

    let shellRoot = null;
    let routeToken = 0;

    const renderShell = () => {
        const main = h("main", { class: "main", id: "main" });
        const nav = h("nav", { "aria-label": "Sezioni" }, NAV.map(([k, l]) => h("a", { href: "#/" + k, "data-k": k, text: l })));
        shellRoot = h("div", { class: "shell" },
            h("aside", { class: "side" },
                h("div", { class: "brand", text: "portfolio / admin" }),
                nav,
                h("div", { class: "who" },
                    h("span", null, "@" + state.me.login, " ", h("span", { class: "badge", text: state.me.role })),
                    h("a", { href: "/", target: "_blank", rel: "noopener", text: "Apri il sito ↗" }),
                    h("button", { class: "btn sm", type: "button", text: "Esci", onclick: logout }))),
            main);
        clear(app()).append(shellRoot);
        route();
    };

    const route = async () => {
        if (!state.me || !shellRoot) return;
        const { name, arg } = parseHash();
        const view = views[name] || views.dashboard;
        const key = views[name] ? name : "dashboard";
        shellRoot.querySelectorAll(".side nav a").forEach((a) => a.classList.toggle("on", a.dataset.k === key));
        const main = $("#main");
        const token = ++routeToken;
        clear(main).append(h("div", { class: "spin", text: "Caricamento…" }));
        try {
            if (!state.general) state.general = await api("GET", "/api/admin/settings/general");
            const out = h("div");
            await view(out, arg);
            if (token === routeToken) clear(main).append(out);
        } catch (e) {
            if (token !== routeToken) return;
            if (e instanceof ApiError && e.status === 401) return;
            clear(main).append(h("div", { class: "card" }, h("p", { text: errText(e) })));
        }
    };

    const pageHead = (title, ...acts) =>
        h("div", { class: "head" }, h("h1", { text: title }), h("div", { class: "acts" }, acts));

    const refresh = () => route();

    views.dashboard = async (root) => {
        const d = await api("GET", "/api/admin/dashboard");
        const labels = [
            ["projects_total", "Progetti"], ["projects_visible", "Progetti visibili"], ["experiences", "Esperienze"],
            ["technologies", "Tecnologie"], ["contacts", "Contatti"], ["media", "Immagini"],
            ["users", "Utenti"], ["admins", "Admin"], ["active_sessions", "Sessioni attive"],
        ];
        root.append(pageHead("Dashboard"),
            h("div", { class: "grid" }, labels.map(([k, l]) => h("div", { class: "stat" }, h("b", { text: String(d.stats[k]) }), h("span", { text: l })))),
            h("div", { class: "cols" },
                h("div", { class: "card" }, h("h3", { text: "Progetti modificati di recente" }),
                    d.recent_projects.length
                        ? h("table", null, h("tbody", null, d.recent_projects.map((p) => h("tr", null,
                            h("td", null, h("a", { href: "#/projects/" + p.id, text: p.title || p.slug })),
                            h("td", null, h("span", { class: "badge", text: p.status })),
                            h("td", null, h("span", { class: "badge " + (p.visible ? "ok" : ""), text: p.visible ? "visibile" : "nascosto" })),
                            h("td", { class: "muted", text: fmtDate(p.updated_at) })))))
                        : h("div", { class: "empty", text: "Nessun progetto." })),
                h("div", { class: "card" }, h("h3", { text: "Ultime modifiche ai contenuti" }),
                    d.recent_changes.length
                        ? h("table", null, h("tbody", null, d.recent_changes.map((a) => h("tr", null,
                            h("td", { text: a.action }),
                            h("td", { class: "muted", text: a.actor_login || "—" }),
                            h("td", { class: "muted", text: fmtDate(a.at) })))))
                        : h("div", { class: "empty", text: "Nessuna modifica registrata." }))));
    };

    const collEdit = async (root, cfg, arg) => {
        let row = null;
        if (arg !== "new") {
            const rows = await api("GET", cfg.path);
            row = rows.find((r) => String(r.id) === arg) || null;
            if (!row) throw new ApiError(404, { error: { message: "Elemento non trovato" } });
        }
        const form = await cfg.form(row);
        const save = submitBar(row ? "Salva" : "Crea", async () => {
            const body = form.get();
            const r = await guard(() => (row ? api("PUT", cfg.path + "/" + row.id, body) : api("POST", cfg.path, body)));
            if (r !== undefined) {
                toast("Salvato", "ok");
                go(cfg.key);
            }
        });
        root.append(
            pageHead((row ? "Modifica " : "Nuovo ") + cfg.single, h("a", { class: "btn", href: "#/" + cfg.key, text: "← Elenco" })),
            h("div", { class: "card" }, form.el, h("div", { class: "inl" }, save)));
    };

    const collection = (cfg) => async (root, arg) => {
        if (arg) return collEdit(root, cfg, arg);
        const rows = await api("GET", cfg.path);
        const move = async (i, dir) => {
            const ids = rows.map((r) => r.id);
            const j = i + dir;
            if (j < 0 || j >= ids.length) return;
            [ids[i], ids[j]] = [ids[j], ids[i]];
            if ((await guard(() => api("PUT", cfg.path + "/order", { ids }))) !== undefined) refresh();
        };
        const toggle = async (r) => {
            const body = Object.assign(cfg.toInput(r), { visible: !r.visible });
            if ((await guard(() => api("PUT", cfg.path + "/" + r.id, body))) !== undefined) refresh();
        };
        const del = async (r) => {
            if (!(await confirmBox("Elimina", "Eliminare «" + cfg.title_of(r) + "»? L'operazione non è reversibile."))) return;
            if ((await guard(() => api("DELETE", cfg.path + "/" + r.id))) !== undefined) {
                toast("Eliminato", "ok");
                refresh();
            }
        };
        const body = rows.length
            ? h("div", { class: "scroll" }, h("table", null,
                h("thead", null, h("tr", null, h("th", { text: "#" }), h("th", { text: cfg.single }), h("th", { text: cfg.meta_label || "" }), h("th", { text: "Stato" }), h("th"))),
                h("tbody", null, rows.map((r, i) => h("tr", null,
                    h("td", { class: "muted", text: String(i + 1) }),
                    h("td", null, h("a", { href: "#/" + cfg.key + "/" + r.id, text: cfg.title_of(r) })),
                    h("td", { class: "muted", text: cfg.meta_of(r) }),
                    h("td", null, h("button", { class: "badge " + (r.visible ? "ok" : ""), type: "button", text: r.visible ? "visibile" : "nascosto", onclick: () => toggle(r) })),
                    h("td", { class: "acts" },
                        h("button", { class: "btn sm", type: "button", text: "↑", "aria-label": "Sposta su", disabled: i === 0, onclick: () => move(i, -1) }),
                        h("button", { class: "btn sm", type: "button", text: "↓", "aria-label": "Sposta giù", disabled: i === rows.length - 1, onclick: () => move(i, 1) }),
                        h("a", { class: "btn sm", href: "#/" + cfg.key + "/" + r.id, text: "Modifica" }),
                        h("button", { class: "btn sm dng", type: "button", text: "Elimina", onclick: () => del(r) }))))))
            )
            : h("div", { class: "empty", text: "Nessun elemento." });
        root.append(pageHead(cfg.title, h("a", { class: "btn pri", href: "#/" + cfg.key + "/new", text: "+ Nuovo" })), h("div", { class: "card" }, body));
    };

    const pick1 = (map) => {
        const d = (state.general && state.general.default_lang) || "it";
        if (!map) return "";
        return map[d] || Object.values(map).find((v) => v) || "";
    };

    const dateField = (label, v) => {
        const i = h("input", { type: "date" });
        i.value = v || "";
        return { input: i, el: field(label, i) };
    };

    const contactForm = (row) => {
        const r = row || { icon: "github", label: "", value: "", href: "", visible: true };
        const icon = select([["email", "Email"], ["telegram", "Telegram"], ["github", "GitHub"], ["discord", "Discord"]], r.icon);
        const label = text(r.label, { maxlength: 40 });
        const value = text(r.value, { maxlength: 120 });
        const href = text(r.href, { maxlength: 2048 });
        const vis = check("Visibile nel sito", r.visible);
        const note = h("small", { class: "hint" });
        const sync = () => {
            const em = icon.value === "email";
            value.disabled = em;
            href.disabled = em;
            vis.input.disabled = em;
            note.textContent = em ? "Per l'email, valore e link derivano dall'indirizzo in «Informazioni»; è sempre visibile." : "Schemi ammessi per il link: https, http, mailto, tel.";
        };
        icon.addEventListener("change", sync);
        sync();
        return {
            el: h("div", null, field("Icona", icon, "Il frontend supporta queste quattro icone."), field("Etichetta", label),
                field("Valore mostrato", value), field("Link", href), note, h("div", { class: "field" }, vis.el)),
            get: () => ({ icon: icon.value, label: label.value, value: value.value, href: href.value, visible: vis.input.checked }),
        };
    };

    const techForm = (row) => {
        const r = row || { name: "", marquee_label: "", icon: "", level: 70, description: {}, in_marquee: true, visible: true };
        const name = text(r.name, { maxlength: 40 });
        const mq = text(r.marquee_label, { maxlength: 40 });
        const icon = text(r.icon, { maxlength: 40 });
        const level = h("input", { type: "range", min: "0", max: "100" });
        level.value = String(r.level);
        const out = h("b", { text: level.value + "%" });
        level.addEventListener("input", () => { out.textContent = level.value + "%"; });
        const desc = i18n("Descrizione", r.description, { area: true, rows: 2, max: 300 });
        const inM = check("Mostra nel marquee", r.in_marquee);
        const vis = check("Visibile nel sito", r.visible);
        return {
            el: h("div", null,
                h("div", { class: "row2" }, field("Nome", name), field("Etichetta marquee", mq, "Se vuota usa il nome."), field("Icona", icon, "Identificativo testuale dell'icona.")),
                field("Livello", h("div", { class: "inl" }, level, out)), desc.el,
                h("div", { class: "inl" }, inM.el, vis.el)),
            get: () => ({
                name: name.value, marquee_label: nul(mq.value), icon: nul(icon.value), level: Number(level.value),
                description: desc.get(), in_marquee: inM.input.checked, visible: vis.input.checked,
            }),
        };
    };

    const expForm = (row) => {
        const r = row || { slug: "", kind: "work", title: {}, subtitle: {}, role: {}, organization: "", description: {}, start_date: null, end_date: null, visible: true };
        const slug = text(r.slug, { maxlength: 49 });
        const kind = select([["work", "Lavorativa"], ["personal", "Personale"]], r.kind);
        const title = i18n("Titolo", r.title, { max: 120 });
        const sub = i18n("Sottotitolo", r.subtitle, { max: 200, hint: "Sono ammessi i tag <b>, <i>, <em>, <strong>, <br>." });
        const role = i18n("Ruolo", r.role, { max: 120 });
        const org = text(r.organization, { maxlength: 120 });
        const desc = i18n("Descrizione", r.description, { area: true, max: 2000 });
        const sd = dateField("Inizio", r.start_date);
        const ed = dateField("Fine", r.end_date);
        const vis = check("Visibile nel sito", r.visible);
        return {
            el: h("div", null,
                h("div", { class: "row2" }, field("Slug", slug, "a-z, 0-9, - e _"), field("Tipo", kind)),
                title.el, sub.el, role.el, field("Organizzazione", org), desc.el,
                h("div", { class: "row2" }, sd.el, ed.el), h("div", { class: "field" }, vis.el)),
            get: () => ({
                slug: slug.value, kind: kind.value, title: title.get(), subtitle: sub.get(), role: role.get(),
                organization: nul(org.value), description: desc.get(), start_date: nul(sd.input.value), end_date: nul(ed.input.value),
                visible: vis.input.checked,
            }),
        };
    };

    const gallery = (images) => {
        const items = (images || []).map((i) => ({ media_id: i.media_id, url: i.url, alt: i18n("Testo alternativo", i.alt, { max: 200 }) }));
        const box = h("div", { class: "gal" });
        const draw = () => {
            clear(box);
            if (!items.length) box.append(h("small", { class: "hint", text: "Nessuna immagine nella galleria." }));
            items.forEach((it, i) => {
                const mv = (d) => {
                    const j = i + d;
                    if (j < 0 || j >= items.length) return;
                    [items[i], items[j]] = [items[j], items[i]];
                    draw();
                };
                box.append(h("div", { class: "it" }, h("img", { src: it.url, alt: "" }), it.alt.el,
                    h("div", { class: "acts inl" },
                        h("button", { class: "btn sm", type: "button", text: "↑", onclick: () => mv(-1) }),
                        h("button", { class: "btn sm", type: "button", text: "↓", onclick: () => mv(1) }),
                        h("button", { class: "btn sm dng", type: "button", text: "✕", onclick: () => { items.splice(i, 1); draw(); } }))));
            });
        };
        draw();
        const addBtn = h("button", {
            class: "btn", type: "button", text: "+ Aggiungi immagine",
            onclick: async () => {
                const m = await pickMedia();
                if (!m) return;
                items.push({ media_id: m.id, url: m.url, alt: i18n("Testo alternativo", {}, { max: 200 }) });
                draw();
            },
        });
        return {
            el: field("Galleria", h("div", null, box, h("p", null, addBtn)), "La prima immagine appare come anteprima al passaggio sul progetto."),
            get: () => items.map((it) => ({ media_id: it.media_id, alt: it.alt.get() })),
        };
    };

    const projectForm = async (row) => {
        const techs = await api("GET", "/api/admin/technologies");
        const r = row || {
            slug: "", name: "", title: {}, subtitle: {}, description: {}, year_label: "", hue: 200, status: "active",
            github_url: "", demo_url: "", started_on: null, ended_on: null, visible: true, technology_ids: [], images: [],
        };
        const slug = text(r.slug, { maxlength: 49 });
        const name = text(r.name, { maxlength: 80 });
        const title = i18n("Titolo", r.title, { max: 120 });
        const sub = i18n("Sottotitolo", r.subtitle, { max: 200, hint: "Sono ammessi i tag <b>, <i>, <em>, <strong>, <br>." });
        const desc = i18n("Descrizione", r.description, { area: true, rows: 5, max: 4000 });
        const year = text(r.year_label, { maxlength: 30 });
        const hue = h("input", { type: "range", min: "0", max: "359" });
        hue.value = String(r.hue);
        const sw = h("span", { class: "swatch" });
        const paint = () => { sw.style.background = "hsl(" + hue.value + " 70% 55%)"; };
        hue.addEventListener("input", paint);
        paint();
        const status = select([["active", "In corso"], ["completed", "Completato"], ["paused", "In pausa"], ["archived", "Archiviato"]], r.status);
        const gh = text(r.github_url || "", { maxlength: 2048, placeholder: "https://github.com/…" });
        const demo = text(r.demo_url || "", { maxlength: 2048, placeholder: "https://…" });
        const sd = dateField("Inizio", r.started_on);
        const ed = dateField("Fine", r.ended_on);
        const vis = check("Visibile nel sito", r.visible);
        const sel = new Set(r.technology_ids);
        const techBoxes = techs.map((t) => {
            const c = check(t.name, sel.has(t.id));
            return { id: t.id, c };
        });
        const gal = gallery(r.images);
        return {
            el: h("div", null,
                h("div", { class: "row2" }, field("Slug", slug, "a-z, 0-9, - e _"), field("Nome (opzionale)", name, "Sostituisce il titolo come nome mostrato nell'elenco.")),
                title.el, sub.el, desc.el,
                h("div", { class: "row2" }, field("Periodo mostrato", year, "Es. «10/2026 -». Se vuoto è calcolato dalle date."), field("Stato", status)),
                h("div", { class: "row2" }, field("Link GitHub", gh), field("Link demo", demo)),
                h("div", { class: "row2" }, sd.el, ed.el),
                field("Tonalità (colore dell'anteprima)", h("div", { class: "inl" }, hue, sw)),
                field("Tecnologie", h("div", { class: "checks" }, techBoxes.length ? techBoxes.map((b) => b.c.el) : h("small", { class: "hint", text: "Crea prima delle tecnologie." }))),
                gal.el, h("div", { class: "field" }, vis.el)),
            get: () => ({
                slug: slug.value, name: nul(name.value), title: title.get(), subtitle: sub.get(), description: desc.get(),
                year_label: nul(year.value), hue: Number(hue.value), status: status.value, github_url: nul(gh.value), demo_url: nul(demo.value),
                started_on: nul(sd.input.value), ended_on: nul(ed.input.value), visible: vis.input.checked,
                technology_ids: techBoxes.filter((b) => b.c.input.checked).map((b) => b.id), images: gal.get(),
            }),
        };
    };

    const imagesIn = (r) => (r.images || []).map((i) => ({ media_id: i.media_id, alt: i.alt || {} }));

    views.projects = collection({
        key: "projects", path: "/api/admin/projects", title: "Progetti", single: "Progetto", meta_label: "Stato",
        title_of: (r) => r.name || pick1(r.title) || r.slug,
        meta_of: (r) => r.status + " · " + (r.images || []).length + " img",
        form: projectForm,
        toInput: (r) => ({
            slug: r.slug, name: r.name, title: r.title, subtitle: r.subtitle, description: r.description, year_label: r.year_label,
            hue: r.hue, status: r.status, github_url: r.github_url, demo_url: r.demo_url, started_on: r.started_on, ended_on: r.ended_on,
            visible: r.visible, technology_ids: r.technology_ids, images: imagesIn(r),
        }),
    });

    views.contacts = collection({
        key: "contacts", path: "/api/admin/contacts", title: "Contatti", single: "Contatto", meta_label: "Valore",
        title_of: (r) => r.label, meta_of: (r) => r.value, form: contactForm,
        toInput: (r) => ({ icon: r.icon, label: r.label, value: r.value, href: r.href, visible: r.visible }),
    });

    views.technologies = collection({
        key: "technologies", path: "/api/admin/technologies", title: "Tecnologie", single: "Tecnologia", meta_label: "Livello",
        title_of: (r) => r.name, meta_of: (r) => r.level + "%" + (r.in_marquee ? " · marquee" : ""), form: techForm,
        toInput: (r) => ({
            name: r.name, marquee_label: r.marquee_label, icon: r.icon, level: r.level, description: r.description,
            in_marquee: r.in_marquee, visible: r.visible,
        }),
    });

    views.experiences = collection({
        key: "experiences", path: "/api/admin/experiences", title: "Esperienze", single: "Esperienza", meta_label: "Tipo",
        title_of: (r) => pick1(r.title) || r.slug, meta_of: (r) => (r.kind === "work" ? "lavorativa" : "personale"), form: expForm,
        toInput: (r) => ({
            slug: r.slug, kind: r.kind, title: r.title, subtitle: r.subtitle, role: r.role, organization: r.organization,
            description: r.description, start_date: r.start_date, end_date: r.end_date, visible: r.visible,
        }),
    });

    views.profile = async (root) => {
        const p = await api("GET", "/api/admin/profile");
        const langs = activeLangs();
        const dn = text(p.display_name, { maxlength: 40 });
        const parts = [0, 1, 2].map((i) => text(p.name_parts[i] || "", { maxlength: 40 }));
        const ok = h("small", { class: "hint" });
        const checkParts = () => {
            const same = parts.map((x) => x.value).join("") === dn.value.trim();
            ok.textContent = same ? "✓ Le tre parti coincidono con il nome." : "✗ Le tre parti unite devono coincidere con il nome visualizzato.";
        };
        [dn, ...parts].forEach((i) => i.addEventListener("input", checkParts));
        checkParts();
        const fn = text(p.first_name || "", { maxlength: 80 });
        const ln = text(p.last_name || "", { maxlength: 80 });
        const job = i18n("Titolo professionale", p.job_title, { max: 120 });
        const head = i18n("Frase di presentazione (hero)", p.headline, { area: true, rows: 3, max: 800, hint: "Tag ammessi: <b>, <i>, <em>, <strong>, <br>." });
        const lead = i18n("Introduzione (about)", p.bio_lead, { area: true, rows: 2, max: 500 });
        const email = text(p.email, { type: "email", maxlength: 254 });
        const avail = select([["available", "Disponibile"], ["limited", "Disponibilità limitata"], ["unavailable", "Non disponibile"]], p.availability);
        const availText = i18n("Testo disponibilità", p.availability_text, { max: 200 });

        const paraBoxes = {};
        const extraParas = {};
        for (const [l, v] of Object.entries(p.bio_paragraphs || {})) if (!langs.includes(l)) extraParas[l] = v;
        const bioEls = langs.map((l) => {
            const list = h("div");
            const areas = [];
            const add = (v) => {
                const t = area(v, 3, 1500);
                const w = h("div", { class: "lrow" }, h("span", { class: "lang", text: l.toUpperCase() }), h("div", null, t,
                    h("button", { class: "btn sm", type: "button", text: "Rimuovi", onclick: () => { areas.splice(areas.indexOf(t), 1); w.remove(); } })));
                areas.push(t);
                list.append(w);
            };
            ((p.bio_paragraphs && p.bio_paragraphs[l]) || []).forEach(add);
            paraBoxes[l] = areas;
            return h("div", null, list, h("button", { class: "btn sm", type: "button", text: "+ Paragrafo " + l.toUpperCase(), onclick: () => { if (areas.length < 12) add(""); } }));
        });

        const save = submitBar("Salva informazioni", async () => {
            const bio = Object.assign({}, extraParas);
            for (const l of langs) bio[l] = paraBoxes[l].map((t) => t.value);
            const body = {
                display_name: dn.value, name_parts: parts.map((x) => x.value), first_name: nul(fn.value), last_name: nul(ln.value),
                job_title: job.get(), headline: head.get(), bio_lead: lead.get(), bio_paragraphs: bio, email: email.value,
                availability: avail.value, availability_text: availText.get(),
            };
            if ((await guard(() => api("PUT", "/api/admin/profile", body))) !== undefined) toast("Informazioni salvate", "ok");
        });

        root.append(pageHead("Informazioni personali"),
            h("div", { class: "card" }, h("h3", { text: "Nome" }),
                h("div", { class: "row2" }, field("Nome visualizzato", dn), field("Nome", fn), field("Cognome", ln)),
                field("Nome diviso in 3 parti (animazione hero)", h("div", { class: "inl" }, parts), "Es. «z» + «Nxk» + «i_». Unite devono dare il nome visualizzato."), ok, job.el),
            h("div", { class: "card" }, h("h3", { text: "Presentazione" }), head.el, lead.el,
                field("Biografia (paragrafi)", h("div", null, bioEls))),
            h("div", { class: "card" }, h("h3", { text: "Contatto e disponibilità" }),
                field("Email", email, "Usata anche dal contatto «Email» e dal frontend."), field("Disponibilità", avail), availText.el),
            h("div", { class: "inl" }, save));
    };

    views.media = async (root) => {
        const items = await api("GET", "/api/admin/media");
        const del = async (m) => {
            if (!(await confirmBox("Elimina immagine", "Eliminare «" + (m.original_name || m.filename) + "»? Non può essere in uso."))) return;
            if ((await guard(() => api("DELETE", "/api/admin/media/" + m.id))) !== undefined) {
                toast("Immagine eliminata", "ok");
                refresh();
            }
        };
        root.append(pageHead("Media", fileButton("+ Carica immagine", refresh)),
            h("p", { class: "muted", text: "Formati: PNG, JPEG, WebP, GIF. Il tipo è verificato dal contenuto del file." }),
            items.length
                ? h("div", { class: "mgrid" }, items.map((m) => h("div", { class: "m" },
                    h("img", { src: m.url, alt: m.original_name || "", loading: "lazy" }),
                    h("small", { text: m.original_name || m.filename }),
                    h("small", { text: Math.max(1, Math.round(m.size_bytes / 1024)) + " KB · " + fmtDate(m.created_at) }),
                    h("button", { class: "btn sm dng", type: "button", text: "Elimina", onclick: () => del(m) }))))
                : h("div", { class: "card empty", text: "Nessuna immagine caricata." }));
    };

    const rowsEditor = (label, items, cols, hint) => {
        const box = h("div");
        const rows = [];
        const add = (it) => {
            const inputs = cols.map((c) => {
                const i = text(it[c.key], { placeholder: c.ph, type: c.type || "text" });
                if (c.type === "number") i.setAttribute("type", "number");
                return i;
            });
            const rec = { inputs };
            const w = h("div", { class: "inl" }, inputs,
                h("button", { class: "btn sm", type: "button", text: "✕", onclick: () => { rows.splice(rows.indexOf(rec), 1); w.remove(); } }));
            rows.push(rec);
            box.append(w);
        };
        items.forEach(add);
        const empty = {};
        cols.forEach((c) => { empty[c.key] = ""; });
        return {
            el: field(label, h("div", null, box, h("p", null, h("button", { class: "btn sm", type: "button", text: "+ Aggiungi", onclick: () => add(empty) }))), hint),
            get: () => rows.map((r) => Object.fromEntries(cols.map((c, i) => [c.key, c.type === "number" ? Number(r.inputs[i].value) : r.inputs[i].value]))),
        };
    };

    const generalTab = async (root) => {
        const g = await api("GET", "/api/admin/settings/general");
        const langsIn = text(g.languages.join(", "), { maxlength: 60 });
        const defLang = text(g.default_lang, { maxlength: 2 });
        const b = g.boot;
        const bv = text(b.version), bb = text(b.build), bk = text(b.kernel), bu = text(b.user), bc = text(b.command);
        const cpu = text(b.cpu, { type: "number" }); cpu.setAttribute("type", "number");
        const ram = text(b.ram, { type: "number" }); ram.setAttribute("type", "number");
        const kinds = text(b.kinds.join(" "));
        const levels = rowsEditor("Livelli di competenza", g.levels, [{ key: "min", ph: "soglia min (0-100)", type: "number" }, { key: "key", ph: "chiave (es. advanced)" }],
            "Ogni chiave richiede il testo «levels.<chiave>» nei Testi dell'interfaccia. Serve un livello con soglia 0.");
        const dark = colorList("Colori accento — tema scuro", g.accents.dark);
        const light = colorList("Colori accento — tema chiaro", g.accents.light);
        const sections = rowsEditor("Sezioni raggiungibili dalla console (comando goto)",
            Object.entries(g.sections).map(([key, value]) => ({ key, value })), [{ key: "key", ph: "nome (es. work)" }, { key: "value", ph: "selettore (es. #work)" }]);
        const save = submitBar("Salva impostazioni generali", async () => {
            const secs = {};
            for (const s of sections.get()) secs[s.key] = s.value;
            const body = {
                default_lang: defLang.value.trim().toLowerCase(),
                languages: langsIn.value.split(/[\s,]+/).filter(Boolean).map((x) => x.toLowerCase()),
                boot: {
                    version: bv.value, build: bb.value, kernel: bk.value, cpu: Number(cpu.value), ram: Number(ram.value),
                    user: bu.value, command: bc.value, kinds: kinds.value.split(/[\s,]+/).filter(Boolean),
                },
                levels: levels.get(), accents: { dark: dark.get(), light: light.get() }, sections: secs,
            };
            const r = await guard(() => api("PUT", "/api/admin/settings/general", body));
            if (r) {
                state.general = r;
                toast("Impostazioni salvate", "ok");
            }
        });
        root.append(
            h("div", { class: "card" }, h("h3", { text: "Lingue" }),
                h("div", { class: "row2" }, field("Lingue attive", langsIn, "Codici a 2 lettere separati da virgola (max 8). Dopo aver aggiunto una lingua, copia i testi dalla scheda «Testi interfaccia»."),
                    field("Lingua predefinita", defLang))),
            h("div", { class: "card" }, h("h3", { text: "Sequenza di avvio" }),
                h("div", { class: "row2" }, field("Versione", bv), field("Build", bb), field("Kernel", bk), field("Utente", bu), field("Comando", bc), field("CPU", cpu), field("RAM (MB)", ram)),
                field("Tipi di riga", kinds, "i = info, o = ok, l = barra di caricamento. Una voce per riga di «boot.lines».")),
            h("div", { class: "card" }, levels.el, dark.el, light.el, sections.el),
            h("div", { class: "inl" }, save));
    };

    const seoTab = async (root) => {
        const s = await api("GET", "/api/admin/settings/seo");
        const media = await api("GET", "/api/admin/media");
        let ogId = s.og_image_media || null;
        const ogInfo = h("small", { class: "hint" });
        const ogImg = h("img", { alt: "", hidden: true });
        const showOg = () => {
            const m = media.find((x) => x.id === ogId);
            ogInfo.textContent = m ? (m.original_name || m.filename) : "Nessuna immagine selezionata";
            if (m) { ogImg.setAttribute("src", m.url); ogImg.removeAttribute("hidden"); } else { ogImg.setAttribute("hidden", ""); }
        };
        showOg();
        const pickOg = async () => {
            const m = await pickMedia();
            if (m) {
                if (!media.find((x) => x.id === m.id)) media.push(m);
                ogId = m.id;
                showOg();
            }
        };
        const title = i18n("Titolo del sito", s.site_title, { max: 70 });
        const desc = i18n("Descrizione del sito", s.site_description, { area: true, rows: 2, max: 320 });
        const ogt = i18n("Titolo Open Graph", s.og_title, { max: 90, hint: "Se vuoto usa il titolo del sito." });
        const ogd = i18n("Descrizione Open Graph", s.og_description, { area: true, rows: 2, max: 300, hint: "Se vuota usa la descrizione del sito." });
        const kw = i18n("Parole chiave", s.keywords, { max: 300 });
        const color = h("input", { type: "color" });
        color.value = /^#[0-9a-fA-F]{6}$/.test(s.theme_color || "") ? s.theme_color : "#0a0a0b";
        const noColor = check("Nessun colore tema", !s.theme_color);
        const robots = check("Consenti l'indicizzazione dai motori di ricerca", s.robots_index);
        const save = submitBar("Salva SEO", async () => {
            const body = {
                site_title: title.get(), site_description: desc.get(), og_title: ogt.get(), og_description: ogd.get(), keywords: kw.get(),
                og_image_media: ogId, theme_color: noColor.input.checked ? null : color.value, robots_index: robots.input.checked,
            };
            if ((await guard(() => api("PUT", "/api/admin/settings/seo", body))) !== undefined) toast("SEO salvata", "ok");
        });
        root.append(
            h("div", { class: "card" }, h("h3", { text: "Metadati" }), title.el, desc.el, kw.el,
                field("Colore tema del browser", h("div", { class: "inl" }, color, noColor.el)), h("div", { class: "field" }, robots.el)),
            h("div", { class: "card" }, h("h3", { text: "Open Graph" }), ogt.el, ogd.el,
                field("Immagine di anteprima", h("div", { class: "gal" }, h("div", { class: "it" }, ogImg, ogInfo,
                    h("div", { class: "acts inl" },
                        h("button", { class: "btn sm", type: "button", text: "Scegli", onclick: pickOg }),
                        h("button", { class: "btn sm", type: "button", text: "Rimuovi", onclick: () => { ogId = null; showOg(); } })))))),
            h("div", { class: "inl" }, save));
    };

    const textsTab = async (root) => {
        const langs = activeLangs();
        const sel = select(langs.map((l) => [l, l.toUpperCase()]), state.general.default_lang);
        const filter = h("input", { type: "search", placeholder: "Filtra per chiave o testo…" });
        const holder = h("div");
        let current = [];
        let original = new Map();
        const draw = () => {
            const q = filter.value.trim().toLowerCase();
            clear(holder);
            const rows = current.filter((r) => !q || r.key.toLowerCase().includes(q) || r.input.value.toLowerCase().includes(q));
            holder.append(rows.length
                ? h("div", { class: "scroll" }, h("table", null, h("tbody", null, rows.map((r) => h("tr", null, h("td", { class: "muted", text: r.key }), h("td", null, r.input))))))
                : h("div", { class: "empty", text: "Nessuna voce." }));
        };
        const load = async () => {
            const d = await guard(() => api("GET", "/api/admin/settings/ui-texts?lang=" + encodeURIComponent(sel.value)));
            if (!d) return;
            original = new Map(d.items.map((i) => [i.key, i.value]));
            current = d.items.map((i) => {
                const long = i.value.length > 60;
                const input = long ? area(i.value, 3, 2000) : text(i.value, { maxlength: 2000 });
                return { key: i.key, input };
            });
            draw();
        };
        sel.addEventListener("change", load);
        filter.addEventListener("input", draw);
        const save = submitBar("Salva modifiche", async () => {
            const items = current.filter((r) => r.input.value !== original.get(r.key)).map((r) => ({ key: r.key, value: r.input.value }));
            if (!items.length) { toast("Nessuna modifica da salvare"); return; }
            if ((await guard(() => api("PUT", "/api/admin/settings/ui-texts", { lang: sel.value, items }))) !== undefined) {
                toast(items.length + " testi salvati", "ok");
                await load();
            }
        });
        const copyFrom = select(langs.map((l) => [l, l.toUpperCase()]), langs[0]);
        const copy = h("button", {
            class: "btn", type: "button", text: "Copia le voci mancanti",
            onclick: async () => {
                if (copyFrom.value === sel.value) { toast("Scegli una lingua di origine diversa", "err"); return; }
                const r = await guard(() => api("POST", "/api/admin/settings/ui-texts/copy", { from: copyFrom.value, to: sel.value }));
                if (r) { toast(r.copied + " voci copiate", "ok"); await load(); }
            },
        });
        root.append(
            h("div", { class: "card" },
                h("p", { class: "muted", text: "Qui sono modificabili i testi dell'interfaccia. Titolo, biografia, esperienze, progetti e tecnologie si gestiscono nelle rispettive sezioni." }),
                h("div", { class: "row2" }, field("Lingua da modificare", sel), field("Copia da (solo voci mancanti)", h("div", { class: "inl" }, copyFrom, copy)))),
            h("div", { class: "card" }, field("Cerca", filter), holder, h("div", { class: "inl" }, save)));
        await load();
    };

    views.settings = async (root, arg) => {
        const tabs = [["general", "Generale", generalTab], ["seo", "SEO e metadati", seoTab], ["texts", "Testi interfaccia", textsTab]];
        const cur = tabs.find((t) => t[0] === arg) || tabs[0];
        const body = h("div");
        root.append(pageHead("Impostazioni"),
            h("div", { class: "tabs", role: "tablist" }, tabs.map((t) => h("button", { type: "button", role: "tab", class: t[0] === cur[0] ? "on" : "", text: t[1], onclick: () => go("settings", t[0]) }))), body);
        await cur[2](body);
    };

    views.security = async (root) => {
        const [users, sessions, mine] = await Promise.all([
            api("GET", "/api/admin/users"), api("GET", "/api/admin/sessions"), api("GET", "/api/me/sessions"),
        ]);
        const currentId = (mine.find((s) => s.current) || {}).id;
        const revokeUser = async (u) => {
            if (!(await confirmBox("Revoca sessioni", "Chiudere tutte le sessioni di @" + u.login + "? (la tua sessione corrente resta attiva)"))) return;
            const r = await guard(() => api("POST", "/api/admin/users/" + u.id + "/revoke-sessions"));
            if (r) { toast(r.revoked + " sessioni revocate", "ok"); refresh(); }
        };
        const revoke = async (s) => {
            if (!(await confirmBox("Revoca sessione", "Chiudere questa sessione?"))) return;
            if ((await guard(() => api("DELETE", "/api/admin/sessions/" + s.id))) !== undefined) {
                if (s.id === currentId) { state.me = null; renderLogin(); return; }
                toast("Sessione revocata", "ok");
                refresh();
            }
        };
        root.append(pageHead("Sicurezza"),
            h("div", { class: "card" }, h("h3", { text: "Utenti" }),
                h("div", { class: "scroll" }, h("table", null,
                    h("thead", null, h("tr", null, ["Login", "GitHub ID", "Ruolo", "Ultimo accesso", ""].map((x) => h("th", { text: x })))),
                    h("tbody", null, users.map((u) => h("tr", null,
                        h("td", { text: "@" + u.login }), h("td", { class: "muted", text: String(u.github_id) }),
                        h("td", null, h("span", { class: "badge " + (u.role === "ADMIN" ? "ok" : ""), text: u.role })),
                        h("td", { class: "muted", text: fmtDate(u.last_login_at) }),
                        h("td", { class: "acts" }, h("button", { class: "btn sm", type: "button", text: "Revoca sessioni", onclick: () => revokeUser(u) })))))))),
            h("div", { class: "card" }, h("h3", { text: "Sessioni attive" }),
                h("p", { class: "muted", text: "Scadenza per inattività e assoluta gestite dal server; il token è conservato solo come hash." }),
                h("div", { class: "scroll" }, h("table", null,
                    h("thead", null, h("tr", null, ["IP", "Browser", "Creata", "Ultima attività", "Scade", ""].map((x) => h("th", { text: x })))),
                    h("tbody", null, sessions.map((s) => h("tr", null,
                        h("td", null, s.ip || "—", s.id === currentId ? h("span", { class: "badge ok", text: " questa" }) : null),
                        h("td", { class: "muted", text: (s.user_agent || "—").slice(0, 60) }),
                        h("td", { class: "muted", text: fmtDate(s.created_at) }), h("td", { class: "muted", text: fmtDate(s.last_seen_at) }),
                        h("td", { class: "muted", text: fmtDate(s.expires_at) }),
                        h("td", { class: "acts" }, h("button", { class: "btn sm dng", type: "button", text: "Revoca", onclick: () => revoke(s) })))))))));
    };

    views.audit = async (root) => {
        const q = { action: "", severity: "", page: 0 };
        const action = text("", { maxlength: 60, placeholder: "es. content. oppure auth.login" });
        const sev = select([["", "Tutte"], ["info", "info"], ["warning", "warning"], ["security", "security"]], "");
        const out = h("div");
        const load = async () => {
            const params = new URLSearchParams({ page: String(q.page), per_page: "25" });
            if (q.action) params.set("action", q.action);
            if (q.severity) params.set("severity", q.severity);
            const d = await guard(() => api("GET", "/api/admin/audit?" + params.toString()));
            if (!d) return;
            const pages = Math.max(1, Math.ceil(d.total / d.per_page));
            clear(out).append(
                d.items.length
                    ? h("div", { class: "scroll" }, h("table", null,
                        h("thead", null, h("tr", null, ["Quando", "Sev.", "Utente", "Azione", "Entità", "IP", "Dettagli"].map((x) => h("th", { text: x })))),
                        h("tbody", null, d.items.map((a) => h("tr", null,
                            h("td", { class: "muted", text: fmtDate(a.at) }),
                            h("td", null, h("span", { class: "badge " + (a.severity === "security" ? "bad" : a.severity === "warning" ? "warn" : ""), text: a.severity })),
                            h("td", { text: a.actor_login || "—" }), h("td", { text: a.action }),
                            h("td", { class: "muted", text: a.entity ? a.entity + (a.entity_id ? ":" + a.entity_id : "") : "—" }),
                            h("td", { class: "muted", text: a.ip || "—" }),
                            h("td", null, h("details", null, h("summary", { text: "vedi" }), h("pre", { class: "json", text: JSON.stringify(a.details, null, 2) }))))))))
                    : h("div", { class: "empty", text: "Nessun evento." }),
                h("div", { class: "pager" },
                    h("button", { class: "btn sm", type: "button", text: "← Precedente", disabled: q.page <= 0, onclick: () => { q.page -= 1; load(); } }),
                    h("small", { text: "Pagina " + (q.page + 1) + " di " + pages + " · " + d.total + " eventi" }),
                    h("button", { class: "btn sm", type: "button", text: "Successiva →", disabled: q.page + 1 >= pages, onclick: () => { q.page += 1; load(); } })));
        };
        const apply = () => { q.action = action.value.trim(); q.severity = sev.value; q.page = 0; load(); };
        action.addEventListener("keydown", (e) => { if (e.key === "Enter") apply(); });
        sev.addEventListener("change", apply);
        root.append(pageHead("Audit log"),
            h("div", { class: "card" }, h("div", { class: "row2" }, field("Azione (prefisso)", action), field("Severità", sev)),
                h("div", { class: "inl" }, h("button", { class: "btn", type: "button", text: "Filtra", onclick: apply }))),
            h("div", { class: "card" }, out));
        await load();
    };

    const boot = async () => {
        clear(app()).append(h("div", { class: "spin", text: "Caricamento…" }));
        let me;
        try {
            const res = await fetch("/api/me", { credentials: "same-origin", headers: { Accept: "application/json" } });
            me = await res.json();
        } catch (e) {
            renderLogin("Server non raggiungibile.");
            return;
        }
        if (!me || !me.authenticated) {
            renderLogin();
            return;
        }
        state.me = me.user;
        state.csrf = me.csrf;
        if (me.user.role !== "ADMIN") {
            clear(app()).append(h("div", { class: "login" }, h("div", { class: "card" },
                h("h1", { text: "Non autorizzato" }),
                h("p", { text: "Questo account non ha i privilegi di amministratore." }),
                h("button", { class: "btn", type: "button", text: "Esci", onclick: logout }))));
            return;
        }
        renderShell();
    };

    window.addEventListener("hashchange", route);
    boot();
})();

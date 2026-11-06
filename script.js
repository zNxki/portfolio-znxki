(async () => {
  const $ = (s) => document.querySelector(s);
  const $$ = (s) => [].slice.call(document.querySelectorAll(s));

  const esc = (s) =>
    String(s).replace(/[&<>"]/g, (c) => {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c];
    });

  const getJSON = async (u) => {
    const r = await fetch(u);

    if (!r.ok) throw new Error(u + " — " + r.status);

    return r.json();
  };

  const dicts = {};

  const loadLang = async (l) => {
    if (!dicts[l]) dicts[l] = await getJSON("lang/" + l + ".json");

    return dicts[l];
  };

  const fatal = (e) => {
    $("#log").textContent =
      "error: " + e.message + "\nopen this site through a local http server.";
  };

  const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const fine = matchMedia("(pointer:fine)").matches;

  let data;
  let lang;
  let thm = "dark";

  try {
    data = await getJSON("data/site.json");
    lang = data.defaultLang;
  } catch (e) {
    fatal(e);
    return;
  }

  try {
    const sl = localStorage.getItem("lang");

    if (data.languages.includes(sl)) lang = sl;

    const s = localStorage.getItem("theme");

    if (s === "light" || s === "system") thm = s;
  } catch (e) {}

  try {
    await loadLang(lang);
  } catch (e) {
    fatal(e);
    return;
  }

  const V = Object.assign({}, data.boot, {
    name: data.name.full,
    email: (data.contacts.find((c) => c.icon === "email") || {}).value || "",
  });

  const extra = {};

  try {
    const pj = await getJSON("data/projects.json");

    (pj.projects || []).forEach((p) => {
      extra[p.id] = p;
    });
  } catch (e) {}

  const plain = (h) =>
    new DOMParser().parseFromString(String(h || ""), "text/html").body
      .textContent;

  const pick = (m) => (m && (m[lang] || m[data.defaultLang])) || "";

  const safeUrl = (u) => (/^https:\/\//.test(u || "") ? u : "");

  const dot = document.querySelector(".bar .tag .dot");

  if (dot && data.availability) dot.dataset.state = data.availability;

  $$("#dd-lang [data-v]").forEach((b) => {
    if (!data.languages.includes(b.dataset.v)) b.parentNode.remove();
  });

  const fmt = (s, x) =>
    s.replace(/\{(\w+)\}/g, (m, k) => {
      if (x && k in x) return x[k];
      return k in V ? V[k] : m;
    });

  const get = (k) =>
    k.split(".").reduce((o, p) => (o == null ? o : o[p]), dicts[lang]);

  const t = (k) => {
    const v = get(k);

    if (typeof v === "string") return fmt(v);

    return v === undefined ? k : v;
  };

  const ts = (o, nm) =>
    fmt(nm || o.t || "") + "<small>" + fmt(o.s || "") + "</small>";

  const roman = (n) => {
    let s = "";

    [
      [10, "x"],
      [9, "ix"],
      [5, "v"],
      [4, "iv"],
      [1, "i"],
    ].forEach(([v, c]) => {
      while (n >= v) {
        s += c;
        n -= v;
      }
    });

    return s + ".";
  };

  let T0 = document.title;

  $("#yr").textContent = new Date().getFullYear();
  $("h1").setAttribute("aria-label", data.name.full);
  $("#cp").textContent = "~/" + data.name.full + " >";

  $("#ct-list").innerHTML = data.contacts
    .map((c, i) => {
      const rel = /^mailto:/.test(c.href) ? "" : ' rel="noopener"';

      return (
        '<li class="rv" style="--d: ' +
        i +
        '"><a href="' +
        esc(c.href) +
        '"' +
        rel +
        '><span class="n"><svg class="ic" data-icon="' +
        esc(c.icon) +
        '" viewBox="0 0 24 24" aria-hidden="true"></svg></span><span class="k">' +
        esc(c.label) +
        '</span><span class="v">' +
        esc(c.value) +
        '</span><span class="ar">↗</span></a></li>'
      );
    })
    .join("");

  $$("#ct-list .ic").forEach((s) => {
    s.innerHTML = $("#c-" + s.dataset.icon).innerHTML;
  });

  $("#exp-list").innerHTML = data.experience
    .map((id, i) => {
      return (
        '<li class="rv" style="--d: ' +
        i +
        '"><span class="r">' +
        roman(i + 1) +
        '</span><span class="t" data-i18n-ts="expItems.' +
        id +
        '"></span></li>'
      );
    })
    .join("");

  $("#proj").innerHTML = data.projects
    .map((p, i) => {
      return (
        '<li class="rv" style="--d: ' +
        i +
        '" data-id="' +
        esc(p.id) +
        '" data-h="' +
        p.hue +
        '"><span class="r">' +
        roman(i + 1) +
        '</span><span class="t" data-i18n-ts="projectItems.' +
        p.id +
        '" data-name="' +
        esc(p.name || "") +
        '"></span><span class="y">' +
        esc(p.year) +
        "</span></li>"
      );
    })
    .join("");

  $$("#proj li").forEach((l) => {
    const x = extra[l.dataset.id] || {};
    const u = safeUrl(x.demo_url) || safeUrl(x.github_url);

    if (!u) return;

    const open = () => window.open(u, "_blank", "noopener,noreferrer");

    l.tabIndex = 0;
    l.setAttribute("role", "link");
    l.addEventListener("click", open);
    l.addEventListener("keydown", (e) => {
      if (e.key === "Enter") open();
    });
  });

  $("#st-list").innerHTML = data.skills
    .map((s) => {
      return '<span data-name="' + esc(s.name) + '">' + esc(s.name) + "</span>";
    })
    .join("");

  const mqOne = data.marquee
    .map((n) => "<span>" + esc(n) + "</span><span>✦</span>")
    .join("");

  $(".mq > div").innerHTML = mqOne + mqOne;

  const CH = "abcdefghijklmnopqrstuvwxyz01_/<>#";

  const scr = (el, txt, dur) => {
    if (reduce) {
      el.textContent = txt;
      return;
    }

    const t0 = performance.now();

    const f = (now) => {
      const p = Math.min(1, (now - t0) / dur);
      let out = "";

      for (let i = 0; i < txt.length; i++) {
        out +=
          i < p * txt.length ? txt[i] : CH[(Math.random() * CH.length) | 0];
      }

      el.textContent = out;

      if (p < 1) requestAnimationFrame(f);
      else el.textContent = txt;
    };

    f(t0);
  };

  const parts = $$("[data-sc]");

  parts.forEach((p, i) => {
    p.dataset.f = data.name.parts[i];
    p.textContent = "";
  });

  const bootLines = get("boot.lines");
  const BL = data.boot.kinds.map((k, i) => [k, fmt(bootLines[i] || "")]);

  let li = 0;
  let bt;
  let booted = false;
  let tm = 0;
  const log = $("#log");

  const add = (h) => {
    log.insertAdjacentHTML("beforeend", h + "\n");
    log.scrollTop = log.scrollHeight;
  };

  const stamp = () => {
    tm += Math.random() * 0.4 + 0.05;
    return "<i>[" + tm.toFixed(4).padStart(9) + "]</i> ";
  };

  const begin = () => {
    if (booted) return;

    booted = true;
    clearTimeout(bt);

    document.body.classList.remove("lock");
    $("#boot").classList.add("done");

    parts.forEach((p, i) => {
      setTimeout(() => scr(p, p.dataset.f, 700), 350 + i * 140);
    });
  };

  const load = (cb) => {
    const el = document.createElement("span");
    const st = stamp();
    let p = 0;

    log.appendChild(el);
    log.append("\n");

    const tick = () => {
      if (booted) return;

      p = Math.min(100, p + Math.random() * 14 + 4);

      const n = Math.round(p / 5);
      const bar = "█".repeat(n) + "░".repeat(20 - n);

      el.innerHTML =
        st +
        (p < 100 ? "<u>[ .... ]</u> " : "<b>[  OK  ]</b> ") +
        "asset [" +
        bar +
        "] " +
        (p | 0) +
        "%";

      if (p < 100) bt = setTimeout(tick, 60);
      else cb();
    };

    tick();
  };

  const fin = () => {
    const el = document.createElement("span");
    const s =
      data.boot.user + "@" + data.name.full + ":~$ " + data.boot.command;
    let i = 0;

    el.className = "pr";
    log.appendChild(el);

    const ty = () => {
      if (booted) return;

      el.textContent = s.slice(0, ++i);

      if (i < s.length) bt = setTimeout(ty, 35);
      else bt = setTimeout(begin, 400);
    };

    ty();
  };

  const step = () => {
    if (booted) return;

    if (li >= BL.length) {
      bt = setTimeout(fin, 300);
      return;
    }

    const [k, x] = BL[li++];

    if (k === "l") {
      load(() => {
        bt = setTimeout(step, 120);
      });
      return;
    }

    add(stamp() + (k === "o" ? "<b>[  OK  ]</b> " : "<u>[ INFO ]</u> ") + x);
    bt = setTimeout(step, 70 + Math.random() * 130);
  };

  if (reduce) {
    log.textContent = "";
    begin();
  } else {
    step();

    addEventListener("keydown", begin);
    $("#boot").addEventListener("pointerdown", begin);
  }

  let mx = innerWidth / 2;
  let my = innerHeight / 3;
  let rx = mx;
  let ry = my;

  addEventListener("pointermove", (e) => {
    mx = e.clientX;
    my = e.clientY;

    document.documentElement.style.setProperty("--x", mx + "px");
    document.documentElement.style.setProperty("--y", my + "px");
  });

  const peek = $("#peek");
  let px = mx;
  let py = my;

  if (fine && !reduce) {
    document.documentElement.classList.add("has-cursor");

    const dot = $("#dot");
    const xy = $("#xy");

    addEventListener("pointermove", () => {
      dot.style.transform = "translate(" + mx + "px," + my + "px)";
      xy.textContent = "x:" + (mx | 0) + " y:" + (my | 0);
    });

    $$("a,button,.st span").forEach((el) => {
      el.addEventListener("pointerenter", () => {
        document.body.classList.add("hl");
      });

      el.addEventListener("pointerleave", () => {
        document.body.classList.remove("hl");
      });
    });

    $$("#proj li").forEach((l) => {
      l.addEventListener("pointerenter", () => {
        const h = +l.dataset.h;

        document.body.classList.add("hp");

        const img = ((extra[l.dataset.id] || {}).images || [])[0];

        peek.style.background = img
          ? "center / cover url(" + JSON.stringify(img.url) + ")"
          : "linear-gradient(135deg,hsl(" +
            h +
            " 70% 55%),hsl(" +
            ((h + 60) % 360) +
            " 70% 30%))";
        peek.classList.add("on");
      });

      l.addEventListener("pointerleave", () => {
        document.body.classList.remove("hp");
        peek.classList.remove("on");
      });
    });

    const h1 = $("h1");

    addEventListener("pointermove", (e) => {
      h1.style.transform =
        "translate(" +
        (e.clientX / innerWidth - 0.5) * -18 +
        "px," +
        (e.clientY / innerHeight - 0.5) * -10 +
        "px)";
    });

    $$(".bar #tcon,.bar .dd-b,#open,.st span").forEach((el) => {
      el.addEventListener("pointermove", (e) => {
        const b = el.getBoundingClientRect();
        el.style.translate =
          (e.clientX - b.left - b.width / 2) * 0.2 +
          "px " +
          (e.clientY - b.top - b.height / 2) * 0.3 +
          "px";
      });

      el.addEventListener("pointerleave", () => {
        el.style.translate = "";
      });
    });

    $("h1").addEventListener("pointerenter", () => {
      parts.forEach((p) => {
        scr(p, p.dataset.f, 500);
      });
    });
  }

  const sw = (h) => {
    let k = 0;

    const w = (n) => {
      [...n.childNodes].forEach((c) => {
        if (c.nodeType === 3) {
          const f = document.createDocumentFragment();

          c.textContent.split(/(\s+)/).forEach((s) => {
            if (!s.trim()) {
              f.append(s);
              return;
            }

            const o = document.createElement("span");
            const i = document.createElement("span");

            o.className = "wd";
            i.textContent = s;
            i.style.transitionDelay = k++ * 80 + "ms";
            o.append(i);
            f.append(o);
          });

          c.replaceWith(f);
        } else if (c.nodeType === 1) {
          w(c);
        }
      });
    };

    w(h);
  };

  const apply = () => {
    document.documentElement.lang = lang;

    $$("[data-i18n]").forEach((e) => {
      e.innerHTML = t(e.dataset.i18n);

      if (e.tagName === "H2") sw(e);
    });

    $$("[data-i18n-ts]").forEach((e) => {
      e.innerHTML = ts(get(e.dataset.i18nTs) || {}, e.dataset.name);
    });

    $$("[data-i18n-aria]").forEach((e) => {
      e.setAttribute("aria-label", t(e.dataset.i18nAria));
    });

    $("#about-p").innerHTML = get("about.p")
      .map((p) => "<p>" + fmt(p) + "</p>")
      .join("");

    T0 = t("meta.title");

    if (!document.hidden) document.title = T0;

    $('meta[name="description"]').setAttribute(
      "content",
      t("meta.description"),
    );
  };

  const mq = matchMedia("(prefers-color-scheme: light)");
  const mode = () =>
    thm === "light" || (thm === "system" && mq.matches) ? "light" : "dark";

  const paint = () => {
    const r = document.documentElement;

    r.dataset.theme = thm;
    r.dataset.mode = mode();
  };

  mq.addEventListener("change", () => {
    if (thm === "system") paint();
  });

  const setLang = async (l) => {
    try {
      await loadLang(l);
    } catch (e) {
      return;
    }

    lang = l;

    try {
      localStorage.setItem("lang", l);
    } catch (e) {}

    apply();
    skRefresh();
    sync($("#dd-lang"), l);
  };

  const setTheme = (t) => {
    thm = t;

    try {
      localStorage.setItem("theme", t);
    } catch (e) {}

    paint();
    document.documentElement.style.removeProperty("--accent");
    ti = 0;
    sync($("#dd-theme"), t);
  };

  paint();
  apply();

  $$(".ol li,.ct li").forEach((el) => {
    el.addEventListener("pointermove", (e) => {
      const b = el.getBoundingClientRect();

      el.style.setProperty("--mx", e.clientX - b.left + "px");
      el.style.setProperty("--my", e.clientY - b.top + "px");
    });
  });

  $$(".st span").forEach((s, i) => {
    s.style.setProperty("--i", i);
  });

  const io = new IntersectionObserver(
    (es) => {
      es.forEach((e) => {
        if (e.isIntersecting) {
          e.target.classList.add("in");

          if (e.target.classList.contains("lbl")) {
            scr(e.target, e.target.textContent, 600);
          }
          io.unobserve(e.target);
        }
      });
    },
    { threshold: 0.12 },
  );

  $$(".rv").forEach((el) => {
    io.observe(el);
  });

  addEventListener("pointerdown", (e) => {
    if (reduce || !booted) return;

    for (let i = 0; i < 10; i++) {
      const s = document.createElement("i");
      const a = (i / 10) * 6.283 + Math.random();
      const d = 40 + Math.random() * 50;

      s.className = "bst";
      s.style.cssText =
        "left:" +
        e.clientX +
        "px;top:" +
        e.clientY +
        "px;--dx:" +
        Math.cos(a) * d +
        "px;--dy:" +
        Math.sin(a) * d +
        "px";
      document.body.appendChild(s);

      setTimeout(() => s.remove(), 700);
    }
  });

  const nav = new IntersectionObserver(
    (es) => {
      es.forEach((e) => {
        if (!e.isIntersecting) return;

        $$(".bar nav a").forEach((a) => {
          a.classList.toggle(
            "on",
            a.getAttribute("href") === "#" + e.target.id,
          );
        });
      });
    },
    { rootMargin: "-45% 0px -50% 0px" },
  );

  $$("section").forEach((s) => {
    nav.observe(s);
  });

  document.addEventListener("visibilitychange", () => {
    document.title = document.hidden ? t("meta.away") : T0;
  });

  const SK = {};

  data.skills.forEach((s) => {
    SK[s.name] = s.level;
  });

  const levelKey = (n) => data.levels.find((l) => n >= l.min).key;

  let skSel = "";

  const skRefresh = () => {
    if (!(skSel in SK)) {
      $("#sk-n").textContent = t("stackUi.pick");
      $("#sk-d").textContent = t("stackUi.hint");
      return;
    }

    $("#sk-n").textContent = skSel;
    $("#sk-d").textContent =
      t("levels." + levelKey(SK[skSel])) +
      " — " +
      (dicts[lang].skills[skSel] || "");
  };

  skRefresh();

  const skP = $("#sk-p");
  const skF = $("#sk-f");
  let skT;

  $$(".st span").forEach((el) => {
    el.tabIndex = 0;
    el.setAttribute("role", "button");

    const pick = () => {
      const n = el.dataset.name;

      if (!(n in SK)) return;

      const d = SK[n];

      skSel = n;

      $$(".st span").forEach((s) => {
        s.classList.toggle("on", s === el);
      });

      skRefresh();
      skF.style.width = d + "%";

      cancelAnimationFrame(skT);

      const t0 = performance.now();

      const f = (now) => {
        const p = reduce ? 1 : Math.min(1, (now - t0) / 700);

        skP.textContent = Math.round(d * p) + "%";

        if (p < 1) skT = requestAnimationFrame(f);
      };

      f(t0);
    };

    el.addEventListener("click", pick);

    el.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        pick();
      }
    });
  });

  const hw = $(".hero .wrap");

  const loop = () => {
    rx += (mx - rx) * 0.16;
    ry += (my - ry) * 0.16;

    if (fine && !reduce) {
      $("#ring").style.transform = "translate(" + rx + "px," + ry + "px)";
      $("#xy").style.transform = "translate(" + rx + "px," + ry + "px)";

      px += (mx - px) * 0.12;
      py += (my - py) * 0.12;

      peek.style.transform =
        "translate(" + (px + 28) + "px," + (py - 70) + "px)";
    }

    hw.style.opacity = Math.max(0, 1 - scrollY / (innerHeight * 0.7));

    const max = document.documentElement.scrollHeight - innerHeight;

    $("#prog").style.transform =
      "scaleX(" + (max > 0 ? scrollY / max : 0) + ")";

    requestAnimationFrame(loop);
  };

  loop();

  setInterval(() => {
    $("#clock").textContent = new Date().toLocaleTimeString(t("locale"));
  }, 1000);

  const con = $("#con");
  const out = $("#out");
  const inp = $("#inp");
  const hist = [];
  let ti = 0;
  let mxOn = false;
  let cols = [];

  const mc = document.createElement("canvas");
  const mctx = mc.getContext("2d");

  mc.id = "mx";
  document.body.prepend(mc);

  const rain = () => {
    if (!mxOn) return;

    mctx.fillStyle =
      mode() === "light" ? "rgba(246,245,241,0.12)" : "rgba(10,10,11,0.12)";
    mctx.fillRect(0, 0, mc.width, mc.height);
    mctx.fillStyle = getComputedStyle(
      document.documentElement,
    ).getPropertyValue("--accent");
    mctx.font = "14px monospace";

    cols.forEach((y, i) => {
      mctx.fillText(CH[(Math.random() * CH.length) | 0], i * 14, y * 14);
      cols[i] = y * 14 > mc.height && Math.random() > 0.975 ? 0 : y + 1;
    });

    setTimeout(() => requestAnimationFrame(rain), 40);
  };

  let hi = 0;

  const CMD = {
    help: () => t("console.help"),

    about: () => t("console.about"),

    skills: () => data.marquee.join(" · "),

    work: (a) => {
      const hit = a && data.projects.find((p) => p.id === a);

      if (hit) {
        const x = extra[hit.id] || {};
        const o = get("projectItems." + hit.id) || {};
        const rows = [
          esc(hit.name || plain(o.t)),
          esc(plain(pick(x.description))),
          x.status ? "status: " + esc(x.status) : "",
          (x.technologies || []).length
            ? "stack: " + esc(x.technologies.join(", "))
            : "",
          safeUrl(x.github_url) ? "github: " + esc(x.github_url) : "",
          safeUrl(x.demo_url) ? "demo: " + esc(x.demo_url) : "",
        ];

        return rows.filter(Boolean).join("\n");
      }

      return data.projects
        .map((p, i) => {
          const o = get("projectItems." + p.id) || {};

          return (
            roman(i + 1) + " " + (p.name || o.t || "") + " (" + p.year + ")"
          );
        })
        .join("\n");
    },

    contact: () => t("console.contact"),

    goto: (a) => {
      const m = data.sections;

      if (a && Object.hasOwn(m, a)) {
        $(m[a]).scrollIntoView({
          behavior: reduce ? "auto" : "smooth",
        });

        return t("console.goingTo") + " <i>" + esc(a) + "</i>…";
      }

      return t("console.unknownSection");
    },

    theme: () => {
      const T = data.accents[mode()];

      ti = (ti + 1) % T.length;
      document.documentElement.style.setProperty("--accent", T[ti]);

      return t("console.theme") + ": <i>" + T[ti] + "</i>";
    },

    whoami: () =>
      t("console.whoami") +
      (pick(data.jobTitle) ? "\n" + esc(pick(data.jobTitle)) : ""),

    matrix: () => {
      if (reduce) return t("console.matrixReduced");

      mxOn = !mxOn;
      mc.style.display = mxOn ? "block" : "none";

      if (mxOn) {
        mc.width = innerWidth;
        mc.height = innerHeight;
        cols = new Array(Math.ceil(innerWidth / 14)).fill(0);
        rain();
      } else {
        mctx.clearRect(0, 0, mc.width, mc.height);
      }

      return "matrix: <i>" + (mxOn ? "on" : "off") + "</i>";
    },

    clear: () => {
      out.innerHTML = "";
      return "";
    },

    exit: () => {
      close();
      return "";
    },
  };

  const openC = () => {
    con.classList.add("open");

    setTimeout(() => {
      inp.focus();
    }, 50);

    if (!out.innerHTML) {
      out.innerHTML = t("console.intro") + "\n";
    }
  };

  const close = () => {
    con.classList.remove("open");
    inp.blur();
  };

  inp.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      const v = inp.value.trim();

      inp.value = "";

      if (!v) return;

      hist.push(v);
      hi = hist.length;

      const a = v.split(/\s+/);
      const c = a[0].toLowerCase();

      const r = Object.hasOwn(CMD, c)
        ? CMD[c](a[1] && a[1].toLowerCase())
        : fmt(get("console.notFound"), { c: esc(c) });

      if (c !== "clear") {
        out.innerHTML += "<b>&gt; " + esc(v) + "</b>\n" + r + "\n";
      }

      out.scrollTop = out.scrollHeight;
    } else if (e.key === "Tab") {
      e.preventDefault();

      const m = Object.keys(CMD).find((k) => {
        return inp.value && k.startsWith(inp.value.toLowerCase());
      });

      if (m) inp.value = m;
    } else if (e.key === "ArrowUp") {
      if (hi > 0) inp.value = hist[--hi] || "";

      e.preventDefault();
    } else if (e.key === "ArrowDown") {
      inp.value = hist[++hi] || "";

      e.preventDefault();
    }
  });

  addEventListener("keydown", (e) => {
    if (!booted) return;

    if (e.key === "/" && document.activeElement !== inp) {
      e.preventDefault();
      openC();
    } else if (e.key === "Escape") {
      close();
    }
  });

  $("#open").onclick = () => {
    con.classList.contains("open") ? close() : openC();
  };

  $("#tcon").onclick = openC;

  const sync = (d, v) => {
    d.querySelectorAll(".dd-m button").forEach((o) => {
      const s = o.dataset.v === v;

      o.classList.toggle("sel", s);
      o.setAttribute("aria-selected", s);

      if (s) {
        const b = d.querySelector(".dd-b");
        const c = b.querySelector(".dd-c");

        b.querySelector("use").setAttribute(
          "href",
          o.querySelector("use").getAttribute("href"),
        );
        if (c) c.textContent = o.dataset.c;
      }
    });
  };

  const dds = $$(".dd");

  const shut = () => {
    dds.forEach((d) => {
      d.classList.remove("open");
      d.querySelector(".dd-b").setAttribute("aria-expanded", "false");
    });
  };

  dds.forEach((d) => {
    const b = d.querySelector(".dd-b");
    const opts = [...d.querySelectorAll(".dd-m button")];

    b.addEventListener("click", (e) => {
      e.stopPropagation();

      const o = d.classList.contains("open");

      shut();

      if (!o) {
        d.classList.add("open");
        b.setAttribute("aria-expanded", "true");
      }
    });

    opts.forEach((o) => {
      o.addEventListener("click", () => {
        shut();
        b.focus();
        d.id === "dd-lang" ? setLang(o.dataset.v) : setTheme(o.dataset.v);
      });
    });

    d.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        shut();
        b.focus();
        return;
      }

      if (!d.classList.contains("open")) return;
      if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;

      e.preventDefault();

      const n =
        opts.indexOf(document.activeElement) + (e.key === "ArrowDown" ? 1 : -1);

      opts[(n + opts.length) % opts.length].focus();
    });
  });

  $$('a[href*="#"]').forEach((link) => {
    link.addEventListener("click", (e) => {
      const href = link.getAttribute("href");
      if (!href || !href.startsWith("#")) return;

      const target = document.querySelector(href);
      if (!target) return;

      e.preventDefault();
      target.scrollIntoView({
        behavior: reduce ? "auto" : "smooth",
        block: "start",
      });
    });
  });

  document.addEventListener("click", shut);

  sync($("#dd-lang"), lang);
  sync($("#dd-theme"), thm);
})();

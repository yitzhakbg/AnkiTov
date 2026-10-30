/* AnkiTov Solo Web Reviewer — vanilla JS, zero external requests (spec §1.2 #3).
   Flow: login (JWT) → session/current → session/cards → show answer → answer
   (idempotent ledger POST) → next card. Progress saves server-side; nothing
   leaves this origin. */
(function () {
  "use strict";

  var TOKEN_KEY = "ankitov_reviewer_token";
  var LOCALES = {
    en: {
      "login.title": "Practice", "login.subtitle": "Brief daily practice, at your own pace. SRS works.",
      "login.email": "Email", "login.password": "Password", "login.start": "Start practicing",
      "review.show": "Show answer", "review.again": "Again", "review.hard": "Hard",
      "review.good": "Good", "review.easy": "Easy", "review.logout": "Log out",
      "review.hint": "Spaced repetition brings back only what is about to fade.",
      "empty.title": "Nothing due right now", "empty.default": "No active practice session.",
      "empty.import_title": "Try a starter pack", "empty.import_sub": "Free CC0 packs — imported straight into your practice space.",
      "err.login": "Login failed — check email and password.", "err.load": "Could not load your session.",
      "err.import": "Import failed.", "importing": "Importing…"
    },
    he: {
      "login.title": "תרגול", "login.subtitle": "תרגול יומי קצר, בקצב שלכם. SRS works.",
      "login.email": "אימייל", "login.password": "סיסמה", "login.start": "התחילו לתרגל",
      "review.show": "הצגת תשובה", "review.again": "שוב", "review.hard": "קשה",
      "review.good": "טוב", "review.easy": "קל", "review.logout": "התנתקות",
      "review.hint": "החזרה מרווחת מחזירה רק את מה שעומד לדהות.",
      "empty.title": "אין כרטיסים לתרגול כרגע", "empty.default": "אין מפגש תרגול פעיל.",
      "empty.import_title": "נסו חבילת התחלה", "empty.import_sub": "חבילות CC0 חינמיות — מיובאות ישירות למרחב התרגול שלכם.",
      "err.login": "ההתחברות נכשלה — בדקו אימייל וסיסמה.", "err.load": "טעינת המפגש נכשלה.",
      "err.import": "הייבוא נכשל.", "importing": "מייבא…"
    }
  };
  var locale = (navigator.language || "en").slice(0, 2) === "he" ? "he" : "en";
  var T = LOCALES[locale];

  function t(key) { return T[key] || LOCALES.en[key] || key; }
  function applyI18n() {
    document.documentElement.lang = locale;
    document.documentElement.dir = locale === "he" ? "rtl" : "ltr";
    document.querySelectorAll("[data-i18n]").forEach(function (el) {
      el.textContent = t(el.getAttribute("data-i18n"));
    });
  }

  var state = { token: localStorage.getItem(TOKEN_KEY), cards: [], idx: 0, sessionId: null, completed: 0, total: 0, cardStart: 0 };

  function api(path, opts) {
    opts = opts || {};
    opts.headers = Object.assign({ "Content-Type": "application/json" }, opts.headers || {});
    if (state.token) opts.headers["Authorization"] = "Bearer " + state.token;
    return fetch(path, opts).then(function (r) {
      if (r.status === 401) { logout(); throw new Error("unauthorized"); }
      return r.json();
    });
  }

  function show(view) {
    ["view-login", "view-review", "view-empty"].forEach(function (id) {
      document.getElementById(id).hidden = id !== view;
    });
  }

  function logout() {
    localStorage.removeItem(TOKEN_KEY);
    state.token = null;
    show("view-login");
  }

  // ── login ──
  document.getElementById("login-form").addEventListener("submit", function (e) {
    e.preventDefault();
    var errEl = document.getElementById("login-error");
    errEl.hidden = true;
    fetch("/api/v1/auth/login", {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        email: document.getElementById("login-email").value,
        password: document.getElementById("login-password").value
      })
    }).then(function (r) { return r.json(); }).then(function (data) {
      var token = data.token || (data.data && data.data.token);
      if (!token) throw new Error("no token");
      state.token = token;
      localStorage.setItem(TOKEN_KEY, token);
      loadSession();
    }).catch(function () {
      errEl.textContent = t("err.login");
      errEl.hidden = false;
    });
  });

  // ── session load ──
  function loadSession() {
    api("/api/v1/reviewer/session/current").then(function (data) {
      if (data.status === "ok" && data.session) {
        state.sessionId = data.session.id;
        state.completed = data.session.cards_completed;
        state.total = data.session.cards_total;
        return api("/api/v1/reviewer/session/cards");
      }
      showEmpty(data.message);
    }).then(function (data) {
      if (!data) return;
      if (data.status !== "ok") { showEmpty(data.message); return; }
      state.cards = data.cards || [];
      state.idx = 0;
      show("view-review");
      renderCard();
    }).catch(function (e) {
      if (e.message !== "unauthorized") showEmpty(t("err.load"));
    });
  }

  function showEmpty(message) {
    document.getElementById("empty-message").textContent = message || t("empty.default");
    show("view-empty");
    loadSeedPacks();
  }

  // ── seed packs (empty state) ──
  function loadSeedPacks() {
    fetch("/api/v1/decks/available").then(function (r) { return r.json(); }).catch(function () { return []; })
      .then(function (packs) {
        if (!Array.isArray(packs)) return;
        var list = document.getElementById("seed-list");
        list.innerHTML = "";
        packs.slice(0, 8).forEach(function (p) {
          var b = document.createElement("button");
          b.className = "btn";
          b.innerHTML = p.name + '<span class="meta">' + (p.card_count || "?") + " · " + (p.language || "?") + "</span>";
          b.addEventListener("click", function () {
            b.disabled = true;
            b.innerHTML = t("importing");
            api("/api/v1/reviewer/import-seed", { method: "POST", body: JSON.stringify({ package: p.package }) })
              .then(function () { loadSession(); })
              .catch(function () { b.innerHTML = t("err.import"); b.disabled = false; });
          });
          list.appendChild(b);
        });
      });
  }

  // ── card rendering ──
  function renderCard() {
    if (state.idx >= state.cards.length) { loadSession(); return; }
    var card = state.cards[state.idx];
    document.getElementById("deck-name").textContent = card.deck || "";
    updateProgress();
    var q = document.getElementById("card-question");
    var a = document.getElementById("card-answer");
    q.innerHTML = card.question;
    a.innerHTML = card.answer;
    q.hidden = false; a.hidden = true;
    document.getElementById("btn-show").hidden = false;
    document.getElementById("answer-buttons").hidden = true;
    if (card.flags && card.flags.rtl) { a.dir = "rtl"; q.dir = "rtl"; } else { q.dir = "auto"; a.dir = "auto"; }
    state.cardStart = Date.now();
  }

  function updateProgress() {
    document.getElementById("session-progress").textContent = state.completed + " / " + state.total;
  }

  // ── answer flow ──
  document.getElementById("btn-show").addEventListener("click", function () {
    document.getElementById("card-question").hidden = true;
    document.getElementById("card-answer").hidden = false;
    document.getElementById("btn-show").hidden = true;
    document.getElementById("answer-buttons").hidden = false;
  });

  document.querySelectorAll("#answer-buttons .btn").forEach(function (btn) {
    btn.addEventListener("click", function () {
      var card = state.cards[state.idx];
      var ease = parseInt(btn.getAttribute("data-ease"), 10);
      var answerId = crypto.randomUUID ? crypto.randomUUID() : String(Date.now()) + "-" + Math.random();
      api("/api/v1/reviewer/answer", {
        method: "POST",
        body: JSON.stringify({
          answer_id: answerId,
          card_id: card.card_id,
          ease: ease,
          time_ms: Math.min(Date.now() - state.cardStart, 600000),
          session_id: state.sessionId
        })
      }).then(function () {
        state.completed += 1;
        state.idx += 1;
        updateProgress();
        renderCard();
      }).catch(function () { /* keep the card up; the ledger is idempotent on retry */ });
    });
  });

  ["btn-logout", "btn-logout2"].forEach(function (id) {
    var el = document.getElementById(id);
    if (el) el.addEventListener("click", logout);
  });

  // ── boot ──
  applyI18n();
  if (state.token) { loadSession(); } else { show("view-login"); }
  if ("serviceWorker" in navigator) {
    navigator.serviceWorker.register("/reviewer/sw.js").catch(function () { /* offline shell is best-effort */ });
  }
})();

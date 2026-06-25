// Dashboard de monitoring — poll de l'API locale /state exposée par le backend Rust.
const $ = (id) => document.getElementById(id);
let lastUpdate = 0;

function fmt(n, d = 2) {
  return (n === null || n === undefined || Number.isNaN(n)) ? "—" : Number(n).toFixed(d);
}
function signed(el, n, d = 2) {
  el.textContent = fmt(n, d);
  el.classList.toggle("pos", n > 0);
  el.classList.toggle("neg", n < 0);
}

async function refresh() {
  try {
    const r = await fetch("/state", { cache: "no-store" });
    const s = await r.json();
    $("status").textContent = "✓ connecté";
    $("status").className = "ok";
    lastUpdate = Date.now();

    const dry = $("dry");
    dry.textContent = s.dry_run ? "PAPER" : "LIVE";
    dry.className = "badge " + (s.dry_run ? "paper" : "live");

    // Radar
    $("binance").innerHTML = s.binance_connected ? '<span class="ok">connecté</span>' : '<span class="ko">déconnecté</span>';
    $("micro").textContent = fmt(s.btc_micro, 1);
    $("obi").textContent = (s.obi >= 0 ? "+" : "") + fmt(s.obi, 3);
    $("kills").textContent = s.kills_emitted ?? 0;

    // Exécuteur
    $("slug").textContent = s.market_slug || "—";
    $("rem").textContent = s.remaining_s != null ? s.remaining_s + "s" : "—";
    $("sigma").textContent = fmt(s.sigma, 2);
    $("fair").textContent = fmt(s.fair, 3);
    $("mid").textContent = fmt(s.up_mid, 3);
    $("quotes").textContent = `${fmt(s.up_bid, 2)} / ${fmt(s.up_ask, 2)}`;
    $("band").innerHTML = s.in_band ? '<span class="ok">oui ✓</span>' : '<span class="ko">non ✗</span>';
    const st = s.paused ? '<span class="ko">KILL ⏸</span>' : '<span class="ok">cotation</span>';
    $("execstate").innerHTML = st;
    $("block").textContent = s.last_block_reason || "—";
    $("signals").textContent = s.signals_received ?? 0;

    // Bankroll & PnL
    $("equity").textContent = fmt(s.equity, 2);
    $("cash").textContent = fmt(s.cash, 2);
    $("posval").textContent = fmt(s.position_value, 2);
    signed($("net"), s.net_exposure, 0);
    signed($("wpnl"), s.window_pnl, 2);
    $("dd").textContent = fmt(s.drawdown, 2);
    signed($("realized"), s.realized_pnl, 2);
    $("updn").textContent = `${fmt(s.up_bal, 0)} / ${fmt(s.down_bal, 0)}`;
    $("mtfills").textContent = `${s.maker_fills ?? 0} / ${s.taker_fills ?? 0}`;
    $("sm").textContent = `${s.sells ?? 0} / ${s.merges ?? 0}`;

    renderBook(s);
  } catch (e) {
    $("status").textContent = "✗ backend injoignable";
    $("status").className = "ko";
  }
}

function renderBook(s) {
  const el = $("book");
  const bids = (s.book_bids || []).slice().sort((a, b) => b.price - a.price);
  const asks = (s.book_asks || []).slice().sort((a, b) => a.price - b.price);
  if (!bids.length && !asks.length) { el.innerHTML = '<div class="muted">en attente…</div>'; return; }

  const maxSize = Math.max(1, ...bids.map(l => l.size), ...asks.map(l => l.size));
  const bar = (sz) => `<span class="bar" style="width:${Math.min(100, sz / maxSize * 100)}%"></span>`;
  const row = (l, cls, tag) =>
    `<div class="lvl ${cls}">${bar(l.size)}<span class="px">${l.price.toFixed(2)}${tag ? ` <span class="tag">${tag}</span>` : ""}</span><span class="sz">${Math.round(l.size)}</span></div>`;

  // Asks affichés du plus haut au plus bas (best ask juste au-dessus du spread).
  let html = asks.slice(0, 6).reverse().map(l => {
    const mine = s.up_ask && Math.abs(l.price - s.up_ask) < 0.005;
    return row(l, "ask" + (mine ? " mine" : ""), mine ? "◄ notre ask" : "");
  }).join("");

  // Si notre ask est hors des niveaux affichés, on l'ajoute comme marqueur.
  if (s.up_ask && !asks.slice(0, 6).some(l => Math.abs(l.price - s.up_ask) < 0.005)) {
    html += row({ price: s.up_ask, size: 0 }, "ask mine", "◄ notre ask");
  }

  html += `<div class="spread-row">mid ${Number(s.up_mid).toFixed(3)} · fair ${Number(s.fair).toFixed(3)}</div>`;

  if (s.up_bid && !bids.slice(0, 6).some(l => Math.abs(l.price - s.up_bid) < 0.005)) {
    html += row({ price: s.up_bid, size: 0 }, "bid mine", "◄ notre bid");
  }
  html += bids.slice(0, 6).map(l => {
    const mine = s.up_bid && Math.abs(l.price - s.up_bid) < 0.005;
    return row(l, "bid" + (mine ? " mine" : ""), mine ? "◄ notre bid" : "");
  }).join("");

  el.innerHTML = html;
}

setInterval(() => {
  $("clock").textContent = new Date().toLocaleTimeString();
  const age = lastUpdate ? Math.round((Date.now() - lastUpdate) / 1000) : "—";
  $("age").textContent = age + "s";
}, 1000);
setInterval(refresh, 1000);
refresh();

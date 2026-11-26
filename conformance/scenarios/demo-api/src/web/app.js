// The rutent demo's page: the network drawn as a graph (nodes as vertices, each provider's tuple a labelled directed
// edge), the demo's acts replayed through the API, and the log as it grows. Every step below is one call to the
// server; the story pane shows each call and what it answered.
"use strict";

const $ = (s, el = document) => el.querySelector(s);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const pace = () => Number($("#pace").value);
const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

// Amounts travel in their asset's smallest unit, two decimals: 5000000 of COP is 50,000.00 COP.
function money(units, asset) {
  if (units == null || !/^\d+$/.test(String(units))) return `${units ?? "?"} ${asset ?? ""}`.trim();
  const s = String(units).padStart(3, "0");
  const whole = s.slice(0, -2).replace(/^0+(?=\d)/, "").replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  return `${whole}.${s.slice(-2)} ${asset ?? ""}`.trim();
}
const toUnits = (text) => {
  const m = String(text).replace(/,/g, "").trim().match(/^(\d+)(?:\.(\d{1,2}))?$/);
  if (!m) return null;
  return String(BigInt(m[1]) * 100n + BigInt((m[2] || "0").padEnd(2, "0")));
};

// ---- Calling the API ------------------------------------------------------------------------------------------

let busy = 0;
function setBusy(delta) {
  busy += delta;
  $("#graph-busy").hidden = busy === 0;
  document.querySelectorAll(".acts button, .try-pane button").forEach((b) => {
    if (b.id !== "try-accept" && b.id !== "try-run") b.disabled = busy > 0;
  });
  if (busy === 0) refreshTryButtons();
}

class ApiError extends Error {
  constructor(status, body) { super(body?.error || `HTTP ${status}`); this.status = status; this.body = body; }
}

// One call, shown in the story pane: the verb and path, then its status and a one-line summary of what it answered.
async function call(method, path, body, summarize, { expect = [] } = {}) {
  const li = story.item("call", `<div class="req"><span class="verb">${method}</span>${esc(path)}<span class="code">…</span></div>`);
  if (body !== undefined) li.title = JSON.stringify(body);
  setBusy(1);
  try {
    const res = await fetch(path, {
      method, headers: body !== undefined ? { "Content-Type": "application/json" } : {},
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });
    const json = await res.json().catch(() => ({}));
    const ok = res.ok || expect.includes(res.status);
    const code = $(".code", li);
    code.textContent = res.status;
    code.className = `code ${res.ok ? "ok" : "no"}`;
    let summary = "";
    try { summary = res.ok && summarize ? summarize(json, res.status) : (json.error ? esc(json.error) : ""); } catch (e) { summary = esc(e.message); }
    li.insertAdjacentHTML("beforeend", `${summary ? `<div class="sum">${summary}</div>` : ""}
      <details><summary>response</summary><pre>${esc(JSON.stringify(json, null, 2))}</pre></details>`);
    if (!ok) { li.classList.add("error"); throw new ApiError(res.status, json); }
    return { status: res.status, ...json };
  } finally {
    setBusy(-1);
    pollLog();
    pollWorld();
  }
}
const get = (path, s) => call("GET", path, undefined, s);
const post = (path, body, s, o) => call("POST", path, body ?? {}, s, o);

// ---- The story pane -------------------------------------------------------------------------------------------

const story = {
  item(kind, html) {
    const li = document.createElement("li");
    li.className = kind;
    li.innerHTML = html;
    $("#story").append(li);
    li.scrollIntoView({ block: "nearest" });
    return li;
  },
  act(title) { this.item("act", esc(title)); },
  say(text) { this.item("say", esc(text)); },
  note(text) { this.item("note", esc(text)); },
  html(html) { return this.item("", html); },
};

// ---- The graph ------------------------------------------------------------------------------------------------

let cy = null;
let net = null;

// Where each node sits: each publisher's bank beside its ledger, the chains in the middle with their addresses
// above and below, so a route reads left to right. A node this table doesn't know sends the drawing to a force
// layout instead.
const PLACES = {
  "sim-co-bank": [40, 150], "sim-andina-ledger": [270, 150],
  "sim-mx-bank": [40, 560], "sim-azteca-ledger": [270, 560],
  "sim-polygon-addresses": [560, 0], "sim-polygon": [560, 190],
  "sim-base": [560, 520], "sim-base-addresses": [560, 710],
  "sim-liberty-ledger": [850, 355], "sim-us-bank": [1080, 355],
};

function drawGraph() {
  const loops = {};
  const elements = [];
  for (const n of net.nodes) {
    elements.push({ group: "nodes", data: { id: n.id, label: `${n.id}\n${n.kind}`, kind: n.kind }, classes: n.kind });
  }
  for (const e of net.edges) {
    const terms = [e.fee !== "0" ? `fee ${e.fee_shown}` : "no fee", e.seconds != null ? `${e.seconds} s` : null].filter(Boolean).join(", ");
    const data = {
      id: e.id, source: e.from, target: e.to, provider: e.provider,
      label: `${e.provider}\n${e.does} · ${e.assets}`, terms,
    };
    if (e.from === e.to) {
      const k = (loops[e.from] = (loops[e.from] ?? -1) + 1);
      const up = (PLACES[e.from]?.[1] ?? 0) < 355;
      data.loopDir = `${(up ? -20 : 200) + k * 70}deg`;
    }
    elements.push({ group: "edges", data, classes: e.from === e.to ? "loop" : "" });
  }
  const h = net.holder;
  elements.push({ group: "nodes", data: { id: "holder", label: `${h.provider}\noperator's wallet (holding account)` }, classes: "holder" });
  elements.push({ group: "edges", data: { id: "holds", source: "holder", target: h.holds, label: "holds" }, classes: "holds" });

  const known = net.nodes.every((n) => PLACES[n.id]);
  cy = cytoscape({
    container: $("#graph"),
    elements,
    wheelSensitivity: 0.25,
    layout: known
      ? { name: "preset", positions: (n) => (n.id() === "holder" ? { x: 800, y: 60 } : { x: PLACES[n.id()][0], y: PLACES[n.id()][1] }), padding: 30 }
      : { name: "cose", animate: false, nodeRepulsion: 9000, idealEdgeLength: 160, padding: 30 },
    style: [
      { selector: "node", style: {
        label: "data(label)", "text-wrap": "wrap", "text-valign": "center", "font-size": 12, "font-weight": 600,
        width: "label", height: "label", padding: "12px", shape: "round-rectangle", "border-width": 2,
        "background-color": "#f1efe8", "border-color": "#8c826b", color: "#1f2328",
      } },
      { selector: "node.private", style: { shape: "ellipse", "background-color": "#eeedfe", "border-color": "#6e5bd0", "border-style": "dashed", padding: "18px" } },
      { selector: "node.public", style: { shape: "ellipse", "background-color": "#e1f5ee", "border-color": "#1d8a64", padding: "18px" } },
      { selector: "node.holder", style: { shape: "tag", "font-size": 10, "font-weight": 400, "background-color": "#fafafa", "border-style": "dotted", "border-color": "#999" } },
      { selector: "node.on-route", style: { "border-width": 4, "border-color": "#2563eb" } },
      { selector: "node.resting", style: { "border-width": 5, "border-color": "#e8a33d", "underlay-color": "#e8a33d", "underlay-opacity": 0.25, "underlay-padding": 8 } },
      { selector: "edge", style: {
        "curve-style": "bezier", "control-point-step-size": 70, width: 1.6, "line-color": "#9aa4ae",
        "target-arrow-shape": "triangle", "target-arrow-color": "#9aa4ae", "arrow-scale": 1.1,
        label: "data(label)", "font-size": 9, "text-wrap": "wrap", "text-rotation": "autorotate",
        "text-background-color": "#ffffff", "text-background-opacity": 0.85, "text-background-padding": "1px", color: "#3b4651",
      } },
      { selector: "edge.loop", style: { "loop-direction": "data(loopDir)", "loop-sweep": "-50deg", "control-point-step-size": 90, "text-rotation": "none" } },
      { selector: "edge.holds", style: { "line-style": "dashed", "target-arrow-shape": "none", "line-color": "#bbb", "font-size": 8 } },
      { selector: "edge.cand", style: { "line-color": "#e8a33d", "target-arrow-color": "#e8a33d", "line-style": "dashed", width: 2.6 } },
      { selector: "edge.chosen", style: { "line-color": "#2563eb", "target-arrow-color": "#2563eb", width: 5, "z-index": 10, "font-weight": 700, color: "#1e3a8a" } },
      { selector: "edge.settled", style: { "line-color": "#059669", "target-arrow-color": "#059669", width: 5, "line-style": "solid", "z-index": 11 } },
      { selector: "edge.failed", style: { "line-color": "#dc2626", "target-arrow-color": "#dc2626", width: 5, "line-style": "solid", "z-index": 12 } },
      { selector: "edge.hover", style: { "line-color": "#9333ea", "target-arrow-color": "#9333ea", width: 5, "line-style": "solid", "z-index": 20 } },
      { selector: ".faded", style: { opacity: 0.35 } },
    ],
  });
  // The pane changes size with the window; the drawing follows it.
  new ResizeObserver(() => { cy.resize(); cy.fit(undefined, 30); }).observe($("#graph"));
  cy.on("mouseover", "edge", (ev) => {
    const d = ev.target.data();
    if (d.terms) $("#graph").title = `${d.provider}: ${d.label.split("\n")[1]} (${d.terms})`;
  });
}

// What the graph shows of a route: the chosen one in blue, the other candidates dashed, the rest faded.
function showRoute(chosenOps, candidates, { keep = false } = {}) {
  if (!keep) clearMarks();
  const used = new Set();
  for (const c of candidates || []) {
    if (c.chosen) continue;
    for (const op of c.route) { cy.getElementById(op.edge).addClass("cand"); used.add(op.edge); }
  }
  for (const op of chosenOps || []) {
    const e = cy.getElementById(op.edge);
    e.removeClass("cand").addClass("chosen");
    used.add(op.edge);
    cy.getElementById(op.from).addClass("on-route");
    cy.getElementById(op.to).addClass("on-route");
  }
  cy.edges().forEach((e) => { if (!used.has(e.id()) && !e.hasClass("holds")) e.addClass("faded"); });
}
function clearMarks() {
  cy.elements().removeClass("cand chosen settled failed hover faded on-route resting");
}
async function animateSegments(segments) {
  for (const s of segments) {
    const e = cy.getElementById(s.edge);
    e.removeClass("faded cand");
    e.addClass(s.outcome === "settled" ? "settled" : "failed");
    if (s.outcome === "settled") {
      cy.nodes().removeClass("resting");
      cy.getElementById(s.to).addClass("resting");
    }
    await sleep(Math.max(150, pace() / 2));
  }
}

// ---- The route pane -------------------------------------------------------------------------------------------

const path = (ops) => ops.map((o) => esc(o.provider)).join(" → ");
function dropReason(d) {
  if (!d) return "";
  if (typeof d === "string") return d;
  const { reason, ...rest } = d;
  const more = Object.entries(rest).map(([k, v]) => `${k}: ${typeof v === "object" ? JSON.stringify(v) : v}`).join(", ");
  return `${reason}${more ? ` (${more})` : ""}`;
}
function showCandidates(title, chosenOps, candidates) {
  $("#route-title").textContent = title;
  const list = [...(candidates || [])].sort((a, b) => (b.chosen - a.chosen) || ((a.dropped ? 1 : 0) - (b.dropped ? 1 : 0)));
  const rows = list.map((c, i) => {
    const badge = c.chosen ? `<span class="badge chosen">chosen</span>`
      : c.dropped ? `<span class="badge dropped">dropped: ${esc(dropReason(c.dropped))}</span>`
        : `<span class="badge short">shortlisted</span>`;
    return `<div class="cand ${c.chosen ? "chosen" : ""}" data-i="${i}">${c.route.length} hops: ${path(c.route)} ${badge}</div>`;
  });
  const chosenTable = (chosenOps || []).map((o, i) =>
    `<tr><td>${i + 1}</td><td>${esc(o.provider)}</td><td>${esc(o.kind)}</td><td>${esc(o.from)} → ${esc(o.to)}</td><td>${esc(o.asset_in)}${o.asset_in !== o.asset_out ? ` → ${esc(o.asset_out)}` : ""}</td></tr>`).join("");
  $("#route").innerHTML = `${chosenTable ? `<table>${chosenTable}</table><hr>` : ""}
    <div class="hint">${list.length} candidate${list.length === 1 ? "" : "s"} weighed; hover one to see it on the graph.</div>${rows.join("")}`;
  document.querySelectorAll("#route .cand").forEach((el) => {
    const c = list[Number(el.dataset.i)];
    el.addEventListener("mouseenter", () => c.route.forEach((o) => cy.getElementById(o.edge).addClass("hover")));
    el.addEventListener("mouseleave", () => cy.edges().removeClass("hover"));
  });
}

// ---- Summaries of what each call answered ---------------------------------------------------------------------

function quoteSummary(q) {
  const shown = q.quote || {};
  const segs = shown.segments || [];
  const assetIn = segs[0]?.from?.asset?.id;
  const assetOut = segs.at(-1)?.to?.asset?.id;
  const rows = segs.map((s) => `<tr><td>${esc(s.provider)}</td><td class="n">${money(s.amount_in, s.from?.asset?.id)}</td>
    <td class="n">fee ${money(s.fee?.amount, s.from?.asset?.id)}</td><td class="n">${money(s.amount_out, s.to?.asset?.id)}</td></tr>`).join("");
  return `order <b>${esc(q.order)}</b>: ${q.route.length} segments, the router weighed ${q.candidates.length} candidate${q.candidates.length === 1 ? "" : "s"}.
    <table><tr><td>leaves the origin</td><td class="n" colspan="3">${money(shown.amount_in, assetIn)}</td></tr>${rows}
    <tr><td>operator's fee</td><td class="n" colspan="3">${money(shown.operator_fee?.amount, assetOut)}</td></tr>
    <tr><td><b>the customer receives</b></td><td class="n" colspan="3"><b>${money(shown.shown_out, assetOut)}</b></td></tr></table>
    <span class="hint">digest ${esc(String(q.digest).slice(0, 18))}…</span>`;
}
const segmentsSummary = (r) => {
  const rows = r.segments.map((s) => `<tr><td>${esc(s.provider)}</td><td>${s.outcome === "settled" ? `settled, gave out ${esc(s.shown_out)}` : `<b class="unverified">${esc(s.outcome)}</b>`}</td>
    <td>${s.receipt ? esc(s.receipt.shown) : ""}</td></tr>`).join("");
  return `${r.route} route: <table>${rows}</table>the order is <b>${esc(r.state)}</b>`;
};
const priceSummary = (o) => {
  if (!o.price) return `the order is <b>${esc(o.state)}</b>; no executed price yet`;
  const p = o.price.shown || {};
  const segs = p.segments || [];
  return `the order is <b>${esc(o.state)}</b>. Executed price, from what each segment did: ${money(p.amount_in, segs[0]?.from?.asset?.id)} in,
    <b>${money(p.shown_out, segs.at(-1)?.to?.asset?.id)}</b> out; recorded once as ${esc(o.price.receipt.shown)}`;
};

// ---- The acts -------------------------------------------------------------------------------------------------

const AMOUNT = "5000000";

async function quote(from, to, objective, title) {
  const q = await post("/quotes", { from, to, amount: AMOUNT, objective }, quoteSummary);
  showRoute(q.route, q.candidates);
  showCandidates(title || `${from} → ${to}, ${objective.replace(/_/g, " ")}`, q.route, q.candidates);
  await sleep(pace());
  return q;
}
async function run(order) {
  const r = await post(`/orders/${order}/run`, {}, segmentsSummary);
  await animateSegments(r.segments);
  await sleep(pace() / 2);
  return r;
}
const accept = (order) => post(`/orders/${order}/accept`, {}, (a) => `the customer accepts on the quote's digest; the order is <b>${esc(a.state)}</b>`);

async function act1() {
  story.act("Act 1 · Pesos in Colombia to dollars in the US, one exchange failing");
  story.say("50,000.00 COP from the customer's account at sim-co-bank to a dollar account at sim-us-bank. No provider does that alone: the route crosses sim-andina's ledger, a public chain and sim-liberty's ledger, converting twice.");
  const st = await get("/status", (s) => s.ready ? `Core is up, ${s.entries} log entries` : "Core isn't up yet");
  if ((st.used || []).includes("sim-andina-cop")) {
    story.note("An order already ran through sim-andina-cop in this Core. A second order through the same private reserve gets no evidence for its reversal (a known M3 finding), so Core is put together again first: about a minute.");
    await post("/reset", {}, (s) => `a new Core, seed ${s.seed}, ${s.entries} log entries`);
  }
  await post("/faults", { kind: "fail", provider: "sim-andina-exchange", operation: "conversion" }, (f) => esc(f.scripted));
  const q = await quote("sim-co-bank", "sim-us-bank", "lowest_total_cost");
  await accept(q.order);
  story.say("Core hands each segment to its provider in turn. The withdrawal settles; the exchange fails, and nothing more is handed over.");
  await run(q.order);
  const rev = await post(`/orders/${q.order}/reversal`, {}, (r) => `Core records the reversal owed: <b>${esc(r.owing.provider)}</b> owes ${esc(r.owing.shown)} back, in full. Asked, the value is shown back${r.word ? `, on ${esc(r.word)}` : ""}.`);
  if (!rev.completed) story.note("The reversal didn't complete: see the response.");
  story.say("A provider's own word never undoes its own failure: inside a private node the word counted is its publisher's, and the log labels it so.");
  const rc = await post(`/orders/${q.order}/recalculate`, {}, (r) => `Core recalculates from <b>${esc(r.from)}</b> to the same destination: ${r.route.length} segments, ${r.candidates.length} candidate${r.candidates.length === 1 ? "" : "s"} weighed`);
  showRoute(rc.route, rc.candidates);
  cy.getElementById(rc.from_node).addClass("resting");
  showCandidates(`recalculated from ${rc.from_node}`, rc.route, rc.candidates);
  story.say("The customer isn't asked again: the new route ends where they accepted.");
  await sleep(pace());
  await run(q.order);
  await get(`/orders/${q.order}`, priceSummary);
}

async function act2() {
  story.act("Act 2 · One request, two objectives, two routes");
  story.say("50,000.00 USDC from the customer's address on sim-polygon to one on sim-base, quoted under each objective. sim-bridge charges 0.10 USDC and takes 15 minutes; sim-bridge-fast charges 3.00 USDC and takes 20 seconds.");
  const cheap = await quote("sim-polygon-addresses", "sim-base-addresses", "lowest_total_cost", "cheapest");
  const fast = await quote("sim-polygon-addresses", "sim-base-addresses", "fastest_settlement", "fastest");
  clearMarks();
  showRoute(fast.route, fast.candidates);
  for (const op of cheap.route) if (!fast.route.some((f) => f.edge === op.edge)) cy.getElementById(op.edge).removeClass("faded").addClass("cand");
  story.say(`Same request, same graph, different edges: the cheapest goes by ${cheap.route.map((o) => o.provider).filter((p) => p.includes("bridge")).join(", ") || "?"}, the fastest by ${fast.route.map((o) => o.provider).filter((p) => p.includes("bridge")).join(", ") || "?"}. The customer takes the fastest.`);
  await sleep(pace());
  await accept(fast.order);
  await run(fast.order);
  await get(`/orders/${fast.order}`, priceSummary);
}

async function act3() {
  story.act("Act 3 · A stuck order, handed on by two people under policy");
  story.say("50,000.00 MXN from sim-mx-bank to USDC at the customer's address on sim-polygon: converted in sim-azteca's ledger, ramped onto sim-base and bridged back to sim-polygon, which the operator's own wallet holds. The last step will be refused its start.");
  await post("/holding", {}, (h) => `the operator's plain wallet <b>${esc(h.provider)}</b> (a real module, ${esc(h.version)}) holds ${esc(h.holds)}`);
  const q = await quote("sim-mx-bank", "sim-polygon-addresses", "lowest_total_cost");
  await accept(q.order);
  await post("/faults", { kind: "refuse_start", nth: q.route.length }, (f) => `planted at the platform's seam: ${esc(f.scripted)} (the deposit to the address)`);
  await run(q.order);
  const stuck = await post(`/orders/${q.order}/stuck`, {}, (s) => `no route is left: the order is <b>${esc(s.state)}</b>, ${esc(s.resting.shown)} resting at ${esc(s.resting.node)}`);
  cy.getElementById(stuck.resting.node).addClass("resting");
  const linked = await post(`/orders/${q.order}/linked`, {}, (l) => `the customer accepts linked order <b>${esc(l.linked)}</b> from ${esc(l.from)}; it starts only when the value is handed on`);
  showRoute(linked.route, linked.candidates, { keep: true });
  cy.getElementById(stuck.resting.node).addClass("resting");
  showCandidates(`linked order from ${linked.from}`, linked.route, linked.candidates);
  await sleep(pace());
  await post(`/orders/${q.order}/decide`, { by: "alice" }, (d) => `<b>alice</b> (desk A) decides to hand it on, citing a provider's kept statement: ${esc(d.outcome)}`);
  await post(`/orders/${q.order}/move`, { by: "alice" }, () => "", { expect: [403] })
    .then((m) => story.note(m.status === 403 ? "alice is refused: whoever decides can't also move it. Two people are needed." : "alice moved it: that shouldn't happen"))
    .catch((e) => story.note(`alice's move: ${e.message}`));
  await post(`/orders/${q.order}/move`, { by: "bob" }, (m) => `<b>bob</b> (desk B) carries it out: ${esc(m.carried_out.shown)}; the wallet's withdrawal record starts the linked order at ${esc(m.handed_on_at)}`);
  await run(linked.linked);
  await post("/notices", {}, (n) => `Core signs and sends ${n.sent} notices to the customer and the operator`);
  await get(`/orders/${linked.linked}`, priceSummary);
  story.say("When automation runs out, people step in, under the same policy and in the same log: who decided, on what evidence, who moved it, and that they were different people.");
}

async function epilogue() {
  story.act("Epilogue · Anyone can check the log");
  story.say("An hour later by Core's clock the log publishes a checkpoint. The folder an outside checker gets is written, and audit-check, the checker's own tool, runs over it as a separate program: no database, no network, no operator secret.");
  const a = await post("/audit", {}, (r) => `${r.entries} entries, ${r.checkpoints} checkpoints, ${r.kept} kept statements, ${r.decision_records} decision records written to <code>${esc(r.out)}</code>`);
  for (const c of a.checks) {
    story.html(`<div class="check ${c.as_told ? "good" : "bad"}"><b>${esc(c.name)}</b>: exit ${c.code}, ${esc(c.verdict)}
      ${c.note ? `<div class="hint">${esc(c.note)}</div>` : ""}<details><summary>audit-check said</summary><pre>${esc(c.output)}</pre></details></div>`);
  }
  story.say(a.as_told ? "The history holds against its checkpoints, and the rewritten copy is caught: the operator can't rewrite what the log says without every checker who holds a checkpoint noticing." : "The outside check didn't come out as the story says: see each check above.");
}

const ACTS = { 1: act1, 2: act2, 3: act3, 4: epilogue };
async function play(which) {
  const list = which === "all" ? [1, 2, 3, 4] : [Number(which)];
  for (const n of list) {
    try {
      await ACTS[n]();
    } catch (e) {
      const li = story.html(`<b class="unverified">The act stopped:</b> ${esc(e.message)}<div class="retry"><button>Reset Core and replay this act</button></div>`);
      $("button", li).addEventListener("click", async () => {
        try { await post("/reset", {}, (s) => `a new Core, seed ${s.seed}`); clearMarks(); await play(String(n)); } catch (err) { story.note(err.message); }
      });
      return;
    }
  }
}

// ---- Try it yourself ------------------------------------------------------------------------------------------

let tried = null;
function refreshTryButtons() {
  $("#try-accept").disabled = busy > 0 || !tried || tried.accepted;
  $("#try-run").disabled = busy > 0 || !tried || !tried.accepted;
}
function setupForms() {
  const f = $("#quote-form");
  f.from.innerHTML = net.origins.map((o) => `<option>${esc(o)}</option>`).join("");
  f.to.innerHTML = net.destinations.map((o) => `<option>${esc(o)}</option>`).join("");
  f.to.value = "sim-us-bank";
  f.addEventListener("submit", async (ev) => {
    ev.preventDefault();
    const amount = toUnits(f.amount.value);
    if (!amount) { story.note("amount: a number with at most two decimals"); return; }
    story.act(`Your order · ${f.from.value} → ${f.to.value}`);
    try {
      const q = await post("/quotes", { from: f.from.value, to: f.to.value, amount, objective: f.objective.value }, quoteSummary, { expect: [422] });
      if (q.status === 422) {
        clearMarks();
        showRoute([], q.candidates || []);
        showCandidates("no route: every candidate dropped", [], q.candidates || []);
        tried = null;
      } else {
        showRoute(q.route, q.candidates);
        showCandidates(`${f.from.value} → ${f.to.value}`, q.route, q.candidates);
        tried = { order: q.order, accepted: false };
      }
    } catch (e) { tried = null; }
    refreshTryButtons();
  });
  $("#try-accept").addEventListener("click", async () => {
    try { await accept(tried.order); tried.accepted = true; } catch (e) { /* shown */ }
    refreshTryButtons();
  });
  $("#try-run").addEventListener("click", async () => {
    try {
      const r = await run(tried.order);
      if (r.segments.some((s) => s.outcome === "failed")) story.note("A segment failed: POST /orders/{id}/reversal, then /recalculate and /run, recover it, as act 1 does.");
      await get(`/orders/${tried.order}`, priceSummary);
    } catch (e) { /* shown */ }
    tried = null;
    refreshTryButtons();
  });
  const g = $("#fault-form");
  g.provider.innerHTML = [...new Set(net.edges.map((e) => e.provider))].map((p) => `<option>${esc(p)}</option>`).join("");
  g.addEventListener("submit", async (ev) => {
    ev.preventDefault();
    try { await post("/faults", { kind: g.kind.value, provider: g.provider.value, operation: g.operation.value || undefined }, (r) => esc(r.scripted)); } catch (e) { /* shown */ }
  });
}

// ---- The log --------------------------------------------------------------------------------------------------

let next = 0;
let polling = false;
async function pollLog() {
  if (polling) return;
  polling = true;
  try {
    const res = await fetch(`/log?since=${next}`);
    if (!res.ok) return;
    const page = await res.json();
    if (page.since < next) { next = 0; $("#log tbody").innerHTML = ""; }
    const tbody = $("#log tbody");
    for (const e of page.entries) {
      const tr = document.createElement("tr");
      tr.className = "fresh";
      tr.dataset.text = e.line.toLowerCase();
      const sig = e.kind === "stated" ? (e.signature === "UNVERIFIED" ? `<span class="unverified">UNVERIFIED</span>` : esc(e.signature)) : "";
      tr.innerHTML = `<td>${e.index}</td><td class="k ${e.kind}">${e.kind}</td><td>${esc(e.label)}</td><td>${esc(e.who)}</td>
        <td>${esc(e.order ?? "")}</td><td>${esc(e.provider ?? "")}</td><td>${sig}</td>`;
      tr.title = e.line;
      if (e.provider) {
        tr.addEventListener("mouseenter", () => cy && cy.edges().filter((x) => x.data("provider") === e.provider).addClass("hover"));
        tr.addEventListener("mouseleave", () => cy && cy.edges().removeClass("hover"));
      }
      filterRow(tr);
      tbody.append(tr);
      setTimeout(() => tr.classList.remove("fresh"), 1500);
    }
    next = page.next;
    if (page.entries.length && $("#log-follow").checked) $("#log").scrollTop = $("#log").scrollHeight;
  } catch (e) { /* the server is busy or gone; the next poll tries again */ } finally { polling = false; }
}
// ---- The network's transitions, from the API's database ------------------------------------------------------

let worldRun = null;
let worldNext = 0;
let worldPolling = false;
async function pollWorld() {
  if (worldPolling) return;
  worldPolling = true;
  try {
    const res = await fetch(`/world/transitions?since=${worldNext}${worldRun ? `&run=${encodeURIComponent(worldRun)}` : ""}`);
    if (!res.ok) return;
    let page = await res.json();
    const st = await (await fetch("/status")).json();
    const current = st.run?.run ?? null;
    $("#world-store").textContent = st.store ?? "?";
    if (current && current !== worldRun) {
      // A new run (Core was put together again): start the table over.
      worldRun = current; worldNext = 0; $("#world tbody").innerHTML = "";
      page = await (await fetch(`/world/transitions?since=0&run=${encodeURIComponent(worldRun)}`)).json();
    }
    $("#world-run").textContent = worldRun ?? "none yet";
    const tbody = $("#world tbody");
    for (const t of page.transitions || []) {
      const tr = document.createElement("tr");
      tr.className = `fresh ${t.dry_run ? "dry" : ""}`;
      tr.dataset.dry = t.dry_run ? "1" : "";
      const move = [t.from_node, t.to_node].filter(Boolean).join(" → ");
      const amount = t.amount_in ? money(t.amount_in, t.asset_in) + (t.asset_out && t.asset_out !== t.asset_in ? ` → ${t.asset_out}` : "") : "";
      const bad = !/^(answered|recorded)$/.test(t.outcome);
      tr.innerHTML = `<td>${t.seq}</td><td>${esc(t.piece)}</td><td>${esc(t.kind)}${t.dry_run ? " (dry run)" : ""}</td><td>${esc(t.provider ?? t.network ?? "")}</td>
        <td>${esc(move)}</td><td>${esc(amount)}</td><td>${esc(t.order_id ?? "")}</td><td class="${bad ? "bad" : ""}">${esc(t.outcome)}</td><td>${esc(t.world_at ?? "")}</td>`;
      tr.dataset.text = JSON.stringify([t.piece, t.kind, t.provider, t.network, t.order_id, t.outcome, move]).toLowerCase();
      tr.title = JSON.stringify(t.detail, null, 1).slice(0, 2000);
      if (t.provider) {
        tr.addEventListener("mouseenter", () => cy && cy.edges().filter((x) => x.data("provider") === t.provider).addClass("hover"));
        tr.addEventListener("mouseleave", () => cy && cy.edges().removeClass("hover"));
      }
      filterWorld(tr);
      tbody.append(tr);
      setTimeout(() => tr.classList.remove("fresh"), 1500);
    }
    worldNext = page.next ?? worldNext;
    if ((page.transitions || []).length) $("#world").scrollTop = $("#world").scrollHeight;
  } catch (e) { /* the next poll tries again */ } finally { worldPolling = false; }
}
function filterWorld(tr) {
  const q = $("#world-filter").value.trim().toLowerCase();
  tr.hidden = (q !== "" && !tr.dataset.text.includes(q)) || (tr.dataset.dry && !$("#world-dry").checked);
}

function filterRow(tr) {
  const q = $("#log-filter").value.trim().toLowerCase();
  tr.hidden = q !== "" && !tr.dataset.text.includes(q);
}

async function refreshStatus() {
  try {
    const s = await (await fetch("/status")).json();
    const el = $("#status");
    el.className = `status ${busy ? "busy" : s.ready ? "ready" : ""}`;
    el.textContent = s.ready ? `Core up · seed ${s.seed} · ${s.entries} log entries · ${s.checkpoints} checkpoints · ${s.orders.length} orders` : "Core not put together yet";
  } catch (e) { $("#status").textContent = "server unreachable"; }
}

// ---- Start ----------------------------------------------------------------------------------------------------

async function main() {
  if (typeof cytoscape === "undefined") {
    $("#graph").innerHTML = `<p style="padding:16px">The graph library (Cytoscape.js, from cdn.jsdelivr.net) didn't load. The API still answers; see <a href="/network">/network</a>.</p>`;
    return;
  }
  net = await (await fetch("/network")).json();
  drawGraph();
  setupForms();
  document.querySelectorAll(".acts button[data-act]").forEach((b) => b.addEventListener("click", () => play(b.dataset.act)));
  $("#reset").addEventListener("click", async () => {
    story.act("Reset");
    try { await post("/reset", {}, (s) => `a new Core, seed ${s.seed}, ${s.entries} log entries`); clearMarks(); } catch (e) { /* shown */ }
  });
  $("#clear-story").addEventListener("click", () => { $("#story").innerHTML = ""; });
  $("#log-filter").addEventListener("input", () => document.querySelectorAll("#log tbody tr").forEach(filterRow));
  const refilter = () => document.querySelectorAll("#world tbody tr").forEach(filterWorld);
  $("#world-filter").addEventListener("input", refilter);
  $("#world-dry").addEventListener("change", refilter);
  story.note(`${net.providers} providers make ${net.edges.length} edges between ${net.nodes.length} nodes. The router searches every path of at most ${net.max_hops} edges. Press an act to replay it through the API.`);
  await pollLog();
  await pollWorld();
  await refreshStatus();
  setInterval(() => { if (!busy) { pollLog(); pollWorld(); } }, 1500);
  setInterval(() => { if (!busy) refreshStatus(); }, 4000);
}
main();

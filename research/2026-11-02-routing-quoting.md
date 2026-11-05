---
name: Quoting, routing, rate graphs, manipulation and order books
status: informative
date: 2026-11-02
---

# Quoting, routing, rate graphs, manipulation and order books

## Basis

**Read on 2026-11-02, inside this project:** `CLAUDE.md`, `PRODUCT.md` (`PS-10`, `PS-14` to `PS-17`, `PS-21`, `PS-23`, `PS-31`), `SPEC.md` (`REQ-QTE-*`, `REQ-EXT-*`, section 8, `OQ-2`, `OQ-8`), and `research/2026-11-02-durable-state.md` (its free option and check-then-act points, which this report builds on and does not repeat).

**Read on 2026-11-02, outside this project:** only public external sources, listed under References with the version and date each shows. Nothing from the directories the brief excludes was opened, and no production system or admin tool was queried.

**How sources were read.**
- *Read directly as text:* FX Global Code (December 2024 PDF), GFXC last look report (August 2021 PDF), SEP-38 (raw Markdown), the Nexus pages cited except the rates page, which went through the digest tool (raw Markdown), Interledger RFCs 0018 and 0034 (raw Markdown), BOLT 7 (field definitions only), Pickhardt and Richter (PDF text). Repository licence and last-push dates came from the GitHub API on 2026-11-02.
- *Read through a fetch tool* that returns a model-written digest, so a digest can misread its page: most other web pages.
- *(search excerpt)* marks a claim taken only from a search result's excerpt, whose page I did not open. Treat those as leads.
- A digest of one documentation page (LND pathfinding) reported that the page carries instructions addressed to AI agents. I did not act on them, and nothing here relies on them.
- Direct quotation is limited to the owner's words and one short phrase from Pickhardt and Richter. Everything else is paraphrase; field names are given as written.

**Tags.**
- **Source:** the cited document says this.
- **Measured:** the cited document reports this as a measurement, under the conditions given.
- **Inference:** my reasoning from sources. It is not stated by any of them.

This report decides nothing. It names no winner, writes no requirement, and does not define route or graph for the owner (`oq-route-meaning`). Vendors are described generically.

**Orchestrator's review, 2026-11-02.** Checked at the source: SEP-38 v2.5.0's status, held reserve and KYC gate; Nexus's 600-second honour rule and its stated trade-off; the NYDFS USD 150 million action. All matched. The LND page's static HTML shows only a pointer to its `llms.txt` index, nothing that directs an agent, and nothing here relied on it. Everything else rests on this report's own tags.

## Findings that matter most

1. **"Firm" is a spectrum, and every firm quote checked has conditions.** One holds capacity until expiry. One is voided when the market drifts past a threshold and falls back to a market rate when settlement outlasts the lock. One is honoured for a fixed window whatever the provider does next. Some leave the provider a last look. The difference is who bears the move, at what price, for how long. A route across rails makes this harder, because no single window covers both a seconds-long leg and a days-long leg. (Source and Inference, sections 1 and 3.)
2. **Rates and fees alone do not determine a best route.** `PS-16` and `PS-17` name rates and fees. The systems checked also carry per-hop limits, minimums, expiry budgets and settlement time, and those break the tidy graph algorithms: arbitrage search on rates is easy, but fixed fees and resource limits make the general problem NP-hard or mixed-integer. So "best" needs a stated objective, and the catalogue needs to expose more than `PS-16` and `PS-17` say today. (Source and Inference, sections 2 and 3.)
3. **A self-dealing or stale route cannot be detected from the chosen route alone.** Detection in every market examined depends on what was *not* chosen, why it was dropped, a reference price, and a trustworthy clock. FX conduct norms already ask for reasons behind rejections to be recorded. A router that can leave out a candidate, or restate a quote, leaves no trace unless the provider's own statement is kept. (Source and Inference, section 4.)

## 1. Quotes

### Forms a quote takes

| Form | What binds the provider | Who bears the move before execution | Example |
|---|---|---|---|
| Indicative price | Nothing | Customer | SEP-38 `GET /price`; a DEX aggregator's price endpoint *(search excerpt)*; FIX `QuoteType` defaults to indicative *(search excerpt)* |
| Firm with held capacity | Amount held in reserve until expiry | Provider, inside its margin | SEP-38 `POST /quote` |
| Lock priced by duration, void on drift | Rate, unless the market moves past a threshold | Provider up to the threshold, then customer | A card processor's FX quote API |
| Scheme-enforced honour window | Any quote issued, for a fixed time, even after the rate changes or is withdrawn | Provider | Nexus, 600 seconds |
| Firm with last look | Provider may still accept or reject | Customer, if rejected | Principle 17 practice |
| Commitment with short validity | Sender's institution commits to deliver | Sender's institution | Open Payments quote |
| Bounded order, no quote commitment | Nothing; execution fails past the customer's bound | Customer up to the bound | Stellar `destMin` and `sendMax`; STREAM per-packet minimum; a quote's `toAmountMin` |
| Auction | A signed limit; the price is what competition finds | Filler or solver inside the auction | UniswapX Dutch orders; CoW batches |

Sources:
- **SEP-38 v2.5.0**, Draft, created 2021-04-09, updated 2025-02-26 ([spec](https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0038.md)). Indicative prices are explicitly not the final conversion price. A firm quote's amount must be held in reserve and left out of later quotes until `expires_at`, which is the time by which the provider must receive funds. The client may propose `expire_after`. Quotes must remain fetchable past expiry. To stop bad actors reserving all capital, servers should only quote to identified, KYC'ed parties. The text read has `decimals` but no minimum or maximum amount field.
- **A card processor's FX quote API**, preview version `2026-09-30.preview` ([guide](https://docs.stripe.com/payments/currencies/localize-prices/fx-quotes-api.md)). Lock durations are none, five minutes, one hour or one day, priced at 0.07% to 0.30% depending on duration and currency group. A lock is invalidated at 3.5% drift for payments and 1% for transfers. Using an expired lock is refused. Non-card methods can outlast the 24-hour lock, and those are then processed at the mid-market rate.
- **Nexus** ([quotes](https://docs.nexusglobalpayments.org/fx-provision/quotes.md), [rates](https://docs.nexusglobalpayments.org/fx-provision/rates-from-third-party-fx-providers)). A provider must honour a quote for at least 600 seconds. The payment is rejected if the instruction arrives later. A withdrawn rate still honours quotes already issued. The authors state the trade-off directly: a longer window would mean worse rates. Nexus is a published scheme specification and not a live service; go-live is targeted for 2027 and the operating company was formed in 2025 *(search excerpt)* ([MAS](https://www.mas.gov.sg/publications/annual-report/2026/annual-report-2025-2026/innovative-and-inclusive-financial-centre/payment-innovation)).
- **Open Payments**: a quote is a commitment by the sender's institution to deliver an amount and is valid for a limited time; the page gives no duration ([resources](https://openpayments.dev/concepts/resources/), last reviewed 2026-11-02). Rafiki, one implementation, defaults to a 5-minute quote lifespan, a 15-second rate lifetime and 1% accepted rate fluctuation ([backend config](https://rafiki.dev/integration/deployment/services/backend-service/)). These are configuration defaults, not measurements.
- **Mojaloop**: the FX provider quotes, the payer's institution accepts, and conversion and transfer terms are committed together ([FX feature, updated 2025-03-14](https://docs.mojaloop.io/product/features/fx.html)). Only payer-side conversion is implemented. Quote expiry is carried in the message header and enforced by the provider, the amounts already include fees, and the terms are carried as a signed condition *(search excerpt)* ([fxQuotes](https://docs.mojaloop.io/product/features/Iso20022/script/fxquotes_PUT.html)).
- **A money-transfer API**: `rateType` is `FIXED` or `FLOATING`, with separate `rateExpirationTime` and `expirationTime`, and quote statuses `PENDING`, `ACCEPTED`, `FUNDED` and `EXPIRED` *(search excerpt; the fetched page was truncated)* ([quote reference](https://docs.wise.com/api-reference/quote)).
- **FIX**: `ValidUntilTime` carries quote expiry and, for FX, the price should be all-in *(search excerpt)* ([FIX dictionary](https://fiximate.fixtrading.org/legacy/en/FIX.4.4/body_505583.html?find=BidPx)).
- **Bounded orders**: Stellar path payments carry `destMin` or `sendMax` and fail with `UNDER_DESTMIN` or `OVER_SENDMAX` ([operations](https://developers.stellar.org/docs/learn/fundamentals/transactions/list-of-operations)). STREAM senders set a minimum per packet, and receivers should not fulfil below it ([STREAM](https://interledger.org/developers/rfcs/stream-protocol/)). A cross-chain quote API gives `toAmountMin` as the guaranteed minimum ([API reference 1.0.0](https://docs.li.fi/api-reference/get-a-quote-for-a-token-transfer)).
- **Auctions**: [UniswapX overview](http://developers.uniswap.org/llms.mdx/docs/liquidity/uniswapx/overview); [CoW competition rules](https://docs.cow.fi/cow-protocol/reference/core/auctions/competition-rules).

Inference: what separates these is not the word "firm". It is four things a quote can state or stay silent on: what voids it, what it reserves, who checks expiry, and what the customer is told if it is not honoured. Nexus has a third party reject at expiry. In the bilateral designs read, the provider itself decides.

### Last look and FX conduct norms

The FX Global Code does not impose legal or regulatory obligations ([Code, updated December 2024](https://www.globalfxc.org/uploads/fx_global.pdf)). All Source, from Principle 17 unless noted:
- Last look is a final chance to accept or reject a trade request against the quoted price. The receiving party has sole discretion, which leaves the client with market risk on rejection.
- It should be used only as a risk control verifying validity and price. It should not be used to gather information with no intent to trade.
- Providers should disclose whether and how price changes in either direction affect the decision, the expected time, and the purpose.
- No trading that uses the request's information during the window, except a documented and disclosed cover-and-deal arrangement.
- Principle 12: quotes should be given with clear intent to trade, and prices for reference only should be labelled as such.
- Principle 14: mark-up should be fair and reasonable; clients should be told the price may include it, and that different clients may get different prices.
- Principle 13: both parties should understand how a reference price is set, and what happens if a third-party source is unavailable.

The GFXC's [last look report, August 2021](https://www.globalfxc.org/uploads/gfxc_report_last_look-1.pdf) goes further. All Source:
- Delay beyond what the checks need is contrary to Principle 17.
- Providers should disclose symmetric or asymmetric price checks, and the maximum and minimum window.
- Rejections should be recorded, with at least a high-level reason available trade by trade.
- Warning signs: long response times against those disclosed, high rejection rates with asymmetry, different response times for accepts and rejects, and a consistently strong market reaction after dealing with one provider.
- Firm venues without last look exist, and clients can choose.

Enforcement example. Source: in November 2015 NYDFS announced that Barclays would pay USD 150 million. The bank used last look as a general filter against unprofitable orders, the check was asymmetric, and clients received generic rejection messages ([NYDFS](https://www.dfs.ny.gov/reports_and_publications/press_releases/pr1511181)).

### Fees and rates in the quote

| Design | How rate and fee are expressed | Notable rule |
|---|---|---|
| SEP-38 | `price` without fees, `total_price` with fees, `fee.total`, `fee.asset`, optional `fee.details[]` | Details must sum to the total. Fees may be taken as margin in the price, as a deduction, or both |
| Card processor FX quote | `exchange_rate`, then `rate_details`: `base_rate`, `duration_premium`, `fx_fee_rate`, `reference_rate`, `reference_rate_provider` | Decomposes the price against a named external reference |
| Nexus | Final rate per quote, plus separately named deducted fees | Providers may not deduct from the payment, so all their margin sits in the rate. Tier and customer improvements are added before being applied, so they do not compound. Rounding is half-even |
| Cross-chain quote API | `feeCosts[]` with `included` flag, `gasCosts[]`, `executionDuration`, `includedSteps[]` | Summing sub-step fees onto the top level double-counts |
| DEX aggregator API | `fees` with protocol, integrator and gas parts, plus `minBuyAmount` *(search excerpt)* ([docs](https://docs.0x.org/docs/introduction/quickstart/swap-tokens-with-0x-swap-api)) | Slippage in basis points, 100 by default on the price endpoint |
| Interledger | No fee field; connectors apply a local rate ([ILPv4](https://interledger.org/developers/rfcs/interledger-protocol/)) | The cost is seen only end to end |

Rules that constrain disclosure, where they apply:
- Source: US remittance pre-payment disclosures require transfer amount, provider fees and taxes separately, total, exchange rate, covered third-party fees and total to recipient. The rate cannot be described as unknown or floating. Disclosures must be accurate when payment is made, and estimates are allowed only where the rule permits ([12 CFR 1005.31](https://www.consumerfinance.gov/rules-policy/regulations/1005/31/)).
- Source: EU rules require currency conversion charges to be shown as a percentage mark-up over the latest ECB reference rate before initiation. Regulation 2019/518 was repealed and codified into 2021/1230 *(search excerpt)* ([EUR-Lex](https://eur-lex.europa.eu/eli/reg/2019/518/oj/eng)).
- I have not assessed which of these apply to rutent or to its operators. That is a legal question for `reg-fintech`.

Scale of the problem, all secondary:
- World Bank: 6.49% average cost of remittances in its March 2025 issue, against a 5% G20 target *(search excerpt; the site returned 403)* ([RPW](https://remittanceprices.worldbank.org)).
- FSB: retail indicators barely moved since 2023, and satisfactory progress by 2027 is unlikely ([FSB, 2025-10-09](https://www.fsb.org/2025/10/g20-roadmap-for-cross-border-payments-consolidated-progress-report-for-2025/)). FXC Intelligence's summary of that report gives 35.4% of retail payments credited within one hour and 62.9% of services disclosing both cost and speed ([FXC](https://fxcintel.com/research/reports/ct-retail-payments-g20-cross-border-roadmap-2025)). The FSB PDF returned an error, so I did not check those against it.

### Who bears the move

Inference, from the table above:
- Someone always bears it. The live choices are the provider, priced in; the customer, as a bound or a re-quote; or an intermediary.
- A window has a price. The card processor charges more for longer locks, and Nexus says longer windows mean worse rates.
- Holding capacity is a cost and an attack surface. SEP-38 gates firm quotes behind KYC for exactly this reason. This links `PS-22` to `REQ-QTE-*`.
- A quote is also a check-then-act (see the durable-state report). Unless it reserves something, two routes can both rely on one provider's capacity. Nexus simply assumes that a provider with active rates holds sufficient funds, and may penalise failure ([liquidity](https://docs.nexusglobalpayments.org/fx-provision/managing-liquidity.md)).

## 2. Routing across many venues

### What the field means by route and graph

Laid out for `oq-route-meaning`. None is preferred here.

| Meaning | Lifetime | Who produces it | Example |
|---|---|---|---|
| Reachability entry: which peer leads to which destination | Until changed | Each node | Interledger routing tables, updated by broadcast ([RFC 0034](https://github.com/interledger/rfcs/blob/master/0034-connector-requirements/0034-connector-requirements.md)) |
| Priced edge advert: fee, limits, expiry budget | Until replaced | The edge's owner | Lightning `channel_update` ([BOLT 7](https://github.com/lightning/bolts/blob/master/07-routing-gossip.md)); a Nexus rate post |
| Path: ordered hops for one payment | One payment | Sender or server | XRPL path; Stellar path; a Lightning route |
| Path set or flow: several paths combined | One payment | Sender or server | XRPL path sets; Lightning multi-part flows; DEX split routes |
| Step plan: ordered actions with an estimate per step | One order | A quoting service | A cross-chain quote's `includedSteps` |
| Batch solution: one settlement across many orders | One batch | Competing solvers | CoW |
| Venue choice: where to send an order | One order, reviewed periodically | A broker | Order routing under best execution ([FINRA 5310](https://www.finra.org/rules-guidance/rulebooks/finra-rules/5310)) |
| Chain of parties | Relationship-based | Banks | Correspondent banking |

"Graph" has similar spread: a market graph of assets and rates ([Sedgewick and Wayne](https://algs4.cs.princeton.edu/44sp/)), a network graph of parties and channels (Lightning), a graph of beliefs about unknown capacity ([Pickhardt and Richter's uncertainty network](https://arxiv.org/pdf/2107.05322)), or the plan of steps drawn as a directed graph (Inference).

Inference: `PS-21`'s "posting a route —a.k.a. graph—" could mean any of these. It matters because an advert is long-lived and posted by a provider, while a per-order plan is short-lived and produced by whoever routes. The answer changes who posts (`OQ-8`), how often, and what audit volume follows.

### Systems compared

| System | What it optimises | How it handles stale data | Known failure modes |
|---|---|---|---|
| Lightning pathfinding | Fees plus an attempt cost divided by success probability (LND); or success probability and fees as a flow (Pickhardt and Richter) | Gossip gives timestamped fees and limits; balances stay unknown, so nodes learn from attempts and let the knowledge decay | Trial and error; a cheapest multi-part payment is NP-hard with base fees |
| Interledger | Nothing globally. Each connector chooses by local table and rate | Senders probe end to end with test packets | Rate changes mid-payment; a connector can reject on amount or liquidity |
| XRPL and Stellar | Server-side search for cheap paths, not guaranteed cheapest. Stellar considers paths of length 5 or shorter | Client-set bounds; XRPL can stream updated paths | Cost changes between submission and execution; exact offers known only after validation |
| DEX aggregators | Output net of gas and fees; convex when no fixed costs | Quote, then a slippage bound at execution | Reverts, sandwiching, collisions between trades |
| RFQ and intents | Price to the user, by auction or solver competition | Short auctions and deadlines | Quoters not filling, concentration, score inflation |
| Correspondent chains | No global optimiser found; relationship-based (Inference) | End-to-end tracking after the fact | Delay at the final bank; compliance holds; operating hours |
| Nexus | Sender's institution picks from a returned list of provider quotes | Fixed honour window | Unknown limits at the destination fail only at delivery |

**Lightning.**
- Source: LND's model combines fee and attempt cost scaled by success probability. It records the latest result per node pair. The default estimator assumes a 60% hop probability and lets failures drift back toward it, with a one-hour half-life in the configuration shown ([LND](https://docs.lightning.engineering/lightning-network-tools/lnd/pathfinding)).
- Source: most-likely multi-part delivery is a convex min-cost flow solvable in polynomial time; the cheapest is NP-hard with base fees ([Pickhardt and Richter, 2021](https://arxiv.org/pdf/2107.05322)).
- Measured: on a graph snapshot taken from gossip, with channel balances split uniformly at random and base fees ignored, a single-digit number of rounds delivered payments close to the sender's balance. The exact algorithm took more than 30 minutes; a heuristic took about 6 seconds with under 1% deviation from optimal cost. In one worked case, 0.4 BTC needed four rounds and about 100 paths in round two. This is a simulation, and the authors say mainnet testing is still needed. Their "several orders of magnitude" improvement claim rests on it.
- Source, weak: a later post by the same author says the earlier liquidity model has flaws *(search excerpt)* ([Delving Bitcoin mirror, 2024-06-17](https://mirror.b10c.me/delvingbitcoin-org/2024-06-17-estimating-likelihood-for-lightning-payments-to-be-in-feasible-id973.html)).

**Interledger quoting and rate probing.**
- Source: connectors apply local rates and may use an order book or any other method. Quoting is left to applications, which can send test packets ([ILPv4](https://interledger.org/developers/rfcs/interledger-protocol/)). Errors include `F08` amount too large, `T04` insufficient liquidity, `R01` insufficient source amount and `R02` insufficient timeout.
- Source: senders should judge amounts only in their own units, because a receiver can manipulate destination units through a connector whose rate it controls ([STREAM](https://interledger.org/developers/rfcs/stream-protocol/)).
- Source: an author's account says source routing was dropped in 2016 because it does not scale with changing rates. A non-binding quoting protocol and liquidity curves were dropped in 2017 once test payments and small uniform packets were assumed ([Schwartz, 2018](https://emschwartz.me/simplifying-interledger-the-graveyard-of-possible-protocol-features/)). The quoting protocol (0008) and the connector-to-connector protocol (0010) now sit in the [deprecated folder](https://github.com/interledger/rfcs/tree/master/deprecated).
- Inference: probing works because a test payment is cheap, fast and harmless. Bank rails and ramps offer no such probe, so this technique may not transfer to a route with a fiat leg.
- I found no published measurement of probing cost or accuracy.

**XRPL and Stellar.**
- Source: XRPL path results change every few seconds, and a server may not find the cheapest path. A modified server could change results for profit, so the docs advise checking several ([paths](https://xrpl.org/docs/concepts/tokens/fungible-tokens/paths)).
- Source: Stellar's limits and failure codes as above.
- Inference: the party computing the route is a trust boundary even in open systems.

**DEX aggregators and split routing.**
- Source: routing across constant-function market makers without fixed costs is convex; adding fixed costs makes it mixed-integer. The same framework can detect arbitrage or certify that none exists ([Angeris et al., 2022](https://arxiv.org/abs/2204.05238); abstract read).
- Measured, by the vendor: over 673,000 trades of one aggregator's API on Ethereum, April to November 2021. About 50% had zero slippage against the quote, 17% positive and 33% negative. In a subset of over 5,000 trades on the vendor's own app, over 90% of those with slippage between 0.49% and 0.5% were flagged as sandwiched, using a naive heuristic. No revert rate is reported ([0x, 2022-04-14](https://0x.org/post/measuring-the-impact-of-hidden-dex-costs)).
- Source: a 2025 thesis comparing five aggregators finds no single best one; it depends on size, urgency and token *(search excerpt)* ([Beelen, CEU](https://www.etd.ceu.edu/2025/beelen_dennis.pdf)).

**RFQ and intent systems.**
- Source: UniswapX orders decay from a maximum price toward a minimum, and fillers compete ([overview](http://developers.uniswap.org/llms.mdx/docs/liquidity/uniswapx/overview)). On mainnet an RFQ winner gets about 24 seconds of exclusivity. A quoter who wins and does not fill is a "fade" and gets cooled off, from 15 minutes upward, with growth on repeat *(search excerpt)* ([filler FAQ](https://developers.uniswap.org/docs/liquidity/uniswapx/filling/faq)).
- Source: in CoW, solvers bid in a batch auction. The contract enforces limit prices and solver whitelisting; orders on the same pair and direction get one price; deadlines are set in blocks, 3 on mainnet. Prices at least as good as baseline liquidity (EBBO), bans on score inflation and shifting surplus are social rules enforced by monitoring, with possible slashing ([rules](https://docs.cow.fi/cow-protocol/reference/core/auctions/competition-rules)).
- Measured: on Ethereum blocks 18,000,000 to 19,164,987, about August 2023 to February 2024, for USDC-WETH and PEPE-WETH, against a simulated top-of-block AMM trade net of gas and fees ([Yuminaga et al., 2025](https://arxiv.org/html/2503.00738v3)):
  - USDC-WETH against V2 is positive, above about 100 bps over roughly 100 ETH.
  - Against V3 it is near zero overall, with solver venues differing at larger sizes.
  - Markouts against a large centralised exchange are negative, narrowing with size.
  - Two solvers account for over 90% of one venue's volume, and three for about 80% of another.
- Measured, by an interested party: UniswapX added almost 5.3 bps to Uniswap's interface on WETH/USDC. The post also says 4.7 bps without explaining the difference, gives no sample size or window, and compares its publisher with a competitor ([Uniswap Labs, 2024-05-01](https://blog.uniswap.org/measuring-price-improvement-with-order-flow-auctions)).

**Correspondent payment routing.**
- Measured: on the bank-messaging network's tracked payments, the median end-to-end time is under two hours, ranging from under five minutes on the fastest routes to over two days on several of the slowest. Most added time is at the beneficiary bank. The average is about one intermediary, and each extra one adds limited time. Time zones show no discernible effect. Causes include capital controls and compliance, weak competition, limited hours and batch processing ([CPMI brief, 2022-02-08](https://www.bis.org/cpmi/publ/swift_gpi.htm)). Coverage is 48% of volume and 56% of value *(search excerpt)*. The page does not give the sample period.
- Source: I read only the landing page of the [2016 CPMI report](https://www.bis.org/cpmi/publ/d147.htm).
- Inference: hop count is a poor proxy for delay here. The last party's hours and compliance dominate, which is also a warning about ranking routes by hops.

**Graph algorithms.**
- Source: a profitable cycle is one whose rate product exceeds one. Taking negated logarithms turns it into a negative cycle, which Bellman-Ford finds in time proportional to edges times vertices ([Sedgewick and Wayne](https://algs4.cs.princeton.edu/44sp/)).
- Measured: 292,606 cyclic arbitrages worth over USD 138 million on Uniswap V2 in eleven months, with the best unexploited opportunity staying above roughly 1 ETH ([Wang et al., v3 2022](https://arxiv.org/abs/2105.02784); abstract read).
- Source: shortest paths with resource constraints are strongly NP-hard, and label-setting with Pareto dominance is the standard exact method *(search excerpt)* ([Boost](https://www.boost.org/latest/libs/graph/doc/r_c_shortest_paths.html)).
- Source: RAPTOR computes Pareto-optimal journeys in rounds, one per transfer, and McRAPTOR adds more criteria. Conditional fares make the problem NP-hard *(search excerpts)* ([Delling et al.](https://i11www.iti.kit.edu/extra/publications/dpw-rbptr-12.pdf), [Euler et al.](https://arxiv.org/pdf/2204.01326)).
- Inference:
  - The logarithm trick assumes a rate that does not depend on amount. Depth, tiers, fixed fees, minimums and limits all break that, so an edge's cost is really a function of the amount.
  - With time and cost both in play there may be no single best route, only a set of trade-offs someone must choose from. A public-transit journey planner is a closer analogy than a currency arbitrage solver.
  - A negative cycle in rutent's rate graph could mean a real opportunity, a stale quote, or a fake one. It is a data-quality signal as much as an algorithmic result.
  - If the catalogue is small, the hard cases may not bite: exhaustive search within a hop limit could be sufficient. I found no source on this at rutent's likely scale, because the scale is unknown (`ASM-1`).

### How route quality is measured afterwards

| Measure | What it needs | Weakness | Source |
|---|---|---|---|
| Implementation shortfall: paper result against real result, including delay | Decision-time price | Paper price is an unattainable ideal | Perold, 1988 *(search excerpt)* ([course notes](https://pages.stern.nyu.edu/~jhasbrou/Teaching/POST%20Draft%20syllabus/OLDpowerpoints/transactionCostAnalysis.pdf)) |
| Quote against realised, plus share of tolerance used | Quote, bound and outcome per trade | Ignores routes not taken | 0x, above |
| Counterfactual welfare: outcome against a simulated alternative | A model of the alternative | Answer depends on the baseline: V2 and V3 gave opposite pictures | Yuminaga et al., above |
| Markout against an external market | A trusted reference | Reference may be stale or unavailable | Yuminaga et al.; FX Code Principle 13 |
| Periodic best-execution review: price improvement and disimprovement, likelihood, speed, size, cost, needs, order-flow payments | Comparable venue statistics | Reviewer may be conflicted | [FINRA 5310 .09](https://www.finra.org/rules-guidance/rulebooks/finra-rules/5310), quarterly, with routing changed or differences justified |
| Reject and response-time analysis | Per-request timing and reasons | Needs provider data | GFXC report, above |
| Reliability: fade or failure rate per provider | Accept and fill records | Can be gamed by selective quoting (Inference) | UniswapX FAQ, above |

Inference: every benchmark needs something rutent would have to record at decision time, and for local-currency corridors a shared reference price may not exist. Delivered amount, settlement time and failure rate can be measured without one.

## 3. Graph and rate data

### Where the data comes from when providers are independent

| Source of data | Who initiates | Freshness | Example |
|---|---|---|---|
| Provider posts a rate or edge | Provider | Valid until replaced | Nexus `POST /rates/`, one-directional, per provider and agent-account pair; Lightning `channel_update` |
| Quote on request | Requester | Valid to its expiry | SEP-38; Mojaloop; UniswapX RFQ |
| Read from public state | Anyone | Per block or ledger | AMM pools; XRPL and Stellar order books |
| Inferred from attempts | Sender | Decays | LND mission control; Interledger test packets |
| Declared rules and limits | Operator or scheme | Slow | Nexus value limits via `GET /countries/`; Lightning `htlc_minimum_msat` and `htlc_maximum_msat` |

Inference: only the first and last suit a graph built ahead of any order. Pricing the second kind means asking providers per order, which adds latency and hands them order intent (section 4). Most real designs mix them: Nexus has a posted base rate with a per-request quote.

Other points:
- Source: Nexus gives a provider the current best rate for a corridor without naming who offers it. Providers cannot call the endpoint showing competitors' rates, and a party that is both must keep the two sets of credentials apart ([revenue model](https://docs.nexusglobalpayments.org/fx-provision/revenue-model-for-fxps.md)).
- Inference: `PS-31` makes reserve capacity unobservable by design, much like Lightning's unknown balances. An engine cannot know a route has capacity until a firm answer or an attempt, and repeated yes-or-no probing is the inference risk `SPEC.md` already lists.

### Multi-hop effects

Inference unless tagged; symbolic because no sourced figures apply to rutent.

- **Compounding.** With proportional fee `p_i` and rate `r_i`, output is the input times the product of `r_i(1 - p_i)`. A fixed fee at hop `k` is then converted through every later hop, so its effect depends on where it sits and on the amount. Costs cannot simply be added across hops.
- **Direction.** These five designs support both fixed-send and fixed-receive: Nexus, Open Payments, Mojaloop, SEP-38, and Stellar's two operations (Source). Fixed-receive means computing backwards through fees and limits, which need not give the same route as forward.
- **Limits at each hop, in each hop's currency.**
  - Source: Nexus applies the lower of two systems' limits at the rate of the specific quote. Limits set by institutions are unknown to it, so an over-limit payment is rejected only at delivery ([value limits](https://docs.nexusglobalpayments.org/payment-processing/maximum-value-of-a-nexus-payment.md)).
  - Source: Interledger's `F08` and `R01` carry too-large and too-small outcomes back to the sender.
- **Minimums.** After a large fee, a small amount can fall under a later hop's minimum, so a route valid at one size is invalid at another.
- **Rounding.** Each hop rounds, and in which direction matters. Nexus fixes half-even (Source). SEP-38 has a `decimals` field per asset and requires its price identity to hold within them (Source).
- **Expiry budget.**
  - Source: each Interledger connector shortens the expiry, and `R02` reports when too little is left.
  - Source: Lightning advertises `cltv_expiry_delta`.
- **Slippage scope.** Source: one API offers per-step slippage by default and a route-wide budget only on EVM routes with at most one swap. Inference: a whole-route bound is hard to give across parties that do not coordinate.
- **Double counting.** Source: that same API warns against adding sub-step fees to top-level fees.
- **Reliability compounds.** Success across hops is roughly the product of each hop's success, when independent. Source, weak: a Lightning developer makes the same point *(search excerpt)* ([lightning-dev, 2022-05](https://diyhpl.us/~bryan/irc/bitcoin/bitcoin-dev/linuxfoundation-pipermail/lightning-dev/2022-May/003587.txt)).
- **Griefing held capacity.** Source: Interledger's working draft warns of attackers tying up liquidity with payments they know will fail. It suggests limits on what any party may hold, and refusing parties with high failure rates ([RFC 0018](https://github.com/interledger/rfcs/blob/master/0018-connector-risk-mitigations/0018-connector-risk-mitigations.md)).

### Routes across rails with very different settlement times

Evidence:
- Source: a card processor's lock can be shorter than some non-card methods, and it falls back to the mid-market rate. So the customer's promise changes when the rail is slow.
- Source: instant systems run 24/7 and wholesale FX markets do not. Nexus providers either freeze rates over the weekend and price in the Monday gap, or keep adjusting to manage liquidity. It also tolerates temporary exceptions for smaller providers without 24/7 capacity.
- Measured: under five minutes to over two days on the same network (CPMI, above).
- Measured: about USD 2.2 trillion a day of FX turnover was exposed to settlement risk in April 2022, 31% of deliverable turnover. Payment-versus-payment mitigates it, but is unavailable, unsuitable or too costly for some ([BIS Quarterly Review, 2022-12-05](https://bis.org/publ/qtrpdf/r_qt2212i.htm)).
- Source, modelled not observed: a cross-chain atomic swap is a free American call option, worth roughly 2% to 3% of value for crypto against about 0.3% for stocks and fiat ([Han, Lin and Yu, AFT 2019](https://eprint.iacr.org/2019/896)).

Inference:
- A route has an exposure window: from the first irreversible leg to the last leg priced. The rate risk across it is not the same as the sum of each quote's risk.
- There are only a few things to do with that window. Price it into one quote covering all of it; quote only the next leg and re-quote; give the customer a bound; have someone pre-fund the slow leg; or reorder so the slow or irreversible part is settled before the next price is fixed. Each moves risk rather than removing it, and some may be ruled out by disclosure rules that forbid "floating" rates.
- Slower legs make capital a cost that a per-hop fee does not show. A pre-funded path may be fast because someone holds inventory, and that holder then bears the rate risk.
- Reachability varies by calendar. An edge's availability, rate and settlement time can depend on the hour, weekend or holiday. A route computed on Friday afternoon can be wrong by Friday night.
- Time is a second cost axis and possibly the first one customers feel, which returns to section 2's point that no single "best" may exist.

## 4. Manipulation

| Abuse | What happens | Norm or evidence | What a record can show |
|---|---|---|---|
| Quote stuffing | Bursts of quotes or cancels to slow others or hide real interest | Principle 12; [UK MAR Art. 12(2)(c)](https://www.legislation.gov.uk/eur/2014/596/article/12) (the page notes a pending revocation); ratio of unexecuted orders to transactions is a regulated measure, RTS 9 *(search excerpt)* ([FIA](https://fia.org/articles/mifid-ii-rts-published-eu-official-journal)) | Per-party rates of quotes, cancels and quote-to-fill, against time |
| Fake liquidity | Quotes never meant to be honoured; fading; wash volume | Principle 12; [Cong et al.](https://nber.org/papers/w30783); UniswapX fade rule; CoW score-inflation ban | Accept and fill rates; refusals against market direction; statistical tests on sizes and prices |
| Stale-quote sniping | Taking a quote after the market has moved | [Budish et al., 2015](https://conference.nber.org/conf_papers/f69665.pdf) *(search excerpt)*; [Aquilina et al., 2022](https://www.nber.org/system/files/working_papers/w29011/w29011.pdf) *(search excerpt)* | Reference price at quote and at acceptance; acceptance time within window |
| Front-running by whoever sees the order | Trading or repricing on order information | Principles 11, 17; [Daian et al., 2019](https://arxiv.org/abs/1904.05234); GFXC warning signs | Market move after request; timing of a provider's other actions, if visible; who was shown what |
| Self-dealing router | Choosing a route for the router's benefit | FINRA interpositioning rule; [SEC 2020-321](https://www.sec.gov/newsroom/press-releases/2020-321) *(search excerpt)*; XRPL's server warning; Principle 8 on capacity | Chosen against candidates; affiliations; pay from providers; share routed over time |
| Hidden fee in a spread | Margin not visible as a fee | Principle 14; Reg E; EU rule; [BNY Mellon, 2015](https://www.justice.gov/archive/usao/nys/pressreleases/March15/BNYMSettlement2.php) *(search excerpt)* | Effective rate against a stated reference at decision time |
| Asymmetric refusal | Refusing only when the market moved against the provider | NYDFS case, GFXC report | Refusals and accepts conditioned on market direction and timing |
| Surplus capture | Keeping better-than-quoted proceeds | Governance posts on an aggregator's surplus *(search excerpt)* ([1inch forum](https://gov.1inch.network/t/remove-the-positive-slippage-fee-for-1inch-stakers/381)) | Quoted against delivered against provider's actual proceeds |
| Griefing | Holding quotes or liquidity with no intent to trade | SEP-38 KYC gate; RFC 0018 | Quote-without-order rates per party |

Evidence, with conditions:
- Measured, weakly: a dissertation by Egginton reports hundreds of quote-stuffing events a trading day, with over 74% of US exchange-traded securities seeing at least one episode in 2010 *(search excerpt; I could not re-find the page, so treat this as a lead)* ([listing](https://network.bepress.com/social-and-behavioral-sciences/economics/finance/page150)). The related paper is Egginton, Van Ness and Van Ness, Financial Management 45(3), 2016, seen only as a citation *(search excerpt)* ([CV](https://business.olemiss.edu/wp-content/uploads/2022/03/vannessR-2023.pdf)); I did not read it.
- Measured: wash trading averaged over 70% of reported volume on unregulated exchanges in a sample of 29, using first-significant-digit, size-rounding and tail-distribution tests, with regulated exchanges as the control ([Cong et al., NBER w30783; Management Science 2023](https://nber.org/papers/w30783)).
- Measured: latency-arbitrage races occur about once per minute per symbol on FTSE 100 stocks, at about half a tick each and about 20% of volume, costing about 0.5 bp, with top firms winning most *(search excerpt)* (Aquilina et al.).
- Source: the arbitrage is a property of continuous-time matching and batch auctions turn speed competition into price competition *(search excerpt)* (Budish et al.).
- Source: AMM liquidity providers' returns can be split into market risk and an alpha part, fees net of losses to arbitrageurs ([Milionis et al.](https://arxiv.org/abs/2208.06046); abstract read). Inference: this is a public-market analogue of the cost of leaving a stale quote exposed.
- Source: BNY Mellon agreed to USD 714 million across several resolutions in 2015 over a standing-instruction FX programme. The SEC said many trades were priced near the session's worst rates, while disclosures described best execution *(search excerpt)*.
- Source: in 2020 a US retail broker agreed to a USD 65 million penalty, without admitting or denying findings. The SEC found its order-flow payment rates led to worse prices costing customers USD 34.1 million net of commission savings *(search excerpt)*.
- Source: FINRA's standard covers reasonable diligence to find the best market, bars interposing a third party inconsistently with that, says the duty cannot be delegated, and lists order-flow payments among review factors (FINRA 5310).
- Source: Nexus separates credentials so quote data does not reach a party's treasury side.

### What makes these detectable from a record

Inference:
- **Counterfactuals must exist.** Most abuses are visible only against what could have happened. The route alone shows nothing.
- **Direction and timing matter more than rates.** Refusal rate alone is ambiguous, as GFXC says. Refusal correlated with market direction, or with response-time differences, is the signal.
- **An outside price is needed**, or an outside statement. A record with only insiders' numbers can be internally consistent and false. Principle 13's concern about third-party reference sources applies.
- **Attribution has to reach the provider.** A quote recorded only by the router can be altered or omitted by the router. If the provider's own signed statement is kept, restating it becomes detectable. Mojaloop's signed terms are one example of this shape *(search excerpt)*.
- **Omission stays hard.** This extends the durable-state report's point on completeness: a dropped candidate leaves no trace unless the provider also holds a receipt that it was asked or that it answered.
- **Statistics over time.** Fading, sniping and self-dealing show in distributions across many orders, not in one replay. Records need to be queryable in bulk, not only provable one at a time.
- **Concentration is itself a signal.** Two solvers at over 90% of one venue shows competition can be open on paper and thin in practice.

### What must be kept to replay why a route was chosen

Sources for the norm, then an Inference list.
- Source: FX Code Principle 36 asks for accurate records of orders and transactions, reasons behind rejections, and time-stamps granular enough to record both acceptance and execution.
- Source: RTS 6 Art. 28 requires each submitted order to be recorded immediately after submission, with updates kept, for five years ([UK-retained text](https://www.legislation.gov.uk/eur/2017/589/chapter/V/data.html); the page marks the regulation for revocation).
- Source: RTS 25 sets clock limits of 100 microseconds from UTC for high-frequency techniques and 1 millisecond for other activity ([annex](https://www.legislation.gov.uk/eur/2017/574/annex)).

Inference. To recompute a decision without calling providers, keep:
- the request as received, including direction, amount, asset pair, customer bounds, and who asked;
- the graph or data version used, with each input's age at decision time;
- every candidate considered, not only the winner, and why each was dropped: limit, stale, no capacity, cost;
- each quote as the provider stated it, its raw response, provider identity, received time and stated validity, plus any signature;
- quotes requested and not answered, and refused, with reason;
- the objective, its weights, the algorithm and code version, any tie-break rule, and any randomness;
- policy and allow-list state at that moment, and which provider module versions gave the data;
- what was shown to the customer and when they accepted;
- the reference price used, and where it came from;
- per-stage outcome: amount, fee, time, and any deviation from quote;
- the clock source and its known tolerance;
- each party's capacity and affiliation, as Principle 8 suggests.

Tension: `PS-30` and `PS-31` limit what may be disclosed. Some of this can be kept and proven without being published, as the durable-state report notes for Merkle structures.

Threat-model mapping, as a pointer for section 8 of `SPEC.md`: forged or restated quotes (spoofing, tampering), denying a quote or refusal (repudiation), order intent leaking (information disclosure), stuffing and griefing (denial of service), router or provider acting beyond its role (elevation of privilege).

## 5. Order books and an open protocol, as later extensions

### Matching models

| Model | How priority works | Requires | Example |
|---|---|---|---|
| Continuous book, price-time | Best price, then arrival | One sequence of arrivals | CME's FIFO; dYdX per its docs *(search excerpts)* |
| Pro rata or hybrid | Share of resting size at a price | Same, plus allocation rules | CME wiki, several years old *(search excerpt)* ([algorithms](https://www.cmegroup.com/confluence/display/EPICSANDBOX/CME+Globex+Matching+Algorithms)) |
| Frequent batch auction | Uniform price per short interval | A clock and a cutoff | Budish et al. *(search excerpt)* |
| Batch with competing solvers | Best bid for the batch; same-pair orders get one price | Monitoring of solvers | CoW |
| RFQ | Best answer to a request | Identified responders | SEP-38; RFQ on UniswapX |
| Dutch auction | First willing filler as the price falls | A curve and a deadline | UniswapX |
| Curve | Formula against pool | Pooled inventory | AMMs |
| Book in the ledger | Ledger order | Shared ledger | XRPL, Stellar |

Source *(search excerpt)*: dYdX keeps an in-memory book on every full node. Proposers match by price-time from their own view, books may differ across nodes at any moment, and matches are checked and voted by two-thirds of stake ([docs](https://docs.dydx.xyz/concepts/trading/limit-orderbook)). This is one published way to have a book without a single operator, on a shared chain that settles the result.

### What fast resolution by independent exchanges would really require

`PS-23` says order books "can be quickly resolved by the exchange providers". All below is Inference, except where tagged.

That phrase can mean different things. Each exchange could keep its own book while rutent routes to it. Orders could live in a shared view that exchanges cross among themselves. Or rutent could run the matching. These are very different products.

Whichever is meant, it needs:
- **Firm, addressable size.** A resting order is a standing firm quote. Unless capacity is reserved or the maker is otherwise answerable, it is a fake-liquidity problem by construction. `PS-31` makes reserve capacity unobservable, so some party has to vouch or credit.
- **A common sequence, or an honest statement that none exists.** Time priority across independent venues has no meaning without one. dYdX accepts diverging views and shares a chain. Rutent's providers do not share one.
- **Rules for trade-throughs.** Source: the US order protection rule, which bars trading through better prices on other exchanges, was proposed for rescission on 2026-06-11, citing complexity and fragmentation; I found no final action *(search excerpts)* ([Sidley](https://www.sidley.com/en/insights/newsupdates/2026/06/sec-proposes-rescission-of-the-order-protection-rule)). Inference: a cross-venue guarantee has been contentious even among regulated exchanges with consolidated data.
- **Speed that fits sandboxed modules.** `PS-20` puts provider code in a sandbox. Matching latency and quote updates at market speed may not sit well with that, and sniping rewards any latency gap.
- **Fast match is not fast settlement.** The match can be instant while the money rails are not. Either makers pre-fund, or someone extends credit, or matched trades can fail later. Settlement risk (section 3) sits here.
- **Cancel, modify and time-in-force rules** that are the same for all, because stuffing and spoofing live in those rules.
- **Market data fairness.** Who sees the book, and when. The arms-race figures in section 4 show what a speed gap can be worth.
- **Surveillance at message rates.** `PS-21` would make every order, cancel and fill a recorded act. Book message rates can dwarf order rates, so the audit path may become the limit.
- **Enough participants.** Source: 0x's relayer model drew criticism for fragmented liquidity, and later products moved toward aggregation and RFQ *(search excerpt, secondary; I found no official statement of why)* ([Delphi](https://members.delphidigital.io/reports/0x-improves-ux-meet-matcha), [Standard Relayer API](https://www.github.com/0xProject/standard-relayer-api)).

### What would have to be true earlier for this to stay possible

Inference. Stated as properties of an earlier design, not as mechanisms:
- A quote is a first-class thing with a maker, firmness, side, size, validity and conditions, and is not tied to one request. That lets a standing offer exist later.
- Partial fill is representable, even if v1 never does one.
- A route or order is not assumed to be one path, one taker and one maker, so many-to-many settlement is not ruled out.
- Matching or selection is separate from settlement, and the state machine does not assume they happen together.
- Providers can declare capabilities, so an exchange offering standing orders is an addition and not a new category.
- Identity, signing and attribution of a quote do not depend on the operator being the only poster.
- Audit records can hold cancels and unfilled offers, and their volume is not assumed to track order volume.
- Amounts, price units and rounding are fixed in a way that survives a shared book, where tick size and precision matter.
- Time and ordering have a stated source.
- Identifiers and message forms are versioned and documented so that an outsider could implement them, without internal details leaking.

### Open quoting and routing protocols

| Protocol | What it standardises | How it fared, on what I could check |
|---|---|---|
| FIX | Messages incl. quote request, quote, quote type, validity; tag `1142` match algorithm *(search excerpts)* | Long-lived; first added validity in FIX 4.0. I did not measure adoption. Standardises messages, not who may quote |
| Interledger (ILPv4, STREAM) | Packet, addressing, errors, stream | Dropped its quoting protocol, source routing, atomic mode and liquidity curves on the way to simplicity (Schwartz). Rafiki is active: last push 2026-11-02. I found no volume data |
| Open Payments | API for quotes, payments and grants | Docs reviewed 2026-11-02. No adoption data found |
| SEP-38 | Indicative and firm quote API, fee object, identity | Still Draft after more than four years, though at v2.5.0 and updated in 2025. Anchor Platform repo is active |
| Mojaloop FSPIOP | Quotes, FX quotes, conditional transfer, on a hub | Foundation names Rwanda, Liberia and The Gambia as live, self-reported *(search excerpt)* ([Foundation](https://mojaloop.io/building-durable-inclusive-instant-payment-systems-across-africa/)). Only payer-side conversion so far |
| Nexus | One connection to many instant systems, ISO 20022 messages, rates, quotes, fees | Handed to a company in 2025; targeted for 2027 *(search excerpt)*. Not live |
| BOLT 7 | Advertised fees, limits and expiry deltas | Three implementations are listed in section 6. Balance discovery stays outside it (Inference, from the fields read) |
| XRPL, Stellar | Order book and path payment in the ledger | Live. Tied to their own ledgers |
| ERC-7683 | A solver-facing interface over opaque orders | See below |

**A conflict to note on ERC-7683.** Excerpts from a vendor said the standard was ratified as Final with wide adoption. The [EIP page](https://eips.ethereum.org/EIPS/eip-7683) read directly lists it as Draft, created 2024-04-11, and says the earlier order structure and settler interfaces were removed because they were only superficially standard. It now standardises a resolver interface and leaves settlement, price determination, verification and solver selection to implementers. I follow the EIP page. I do not carry the adoption claims.

Inference across the table:
- Protocols that lasted standardised messages and failures, and left pricing and selection to participants. Both Interledger and ERC-7683 removed things after meeting practice.
- Each open system has a gate somewhere. SEP-38 wants KYC. CoW says anyone may build a solver, and also whitelists and bonds. Nexus onboards. "Open" here tends to mean an open interface with some vetting. `PS-22` already implies vetting.
- Adoption of one over others is not something this report can rank. I found no independent adoption figures for any of them.

## 6. Reusable libraries

Licence and last push from the GitHub API on 2026-11-02; a recent push says only that someone is committing. I did not evaluate quality or documentation beyond what is noted. "NOASSERTION" repositories had their licence file checked.

**I found no standalone, well-documented library that does multi-provider quoting or routing across fiat and on-chain.** One search for it returned only rate fetchers and clients to commercial APIs, plus one EVM-only crate *(search excerpt)*. That is a weak negative, not proof. `PS-10`'s reusable quoting engine appears to have no off-the-shelf equivalent.

| Library | What it is | Licence | Last push | Languages | Standalone? |
|---|---|---|---|---|---|
| [OR-Tools](https://github.com/google/or-tools) | Min-cost flow, routing and solvers | Apache-2.0 | 2026-11-02 | C++, with Python, C#, Java wrappers | Yes; knows nothing about money |
| [NetworkX](https://github.com/networkx/networkx) | Graph algorithms | BSD-3 | 2026-11-01 | Python | Yes |
| [petgraph](https://github.com/petgraph/petgraph) | Graph data structures and algorithms | Apache-2.0 or MIT | 2026-11-02 | Rust | Yes |
| [igraph](https://github.com/igraph/igraph) | Network analysis | GPL-2.0 | 2026-11-02 | C; Python binding | Yes; licence matters for `OQ-16` |
| [Boost Graph](https://www.boost.org/latest/libs/graph/doc/r_c_shortest_paths.html) | Includes resource-constrained shortest paths | Boost (Inference; not checked here) | Not checked | C++ | Yes |
| [rust-lightning (LDK)](https://github.com/lightningdevkit/rust-lightning) | Lightning implementation as a library, with routing | Apache-2.0 or MIT | 2026-11-01 | Rust | Library, but bound to Lightning's model; the GitHub repo says it is a mirror |
| [LND](https://github.com/lightningnetwork/lnd) | Node with pathfinding | MIT | 2026-11-02 | Go | A node, not a library |
| [Core Lightning](https://github.com/ElementsProject/lightning) | Node | BSD-MIT, with exceptions | 2026-11-02 | C | A node |
| [pickhardtpayments](https://github.com/renepickhardt/pickhardtpayments) | Min-cost-flow research code; includes an experiment-only oracle *(search excerpt)* | Apache-2.0 | 2023-05-03 | Python | Research; no push since 2023 |
| [Uniswap smart-order-router](https://github.com/Uniswap/smart-order-router) | Split routing with gas cost | GPL-3.0 | 2026-10-22 | TypeScript | Tied to its own protocol and chain RPC; licence matters |
| [balancer-sor](https://github.com/balancer/balancer-sor) | Smart order router | Not stated by the API | Archived | TypeScript | Archived; [b-sdk](https://github.com/balancer/b-sdk) (MIT) is the active SDK |
| [CoW services](https://github.com/cowprotocol/services) | Auction and order services | Apache-2.0 | 2026-11-02 | Rust | Services, not a library |
| [LI.FI SDK](https://github.com/lifinance/sdk) | Client for a hosted routing service | Apache-2.0 | 2026-10-29 | TypeScript | Client; the routing is hosted (Inference) |
| [Rafiki](https://github.com/interledger/rafiki) | Interledger service for wallets | Apache-2.0 | 2026-11-02 | TypeScript | Service |
| [Mojaloop quoting-service](https://github.com/mojaloop/quoting-service) | Hub quoting service | Apache-2.0 | 2026-10-28 | JavaScript | Service in a hub |
| [Anchor Platform](https://github.com/stellar/anchor-platform) | Implements Stellar anchor protocols | Apache-2.0 | 2026-11-02 | Kotlin | Service |
| [CCXT](https://github.com/ccxt/ccxt) | Unified client for over 100 crypto venues | MIT | 2026-11-02 | JS/TS, Python, C#, PHP, Go, Java, Rust | A library for reaching venues, not for routing |
| [Moneta](https://github.com/JavaMoney/jsr354-ri) | JSR 354 money and currency API | Apache-2.0 | 2026-10-02 | Java | Amount handling only |
| [Dinero.js](https://github.com/dinerojs/dinero.js) | Money arithmetic | MIT | 2026-11-01 | TypeScript | Amount handling only |

Inference:
- Three layers exist separately: graph and optimisation libraries, money arithmetic, and venue clients. The piece between them, which turns provider statements into a comparable priced graph with limits, time and validity, is the gap.
- Language reach is uneven. Nothing here is usable from many languages with one implementation, except by wrappers. Whether that matters depends on the owner, as in the durable-state report.
- Reusing a protocol router means importing its protocol's assumptions, such as chain state or gossip.

## 7. What this implies

### 7a. Capabilities the spec authors might state

No mechanism named. Each could be met in more than one way. Prompts, not requirements.

**A quote's content** (`REQ-QTE-*`, from `PS-16`, `PS-17`, `PS-21`)
- A quote states whether it binds, until when, for what amount and direction, and what would void it.
- Both a fixed amount sent and a fixed amount received can be asked for.
- Total delivered, each fee and the rate are separately visible, and fees already included are distinguishable from fees on top.
- Margin held in a rate can be measured against a stated reference where one exists, or the absence of one is stated.
- Rounding rules and units are stated.
- Reference prices are labelled as such, and the source is named.
- Whether a quote holds capacity is stated, and capacity cannot be tied up without limit by a party with no intent to trade.

**Who bears the move** (`REQ-QTE-*`, `REQ-ORD-*`)
- For each stage of a route, it is stated who bears a rate change, from when to when.
- A customer can set a bound on the outcome, and an order that would break it does not proceed.
- A customer can verify the outcome in their own units, without trusting intermediaries' figures.
- What the customer was told is consistent with any disclosure rule that applies, and can be shown later.
- Validity is checked at acceptance; a refusal gives a reason that distinguishes at least validity from price.

**Provider data** (`REQ-CAT-*`, `REQ-MDL-*`, from `PS-14` to `PS-17`)
- Beyond rates and fees, a provider can declare minimums, maximums, settlement time, hours of operation, and how long a stated figure stays true.
- Each figure used in a decision has a known age and a known origin.
- Routing can treat a figure as too old under a stated policy.
- Reserve balance privacy holds even under repeated queries from routing (`PS-31`).

**The engine as a component** (`REQ-MOD-*`, `REQ-QTE-*`, from `PS-10`)
- It runs with no live provider and no other rutent part, on supplied data alone.
- Given the same inputs and version, it gives the same result.
- The objective used is explicit. More than one criterion can apply, and the result can express a trade-off.
- Costs depend on the amount, so a route is evaluated at the actual amount and direction.
- Per-hop limits, fixed fees, minimums, expiry budgets and rounding are all honoured.
- The engine can say that no route fits the bounds, and why, rather than returning a poor one.
- Cycles of rates that imply free profit are surfaced rather than silently followed.
- A route's exposure window and estimated total time are outputs, not by-products.

**Evidence and replay** (`REQ-AUD-*`, from `PS-21`)
- A route's decision record lets a verifier recompute the choice without contacting a provider.
- Candidates not chosen, and why, are in that record.
- A provider's statement is attributable to that provider and cannot be restated by the router without detection.
- Requests that went unanswered, refusals and cancels are recorded, not only successes.
- The time source and its tolerance are stated.
- Records support bulk analysis across providers and time.

**Integrity** (`REQ-AUZ-*`, section 8, from `PS-16`, `PS-21`)
- Each party's role in a decision, and any affiliation between router and provider, is recorded and can be disclosed.
- Order information goes only to those who need it, and a provider cannot see competitors' quotes for a live order.
- Abusive posting of quotes or routes, including high cancel or low fill rates, is measurable per party and can be limited.
- Provider reliability, such as honouring rates and fill time, is measurable from records.
- Route quality is reviewed against a declared benchmark at a stated interval, and a material difference leads to a change or a recorded justification.

**Staying extensible** (`REQ-EXT-*`, from `PS-23`)
- The properties listed in section 5, as far as they can be tested without building the extension.
- An outside party could implement the quote and route interfaces from documentation alone.

### 7b. Candidate mechanisms for non-binding suggestions

For `suggestions/`. No ranking implied. Facts are sourced above; trade-offs are Inference.

**How firm a quote is**

| Candidate | For | Against |
|---|---|---|
| Fixed scheme-wide honour window (Nexus-style) | Simple; comparable across providers; checked by a third party | Providers price in the worst case; hard across rails; the window itself is a free option |
| Per-quote window with held capacity (SEP-38-style) | Customer certainty; a real claim on capacity | Costs capacity; griefing; needs identity gates |
| Lock priced by duration, void on drift | Risk is priced and visible; provider's tail is capped | Customer's certainty has an exception; fallback rule needs disclosure |
| Bounded order only | Provider need not hold anything; works without quotes | Failures replace bad prices; hard on fiat rails with no atomic failure |
| Auction or RFQ with fade penalty | Competitive, per order | Reveals intent to bidders; may concentrate; needs participants |
| Last look, disclosed and symmetric | Lets thin providers quote at all | Customer carries rejection risk; conduct norms apply; needs detailed records |

**Finding a route**

| Candidate | For | Against |
|---|---|---|
| Bellman-Ford on negative log rates | Well known; finds cycles | Assumes rates independent of amount and no resource limits |
| Exhaustive search under a hop limit | Exact and easy to explain and replay; Stellar's limit of 5 shows the shape | Grows fast; needs small catalogue; `ASM-1` is unknown |
| Multi-criteria labels, or round-based search | Gives real trade-offs; handles limits | NP-hard in the worst case; users must choose among results |
| Min-cost flow | Handles splitting; library support | Fixed fees break convexity; assumes splitting is allowed and useful |
| Convex optimisation over depth curves | Handles split and depth well | Needs curves providers may not publish; fixed costs make it mixed-integer |
| Competing external solvers | Puts the search on others; open extensibility | Monitoring, bonding, concentration, and new trust relationships |
| Simple ranked list of known corridors | Very easy to audit | Not really a graph; may not suit `PS-23` |

**Getting the data**

| Candidate | For | Against |
|---|---|---|
| Providers post rates and limits | Graph exists before any order; no intent revealed | Stale unless honoured; posting volume is an abuse surface |
| Quote per order | Fresh and specific | Latency; reveals intent; provider rate limits |
| Mixed: posted base plus per-order firm quote | Separates exploring from committing | Two paths to keep consistent; a posted rate may be a lure unless measured |
| Test payments | Measures reality | Not available on most rails; costs money; touches `PS-31` |

**Evidence and abuse control**

| Candidate | For | Against |
|---|---|---|
| Provider-signed quotes and refusals | Router cannot restate; supports repudiation claims | Providers must sign; key management for modules |
| Per-provider reliability statistics, possibly published | Makes fading visible; informs routing | Needs a clean denominator; gaming; provider confidentiality |
| Limits on quote-to-fill or cancel ratios | Well-known control | Thresholds are hard to set; may hit honest thin makers |
| Credential separation between quote data and treasury or routing roles | Precedent in Nexus | Only as strong as enforcement |
| Short batches for selection, not continuous | Reduces speed races | Adds latency; a design choice for later order books |
| Independent reference source for benchmarks | Outside view of margin | Licensing and availability; thin corridors may have none |

**Protocol alignment**

| Candidate | For | Against |
|---|---|---|
| Shape quotes after SEP-38 | Closest public shape for ramps; fee object | Draft status; tied to Stellar identity |
| Shape after Open Payments or Mojaloop | Fixed-send and fixed-receive; live use for Mojaloop | Assumes their parties and hub or grants model |
| Map to ISO 20022 and FIX at the edges | Familiar to institutions | Heavy; an edge mapping does not solve semantics |
| Own documented schema with a versioned mapping | Fits rutent | No outside reach until adopted |

**Libraries.** See section 6. Graph and optimisation libraries are options under any route-finding candidate. GPL-licensed ones depend on `OQ-16`.

## 8. Questions only the owner can answer

- What do "route" and "graph" mean in `PS-21`: a provider's advertised capability, a plan for one order, or both (`oq-route-meaning`)?
- Who posts routes: providers, an operator, third-party routers, or only the engine itself (`OQ-8`)?
- When posted, is a route a statement the poster stands behind?
- May a router also be, or be affiliated with, a provider?
- What is rutent optimising for by default: lowest cost, fastest, most reliable, or the customer's stated choice?
- Must rutent offer one all-in firm price for a whole route, or may it quote only the next leg?
- If a route has a long leg, who bears the move during it: the customer, a provider, or someone holding inventory?
- May a quote float, or be re-priced mid-route, and does any target jurisdiction forbid it?
- May a provider use last look? If so, must it be symmetric and disclosed?
- May rutent add its own margin, and must every margin be shown?
- Must ramps and exchanges expose limits, minimums, settlement time and hours, in addition to the rates and fees `PS-16` and `PS-17` name?
- May a route split across several providers?
- How many providers and corridors are expected in v1, and how fresh must rates be?
- Which reference price, if any, counts, including for corridors that may have none?
- Are provider-signed quotes a condition of joining the catalogue (`OQ-13`)?
- Who is entitled to see quotes and route decisions, and how soon: only the customer, all providers, regulators, the public (`OQ-11`)?
- May a provider see that another provider was asked?
- May quote identity gates apply, so that only known parties can obtain firm quotes (`PS-22`)?
- For `PS-23`, does "resolved by the exchange providers" mean exchanges keep their own books, cross orders among themselves, or rely on a shared book?
- What does "more open protocol" open to: more providers, third-party routers, other operators, or interoperating with other networks?
- Which licences may the quoting engine depend on, if it is to be reused elsewhere (`OQ-16`)?
- From which languages must the engine be usable (`PS-10`, `PS-20`)?
- How often should route quality be reviewed, by whom, and against what?

## 9. What I could not determine

- Any independent measurement of provider firmness, fade rates or rejection rates in fiat-to-crypto ramps. I found none.
- Published latency or rate limits of real quote APIs at scale.
- Any published measurement of Interledger probing cost or accuracy, or Interledger volume.
- Independent adoption figures for any open protocol in section 5. Mojaloop's are self-reported, and ERC-7683's adoption claims come from a vendor and conflict on status with the EIP page.
- Whether Nexus's live rulebook will match its documentation, or its current go-live date beyond a 2027 target.
- FSB KPI figures from the report itself: the PDF returned an error, and those are through a secondary summary.
- The latest World Bank global average beyond the March 2025 excerpt. The site returned 403.
- Current FIX enum values for `QuoteType`, beyond the default being indicative.
- A primary text of RTS 9, and whether EU and UK order record rules have since changed. The UK pages read mark the regulations for revocation.
- The full text of Budish et al., Aquilina et al., Egginton et al., Perold, and the BNY Mellon and SEC broker orders. I rely on excerpts.
- Whether the SEC finalised rescission of Rule 611. Excerpts report a June 2026 proposal, with comments closing in August. A FINRA notice from July 2026 appears to discuss a post-rescission world *(search excerpt)* ([RN 26-15](https://www.finra.org/sites/default/files/2026-07/Regulatory-Notice-26-15.pdf)).
- Why 0x's relayer model gave way, from a primary source.
- Which conduct and disclosure rules apply to rutent or its operators. This is legal advice, and the brief does not ask for it.
- Bre-B, SPEI or Pix settlement windows and limits as they would affect a route. I did not research rail rules here.
- How much of Lightning's and DEX evidence transfers to rails with no cheap test or atomic failure. I treat this as open and my inferences on it as untested.
- Documentation quality for the libraries in section 6. I read metadata and READMEs only in part.

## References

All accessed 2026-11-02. Version or date as shown by the source; "undated" means none was visible.

**FX conduct and disclosure**
- [FX Global Code, updated December 2024](https://www.globalfxc.org/uploads/fx_global.pdf). [GFXC Execution Principles Working Group Report on Last Look, August 2021](https://www.globalfxc.org/uploads/gfxc_report_last_look-1.pdf). [Outcomes of the Three-Year Review, December 2024](https://www.globalfxc.org/uploads/gfxc_3year_review_outcomes_dec2024.pdf).
- [NYDFS press release, 2015-11-18](https://www.dfs.ny.gov/reports_and_publications/press_releases/pr1511181).
- [12 CFR 1005.31, CFPB](https://www.consumerfinance.gov/rules-policy/regulations/1005/31/). [Regulation (EU) 2019/518](https://eur-lex.europa.eu/eli/reg/2019/518/oj/eng).
- [World Bank Remittance Prices Worldwide](https://remittanceprices.worldbank.org). [FSB KPI and progress report, 2025-10-09](https://www.fsb.org/2025/10/g20-roadmap-for-cross-border-payments-consolidated-progress-report-for-2025/). [FXC Intelligence summary](https://fxcintel.com/research/reports/ct-retail-payments-g20-cross-border-roadmap-2025).
- [FINRA Rule 5310, current from 2014-05-09](https://www.finra.org/rules-guidance/rulebooks/finra-rules/5310). [Regulatory Notice 26-15, July 2026](https://www.finra.org/sites/default/files/2026-07/Regulatory-Notice-26-15.pdf).
- [SEC 2020-321, 2020-12-17](https://www.sec.gov/newsroom/press-releases/2020-321). [US Attorney SDNY, March 2015](https://www.justice.gov/archive/usao/nys/pressreleases/March15/BNYMSettlement2.php). [Sidley on the SEC Rule 611 proposal, June 2026](https://www.sidley.com/en/insights/newsupdates/2026/06/sec-proposes-rescission-of-the-order-protection-rule).
- [UK-retained MAR Art. 12](https://www.legislation.gov.uk/eur/2014/596/article/12). [RTS 6, Chapter V](https://www.legislation.gov.uk/eur/2017/589/chapter/V/data.html). [RTS 25, Annex](https://www.legislation.gov.uk/eur/2017/574/annex).

**Quote APIs and schemes**
- [SEP-38 v2.5.0, updated 2025-02-26](https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0038.md). [Stellar operations](https://developers.stellar.org/docs/learn/fundamentals/transactions/list-of-operations). [XRPL paths](https://xrpl.org/docs/concepts/tokens/fungible-tokens/paths).
- Nexus, undated: [quotes](https://docs.nexusglobalpayments.org/fx-provision/quotes.md), [rates](https://docs.nexusglobalpayments.org/fx-provision/rates-from-third-party-fx-providers), [revenue model](https://docs.nexusglobalpayments.org/fx-provision/revenue-model-for-fxps.md), [liquidity](https://docs.nexusglobalpayments.org/fx-provision/managing-liquidity.md), [value limits](https://docs.nexusglobalpayments.org/payment-processing/maximum-value-of-a-nexus-payment.md). [MAS annual report 2025-26](https://www.mas.gov.sg/publications/annual-report/2026/annual-report-2025-2026/innovative-and-inclusive-financial-centre/payment-innovation).
- [Open Payments resources, reviewed 2026-11-02](https://openpayments.dev/concepts/resources/). [Rafiki backend configuration](https://rafiki.dev/integration/deployment/services/backend-service/).
- Mojaloop: [FX, updated 2025-03-14](https://docs.mojaloop.io/product/features/fx.html), [PUT fxQuotes](https://docs.mojaloop.io/product/features/Iso20022/script/fxquotes_PUT.html), [Foundation post](https://mojaloop.io/building-durable-inclusive-instant-payment-systems-across-africa/).
- [Card processor FX quotes guide, preview 2026-09-30](https://docs.stripe.com/payments/currencies/localize-prices/fx-quotes-api.md). [Money-transfer API quote reference](https://docs.wise.com/api-reference/quote). [Cross-chain quote API 1.0.0](https://docs.li.fi/api-reference/get-a-quote-for-a-token-transfer). [DEX aggregator quickstart](https://docs.0x.org/docs/introduction/quickstart/swap-tokens-with-0x-swap-api). [FIX dictionary](https://fiximate.fixtrading.org/legacy/en/FIX.4.4/body_505583.html?find=BidPx).

**Routing and protocols**
- [ILPv4](https://interledger.org/developers/rfcs/interledger-protocol/), [STREAM](https://interledger.org/developers/rfcs/stream-protocol/), [RFC 0018, draft 3](https://github.com/interledger/rfcs/blob/master/0018-connector-risk-mitigations/0018-connector-risk-mitigations.md), [RFC 0034](https://github.com/interledger/rfcs/blob/master/0034-connector-requirements/0034-connector-requirements.md), [deprecated RFCs](https://github.com/interledger/rfcs/tree/master/deprecated). [Schwartz, 2018-01-29](https://emschwartz.me/simplifying-interledger-the-graveyard-of-possible-protocol-features/).
- [BOLT 7](https://github.com/lightning/bolts/blob/master/07-routing-gossip.md). [LND pathfinding](https://docs.lightning.engineering/lightning-network-tools/lnd/pathfinding). [Pickhardt and Richter, 2021-07-12](https://arxiv.org/pdf/2107.05322). [Delving Bitcoin mirror, 2024-06-17](https://mirror.b10c.me/delvingbitcoin-org/2024-06-17-estimating-likelihood-for-lightning-payments-to-be-in-feasible-id973.html). [lightning-dev, May 2022](https://diyhpl.us/~bryan/irc/bitcoin/bitcoin-dev/linuxfoundation-pipermail/lightning-dev/2022-May/003587.txt).
- [Angeris, Chitra, Evans, Boyd, 2022-04-11](https://arxiv.org/abs/2204.05238). [Wang et al., v3 2022-01-14](https://arxiv.org/abs/2105.02784). [Sedgewick and Wayne, Algorithms 4.4](https://algs4.cs.princeton.edu/44sp/). [Boost r_c_shortest_paths](https://www.boost.org/latest/libs/graph/doc/r_c_shortest_paths.html). [RAPTOR, Delling, Pajor, Werneck](https://i11www.iti.kit.edu/extra/publications/dpw-rbptr-12.pdf). [Euler, Lindner, Borndörfer](https://arxiv.org/pdf/2204.01326).
- [UniswapX overview](http://developers.uniswap.org/llms.mdx/docs/liquidity/uniswapx/overview), [filler FAQ](https://developers.uniswap.org/docs/liquidity/uniswapx/filling/faq). [CoW solvers](https://docs.cow.fi/cow-protocol/concepts/introduction/solvers), [competition rules](https://docs.cow.fi/cow-protocol/reference/core/auctions/competition-rules). [Yuminaga, Chen, Sui, v3 2025-05-06](https://arxiv.org/html/2503.00738v3). [Uniswap Labs, 2024-05-01](https://blog.uniswap.org/measuring-price-improvement-with-order-flow-auctions). [0x, 2022-04-14](https://0x.org/post/measuring-the-impact-of-hidden-dex-costs). [Beelen, CEU thesis, 2025](https://www.etd.ceu.edu/2025/beelen_dennis.pdf).
- [EIP-7683, Draft, created 2024-04-11](https://eips.ethereum.org/EIPS/eip-7683). [Standard Relayer API](https://www.github.com/0xProject/standard-relayer-api). [Delphi report](https://members.delphidigital.io/reports/0x-improves-ux-meet-matcha).
- [CPMI brief, 2022-02-08](https://www.bis.org/cpmi/publ/swift_gpi.htm). [CPMI Correspondent banking, 2016-07-13](https://www.bis.org/cpmi/publ/d147.htm). [Glowka and Nilsson, BIS Quarterly Review, 2022-12-05](https://bis.org/publ/qtrpdf/r_qt2212i.htm). [Han, Lin, Yu, AFT 2019](https://eprint.iacr.org/2019/896).

**Manipulation and market structure**
- [Budish, Cramton, Shim, working version](https://conference.nber.org/conf_papers/f69665.pdf); QJE 130(4), 2015. [Aquilina, Budish, O'Neill, NBER w29011](https://www.nber.org/system/files/working_papers/w29011/w29011.pdf); QJE 2022. [Milionis et al., v6 2026-08-20](https://arxiv.org/abs/2208.06046). [Daian et al., 2019-04-10](https://arxiv.org/abs/1904.05234).
- [Cong, Li, Tang, Yang, NBER w30783](https://nber.org/papers/w30783); Management Science 69(11), 2023. [Egginton, Van Ness, Van Ness, 2016, citation in CV](https://business.olemiss.edu/wp-content/uploads/2022/03/vannessR-2023.pdf). [Dissertation listing](https://network.bepress.com/social-and-behavioral-sciences/economics/finance/page150). [FIA on MiFID II RTS](https://fia.org/articles/mifid-ii-rts-published-eu-official-journal). [Governance thread on surplus](https://gov.1inch.network/t/remove-the-positive-slippage-fee-for-1inch-stakers/381).
- [CME Globex matching algorithms](https://www.cmegroup.com/confluence/display/EPICSANDBOX/CME+Globex+Matching+Algorithms). [dYdX limit order book](https://docs.dydx.xyz/concepts/trading/limit-orderbook).

**Libraries:** repositories as linked in section 6.

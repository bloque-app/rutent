---
name: Verifier providers and privacy at route-building time
status: informative
date: 2026-11-03
---

# Verifier providers and privacy at route-building time

## Basis

**This is not legal advice.** Everything under "reliance" and "liability" is an engineer's reading of public texts. A lawyer in the relevant country could disagree.

**Read first, inside this project, on 2026-11-03:** `CLAUDE.md` and `research/2026-11-02-compliance-proofs.md` (called "the compliance report" below; it was shown to me up to line 376 of 534, and I did not read the rest). I do not repeat its findings; I cite its sections where I build on them. Nothing else in the project was read.

**Read outside the project:** public sources only.

**Orchestrator's review, 2026-11-03.** Checked at the source: RFC 9901 section 10.1, including "cannot be achieved" for salted-hash approaches, the power-dynamic warning, and the callback and timing side channels; OpenID Connect Core section 8 on pairwise values and reversibility; the FATF conditions, ultimate responsibility, and the line between reliance and outsourcing. Three limits apply. The FATF source is a 2013 slide deck, not the current Recommendation. Everything tagged secondary was not checked. The legal fit stays an engineer's reading, and no legal claim here should reach `SPEC.md` as fact without counsel. Two corrections were made: finding 2 now says which party is which, and section 1 no longer claims `PS-30` and `PS-31` are meant to prevent a concentrated view, since `PS-30` is a non-goal and `PS-31` concerns balances.

**A word that means two things.** The owner's "verifier" gives judgements, which makes it an issuer in credential standards. In those standards, including RFC 9901, a "verifier" is the party that checks a credential, which here would be a provider or the platform. Sections 1 and 3 to 5 use the owner's sense. Where section 2 quotes a standard, it uses the standard's sense.

**Tags on load-bearing claims:**
- **[source]** I read the primary text myself (downloaded and read as text).
- **[secondary]** A summary, law-firm note, vendor page, or a fetch tool's digest of the primary text.
- **[inferred]** My reasoning. No source says it.

**What I could not do.**
- The FATF site refused my request (403) and I did not try to get past it. For Recommendation 17 I read a FATF-authored 2013 briefing on the revised standards (hosted by the OSCE), and I rely on the compliance report's direct reading of the Spanish edition for the Interpretive Note. I did not read the current English text of the Recommendation.
- The EUR-Lex page for Regulation (EU) 2024/1183 returned nothing usable, so every EU wallet statement below is secondary.
- Several fetches returned server errors (the Spanish data protection authority's blog posts), so I use their positions only as reported by search excerpts.
- I found no primary or strong source on third-party reliance for Mexico. For Colombia and Brazil I reuse the compliance report's sourced readings and added nothing new.
- I did not read the FATF 2020 digital identity guidance, which probably bears on question 3. It is a lead, not a finding.
- No web page I read contained instructions directed at me.

## Findings that matter most

1. **The leak in a verifier design is set by who is asked, with what, and when, more than by the credential format.** Even with perfect cryptography, a verifier that is called at route time learns that a person is considering a transaction. In a design where eligibility is checked for every candidate provider, that includes providers that never get the order. [inferred, sections 1]
2. **A party that issues the credential can't be kept from learning where it was used, if that credential is SD-JWT-like and the checking party forwards it or is made to.** An owner's verifier that is also the identity provider is that issuer. RFC 9901 says issuer-to-verifier unlinkability against a careless, colluding, compromised or coerced verifier, in the RFC's sense, cannot be achieved with salted-hash credentials such as SD-JWT, because the signed credential itself goes to the checking party, who can forward it. BBS and zero-knowledge proofs claim otherwise, and neither was found in regulated production. [source for the RFC, secondary for the rest, section 2]
3. **A bare "yes" does not fit the shape of FATF reliance.** Recommendation 17 as I read it makes the relying institution obtain identity information immediately and be able to get documents without delay, and leaves ultimate responsibility with it. A judgement with no data path behind it looks like a contract representation, not reliance. [source for the conditions; inferred for the fit, section 3]
4. **I found no regulated production use of an unlinkable or zero-knowledge credential for KYC or eligibility.** Production exists for age checks and for issuer-signed attestations that identify the wallet. Travel Rule networks and reusable-KYC registries exist, and neither hides who is asking from the operator. [secondary, absence in limited search, section 2]
5. **Existing schemes put the loss on whoever the contract and the regulator say, never on the judgement's author by default.** In the cases I found the checking institution keeps the regulatory duty, the vouching party's exposure is contractual or fault-based, and staleness is handled by validity periods and status lists whose own privacy text is non-normative. [source and secondary, section 4]

## 1. Who learns what, by design

### The setting

Inference. At route-building time the platform has a set of candidate routes. Each route has several stages, each stage has several possible providers, and each provider has its own eligibility rules. An eligibility check is needed before a provider can be priced or kept in the running (the owner's premise). So a check can be sent to many providers that will never see an order. I call those the unchosen providers. Eligibility judgements go to the platform, which needs them to decide.

Six leaks are tracked: the person's identity (I), that an order exists (E), its amount (A), its corridor, meaning source and destination country, asset and rail (C), correlation of one person across providers (X), and the verifier seeing every transaction, "phone home" (P).

### Candidate designs

- **D1. Each provider checks its own records.** The platform sends a pseudonymous subject reference to each candidate, which looks the person up (the compliance report's `PS-22` shape). No verifier exists.
- **D2. Presentation to each provider.** The person, or an agent for them, presents a credential or proof to each candidate. No verifier is called.
- **D3. Separate verifier, called per check.** An independent verifier holds or can derive judgements. The provider or the platform asks it "may subject S do T?".
- **D4. Verifier that is also the identity provider.** As D3, but the verifier issued the person's identity and holds the underlying documents. This is the owner's second idea.
- **D5. Verifier issues time-limited attestations to the person, who carries them.** No call at check time. The verifier is contacted at issuance only.
- **D6. Anonymous or zero-knowledge presentation.** As D2, with a proof about a hidden credential and no stable identifier.

### Leak table

All cells are Inference, built from the sources in section 2. "Yes" means the party learns it by default. "Depends" names what it depends on.

| Design | Who learns identity (I) | Order exists (E) | Amount (A) | Corridor (C) | Cross-provider correlation (X) | Phone home (P) |
|---|---|---|---|---|---|---|
| D1 | Each candidate, if the reference is stable or the person is already its customer. The platform sees only the reference | Every candidate asked, chosen or not | Only if sent with the question | Each candidate sees its own stage; platform route shape is exposed to those asked | The platform, if one reference is used everywhere. None if references differ per provider | None; but each provider sees every question |
| D2 | Each candidate sees what is presented | Every candidate presented to | Only if bound into the presentation | As D1 | Candidates, if the credential or key is the same; none if each use is fresh and the issuer is not asked | None, unless the provider checks status |
| D3 | The verifier, always. Candidates see the verdict and a reference | The verifier, for every candidate, and every unchosen candidate if it also asks | Verifier, if the question carries it; a "class" question can avoid it | Verifier, if carried | The verifier sees all of it, being the common point | Yes, by construction |
| D4 | As D3, plus the verifier holds the documents | As D3 | As D3 | As D3 | As D3, plus joins to everything else it knows | Yes, and strongest: the same party knows the real person and every query |
| D5 | The verifier, at issuance only | The verifier does not see individual orders, but may see issuance cadence | Not unless the attestation is order-specific | Only at issuance | Between providers: yes if the attestation is the same bytes; no if issued fresh per use | Low. A status check re-introduces it |
| D6 | The proof reveals only the statement, if sound | Every candidate presented to | Only if bound | As D1 | None in the scheme's claim, but metadata (network address, timing, message counts) can still link | None |

### Reading across

Inference.
- **Unchosen providers always learn existence unless the design avoids asking them.** The only structural cure is to ask fewer providers or ask a class question (see below). Every design above leaks E to all candidates asked.
- **A separate verifier concentrates everything.** In D3 and D4 the verifier is the one party that sees all candidates, all orders and all persons. Its position is the same as the platform's would be if the platform did the checks. D4 adds one more thing, the documents.
- **The class-question idea.** A check can be asked at three grains: by person alone ("may this person use rails of this kind in this country?"), by person and class of transaction, or by person and order (amount, corridor). Coarser grains leak less per query and are reusable across candidates, but they say less and go stale. Each grain trades a leak for a weaker judgement. The compliance report's inference about repeated yes-or-no checks (section 5) applies: if the question can be asked with a chosen threshold, the answer leaks the threshold.
- **Binding to the order and unlinkability pull apart.** A judgement bound to one order, nonce and audience cannot be replayed (compliance report section 2), but it carries the order's existence. A judgement bound to nothing is cheap and private and is a bearer token.
- **Correlation is a choice, not a default.** Per-provider references stop providers and the platform from joining a person across the route, and also stop anyone detecting one person fronting under many references (compliance report section 5, the "rutent as correlator" row). Which of the two is wanted is a question for the owner.

## 2. State of the art for proving a claim without identity

### Summary table

| Technology | Maturity | Stops verifier-to-verifier linking | Stops issuer-to-verifier linking | Regulated finance in production |
|---|---|---|---|---|
| Selective-disclosure credentials, SD-JWT | RFC 9901, Standards Track, read at source. SD-JWT VC is a separate draft (compliance report) | Yes with batch issuance or one-time use; otherwise no | Only against an honest verifier; no against a colluding or coerced one [source] | Not found in my searches |
| ISO mdoc / mobile driving licence | ISO standard; supported by OpenID4VP | Same limits as SD-JWT: unlinkability depends on batch issuance [secondary, EU ARF discussion] | Same | Government identity use; I found no financial use in my searches |
| BBS signatures | W3C draft suite and an IETF research-group draft, not an RFC (compliance report) | Yes by design | Yes by design | Not found |
| OpenID4VP | Final, 2025-07-09 (compliance report) | Not itself; carries presentations | Not itself | Used as transport in the EU wallet plans [secondary] |
| Pairwise subject identifiers (OpenID Connect) | Final specification, widely deployed in single sign-on | Yes between clients, if the provider computes one per sector [source] | No: the provider that computes them can reverse them [source] | Single sign-on generally; no finance-specific finding |
| Zero-knowledge proof over an existing credential | Open-source library released 2025-07-03; IETF research-group draft; external reviews reported as underway [secondary] | Yes in the scheme's claim | Yes in the scheme's claim, with no issuer change | Age checks in a consumer wallet [secondary]. No KYC found |
| Public-chain attestations | Deployed by large exchanges | No: attestation is linked to a public wallet | n/a | Yes, by exchanges, as eligibility gates [secondary]. Reveals wallet-to-verified-account |

### What the sources say

**SD-JWT, RFC 9901, section 10.1 [source].** The RFC separates four properties. Presentation unlinkability: a verifier should not link two presentations of the same credential. Verifier-to-verifier unlinkability: two verifiers should not learn that the same credential was shown to both. Issuer-to-verifier unlinkability with an honest verifier, which "can generally be achieved". Issuer-to-verifier unlinkability against a careless, colluding, compromised or coerced verifier, which "cannot be achieved" in salted-hash approaches, because the signed credential goes straight to the verifier. The text adds that unlinkability fails when disclosed claims identify the person, such as a tax identification number. It warns that a government issuer, or a large provider with a bigger ecosystem, can compel an otherwise honest verifier to report back, and that deployers must mitigate this or tell users. A callback from verifier to issuer, such as a revocation check, can leak usage, and request timing can be a side channel; the status-list draft is given as an example of minimal disclosure.

**Pairwise identifiers, OpenID Connect Core 1.0, section 8 [source].** A provider gives a different subject value to each client "so as not to enable Clients to correlate the End-User's activities without permission". The value must not be reversible by any party other than the provider. Inference: pairwise identifiers stop the relying parties from joining a person, and by design give the provider total knowledge. They match D4 and D1 exactly: the party that issues the pairwise references knows what they mean.

**BBS and status lists [secondary, from the compliance report].** The compliance report (section 1) read these directly as digests: BBS gives unlinkable derived proofs; revealed indexes, message counts and proof options still narrow the anonymity set. Status lists protect holders only by hiding in a large list, with a stated minimum of 131,072 entries for the W3C bitstring version, and the report infers that a provider with a few thousand users is the small population those documents warn about.

**Zero-knowledge credentials.**
- A large technology company opened its zero-knowledge libraries on 2025-07-03, and the IETF research-group draft says the scheme works over existing mdoc credentials without new issuance [secondary, search excerpts]. The same search surfaced a claim that a German savings-bank partnership was named as the first national credential partner; I could not confirm it. [secondary, weak]
- I found demos and academic papers for KYC-specific proofs (compliance report, section 1) and no regulator accepting a proof as customer due diligence. This is an absence in limited searching.

**Public-chain attestations [secondary].** A large exchange attests on a public chain that a wallet belongs to a verified account, and publishes coarse types such as account, country of residence and membership. A second exchange issues a non-transferable token to verified users, reported at about 1.15 million tokens in early 2026. Inference: these give eligibility gating with no per-query phone home, and they publish the link between a wallet and a verified customer to anyone, which is the opposite of unlinkability. The attester alone can revoke or update.

**A credential framework for on-chain finance.** A payments company released an open-source framework for identity credentials on chains and said it intended to issue and verify KYC credentials [secondary]. I found no evidence of a live regulated deployment, and I could not tie it to a lending protocol that was reported as using whitelisters instead. Treat the outcome as unknown.

### Real attempts and how they turned out

**Travel Rule networks [secondary, vendor and industry pieces].**
- Several protocols compete (TRP, TRISA, Sygna Bridge and others). A 2024 industry survey reported interoperability as the top hurdle, at 34 percent of respondents, and 67 percent using only one protocol. The number of providers halting transfers when they could not send the data nearly tripled in that year.
- The "sunrise problem" means the counterparty cannot yet receive data. Regulators do not mandate how data is exchanged.
- Privacy handling is each protocol's own claim (one describes itself as open-source and decentralised, without outside assessment). I found nothing independent on what the directories leak about who is looking up whom.
- Inference: these networks move identity data between obliged providers on purpose. They are a solution to "the counterparty must receive data", and they do not hide the sender or receiver from the providers involved. The FATF position, in the compliance report (section 3), is that required data must reach the other provider before or with the transfer.

**Reusable KYC.**
- The SWIFT KYC Registry (launched December 2014) shared bank KYC files among correspondent banks. A 2016 trade-press article put about 4,000 of 7,000 target members as joined against a 60 to 70 percent aim, and the operator's later material claims 7,000 or more institutions [secondary, operator material]. The operator says contributing banks keep ownership of and responsibility for their data, and that judgements such as risk scoring remain with the using institution [secondary]. I found no independent measurement of how often users still repeat the diligence.
- Inference: the Registry shares files among regulated peers who each still decide. It is the opposite of "someone else's judgement instead of data".
- I found nothing primary on India's central KYC registry or on national KYC utilities. I could not find Colombian, Brazilian or Mexican acceptance of a private reusable credential (compliance report, section 1).

**EU digital identity wallet and financial onboarding [secondary].**
- Member states must offer a wallet by the end of 2026, and anti-money-laundering obligations under the new EU regulation apply from 2027-07-10 (compliance report). Commentary says a wallet covers identity verification, not the rest of due diligence.
- Reported provisions in Regulation 2024/1183: wallets must not let attestation issuers learn how attestations are used and must support unlinkability (Article 5a(16)); wallets must offer pseudonyms where the law does not require identification, and relying parties register in their country (Article 5b) [secondary: a German government guide, a conference deck and two civil-society documents; I could not read the Regulation text itself].
- Critics reported by search excerpts say the architecture document of the time offered no unlinkability from the identity provider or relying party and none across presentations to the same relying party, and suggested blind signatures or zero-knowledge proofs. A research project reports zero-knowledge anonymous credentials for multi-show unlinkability and states plainly that it is not production KYC [secondary, search excerpt]. A civil-society letter of 2026 argues draft implementing acts narrow the pseudonym right [secondary]; I did not check the acts.
- I found no regulated institution onboarding through the wallet in production, and no official 2026 EU text on the financial onboarding flow. The EU is outside the project's named countries and matters only as the most advanced regulated example.

**Wallet-based and attestation-based eligibility in crypto.** See the public-chain row above. Inference: this model satisfies "provider can check eligibility cheaply" and gives up privacy to the chain's readers.

## 3. Can a regulated provider rely on someone else's judgement?

An engineer's reading, not legal advice.

### FATF Recommendation 17

**Conditions [source, FATF 2013 briefing on the revised standards; matches the compliance report's direct reading of the Spanish edition].** Financial institutions may rely on third parties for part of customer due diligence, if:
- information on the customer's identity, beneficial owner and nature of business is immediately available to the relying institution;
- the relevant documents are available on request without delay;
- the third party is regulated, supervised or monitored and complies with due diligence requirements; and
- the risk level of the country where the third party sits is considered.

The briefing states that the ultimate responsibility for due diligence stays with the relying institution. It also says the revised text draws a clear line between reliance and outsourcing or agency, and allows more flexible reliance inside one financial group.

**The Interpretive Note [source, compliance report, Spanish edition].** Reliance does not cover outsourcing or agency. In true reliance the third party has its own relationship with the customer and applies its own procedures. "Third parties" means supervised financial institutions or designated non-financial businesses. A hedge: I did not read the current English text, and the compliance report could not confirm the Spanish edition is unchanged.

### What stays with the relying party

Inference from the text above.
- **The responsibility.** Always, per the briefing. A verifier's yes does not discharge it.
- **A data path.** The relying party needs identity information to be "immediately available" and documents available "without delay". This does not say the relying party must hold the data. A path that delivers it on demand might meet the text. I found no source that says so either way. This is the single most important unsettled reading for the owner's idea.
- **A judgement about the third party.** The relying institution must decide that the third party is supervised and adequate, including its country. A verifier that is unsupervised does not qualify as a "third party" at all. Then the arrangement is contractual, or it is outsourcing or agency, where the provider's own procedures apply and the provider keeps access to everything.
- **Sanctions, monitoring and suspicious activity.** These are separate duties. The compliance report shows Colombia and Brazil saying the suspicious-activity function cannot be outsourced (its section 2 table).

### Three ways a verifier could sit in law

Inference.
1. **Verifier is a supervised institution and a Recommendation 17 third party.** The provider relies on it. The data path above is required, and the verifier needs a relationship with the customer and applies its own procedures.
2. **Verifier is an agent or outsourced function of the provider.** Then it follows the provider's procedures, the provider is responsible, and the provider needs full access to what the agent holds.
3. **Verifier is an unsupervised service giving a signal.** Then the provider cannot treat it as customer due diligence. It is an input to the provider's own risk decision, at most (this matches the compliance report's reading for the platform).

The owner's idea, a verifier that "exposes judgements", looks like case 1 or 3 depending on whether it is supervised and whether a data path exists. D4, where the identity provider also judges, fits case 1 most naturally, because it holds the relationship and the documents.

### Colombia, Brazil, Mexico

- **Colombia [secondary, from the compliance report].** A supervised entity may contract out customer-knowledge work under its own procedures. The decision to onboard cannot be delegated, and neither can identifying and reporting unusual and suspicious operations (Superfinanciera concept 2025076970-002). This is outsourcing, not reliance on another institution's conclusion. I added nothing new.
- **Brazil [secondary, from the compliance report].** The authorised provider answers for identification, qualification, fraud and AML controls and stays responsible when delegating accessory tasks (Joint Resolution 16). Brazilian norms on virtual assets make the contracting provider responsible for contracted entities. Circular 3,978 says suspicious-activity analysis cannot be outsourced. I found no text letting a Brazilian provider rely on another party's customer conclusion without holding data.
- **Mexico.** I found no primary or strong source on reliance for Mexico. A search returned vendor guides and a 2020 law-firm note on simplified measures, and one vendor source says banks must check fingerprints against the national voter register, which I could not verify. I cannot say whether a Mexican obliged party may rely on another's judgement.

## 4. What a wrong verifier costs, and who carries it

### The cost

Inference, building on the compliance report (section 2, "Incentives and error").
- **A false yes (no one is harmed in the check, harm comes later).** The provider onboards or serves someone it should not have. Its own regulator can treat that as the provider's failure. The compliance report's worked case: a UK bank was fined £28,959,426 in 2024 for screening against only part of a sanctions list over years and for opening accounts in breach of a restriction [source, FCA final notice, read by the compliance report]. An honest "screened" from that bank would have been false.
- **A false no.** A lawful user is blocked, possibly after a reserve or exchange has already been committed in a route. The loss is time, locked funds and abandoned orders, borne by the user and by whoever planned the route.
- **A stale yes.** A once-true judgement outlives the facts: a sanctions listing, a revoked licence, a closed account.
- **A breach at the verifier.** In D3 and D4 the verifier holds the cross-provider record. A breach exposes everyone's route history. RFC 9901 section 10.1 names a breach at several verifiers as a linking route [source].

### Who carries it in existing schemes

- **Regulatory consequences stay with the institution that relied or delegated** (FATF briefing; EU AMLR Article 48, UK reg. 39, Colombia, Brazil, all in the compliance report). The verifier's error is the relying party's compliance failure.
- **Contractual and fault-based liability for the verifier.**
  - Under the eIDAS trust services rules, as reported by search excerpts of Article 13 of Regulation 910/2014: a trust service provider is liable for damage caused intentionally or negligently by failing its obligations; for qualified providers fault is presumed and the provider must prove otherwise; for others the claimant must prove it; qualified providers must hold insurance. The excerpt names an operator's mistake in identifying a user as an example [secondary]. I did not find how this applies to wallet attestations.
  - The SWIFT KYC Registry operator says it checks completeness and accuracy, while data owners remain responsible for their own information, and its marketing says banks can rely on the accuracy; I could not find the user terms with any indemnity or cap [secondary].
  - Norwegian bank identity: supreme court rulings put loss from misuse of a BankID on the holder when the holder knowingly handed over credentials, and in another case did not put the full loss on a victim of phone fraud [secondary]. The operative terms for issuer liability were not readable. Swedish relying parties contract through a bank and the page states no liability terms [secondary].
- **Card-network style liability shifts.** I know that card schemes shift fraud liability by contract when a stronger authentication step is used, but I did not research it and cite no source.
- **No scheme I found makes the judgement's author liable by default for a regulated relying party's compliance loss.**

### Revocation and staleness

- **Validity periods and status lists.** The W3C VC data model and the bitstring status list let an issuer revoke or suspend, but set no minimum or maximum validity period, and their privacy text is non-normative (compliance report). The token status-list draft (reported draft-21, June 2026) does the same for JOSE and COSE tokens (compliance report).
- **Usage leaks through the check.** RFC 9901 says a verifier-to-issuer callback such as a revocation check can leak usage and that timing can be a side channel [source]. Caching or proxying cuts the leak and adds staleness. This is the same trade as D3 versus D5.
- **Short-lived attestations** trade revocation for expiry. No source I read states the right lifetime; the compliance report notes there is none.
- **Sanctions and licence status** change on their own clock. The compliance report's detection table (section 2) says a one-time assumption of licence status is a failure mode.

## 5. What a spec could require, kept apart from how

### 5a. Capabilities (what)

None names a mechanism. Each is a prompt for the spec authors, not a requirement. Items from the compliance report's section 6a are not repeated; these add to them.

**Judgements and who may ask**
- A judgement states its subject reference, scope (person, class of transaction, or order), jurisdiction, maker, time made and time of validity, and says which of these it is.
- A judgement's scope is as coarse as the decision needs. A coarser question is preferred to a finer one when both would settle the decision.
- A party that asks for a judgement is identified to the maker and bounded in what it may ask (who, about whom, how often).
- A judgement is bound to the request it supports where replay matters, and is unbound only where the order's existence must not be exposed. The choice is stated.
- The maker of a judgement cannot be unseen by an auditor: who asked, when, and what came back is recorded somewhere an authorised third party can read, without making that record a general registry.

**Candidate-set privacy**
- The set of providers asked during route-building is bounded and stated. A provider not chosen learns no more than the question it was asked.
- Eligibility is checked at the coarsest grain first and refined only for providers still in the running.
- A provider can tell what a question carries (subject, class, amount, corridor) before it answers.

**Verifier visibility**
- What a verifier learns per query is a declared list, and nothing beyond it is retained.
- The verifier's knowledge of orders is stated and, where possible, absent.
- A verifier that is also an identity provider is declared as such, and its visibility is declared separately for each role.
- A judgement in force can be checked for withdrawal without telling its maker which party checked, or, where that is not possible, the leak is declared.

**Staleness and withdrawal**
- A judgement has a stated validity bound and a defined behaviour for orders in flight when it ends or is withdrawn.
- A withdrawal reaches parties who relied on it within a stated time.

**Reliance and data path**
- For each provider and jurisdiction, the spec states whether the provider must be able to obtain identity information behind a judgement, and how fast. If it must, a path exists that does not run through the platform.
- A relying provider can tell whether a verifier is supervised, and in which jurisdiction, before relying on it.
- A judgement is recorded as second-hand: the provider relied on, with the maker named, and kept apart from anything a provider verified itself.

**Liability and evidence**
- Each judgement is attributable to its maker in a way the maker cannot deny.
- Terms that allocate loss for a wrong judgement are visible to the parties before they rely on it.
- A false judgement can be shown to be false after the fact, to an authorised third party, without disclosing more than needed.

**Correlation**
- Whether one person can be linked across providers is a declared property of each deployment, with the reason recorded.

### 5b. Candidate mechanisms (how), with no choice made

These are the mechanisms named in the sources above, listed so the owner sees the field.
- Provider-held records queried by pseudonymous reference (D1).
- Per-provider presentation of a signed credential, with selective disclosure (D2).
- Verifier called per check, separate from or equal to the identity provider (D3, D4).
- Time-limited attestations carried by the person (D5).
- Unlinkable signatures or zero-knowledge proofs over a credential (D6).
- Pairwise references computed per relying party.
- Status lists, with caching or proxying.
- Public-chain attestations.
- Reliance by contract with a supervised institution, or outsourcing/agency with data access.

**Non-binding note.** I recommend nothing. If the owner wants a starting reading, it is that the mechanisms differ more in who they trust with the full picture than in the cryptography, and that the verifier-as-identity-provider variant concentrates the most knowledge in one party.

### 5c. Questions only the owner can answer (six)

Each would change a requirement.
1. **Does an end user ever hold or present anything themselves, or is the user always represented by their provider?** If a person holds a credential, selective disclosure and unlinkability have someone to run them. If not, most of section 2 does not apply (compliance report, "Reading across").
2. **Must every relying provider be able to obtain identity data behind a judgement, or may a bare judgement suffice in some corridors?** This decides whether the judgement-only design is permitted, per section 3.
3. **May a verifier see the order (existence, amount, corridor), or only a subject and a class?** This decides whether per-order binding is allowed or forbidden, and how large the phone-home leak can be.
4. **Should an unchosen candidate learn that a check happened?** If not, the spec must require asking fewer providers or coarser questions.
5. **Is linking one person across providers wanted (to catch fronting) or forbidden (for privacy), or a per-deployment choice?** The compliance report shows these pull apart.
6. **Who must carry the loss for a wrong judgement, and may the platform or the project be a party to that arrangement at all?** This decides what liability and evidence requirements the spec can place on a verifier, and whether a verifier that is also an identity provider is acceptable.

## Sources

Read at source (downloaded and read as text):
- [RFC 9901, SD-JWT, section 10.1](https://www.rfc-editor.org/rfc/rfc9901.html)
- [OpenID Connect Core 1.0, section 8](https://openid.net/specs/openid-connect-core-1_0.html)
- [FATF briefing on the revised standards, section on Recommendation 17, 2013](https://cdn.osce.org/sites/default/files/f/documents/1/e/101961.pdf)

Secondary, digest or search excerpt only:
- [Travel Rule protocol landscape (2026 analysis)](https://comsuregroup.com/news/progress-on-the-jersey-vasps-travel-rule-but-interoperability-and-the-sunrise-problem-still-bite/) and [2024 survey post](https://notabene.id/post/2024-travel-rule-compliance-key-implementation-challenges-for-vasps)
- [SWIFT KYC Registry material](https://www.swift.com/news-events/news/banks-better-together-kyc), [2015 adoption report](https://www.theglobaltreasurer.com/2015/09/15/swift-kyc-registry-moves-beyond-1000-adoptions/)
- [Coinbase Verifications documentation](https://docs.cdp.coinbase.com/verifications/introduction/attestations)
- [Circle Verite](https://developers.circle.com/verite/issuer-overview)
- [Google open-sourced ZK for age assurance](https://blog.google/technology/safety-security/opening-up-zero-knowledge-proof-technology-to-promote-privacy-in-age-assurance/), [IETF draft slides](https://datatracker.ietf.org/meeting/124/materials/slides-124-cfrg-longfellow-zk-01)
- [epicenter.works on the EU architecture document](https://epicenter.works/fileadmin/medienspiegel/user_upload/epicenter.works_-_ARF1.4.pdf), [Spanish data protection authority blog](https://www.aepd.es/en/press-and-communication/blog/eidas2-the-eudi-wallet-and-the-gdpr-iii), [German government guide](https://bmi.usercontent.opencode.de/eudi-wallet/eidas2/en/eidas2/), [civil-society letter](https://www.akeuropa.eu/sites/default/files/2026-03/eIDAS%20IAs%20OpenLetter-rv2%20%28002%29.pdf)
- [eIDAS Article 13 text](https://www.legislation.gov.uk/eur/2014/910/article/13) (via search excerpt)
- [Norwegian Supreme Court, BankID misuse](https://www.domstol.no/en/supremecourt/rulings/rulings-2026/supreme-court---civil-cases/HR-2026-1359-A/)
- Everything labelled "compliance report": `research/2026-11-02-compliance-proofs.md`.
